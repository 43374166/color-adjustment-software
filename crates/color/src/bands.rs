//! 8 色带（红橙黄绿青蓝紫洋红）的中心色相与权重。
//!
//! 带心沿用 Lightroom / LightCraft 的 HSV 色相定义（0/30/60/120/180/225/270/315°），
//! 再映射到 OkLCh 色相作为调色空间；权重是"相邻两带之间的升余弦分割"（单位分解，
//! 任意色相恰好由两个相邻带覆盖），比固定宽度的钟形叠加更可预测。

use std::f32::consts::{PI, TAU};
use std::sync::OnceLock;

use crate::perceptual::{hsv_to_rgb, lab_to_lch, oklab_from_linear_srgb, wrap_angle};
use crate::transfer::srgb_to_linear;

/// 各带心的 HSV 色相（度）。
pub const MIXER_HUES_DEG: [f64; 8] = [0.0, 30.0, 60.0, 120.0, 180.0, 225.0, 270.0, 315.0];

/// 纯 sRGB 色的 HSV 色相（度）→ OkLCh 色相（弧度）。
pub fn oklch_hue_of_srgb_hue(deg: f64) -> f32 {
    let c = hsv_to_rgb([(deg / 360.0) as f32, 1.0, 1.0]);
    let lin = c.map(srgb_to_linear);
    lab_to_lch(oklab_from_linear_srgb(lin))[2]
}

/// 每个色带中心的 OkLCh 色相（弧度，按带序）。
pub fn band_hues() -> &'static [f32; 8] {
    static HUES: OnceLock<[f32; 8]> = OnceLock::new();
    HUES.get_or_init(|| MIXER_HUES_DEG.map(oklch_hue_of_srgb_hue))
}

/// 色相 `h`（弧度）在 8 个带上的权重（和恒为 1）。
#[inline]
pub fn band_weights(h: f32) -> [f32; 8] {
    let hues = band_hues();
    let mut w = [0.0f32; 8];
    for i in 0..8 {
        let a = hues[i];
        let b = hues[(i + 1) % 8];
        let span = wrap_angle(b - a).rem_euclid(TAU);
        let d = wrap_angle(h - a).rem_euclid(TAU);
        if d <= span {
            let t = if span <= 1e-9 { 0.0 } else { d / span };
            let s = 0.5 - 0.5 * (t * PI).cos();
            w[i] += 1.0 - s;
            w[(i + 1) % 8] += s;
            break;
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_partition_unity() {
        for i in 0..720 {
            let h = (i as f32).to_radians() - PI;
            let w = band_weights(h);
            let sum: f32 = w.iter().sum();
            assert!((sum - 1.0).abs() < 1e-4, "{i}: {sum}");
            assert!(w.iter().all(|v| (-1e-6..=1.0 + 1e-6).contains(v)));
        }
    }

    #[test]
    fn band_centre_is_dominated_by_its_band() {
        for (i, hue) in band_hues().iter().enumerate() {
            let w = band_weights(*hue);
            assert!(w[i] > 0.99, "band {i}: {}", w[i]);
        }
    }

    #[test]
    fn red_band_hue_is_near_zero() {
        let red = band_hues()[0];
        assert!(wrap_angle(red).abs() < 1.0, "{red}");
    }
}
