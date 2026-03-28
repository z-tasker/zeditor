use eframe::egui;
use std::time::Instant;
use std::path::Path;

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
    
    // Loading
    pending_video: Option<String>,
}

impl ChannelSurfer {
    pub fn new(video_paths: Vec<String>) -> Self {
        // Initialize audio device
        let audio_device = egui_video::AudioDevice::new().ok();
        
        // Get first video to load
        let pending = video_paths.first().cloned();
        
        Self {
            channels: video_paths,
            current_channel: 0,
            playing: true, // Auto-play
            muted: true,   // Start muted
            player: None,
            audio_device,
            show_channel_info: true,
            channel_info_timer: Some(Instant::now()),
            pending_video: pending,
        }
    }
    
    fn load_video(&mut self, ctx: &egui::Context, path: &str) -> anyhow::Result<()> {
        // Ensure ffmpeg is available
        ffmpeg_sidecar::download::auto_download()?;
        
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
    
    fn next_channel(&mut self, ctx: &egui::Context) {
        if self.channels.is_empty() {
            return;
        }
        
        self.current_channel = (self.current_channel + 1) % self.channels.len();
        let path = self.channels[self.current_channel].clone();
        
        if let Err(e) = self.load_video(ctx, &path) {
            eprintln!("Failed to load channel {}: {}", self.current_channel, e);
        }
    }
    
    fn prev_channel(&mut self, ctx: &egui::Context) {
        if self.channels.is_empty() {
            return;
        }
        
        self.current_channel = if self.current_channel == 0 {
            self.channels.len() - 1
        } else {
            self.current_channel - 1
        };
        
        let path = self.channels[self.current_channel].clone();
        
        if let Err(e) = self.load_video(ctx, &path) {
            eprintln!("Failed to load channel {}: {}", self.current_channel, e);
        }
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
                eprintln!("Failed to load video: {}", e);
            }
        }
        
        // Update player state
        if let Some(ref mut player) = self.player {
            player.process_state();
        }
        
        // Request continuous repaints for smooth playback
        ctx.request_repaint();
        
        // Handle input
        ctx.input(|i| {
            // Left click or right arrow -> next channel
            if i.pointer.button_pressed(egui::PointerButton::Primary) ||
               i.key_pressed(egui::Key::ArrowRight) {
                self.next_channel(ctx);
            }
            
            // Right click or left arrow -> previous channel
            if i.pointer.button_pressed(egui::PointerButton::Secondary) ||
               i.key_pressed(egui::Key::ArrowLeft) {
                self.prev_channel(ctx);
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
        
        // Central panel - video display
        egui::CentralPanel::default().show(ctx, |ui| {
            // Black background
            let rect = ui.available_rect_before_wrap();
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
        
        // Bottom panel - controls hint
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

fn main() -> eframe::Result<()> {
    // Collect video paths from command line arguments
    let args: Vec<String> = std::env::args().collect();
    let video_paths: Vec<String> = args.into_iter().skip(1).collect();
    
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([640.0, 480.0])
            .with_title("Channel Surfer"),
        ..Default::default()
    };
    
    eframe::run_native(
        "Channel Surfer",
        options,
        Box::new(|_cc| Ok(Box::new(ChannelSurfer::new(video_paths)))),
    )
}
