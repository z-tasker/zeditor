//! CPU-based video effects for post-processing
//! 
//! This module provides efficient pixel manipulation effects that work
//! on Raspberry Pi and other devices without GPU acceleration.

use egui::ColorImage;

/// Types of CPU video effects
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VideoEffect {
    /// No effect - passthrough
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
            VideoEffect::Compression { quality } => "H264",
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
        }
    }
    
    /// Cycle to next effect
    pub fn next(&self) -> VideoEffect {
        match self {
            VideoEffect::None => VideoEffect::Pixelate { block_size: 16 },
            VideoEffect::Pixelate { block_size } if *block_size < 32 => 
                VideoEffect::Pixelate { block_size: block_size * 2 },
            VideoEffect::Pixelate { .. } => VideoEffect::Sepia,
            VideoEffect::Sepia => VideoEffect::RgbSplit { offset: 5 },
            VideoEffect::RgbSplit { offset } if *offset < 15 =>
                VideoEffect::RgbSplit { offset: offset + 5 },
            VideoEffect::RgbSplit { .. } => VideoEffect::Invert,
            VideoEffect::Invert => VideoEffect::Contrast { factor: 1.5 },
            VideoEffect::Contrast { factor } if *factor < 2.5 =>
                VideoEffect::Contrast { factor: factor + 0.5 },
            VideoEffect::Contrast { .. } => VideoEffect::None,
        }
    }
}

impl Default for VideoEffect {
    fn default() -> Self {
        VideoEffect::None
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
            let b_x = if x >= offset { x - offset } else { 0 };
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
