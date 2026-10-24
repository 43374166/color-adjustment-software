//! 调色算子：自然饱和度 / 饱和度 / 8 色带混色 / 点颜色 / 颜色分级 / 相机校准 / 黑白。
//!
//! 全部在 OkLab / OkLCh 上运算（输入输出是工作空间的**线性** RGB）。
//! 管线结构与参数语义参考 LightCraft（MIT OR Apache-2.0）的 `crates/color`、`crates/pipeline`
//! （自然饱和度的低饱和加权与肤色保护、8 色带的分割权重、点颜色的软范围、三分级的混合与平衡、
//! 相机校准的基色矩阵构造），按 MIT 条款吸收，见 crate 根文档与 `README.md`。

use crate::bands::{band_weights, oklch_hue_of_srgb_hue};
use crate::math::{luminance, Mat3, RgbSpace};
use crate::perceptual::{lab_to_lch, lch_to_lab, oklab_from_rgb, rgb_from_oklab, wrap_angle};

#[inline]
fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 单个色带的 HSL 调整（-100..100）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hsl {
    pub hue: f64,
    pub sat: f64,
    pub lum: f64,
}

/// 分级色轮（`hue` 度、`sat` 0..100、`lum` -100..100）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wheel {
    pub hue: f64,
    pub sat: f64,
    pub lum: f64,
}

/// 颜色分级：三分区 + 全局，混合（blending）与平衡（balance）语义与 Lightroom 一致。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Grading {
    pub shadows: Wheel,
    pub midtones: Wheel,
    pub highlights: Wheel,
    pub global: Wheel,
    pub blending: f64,
    pub balance: f64,
}

impl Grading {
    pub fn is_neutral(&self) -> bool {
        [self.shadows, self.midtones, self.highlights, self.global]
            .iter()
            .all(|w| w.sat == 0.0 && w.lum == 0.0)
    }
}

/// 8 色带混色（顺序：红 橙 黄 绿 青 蓝 紫 洋红）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mixer {
    pub bands: [Hsl; 8],
}

impl Mixer {
    pub fn is_neutral(&self) -> bool {
        self.bands
            .iter()
            .all(|b| b.hue == 0.0 && b.sat == 0.0 && b.lum == 0.0)
    }
}

/// 点颜色采样（对应 Lightroom 的 Point Color）。
///
/// `lum` / `chroma` / `hue` 是采样点的 OkLab 值（`hue` 为度），其余是调整量与范围。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointColor {
    pub lum: f64,
    pub chroma: f64,
    pub hue: f64,
    pub hue_shift: f64,
    pub sat_shift: f64,
    pub lum_shift: f64,
    /// 向采样点压缩（负）或扩散（正）。
    pub variance: f64,
    pub range: f64,
    pub hue_range: f64,
    pub sat_range: f64,
    pub lum_range: f64,
}

impl PointColor {
    pub fn is_neutral(&self) -> bool {
        self.hue_shift == 0.0
            && self.sat_shift == 0.0
            && self.lum_shift == 0.0
            && self.variance == 0.0
    }
}

/// 相机校准（Lightroom 的 Calibration 面板）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Calibration {
    pub red_hue: f64,
    pub red_sat: f64,
    pub green_hue: f64,
    pub green_sat: f64,
    pub blue_hue: f64,
    pub blue_sat: f64,
    /// 阴影色调：-100 绿 ↔ +100 洋红。
    pub shadows_tint: f64,
}

/// 校准基色在 ±100 时的色相旋转（弧度）与饱和度缩放。
pub const CALIB_HUE: f32 = 0.5;
pub const CALIB_SAT: f32 = 0.6;
/// 校准阴影色调在 ±100 时绿色通道的相对变化。
pub const SHADOW_TINT: f32 = 0.3;

/// 单点颜色的解析参数（内部使用）。
#[derive(Clone, Copy, Debug)]
struct PointK {
    l: f32,
    c: f32,
    h: f32,
    dh: f32,
    sat: f32,
    dl: f32,
    var: f32,
    wh: f32,
    wc: f32,
    wl: f32,
}

/// 每个点颜色样本占用的 GPU float 数（与 [`point_slots`] 一致）。
pub const POINT_WORDS: usize = 10;

/// 点颜色样本 → GPU uniform 字（顺序：`l, c, h, dh, sat, dl, var, wh, wc, wl`）。
///
/// 着色器按同一顺序还原 [`PointK`] 的权重与调整，保证 CPU / GPU 一致。
pub fn point_slots(p: &PointColor) -> [f32; POINT_WORDS] {
    let k = PointK::new(p);
    [k.l, k.c, k.h, k.dh, k.sat, k.dl, k.var, k.wh, k.wc, k.wl]
}

impl PointK {
    fn new(p: &PointColor) -> PointK {
        let k = (p.range.clamp(0.0, 100.0) / 50.0).max(0.05) as f32;
        let f = |v: f64| (v.clamp(0.0, 100.0) / 100.0) as f32;
        PointK {
            l: p.lum as f32,
            c: p.chroma.max(0.0) as f32,
            h: (p.hue as f32).to_radians(),
            dh: (p.hue_shift.clamp(-100.0, 100.0) / 100.0) as f32 * 0.5,
            sat: (p.sat_shift.clamp(-100.0, 100.0) / 100.0) as f32,
            dl: (p.lum_shift.clamp(-100.0, 100.0) / 100.0) as f32 * 0.2,
            var: (p.variance.clamp(-100.0, 100.0) / 100.0) as f32 * 0.8,
            wh: (0.1 + 0.6 * f(p.hue_range)) * k,
            wc: (0.02 + 0.12 * f(p.sat_range)) * k,
            wl: (0.05 + 0.4 * f(p.lum_range)) * k,
        }
    }

    /// 该 OkLCh 颜色属于本采样范围的程度（0..1，软边缘）。
    fn weight(&self, l: f32, c: f32, h: f32) -> f32 {
        let wh = if self.c < 0.02 {
            1.0
        } else {
            (1.0 - smooth(0.5 * self.wh, self.wh, wrap_angle(h - self.h).abs()))
                * smooth(0.005, 0.025, c)
        };
        if wh <= 0.0 {
            return 0.0;
        }
        let wc = 1.0 - smooth(0.5 * self.wc, self.wc, (c - self.c).abs());
        let wl = 1.0 - smooth(0.5 * self.wl, self.wl, (l - self.l).abs());
        wh * wc * wl
    }

    fn apply(&self, lch: [f32; 3]) -> [f32; 3] {
        let [mut l, mut c, mut h] = lch;
        let w = self.weight(l, c, h);
        if w <= 0.0 {
            return lch;
        }
        h += w * (self.var * wrap_angle(h - self.h) + self.dh);
        c += w * self.var * (c - self.c);
        c = (c * (1.0 + w * self.sat)).max(0.0);
        l += w * (self.var * (l - self.l) + self.dl);
        [l, c, h]
    }
}

/// 一组调色算子（构建一次、逐像素应用）。
#[derive(Clone, Debug)]
pub struct ColorOps {
    space: RgbSpace,
    pub vibrance: f64,
    pub saturation: f64,
    pub mixer: Option<Mixer>,
    pub points: Vec<PointColor>,
    pub grading: Option<Grading>,
    /// 黑白混色（8 带，-100..100）；`Some` 表示输出为单色。
    pub bw: Option<[f64; 8]>,
    skin_hue: f32,
}

impl ColorOps {
    /// 工作空间决定 OkLab 换算的往返精度（sRGB 与 Rec.2020 都可）。
    pub fn new(space: RgbSpace) -> Self {
        Self {
            space,
            vibrance: 0.0,
            saturation: 0.0,
            mixer: None,
            points: Vec::new(),
            grading: None,
            bw: None,
            skin_hue: oklch_hue_of_srgb_hue(25.0),
        }
    }

    pub fn is_identity(&self) -> bool {
        self.saturation == 0.0
            && self.vibrance == 0.0
            && self.mixer.is_none_or(|m| m.is_neutral())
            && self.points.iter().all(|p| p.is_neutral())
            && self.grading.is_none_or(|g| g.is_neutral())
            && self.bw.is_none()
    }

    /// 逐像素应用。输入 / 输出是 `space` 的线性 RGB（可含 <0 或 >1 的高光余量）。
    #[allow(clippy::needless_range_loop)]
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.is_identity() {
            return rgb;
        }
        let lab = oklab_from_rgb(&self.space, rgb);
        let needs_lch = self.mixer.is_some() || !self.points.is_empty() || self.bw.is_some();
        if !needs_lch {
            let mut lab = lab;
            let c0 = (lab[1] * lab[1] + lab[2] * lab[2]).sqrt();
            let mut c = c0;
            c = self.vibrance_saturation(c, lab[2].atan2(lab[1]));
            if c0 > 1e-12 {
                let k = c / c0;
                lab[1] *= k;
                lab[2] *= k;
            }
            return rgb_from_oklab(&self.space, self.grade(lab));
        }

        let [mut l, mut c, mut h] = lab_to_lch(lab);
        if let Some(m) = &self.mixer {
            let w = band_weights(h);
            let (mut dh, mut ds, mut dl) = (0.0f32, 0.0f32, 0.0f32);
            for i in 0..8 {
                dh += w[i] * (m.bands[i].hue / 100.0) as f32 * 0.5;
                ds += w[i] * (m.bands[i].sat / 100.0) as f32;
                dl += w[i] * (m.bands[i].lum / 100.0) as f32 * 0.18;
            }
            let chroma_w = (c / 0.12).min(1.0);
            h += dh * chroma_w;
            c *= (1.0 + ds).max(0.0);
            // 明亮度正向提升时轻微去饱和：OkLCh 保持彩度会显得更艳，
            // Lightroom 的明亮度 + 是"柔光"方向（越亮越淡）。
            let dl_pos = dl.max(0.0);
            if dl_pos > 0.0 {
                c *= (1.0 - 1.2 * dl_pos).max(0.0);
            }
            l += dl * chroma_w * l.max(0.05).sqrt();
        }
        for p in &self.points {
            if !p.is_neutral() {
                [l, c, h] = PointK::new(p).apply([l, c, h]);
            }
        }
        c = self.vibrance_saturation(c, h);
        if let Some(bw) = &self.bw {
            let w = band_weights(h);
            let mix: f32 = (0..8).map(|i| w[i] * (bw[i] / 100.0) as f32).sum();
            l = (l + mix * (c / 0.2).min(1.0) * 0.25).max(0.0);
            c = 0.0;
        }
        rgb_from_oklab(&self.space, self.grade(lch_to_lab([l, c, h])))
    }

    /// 自然饱和度 + 饱和度（自然饱和度对低饱和色加权、对肤色保护）。
    #[inline]
    fn vibrance_saturation(&self, mut c: f32, h: f32) -> f32 {
        if self.vibrance != 0.0 {
            let low = 1.0 - (c / 0.22).clamp(0.0, 1.0);
            let skin = if self.vibrance > 0.0 {
                1.0 - 0.6 * (-(wrap_angle(h - self.skin_hue) / 0.35).powi(2)).exp()
            } else {
                1.0
            };
            c *= (1.0 + self.vibrance as f32 * low * low * skin * 1.2).max(0.0);
        }
        if self.saturation != 0.0 {
            c *= (1.0 + self.saturation as f32).max(0.0);
        }
        c
    }

    /// 三分级 + 全局（OkLab 上按亮度分区的色相 / 色度 / 明度偏移）。
    fn grade(&self, mut lab: [f32; 3]) -> [f32; 3] {
        let Some(g) = &self.grading else {
            return lab;
        };
        if g.is_neutral() {
            return lab;
        }
        let wheel = |w: &Wheel| {
            let h = oklch_hue_of_srgb_hue(w.hue);
            let s = (w.sat / 100.0) as f32 * 0.09;
            (s * h.cos(), s * h.sin(), (w.lum / 100.0) as f32 * 0.12)
        };
        let m = 0.5 - (g.balance / 100.0) as f32 * 0.25;
        let width = 0.15 + (g.blending / 100.0) as f32 * 0.5;
        let ws = 1.0 - smooth(m - width, m + width * 0.25, lab[0]);
        let wh = smooth(m - width * 0.25, m + width, lab[0]);
        let wm = (1.0 - ws - wh).max(0.0);
        let wheels = [
            wheel(&g.shadows),
            wheel(&g.midtones),
            wheel(&g.highlights),
            wheel(&g.global),
        ];
        for (k, wt) in wheels.iter().zip([ws, wm, wh, 1.0]) {
            lab[1] += k.0 * wt;
            lab[2] += k.1 * wt;
            lab[0] += k.2 * wt;
        }
        lab
    }
}

/// 校准基色的 3×3 矩阵（工作空间线性 RGB，行主序）。
///
/// 每个基色在 OkLCh 上做色相旋转与色度缩放（保持 OkLab 明度），
/// 再把三列按 [1,1,1] → [1,1,1] 归一化（中性不变）。
pub fn calibration_matrix(c: &Calibration, space: RgbSpace) -> Option<[[f32; 3]; 3]> {
    let prim = [
        (c.red_hue, c.red_sat),
        (c.green_hue, c.green_sat),
        (c.blue_hue, c.blue_sat),
    ];
    if prim.iter().all(|(h, s)| *h == 0.0 && *s == 0.0) {
        return None;
    }
    let mut p = [[0.0f64; 3]; 3];
    for (i, (hue, sat)) in prim.iter().enumerate() {
        let mut e = [0.0f32; 3];
        e[i] = 1.0;
        let lab = oklab_from_rgb(&space, e);
        let [l, ch, h] = lab_to_lch(lab);
        let h2 = h + (hue.clamp(-100.0, 100.0) / 100.0) as f32 * CALIB_HUE;
        let c2 = ch * (1.0 + (sat.clamp(-100.0, 100.0) / 100.0) as f32 * CALIB_SAT);
        let q = rgb_from_oklab(&space, lch_to_lab([l, c2, h2]));
        for (r, row) in p.iter_mut().enumerate() {
            row[i] = q[r] as f64;
        }
    }
    let m = Mat3(p);
    let k = m.inverse()?.apply([1.0, 1.0, 1.0]);
    Some(std::array::from_fn(|r| {
        std::array::from_fn(|col| (p[r][col] * k[col]) as f32)
    }))
}

/// 场景线性校准：基色矩阵 + 阴影色调（保持亮度）。
pub fn calibrate(
    rgb: [f32; 3],
    m: Option<&[[f32; 3]; 3]>,
    shadow_tint: f32,
    luma: [f32; 3],
) -> [f32; 3] {
    let mut c = rgb;
    if let Some(m) = m {
        c = std::array::from_fn(|r| (m[r][0] * c[0] + m[r][1] * c[1] + m[r][2] * c[2]).max(0.0));
    }
    if shadow_tint != 0.0 {
        let y0 = luminance(c, luma);
        let w = 1.0 - smooth(-5.0, -0.5, (y0.max(1e-7) / 0.18).log2());
        c[1] *= (1.0 - SHADOW_TINT * shadow_tint * w).max(0.0);
        let y1 = luminance(c, luma).max(1e-9);
        c = c.map(|v| v * y0 / y1);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{REC2020, SRGB};
    use crate::perceptual::{lab_to_lch, oklab_from_rgb};

    fn ops() -> ColorOps {
        ColorOps::new(REC2020)
    }

    fn luma() -> [f32; 3] {
        REC2020.luma().map(|v| v as f32)
    }

    #[test]
    fn neutral_is_identity() {
        let o = ops();
        assert!(o.is_identity());
        let v = [0.2, 0.3, 0.4];
        let out = o.apply(v);
        for i in 0..3 {
            assert!((out[i] - v[i]).abs() < 1e-6, "{out:?}");
        }
    }

    #[test]
    fn saturation_minus_100_is_grey() {
        let mut o = ops();
        o.saturation = -100.0;
        let g = o.apply([0.4, 0.1, 0.05]);
        assert!(
            (g[0] - g[1]).abs() < 2e-3 && (g[1] - g[2]).abs() < 2e-3,
            "{g:?}"
        );
    }

    #[test]
    fn vibrance_lifts_weak_colours_more_than_saturated_ones() {
        let mut o = ops();
        o.vibrance = 60.0;
        let weak = [0.30, 0.28, 0.26];
        let strong = [0.55, 0.20, 0.15];
        let w0 = lab_to_lch(oklab_from_rgb(&REC2020, weak))[1];
        let s0 = lab_to_lch(oklab_from_rgb(&REC2020, strong))[1];
        let w1 = lab_to_lch(oklab_from_rgb(&REC2020, o.apply(weak)))[1];
        let s1 = lab_to_lch(oklab_from_rgb(&REC2020, o.apply(strong)))[1];
        assert!(w1 / w0 > s1 / s0, "{} vs {}", w1 / w0, s1 / s0);
    }

    #[test]
    fn mixer_targets_its_band() {
        let mut o = ops();
        let mut mixer = Mixer::default();
        mixer.bands[5].sat = -100.0; // 蓝
        o.mixer = Some(mixer);
        let blue = [0.02, 0.05, 0.4];
        let red = [0.4, 0.03, 0.02];
        let cb = lab_to_lch(oklab_from_rgb(&REC2020, o.apply(blue)))[1];
        let cr0 = lab_to_lch(oklab_from_rgb(&REC2020, red))[1];
        let cr = lab_to_lch(oklab_from_rgb(&REC2020, o.apply(red)))[1];
        assert!(cb < 0.02, "{cb}");
        assert!((cr - cr0).abs() < 0.01, "{cr0} {cr}");
    }

    #[test]
    fn grading_tints_shadows_only() {
        let mut o = ops();
        o.grading = Some(Grading {
            shadows: Wheel {
                hue: 220.0,
                sat: 60.0,
                lum: 0.0,
            },
            ..Default::default()
        });
        let dark = o.apply([0.01, 0.01, 0.01]);
        let bright = o.apply([0.8, 0.8, 0.8]);
        assert!(dark[2] > dark[0], "{dark:?}");
        assert!((bright[2] - bright[0]).abs() < 0.02, "{bright:?}");
    }

    #[test]
    fn point_color_targets_its_range() {
        let skin = [0.35, 0.16, 0.09];
        let [l, c, h] = lab_to_lch(oklab_from_rgb(&REC2020, skin));
        let sample = PointColor {
            lum: l as f64,
            chroma: c as f64,
            hue: (h as f64).to_degrees(),
            hue_shift: 60.0,
            sat_shift: -50.0,
            ..Default::default()
        };
        let mut o = ops();
        o.points.push(sample);
        let out = lab_to_lch(oklab_from_rgb(&REC2020, o.apply(skin)));
        assert!(out[1] < c * 0.7, "chroma {c} -> {}", out[1]);
        assert!(wrap_angle(out[2] - h) > 0.2, "hue {h} -> {}", out[2]);
        for far in [[0.02, 0.05, 0.4], [0.05, 0.3, 0.05], [0.3, 0.3, 0.3]] {
            let f = o.apply(far);
            for i in 0..3 {
                assert!((f[i] - far[i]).abs() < 1e-4, "{far:?} -> {f:?}");
            }
        }
    }

    #[test]
    fn bw_mix_brightens_its_band() {
        let blue = [0.05, 0.1, 0.5];
        let mut o = ops();
        o.bw = Some([0.0; 8]);
        let base = o.apply(blue);
        assert!((base[0] - base[2]).abs() < 2e-3, "单色 {base:?}");
        let mut bands = [0.0f64; 8];
        bands[5] = 80.0;
        o.bw = Some(bands);
        let lifted = o.apply(blue);
        assert!(lifted[1] > base[1], "{base:?} -> {lifted:?}");
    }

    #[test]
    fn calibration_keeps_white_and_moves_primaries() {
        let grey = [0.3, 0.3, 0.3];
        let cal = Calibration {
            red_hue: 60.0,
            blue_sat: -50.0,
            ..Default::default()
        };
        let m = calibration_matrix(&cal, REC2020).unwrap();
        let out = calibrate(grey, Some(&m), 0.0, luma());
        for v in out {
            assert!((v - 0.3).abs() < 3e-3, "{out:?}");
        }
        let red = [0.4, 0.05, 0.03];
        let h0 = lab_to_lch(oklab_from_rgb(&REC2020, red))[2];
        let h1 = lab_to_lch(oklab_from_rgb(
            &REC2020,
            calibrate(red, Some(&m), 0.0, luma()),
        ))[2];
        assert!(h1 - h0 > 0.05, "{h0} -> {h1}");
    }

    #[test]
    fn works_in_srgb_working_space_too() {
        let mut o = ColorOps::new(SRGB);
        o.saturation = 25.0;
        let v = [0.3, 0.2, 0.1];
        let out = o.apply(v);
        let c0 = lab_to_lch(oklab_from_rgb(&SRGB, v))[1];
        let c1 = lab_to_lch(oklab_from_rgb(&SRGB, out))[1];
        assert!(c1 > c0, "{c0} -> {c1}");
    }
}
