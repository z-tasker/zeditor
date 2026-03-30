//! CPU-based video effects for post-processing
//!
//! This module provides efficient pixel manipulation effects that work
//! on Raspberry Pi and other devices without GPU acceleration.

use egui::ColorImage;

/// Types of CPU video effects
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum VideoEffect {
    /// No effect - passthrough
    #[default]
    None,
    /// Heavy pixelation (blocky compression artifacts)
    Pixelate { block_size: usize },
    /// Sepia tone (old film look)
    Sepia,
    /// RGB split / chromatic aberration
    RgbSplit { offset: usize },
    /// Color inversion
    Invert,
    /// High contrast
    Contrast { factor: f32 },
    /// H.264/MPEG compression artifacts (DCT blocking, chroma subsampling)
    Compression { quality: u8 },
    /// Decoder corruption - frozen macroblocks, static bursts
    Glitch {
        /// Intensity 1-10
        intensity: u8,
        /// Seed for random corruption patterns
        seed: u32,
    },
    /// Motion trail with corruption (like P-frame decode errors)
    MotionGlitch { trail_length: u8 },
    /// Datamoshing - motion vectors applied wrong (block displacement)
    Datamosh { displacement: i8 },
}

impl VideoEffect {
    /// Get display name
    pub fn name(&self) -> &'static str {
        match self {
            VideoEffect::None => "NONE",
            VideoEffect::Pixelate { .. } => "PIXELATE",
            VideoEffect::Sepia => "SEPIA",
            VideoEffect::RgbSplit { .. } => "RGB SPLIT",
            VideoEffect::Invert => "INVERT",
            VideoEffect::Contrast { .. } => "CONTRAST",
            VideoEffect::Compression { .. } => "H264",
            VideoEffect::Glitch { .. } => "GLITCH",
            VideoEffect::MotionGlitch { .. } => "MOTION GLITCH",
            VideoEffect::Datamosh { .. } => "DATAMOSH",
        }
    }

    /// Apply the effect to a ColorImage
    pub fn apply(&self, input: &ColorImage) -> ColorImage {
        match self {
            VideoEffect::None => input.clone(),
            VideoEffect::Pixelate { block_size } => apply_pixelate(input, *block_size),
            VideoEffect::Sepia => apply_sepia(input),
            VideoEffect::RgbSplit { offset } => apply_rgb_split(input, *offset),
            VideoEffect::Invert => apply_invert(input),
            VideoEffect::Contrast { factor } => apply_contrast(input, *factor),
            VideoEffect::Compression { quality } => apply_compression(input, *quality),
            VideoEffect::Glitch { intensity, seed } => apply_glitch(input, *intensity, *seed),
            VideoEffect::MotionGlitch { trail_length } => apply_motion_glitch(input, *trail_length),
            VideoEffect::Datamosh { displacement } => apply_datamosh(input, *displacement),
        }
    }

    /// Cycle to next effect
    pub fn next(&self) -> VideoEffect {
        match self {
            VideoEffect::None => VideoEffect::Pixelate { block_size: 16 },
            VideoEffect::Pixelate { block_size } if *block_size < 32 => VideoEffect::Pixelate {
                block_size: block_size * 2,
            },
            VideoEffect::Pixelate { .. } => VideoEffect::Sepia,
            VideoEffect::Sepia => VideoEffect::RgbSplit { offset: 5 },
            VideoEffect::RgbSplit { offset } if *offset < 15 => {
                VideoEffect::RgbSplit { offset: offset + 5 }
            }
            VideoEffect::RgbSplit { .. } => VideoEffect::Invert,
            VideoEffect::Invert => VideoEffect::Contrast { factor: 1.5 },
            VideoEffect::Contrast { factor } if *factor < 2.5 => VideoEffect::Contrast {
                factor: factor + 0.5,
            },
            VideoEffect::Contrast { .. } => VideoEffect::Compression { quality: 20 },
            VideoEffect::Compression { quality } if *quality > 5 => VideoEffect::Compression {
                quality: quality / 2,
            },
            VideoEffect::Compression { .. } => VideoEffect::Glitch {
                intensity: 3,
                seed: 0,
            },
            VideoEffect::Glitch { intensity, .. } if *intensity < 8 => VideoEffect::Glitch {
                intensity: intensity + 2,
                seed: 12345,
            },
            VideoEffect::Glitch { .. } => VideoEffect::MotionGlitch { trail_length: 5 },
            VideoEffect::MotionGlitch { trail_length } if *trail_length < 15 => {
                VideoEffect::MotionGlitch {
                    trail_length: trail_length + 5,
                }
            }
            VideoEffect::MotionGlitch { .. } => VideoEffect::Datamosh { displacement: 8 },
            VideoEffect::Datamosh { displacement } if *displacement > 0 => VideoEffect::Datamosh {
                displacement: -displacement,
            },
            VideoEffect::Datamosh { .. } => VideoEffect::None,
        }
    }
}

/// Apply pixelation effect
fn apply_pixelate(input: &ColorImage, block_size: usize) -> ColorImage {
    let [w, h] = input.size;
    let mut output = input.clone();

    // Process in blocks
    for y in (0..h).step_by(block_size) {
        for x in (0..w).step_by(block_size) {
            // Calculate average color for this block
            let mut r_sum: u32 = 0;
            let mut g_sum: u32 = 0;
            let mut b_sum: u32 = 0;
            let mut count: u32 = 0;

            let block_h = (y + block_size).min(h) - y;
            let block_w = (x + block_size).min(w) - x;

            for by in 0..block_h {
                for bx in 0..block_w {
                    let idx = (y + by) * w + (x + bx);
                    let pixel = input.pixels[idx];
                    r_sum += pixel.r() as u32;
                    g_sum += pixel.g() as u32;
                    b_sum += pixel.b() as u32;
                    count += 1;
                }
            }

            let avg_r = (r_sum / count) as u8;
            let avg_g = (g_sum / count) as u8;
            let avg_b = (b_sum / count) as u8;
            let avg_color = egui::Color32::from_rgb(avg_r, avg_g, avg_b);

            // Fill block with average color
            for by in 0..block_h {
                for bx in 0..block_w {
                    let idx = (y + by) * w + (x + bx);
                    output.pixels[idx] = avg_color;
                }
            }
        }
    }

    output
}

/// Apply sepia tone
fn apply_sepia(input: &ColorImage) -> ColorImage {
    let mut output = input.clone();

    for pixel in output.pixels.iter_mut() {
        let r = pixel.r() as f32;
        let g = pixel.g() as f32;
        let b = pixel.b() as f32;

        let new_r = (r * 0.393 + g * 0.769 + b * 0.189).min(255.0) as u8;
        let new_g = (r * 0.349 + g * 0.686 + b * 0.168).min(255.0) as u8;
        let new_b = (r * 0.272 + g * 0.534 + b * 0.131).min(255.0) as u8;

        *pixel = egui::Color32::from_rgb(new_r, new_g, new_b);
    }

    output
}

/// Apply RGB split / chromatic aberration
fn apply_rgb_split(input: &ColorImage, offset: usize) -> ColorImage {
    let [w, h] = input.size;
    let mut output = input.clone();
    let offset = offset.min(w / 4);

    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;

            // Get R from offset to the right
            let r_idx = (y * w + (x + offset).min(w - 1)).min(input.pixels.len() - 1);
            let r = input.pixels[r_idx].r();

            // Get G from current position
            let g = input.pixels[idx].g();

            // Get B from offset to the left
            let b_x = x.saturating_sub(offset);
            let b_idx = y * w + b_x;
            let b = input.pixels[b_idx].b();

            output.pixels[idx] = egui::Color32::from_rgb(r, g, b);
        }
    }

    output
}

/// Apply color inversion
fn apply_invert(input: &ColorImage) -> ColorImage {
    let mut output = input.clone();

    for pixel in output.pixels.iter_mut() {
        let r = 255 - pixel.r();
        let g = 255 - pixel.g();
        let b = 255 - pixel.b();
        *pixel = egui::Color32::from_rgb(r, g, b);
    }

    output
}

/// Apply contrast adjustment
fn apply_contrast(input: &ColorImage, factor: f32) -> ColorImage {
    let mut output = input.clone();

    for pixel in output.pixels.iter_mut() {
        let r = ((pixel.r() as f32 - 128.0) * factor + 128.0).clamp(0.0, 255.0) as u8;
        let g = ((pixel.g() as f32 - 128.0) * factor + 128.0).clamp(0.0, 255.0) as u8;
        let b = ((pixel.b() as f32 - 128.0) * factor + 128.0).clamp(0.0, 255.0) as u8;
        *pixel = egui::Color32::from_rgb(r, g, b);
    }

    output
}

/// Apply H.264/MPEG compression artifacts
/// quality: 1-50, lower = more compression artifacts
fn apply_compression(input: &ColorImage, quality: u8) -> ColorImage {
    let [w, h] = input.size;
    let mut output = input.clone();

    // Block size for DCT (8 or 16 pixels)
    let block_size = 8usize;
    // Chroma subsampling factor (2 for 4:2:0)
    let chroma_subsample = 2usize;
    // Quantization step based on quality
    let quant_step = (51 - quality.clamp(1, 50)) as u16;

    // First pass: Chroma subsampling (4:2:0)
    // Process color in 2x2 blocks
    for y in (0..h).step_by(chroma_subsample) {
        for x in (0..w).step_by(chroma_subsample) {
            let mut r_sum: u32 = 0;
            let mut g_sum: u32 = 0;
            let mut b_sum: u32 = 0;
            let mut count: u32 = 0;

            // Average the 2x2 block
            for dy in 0..chroma_subsample {
                for dx in 0..chroma_subsample {
                    if y + dy < h && x + dx < w {
                        let idx = (y + dy) * w + (x + dx);
                        let pixel = input.pixels[idx];
                        r_sum += pixel.r() as u32;
                        g_sum += pixel.g() as u32;
                        b_sum += pixel.b() as u32;
                        count += 1;
                    }
                }
            }

            let avg_r = (r_sum / count) as u8;
            let avg_g = (g_sum / count) as u8;
            let avg_b = (b_sum / count) as u8;

            // Apply subsampled color back to block
            for dy in 0..chroma_subsample {
                for dx in 0..chroma_subsample {
                    if y + dy < h && x + dx < w {
                        let idx = (y + dy) * w + (x + dx);
                        let orig = input.pixels[idx];
                        // Keep luma (brightness) from original, use subsampled chroma
                        let luma = (orig.r() as u16 + orig.g() as u16 + orig.b() as u16) / 3;
                        let color_luma = (avg_r as u16 + avg_g as u16 + avg_b as u16) / 3;
                        let luma_diff = luma.saturating_sub(color_luma);

                        let new_r = (avg_r as u16 + luma_diff).clamp(0, 255) as u8;
                        let new_g = (avg_g as u16 + luma_diff).clamp(0, 255) as u8;
                        let new_b = (avg_b as u16 + luma_diff).clamp(0, 255) as u8;

                        output.pixels[idx] = egui::Color32::from_rgb(new_r, new_g, new_b);
                    }
                }
            }
        }
    }

    // Second pass: DCT blocking and quantization
    for y in (0..h).step_by(block_size) {
        for x in (0..w).step_by(block_size) {
            // Calculate average color for block
            let mut r_sum: u32 = 0;
            let mut g_sum: u32 = 0;
            let mut b_sum: u32 = 0;
            let mut count: u32 = 0;

            let block_h = (y + block_size).min(h) - y;
            let block_w = (x + block_size).min(w) - x;

            for by in 0..block_h {
                for bx in 0..block_w {
                    let idx = (y + by) * w + (x + bx);
                    let pixel = output.pixels[idx];
                    r_sum += pixel.r() as u32;
                    g_sum += pixel.g() as u32;
                    b_sum += pixel.b() as u32;
                    count += 1;
                }
            }

            // Quantize to fewer levels based on quality
            let avg_r = ((r_sum / count) as u16 / quant_step * quant_step).clamp(0, 255) as u8;
            let avg_g = ((g_sum / count) as u16 / quant_step * quant_step).clamp(0, 255) as u8;
            let avg_b = ((b_sum / count) as u16 / quant_step * quant_step).clamp(0, 255) as u8;

            // Add blocking artifact (slight darkening at edges)
            let block_artifact = if quality < 15 { 0.9 } else { 0.95 };

            for by in 0..block_h {
                for bx in 0..block_w {
                    let idx = (y + by) * w + (x + bx);
                    let original = output.pixels[idx];

                    // Mix original with quantized average (simulating DCT)
                    let mix_factor = 0.7; // How much blockiness
                    let r = (original.r() as f32 * (1.0 - mix_factor) + avg_r as f32 * mix_factor)
                        as u8;
                    let g = (original.g() as f32 * (1.0 - mix_factor) + avg_g as f32 * mix_factor)
                        as u8;
                    let b = (original.b() as f32 * (1.0 - mix_factor) + avg_b as f32 * mix_factor)
                        as u8;

                    // Apply blocking artifact at edges
                    let is_edge = by == 0 || by == block_h - 1 || bx == 0 || bx == block_w - 1;
                    let final_r = if is_edge {
                        (r as f32 * block_artifact) as u8
                    } else {
                        r
                    };
                    let final_g = if is_edge {
                        (g as f32 * block_artifact) as u8
                    } else {
                        g
                    };
                    let final_b = if is_edge {
                        (b as f32 * block_artifact) as u8
                    } else {
                        b
                    };

                    output.pixels[idx] = egui::Color32::from_rgb(final_r, final_g, final_b);
                }
            }
        }
    }

    output
}

/// Simulates decoder corruption - frozen macroblocks, static bursts, and data corruption
/// STUB: Currently disabled, using passthrough
fn apply_glitch(_input: &ColorImage, _intensity: u8, _seed: u32) -> ColorImage {
    // TODO: Implement proper glitch effects
    // Currently just returns input unchanged - needs heavier duty approach
    _input.clone()
}

/// Motion glitch - trails and corrupted motion blocks
/// STUB: Currently disabled, using passthrough
fn apply_motion_glitch(_input: &ColorImage, _trail_length: u8) -> ColorImage {
    // TODO: Implement proper motion glitch effects
    // Currently just returns input unchanged - needs heavier duty approach
    _input.clone()
}

/// Datamoshing effect - blocks get displaced and overlaid incorrectly
/// STUB: Currently disabled, using passthrough  
fn apply_datamosh(_input: &ColorImage, _displacement: i8) -> ColorImage {
    // TODO: Implement proper datamosh effects
    // Currently just returns input unchanged - needs heavier duty approach
    _input.clone()
}
