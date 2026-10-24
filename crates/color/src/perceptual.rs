//! 感知空间：OkLab / OkLCh（调色算子的工作空间）与 HSV / HSL。
//!
//! OkLab 常量来自 Björn Ottosson 的公开文章（MIT 许可的实现被广泛引用）；
//! 这里通过"任意线性 RGB → XYZ → 线性 sRGB → OkLab"实现，避免记错直接 XYZ 版本的矩阵。

use crate::math::{Mat3, RgbSpace, SRGB};

/// OkLab 的 LMS 锥体矩阵（线性 sRGB 输入）。
const M1: Mat3 = Mat3([
    [0.412_221_470_8, 0.536_332_536_3, 0.051_445_992_9],
    [0.211_903_498_2, 0.680_699_545_1, 0.107_396_956_6],
    [0.088_302_461_9, 0.281_718_837_6, 0.629_978_700_5],
]);

/// LMS' → OkLab 的矩阵。
const M2: Mat3 = Mat3([
    [0.210_454_255_3, 0.793_617_785_0, -0.004_072_046_8],
    [1.977_998_495_1, -2.428_592_205_0, 0.450_593_709_9],
    [0.025_904_037_1, 0.782_771_766_2, -0.808_675_766_0],
]);

fn m1_inv() -> Mat3 {
    M1.inverse().unwrap_or(Mat3::IDENTITY)
}

fn m2_inv() -> Mat3 {
    M2.inverse().unwrap_or(Mat3::IDENTITY)
}

/// 线性 sRGB → OkLab。
pub fn oklab_from_linear_srgb(rgb: [f32; 3]) -> [f32; 3] {
    let lms = M1.apply([rgb[0] as f64, rgb[1] as f64, rgb[2] as f64]);
    let lms = [lms[0].cbrt(), lms[1].cbrt(), lms[2].cbrt()];
    let v = M2.apply(lms);
    [v[0] as f32, v[1] as f32, v[2] as f32]
}

/// OkLab → 线性 sRGB（可能超出 0..1，调用方自行处理色域）。
pub fn linear_srgb_from_oklab(lab: [f32; 3]) -> [f32; 3] {
    let lms = m2_inv().apply([lab[0] as f64, lab[1] as f64, lab[2] as f64]);
    let lms = [lms[0].powi(3), lms[1].powi(3), lms[2].powi(3)];
    let v = m1_inv().apply(lms);
    [v[0] as f32, v[1] as f32, v[2] as f32]
}

/// 任意空间线性 RGB → OkLab。
pub fn oklab_from_rgb(space: &RgbSpace, rgb: [f32; 3]) -> [f32; 3] {
    let xyz = space
        .to_xyz()
        .apply([rgb[0] as f64, rgb[1] as f64, rgb[2] as f64]);
    let lin = SRGB.from_xyz().apply(xyz);
    oklab_from_linear_srgb([lin[0] as f32, lin[1] as f32, lin[2] as f32])
}

/// OkLab → 任意空间线性 RGB。
pub fn rgb_from_oklab(space: &RgbSpace, lab: [f32; 3]) -> [f32; 3] {
    let lin = linear_srgb_from_oklab(lab);
    let xyz = SRGB
        .to_xyz()
        .apply([lin[0] as f64, lin[1] as f64, lin[2] as f64]);
    let rgb = space.from_xyz().apply(xyz);
    [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32]
}

/// OkLab → OkLCh（`[L, C, h(rad)]`）。
pub fn lab_to_lch(lab: [f32; 3]) -> [f32; 3] {
    let c = (lab[1] * lab[1] + lab[2] * lab[2]).sqrt();
    [lab[0], c, lab[2].atan2(lab[1])]
}

/// OkLCh → OkLab。
pub fn lch_to_lab(lch: [f32; 3]) -> [f32; 3] {
    [lch[0], lch[1] * lch[2].cos(), lch[1] * lch[2].sin()]
}

/// 角度归一化到 (-π, π]。
#[inline]
pub fn wrap_angle(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (a + PI).rem_euclid(TAU) - PI
}

/// 编码值 RGB（0..1）→ HSV。
pub fn rgb_to_hsv(c: [f32; 3]) -> [f32; 3] {
    let (r, g, b) = (c[0], c[1], c[2]);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let mut h = if d <= 1e-9 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h /= 6.0;
    let s = if max <= 1e-9 { 0.0 } else { d / max };
    [h, s, max]
}

/// HSV → 编码值 RGB。
pub fn hsv_to_rgb(hsv: [f32; 3]) -> [f32; 3] {
    let h = hsv[0].rem_euclid(1.0);
    let s = hsv[1].clamp(0.0, 1.0);
    let v = hsv[2];
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    match (i as i32).rem_euclid(6) {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// 编码值 RGB（0..1）→ HSL。
pub fn rgb_to_hsl(c: [f32; 3]) -> [f32; 3] {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let l = (max + min) * 0.5;
    let d = max - min;
    if d <= 1e-9 {
        return [0.0, 0.0, l];
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-9);
    let h = rgb_to_hsv(c)[0];
    [h, s, l]
}

/// HSL → 编码值 RGB。
pub fn hsl_to_rgb(hsl: [f32; 3]) -> [f32; 3] {
    let h = hsl[0].rem_euclid(1.0) * 6.0;
    let s = hsl[1].clamp(0.0, 1.0);
    let l = hsl[2].clamp(0.0, 1.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let m = l - c * 0.5;
    let (r, g, b) = match h as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::REC2020;

    #[test]
    fn oklab_roundtrip() {
        for c in [
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.4, 0.1, 0.05],
            [0.02, 0.05, 0.4],
            [0.9, 0.3, 0.2],
        ] {
            let back = linear_srgb_from_oklab(oklab_from_linear_srgb(c));
            for i in 0..3 {
                assert!((back[i] - c[i]).abs() < 1e-4, "{c:?} -> {back:?}");
            }
        }
        // 任意工作空间（Rec.2020）往返
        let v = [0.6, 0.25, 0.1];
        let back = rgb_from_oklab(&REC2020, oklab_from_rgb(&REC2020, v));
        for i in 0..3 {
            assert!((back[i] - v[i]).abs() < 1e-4, "{v:?} -> {back:?}");
        }
    }

    #[test]
    fn oklab_lightness_is_sane() {
        let l = oklab_from_linear_srgb([1.0, 1.0, 1.0])[0];
        assert!((l - 1.0).abs() < 1e-4, "{l}");
        let mid = oklab_from_linear_srgb([0.18, 0.18, 0.18])[0];
        assert!((0.3..0.7).contains(&mid), "{mid}");
    }

    #[test]
    fn known_hues() {
        let red = lab_to_lch(oklab_from_linear_srgb([1.0, 0.0, 0.0]));
        let hue = red[2].to_degrees().rem_euclid(360.0);
        assert!((25.0..35.0).contains(&hue), "{hue}");
        let blue = lab_to_lch(oklab_from_linear_srgb([0.0, 0.0, 1.0]));
        let bh = blue[2].to_degrees().rem_euclid(360.0);
        assert!((250.0..280.0).contains(&bh), "{bh}");
    }

    #[test]
    fn hsv_hsl_roundtrip() {
        for c in [[0.2, 0.7, 0.4], [0.9, 0.1, 0.3], [0.5, 0.5, 0.5]] {
            let v = hsv_to_rgb(rgb_to_hsv(c));
            let l = hsl_to_rgb(rgb_to_hsl(c));
            for i in 0..3 {
                assert!((v[i] - c[i]).abs() < 1e-4, "{c:?} {v:?}");
                assert!((l[i] - c[i]).abs() < 1e-4, "{c:?} {l:?}");
            }
        }
    }
}
