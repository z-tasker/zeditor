# zeditor Makefile
# Requires: brew install ffmpeg@7 sdl2 pkgconf

FFMPEG_DIR := /opt/homebrew/opt/ffmpeg@7
PKG_CONFIG_PATH := /opt/homebrew/opt/ffmpeg@7/lib/pkgconfig:/opt/homebrew/opt/sdl2/lib/pkgconfig
LIBRARY_PATH := /opt/homebrew/opt/sdl2/lib

export FFMPEG_DIR
export PKG_CONFIG_PATH
export LIBRARY_PATH

.PHONY: build run clean

build:
	cargo build --release

run: build
	cargo run --release -- test-video.mp4

clean:
	cargo clean
