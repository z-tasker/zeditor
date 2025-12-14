use eframe::egui;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    Insert,
}

struct MpvHandle {
    stream: UnixStream,
    _child: Child,
}

impl MpvHandle {
    fn send_command(&mut self, command: serde_json::Value) {
        let msg = format!("{}\n", command.to_string());
        let _ = self.stream.write_all(msg.as_bytes());
    }

    fn get_property_async(&mut self, property: &str, request_id: i64) {
        let cmd = serde_json::json!({
            "command": ["get_property", property],
            "request_id": request_id
        });
        let msg = format!("{}\n", cmd.to_string());
        let _ = self.stream.write_all(msg.as_bytes());
    }
}

struct App {
    video_path: Option<String>,
    total_frames: Option<u64>,
    current_frame: u64,
    fps: Option<f64>,
    duration: Option<f64>,
    playing: bool,
    speed: f64,
    mode: Mode,
    chunk_frames: u64,
    clip_start: Option<u64>,
    clip_end: Option<u64>,
    clip_name: String,
    mpv: Option<MpvHandle>,
    muted: bool,
    looping_clip: bool,
    export_status: Option<String>,
    status_time: Option<Instant>,
    last_update: Instant,
    pending_video: Option<String>,
}

impl App {
    fn new(video_path: Option<String>) -> Self {
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
            mpv: None,
            muted: true, // Start muted
            looping_clip: false,
            export_status: None,
            status_time: None,
            last_update: Instant::now(),
            pending_video: video_path,
        }
    }

    fn load_video_metadata(&mut self, path: &str) -> anyhow::Result<()> {
        // Ensure ffmpeg is available
        ffmpeg_sidecar::download::auto_download()?;

        // Run ffprobe to get video info
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

        self.video_path = Some(path.to_string());
        self.fps = Some(fps);
        self.duration = Some(duration);
        self.total_frames = Some((duration * fps) as u64);
        self.current_frame = 0;

        Ok(())
    }

    fn spawn_mpv(&mut self, path: &str) -> anyhow::Result<()> {
        let sock_path = "/tmp/zeditor_mpv.sock";
        let _ = std::fs::remove_file(sock_path);

        // Spawn mpv in its own window, positioned to the side
        let child = Command::new("mpv")
            .args([
                "--really-quiet",
                "--no-terminal",
                "--force-window=yes",
                "--input-ipc-server=/tmp/zeditor_mpv.sock",
                "--keep-open=yes",
                "--idle=no",
                "--geometry=900x600+50+50",
                "--title=zeditor - video",
                path,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        // Wait for socket
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Connect to socket
        let stream = UnixStream::connect(sock_path)?;
        stream.set_nonblocking(true)?;

        self.mpv = Some(MpvHandle { stream, _child: child });

        // Start muted
        let cmd = serde_json::json!({"command": ["set_property", "mute", true]});
        self.send_mpv_command(cmd);

        Ok(())
    }

    fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        let cmd = serde_json::json!({"command": ["set_property", "mute", self.muted]});
        self.send_mpv_command(cmd);
    }

    fn send_mpv_command(&mut self, command: serde_json::Value) {
        if let Some(mpv) = &mut self.mpv {
            mpv.send_command(command);
        }
    }

    fn poll_mpv(&mut self) {
        if let Some(mpv) = &mut self.mpv {
            // Try to read responses
            if let Ok(stream) = mpv.stream.try_clone() {
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    if let Ok(response) = serde_json::from_str::<Value>(&line) {
                        if let Some(data) = response.get("data") {
                            if let Some(time) = data.as_f64() {
                                if let Some(fps) = self.fps {
                                    self.current_frame = (time * fps) as u64;
                                }
                            }
                        }
                    }
                    line.clear();
                }
            }
            // Request current time
            mpv.get_property_async("time-pos", 1);
        }
    }

    fn update_state(&mut self) {
        let now = Instant::now();

        // Poll mpv for current position
        self.poll_mpv();

        // Handle clip looping
        if self.looping_clip {
            if let (Some(start), Some(end), Some(fps)) = (self.clip_start, self.clip_end, self.fps) {
                if self.current_frame >= end {
                    let start_time = start as f64 / fps;
                    let cmd = serde_json::json!({"command": ["seek", start_time, "absolute"]});
                    self.send_mpv_command(cmd);
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
        self.current_frame = frame;
        if let Some(fps) = self.fps {
            let time = frame as f64 / fps;
            let cmd = serde_json::json!({"command": ["seek", time, "absolute"]});
            self.send_mpv_command(cmd);
        }
    }

    fn toggle_play(&mut self) {
        self.playing = !self.playing;
        let cmd = serde_json::json!({"command": ["cycle", "pause"]});
        self.send_mpv_command(cmd);
    }

    fn set_speed(&mut self, speed: f64) {
        self.speed = speed.clamp(0.1, 10.0);
        let cmd = serde_json::json!({"command": ["set_property", "speed", self.speed]});
        self.send_mpv_command(cmd);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check if we need to load a pending video
        if let Some(path) = self.pending_video.take() {
            if let Err(e) = self.load_video_metadata(&path) {
                self.export_status = Some(format!("Failed to load metadata: {}", e));
                self.status_time = Some(Instant::now());
            } else if let Err(e) = self.spawn_mpv(&path) {
                self.export_status = Some(format!("Failed to spawn mpv: {}", e));
                self.status_time = Some(Instant::now());
            }
        }

        // Update state
        self.update_state();

        // Request continuous repaints for smooth updates
        ctx.request_repaint();

        // Top panel for mode and video info
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let mode_text = match self.mode {
                    Mode::Normal => "NORMAL",
                    Mode::Insert => "INSERT",
                };
                let mode_color = match self.mode {
                    Mode::Normal => egui::Color32::GREEN,
                    Mode::Insert => egui::Color32::YELLOW,
                };
                ui.colored_label(mode_color, egui::RichText::new(mode_text).strong().size(16.0));
                ui.separator();

                if let Some(path) = &self.video_path {
                    ui.label(path);
                } else {
                    ui.label("No video loaded - drag & drop or pass as argument");
                }

                if self.looping_clip {
                    ui.separator();
                    ui.colored_label(egui::Color32::LIGHT_BLUE, "LOOPING");
                }
            });
        });

        // Bottom panel for controls
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            // Status bar
            if let Some(status) = &self.export_status {
                ui.colored_label(egui::Color32::GREEN, format!(">> {}", status));
                ui.separator();
            }

            // Timeline/scrubber
            if let (Some(total), Some(fps)) = (self.total_frames, self.fps) {
                let mut frame = self.current_frame as f64;
                
                ui.horizontal(|ui| {
                    ui.label(format!("Frame: {} / {}", self.current_frame, total));
                    ui.separator();
                    ui.label(format!("Time: {:.2}s / {:.2}s",
                        self.current_frame as f64 / fps,
                        total as f64 / fps));
                    ui.separator();
                    ui.label(format!("Speed: {:.1}x", self.speed));
                    ui.separator();
                    let mute_text = if self.muted { "MUTED" } else { "Audio ON" };
                    let mute_color = if self.muted { egui::Color32::GRAY } else { egui::Color32::LIGHT_GREEN };
                    ui.colored_label(mute_color, mute_text);
                });

                let slider = egui::Slider::new(&mut frame, 0.0..=(total as f64))
                    .show_value(false)
                    .trailing_fill(true);

                if ui.add(slider).changed() {
                    self.seek_to_frame(frame as u64);
                }

                // Clip markers
                ui.horizontal(|ui| {
                    if let Some(start) = self.clip_start {
                        ui.colored_label(egui::Color32::GREEN,
                            format!("IN: {} ({:.2}s)", start, start as f64 / fps));
                    }
                    if let Some(end) = self.clip_end {
                        ui.colored_label(egui::Color32::RED,
                            format!("OUT: {} ({:.2}s)", end, end as f64 / fps));
                    }
                    if let (Some(start), Some(end)) = (self.clip_start, self.clip_end) {
                        let dur = (end - start) as f64 / fps;
                        ui.label(format!("Clip: {:.2}s", dur));
                    }
                });
            }

            ui.separator();

            // Playback controls
            ui.horizontal(|ui| {
                if ui.button(if self.playing { "|| Pause" } else { "|> Play" }).clicked() {
                    self.toggle_play();
                }

                ui.separator();

                if ui.button("<<").clicked() {
                    let f = self.current_frame.saturating_sub(self.chunk_frames);
                    self.seek_to_frame(f);
                }
                if ui.button("<").clicked() {
                    let f = self.current_frame.saturating_sub(1);
                    self.seek_to_frame(f);
                }
                if ui.button(">").clicked() {
                    let f = self.current_frame.saturating_add(1);
                    self.seek_to_frame(f);
                }
                if ui.button(">>").clicked() {
                    let f = self.current_frame.saturating_add(self.chunk_frames);
                    self.seek_to_frame(f);
                }

                ui.separator();

                if ui.button("-").clicked() {
                    self.set_speed(self.speed - 0.5);
                }
                if ui.button("+").clicked() {
                    self.set_speed(self.speed + 0.5);
                }

                ui.separator();

                let in_color = if self.clip_start.is_some() { egui::Color32::GREEN } else { egui::Color32::GRAY };
                if ui.button(egui::RichText::new("[I]n").color(in_color)).clicked() {
                    self.clip_start = Some(self.current_frame);
                }

                let out_color = if self.clip_end.is_some() { egui::Color32::RED } else { egui::Color32::GRAY };
                if ui.button(egui::RichText::new("[O]ut").color(out_color)).clicked() {
                    self.clip_end = Some(self.current_frame);
                    if self.clip_start.is_some() {
                        self.looping_clip = true;
                        if let Some(start) = self.clip_start {
                            self.seek_to_frame(start);
                        }
                    }
                }

                if ui.button("Clear").clicked() {
                    self.clip_start = None;
                    self.clip_end = None;
                    self.looping_clip = false;
                }

                let loop_text = if self.looping_clip { "Loop ON" } else { "Loop OFF" };
                if ui.button(loop_text).clicked() {
                    self.looping_clip = !self.looping_clip;
                }
            });

            ui.separator();

            // Clip name and export
            ui.horizontal(|ui| {
                ui.label("Clip name:");
                let response = ui.text_edit_singleline(&mut self.clip_name);

                let can_export = self.clip_start.is_some() && self.clip_end.is_some() && !self.clip_name.is_empty();

                if ui.add_enabled(can_export, egui::Button::new("Export")).clicked() {
                    match self.export_clip() {
                        Ok(filename) => {
                            self.export_status = Some(format!("Saved: {}", filename));
                            self.status_time = Some(Instant::now());
                            self.clip_start = None;
                            self.clip_end = None;
                            self.clip_name.clear();
                            self.looping_clip = false;
                        }
                        Err(e) => {
                            self.export_status = Some(format!("Export failed: {}", e));
                            self.status_time = Some(Instant::now());
                        }
                    }
                }

                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && can_export {
                    match self.export_clip() {
                        Ok(filename) => {
                            self.export_status = Some(format!("Saved: {}", filename));
                            self.status_time = Some(Instant::now());
                            self.clip_start = None;
                            self.clip_end = None;
                            self.clip_name.clear();
                            self.looping_clip = false;
                        }
                        Err(e) => {
                            self.export_status = Some(format!("Export failed: {}", e));
                            self.status_time = Some(Instant::now());
                        }
                    }
                }
            });

            ui.separator();

            // Help
            ui.horizontal(|ui| {
                let help = match self.mode {
                    Mode::Normal => "[i] Insert mode | [Space] Play/Pause | [w/b] Speed | [Shift+I/O] Set marks",
                    Mode::Insert => "[Esc/Enter] Exit | [h/l] Frame | [w/b] Chunk | [i] IN | [o] OUT",
                };
                ui.label(help);
            });
        });

        // Central panel - instructions
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| {
                if self.video_path.is_some() {
                    ui.label("Video playing in mpv window");
                } else {
                    ui.label("Drag and drop a video file\nor pass path as command line argument");
                }
            });
        });

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

impl App {
    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        let mut mode_change: Option<Mode> = None;
        let mut seek_frame: Option<u64> = None;
        let mut should_toggle_play = false;
        let mut should_set_in = false;
        let mut should_set_out = false;
        let mut speed_delta: f64 = 0.0;
        let mut should_toggle_mute = false;

        // Check if text input has focus
        let text_focused = ctx.memory(|m| m.focused().is_some());

        ctx.input(|i| {
            // In normal mode, skip letter keys if text field is focused (but allow space, brackets)
            let skip_letters = text_focused && self.mode == Mode::Normal;

            // 'a' toggles mute in any mode (unless typing)
            if i.key_pressed(egui::Key::A) && !skip_letters {
                should_toggle_mute = true;
            }

            match self.mode {
                Mode::Normal => {
                    if !skip_letters {
                        if i.key_pressed(egui::Key::I) && !i.modifiers.shift {
                            mode_change = Some(Mode::Insert);
                        }
                        if i.key_pressed(egui::Key::I) && i.modifiers.shift {
                            should_set_in = true;
                        }
                        if i.key_pressed(egui::Key::O) && i.modifiers.shift {
                            should_set_out = true;
                        }
                        if i.key_pressed(egui::Key::L) {
                            should_toggle_play = true;
                        }
                        if i.key_pressed(egui::Key::W) {
                            speed_delta = 0.5;
                        }
                        if i.key_pressed(egui::Key::B) {
                            speed_delta = -0.5;
                        }
                    }
                    // These work even when text focused
                    if i.key_pressed(egui::Key::Space) {
                        should_toggle_play = true;
                    }
                    if i.key_pressed(egui::Key::CloseBracket) {
                        speed_delta = 0.5;
                    }
                    if i.key_pressed(egui::Key::OpenBracket) {
                        speed_delta = -0.5;
                    }
                }
                Mode::Insert => {
                    if i.key_pressed(egui::Key::Escape) {
                        mode_change = Some(Mode::Normal);
                    }
                    if i.key_pressed(egui::Key::H) || i.key_pressed(egui::Key::ArrowLeft) {
                        seek_frame = Some(self.current_frame.saturating_sub(1));
                    }
                    if i.key_pressed(egui::Key::L) || i.key_pressed(egui::Key::ArrowRight) {
                        let max = self.total_frames.unwrap_or(u64::MAX).saturating_sub(1);
                        seek_frame = Some(self.current_frame.saturating_add(1).min(max));
                    }
                    if i.key_pressed(egui::Key::B) || i.key_pressed(egui::Key::ArrowUp) {
                        seek_frame = Some(self.current_frame.saturating_sub(self.chunk_frames));
                    }
                    if i.key_pressed(egui::Key::W) || i.key_pressed(egui::Key::ArrowDown) {
                        let max = self.total_frames.unwrap_or(u64::MAX).saturating_sub(1);
                        seek_frame = Some(self.current_frame.saturating_add(self.chunk_frames).min(max));
                    }
                    // i sets IN point (if valid: must be <= OUT if OUT exists)
                    if i.key_pressed(egui::Key::I) {
                        let valid = self.clip_end.map_or(true, |end| self.current_frame <= end);
                        if valid {
                            should_set_in = true;
                        }
                    }
                    // o sets OUT point (if valid: must be >= IN if IN exists)
                    if i.key_pressed(egui::Key::O) {
                        let valid = self.clip_start.map_or(true, |start| self.current_frame >= start);
                        if valid {
                            should_set_out = true;
                        }
                    }
                    // Enter exits insert mode
                    if i.key_pressed(egui::Key::Enter) {
                        mode_change = Some(Mode::Normal);
                    }
                }
            }
        });

        // Apply changes
        if let Some(new_mode) = mode_change {
            if new_mode == Mode::Insert {
                // Entering insert mode - pause
                self.playing = false;
                let cmd = serde_json::json!({"command": ["set_property", "pause", true]});
                self.send_mpv_command(cmd);
            } else if new_mode == Mode::Normal && self.mode == Mode::Insert {
                // Exiting insert mode - resume playback
                self.playing = true;
                let cmd = serde_json::json!({"command": ["set_property", "pause", false]});
                self.send_mpv_command(cmd);
            }
            self.mode = new_mode;
        }

        if let Some(frame) = seek_frame {
            self.seek_to_frame(frame);
        }

        if should_toggle_play {
            self.toggle_play();
        }

        if should_set_in {
            self.clip_start = Some(self.current_frame);
        }

        if should_set_out {
            self.clip_end = Some(self.current_frame);
            // Only auto-loop if we're in Normal mode (set via button/shift+O)
            if self.mode == Mode::Normal {
                self.looping_clip = true;
                if let Some(start) = self.clip_start {
                    self.seek_to_frame(start);
                }
            }
        }

        if speed_delta != 0.0 {
            self.set_speed(self.speed + speed_delta);
        }

        if should_toggle_mute {
            self.toggle_mute();
        }
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let video_path = args.get(1).cloned();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 280.0])
            .with_min_inner_size([600.0, 200.0])
            .with_position([960.0, 50.0])
            .with_title("zeditor"),
        ..Default::default()
    };

    eframe::run_native(
        "zeditor",
        options,
        Box::new(|_cc| Ok(Box::new(App::new(video_path)))),
    )
}
