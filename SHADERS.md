# CPU Shader System — Stateful Video Effects

## Problem

Real video corruption artifacts are temporal — they depend on relationships between frames over time. The current effect system is stateless: each effect receives a single `ColorImage` and returns a processed copy. This makes it impossible to simulate:

- Frozen macroblocks (regions stuck on old frame data)
- Motion smearing (motion vectors applied to stale references)
- Partial frame updates (CPU falling behind, only some blocks refresh)
- Datamoshing (motion from one scene applied to pixels of another)

## Architecture

### Frame History Buffer

The effect processor maintains a ring buffer of recent decoded frames.

```rust
struct EffectProcessor {
    /// Ring buffer of recent frames (newest at back)
    frame_history: VecDeque<ColorImage>,
    /// Max frames to retain
    max_history: usize, // 30 frames = ~1 second at 30fps

    /// Per-block state: which frame index each macroblock is currently showing
    /// Indexed as [block_y][block_x]
    block_frame_index: Vec<Vec<usize>>,

    /// The last composited output (what's "on screen")
    display_buffer: Option<ColorImage>,

    /// Monotonic frame counter
    frame_count: u64,

    /// Active corruption effect
    active_effect: CorruptionEffect,

    /// RNG state for deterministic but evolving corruption patterns
    rng_state: u64,
}
```

### Macroblock Grid

The frame is divided into a grid of macroblocks (16x16 pixels, matching H.264). Each block tracks its own state independently.

```rust
struct MacroblockState {
    /// Which frame from history this block is currently displaying
    source_frame_age: usize, // 0 = current, 1 = one frame old, etc.
    /// Pixel offset applied to this block (wrong motion vector)
    motion_offset: (i16, i16),
    /// Whether this block is "frozen" (not updating)
    frozen: bool,
    /// How many frames this block has been frozen
    frozen_duration: u32,
}
```

### Effect Signature

Replace the stateless `apply()` with a stateful `process_frame()`:

```rust
impl EffectProcessor {
    /// Feed a new decoded frame into the processor.
    /// Returns the composited output frame for display.
    fn process_frame(&mut self, new_frame: &ColorImage) -> ColorImage {
        // 1. Push new_frame into history, evict oldest if full
        // 2. Run active corruption effect against history + block state
        // 3. Composite output from per-block sources
        // 4. Update block states (freeze/unfreeze, drift motion vectors, etc.)
        // 5. Store result in display_buffer and return clone
    }
}
```

## Corruption Effects

### 1. Frozen Blocks (I-frame loss)

Simulates losing a keyframe — random rectangular regions stop updating and show stale frame data while the rest of the video plays normally.

**Behavior:**
- On each frame, some percentage of macroblocks "freeze" (stop pulling from current frame)
- Frozen blocks display pixels from their `source_frame_age` in the history buffer
- Freeze events cluster spatially (corruption tends to affect rectangular regions, not random scattered blocks)
- Frozen blocks eventually "recover" (snap to current frame) when a simulated keyframe arrives — this should happen periodically (every 1-3 seconds) and affect all frozen blocks at once
- At higher intensities, more blocks freeze and they stay frozen longer

**Parameters:**
- `freeze_probability: f32` — chance per frame that a non-frozen block freezes (0.0-1.0)
- `cluster_size: (usize, usize)` — min/max blocks per freeze cluster
- `recovery_interval_frames: u32` — how often a simulated keyframe clears all frozen blocks

### 2. Motion Smear (P-frame decode error)

Simulates motion vectors being applied to the wrong reference frame. Objects leave trails of themselves.

**Behavior:**
- Compute a rough per-block motion estimate by diffing current frame against previous frame (sum of absolute pixel differences per block)
- For blocks with high motion, apply the detected motion offset to pixels from an older frame instead of the current one
- This creates a "trail" effect: the object's shape from 2-5 frames ago gets shifted by the current motion vector
- Trails accumulate — the display buffer is not fully cleared, old smears persist and compound

**Parameters:**
- `trail_frames: usize` — how many frames back to source trail pixels from
- `motion_threshold: u32` — SAD threshold above which a block is considered "moving"
- `decay: f32` — how quickly trails fade (0.0 = permanent, 1.0 = instant)
- `blend_mode: BlendMode` — how trail pixels combine with current frame (overwrite, average, max)

### 3. Partial Update (bitstream starvation)

Simulates the display not getting a full frame in time — only some portion of the frame updates each tick.

**Behavior:**
- Each frame, only N% of macroblocks get updated with new pixel data
- The rest retain whatever was in the display buffer from the previous output
- Updates scan top-to-bottom, left-to-right (like a raster scan that got cut short)
- The "cutoff point" varies per frame — sometimes you get 90% of the frame, sometimes 20%
- Occasionally the update stalls completely for 2-3 frames (full freeze), then catches up with a jump

**Parameters:**
- `update_rate: f32` — average percentage of blocks updated per frame (0.0-1.0)
- `variance: f32` — how much the update rate fluctuates frame to frame
- `stall_probability: f32` — chance per frame of a complete stall
- `stall_duration_range: (u32, u32)` — min/max frames for a stall

### 4. Datamosh (I-frame removal)

Simulates removing keyframes so motion compensation applies to wrong pixel data. This is the "melting/liquid" glitch art look.

**Behavior:**
- Maintain a "reference frame" that does NOT update with new decoded frames (simulating a missing I-frame)
- Apply per-block motion vectors (estimated from frame diffs) to the stale reference frame's pixels
- The reference frame only updates on a simulated keyframe interval
- Between keyframes, motion keeps getting applied to increasingly stale data
- Result: shapes from the old scene "flow" according to motion in the new scene

**Parameters:**
- `keyframe_interval_frames: u32` — how often the reference frame updates
- `motion_scale: f32` — multiplier on detected motion vectors (>1.0 = exaggerated)
- `block_size: usize` — granularity of motion estimation (8, 16, or 32)

### 5. Corruption Cascade (combined)

Layer multiple effects for maximum destruction. This is the "everything is falling apart" mode.

**Behavior:**
- Start with partial updates (starvation)
- Frozen blocks accumulate over time
- Motion smear kicks in on moving regions
- Occasional full-frame recovery (keyframe) resets everything briefly before it starts degrading again
- Intensity ramps up over time, creating a progression from "slightly glitchy" to "unwatchable"

**Parameters:**
- `intensity: f32` — master intensity (0.0-1.0), scales all sub-effect parameters
- `ramp_speed: f32` — how quickly intensity increases over time (0.0 = static)

## Motion Estimation

All motion-dependent effects need a cheap per-block motion estimate. Full block matching is too expensive for real-time on Pi.

**Approach: Sum of Absolute Differences (SAD)**

```
For each 16x16 macroblock:
  1. Compare block at (bx, by) in current frame vs same position in previous frame
  2. SAD = sum of |current_pixel - previous_pixel| across all pixels and channels
  3. If SAD > threshold, block has motion
  4. Estimate direction by comparing SAD of block vs offset neighbors:
     - Compare (bx, by) in current vs (bx-1, by), (bx+1, by), (bx, by-1), (bx, by+1) in previous
     - Lowest SAD neighbor gives rough motion direction
```

This is O(pixels_per_block * 5) per block — ~5120 ops per 16x16 block, which is fast enough.

For even cheaper estimation, downsample to 4x4 per block before comparing.

## Integration with channel_surfer.rs

### Changes to ChannelSurfer struct

```rust
pub struct ChannelSurfer {
    // ... existing fields ...

    /// Stateful effect processor (replaces active_effect + last_processed_frame)
    effect_processor: EffectProcessor,
}
```

### Changes to frame display logic

Replace the current block in `update()` that calls `self.active_effect.apply(&original_frame)` with:

```rust
let frame_to_display = if self.effect_processor.is_active() {
    if let Some(original_frame) = player.get_latest_frame() {
        self.effect_processor.process_frame(&original_frame)
    } else {
        // No new frame — return current display buffer unchanged
        self.effect_processor.current_display().clone()
    }
} else {
    // No effect — passthrough
    // (render directly via player.render_frame)
};
```

### Effect cycling

Scroll wheel / S key cycles `effect_processor.active_effect` through the corruption types. The processor retains its frame history across effect changes (so switching effects mid-stream doesn't lose temporal state).

## Performance Budget (Raspberry Pi 4)

Target: 30fps at 720p (1280x720)

- Frame history: 30 frames * 1280*720*4 bytes = ~106 MB — too much. Cap at 10 frames (~35 MB) or store at half resolution.
- Macroblock grid at 16px: 80x45 = 3600 blocks. Per-block operations are cheap.
- SAD motion estimation: 3600 blocks * 5120 ops = ~18M ops. At ~2 GFLOPS on Pi 4 ARM, this is ~9ms. Acceptable but tight. Use 4x4 downsampled SAD (~1.2ms) for real-time.
- Block compositing: memcpy per block from correct history frame. ~3600 * 256 bytes = ~900 KB of copies. Fast.

**Optimization levers if too slow:**
- Larger macroblocks (32x32 instead of 16x16) — 4x fewer blocks
- Skip motion estimation on frozen blocks
- Only compute SAD on blocks that changed (early-out if first row matches)
- Process at half resolution, upscale for display
- Skip frames — process every 2nd frame, reuse display buffer between

## File Layout

```
src/
├── shader.rs              # Keep existing stateless effects (pixelate, sepia, etc.)
├── corruption.rs          # NEW: EffectProcessor, frame history, corruption effects
└── channel_surfer.rs      # Wire EffectProcessor into update loop
```

Stateless effects in `shader.rs` stay as-is. They still work for simple color grading. The new `corruption.rs` handles temporal/stateful effects only.
