mod controls;
mod ui;

use eframe::egui;
use std::process::Command;
use std::time::Instant;
use serde_json::Value;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Normal,
    Insert,
}

pub struct App {
    pub video_path: Option<String>,
    pub total_frames: Option<u64>,
    pub current_frame: u64,
    pub fps: Option<f64>,
    pub duration: Option<f64>,
    pub playing: bool,
    pub speed: f64,
    pub mode: Mode,
    pub chunk_frames: u64,
    pub clip_start: Option<u64>,
    pub clip_end: Option<u64>,
    pub clip_name: String,
    pub naming_clip: bool,
    pub muted: bool,
    pub looping_clip: bool,
    pub export_status: Option<String>,
    pub status_time: Option<Instant>,
    last_update: Instant,
    pub pending_video: Option<String>,
    // egui-video fields
    player: Option<egui_video::Player>,
    audio_device: Option<egui_video::AudioDevice>,
    // Speed control
    last_speed_step: Instant,
}

impl App {
    fn new(video_path: Option<String>) -> Self {
        // Initialize audio device
        let audio_device = egui_video::AudioDevice::new().ok();
        
        Self {
            video_path: None,
            total_frames: None,
            current_frame: 0,
            fps: None,
            duration: None,
            playing: false,
            speed: 1.0,
            mode: Mode::Normal,
            chunk_frames: 30,
            clip_start: None,
            clip_end: None,
            clip_name: String::new(),
            naming_clip: false,
            muted: true, // Start muted
            looping_clip: false,
            export_status: None,
            status_time: None,
            last_update: Instant::now(),
            pending_video: video_path,
            player: None,
            audio_device,
            last_speed_step: Instant::now(),
        }
    }

    fn load_video(&mut self, ctx: &egui::Context, path: &str) -> anyhow::Result<()> {
        // Ensure ffmpeg is available for export
        ffmpeg_sidecar::download::auto_download()?;

        // Get video metadata via ffprobe
        let output = Command::new("ffprobe")
            .args([
                "-v", "quiet",
                "-print_format", "json",
                "-show_format",
                "-show_streams",
                path,
            ])
            .output()?;

        let json: Value = serde_json::from_slice(&output.stdout)?;
        let streams = json["streams"].as_array().ok_or(anyhow::anyhow!("no streams"))?;
        let video_stream = streams.iter().find(|s| s["codec_type"] == "video")
            .ok_or(anyhow::anyhow!("no video stream"))?;

        let duration: f64 = json["format"]["duration"].as_str()
            .ok_or(anyhow::anyhow!("no duration"))?.parse()?;
        let fps_str = video_stream["r_frame_rate"].as_str()
            .ok_or(anyhow::anyhow!("no frame rate"))?;
        let fps_parts: Vec<&str> = fps_str.split('/').collect();
        let fps: f64 = fps_parts[0].parse::<f64>()? / fps_parts[1].parse::<f64>()?;

        // Create egui-video player
        let path_string = path.to_string();
        let mut player = egui_video::Player::new(ctx, &path_string)?;
        
        // Add audio if device is available
        if let Some(ref mut audio_device) = self.audio_device {
            player = player.with_audio(audio_device)?;
            // Start muted
            if self.muted {
                player.options.audio_volume.set(0.0);
            }
        }

        self.video_path = Some(path.to_string());
        self.fps = Some(fps);
        self.duration = Some(duration);
        self.total_frames = Some((duration * fps) as u64);
        self.current_frame = 0;
        self.player = Some(player);

        Ok(())
    }

    fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        if let Some(ref mut player) = self.player {
            let volume = if self.muted { 0.0 } else { 1.0 };
            player.options.audio_volume.set(volume as f32);
        }
    }

    fn update_state(&mut self) {
        let now = Instant::now();

        // Determine loop boundaries
        let (loop_start, loop_end) = if self.looping_clip {
            (self.clip_start, self.clip_end)
        } else {
            (None, None)
        };

        // Handle speed modulation via frame stepping (for non-1x speeds)
        if self.playing && self.speed != 1.0 {
            let fps = self.fps.unwrap_or(30.0);
            let step_interval = std::time::Duration::from_millis((1000.0 / (fps * self.speed)) as u64);
            
            if now.duration_since(self.last_speed_step) >= step_interval {
                let total = self.total_frames.unwrap_or(u64::MAX);
                let current = self.current_frame;
                
                // Calculate next frame, respecting loop boundaries
                let mut next_frame = current.saturating_add(1);
                
                // Handle looping
                if let (Some(start), Some(end)) = (loop_start, loop_end) {
                    if next_frame >= end {
                        next_frame = start;
                    }
                } else if next_frame >= total {
                    next_frame = 0;
                }
                
                // Seek to next frame
                if let (Some(ref mut player), Some(duration)) = (&mut self.player, self.duration) {
                    let time = next_frame as f64 / fps;
                    let seek_frac = (time / duration) as f32;
                    player.seek(seek_frac.clamp(0.0, 1.0));
                    player.process_state();
                    self.current_frame = next_frame;
                }
                
                self.last_speed_step = now;
            }
        } else if self.playing {
            // Normal 1x playback: Update current frame from player position
            if let Some(ref player) = self.player {
                if let Some(fps) = self.fps {
                    let elapsed_ms = player.elapsed_ms();
                    self.current_frame = ((elapsed_ms as f64 / 1000.0) * fps) as u64;
                    
                    // Handle looping at 1x speed
                    if let (Some(start), Some(end)) = (loop_start, loop_end) {
                        if self.current_frame >= end {
                            self.seek_to_frame(start);
                        }
                    }
                }
            }
        }

        // Clear export status after 3 seconds
        if let Some(status_time) = self.status_time {
            if now.duration_since(status_time).as_secs() > 3 {
                self.export_status = None;
                self.status_time = None;
            }
        }

        self.last_update = now;
    }

    fn export_clip(&mut self) -> anyhow::Result<String> {
        let start = self.clip_start.ok_or(anyhow::anyhow!("no clip start"))?;
        let end = self.clip_end.ok_or(anyhow::anyhow!("no clip end"))?;
        let video_path = self.video_path.as_ref().ok_or(anyhow::anyhow!("no video"))?;
        let fps = self.fps.ok_or(anyhow::anyhow!("no fps"))?;

        if self.clip_name.is_empty() {
            return Err(anyhow::anyhow!("no clip name"));
        }

        let start_time = start as f64 / fps;
        let duration = (end - start) as f64 / fps;

        let video_stem = std::path::Path::new(video_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("clips");

        let clips_dir = format!("{}_clips", video_stem);
        std::fs::create_dir_all(&clips_dir)?;

        let filename = if std::path::Path::new(&self.clip_name).extension().is_none() {
            format!("{}/{}.mp4", clips_dir, self.clip_name)
        } else {
            format!("{}/{}", clips_dir, self.clip_name)
        };

        let output = Command::new("ffmpeg")
            .args([
                "-y",
                "-i", video_path,
                "-ss", &format!("{:.3}", start_time),
                "-t", &format!("{:.3}", duration),
                "-c", "copy",
                &filename,
            ])
            .output()?;

        if output.status.success() {
            Ok(filename)
        } else {
            Err(anyhow::anyhow!("ffmpeg failed"))
        }
    }

    fn seek_to_frame(&mut self, frame: u64) {
        if let (Some(ref mut player), Some(duration)) = (&mut self.player, self.duration) {
            if let Some(fps) = self.fps {
                let time = frame as f64 / fps;
                let seek_frac = (time / duration) as f32;
                player.seek(seek_frac.clamp(0.0, 1.0));
                // Force position update after seek
                player.process_state();
                // Update current_frame after seek
                self.current_frame = frame;
            }
        }
    }

    fn toggle_play(&mut self) {
        self.playing = !self.playing;
        if let Some(ref mut player) = self.player {
            if self.playing {
                // Only use normal playback if speed is 1.0
                if self.speed == 1.0 {
                    player.start();
                }
            } else {
                player.pause();
            }
        }
        self.last_speed_step = Instant::now();
    }

    fn set_speed(&mut self, speed: f64) {
        let old_speed = self.speed;
        self.speed = speed.clamp(0.1, 10.0);
        
        // Handle playback mode change
        if let Some(ref mut player) = self.player {
            if self.playing {
                if self.speed == 1.0 && old_speed != 1.0 {
                    // Switching back to normal playback - seek to current position first
                    // then start playback from there
                    if let (Some(duration), Some(fps)) = (self.duration, self.fps) {
                        let time = self.current_frame as f64 / fps;
                        let seek_frac = (time / duration) as f32;
                        player.seek(seek_frac.clamp(0.0, 1.0));
                        player.process_state();
                    }
                    player.start();
                } else if self.speed != 1.0 {
                    // Manual frame stepping mode - pause the player
                    player.pause();
                }
            }
        }
        self.last_speed_step = Instant::now();
    }

    fn pause_player(&mut self) {
        if let Some(ref mut player) = self.player {
            player.pause();
        }
        self.playing = false;
    }

    fn resume_player(&mut self) {
        if let Some(ref mut player) = self.player {
            player.start();
        }
        self.playing = true;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check if we need to load a pending video
        if let Some(path) = self.pending_video.take() {
            if let Err(e) = self.load_video(ctx, &path) {
                self.export_status = Some(format!("Failed to load video: {}", e));
                self.status_time = Some(Instant::now());
            }
        }

        // Update state
        self.update_state();

        // Request continuous repaints for smooth updates
        ctx.request_repaint();

        // Render UI panels
        self.render_top_panel(ctx);
        self.render_bottom_panel(ctx);
        self.render_central_panel(ctx);

        // Keyboard shortcuts
        self.handle_keyboard(ctx);

        // Handle file drops
        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                if let Some(path) = i.raw.dropped_files[0].path.as_ref() {
                    self.pending_video = Some(path.to_string_lossy().to_string());
                }
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let video_path = args.get(1).cloned();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 600.0])
            .with_title("zeditor"),
        ..Default::default()
    };

    eframe::run_native(
        "zeditor",
        options,
        Box::new(|_cc| Ok(Box::new(App::new(video_path)))),
    )
}
