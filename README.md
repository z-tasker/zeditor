# zeditor

A video editor built with egui and egui-video for single-window video playback.

## Features

- Single-window video playback (no separate mpv window)
- Frame-by-frame navigation
- Clip marking and export
- Keyboard shortcuts for efficient editing
- Drag & drop video files

## Installation

### Dependencies (macOS)

```bash
brew install ffmpeg@7 sdl2
```

### Build

```bash
make build    # Build release binary
make run      # Build and run with test-video.mp4
make clean    # Clean build artifacts
```

## Usage

- Drag & drop a video file or pass as argument
- **Normal mode**: `i` to enter Insert mode, Space to play/pause
- **Insert mode**: `h/l` for frame navigation, `i/o` for clip marks
- Export clips with IN/OUT marks set

## Controls

- `Space` / `l`: Play/pause
- `h/l` or `←/→`: Frame back/forward (Insert mode)
- `w/b` or `↑/↓`: Chunk navigation (Insert mode)
- `i`: Enter Insert mode / Set IN mark
- `o`: Set OUT mark
- `a`: Toggle mute
- `+/-`: Adjust speed

## Architecture

Migrated from mpv subprocess to egui-video for integrated video rendering.