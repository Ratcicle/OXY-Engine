//! Real RGBA texture editing. The UV origin is the top-left of the PNG.
use image::{ImageEncoder, ImageFormat};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, io::Cursor, path::Path};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaintImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}
impl PaintImage {
    pub fn new(width: u32, height: u32, color: [u8; 4]) -> Result<Self, String> {
        if width == 0
            || height == 0
            || width > 16384
            || height > 16384
            || u64::from(width) * u64::from(height) > 67_108_864
        {
            return Err("Textura deve ter dimensões positivas e até 64 milhões de pixels".into());
        }
        let mut pixels = vec![0; (width as usize) * (height as usize) * 4];
        for p in pixels.chunks_exact_mut(4) {
            p.copy_from_slice(&color)
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.width == 0
            || self.height == 0
            || self.width > 16384
            || self.height > 16384
            || u64::from(self.width) * u64::from(self.height) > 67_108_864
            || self.pixels.len() as u64 != u64::from(self.width) * u64::from(self.height) * 4
        {
            Err("Buffer de textura RGBA inválido".into())
        } else {
            Ok(())
        }
    }
    pub fn from_png(bytes: &[u8]) -> Result<Self, String> {
        let mut reader = image::ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16384);
        limits.max_image_height = Some(16384);
        limits.max_alloc = Some(300_000_000);
        reader.limits(limits);
        let rgba = reader
            .decode()
            .map_err(|e| format!("PNG inválido: {e}"))?
            .to_rgba8();
        let result = Self {
            width: rgba.width(),
            height: rgba.height(),
            pixels: rgba.into_raw(),
        };
        result.validate()?;
        Ok(result)
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path)
            .map_err(|e| format!("Não foi possível ler {}: {e}", path.display()))?;
        Self::from_png(&bytes)
    }
    pub fn to_png(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(
                &self.pixels,
                self.width,
                self.height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
        Ok(bytes)
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        crate::persistence::safe_write(path, &self.to_png()?)
    }
    pub fn rgba(&self) -> &[u8] {
        &self.pixels
    }
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let i = ((y * self.width + x) * 4) as usize;
        self.pixels[i..i + 4].try_into().unwrap_or([0; 4])
    }
    pub fn set_pixel(&mut self, x: u32, y: u32, color: [u8; 4]) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let i = ((y * self.width + x) * 4) as usize;
        if self.pixels[i..i + 4] == color {
            return false;
        }
        self.pixels[i..i + 4].copy_from_slice(&color);
        true
    }
    pub fn sample_uv(&self, uv: [f32; 2]) -> [u8; 4] {
        let [x, y] = self.uv_pixel(uv);
        self.pixel(x, y)
    }
    pub fn uv_pixel(&self, uv: [f32; 2]) -> [u32; 2] {
        [
            ((uv[0].clamp(0., 1.) * self.width as f32).floor() as u32)
                .min(self.width.saturating_sub(1)),
            ((uv[1].clamp(0., 1.) * self.height as f32).floor() as u32)
                .min(self.height.saturating_sub(1)),
        ]
    }
    pub fn paint_uv(&mut self, uv: [f32; 2], radius: f32, color: [u8; 4]) -> bool {
        self.brush(
            [uv[0] * self.width as f32, uv[1] * self.height as f32],
            radius,
            color,
        )
    }
    pub fn brush(&mut self, center: [f32; 2], radius: f32, color: [u8; 4]) -> bool {
        if !center.iter().all(|v| v.is_finite()) || !radius.is_finite() {
            return false;
        }
        let radius = radius.clamp(0.5, 2048.);
        let min_x = (center[0] - radius).floor().max(0.) as u32;
        let max_x = (center[0] + radius).ceil().max(0.).min(self.width as f32) as u32;
        let min_y = (center[1] - radius).floor().max(0.) as u32;
        let max_y = (center[1] + radius).ceil().max(0.).min(self.height as f32) as u32;
        let mut changed = false;
        for y in min_y..max_y {
            for x in min_x..max_x {
                let dx = x as f32 + 0.5 - center[0];
                let dy = y as f32 + 0.5 - center[1];
                if dx * dx + dy * dy <= radius * radius {
                    changed |= self.set_pixel(x, y, over(color, self.pixel(x, y)));
                }
            }
        }
        changed
    }
    pub fn stroke(&mut self, from: [f32; 2], to: [f32; 2], radius: f32, color: [u8; 4]) -> bool {
        if !from.iter().chain(&to).all(|v| v.is_finite()) || !radius.is_finite() {
            return false;
        }
        let dx = to[0] - from[0];
        let dy = to[1] - from[1];
        let steps = ((dx * dx + dy * dy).sqrt() / (radius.max(0.5) * 0.3))
            .ceil()
            .clamp(1., 32768.) as usize;
        let mut changed = false;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            changed |= self.brush([from[0] + dx * t, from[1] + dy * t], radius, color)
        }
        changed
    }
    pub fn fill(&mut self, color: [u8; 4]) -> bool {
        let mut changed = false;
        for pixel in self.pixels.chunks_exact_mut(4) {
            if pixel != color {
                pixel.copy_from_slice(&color);
                changed = true
            }
        }
        changed
    }
    pub fn flood_fill(&mut self, x: u32, y: u32, color: [u8; 4]) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let original = self.pixel(x, y);
        if original == color {
            return false;
        }
        let mut queue = VecDeque::from([(x, y)]);
        self.set_pixel(x, y, color);
        while let Some((x, y)) = queue.pop_front() {
            for (nx, ny) in [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ] {
                if nx < self.width && ny < self.height && self.pixel(nx, ny) == original {
                    self.set_pixel(nx, ny, color);
                    queue.push_back((nx, ny));
                }
            }
        }
        true
    }
}
fn over(source: [u8; 4], destination: [u8; 4]) -> [u8; 4] {
    let sa = f32::from(source[3]) / 255.;
    let da = f32::from(destination[3]) / 255.;
    let alpha = sa + da * (1. - sa);
    if alpha <= 0. {
        return [0; 4];
    }
    let mut out = [0; 4];
    for i in 0..3 {
        out[i] = ((f32::from(source[i]) * sa + f32::from(destination[i]) * da * (1. - sa)) / alpha)
            .round()
            .clamp(0., 255.) as u8
    }
    out[3] = (alpha * 255.).round() as u8;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_png_roundtrip() {
        let mut p = PaintImage::new(256, 256, [0; 4]).unwrap();
        p.brush([128., 128.], 8., [240, 20, 50, 255]);
        let decoded = PaintImage::from_png(&p.to_png().unwrap()).unwrap();
        assert_eq!(p, decoded);
        assert_eq!(decoded.pixel(128, 128), [240, 20, 50, 255]);
        assert_eq!(decoded.pixel(0, 0), [0; 4]);
    }
    #[test]
    fn flood_fill_is_bounded_by_pixels() {
        let mut p = PaintImage::new(8, 8, [0, 0, 0, 255]).unwrap();
        for y in 0..8 {
            p.set_pixel(4, y, [255; 4]);
        }
        p.flood_fill(0, 0, [255, 0, 0, 255]);
        assert_eq!(p.pixel(3, 7), [255, 0, 0, 255]);
        assert_eq!(p.pixel(5, 7), [0, 0, 0, 255]);
    }
}
