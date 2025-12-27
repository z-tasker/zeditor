use eframe::egui;

use crate::{App, Mode};

/// Actions that can be triggered by keyboard input
#[derive(Default)]
pub struct ControlActions {
    pub mode_change: Option<Mode>,
    pub seek_frame: Option<u64>,
    pub toggle_play: bool,
    pub set_in: bool,
    pub set_out: bool,
    pub speed_delta: f64,
    pub toggle_mute: bool,
    pub start_naming_clip: bool,
}

impl App {
    /// Process keyboard input and return actions to apply
    pub fn handle_keyboard(&mut self, ctx: &egui::Context) {
        let actions = self.collect_keyboard_actions(ctx);
        self.apply_actions(actions);
    }

    fn collect_keyboard_actions(&self, ctx: &egui::Context) -> ControlActions {
        let mut actions = ControlActions::default();

        // Check if text input has focus
        let text_focused = ctx.memory(|m| m.focused().is_some());

        ctx.input(|i| {
            // In normal mode, skip letter keys if text field is focused
            let skip_letters = text_focused && self.mode == Mode::Normal;

            // 'a' toggles mute in any mode (unless typing)
            if i.key_pressed(egui::Key::A) && !skip_letters {
                actions.toggle_mute = true;
            }

            match self.mode {
                Mode::Normal => {
                    self.collect_normal_mode_actions(i, skip_letters, &mut actions);
                }
                Mode::Insert => {
                    self.collect_insert_mode_actions(i, &mut actions);
                }
            }
        });

        actions
    }

    fn collect_normal_mode_actions(
        &self,
        i: &egui::InputState,
        skip_letters: bool,
        actions: &mut ControlActions,
    ) {
        if !skip_letters {
            // i enters insert mode (without shift)
            if i.key_pressed(egui::Key::I) && !i.modifiers.shift {
                actions.mode_change = Some(Mode::Insert);
            }
            // Shift+I sets IN mark
            if i.key_pressed(egui::Key::I) && i.modifiers.shift {
                actions.set_in = true;
            }
            // Shift+O sets OUT mark
            if i.key_pressed(egui::Key::O) && i.modifiers.shift {
                actions.set_out = true;
            }
            // l toggles play
            if i.key_pressed(egui::Key::L) {
                actions.toggle_play = true;
            }
            // w increases speed
            if i.key_pressed(egui::Key::W) {
                actions.speed_delta = 0.5;
            }
            // b decreases speed
            if i.key_pressed(egui::Key::B) {
                actions.speed_delta = -0.5;
            }
        }

        // These work even when text field is focused
        if i.key_pressed(egui::Key::Space) {
            actions.toggle_play = true;
        }
        if i.key_pressed(egui::Key::CloseBracket) {
            actions.speed_delta = 0.5;
        }
        if i.key_pressed(egui::Key::OpenBracket) {
            actions.speed_delta = -0.5;
        }
    }

    fn collect_insert_mode_actions(&self, i: &egui::InputState, actions: &mut ControlActions) {
        // Escape exits to normal mode
        if i.key_pressed(egui::Key::Escape) {
            actions.mode_change = Some(Mode::Normal);
        }

        // h/left: frame back
        if i.key_pressed(egui::Key::H) || i.key_pressed(egui::Key::ArrowLeft) {
            actions.seek_frame = Some(self.current_frame.saturating_sub(1));
        }

        // l/right: frame forward
        if i.key_pressed(egui::Key::L) || i.key_pressed(egui::Key::ArrowRight) {
            let max = self.total_frames.unwrap_or(u64::MAX).saturating_sub(1);
            actions.seek_frame = Some(self.current_frame.saturating_add(1).min(max));
        }

        // b/up: chunk back
        if i.key_pressed(egui::Key::B) || i.key_pressed(egui::Key::ArrowUp) {
            actions.seek_frame = Some(self.current_frame.saturating_sub(self.chunk_frames));
        }

        // w/down: chunk forward
        if i.key_pressed(egui::Key::W) || i.key_pressed(egui::Key::ArrowDown) {
            let max = self.total_frames.unwrap_or(u64::MAX).saturating_sub(1);
            actions.seek_frame = Some(self.current_frame.saturating_add(self.chunk_frames).min(max));
        }

        // i sets IN point (must be <= OUT if OUT exists)
        if i.key_pressed(egui::Key::I) {
            let valid = self.clip_end.map_or(true, |end| self.current_frame <= end);
            if valid {
                actions.set_in = true;
            }
        }

        // o sets OUT point (must be >= IN if IN exists)
        if i.key_pressed(egui::Key::O) {
            let valid = self.clip_start.map_or(true, |start| self.current_frame >= start);
            if valid {
                actions.set_out = true;
            }
        }

        // Enter: if IN and OUT are set, start naming clip; otherwise exit insert mode
        if i.key_pressed(egui::Key::Enter) {
            if self.clip_start.is_some() && self.clip_end.is_some() {
                actions.start_naming_clip = true;
            } else {
                actions.mode_change = Some(Mode::Normal);
            }
        }
    }

    fn apply_actions(&mut self, actions: ControlActions) {
        // Mode changes
        if let Some(new_mode) = actions.mode_change {
            self.apply_mode_change(new_mode);
        }

        // Seeking
        if let Some(frame) = actions.seek_frame {
            self.seek_to_frame(frame);
        }

        // Play/pause toggle
        if actions.toggle_play {
            self.toggle_play();
        }

        // Set IN mark
        if actions.set_in {
            self.clip_start = Some(self.current_frame);
        }

        // Set OUT mark
        if actions.set_out {
            self.clip_end = Some(self.current_frame);
            // Auto-loop only in Normal mode
            if self.mode == Mode::Normal {
                self.looping_clip = true;
                if let Some(start) = self.clip_start {
                    self.seek_to_frame(start);
                }
            }
        }

        // Speed adjustment
        if actions.speed_delta != 0.0 {
            self.set_speed(self.speed + actions.speed_delta);
        }

        // Mute toggle
        if actions.toggle_mute {
            self.toggle_mute();
        }

        // Start naming clip (for export flow)
        if actions.start_naming_clip {
            self.naming_clip = true;
        }
    }

    fn apply_mode_change(&mut self, new_mode: Mode) {
        if new_mode == Mode::Insert {
            // Entering insert mode - pause
            self.pause_player();
        } else if new_mode == Mode::Normal && self.mode == Mode::Insert {
            // Exiting insert mode - resume playback
            self.resume_player();
        }
        self.mode = new_mode;
    }
}
