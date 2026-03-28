# zeditor

A video editor built with egui and egui-video for single-window video playback.

## Features

- Single-window video playback
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
