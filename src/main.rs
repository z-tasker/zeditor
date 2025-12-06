use std::io;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::net::UnixStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::fs;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    Insert,
}

struct App {
    video_path: Option<String>,
    total_frames: Option<u64>,
    current_frame: u64,
    fps: Option<f64>,
    playing: bool,
    direction: i32, // 1 forward, -1 backward
    speed: f64, // 1.0 or 2.0
    mode: Mode,
    chunk_frames: u64,
    clip_start: Option<u64>,
    clip_end: Option<u64>,
    clip_name: Option<String>,
    mpv_stream: Option<UnixStream>,
}

impl App {
    fn new() -> Self {
        Self {
            video_path: None,
            total_frames: None,
            current_frame: 0,
            fps: None,
            playing: false,
            direction: 1,
            speed: 1.0,
            mode: Mode::Normal,
            chunk_frames: 30,
            clip_start: None,
            clip_end: None,
            clip_name: None,
            mpv_stream: None,
        }
    }

    async fn send_command(&mut self, command: serde_json::Value) {
        if let Some(stream) = &mut self.mpv_stream {
            let msg = format!("{}\n", command.to_string());
            let _ = stream.write_all(msg.as_bytes()).await;
        }
    }

    async fn get_time(&mut self) -> Option<f64> {
        if let Some(stream) = &mut self.mpv_stream {
            let cmd = serde_json::json!({"command": ["get_property", "time-pos"]});
            let msg = format!("{}\n", cmd.to_string());
            let _ = stream.write_all(msg.as_bytes()).await;

            // Read response
            let mut buf = [0; 1024];
            if let Ok(n) = stream.read(&mut buf).await {
                if n > 0 {
                    if let Ok(response) = serde_json::from_slice::<serde_json::Value>(&buf[..n]) {
                        if let Some(data) = response.get("data") {
                            return data.as_f64();
                        }
                    }
                }
            }
        }
        None
    }

    async fn update(&mut self, _delta_ms: u64) {
        // Get current time from mpv
        if let Some(time) = self.get_time().await {
            if let Some(fps) = self.fps {
                self.current_frame = (time * fps) as u64;
            }
        }
    }

    async fn export_clip(&self) -> anyhow::Result<()> {
        if let (Some(start), Some(end), Some(name), Some(video_path), Some(fps)) =
            (self.clip_start, self.clip_end, &self.clip_name, &self.video_path, self.fps) {
            let start_time = start as f64 / fps;
            let duration = (end - start) as f64 / fps;

            let output = tokio::process::Command::new("ffmpeg")
                .args(&[
                    "-i", video_path,
                    "-ss", &format!("{:.3}", start_time),
                    "-t", &format!("{:.3}", duration),
                    "-c", "copy",
                    name,
                ])
                .output()
                .await?;

            if output.status.success() {
                Ok(())
            } else {
                Err(anyhow::anyhow!("ffmpeg failed"))
            }
        } else {
            Err(anyhow::anyhow!("clip not fully set"))
        }
    }

    async fn load_video(&mut self, path: String) -> anyhow::Result<()> {
        // Ensure ffmpeg is available
        ffmpeg_sidecar::download::auto_download()?;

        // Run ffprobe to get video info
        let output = tokio::process::Command::new("ffprobe")
            .args(&[
                "-v", "quiet",
                "-print_format", "json",
                "-show_format",
                "-show_streams",
                &path,
            ])
            .output()
            .await?;

        let json: Value = serde_json::from_slice(&output.stdout)?;
        let streams = json["streams"].as_array().unwrap();
        let video_stream = streams.iter().find(|s| s["codec_type"] == "video").unwrap();

        let duration: f64 = json["format"]["duration"].as_str().unwrap().parse()?;
        let fps_str = video_stream["r_frame_rate"].as_str().unwrap();
        let fps_parts: Vec<&str> = fps_str.split('/').collect();
        let fps: f64 = fps_parts[0].parse::<f64>()? / fps_parts[1].parse::<f64>()?;

        self.video_path = Some(path.clone());
        self.fps = Some(fps);
        self.total_frames = Some((duration * fps) as u64);
        self.current_frame = 0;

        // Spawn mpv
        let sock_path = "/tmp/zeditor_mpv.sock";
        let _ = fs::remove_file(sock_path); // remove if exists
        let child = tokio::process::Command::new("mpv")
            .args(&[
                "--really-quiet",
                "--no-terminal",
                "--ontop", // always on top
                "--input-ipc-server=/tmp/zeditor_mpv.sock",
                &path,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        // Wait for socket
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Connect to socket
        let stream = UnixStream::connect(sock_path).await?;
        self.mpv_stream = Some(stream);

        Ok(())
    }

    fn draw(&self, f: &mut ratatui::Frame) {
        let size = f.size();

        // Split vertically: preview (main) and control bar (bottom)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(10), // preview area
                Constraint::Length(5), // control bar
            ])
            .split(size);

        // Preview window
        let mode_str = match self.mode {
            Mode::Normal => "Normal",
            Mode::Insert => "Insert",
        };
        let preview_text = if let Some(path) = &self.video_path {
            format!("Mode: {}\nVideo: {}\nFrame: {} / {}\nFPS: {:.2}\nPlaying: {} Dir: {} Speed: {:.1}\n\n[Video playing in separate window]",
                    mode_str,
                    path,
                    self.current_frame,
                    self.total_frames.unwrap_or(0),
                    self.fps.unwrap_or(0.0),
                    self.playing,
                    if self.direction == 1 { "fwd" } else { "bwd" },
                    self.speed)
        } else {
            format!("Mode: {}\nNo video loaded. Provide path as arg.", mode_str)
        };
        let preview = Paragraph::new(preview_text)
            .block(Block::default().title("Preview").borders(Borders::ALL));
        f.render_widget(preview, chunks[0]);

        // Control bar
        let control_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25), // clip start thumb
                Constraint::Percentage(25), // clip end thumb
                Constraint::Percentage(50), // other controls
            ])
            .split(chunks[1]);

        let start_text = if let Some(frame) = self.clip_start {
            format!("Frame\n{}", frame)
        } else {
            "Not set".to_string()
        };
        let start_thumb = Paragraph::new(start_text)
            .block(Block::default().title("Clip Start").borders(Borders::ALL))
            .style(Style::default().fg(Color::Green));
        f.render_widget(start_thumb, control_chunks[0]);

        let end_text = if let Some(frame) = self.clip_end {
            format!("Frame\n{}", frame)
        } else {
            "Not set".to_string()
        };
        let end_thumb = Paragraph::new(end_text)
            .block(Block::default().title("Clip End").borders(Borders::ALL))
            .style(Style::default().fg(Color::Red));
        f.render_widget(end_thumb, control_chunks[1]);

        let controls = Paragraph::new("Controls here")
            .block(Block::default().title("Controls").borders(Borders::ALL));
        f.render_widget(controls, control_chunks[2]);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let video_path = args.get(1).cloned();
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    if let Some(path) = video_path {
        app.load_video(path).await?;
    }

    // run app
    let res = run_app(&mut terminal, &mut app).await;

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> io::Result<()> {
    let mut last_update = Instant::now();
    let _last_frame = app.current_frame;
    loop {


        terminal.draw(|f| app.draw(f))?;

        let timeout = Duration::from_millis(100);
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(());
                }
                match app.mode {
                    Mode::Normal => match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char(' ') => {
                            if app.clip_start.is_some() && app.clip_end.is_none() {
                                app.current_frame = app.clip_start.unwrap();
                            }
                            app.playing = !app.playing;
                            // Send pause/play to mpv
                            let cmd = serde_json::json!({"command": ["cycle", "pause"]});
                            app.send_command(cmd).await;
                        }
                        KeyCode::Char('h') => {
                            app.direction = -1;
                            app.playing = true;
                        }
                        KeyCode::Char('l') => {
                            app.direction = 1;
                            app.playing = true;
                        }
                        KeyCode::Char('w') => {
                            app.speed = (app.speed + 0.5).min(3.0);
                            let cmd = serde_json::json!({"command": ["set_property", "speed", app.speed]});
                            app.send_command(cmd).await;
                        }
                        KeyCode::Char('b') => {
                            app.speed = (app.speed - 0.5).max(0.5);
                            let cmd = serde_json::json!({"command": ["set_property", "speed", app.speed]});
                            app.send_command(cmd).await;
                        }
                        KeyCode::Char('i') => {
                            app.mode = Mode::Insert;
                            app.playing = false;
                            // Pause video
                            let cmd = serde_json::json!({"command": ["cycle", "pause"]});
                            app.send_command(cmd).await;
                        }
                        KeyCode::Enter => {
                            if app.clip_start.is_some() && app.clip_end.is_some() {
                                app.clip_name = Some("clip.mp4".to_string());
                                if let Err(_e) = app.export_clip().await {
                                }
                            }
                        }
                        _ => {}
                    },
                    Mode::Insert => match key.code {
                        KeyCode::Esc => {
                            app.mode = Mode::Normal;
                        }
                        KeyCode::Enter => {
                            if app.clip_start.is_none() {
                                app.clip_start = Some(app.current_frame);
                            } else if app.clip_end.is_none() {
                                app.clip_end = Some(app.current_frame);
                            }
                        }
                        KeyCode::Char('h') => {
                            app.current_frame = app.current_frame.saturating_sub(1);
                            let time = app.current_frame as f64 / app.fps.unwrap_or(1.0);
                            let cmd = serde_json::json!({"command": ["seek", time, "absolute"]});
                            app.send_command(cmd).await;
                        }
                        KeyCode::Char('l') => {
                            app.current_frame = app.current_frame.saturating_add(1)
                                .min(app.total_frames.unwrap_or(0).saturating_sub(1));
                            let time = app.current_frame as f64 / app.fps.unwrap_or(1.0);
                            let cmd = serde_json::json!({"command": ["seek", time, "absolute"]});
                            app.send_command(cmd).await;
                        }
                        KeyCode::Char('b') => {
                            app.current_frame = app.current_frame.saturating_sub(app.chunk_frames);
                            let time = app.current_frame as f64 / app.fps.unwrap_or(1.0);
                            let cmd = serde_json::json!({"command": ["seek", time, "absolute"]});
                            app.send_command(cmd).await;
                        }
                        KeyCode::Char('w') => {
                            app.current_frame = app.current_frame.saturating_add(app.chunk_frames)
                                .min(app.total_frames.unwrap_or(0).saturating_sub(1));
                            let time = app.current_frame as f64 / app.fps.unwrap_or(1.0);
                            let cmd = serde_json::json!({"command": ["seek", time, "absolute"]});
                            app.send_command(cmd).await;
                        }
                        _ => {}
                    },
                }
            }
        }

        let now = Instant::now();
        let delta = now.duration_since(last_update).as_millis() as u64;
        app.update(delta).await;
        last_update = now;
    }
}
