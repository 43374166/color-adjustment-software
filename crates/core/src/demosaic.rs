//! Minimal RGGB demosaic for LibRaw `raw2image` data.
//!
//! `imgdata.image` from LibRaw is a 4-channel mosaic (one channel populated per
//! pixel), not RGB. This module does a small edge-aware-ish bilinear pass so the
//! camera-RGB/DCP path has a well-defined input. It is deliberately simple and
//! testable; a higher-quality demosaic can replace it later.

/// Demosaic an RGGB 4-channel mosaic into linear RGB.
///
/// - `mosaic` is row-major, channel order R, G1, B, G2.
/// - `black` / `white` are sensor levels; output is `(value-black)/(white-black)`
///   clamped to `[0,1]`.
/// - The CFA is fixed RGGB because Sony ARW files are RGGB.
pub fn demosaic_rggb(
    mosaic: &[[u16; 4]],
    width: usize,
    height: usize,
    black: f32,
    white: f32,
) -> Vec<[f32; 3]> {
    let count = width.saturating_mul(height);
    if count == 0 || mosaic.len() < count {
        return Vec::new();
    }
    let scale = (white - black).max(1.0);
    let mut out = vec![[0.0f32; 3]; count];

    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let mut sums = [0.0f32; 3];
            let mut weights = [0.0f32; 3];
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let nx = (x as i32 + dx).clamp(0, width as i32 - 1) as usize;
                    let ny = (y as i32 + dy).clamp(0, height as i32 - 1) as usize;
                    let neighbor = mosaic[ny * width + nx];
                    let (channel, value) = match (ny % 2, nx % 2) {
                        (0, 0) => (0, neighbor[0]),
                        (0, 1) | (1, 0) => (1, {
                            // Prefer the green site that belongs to this pixel.
                            if ny.is_multiple_of(2) {
                                neighbor[1]
                            } else {
                                neighbor[3]
                            }
                        }),
                        _ => (2, neighbor[2]),
                    };
                    sums[channel] += value as f32;
                    weights[channel] += 1.0;
                }
            }
            for channel in 0..3 {
                let value = if weights[channel] > 0.0 {
                    sums[channel] / weights[channel]
                } else {
                    let fallback = match channel {
                        0 => mosaic[index][0],
                        1 => mosaic[index][1].max(mosaic[index][3]),
                        _ => mosaic[index][2],
                    };
                    fallback as f32
                };
                out[index][channel] = ((value - black) / scale).clamp(0.0, 1.0);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant(width: usize, height: usize, r: u16, g: u16, b: u16) -> Vec<[u16; 4]> {
        let mut image = vec![[0u16; 4]; width * height];
        for y in 0..height {
            for x in 0..width {
                let pixel = &mut image[y * width + x];
                match (y % 2, x % 2) {
                    (0, 0) => pixel[0] = r,
                    (0, 1) => pixel[1] = g,
                    (1, 0) => pixel[3] = g,
                    _ => pixel[2] = b,
                }
            }
        }
        image
    }

    #[test]
    fn constant_colour_is_preserved() {
        let image = constant(6, 6, 8000, 4000, 6000);
        let rgb = demosaic_rggb(&image, 6, 6, 0.0, 16383.0);
        assert_eq!(rgb.len(), 36);
        for pixel in rgb {
            assert!((pixel[0] - 8000.0 / 16383.0).abs() < 1e-3);
            assert!((pixel[1] - 4000.0 / 16383.0).abs() < 1e-3);
            assert!((pixel[2] - 6000.0 / 16383.0).abs() < 1e-3);
        }
    }

    #[test]
    fn black_and_white_are_clamped() {
        let image = constant(4, 4, 0, 0, 0);
        let rgb = demosaic_rggb(&image, 4, 4, 512.0, 16383.0);
        assert!(rgb.iter().all(|p| p.iter().all(|v| *v == 0.0)));
    }

    #[test]
    fn empty_input_is_safe() {
        assert!(demosaic_rggb(&[], 0, 0, 0.0, 1.0).is_empty());
    }
}
