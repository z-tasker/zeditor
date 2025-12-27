#!/bin/bash
set -e

# zeditor test script
# Requires: brew install ffmpeg@7 sdl2

export PKG_CONFIG_PATH="/opt/homebrew/opt/ffmpeg@7/lib/pkgconfig:$PKG_CONFIG_PATH"
export LIBRARY_PATH="/opt/homebrew/opt/sdl2/lib:$LIBRARY_PATH"

cargo run --release -- test-video.mp4
