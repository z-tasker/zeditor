use eframe::egui;
use std::time::Instant;
use std::path::Path;
use std::process::Command;

pub struct ChannelSurfer {
    // List of video files (channels)
    pub channels: Vec<String>,
    pub current_channel: usize,
    
    // Playback state
    pub playing: bool,
    pub muted: bool,
    
    // egui-video fields
    player: Option<egui_video::Player>,
    audio_device: Option<egui_video::AudioDevice>,
    
    // UI state
    pub show_channel_info: bool,
    pub channel_info_timer: Option<Instant>,
    
    // Mouse activity tracking for controls auto-hide
    last_mouse_activity: Instant,
    show_controls: bool,
    mouse_pos: Option<egui::Pos2>,
    
    // Loading
    pending_video: Option<String>,
}

impl ChannelSurfer {
    pub fn new(video_paths: Vec<String>) -> Self {
        // Initialize audio device
        let audio_device = egui_video::AudioDevice::new().ok();
        
        // Get first video to load
        let pending = video_paths.first().cloned();
        
        let now = Instant::now();
        
        Self {
            channels: video_paths,
            current_channel: 0,
            playing: true, // Auto-play
            muted: true,   // Start muted
            player: None,
            audio_device,
            show_channel_info: true,
            channel_info_timer: Some(now),
            last_mouse_activity: now,
            show_controls: true,
            mouse_pos: None,
            pending_video: pending,
        }
    }
    
    fn load_video(&mut self, ctx: &egui::Context, path: &str) -> anyhow::Result<()> {
        // Ensure ffmpeg is available
        ffmpeg_sidecar::download::auto_download()?;
        
        // Drop the existing player to free resources before loading new video
        if self.player.is_some() {
            self.player = None;
            // Small delay to let resources clean up
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        
        // Create egui-video player
        let path_string = path.to_string();
        let mut player = egui_video::Player::new(ctx, &path_string)?;
        
        // Add audio if device is available
        if let Some(ref mut audio_device) = self.audio_device {
            player = player.with_audio(audio_device)?;
            // Set volume based on mute state
            let volume = if self.muted { 0.0 } else { 1.0 };
            player.options.audio_volume.set(volume as f32);
        }
        
        // Auto-start playback
        player.start();
        
        self.player = Some(player);
        self.playing = true;
        
        // Show channel info briefly
        self.show_channel_info = true;
        self.channel_info_timer = Some(Instant::now());
        
        Ok(())
    }
    
    fn next_channel(&mut self) {
        if self.channels.is_empty() {
            return;
        }
        
        self.current_channel = (self.current_channel + 1) % self.channels.len();
        let path = self.channels[self.current_channel].clone();
        self.pending_video = Some(path);
    }
    
    fn prev_channel(&mut self) {
        if self.channels.is_empty() {
            return;
        }
        
        self.current_channel = if self.current_channel == 0 {
            self.channels.len() - 1
        } else {
            self.current_channel - 1
        };
        
        let path = self.channels[self.current_channel].clone();
        self.pending_video = Some(path);
    }
    
    fn toggle_play(&mut self) {
        self.playing = !self.playing;
        if let Some(ref mut player) = self.player {
            if self.playing {
                player.start();
            } else {
                player.pause();
            }
        }
    }
    
    fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        if let Some(ref mut player) = self.player {
            let volume = if self.muted { 0.0 } else { 1.0 };
            player.options.audio_volume.set(volume as f32);
        }
    }
    
    fn get_channel_name(&self) -> String {
        if self.channels.is_empty() {
            return "No channels".to_string();
        }
        
        let path = &self.channels[self.current_channel];
        Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string()
    }
    
    fn draw_channel_overlay(&mut self, ctx: &egui::Context) {
        // Hide channel info after 2 seconds
        if let Some(timer) = self.channel_info_timer {
            if timer.elapsed().as_secs() > 2 {
                self.show_channel_info = false;
                self.channel_info_timer = None;
            }
        }
        
        if !self.show_channel_info {
            return;
        }
        
        let channel_num = self.current_channel + 1;
        let total = self.channels.len();
        let name = self.get_channel_name();
        
        egui::Window::new("channel_info")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::RIGHT_TOP, [-20.0, 20.0])
            .frame(egui::Frame::window(&egui::Style::default())
                .fill(egui::Color32::from_rgba_premultiplied(0, 0, 0, 200))
                .stroke(egui::Stroke::NONE))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(format!("CH {}", channel_num))
                        .size(48.0)
                        .color(egui::Color32::WHITE)
                        .strong()
                );
                ui.label(
                    egui::RichText::new(format!("{} / {}", channel_num, total))
                        .size(16.0)
                        .color(egui::Color32::LIGHT_GRAY)
                );
                ui.label(
                    egui::RichText::new(&name)
                        .size(14.0)
                        .color(egui::Color32::WHITE)
                );
            });
    }
}

impl eframe::App for ChannelSurfer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Load pending video
        if let Some(path) = self.pending_video.take() {
            if let Err(e) = self.load_video(ctx, &path) {
                println!("⚠️  Failed to load initial video ({}): {}", path, e);
                // Note: Videos are pre-validated with ffprobe, so failures here are rare
            }
        }
        
        // Update player state
        if let Some(ref mut player) = self.player {
            player.process_state();
        }
        
        // Request continuous repaints for smooth playback
        ctx.request_repaint();
        
        // Track mouse movement for cursor auto-hide only
        ctx.input(|i| {
            let current_pos = i.pointer.latest_pos();
            let now = Instant::now();
            
            // Check if mouse moved
            if let (Some(new_pos), Some(old_pos)) = (current_pos, self.mouse_pos) {
                if new_pos != old_pos {
                    self.last_mouse_activity = now;
                }
            } else if current_pos.is_some() && self.mouse_pos.is_none() {
                // Mouse just entered window
                self.last_mouse_activity = now;
            }
            
            self.mouse_pos = current_pos;
        });
        
        // Auto-hide controls once after 3 seconds (never bring them back)
        if self.show_controls && self.last_mouse_activity.elapsed().as_secs() > 3 {
            self.show_controls = false;
        }
        
        // Auto-hide OS cursor after 3 seconds of mouse inactivity
        let cursor_should_show = self.last_mouse_activity.elapsed().as_secs() < 3;
        if cursor_should_show {
            ctx.set_cursor_icon(egui::CursorIcon::Default);
        } else {
            ctx.set_cursor_icon(egui::CursorIcon::None);
        }
        
        // Handle input
        ctx.input(|i| {
            // Left click or right arrow -> next channel
            if i.pointer.button_pressed(egui::PointerButton::Primary) ||
               i.key_pressed(egui::Key::ArrowRight) {
                self.next_channel();
            }
            
            // Right click or left arrow -> previous channel
            if i.pointer.button_pressed(egui::PointerButton::Secondary) ||
               i.key_pressed(egui::Key::ArrowLeft) {
                self.prev_channel();
            }
            
            // Space to pause/play
            if i.key_pressed(egui::Key::Space) {
                self.toggle_play();
            }
            
            // M to toggle mute
            if i.key_pressed(egui::Key::M) {
                self.toggle_mute();
            }
            
            // Q or Esc to show channel info again
            if i.key_pressed(egui::Key::Q) || i.key_pressed(egui::Key::Escape) {
                self.show_channel_info = true;
                self.channel_info_timer = Some(Instant::now());
            }
            
            // Handle file drops - add to channels
            if !i.raw.dropped_files.is_empty() {
                for file in &i.raw.dropped_files {
                    if let Some(path) = file.path.as_ref() {
                        let path_str = path.to_string_lossy().to_string();
                        if !self.channels.contains(&path_str) {
                            self.channels.push(path_str);
                        }
                    }
                }
            }
        });
        
        // Central panel - video display (no margins or padding)
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style())
                .inner_margin(0.0)
                .outer_margin(0.0))
            .show(ctx, |ui| {
            // Black background covering entire available area (no margins)
            let rect = ui.max_rect();
            ui.painter().rect_filled(
                rect,
                0.0,
                egui::Color32::BLACK,
            );
            
            if let Some(ref mut player) = self.player {
                // Get video size and calculate aspect-ratio-preserving size
                let video_size = player.size;
                let aspect = video_size.x / video_size.y;
                
                let available = ui.available_size();
                let target_size = if available.x / available.y > aspect {
                    // Height is limiting factor
                    egui::vec2(available.y * aspect, available.y)
                } else {
                    // Width is limiting factor
                    egui::vec2(available.x, available.x / aspect)
                };
                
                // Center the video
                ui.centered_and_justified(|ui| {
                    player.render_frame(ui, target_size);
                });
            } else {
                // No video loaded - show instructions
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("📺 Channel Surfer")
                                .size(32.0)
                                .color(egui::Color32::WHITE)
                        );
                        ui.add_space(20.0);
                        if self.channels.is_empty() {
                            ui.label(
                                egui::RichText::new("Drop video files here or run with video paths as arguments")
                                    .size(16.0)
                                    .color(egui::Color32::GRAY)
                            );
                        } else {
                            ui.label(
                                egui::RichText::new(format!("{} channels loaded", self.channels.len()))
                                    .size(16.0)
                                    .color(egui::Color32::GRAY)
                            );
                            ui.add_space(10.0);
                            ui.label(
                                egui::RichText::new("Loading...")
                                    .size(14.0)
                                    .color(egui::Color32::LIGHT_GRAY)
                            );
                        }
                    });
                });
            }
        });
        
        // Draw channel overlay
        self.draw_channel_overlay(ctx);
        
        // Bottom panel - controls hint (auto-hides after mouse inactivity)
        if self.show_controls {
            egui::TopBottomPanel::bottom("controls_hint").show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        egui::RichText::new("Left Click / → = Next Channel    Right Click / ← = Previous Channel    Space = Play/Pause    M = Mute    Q = Show Info")
                            .size(12.0)
                            .color(egui::Color32::GRAY)
                    );
                });
            });
        }
    }
}

fn main() -> eframe::Result<()> {
    // Collect video paths from command line arguments
    // Directories are scanned for video files
    let args: Vec<String> = std::env::args().collect();
    let mut video_paths: Vec<String> = Vec::new();
    
    // Common video extensions to look for
    let video_exts = ["mp4", "mov", "avi", "mkv", "webm", "flv", "wmv", "m4v", "mpg", "mpeg", "3gp", "3g2", "ogv"];
    // Extensions to explicitly exclude (images, audio, etc.)
    let exclude_exts = ["png", "jpg", "jpeg", "gif", "bmp", "tiff", "webp", "svg", "heic", "raw", "mp3", "wav", "flac", "aac", "ogg", "wma", "m4a", "txt", "json", "xml", "pdf", "doc", "zip", "tar", "gz", "dmg", "app", "exe"];
    
    for arg in args.iter().skip(1) {
        let path = std::path::Path::new(arg);
        
        if path.is_dir() {
            // Scan directory for video files
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let entry_path = entry.path();
                    if entry_path.is_file() {
                        // Skip hidden files (starting with .)
                        if let Some(filename) = entry_path.file_name() {
                            if let Some(name_str) = filename.to_str() {
                                if name_str.starts_with('.') {
                                    continue;
                                }
                            }
                        }
                        
                        if let Some(ext) = entry_path.extension() {
                            if let Some(ext_str) = ext.to_str() {
                                let ext_lower = ext_str.to_lowercase();
                                // Must be in video_exts AND not in exclude_exts
                                if video_exts.iter().any(|&e| ext_lower == e) 
                                    && !exclude_exts.iter().any(|&e| ext_lower == e) {
                                    video_paths.push(entry_path.to_string_lossy().to_string());
                                }
                            }
                        }
                    }
                }
            }
        } else if path.is_file() {
            // Direct file path
            video_paths.push(arg.clone());
        }
    }
    
    // Sort videos alphabetically for consistent ordering
    video_paths.sort();
    
    // Validate videos with ffprobe to filter out corrupted files
    println!("🔍 Validating {} potential videos...", video_paths.len());
    let valid_paths: Vec<String> = video_paths
        .into_iter()
        .filter(|path| {
            let valid = Command::new("ffprobe")
                .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=noprint_wrappers=1", path])
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false);
            if !valid {
                println!("  ✗ Skipping (invalid): {}", Path::new(path).file_name().unwrap_or_default().to_string_lossy());
            }
            valid
        })
        .collect();
    
    println!("✅ Found {} valid videos", valid_paths.len());
    
    let video_paths = valid_paths;
    
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_fullscreen(true)
            .with_title("Channel Surfer"),
        ..Default::default()
    };
    
    eframe::run_native(
        "Channel Surfer",
        options,
        Box::new(|_cc| Ok(Box::new(ChannelSurfer::new(video_paths)))),
    )
}
