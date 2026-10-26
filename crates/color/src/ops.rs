//! 调色算子：自然饱和度 / 饱和度 / 8 色带混色 / 点颜色 / 颜色分级 / 相机校准 / 黑白。
//!
//! 全部在 OkLab / OkLCh 上运算（输入输出是工作空间的**线性** RGB）。
//! 管线结构与参数语义参考 LightCraft（MIT OR Apache-2.0）的 `crates/color`、`crates/pipeline`
//! （自然饱和度的低饱和加权与肤色保护、8 色带的分割权重、点颜色的软范围、三分级的混合与平衡、
//! 相机校准的基色矩阵构造），按 MIT 条款吸收，见 crate 根文档与 `README.md`。

use crate::bands::{band_weights_hue, band_weights_rgb, oklch_hue_of_srgb_hue};
use crate::math::{luminance, Mat3, RgbSpace, SRGB};
use crate::perceptual::{
    hsl_to_rgb, lab_to_lch, lch_to_lab, oklab_from_rgb, rgb_from_oklab, rgb_to_hsl, wrap_angle,
};
use crate::transfer::{linear_to_srgb, srgb_to_linear};

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

    /// Lightroom 语义的 HSL 面板：先按像素的 **HSL 色相**选带，再在 HSL 中
    /// 直接移动 H / S / L。与 OkLCh 版相比，色带边界、饱和度和明亮度的
    /// 响应更接近 Lightroom 的 HSL 面板；OkLCh 仍用于后面的自然饱和度/分级。
    fn apply_hsl_mixer(&self, rgb: [f32; 3]) -> [f32; 3] {
        let Some(mixer) = &self.mixer else {
            return rgb;
        };
        if mixer.is_neutral() || !rgb.iter().all(|v| v.is_finite()) {
            return rgb;
        }
        let srgb_linear = if self.space == SRGB {
            rgb
        } else {
            self.space.to_space(&SRGB).apply_f32(rgb)
        };
        let encoded = srgb_linear.map(linear_to_srgb).map(|v| v.clamp(0.0, 1.0));
        let [h, s, l] = rgb_to_hsl(encoded);
        if s <= 1e-4 {
            return rgb;
        }
        let w = band_weights_hue(h * std::f32::consts::TAU);
        let (mut dh, mut ds, mut dl) = (0.0f32, 0.0f32, 0.0f32);
        for (i, weight) in w.iter().enumerate() {
            dh += *weight * (mixer.bands[i].hue / 100.0) as f32;
            ds += *weight * (mixer.bands[i].sat / 100.0) as f32;
            dl += *weight * (mixer.bands[i].lum / 100.0) as f32;
        }
        // ±100 的行程：色相约 ±30°，饱和度 ±100%，HSL 明度约 ±0.30。
        let hue = (h + dh * (30.0 / 360.0)).rem_euclid(1.0);
        let sat = (s * (1.0 + ds)).clamp(0.0, 1.0);
        let lum = (l + dl * 0.30 * (1.0 - 0.35 * l)).clamp(0.0, 1.0);
        let out = hsl_to_rgb([hue, sat, lum]).map(srgb_to_linear);
        if self.space == SRGB {
            out
        } else {
            SRGB.to_space(&self.space).apply_f32(out)
        }
    }

    /// 逐像素应用。输入 / 输出是 `space` 的线性 RGB（可含 <0 或 >1 的高光余量）。
    #[allow(clippy::needless_range_loop)]
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.is_identity() {
            return rgb;
        }
        let band_w = band_weights_rgb(rgb);
        let rgb = self.apply_hsl_mixer(rgb);
        let lab = oklab_from_rgb(&self.space, rgb);
        let needs_lch = !self.points.is_empty() || self.bw.is_some();
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
        for p in &self.points {
            if !p.is_neutral() {
                [l, c, h] = PointK::new(p).apply([l, c, h]);
                // 点颜色的 lum_shift / variance 在极值下同样可能把 L 推出 [0,1]。
                l = l.clamp(0.0, 1.0);
                c = c.max(0.0);
                h = wrap_angle(h);
            }
        }
        c = self.vibrance_saturation(c, h).max(0.0);
        if let Some(bw) = &self.bw {
            let w = band_w;
            let mix: f32 = (0..8).map(|i| w[i] * (bw[i] / 100.0) as f32).sum();
            l = (l + mix * (c / 0.2).min(1.0) * 0.25).clamp(0.0, 1.0);
            c = 0.0;
        }
        l = l.clamp(0.0, 1.0);
        c = c.max(0.0);
        h = wrap_angle(h);
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
        // 分级亮度在 ±100 时可能把纯黑推成负数 / 把纯白推过 1；
        // 输出必须落在显示参考的 L 范围，否则后续 OkLab→RGB 会出现负通道。
        lab[0] = lab[0].clamp(0.0, 1.0);
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
        let mut o = ColorOps::new(SRGB);
        let mut mixer = Mixer::default();
        mixer.bands[5].sat = -100.0; // 蓝
        o.mixer = Some(mixer);
        let blue = [0.0, 0.0, 0.4];
        let red = [0.4, 0.03, 0.02];
        let cb = lab_to_lch(oklab_from_rgb(&SRGB, o.apply(blue)))[1];
        let cr0 = lab_to_lch(oklab_from_rgb(&SRGB, red))[1];
        let cr = lab_to_lch(oklab_from_rgb(&SRGB, o.apply(red)))[1];
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
    fn hsl_mixer_red_saturation_is_selective() {
        let mut o = ColorOps::new(SRGB);
        let mut mixer = Mixer::default();
        mixer.bands[0].sat = -100.0; // 红
        o.mixer = Some(mixer);
        let red = [0.4, 0.02, 0.01];
        let blue = [0.01, 0.02, 0.4];
        let red_out = lab_to_lch(oklab_from_rgb(&SRGB, o.apply(red)))[1];
        assert!(red_out < 0.02, "red chroma {red_out}");
        let blue_out = o.apply(blue);
        let blue_ref = lab_to_lch(oklab_from_rgb(&SRGB, blue))[1];
        assert!((blue_ref - lab_to_lch(oklab_from_rgb(&SRGB, blue_out))[1]).abs() < 0.01);
    }

    #[test]
    fn hsl_hue_shift_red_moves_toward_orange() {
        let mut o = ColorOps::new(SRGB);
        let mut mixer = Mixer::default();
        mixer.bands[0].hue = 100.0;
        o.mixer = Some(mixer);
        let red = [0.4, 0.02, 0.01];
        let out = o.apply(red);
        let hue = rgb_to_hsl(out.map(linear_to_srgb))[0] * 360.0;
        assert!((20.0..45.0).contains(&hue), "red hue -> {hue}");
    }

    #[test]
    fn hsl_luminance_moves_monotonically_without_inversion() {
        let mut o = ColorOps::new(SRGB);
        let mut mixer = Mixer::default();
        mixer.bands[0].lum = 100.0;
        o.mixer = Some(mixer);
        let red = [0.4, 0.02, 0.01];
        let bright = o.apply(red);
        let mut o2 = ColorOps::new(SRGB);
        let mut mixer2 = Mixer::default();
        mixer2.bands[0].lum = -100.0;
        o2.mixer = Some(mixer2);
        let dark = o2.apply(red);
        let l = |rgb| lab_to_lch(oklab_from_rgb(&SRGB, rgb))[0];
        assert!(l(bright) > l(red), "{} vs {}", l(red), l(bright));
        assert!(l(dark) < l(red), "{} vs {}", l(red), l(dark));
        assert!(bright
            .iter()
            .chain(dark.iter())
            .all(|v| v.is_finite() && *v >= 0.0));
    }

    #[test]
    fn hsl_mixer_keeps_neutrals() {
        let mut o = ColorOps::new(SRGB);
        let mut mixer = Mixer::default();
        for band in &mut mixer.bands {
            band.hue = 100.0;
            band.sat = -100.0;
            band.lum = 100.0;
        }
        o.mixer = Some(mixer);
        for grey in [
            [0.0, 0.0, 0.0],
            [0.2, 0.2, 0.2],
            [0.5, 0.5, 0.5],
            [1.0, 1.0, 1.0],
        ] {
            let out = o.apply(grey);
            for i in 0..3 {
                assert!((out[i] - grey[i]).abs() < 1e-4, "{grey:?} -> {out:?}");
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

    #[test]
    fn hsl_luminance_extremes_keep_black_and_white() {
        // 全色带明亮度 +100 / -100 都不能把纯黑抬灰、把纯白压灰。
        for lum in [-100.0, 100.0] {
            let mut m = Mixer::default();
            for band in &mut m.bands {
                band.lum = lum;
            }
            let mut o = ops();
            o.mixer = Some(m);
            let black = o.apply([0.0, 0.0, 0.0]);
            let white = o.apply([1.0, 1.0, 1.0]);
            assert!(
                black.iter().all(|v| v.abs() < 1e-4),
                "lum {lum}: black -> {black:?}"
            );
            assert!(
                white.iter().all(|v| (*v - 1.0).abs() < 2e-3),
                "lum {lum}: white -> {white:?}"
            );
        }
    }

    #[test]
    fn point_color_extreme_lum_shift_keeps_black_black() {
        let p = PointColor {
            lum: 0.0,
            chroma: 0.0,
            hue: 0.0,
            lum_shift: -100.0,
            range: 100.0,
            lum_range: 100.0,
            ..Default::default()
        };
        let mut o = ops();
        o.points.push(p);
        let out = o.apply([0.0, 0.0, 0.0]);
        assert!(
            out.iter().all(|v| v.abs() < 1e-4),
            "point lum -100 on black -> {out:?}"
        );
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn grading_extreme_lightness_never_produces_negative_luminance() {
        let g = Grading {
            shadows: Wheel {
                hue: 220.0,
                sat: 100.0,
                lum: 100.0,
            },
            midtones: Wheel {
                hue: 120.0,
                sat: 100.0,
                lum: -100.0,
            },
            highlights: Wheel {
                hue: 40.0,
                sat: 100.0,
                lum: 100.0,
            },
            global: Wheel {
                hue: 300.0,
                sat: 100.0,
                lum: -100.0,
            },
            blending: 100.0,
            balance: 100.0,
        };
        let mut o = ops();
        o.grading = Some(g);
        for rgb in [
            [0.0, 0.0, 0.0],
            [0.001, 0.001, 0.001],
            [0.18, 0.18, 0.18],
            [1.0, 1.0, 1.0],
        ] {
            let out = o.apply(rgb);
            assert!(out.iter().all(|v| v.is_finite()), "{rgb:?} -> {out:?}");
            let y: f32 = out[0] * 0.2126 + out[1] * 0.7152 + out[2] * 0.0722;
            assert!(y >= -1e-4, "{rgb:?} -> {out:?} (Y={y})");
        }
    }

    #[test]
    fn saturation_extremes_never_change_neutral_grey() {
        for sat in [-100.0, 100.0] {
            let mut o = ops();
            o.saturation = sat;
            let grey = [0.18, 0.18, 0.18];
            let out = o.apply(grey);
            for v in out {
                assert!((v - 0.18).abs() < 1e-4, "sat {sat}: {out:?}");
            }
        }
    }
}
