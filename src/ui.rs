use crate::{App, Mode};
use eframe::egui;

impl App {
    pub fn render_top_panel(&mut self, ctx: &egui::Context) {
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
                ui.colored_label(
                    mode_color,
                    egui::RichText::new(mode_text).strong().size(16.0),
                );
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
    }

    pub fn render_bottom_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            // Status bar - always reserve space, only show text when present
            ui.horizontal(|ui| {
                if let Some(status) = &self.export_status {
                    ui.colored_label(egui::Color32::GREEN, format!(">> {}", status));
                } else {
                    // Reserve vertical space with invisible text
                    ui.label(" ");
                }
            });
            ui.separator();

            // Timeline/scrubber
            if let (Some(total), Some(fps)) = (self.total_frames, self.fps) {
                let mut frame = self.current_frame as f64;

                ui.horizontal(|ui| {
                    ui.label(format!("Frame: {} / {}", self.current_frame, total));
                    ui.separator();
                    ui.label(format!(
                        "Time: {:.2}s / {:.2}s",
                        self.current_frame as f64 / fps,
                        total as f64 / fps
                    ));
                    ui.separator();
                    ui.label(format!("Speed: {:.1}x", self.speed));
                    ui.separator();
                    let mute_text = if self.muted { "MUTED" } else { "Audio ON" };
                    let mute_color = if self.muted {
                        egui::Color32::GRAY
                    } else {
                        egui::Color32::LIGHT_GREEN
                    };
                    ui.colored_label(mute_color, mute_text);
                });

                // Full-width slider
                let slider = egui::Slider::new(&mut frame, 0.0..=(total as f64))
                    .show_value(false)
                    .trailing_fill(true);

                if ui.add_sized([ui.available_width(), 20.0], slider).changed() {
                    self.seek_to_frame(frame as u64);
                }

                // Clip markers
                ui.horizontal(|ui| {
                    if let Some(start) = self.clip_start {
                        ui.colored_label(
                            egui::Color32::GREEN,
                            format!("IN: {} ({:.2}s)", start, start as f64 / fps),
                        );
                    }
                    if let Some(end) = self.clip_end {
                        ui.colored_label(
                            egui::Color32::RED,
                            format!("OUT: {} ({:.2}s)", end, end as f64 / fps),
                        );
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
                if ui
                    .button(if self.playing { "|| Pause" } else { "|> Play" })
                    .clicked()
                {
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

                let in_color = if self.clip_start.is_some() {
                    egui::Color32::GREEN
                } else {
                    egui::Color32::GRAY
                };
                if ui
                    .button(egui::RichText::new("[I]n").color(in_color))
                    .clicked()
                {
                    self.clip_start = Some(self.current_frame);
                }

                let out_color = if self.clip_end.is_some() {
                    egui::Color32::RED
                } else {
                    egui::Color32::GRAY
                };
                if ui
                    .button(egui::RichText::new("[O]ut").color(out_color))
                    .clicked()
                {
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

                let loop_text = if self.looping_clip {
                    "Loop ON"
                } else {
                    "Loop OFF"
                };
                if ui.button(loop_text).clicked() {
                    self.looping_clip = !self.looping_clip;
                }
            });

            ui.separator();

            // Clip name and export
            ui.horizontal(|ui| {
                ui.label("Clip name:");
                let clip_name_id = egui::Id::new("clip_name_input");
                let response =
                    ui.add(egui::TextEdit::singleline(&mut self.clip_name).id(clip_name_id));

                // Focus the clip name input when naming_clip is set
                if self.naming_clip {
                    response.request_focus();
                    self.naming_clip = false;
                }

                let can_export = self.clip_start.is_some()
                    && self.clip_end.is_some()
                    && !self.clip_name.is_empty();

                if ui
                    .add_enabled(can_export, egui::Button::new("Export"))
                    .clicked()
                {
                    match self.export_clip() {
                        Ok(filename) => {
                            self.export_status = Some(format!("Saved: {}", filename));
                            self.status_time = Some(std::time::Instant::now());
                            self.clip_start = None;
                            self.clip_end = None;
                            self.clip_name.clear();
                            self.looping_clip = false;
                        }
                        Err(e) => {
                            self.export_status = Some(format!("Export failed: {}", e));
                            self.status_time = Some(std::time::Instant::now());
                        }
                    }
                }

                // Enter in clip name input triggers export
                if response.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && can_export
                {
                    match self.export_clip() {
                        Ok(filename) => {
                            self.export_status = Some(format!("Saved: {}", filename));
                            self.status_time = Some(std::time::Instant::now());
                            self.clip_start = None;
                            self.clip_end = None;
                            self.clip_name.clear();
                            self.looping_clip = false;
                            self.mode = Mode::Normal;
                        }
                        Err(e) => {
                            self.export_status = Some(format!("Export failed: {}", e));
                            self.status_time = Some(std::time::Instant::now());
                        }
                    }
                }
            });

            ui.separator();

            // Help
            ui.horizontal(|ui| {
                let help = match self.mode {
                    Mode::Normal => {
                        "[i] Insert mode | [Space] Play/Pause | [w/b] Speed | [Shift+I/O] Set marks"
                    }
                    Mode::Insert => {
                        "[Esc/Enter] Exit | [h/l] Frame | [w/b] Chunk | [i] IN | [o] OUT"
                    }
                };
                ui.label(help);
            });
        });
    }

    pub fn render_central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // Black background
            ui.painter().rect_filled(
                egui::Rect::from_min_size(ui.cursor().min, ui.available_size()),
                0.0,
                egui::Color32::BLACK,
            );

            if let Some(ref mut player) = self.player {
                // Get available space
                let available = ui.available_size();
                let video_size = player.size;
                let aspect = video_size.x / video_size.y;

                // Calculate size that fits available space while maintaining aspect ratio
                let size = if available.x / available.y > aspect {
                    // Available space is wider than video aspect ratio
                    egui::vec2(available.y * aspect, available.y)
                } else {
                    // Available space is taller than video aspect ratio
                    egui::vec2(available.x, available.x / aspect)
                };

                // Center video in available space
                ui.centered_and_justified(|ui| {
                    // Use render_frame to show just the video frame (no egui-video controls)
                    // We call process_state to update the player
                    player.process_state();
                    player.render_frame(ui, size);
                });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.colored_label(
                        egui::Color32::WHITE,
                        "Drag and drop a video file\nor pass path as command line argument",
                    );
                });
            }
        });
    }
}
