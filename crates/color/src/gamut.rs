//! 色域处理：软裁剪与"向亮度去饱和"的色域映射（保持色相）。
//!
//! 场景线性管线的输出可能超出显示色域；硬裁剪会把饱和高光切成死色块，
//! 这里用单调软膝把通道压回 0..1，必要时再整体去饱和。

use crate::math::luminance;

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
}
