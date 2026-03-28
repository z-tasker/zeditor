use eframe::egui;
use std::time::Instant;
use std::path::Path;
use std::process::Command;

mod shader;
use shader::VideoEffect;

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
    
    // Video metadata for scrubber
    video_duration_ms: f64,
    
    // UI state
    pub show_channel_info: bool,
    pub channel_info_timer: Option<Instant>,
    
    // Mouse activity tracking for controls auto-hide
    last_mouse_activity: Instant,
    mouse_pos: Option<egui::Pos2>,
    
    // Loading
    pending_video: Option<String>,
    
    // Scrubber seeking state
    is_seeking: bool,
    seek_target: f64,
    
    // Glitch mode for compression artifacts
    glitch_mode: bool,
    
    // Active CPU video effect for post-processing
    active_effect: VideoEffect,
    
    // Processed frame texture (when using CPU effects)
    processed_texture: Option<egui::TextureHandle>,
    
    // Last processed frame to prevent flicker when decoder is busy
    last_processed_frame: Option<egui::ColorImage>,
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
            video_duration_ms: 0.0,
            show_channel_info: true,
            channel_info_timer: Some(now),
            last_mouse_activity: now,
            mouse_pos: None,
            pending_video: pending,
            is_seeking: false,
            seek_target: 0.0,
            glitch_mode: false,
            active_effect: VideoEffect::default(),
            processed_texture: None,
            last_processed_frame: None,
        }
    }
    
    fn cycle_effect(&mut self) {
        self.active_effect = self.active_effect.next();
        // Clear cache when effect changes to prevent visual glitches
        self.last_processed_frame = None;
        println!("Effect: {}", self.active_effect.name());
    }
    
    fn get_effect_name(&self) -> Option<&str> {
        if self.active_effect == VideoEffect::None {
            None
        } else {
            Some(self.active_effect.name())
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
        
        // Get video duration for scrubber (public field, not method)
        self.video_duration_ms = player.duration_ms as f64;
        
        // Auto-start playback
        player.start();
        
        self.player = Some(player);
        self.playing = true;
        self.is_seeking = false;
        
        // Clear processed texture and cache on video change
        self.processed_texture = None;
        self.last_processed_frame = None;
        
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
    
    fn format_time_pair(current_secs: f64, total_secs: f64) -> (String, String) {
        let current_hours = (current_secs / 3600.0) as u64;
        let current_mins = ((current_secs % 3600.0) / 60.0) as u64;
        let current_secs_rem = (current_secs % 60.0) as u64;
        
        let total_hours = (total_secs / 3600.0) as u64;
        let total_mins = ((total_secs % 3600.0) / 60.0) as u64;
        let total_secs_rem = (total_secs % 60.0) as u64;
        
        // Match format based on total duration length
        if total_hours > 0 {
            // Long videos: HH:MM:SS
            (
                format!("{:02}:{:02}:{:02}", current_hours, current_mins, current_secs_rem),
                format!("{:02}:{:02}:{:02}", total_hours, total_mins, total_secs_rem)
            )
        } else if total_mins > 0 {
            // Medium videos: MM:SS
            (
                format!("{:02}:{:02}", current_mins, current_secs_rem),
                format!("{:02}:{:02}", total_mins, total_secs_rem)
            )
        } else {
            // Short videos (< 1 min): just seconds
            (
                format!("{}", current_secs_rem),
                format!("{}", total_secs_rem)
            )
        }
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
                if self.glitch_mode {
                    ui.label(
                        egui::RichText::new("GLITCH")
                            .size(14.0)
                            .color(egui::Color32::RED)
                            .strong()
                    );
                }
                if let Some(effect_name) = self.get_effect_name() {
                    ui.label(
                        egui::RichText::new(format!("FX: {}", effect_name))
                            .size(12.0)
                            .color(egui::Color32::YELLOW)
                            .strong()
                    );
                }
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
        
        // Auto-hide OS cursor after 3 seconds of mouse inactivity
        let cursor_should_show = self.last_mouse_activity.elapsed().as_secs() < 3;
        if cursor_should_show {
            ctx.set_cursor_icon(egui::CursorIcon::Default);
        } else {
            ctx.set_cursor_icon(egui::CursorIcon::None);
        }
        
        // Handle keyboard input (not mouse clicks)
        ctx.input(|i| {
            // Right arrow -> next channel
            if i.key_pressed(egui::Key::ArrowRight) {
                self.next_channel();
            }
            
            // Left arrow -> previous channel
            if i.key_pressed(egui::Key::ArrowLeft) {
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
            
            // G to toggle glitch mode
            if i.key_pressed(egui::Key::G) {
                self.glitch_mode = !self.glitch_mode;
                println!("Glitch mode: {}", if self.glitch_mode { "ON" } else { "OFF" });
            }
            
            // S to cycle through video effects
            if i.key_pressed(egui::Key::S) {
                self.cycle_effect();
            }
            
            // Mouse scroll wheel to cycle effects
            let scroll_delta = i.smooth_scroll_delta;
            if scroll_delta.y > 0.0 {
                // Scroll up - previous effect
                self.active_effect = match self.active_effect {
                    VideoEffect::Pixelate { block_size } if block_size > 8 => 
                        VideoEffect::Pixelate { block_size: block_size / 2 },
                    VideoEffect::Pixelate { .. } => VideoEffect::None,
                    VideoEffect::Sepia => VideoEffect::Pixelate { block_size: 32 },
                    VideoEffect::RgbSplit { offset } if offset > 5 =>
                        VideoEffect::RgbSplit { offset: offset - 5 },
                    VideoEffect::RgbSplit { .. } => VideoEffect::Sepia,
                    VideoEffect::Invert => VideoEffect::RgbSplit { offset: 15 },
                    VideoEffect::Contrast { factor } if factor > 1.5 =>
                        VideoEffect::Contrast { factor: factor - 0.5 },
                    VideoEffect::Contrast { .. } => VideoEffect::Invert,
                    VideoEffect::Compression { quality } if quality < 40 =>
                        VideoEffect::Compression { quality: quality * 2 },
                    VideoEffect::Compression { .. } => VideoEffect::Contrast { factor: 2.5 },
                    VideoEffect::Glitch { .. } => VideoEffect::Compression { quality: 5 },
                    VideoEffect::MotionGlitch { trail_length } if trail_length > 5 =>
                        VideoEffect::MotionGlitch { trail_length: trail_length - 5 },
                    VideoEffect::MotionGlitch { .. } => VideoEffect::Glitch { intensity: 10, seed: 0 },
                    VideoEffect::Datamosh { displacement } if displacement < 0 =>
                        VideoEffect::Datamosh { displacement: -displacement },
                    VideoEffect::Datamosh { .. } => VideoEffect::MotionGlitch { trail_length: 15 },
                    VideoEffect::None => VideoEffect::Datamosh { displacement: -8 },
                };
                self.last_processed_frame = None;
                println!("Effect: {}", self.active_effect.name());
            } else if scroll_delta.y < 0.0 {
                // Scroll down - next effect (same as S key)
                self.cycle_effect();
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
                
                // Apply CPU effects if active
                let video_response = if self.active_effect != VideoEffect::None {
                    // Try to get latest frame and apply effect
                    let frame_to_display = if let Some(original_frame) = player.get_latest_frame() {
                        // Process new frame
                        let processed = self.active_effect.apply(&original_frame);
                        self.last_processed_frame = Some(processed.clone());
                        processed
                    } else if let Some(ref cached) = self.last_processed_frame {
                        // Use cached frame if we can't get new one (prevents flicker)
                        cached.clone()
                    } else {
                        // No frame at all yet - create placeholder
                        egui::ColorImage::new([1, 1], egui::Color32::BLACK)
                    };
                    
                    // Update or create processed texture
                    let texture = self.processed_texture.get_or_insert_with(|| {
                        ui.ctx().load_texture(
                            "processed_video",
                            egui::ColorImage::example(),
                            Default::default()
                        )
                    });
                    
                    texture.set(frame_to_display, Default::default());
                    
                    // Display processed frame
                    ui.centered_and_justified(|ui| {
                        ui.add(
                            egui::Image::new(egui::load::SizedTexture::new(texture.id(), target_size))
                                .sense(egui::Sense::click())
                        )
                    }).inner
                } else {
                    // No effect - display normal video
                    ui.centered_and_justified(|ui| {
                        player.render_frame(ui, target_size)
                    }).inner
                };
                
                // Handle clicks on the video area (not on UI controls like scrubber)
                // Only change channel if click is in the upper 85% of screen (above scrubber)
                let click_pos = ctx.input(|i| i.pointer.latest_pos());
                let screen_height = ctx.screen_rect().height();
                let is_above_scrubber = click_pos.map(|p| p.y < screen_height * 0.85).unwrap_or(false);
                
                if is_above_scrubber {
                    if video_response.clicked_by(egui::PointerButton::Primary) {
                        self.next_channel();
                    }
                    if video_response.clicked_by(egui::PointerButton::Secondary) {
                        self.prev_channel();
                    }
                }
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
        
        // Bottom scrubber panel - auto-hides/shows based on mouse activity like cursor
        let show_scrubber = self.last_mouse_activity.elapsed().as_secs() < 3;
        if show_scrubber {
            egui::TopBottomPanel::bottom("scrubber_panel")
                .frame(egui::Frame::none()
                    .inner_margin(8.0)
                    .fill(egui::Color32::from_rgb(20, 20, 20)))
                .show(ctx, |ui| {
                    if let Some(ref player) = self.player {
                        let duration_ms = self.video_duration_ms;
                        let current_ms = if self.is_seeking {
                            self.seek_target * duration_ms
                        } else {
                            player.elapsed_ms() as f64
                        };
                        
                        if duration_ms > 0.0 {
                            let progress = current_ms / duration_ms;
                            let bar_width = ui.available_width() - 20.0;
                            let bar_height = 28.0;
                            
                            // Draw a custom full-width progress bar
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(bar_width, bar_height),
                                egui::Sense::click_and_drag()
                            );
                            
                            // Draw the background
                            let bg_color = egui::Color32::from_rgb(50, 50, 50);
                            ui.painter().rect_filled(rect, 4.0, bg_color);
                            
                            // Draw the filled portion
                            let filled_width = rect.width() * progress as f32;
                            if filled_width > 0.0 {
                                let filled_rect = egui::Rect::from_min_size(
                                    rect.min,
                                    egui::vec2(filled_width, rect.height())
                                );
                                ui.painter().rect_filled(filled_rect, 4.0, egui::Color32::from_rgb(0, 120, 255));
                            }
                            
                            // Draw the handle
                            let handle_size = 16.0;
                            let handle_pos = egui::pos2(
                                rect.min.x + filled_width - (handle_size / 2.0),
                                rect.center().y - (handle_size / 2.0)
                            );
                            let handle_rect = egui::Rect::from_min_size(handle_pos, egui::vec2(handle_size, handle_size));
                            ui.painter().circle_filled(
                                handle_rect.center(),
                                handle_size / 2.0,
                                egui::Color32::WHITE
                            );
                            
                            // Handle interaction
                            if response.dragged() || response.clicked() {
                                if let Some(pointer_pos) = response.interact_pointer_pos() {
                                    let relative_x = (pointer_pos.x - rect.min.x).max(0.0).min(rect.width());
                                    let seek_frac = relative_x / rect.width();
                                    
                                    self.is_seeking = true;
                                    self.seek_target = seek_frac as f64;
                                    
                                    // Perform the seek
                                    if let Some(ref mut player) = self.player {
                                        player.seek(seek_frac as f32);
                                    }
                                }
                            }
                            
                            if response.drag_stopped() {
                                self.is_seeking = false;
                            }
                            
                            // Time and percentage on a second line
                            ui.horizontal(|ui| {
                                let current_secs = current_ms / 1000.0;
                                let total_secs = duration_ms / 1000.0;
                                let (current_formatted, total_formatted) = Self::format_time_pair(current_secs, total_secs);
                                ui.label(
                                    egui::RichText::new(format!("{} / {}", current_formatted, total_formatted))
                                        .size(13.0)
                                        .color(egui::Color32::LIGHT_GRAY)
                                );
                                
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{:.0}%", progress * 100.0))
                                            .size(13.0)
                                            .color(egui::Color32::LIGHT_GRAY)
                                    );
                                });
                            });
                        }
                    }
                });
        }
        
        // Controls are intuitive: click video for next channel, arrow keys, space to pause, etc.
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
