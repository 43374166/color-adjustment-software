//! 色域处理：软裁剪与"向亮度去饱和"的色域映射（保持色相）。
//!
//! 场景线性管线的输出可能超出显示色域；硬裁剪会把饱和高光切成死色块，
//! 这里用单调软膝把通道压回 0..1，必要时再整体去饱和。

use crate::math::{luminance, RgbSpace};
use crate::perceptual::{lab_to_lch, lch_to_lab, oklab_from_rgb, rgb_from_oklab};

/// 单调软膝：`v <= knee` 原样，超过后趋于 1（C¹ 连续）。
#[inline]
pub fn soft_clip(v: f32, knee: f32) -> f32 {
    let knee = knee.clamp(0.0, 0.999);
    if v <= knee {
        v
    } else {
        let r = 1.0 - knee;
        knee + r * (1.0 - (-(v - knee) / r).exp())
    }
}

/// 把线性 RGB 映射回 `[0, 1]`：先软裁剪，再按需要整体去饱和（朝亮度收缩）。
///
/// 返回映射后的颜色与所用的饱和度缩放（1 表示本来就在色域内）。
pub fn map_to_gamut(rgb: [f32; 3], luma: [f32; 3]) -> ([f32; 3], f32) {
    let clipped = rgb.map(|v| soft_clip(v, 0.9));
    let lo = clipped[0].min(clipped[1]).min(clipped[2]);
    let hi = clipped[0].max(clipped[1]).max(clipped[2]);
    if lo >= 0.0 && hi <= 1.0 {
        return (clipped, 1.0);
    }
    let y = luminance(clipped, luma).clamp(0.0, 1.0);
    let (mut lo_k, mut hi_k) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let k = (lo_k + hi_k) * 0.5;
        let c: [f32; 3] = std::array::from_fn(|i| y + (clipped[i] - y) * k);
        if c.iter().all(|v| (0.0..=1.0).contains(v)) {
            lo_k = k;
        } else {
            hi_k = k;
        }
    }
    let y0 = luminance(clipped, luma);
    let c: [f32; 3] = std::array::from_fn(|i| (y + (clipped[i] - y) * lo_k).clamp(0.0, 1.0));
    let y1 = luminance(c, luma).max(1e-9);
    let out = c.map(|v| (v * y0 / y1).clamp(0.0, 1.0));
    (out, lo_k)
}

/// 感知空间色域映射：在 OkLCh 中以固定明度 / 色相降低彩度，直到进入 `space` 的 0..1。
///
/// 与 [`map_to_gamut`] 的区别：后者在 RGB 中朝亮度缩放，色相会因 RGB 非线性而漂移；
/// 本函数保持 OkLCh 的 `L` 与 `h`，只压缩 `C`，因此 `饱和度 +100` 时红色不会偏橙。
/// 返回映射后的 RGB 与彩度比例（1 表示本来就在色域内）。
pub fn map_to_gamut_oklch(rgb: [f32; 3], space: RgbSpace) -> ([f32; 3], f32) {
    if rgb.iter().all(|v| v.is_finite()) && rgb.iter().all(|v| (0.0..=1.0).contains(v)) {
        return (rgb, 1.0);
    }
    let [l, c, h] = lab_to_lch(oklab_from_rgb(&space, rgb));
    let l = if l.is_finite() {
        l.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let c = if c.is_finite() { c.max(0.0) } else { 0.0 };
    if c <= 1e-7 {
        let grey = rgb_from_oklab(&space, lch_to_lab([l, 0.0, 0.0]));
        return (grey.map(|v| v.clamp(0.0, 1.0)), 1.0);
    }
    let mut lo = 0.0f32;
    let mut hi = c;
    for _ in 0..12 {
        let mid = (lo + hi) * 0.5;
        let trial = rgb_from_oklab(&space, lch_to_lab([l, mid, h]));
        if trial.iter().all(|v| (0.0..=1.0).contains(v)) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let out = if c <= lo * 0.85 {
        rgb
    } else {
        let knee = lo * 0.85;
        let range = (lo - knee).max(1e-7);
        let t = ((c - knee) / range).clamp(0.0, 1.0);
        // Smooth knee: f(0)=0, f(1)=1, and slightly below the hard boundary
        // inside the knee; above the boundary we fall back to it.
        let soft = t - 0.15 * t * (1.0 - t);
        let c_out = if c > lo {
            lo
        } else {
            knee + range * soft
        };
        let mapped = rgb_from_oklab(&space, lch_to_lab([l, c_out, h]));
        return (mapped.map(|v| v.clamp(0.0, 1.0)), c_out / c);
    };
    (out.map(|v| v.clamp(0.0, 1.0)), 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::REC2020;

    fn luma() -> [f32; 3] {
        REC2020.luma().map(|x| x as f32)
    }

    #[test]
    fn in_gamut_colours_pass_through() {
        let v = [0.3, 0.5, 0.2];
        let (out, k) = map_to_gamut(v, luma());
        assert_eq!(k, 1.0);
        for i in 0..3 {
            assert!((out[i] - v[i]).abs() < 1e-6, "{out:?}");
        }
    }

    #[test]
    fn out_of_gamut_is_mapped_into_range() {
        for v in [[2.0, 0.1, -0.2], [0.0, 3.0, 0.5], [1.5, 1.2, 0.9]] {
            let (out, k) = map_to_gamut(v, luma());
            assert!(k <= 1.0, "{v:?}");
            for c in out {
                assert!((0.0..=1.0).contains(&c), "{v:?} -> {out:?}");
            }
        }
        // 负通道必须走"去饱和"（软裁剪解决不了下溢）。
        let (_, k) = map_to_gamut([2.0, 0.1, -0.2], luma());
        assert!(k < 1.0, "{k}");
    }

    #[test]
    fn soft_clip_is_monotone_and_continuous() {
        let mut prev = 0.0f32;
        for i in 0..2000 {
            let v = i as f32 / 100.0;
            let o = soft_clip(v, 0.9);
            assert!(o >= prev, "{v}");
            assert!(o <= 1.0);
            prev = o;
        }
        assert!(soft_clip(20.0, 0.9) > 0.999);
    }

    #[test]
    fn oklch_map_preserves_hue_and_lightness() {
        for v in [[2.0, 0.1, -0.2], [0.0, 3.0, 0.5], [1.5, 1.2, 0.9]] {
            let before = lab_to_lch(oklab_from_rgb(&REC2020, v));
            let (out, k) = map_to_gamut_oklch(v, REC2020);
            let after = lab_to_lch(oklab_from_rgb(&REC2020, out));
            assert!(k < 1.0, "{v:?}");
            if before[0] <= 1.0 {
                assert!(
                    (before[0] - after[0]).abs() < 0.01,
                    "{v:?}: L {} -> {}",
                    before[0],
                    after[0]
                );
            } else {
                // 输入本身高于白点：L 被夹到 1，不再要求保持原 L。
                assert!(after[0] <= 1.0 + 1e-4, "{v:?}: L {}", after[0]);
            }
            let dh = crate::perceptual::wrap_angle(after[2] - before[2]).abs();
            // 极亮 / 极低彩度的输入夹到白点后色相没有意义，只要求结果在色域内。
            if after[1] > 0.02 {
                assert!(dh < 0.01, "{v:?}: hue drift {dh}");
            }
            assert!(out.iter().all(|c| (0.0..=1.0).contains(c)), "{out:?}");
        }
        let in_gamut = [0.2, 0.4, 0.1];
        assert_eq!(map_to_gamut_oklch(in_gamut, REC2020), (in_gamut, 1.0));
    }
}
