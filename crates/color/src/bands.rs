//! 8 色带（红橙黄绿青蓝紫洋红）的中心色相与权重。
//!
//! 带心沿用 Lightroom 的 HSL 色相定义（0/30/60/120/180/225/270/315°）。
//! 关键点：**色带选择使用像素的 HSL 色相**，而不是 OkLCh 色相；Lightroom 的
//! HSL 面板也是按显示参考的 HSL 色相分带。OkLCh 只用于调整本身（保色相）。
//!
//! 权重使用相邻色带中心之间重叠的升余弦窗，再归一化；红带在 0° 两侧对称，
//! 不会像“相邻两带硬分割”那样，因为红→橙只有 30°、红←洋红有 45° 而产生
//! 左右不对称的影响范围。

use std::f32::consts::{PI, TAU};
use std::sync::OnceLock;

use crate::perceptual::{hsv_to_rgb, lab_to_lch, oklab_from_linear_srgb, rgb_to_hsl, wrap_angle};
use crate::transfer::{linear_to_srgb, srgb_to_linear};

/// 纯 sRGB 色的 HSV 色相（度）→ OkLCh 色相（弧度）。
///
/// 用于颜色分级/肤色中心等需要 OkLCh 色相的地方；HSL 面板的色带选择
/// 不再使用它，而是使用像素自身的 HSL 色相。
pub fn oklch_hue_of_srgb_hue(deg: f64) -> f32 {
    let c = hsv_to_rgb([(deg / 360.0) as f32, 1.0, 1.0]);
    let lin = c.map(srgb_to_linear);
    lab_to_lch(oklab_from_linear_srgb(lin))[2]
}

/// 每个色带中心的 OkLCh 色相（弧度，按带序）；供 UI/GPU 的色带显示使用。
pub fn band_hues() -> &'static [f32; 8] {
    static HUES: OnceLock<[f32; 8]> = OnceLock::new();
    HUES.get_or_init(|| MIXER_HUES_DEG.map(oklch_hue_of_srgb_hue))
}

/// 8 个带上的权重（按 OkLCh 色相）——保留给旧的 UI/测试调用。
///
/// HSL 调色请使用 [`band_weights_rgb`]；它按 Lightroom 的 HSL 语义选带。
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

/// 各带心的 HSL 色相（度）。
pub const MIXER_HUES_DEG: [f64; 8] = [0.0, 30.0, 60.0, 120.0, 180.0, 225.0, 270.0, 315.0];

#[inline]
fn wrap_positive(a: f32) -> f32 {
    a.rem_euclid(TAU)
}

/// 色带中心（弧度）。
pub fn band_hues_hsl() -> [f32; 8] {
    MIXER_HUES_DEG.map(|deg| (deg as f32).to_radians())
}

/// 按 HSL 色相 `h`（弧度）计算 8 个带的权重（归一化，和恒为 1）。
///
/// 每个带用一个升余弦窗，半宽取左右相邻中心距离的 0.75 倍；窗口重叠后
/// 归一化，因此带心仍是该带主导，带间过渡连续。
#[inline]
pub fn band_weights_hue(h: f32) -> [f32; 8] {
    let centers = band_hues_hsl();
    let mut raw = [0.0f32; 8];
    for i in 0..8 {
        let left = wrap_positive(centers[i] - centers[(i + 7) % 8]);
        let right = wrap_positive(centers[(i + 1) % 8] - centers[i]);
        let half = 0.65 * left.max(right);
        let d = wrap_positive(h - centers[i]);
        let d = d.min(TAU - d);
        if d < half && half > 1e-6 {
            raw[i] = 0.5 + 0.5 * (PI * d / half).cos();
        }
    }
    let sum: f32 = raw.iter().sum();
    if sum <= 1e-6 {
        // 理论上不会发生（窗口覆盖整个圆），保底给最近的带。
        let mut best = 0;
        let mut best_d = f32::MAX;
        for (i, center) in centers.iter().enumerate() {
            let d = wrap_positive(h - center);
            let d = d.min(TAU - d);
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        raw[best] = 1.0;
        return raw;
    }
    for value in &mut raw {
        *value /= sum;
    }
    raw
}

/// 线性 sRGB 像素的 HSL 色相（弧度）。
pub fn hsl_hue_of_linear_srgb(rgb: [f32; 3]) -> f32 {
    let encoded = rgb.map(linear_to_srgb);
    rgb_to_hsl(encoded)[0] * TAU
}

/// 按像素颜色选择色带（HSL 色相）。
#[inline]
pub fn band_weights_rgb(rgb: [f32; 3]) -> [f32; 8] {
    band_weights_hue(hsl_hue_of_linear_srgb(rgb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_partition_unity() {
        for i in 0..720 {
            let h = (i as f32).to_radians() - PI;
            let w = band_weights_hue(h);
            let sum: f32 = w.iter().sum();
            assert!((sum - 1.0).abs() < 1e-4, "{i}: {sum}");
            assert!(w.iter().all(|v| (-1e-6..=1.0 + 1e-6).contains(v)));
        }
    }

    #[test]
    fn band_centre_is_dominated_by_its_band() {
        for (i, hue) in band_hues_hsl().iter().enumerate() {
            let w = band_weights_hue(*hue);
            assert!(w[i] > 0.85, "band {i}: {}", w[i]);
        }
    }

    #[test]
    fn red_is_symmetric_around_zero() {
        // 红带在 0° 左右应大致对称；旧的分割权重在 350° 与 10° 上不一致。
        let left = band_weights_hue((-10.0f32).to_radians())[0];
        let right = band_weights_hue(10.0f32.to_radians())[0];
        assert!((left - right).abs() < 0.05, "{left} vs {right}");
    }

    #[test]
    fn pure_colours_hit_their_hsl_band() {
        let cases = [
            ([1.0, 0.0, 0.0], 0),
            ([1.0, 0.5, 0.0], 1),
            ([1.0, 1.0, 0.0], 2),
            ([0.0, 1.0, 0.0], 3),
            ([0.0, 1.0, 1.0], 4),
            ([0.0, 0.0, 1.0], 5),
            ([0.5, 0.0, 1.0], 6),
            ([1.0, 0.0, 1.0], 7),
        ];
        for (srgb, band) in cases {
            let rgb = srgb.map(srgb_to_linear);
            let w = band_weights_rgb(rgb);
            assert!(w[band] > 0.8, "{srgb:?} -> {w:?}, expected {band}");
        }
    }
}
