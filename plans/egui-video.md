# Video Integration Plan: Migrate to egui-video

## Design Overview

Currently, zeditor spawns mpv as a subprocess in its own window and communicates via IPC over a Unix socket. This creates a two-window experience. We'll replace this with `egui-video`, which decodes video using ffmpeg and renders frames directly into the egui UI as textures.

### Architecture Change

```
BEFORE:
┌─────────────────┐     IPC/Socket      ┌─────────────────┐
│   egui window   │ ◄─────────────────► │   mpv window    │
│   (controls)    │                     │   (video)       │
└─────────────────┘                     └─────────────────┘

AFTER:
┌─────────────────────────────────────────┐
│              egui window                │
│  ┌───────────────────────────────────┐  │
│  │     egui-video Player widget      │  │
│  │     (video renders here)          │  │
│  └───────────────────────────────────┘  │
│  ┌───────────────────────────────────┐  │
│  │          controls panel           │  │
│  └───────────────────────────────────┘  │
└─────────────────────────────────────────┘
```

### Key Benefits
- Single window application
- No subprocess management
- Direct frame access for future features (thumbnails, frame export)
- Simpler codebase

---

## Implementation Tasks

### 1. Update Dependencies

**Cargo.toml changes:**
- Add `egui-video` with audio feature
- Keep `ffmpeg-sidecar` for export functionality

```toml
[dependencies]
egui-video = { version = "0.5", features = ["audio"] }
```

**System requirement:** ffmpeg 6 or 7 libraries must be installed. On macOS:
```bash
brew install ffmpeg
```

### 2. Remove MpvHandle Infrastructure

Delete the following from `src/main.rs`:
- `MpvHandle` struct (lines 14-33)
- `spawn_mpv()` method (lines 118-153)
- `send_mpv_command()` method (lines 161-165)
- `poll_mpv()` method (lines 167-189)
- All mpv-related JSON command construction

### 3. Add egui-video Player

Replace `mpv: Option<MpvHandle>` in the `App` struct with:
```rust
player: Option<egui_video::Player>,
audio_device: Option<egui_video::AudioDevice>,
```

Initialize audio device once at startup (it must persist for the app lifetime).

### 4. Adapt Video Loading

Replace `spawn_mpv()` with player creation:
```rust
fn load_video(&mut self, ctx: &egui::Context, path: &str) -> anyhow::Result<()> {
    let mut player = egui_video::Player::new(ctx, path)?;
    if let Some(audio) = &mut self.audio_device {
        player = player.with_audio(audio);
    }
    self.player = Some(player);
    // ... extract metadata (fps, duration, etc.)
    Ok(())
}
```

### 5. Map Control Operations

| Current (mpv IPC)                          | New (egui-video)                    |
|--------------------------------------------|-------------------------------------|
| `{"command": ["seek", time, "absolute"]}`  | `player.seek(time)`                 |
| `{"command": ["cycle", "pause"]}`          | `player.start()` / `player.stop()`  |
| `{"command": ["set_property", "speed", x]}`| `player.set_speed(x)` (if available, else manual) |
| `{"command": ["set_property", "mute", b]}` | `player.set_volume(0.0)` or similar |
| `{"command": ["get_property", "time-pos"]}` | `player.position` (direct field access) |

Note: Check `egui-video` API for exact method names. Some features like speed control may need to be implemented via seeking or may not be directly available.

### 6. Update UI Rendering

In the `CentralPanel`, replace the placeholder text with the video widget:
```rust
egui::CentralPanel::default().show(ctx, |ui| {
    if let Some(player) = &mut self.player {
        let size = [player.width as f32, player.height as f32];
        player.ui(ui, size);
    } else {
        ui.centered_and_justified(|ui| {
            ui.label("Drag and drop a video file");
        });
    }
});
```

### 7. Adjust Window Size

The current window is small (800x280) because video was in a separate window. Resize to accommodate video:
```rust
let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default()
        .with_inner_size([1200.0, 800.0])  // Larger to fit video
        .with_min_inner_size([800.0, 600.0])
        .with_title("zeditor"),
    ..Default::default()
};
```

### 8. Handle Frame Position Tracking

egui-video provides position info. Map this to your frame-based UI:
```rust
fn update_state(&mut self) {
    if let Some(player) = &self.player {
        if let Some(fps) = self.fps {
            self.current_frame = (player.position * fps) as u64;
        }
    }
    // ... rest of state update
}
```

### 9. Clip Looping Logic

The current `looping_clip` logic seeks back to start when reaching the end frame. This should work similarly with egui-video's seek capability.

---

## Potential Challenges

1. **Speed control**: mpv has native speed control. egui-video may not expose this directly. Workaround: adjust seek intervals or check if the underlying ffmpeg bindings support it.

2. **Frame-accurate seeking**: egui-video uses time-based seeking. Your frame-based UI should continue to work by converting frame numbers to timestamps using fps.

3. **Build complexity**: rust-ffmpeg requires ffmpeg development libraries. The README suggests following their [build instructions](https://github.com/zmwangx/rust-ffmpeg/wiki/Notes-on-building).

4. **Performance**: The egui-video README notes that release mode (`--release` or `opt-level=3`) is required for smooth playback.

---

## Testing Plan

1. Build with `cargo build --release`
2. Test video loading (drag-drop and CLI argument)
3. Test playback controls (play/pause/seek)
4. Test frame-by-frame navigation in Insert mode
5. Test clip marking and looping
6. Test audio (mute toggle)
7. Test export (should work unchanged - uses ffmpeg CLI)

---

## Migration Order

1. Add dependencies, verify build
2. Initialize audio device in main()
3. Replace video loading
4. Replace playback controls one-by-one
5. Update UI to show video in CentralPanel
6. Remove all mpv code
7. Adjust window sizing
8. Test thoroughly
