//! 显示域基本色调段：曝光之后，对比度 → 高光 → 阴影 → 白色 → 黑色。
//!
//! 与 GPU `color_pipeline.wgsl` 的 `pipeline_tone` 同公式：
//!
//! 1. 把场景线性 RGB 编码到 sRGB 显示域，取显示亮度 `Y`；
//! 2. 按 Lightroom 的顺序依次做对比度 S 曲线、高光、阴影、白色、黑色；
//! 3. 高光 / 阴影的**区域权重**取自基底层（大半径模糊后的亮度），
//!    而不是像素自己的亮度——小面积高光不会被“顺手”压暗，大面积天空才会被恢复；
//! 4. 用“亮度比”缩放 RGB（保持色相 / 通道比例），再做色域映射。
//!
//! 这一版是**端点锚定**的：对比度 / 高光 / 阴影不会把纯黑 / 纯白整体推走；
//! 只有白色（负方向）和黑色（正方向）按 LR 的端点语义移动黑 / 白场。

use fdxmp_color::gamut::map_to_gamut;
use fdxmp_color::luminance;
use fdxmp_color::transfer::{linear_to_srgb, srgb_to_linear};

use crate::params::EditParams;

/// 场景中灰（线性）。
pub const GREY: f32 = 0.18;

/// 显示域亮度权重（sRGB 编码域；与 GPU 同一组系数）。
pub const DISPLAY_LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// 对比度曲线强度（拟合自 LR 参考：S 曲线交叉点约 0.72，端点固定）。
pub const CONTRAST_STRENGTH: f32 = 1.3155;
/// 对比度 S 曲线的固定点（显示域亮度）。
pub const CONTRAST_PIVOT: f32 = 0.7215;
/// LR 参考曲线：曝光 -1EV / 0 / +1EV。
///
/// 曝光不是单纯的线性 ×2：LR 在 +1 时有高光肩部，在 -1 时也保留白点。
/// 参考数据只到 ±1EV；±2..±5EV 由该曲线按整数档复合得到（见 [`exposure_tone_lut`]）。
pub const EXPOSURE_CURVES: [[f32; TONE_CURVE_POINTS]; 3] = [
    [
        0.0000, 0.0351, 0.0713, 0.1074, 0.1464, 0.1898, 0.2348, 0.2782, 0.3235, 0.3698, 0.4203,
        0.4784, 0.5514, 0.6263, 0.7182, 0.8272, 1.0000,
    ],
    IDENTITY_CURVE,
    [
        0.0000, 0.1112, 0.2172, 0.3106, 0.3969, 0.4854, 0.5685, 0.6402, 0.7041, 0.7594, 0.8095,
        0.8538, 0.8961, 0.9318, 0.9590, 0.9833, 1.0000,
    ],
];

/// 参考曲线的采样点数（0..1，步长 1/16）。
const TONE_CURVE_POINTS: usize = 17;
/// 运行时 LUT 大小（与 GPU `TONE_LUT` 槽一致）。
pub const TONE_LUT_SIZE: usize = 256;

const IDENTITY_CURVE: [f32; TONE_CURVE_POINTS] = [
    0.0000, 0.0625, 0.1250, 0.1875, 0.2500, 0.3125, 0.3750, 0.4375, 0.5000, 0.5625, 0.6250, 0.6875,
    0.7500, 0.8125, 0.8750, 0.9375, 1.0000,
];

/// LR 参考曲线：highlight -100 / -50 / 0 / +50 / +100。
///
/// 从 `reports/lr_basic_compare.csv` 对应的 69 张参考图提取：
/// 先把每档滑块的亮度映射做单调回归，再在 17 个点上采样。
/// 端点按真实黑/白像素修正：高光 -100 仍会把纯白压到约 0.94。
pub const HIGHLIGHT_CURVES: [[f32; TONE_CURVE_POINTS]; 5] = [
    [
        0.0000, 0.0555, 0.1060, 0.1551, 0.2049, 0.2577, 0.3129, 0.3646, 0.4127, 0.4616, 0.4990,
        0.5305, 0.6111, 0.7001, 0.7135, 0.7760, 0.9400,
    ],
    [
        0.0000, 0.0592, 0.1152, 0.1708, 0.2263, 0.2842, 0.3432, 0.3996, 0.4549, 0.5112, 0.5623,
        0.6103, 0.6839, 0.7589, 0.7988, 0.8651, 0.9650,
    ],
    IDENTITY_CURVE,
    [
        0.0000, 0.0671, 0.1362, 0.2063, 0.2738, 0.3431, 0.4111, 0.4774, 0.5473, 0.6133, 0.6863,
        0.7598, 0.8152, 0.8621, 0.9487, 0.9877, 1.0000,
    ],
    [
        0.0000, 0.0715, 0.1481, 0.2260, 0.2999, 0.3755, 0.4486, 0.5196, 0.5943, 0.6632, 0.7438,
        0.8246, 0.8552, 0.9010, 0.9784, 0.9936, 1.0000,
    ],
];

/// LR 参考曲线：shadows -100 / -50 / 0 / +50 / +100。
pub const SHADOW_CURVES: [[f32; TONE_CURVE_POINTS]; 5] = [
    [
        0.0000, 0.0065, 0.0295, 0.0604, 0.0972, 0.1460, 0.2205, 0.2871, 0.3417, 0.4092, 0.4955,
        0.5902, 0.6878, 0.7793, 0.8527, 0.9246, 1.0000,
    ],
    [
        0.0000, 0.0215, 0.0623, 0.1069, 0.1579, 0.2173, 0.2903, 0.3560, 0.4148, 0.4821, 0.5602,
        0.6395, 0.7207, 0.7958, 0.8638, 0.9310, 1.0000,
    ],
    IDENTITY_CURVE,
    [
        0.0060, 0.1510, 0.2395, 0.3084, 0.3722, 0.4357, 0.4801, 0.5304, 0.5919, 0.6422, 0.6883,
        0.7333, 0.7828, 0.8272, 0.8847, 0.9426, 1.0000,
    ],
    [
        0.0120, 0.3280, 0.4181, 0.4772, 0.5352, 0.5858, 0.5952, 0.6262, 0.6811, 0.7184, 0.7477,
        0.7767, 0.8116, 0.8421, 0.8947, 0.9484, 1.0000,
    ],
];

/// LR 参考曲线：whites -100 / -50 / 0 / +50 / +100。
// 0.3010 是参考采样值，不是 log10(2) 常量；允许 clippy 的近似常量检查。
#[allow(clippy::approx_constant)]
pub const WHITE_CURVES: [[f32; TONE_CURVE_POINTS]; 5] = [
    [
        0.0000, 0.0627, 0.1236, 0.1837, 0.2418, 0.3010, 0.3592, 0.4142, 0.4669, 0.5188, 0.5680,
        0.6173, 0.6712, 0.7187, 0.7752, 0.8410, 1.0000,
    ],
    [
        0.0000, 0.0627, 0.1237, 0.1840, 0.2426, 0.3023, 0.3616, 0.4179, 0.4735, 0.5290, 0.5842,
        0.6405, 0.7023, 0.7599, 0.8256, 0.8987, 1.0000,
    ],
    IDENTITY_CURVE,
    [
        0.0000, 0.0636, 0.1280, 0.1943, 0.2607, 0.3305, 0.3993, 0.4671, 0.5436, 0.6156, 0.6917,
        0.7639, 0.8259, 0.8801, 0.9458, 0.9877, 1.0000,
    ],
    [
        0.0000, 0.0683, 0.1482, 0.2393, 0.3366, 0.4415, 0.5214, 0.6012, 0.7200, 0.8097, 0.8942,
        0.9560, 0.9764, 0.9945, 0.9991, 0.9999, 1.0000,
    ],
];

/// LR 参考曲线：blacks -100 / -50 / 0 / +50 / +100。
pub const BLACK_CURVES: [[f32; TONE_CURVE_POINTS]; 5] = [
    [
        0.0000, 0.0000, 0.0013, 0.0042, 0.0344, 0.1034, 0.1878, 0.2705, 0.3511, 0.4323, 0.5182,
        0.6067, 0.6935, 0.7703, 0.8536, 0.9290, 1.0000,
    ],
    [
        0.0000, 0.0057, 0.0689, 0.1329, 0.2013, 0.2720, 0.3407, 0.4058, 0.4725, 0.5387, 0.6063,
        0.6730, 0.7420, 0.8047, 0.8707, 0.9356, 1.0000,
    ],
    IDENTITY_CURVE,
    [
        0.0000, 0.0799, 0.1551, 0.2245, 0.2886, 0.3533, 0.4179, 0.4785, 0.5370, 0.5933, 0.6491,
        0.7051, 0.7650, 0.8197, 0.8791, 0.9387, 1.0000,
    ],
    [
        0.0000, 0.1016, 0.1899, 0.2636, 0.3290, 0.3938, 0.4582, 0.5165, 0.5688, 0.6192, 0.6686,
        0.7193, 0.7749, 0.8259, 0.8825, 0.9396, 1.0000,
    ],
];

/// 在 17 点曲线上线性采样。
#[inline]
fn sample_curve(curve: &[f32; TONE_CURVE_POINTS], x: f32) -> f32 {
    let position = x.clamp(0.0, 1.0) * (TONE_CURVE_POINTS - 1) as f32;
    let index = (position.floor() as usize).min(TONE_CURVE_POINTS - 1);
    let next = (index + 1).min(TONE_CURVE_POINTS - 1);
    let fraction = position - index as f32;
    curve[index] * (1.0 - fraction) + curve[next] * fraction
}

/// 按滑杆值在 5 档参考曲线之间插值，生成 256 项亮度 LUT。
pub fn reference_tone_lut(curves: &[[f32; TONE_CURVE_POINTS]; 5], value: f32) -> [f32; TONE_LUT_SIZE] {
    let position = ((value.clamp(-100.0, 100.0) + 100.0) / 50.0).clamp(0.0, 4.0);
    let low = position.floor() as usize;
    let high = (low + 1).min(4);
    let fraction = position - low as f32;
    std::array::from_fn(|index| {
        let x = index as f32 / (TONE_LUT_SIZE - 1) as f32;
        let a = sample_curve(&curves[low], x);
        let b = sample_curve(&curves[high], x);
        (a + (b - a) * fraction).clamp(0.0, 1.0)
    })
}

/// 曝光 LUT（-5..+5 EV）；参考曲线只到 ±1EV，更大档位由曲线复合得到。
pub fn exposure_tone_lut(exposure: f32) -> [f32; TONE_LUT_SIZE] {
    let clamped = exposure.clamp(-5.0, 5.0);
    let curve = if clamped >= 0.0 {
        &EXPOSURE_CURVES[2]
    } else {
        &EXPOSURE_CURVES[0]
    };
    let magnitude = clamped.abs();
    let steps = magnitude.floor() as usize;
    let fraction = magnitude - steps as f32;

    // 参考数据只到 ±1EV：把该曲线按整数档复合（h∘h…）得到 ±2..±5EV，
    // 小数部分在相邻两档之间混合。这样大曝光也有平滑的高光肩部，
    // 而不是线性 ×2^n 后直接削顶成死白。
    let mut levels = [[0.0f32; TONE_LUT_SIZE]; 6];
    for (index, slot) in levels[0].iter_mut().enumerate() {
        *slot = index as f32 / (TONE_LUT_SIZE - 1) as f32;
    }
    for level in 1..=5 {
        let (previous, current) = levels.split_at_mut(level);
        for (slot, source) in current[0].iter_mut().zip(previous[level - 1]) {
            *slot = sample_curve(curve, source);
        }
    }

    let low = steps.min(5);
    let high = (steps + 1).min(5);
    std::array::from_fn(|index| {
        let a = levels[low][index];
        let b = levels[high][index];
        (a + (b - a) * fraction).clamp(0.0, 1.0)
    })
}

/// 运行时 LUT 线性采样（CPU / GPU 同一套映射）。
#[inline]
pub fn sample_tone_lut(lut: &[f32; TONE_LUT_SIZE], x: f32) -> f32 {
    let position = x.clamp(0.0, 1.0) * (TONE_LUT_SIZE - 1) as f32;
    let index = (position.floor() as usize).min(TONE_LUT_SIZE - 1);
    let next = (index + 1).min(TONE_LUT_SIZE - 1);
    let fraction = position - index as f32;
    lut[index] * (1.0 - fraction) + lut[next] * fraction
}

/// 大半径基底层使用的模糊半径（与 GPU 的大模糊一致：`BLUR_RADIUS × 分辨率缩放`）。
pub const BASE_RADIUS: f32 = 4.0;

/// 半径缩放的参考长边（与 `gpu_pipeline::BLUR_REFERENCE_EDGE` 一致）。
pub const REFERENCE_EDGE: f32 = 2048.0;

/// 半径缩放上限（与 `gpu_pipeline::BLUR_SCALE_MAX` 一致）。
pub const SCALE_MAX: f32 = 4.0;

/// 长边（像素）→ 半径缩放（只放大不缩小，1..=4）。
///
/// 与 GPU 的 `detail_blur_scale` 同一公式：代理预览、1:1 与全分辨率导出的
/// 局部色调尺度保持一致。
pub fn blur_scale(long_edge: f32) -> f32 {
    (long_edge / REFERENCE_EDGE).clamp(1.0, SCALE_MAX)
}

/// 显示域基本色调段（构建一次，逐像素应用）。
#[derive(Clone, Debug)]
pub struct ToneStage {
    contrast: f32,
    exposure: f32,
    exposure_lut: [f32; TONE_LUT_SIZE],
    use_exposure_lut: bool,
    highlights_lut: [f32; TONE_LUT_SIZE],
    shadows_lut: [f32; TONE_LUT_SIZE],
    whites_lut: [f32; TONE_LUT_SIZE],
    blacks_lut: [f32; TONE_LUT_SIZE],
    active: bool,
    luma: [f32; 3],
}

impl ToneStage {
    /// 从编辑参数构建；曝光 ±1EV 和四个基本滑块各生成一张 256 项参考曲线 LUT。
    pub fn from_params(params: &EditParams) -> Self {
        let light = &params.light;
        let contrast = (light.contrast / 100.0).clamp(-1.0, 1.0);
        let exposure = light.exposure;
        let use_exposure_lut = exposure != 0.0;
        let exposure_lut = exposure_tone_lut(exposure);
        let highlights_lut = reference_tone_lut(&HIGHLIGHT_CURVES, light.highlights);
        let shadows_lut = reference_tone_lut(&SHADOW_CURVES, light.shadows);
        let whites_lut = reference_tone_lut(&WHITE_CURVES, light.whites);
        let blacks_lut = reference_tone_lut(&BLACK_CURVES, light.blacks);
        Self {
            contrast,
            exposure,
            exposure_lut,
            use_exposure_lut,
            highlights_lut,
            shadows_lut,
            whites_lut,
            blacks_lut,
            active: use_exposure_lut
                || contrast != 0.0
                || light.highlights != 0.0
                || light.shadows != 0.0
                || light.whites != 0.0
                || light.blacks != 0.0,
            luma: crate::color_engine::WORKSPACE
                .luma()
                .map(|value| value as f32),
        }
    }

    /// 全部为默认值时（GPU / CPU 可跳过整个色调段）。
    pub fn is_identity(&self) -> bool {
        !self.active
    }

    /// 五张参考曲线 LUT（高光 / 阴影 / 白色 / 黑色 / 曝光），供 GPU 打包。
    pub fn luts(&self) -> [&[f32; TONE_LUT_SIZE]; 5] {
        [
            &self.highlights_lut,
            &self.shadows_lut,
            &self.whites_lut,
            &self.blacks_lut,
            &self.exposure_lut,
        ]
    }

    /// 场景线性 RGB → 场景线性 RGB。
    ///
    /// 历史参数 `base_ev` 保留给后续局部色调；当前参考 LUT 是全局亮度曲线。
    pub fn apply(&self, rgb: [f32; 3], _base_ev: f32) -> [f32; 3] {
        if !self.active {
            return rgb;
        }

        // 曝光 LUT 需要在“曝光前”的亮度上查表；apply_reference 已经乘过 2^EV，
        // 因此在色调段内先除回来。|EV| > 1 时暂时走原来的线性曝光路径。
        let gain = if self.use_exposure_lut {
            self.exposure.exp2()
        } else {
            1.0
        };
        let base_rgb = if gain != 1.0 {
            rgb.map(|value| value / gain)
        } else {
            rgb
        };
        let encoded = base_rgb.map(|value| linear_to_srgb(value.max(0.0)).clamp(0.0, 1.0));
        let y = display_luma(encoded);

        // 按 LR 顺序：曝光 → 对比度 → 高光 → 阴影 → 白色 → 黑色。
        let mapped = self.map_luminance(y);

        // 按亮度比缩放 RGB（保持色相 / 通道比例）；纯黑单独处理。
        let mut out = if y <= 1e-6 {
            [srgb_to_linear(mapped); 3]
        } else {
            let ratio = srgb_to_linear(mapped) / srgb_to_linear(y).max(1e-6);
            base_rgb.map(|value| value * ratio)
        };
        out = out.map(|value| value.max(0.0));
        map_to_gamut(out, self.luma).0
    }

    /// 显示亮度 `y` 的逐段映射。
    fn map_luminance(&self, y: f32) -> f32 {
        let mut v = y.clamp(0.0, 1.0);

        if self.use_exposure_lut {
            v = sample_tone_lut(&self.exposure_lut, v);
        }

        // 对比度：端点固定的三次 S 曲线。曲线在 v = CONTRAST_PIVOT 处不动，
        // 0 / 1 也不动；该式由 LR 参考的灰阶映射拟合。
        if self.contrast != 0.0 {
            v += self.contrast
                * CONTRAST_STRENGTH
                * (v - CONTRAST_PIVOT)
                * v
                * (1.0 - v);
            v = v.clamp(0.0, 1.0);
        }

        v = sample_tone_lut(&self.highlights_lut, v);
        v = sample_tone_lut(&self.shadows_lut, v);
        v = sample_tone_lut(&self.whites_lut, v);
        v = sample_tone_lut(&self.blacks_lut, v);
        v.clamp(0.0, 1.0)
    }

    /// 像素自身的场景 EV（没有基底层时的退化权重）。
    pub fn self_base_ev(&self, rgb: [f32; 3]) -> f32 {
        (luminance(rgb, self.luma).max(1e-6) / GREY).log2()
    }
}

#[inline]
fn display_luma(rgb: [f32; 3]) -> f32 {
    rgb[0] * DISPLAY_LUMA[0] + rgb[1] * DISPLAY_LUMA[1] + rgb[2] * DISPLAY_LUMA[2]
}

/// 基底层：大半径模糊后的场景线性图像（取亮度后转成相对中灰的 EV）。
///
/// 与 GPU **完全同核同分辨率**：可分离、抽头间距 1px、权重 `1/(1+i²)`、边界夹取，
/// 先横向后纵向各一趟——GPU 也是这样模糊 `blur_tex`（大模糊参考），
/// 因此两端的高光 / 阴影权重逐像素一致（区域渲染只要余量 ≥ 半径也一致）。
#[derive(Clone, Debug)]
pub struct ToneBase {
    data: Vec<[f32; 3]>,
    width: usize,
    height: usize,
}

impl ToneBase {
    /// 从场景线性图像构建；`radius` 为像素半径（`BASE_RADIUS × 分辨率缩放`）。
    pub fn build(linear: &[[f32; 3]], width: usize, height: usize, radius: f32) -> Self {
        let radius = radius.round().clamp(1.0, 24.0) as i32;
        let blurred = blur_kernel(linear, width, height, radius);
        Self {
            data: blurred,
            width,
            height,
        }
    }

    /// 像素处的基底层 EV（相对中灰）。
    pub fn ev_at(&self, x: usize, y: usize, luma: [f32; 3]) -> f32 {
        let value = self.data[y.min(self.height - 1) * self.width + x.min(self.width - 1)];
        (luminance(value, luma).max(1e-6) / GREY).log2()
    }
}

/// 与 GPU `cs_blur` 同核的可分离模糊（权重 `1/(1+i²)`，抽头间距 1px，边界夹取）。
fn blur_kernel(src: &[[f32; 3]], width: usize, height: usize, radius: i32) -> Vec<[f32; 3]> {
    let mut horizontal = vec![[0.0f32; 3]; width * height];
    for y in 0..height {
        for x in 0..width {
            let mut acc = [0.0f32; 3];
            let mut weight_sum = 0.0f32;
            for i in -radius..=radius {
                let sx = (x as i32 + i).clamp(0, width as i32 - 1) as usize;
                let weight = 1.0 / (1.0 + (i * i) as f32);
                let sample = src[y * width + sx];
                for channel in 0..3 {
                    acc[channel] += sample[channel] * weight;
                }
                weight_sum += weight;
            }
            for channel in 0..3 {
                horizontal[y * width + x][channel] = acc[channel] / weight_sum.max(1e-4);
            }
        }
    }
    let mut out = vec![[0.0f32; 3]; width * height];
    for y in 0..height {
        for x in 0..width {
            let mut acc = [0.0f32; 3];
            let mut weight_sum = 0.0f32;
            for i in -radius..=radius {
                let sy = (y as i32 + i).clamp(0, height as i32 - 1) as usize;
                let weight = 1.0 / (1.0 + (i * i) as f32);
                let sample = horizontal[sy * width + x];
                for channel in 0..3 {
                    acc[channel] += sample[channel] * weight;
                }
                weight_sum += weight;
            }
            for channel in 0..3 {
                out[y * width + x][channel] = acc[channel] / weight_sum.max(1e-4);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::EditParams;

    #[test]
    fn neutral_is_identity() {
        let stage = ToneStage::from_params(&EditParams::default());
        assert!(stage.is_identity());
        let rgb = [0.2, 0.35, 0.6];
        assert_eq!(stage.apply(rgb, 0.0), rgb);
    }

    #[test]
    fn contrast_is_endpoint_anchored_s_curve() {
        let mut params = EditParams::default();
        params.light.contrast = 100.0;
        let stage = ToneStage::from_params(&params);
        let black = stage.apply([0.0; 3], -8.0);
        let white = stage.apply([1.0; 3], 4.0);
        assert!(black.iter().all(|value| *value <= 1e-6), "{black:?}");
        assert!(white.iter().all(|value| *value >= 1.0 - 1e-5), "{white:?}");

        // 正对比度压暗阴影、提亮高光，但不把端点推走。
        let dark = [0.05f32; 3];
        let bright = [0.7f32; 3];
        let base = ToneStage::from_params(&EditParams::default());
        let dark_out = stage.apply(dark, base.self_base_ev(dark));
        let bright_out = stage.apply(bright, base.self_base_ev(bright));
        assert!(dark_out[0] < dark[0], "{dark:?} -> {dark_out:?}");
        assert!(bright_out[0] > bright[0], "{bright:?} -> {bright_out:?}");
    }

    #[test]
    fn exposure_above_one_ev_keeps_highlight_rolloff() {
        // +2EV 以前会走线性路径直接削顶（死白），现在用复合曲线保留层次。
        let mut params = EditParams::default();
        params.light.exposure = 2.0;
        let stage = ToneStage::from_params(&params);
        assert!(stage.use_exposure_lut, "+2EV 应使用曝光曲线");

        let mut prev = -1.0;
        for index in 0..=100 {
            let y = index as f32 / 100.0;
            let value = stage.map_luminance(y);
            assert!((0.0..=1.0).contains(&value), "{y} -> {value}");
            assert!(value >= prev - 1e-5, "{y}: {value} < {prev}");
            prev = value;
        }
        // 中高调仍有层次，而不是全白。
        let mid = stage.map_luminance(0.3);
        let high = stage.map_luminance(0.7);
        assert!(mid < 0.98, "+2EV 中调不该直接到白: {mid}");
        assert!(high > mid, "高调应更亮: {high} <= {mid}");

        // +1EV 也比线性更收敛，且两者单调递增。
        params.light.exposure = 1.0;
        let one = ToneStage::from_params(&params);
        assert!(one.map_luminance(0.3) < mid);
    }

    #[test]
    fn exposure_curve_matches_linear_for_fractional_steps() {
        // 0EV 恒等；>1EV 的曲线在端点仍到白。
        let identity = ToneStage::from_params(&EditParams::default());
        assert!(!identity.use_exposure_lut);
        for value in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            assert!((identity.map_luminance(value) - value).abs() < 1e-4);
        }

        let mut params = EditParams::default();
        params.light.exposure = 3.5;
        let stage = ToneStage::from_params(&params);
        assert!(stage.use_exposure_lut);
        assert!((stage.map_luminance(1.0) - 1.0).abs() < 1e-4);
        assert!(stage.map_luminance(0.0) < 0.02);
    }

    #[test]
    fn highlights_follow_reference_curve() {
        let mut params = EditParams::default();
        params.light.highlights = -100.0;
        let stage = ToneStage::from_params(&params);
        let high = stage.map_luminance(0.7);
        let white = stage.map_luminance(1.0);
        assert!(high < 0.7 && high > 0.5, "高光 -100: {high}");
        assert!((white - 0.94).abs() < 0.01, "白场应收敛到 ~0.94: {white}");

        params.light.highlights = 100.0;
        let stage = ToneStage::from_params(&params);
        assert!(stage.map_luminance(0.7) > 0.75);
        assert!((stage.map_luminance(1.0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn shadows_follow_reference_curve() {
        let mut params = EditParams::default();
        params.light.shadows = 100.0;
        let stage = ToneStage::from_params(&params);
        let black = stage.map_luminance(0.0);
        let dark = stage.map_luminance(0.1);
        let high = stage.map_luminance(0.7);
        assert!((black - 0.012).abs() < 0.01, "纯黑轻微抬起：{black}");
        assert!(dark > 0.2, "暗部 +100 应大幅提亮：{dark}");
        assert!(high > 0.7, "LR shadows 会轻微抬亮高调：{high}");

        params.light.shadows = -100.0;
        let stage = ToneStage::from_params(&params);
        assert!(stage.map_luminance(0.1) < 0.1);
    }

    #[test]
    fn whites_and_blacks_follow_reference_curve() {
        let mut whites = EditParams::default();
        whites.light.whites = -100.0;
        let stage = ToneStage::from_params(&whites);
        assert!((stage.map_luminance(1.0) - 1.0).abs() < 1e-5);
        assert!(stage.map_luminance(0.75) < 0.75);
        assert!(stage.map_luminance(0.5) < 0.5);

        let mut blacks = EditParams::default();
        blacks.light.blacks = 100.0;
        let stage = ToneStage::from_params(&blacks);
        assert!((stage.map_luminance(0.0)).abs() < 1e-5);
        assert!(stage.map_luminance(0.1) > 0.1);
        assert!(stage.map_luminance(0.5) > 0.5);

        blacks.light.blacks = -100.0;
        let stage = ToneStage::from_params(&blacks);
        assert!(stage.map_luminance(0.1) < 0.1);
        assert!((stage.map_luminance(1.0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn reference_luts_are_monotone_at_every_stop() {
        for value in (-100..=100).step_by(25) {
            let value = value as f32;
            for curves in [&HIGHLIGHT_CURVES, &SHADOW_CURVES, &WHITE_CURVES, &BLACK_CURVES] {
                let lut = reference_tone_lut(curves, value);
                let mut previous = -1.0f32;
                for sample in lut {
                    assert!(sample >= previous - 1e-6, "{value}: {sample} < {previous}");
                    assert!((0.0..=1.0).contains(&sample));
                    previous = sample;
                }
            }
        }
    }

    #[test]
    fn output_stays_in_gamut() {
        let mut params = EditParams::default();
        params.light.contrast = 100.0;
        params.light.highlights = -100.0;
        params.light.shadows = 100.0;
        params.light.whites = -100.0;
        params.light.blacks = 100.0;
        let stage = ToneStage::from_params(&params);
        for sample in [
            [0.0f32, 0.0, 0.0],
            [5.0, 5.0, 5.0],
            [0.02, 0.9, 2.5],
            [1.0, 0.0, 0.0],
        ] {
            let out = stage.apply(sample, 0.0);
            for value in out {
                assert!((0.0..=1.0).contains(&value), "{sample:?} -> {out:?}");
            }
        }
    }
}
