//! 色调映射：场景参考的胶片式曲线与显示参考的曲线。
//!
//! - [`ToneMap::film`]：对数域绕中灰（0.18）的对比度缩放 + 可移动白点的扩展 Reinhard 肩部
//!   + 软膝趾部，保证单调、有界，并保留高光余量（与 Lightroom"高光不硬切"的行为同类）。
//! - [`ToneMap::display`]：显示域曲线的中性设置为恒等（JPEG / 已渲染源必须原样通过）。
//!
//! 结构与常量参考 LightCraft（MIT OR Apache-2.0）的 `crates/pipeline/src/tone.rs`。

/// 场景中灰（线性）。
pub const GREY: f32 = 0.18;

/// 场景白（线性 1.0）相对中灰的 EV（`log2(1/0.18)`）。
///
/// 电影曲线的白点在这里是**恒等锚点**：三个滑块为 0 时曲线逐点等于输入。
pub const SCENE_WHITE_EV: f32 = 2.473_9;
/// LUT 覆盖的场景 EV 范围（相对中灰）。
pub const LUT_MIN_EV: f32 = -14.0;
pub const LUT_MAX_EV: f32 = 10.0;
/// LUT 采样数。
pub const LUT_N: usize = 4096;

#[derive(Clone, Debug)]
pub struct ToneMap {
    lut: Vec<f32>,
    /// 黑场值（`blacks > 0` 时的黑点抬升量；用于纯黑像素，Lightroom 的"黑色 +"语义）。
    floor: f32,
}

#[inline]
fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl ToneMap {
    /// 场景参考曲线：`contrast` / `whites` / `blacks` 取 Lightroom 滑杆单位（-100..100）。
    ///
    /// 中性（0 / 0 / 0）是**恒等**：对比度斜率 1、白点在场景白（线性 1.0）、无趾部——
    /// 8bit sRGB / JPEG 源原样通过，RAW 的高光余量不被无谓压暗。
    /// - 对比度：对数域（EV）斜率的乘性缩放（±100 → 0.6× / 1.55×）；
    /// - 白色：白点 EV 移动（+100 白点下移 → 高光更亮直至裁切；-100 上移 → 高光收敛留余量）；
    /// - 黑色：软膝趾部（-100 压黑，+100 抬黑）。
    pub fn film(contrast: f64, whites: f64, blacks: f64) -> ToneMap {
        let c = (contrast / 100.0) as f32;
        let slope = if c >= 0.0 {
            1.0 + 0.55 * c
        } else {
            1.0 + 0.4 * c
        };
        let white_ev = SCENE_WHITE_EV - 0.9 * (whites as f32 / 100.0);
        let wl = GREY * 2f32.powf(white_ev);
        let b = (blacks / 100.0) as f32;
        let lut = (0..LUT_N)
            .map(|i| {
                let ev = LUT_MIN_EV + (LUT_MAX_EV - LUT_MIN_EV) * i as f32 / (LUT_N - 1) as f32;
                let y = GREY * 2f32.powf(ev * slope);
                // 扩展 Reinhard（白点 wl）：y(1 + y/wl²)/(1 + y)；wl = 场景白时逐点恒等。
                let mut o = (y * (1.0 + y / (wl * wl)) / (1.0 + y)).min(1.0);
                if b < 0.0 {
                    // 软膝压黑，并归一化让 1 保持 1
                    let k = -b * 0.035;
                    let e = 0.004;
                    let soft = |v: f32| ((v - k) + ((v - k) * (v - k) + e * e).sqrt()) * 0.5;
                    o = (soft(o) - soft(0.0)) / (soft(1.0) - soft(0.0));
                } else if b > 0.0 {
                    let k = b * 0.03;
                    o = k + (1.0 - k) * o;
                }
                o.clamp(0.0, 1.0)
            })
            .collect();
        // 纯黑的落点：黑色 + 时黑点被抬起（Lightroom 同一语义），否则就是 0。
        let floor = if b > 0.0 {
            (b * 0.03).clamp(0.0, 0.5)
        } else {
            0.0
        };
        ToneMap { lut, floor }
    }

    /// 显示参考曲线：中性设置下恒等；对比度是绕中灰的 S 曲线，白 / 黑调整上下段，
    /// 0.95 以上给一段短肩部。
    pub fn display(contrast: f64, whites: f64, blacks: f64) -> ToneMap {
        let c = (contrast / 100.0) as f32;
        let w = (whites / 100.0) as f32;
        let b = (blacks / 100.0) as f32;
        let m = GREY.powf(1.0 / 2.2);
        let lut = (0..LUT_N)
            .map(|i| {
                let ev = LUT_MIN_EV + (LUT_MAX_EV - LUT_MIN_EV) * i as f32 / (LUT_N - 1) as f32;
                let y = GREY * 2f32.powf(ev);
                let mut p = y.powf(1.0 / 2.2);
                if p <= 1.0 {
                    p += c * 0.35 * (p - m) * (1.0 - (2.0 * p - 1.0).powi(2));
                    let up = smooth(0.45, 1.0, p);
                    p += w * 0.12 * up * (1.0 - p * 0.5);
                    let lo = 1.0 - smooth(0.0, 0.45, p);
                    p += b * 0.07 * lo * (p * 2.0).min(1.0);
                } else {
                    p += w * 0.12 * 0.5;
                }
                let mut o = p.max(0.0).powf(2.2);
                if o > 0.95 {
                    let d = (o - 0.95).min(0.1);
                    o = 0.95 + d - d * d / 0.2;
                }
                o.clamp(0.0, 1.0)
            })
            .collect();
        ToneMap { lut, floor: 0.0 }
    }

    /// 采样表（`LUT_N` 项，均匀分布在对数 EV 上）。
    pub fn lut(&self) -> &[f32] {
        &self.lut
    }

    /// 场景亮度 → 显示亮度（低于 `LUT_MIN_EV` 时线性外推，避免黑场断层）。
    #[inline]
    pub fn apply(&self, y: f32) -> f32 {
        if !y.is_finite() || y <= 0.0 {
            return self.floor;
        }
        let ev = (y / GREY).log2();
        let f =
            ((ev - LUT_MIN_EV) / (LUT_MAX_EV - LUT_MIN_EV)).clamp(0.0, 1.0) * (LUT_N - 1) as f32;
        let i = (f as usize).min(LUT_N - 2);
        let t = f - i as f32;
        let v = self.lut[i] + (self.lut[i + 1] - self.lut[i]) * t;
        if ev < LUT_MIN_EV {
            v * (y / (GREY * 2f32.powf(LUT_MIN_EV)))
        } else {
            v
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn film_is_monotone_and_bounded() {
        for (c, w, b) in [
            (0.0, 0.0, 0.0),
            (100.0, 100.0, -100.0),
            (-100.0, -100.0, 100.0),
            (50.0, -30.0, -40.0),
        ] {
            let t = ToneMap::film(c, w, b);
            let mut prev = -1.0;
            for i in 0..3000 {
                let y = 1e-5 * 1.01f32.powi(i);
                let o = t.apply(y);
                assert!((0.0..=1.0).contains(&o), "{c} {w} {b} {y}: {o}");
                assert!(o >= prev - 1e-6, "{c} {w} {b} {y}: {o} < {prev}");
                prev = o;
            }
        }
    }

    #[test]
    fn film_keeps_grey_and_rolls_highlights() {
        // 中性 = 恒等（8bit / JPEG 源原样通过）。
        let t = ToneMap::film(0.0, 0.0, 0.0);
        let g = t.apply(GREY);
        assert!((0.15..0.24).contains(&g), "{g}");
        for value in [0.05f32, 0.18, 0.5, 1.0] {
            assert!(
                (t.apply(value) - value).abs() < 0.01,
                "{value} -> {}",
                t.apply(value)
            );
        }
        // 白色下移白点：高光更亮，直接到白。
        let bright = ToneMap::film(0.0, 60.0, 0.0);
        assert!(bright.apply(0.8) > t.apply(0.8), "{}", bright.apply(0.8));
        assert!(bright.apply(1.0) >= 0.99, "{}", bright.apply(1.0));
        // 白色上移白点：高光收敛留余量（>1 的场景余量被压回显示范围）。
        let soft = ToneMap::film(0.0, -60.0, 0.0);
        assert!(
            soft.apply(1.0) < 0.95 && soft.apply(1.0) > 0.6,
            "{}",
            soft.apply(1.0)
        );
        // 肩部之下的高光被平滑压暗，且仍然单调。
        assert!(soft.apply(0.5) < 0.48, "{}", soft.apply(0.5));
        assert!(soft.apply(1.0) > soft.apply(0.5));
    }

    #[test]
    fn film_sliders_move_the_right_way() {
        let base = ToneMap::film(0.0, 0.0, 0.0);
        let contrast = ToneMap::film(60.0, 0.0, 0.0);
        assert!(contrast.apply(0.05) < base.apply(0.05));
        assert!(contrast.apply(0.8) > base.apply(0.8));
        assert!(ToneMap::film(0.0, 60.0, 0.0).apply(0.8) > base.apply(0.8));
        assert!(ToneMap::film(0.0, 0.0, -60.0).apply(0.01) < base.apply(0.01));
        assert!(ToneMap::film(0.0, 0.0, 60.0).apply(0.01) > base.apply(0.01));
    }

    #[test]
    fn display_is_identity_at_neutral() {
        let t = ToneMap::display(0.0, 0.0, 0.0);
        for i in 1..=95 {
            let y = i as f32 / 100.0;
            assert!((t.apply(y) - y).abs() < 2e-3, "{y} -> {}", t.apply(y));
        }
        let c = ToneMap::display(60.0, 0.0, 0.0);
        assert!(c.apply(0.05) < 0.05 && c.apply(0.7) > 0.7);
    }
}
