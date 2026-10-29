//! 显示域色调段与局部高光 / 阴影。
//!
//! 管线分两层：
//!
//! 1. [`ToneStage`]：曝光之后，对比度 → 白色 → 黑色（显示域端点锚定曲线）；
//! 2. [`apply_local_tone`]：渲染完成后，用低分辨率 guided 亮度基底做局部曝光补偿，
//!    高光 / 阴影不再用全局 1D LUT，而是按“区域亮度”提亮 / 压暗：
//!    - 大面积天空、逆光背景会被整体压 / 提；
//!    - 同一区域内的细节保留局部对比；
//!    - 纯黑乘增益仍为黑，纯白会被高光负向压到显示白以内。
//!
//! 局部基底在显示域构建（在最终输出上取亮度），因为本版仍以 RGBA8 预览 / 导出；
//! 这样可以避免为局部影调再引入一套全分辨率浮点缓冲。后续若升级到场景线性浮点，
//! 只需把基底构建移到 tone 之前，局部增益公式不变。
//!
//! 这一版仍是**端点锚定**的：对比度不会把纯黑 / 纯白整体推走；
//! 白色（负方向）和黑色（正方向）按 LR 的端点语义移动黑 / 白场。

use fdxmp_color::gamut::map_to_gamut;
use fdxmp_color::luminance;
use fdxmp_color::transfer::{linear_to_srgb, srgb_to_linear};
use image::RgbaImage;
use rayon::prelude::*;

use crate::params::EditParams;

/// 场景中灰（线性）。
pub const GREY: f32 = 0.18;

/// 显示域亮度权重（sRGB 编码域；与 GPU 同一组系数）。
pub const DISPLAY_LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// 对比度曲线强度（拟合自 LR 参考：S 曲线交叉点约 0.72，端点固定）。
pub const CONTRAST_STRENGTH: f32 = 1.3155;
/// 对比度 S 曲线的固定点（显示域亮度）。
pub const CONTRAST_PIVOT: f32 = 0.7215;

/// 高光 / 阴影在 +100 / −100 时的最大局部曝光量（EV）。
///
/// 1.8 EV 足够处理逆光天空和背光人脸，又不至于把整块区域推成死白 / 死黑。
pub const LOCAL_HIGHLIGHT_MAX_EV: f32 = 1.2;
/// 阴影方向的最大局部曝光量（EV），与高光分开调。
pub const LOCAL_SHADOW_MAX_EV: f32 = 1.2;
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
pub fn reference_tone_lut(
    curves: &[[f32; TONE_CURVE_POINTS]; 5],
    value: f32,
) -> [f32; TONE_LUT_SIZE] {
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

/// 显示域色调段（对比度 / 白色 / 黑色）。
///
/// 高光 / 阴影的**有效实现**已移到 [`apply_local_tone`]：CPU 路径不再用
/// `highlights_lut` / `shadows_lut`；两张表仍保留，只为 GPU 旧 5 槽布局与
/// Windows 直接使用 `ColorPipeline` 的路径兼容。
#[derive(Clone, Debug)]
pub struct ToneStage {
    contrast: f32,
    /// 曝光曲线槽位（保留给 GPU uniform 布局，始终是单位映射）。
    ///
    /// 曝光按 darktable 的模型走**场景线性纯增益**（`pipeline::apply_reference`
    /// 里乘 2^EV），不再叠一层显示域曲线：曲线放大会让暗部获得远大于亮部的
    /// 增益，观感就是"一提曝光，颜色更艳/更糊"。
    exposure_lut: [f32; TONE_LUT_SIZE],
    highlights_lut: [f32; TONE_LUT_SIZE],
    shadows_lut: [f32; TONE_LUT_SIZE],
    whites_lut: [f32; TONE_LUT_SIZE],
    blacks_lut: [f32; TONE_LUT_SIZE],
    /// 蒙版局部调整仍走旧的高光 / 阴影 LUT（逐蒙版做 guided 基底成本高，
    /// 且蒙版本身就是空间局部的）；全局路径为 false，由 `apply_local_tone` 负责。
    legacy_local_luts: bool,
    active: bool,
    luma: [f32; 3],
}

impl ToneStage {
    /// 全局色调段：高光 / 阴影不在这里，交给 [`apply_local_tone`]。
    pub fn from_params(params: &EditParams) -> Self {
        Self::build(params, false)
    }

    /// 蒙版局部色调段：保留旧的高光 / 阴影参考 LUT，避免蒙版局部亮度滑杆失效。
    pub fn from_params_mask(params: &EditParams) -> Self {
        Self::build(params, true)
    }

    fn build(params: &EditParams, legacy_local_luts: bool) -> Self {
        let light = &params.light;
        let contrast = (light.contrast / 100.0).clamp(-1.0, 1.0);
        let exposure_lut = std::array::from_fn(|index| index as f32 / (TONE_LUT_SIZE - 1) as f32);
        // GPU 仍按旧 5 槽布局读取 LUT；CPU 预览 / 导出已经不走这两张表，
        // 高光 / 阴影改由 `apply_local_tone` 做局部曝光。保留旧表是为了
        // Windows 直接使用 `ColorPipeline` 时不至于突然失去这两个滑杆。
        let highlights_lut = reference_tone_lut(&HIGHLIGHT_CURVES, light.highlights);
        let shadows_lut = reference_tone_lut(&SHADOW_CURVES, light.shadows);
        let whites_lut = reference_tone_lut(&WHITE_CURVES, light.whites);
        let blacks_lut = reference_tone_lut(&BLACK_CURVES, light.blacks);
        Self {
            contrast,
            exposure_lut,
            highlights_lut,
            shadows_lut,
            whites_lut,
            blacks_lut,
            legacy_local_luts,
            // 全局：对比度 / 白 / 黑才触发本段；蒙版：高光 / 阴影也走旧 LUT。
            active: contrast != 0.0
                || light.whites != 0.0
                || light.blacks != 0.0
                || (legacy_local_luts && (light.highlights != 0.0 || light.shadows != 0.0)),
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

    /// 场景线性 RGB → 场景线性 RGB（全局 filmic 风格；蒙版走 legacy 显示域 LUT）。
    ///
    /// 历史参数 `base_ev` 保留给后续局部色调；高光 / 阴影在渲染后单独处理。
    pub fn apply(&self, rgb: [f32; 3], _base_ev: f32) -> [f32; 3] {
        if !self.active {
            return rgb;
        }

        // 曝光已在 `apply_reference` 里按场景线性乘过 2^EV（darktable 模型），
        // 色调段不再重复处理曝光。
        let scene_y = display_luma(rgb).max(0.0);
        let (mapped, ratio) = if self.legacy_local_luts {
            // 蒙版局部调整：保持旧显示域参考 LUT（局部高光 / 阴影不能丢）。
            let display_y = linear_to_srgb(scene_y);
            let mapped = self.map_luminance_legacy(display_y);
            let ratio = if scene_y <= 1e-6 {
                0.0
            } else {
                srgb_to_linear(mapped) / scene_y.max(1e-6)
            };
            (srgb_to_linear(mapped), ratio)
        } else {
            // 全局：对比度改走对数场景亮度上的端点固定 S 曲线（更接近 filmic）；
            // 白 / 黑场仍保留 Lightroom 的显示域端点 LUT 语义，避免把用户
            // 熟悉的“白 + = 提亮近白 / 黑 + = 抬暗部”变成 darktable 的 EV 边界。
            let contrast_linear = if self.contrast == 0.0 {
                scene_y
            } else {
                self.map_scene_contrast(scene_y)
            };
            let mapped_display = linear_to_srgb(contrast_linear);
            let mut v = sample_tone_lut(&self.whites_lut, mapped_display);
            v = sample_tone_lut(&self.blacks_lut, v);
            let mapped_linear = srgb_to_linear(v);
            let ratio = if scene_y <= 1e-6 {
                0.0
            } else {
                mapped_linear / scene_y.max(1e-6)
            };
            (mapped_linear, ratio)
        };
        let mut out = if scene_y <= 1e-6 {
            [mapped; 3]
        } else {
            rgb.map(|value| value * ratio)
        };
        out = out.map(|value| value.max(0.0));
        map_to_gamut(out, self.luma).0
    }

    /// 全局 filmic 风格对比度：输入场景线性亮度，输出显示线性亮度。
    ///
    /// 先把场景亮度换成相对 18% 中灰的 EV，在固定白场 2.47EV / 黑场 −8EV
    /// 的归一化对数范围里做**端点固定 S 曲线**（锚点仍是 18% 中灰）。
    /// 白 / 黑场不在这里移动，仍由后面的 Lightroom 端点 LUT 负责。
    fn map_scene_contrast(&self, y: f32) -> f32 {
        let x = (y.max(1e-8) / GREY).log2();
        let z_white = 2.47;
        let z_black = -8.0;
        let span = z_white - z_black;
        let t = ((x - z_black) / span).clamp(0.0, 1.0);
        let pivot = ((-z_black) / span).clamp(0.2, 0.8);
        let c = self.contrast;
        // 导数在 pivot 处为 1 + c*0.8：+100 变陡、−100 变平；两端固定。
        let s =
            (t + c * 0.8 * (t - pivot) * t * (1.0 - t) / (pivot * (1.0 - pivot))).clamp(0.0, 1.0);
        let z = z_black + s * span;
        let y_white = GREY * 2f32.powf(z_white);
        let y_black = GREY * 2f32.powf(z_black);
        let denominator = (y_white - y_black).max(1e-6);
        ((GREY * 2f32.powf(z) - y_black) / denominator).clamp(0.0, 1.0)
    }

    /// 蒙版 legacy 映射：显示域对比度 + 四张参考 LUT（旧行为）。
    fn map_luminance_legacy(&self, y: f32) -> f32 {
        let mut v = y.clamp(0.0, 1.0);
        if self.contrast != 0.0 {
            v += self.contrast * CONTRAST_STRENGTH * (v - CONTRAST_PIVOT) * v * (1.0 - v);
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

/// 局部影调基底：低分辨率显示域 EV 网格，经 guided filter 平滑。
///
/// 只看最终输出的亮度，不依赖 RAW 线性数据；因此预览、区域帧、导出都复用同一套
/// 构建逻辑。网格长边约 128–512，`ev_at` 双线性采样，单帧构建成本很低。
#[derive(Clone, Debug)]
pub struct LocalToneBase {
    width: usize,
    height: usize,
    grid_width: usize,
    grid_height: usize,
    ev: Vec<f32>,
}

impl LocalToneBase {
    /// 从最终 sRGB8 输出构建。
    pub fn build(image: &RgbaImage) -> Option<Self> {
        let width = image.width() as usize;
        let height = image.height() as usize;
        if width < 16 || height < 16 {
            return None;
        }
        let long = width.max(height) as f32;
        let edge = (long / 8.0).clamp(128.0, 512.0);
        let grid_width = ((width as f32 * edge / long).round() as usize).max(8);
        let grid_height = ((height as f32 * edge / long).round() as usize).max(8);
        let mut grid = vec![0.0f32; grid_width * grid_height];
        let pixels = image.as_raw();
        for gy in 0..grid_height {
            let y0 = gy * height / grid_height;
            let y1 = ((gy + 1) * height / grid_height).max(y0 + 1).min(height);
            for gx in 0..grid_width {
                let x0 = gx * width / grid_width;
                let x1 = ((gx + 1) * width / grid_width).max(x0 + 1).min(width);
                let mut sum = 0.0f64;
                let mut count = 0usize;
                for y in y0..y1 {
                    let row = y * width;
                    for x in x0..x1 {
                        let offset = (row + x) * 4;
                        let r = srgb_to_linear(pixels[offset] as f32 / 255.0);
                        let g = srgb_to_linear(pixels[offset + 1] as f32 / 255.0);
                        let b = srgb_to_linear(pixels[offset + 2] as f32 / 255.0);
                        sum += (r * 0.2126 + g * 0.7152 + b * 0.0722) as f64;
                        count += 1;
                    }
                }
                let luma = if count == 0 {
                    0.0
                } else {
                    (sum / count as f64) as f32
                };
                grid[gy * grid_width + gx] = (luma.max(1e-6) / GREY).log2().clamp(-12.0, 6.0);
            }
        }
        // 半径约短边的 4%：足够跨过肤色 / 天空的大块区域，又不会把边缘糊平。
        let radius = ((grid_width.min(grid_height) as f32) * 0.04)
            .round()
            .max(2.0) as usize;
        let ev = guided_filter(&grid, grid_width, grid_height, radius, 0.02);
        Some(Self {
            width,
            height,
            grid_width,
            grid_height,
            ev,
        })
    }

    /// 全分辨率像素坐标处的基底 EV（双线性）。
    pub fn ev_at(&self, x: usize, y: usize) -> f32 {
        let fx = (x.min(self.width.saturating_sub(1)) as f32
            / self.width.saturating_sub(1).max(1) as f32)
            * self.grid_width.saturating_sub(1) as f32;
        let fy = (y.min(self.height.saturating_sub(1)) as f32
            / self.height.saturating_sub(1).max(1) as f32)
            * self.grid_height.saturating_sub(1) as f32;
        let x0 = fx.floor() as usize;
        let y0 = fy.floor() as usize;
        let x1 = (x0 + 1).min(self.grid_width.saturating_sub(1));
        let y1 = (y0 + 1).min(self.grid_height.saturating_sub(1));
        let tx = fx - x0 as f32;
        let ty = fy - y0 as f32;
        let p00 = self.ev[y0 * self.grid_width + x0];
        let p10 = self.ev[y0 * self.grid_width + x1];
        let p01 = self.ev[y1 * self.grid_width + x0];
        let p11 = self.ev[y1 * self.grid_width + x1];
        let top = p00 + (p10 - p00) * tx;
        let bottom = p01 + (p11 - p01) * tx;
        top + (bottom - top) * ty
    }
}

/// 在最终输出上应用高光 / 阴影局部曝光。
///
/// 高光：只压 / 提基底 EV > 1.2 的区域；阴影：只压 / 提基底 EV < −1.5 的区域。
/// 中灰附近权重为 0，因此不会像原来的全局 LUT 那样把中间调整体推亮 / 推暗。
pub fn apply_local_tone(image: &mut RgbaImage, params: &EditParams) {
    let highlights = (params.light.highlights / 100.0).clamp(-1.0, 1.0);
    let shadows = (params.light.shadows / 100.0).clamp(-1.0, 1.0);
    if highlights.abs() < 1e-6 && shadows.abs() < 1e-6 {
        return;
    }
    let Some(base) = LocalToneBase::build(image) else {
        return;
    };
    let width = image.width() as usize;
    let pixels = image.as_mut();
    pixels
        .par_chunks_mut(4)
        .enumerate()
        .for_each(|(index, pixel)| {
            let x = index % width;
            let y = index / width;
            let gain_ev = local_gain_ev(base.ev_at(x, y), highlights, shadows);
            if gain_ev.abs() < 1e-5 {
                return;
            }
            let gain = 2f32.powf(gain_ev);
            let mut linear = [0.0f32; 3];
            for (channel_index, channel) in pixel.iter().take(3).enumerate() {
                linear[channel_index] = srgb_to_linear(*channel as f32 / 255.0) * gain;
            }
            // 高光正向增益可能把某个通道推出 1.0；按最大通道等比收回来，
            // 比逐通道硬夹更不容易偏色（暗部不受影响）。
            let max_channel = linear.iter().copied().fold(0.0f32, f32::max);
            if max_channel > 1.0 {
                for value in &mut linear {
                    *value /= max_channel;
                }
            }
            for (channel, value) in pixel.iter_mut().take(3).zip(linear) {
                *channel = (linear_to_srgb(value.clamp(0.0, 1.0)) * 255.0).round() as u8;
            }
        });
}

/// 局部曝光量（EV）：中灰附近为 0，高光 / 阴影两端平滑过渡。
#[inline]
fn local_gain_ev(base_ev: f32, highlights: f32, shadows: f32) -> f32 {
    let highlight_weight = smoothstep(1.2, 3.2, base_ev);
    let shadow_weight = 1.0 - smoothstep(-4.5, -1.5, base_ev);
    (highlights * LOCAL_HIGHLIGHT_MAX_EV * highlight_weight
        + shadows * LOCAL_SHADOW_MAX_EV * shadow_weight)
        .clamp(-3.5, 3.5)
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 自引导 guided filter（网格尺寸很小，积分图 O(N)）。
fn guided_filter(src: &[f32], width: usize, height: usize, radius: usize, eps: f32) -> Vec<f32> {
    let mean = box_mean(src, width, height, radius);
    let squared: Vec<f32> = src.iter().map(|value| value * value).collect();
    let mean_squared = box_mean(&squared, width, height, radius);
    let mut a = vec![0.0f32; src.len()];
    let mut b = vec![0.0f32; src.len()];
    for index in 0..src.len() {
        let variance = (mean_squared[index] - mean[index] * mean[index]).max(0.0);
        let ai = variance / (variance + eps);
        a[index] = ai;
        b[index] = mean[index] * (1.0 - ai);
    }
    let mean_a = box_mean(&a, width, height, radius);
    let mean_b = box_mean(&b, width, height, radius);
    (0..src.len())
        .map(|index| mean_a[index] * src[index] + mean_b[index])
        .collect()
}

/// 积分图盒式均值（边界夹取）。
fn box_mean(src: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    let stride = width + 1;
    let mut integral = vec![0.0f64; stride * (height + 1)];
    for y in 0..height {
        let mut row_sum = 0.0f64;
        for x in 0..width {
            row_sum += src[y * width + x] as f64;
            integral[(y + 1) * stride + x + 1] = integral[y * stride + x + 1] + row_sum;
        }
    }
    let mut out = vec![0.0f32; src.len()];
    for y in 0..height {
        let y0 = y.saturating_sub(radius);
        let y1 = (y + radius).min(height - 1);
        for x in 0..width {
            let x0 = x.saturating_sub(radius);
            let x1 = (x + radius).min(width - 1);
            let sum = integral[(y1 + 1) * stride + x1 + 1]
                - integral[y0 * stride + x1 + 1]
                - integral[(y1 + 1) * stride + x0]
                + integral[y0 * stride + x0];
            let count = (x1 - x0 + 1) * (y1 - y0 + 1);
            out[y * width + x] = (sum / count as f64) as f32;
        }
    }
    out
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

        // 正对比度压暗阴影、提亮高光，但不把端点推走；18% 中灰仍是锚点。
        let mid = [0.18f32; 3];
        let dark = [0.01f32; 3];
        let bright = [0.7f32; 3];
        let base = ToneStage::from_params(&EditParams::default());
        let mid_out = stage.apply(mid, base.self_base_ev(mid));
        let dark_out = stage.apply(dark, base.self_base_ev(dark));
        let bright_out = stage.apply(bright, base.self_base_ev(bright));
        assert!((mid_out[0] - 0.18).abs() < 0.01, "中灰应保持：{mid_out:?}");
        assert!(dark_out[0] < dark[0], "{dark:?} -> {dark_out:?}");
        assert!(bright_out[0] > bright[0], "{bright:?} -> {bright_out:?}");
    }

    #[test]
    fn exposure_is_pure_linear_gain() {
        // 曝光走场景线性纯增益（darktable 模型），不进色调段、不叠显示域曲线。
        let mut params = EditParams::default();
        params.light.exposure = 3.0;
        let stage = ToneStage::from_params(&params);
        assert!(stage.is_identity(), "曝光不该进入色调段");
        let out = crate::pipeline::apply_reference([0.1, 0.05, 0.02], &params);
        assert!((out[0] - 0.8).abs() < 1e-5, "{out:?}");
        assert!((out[1] - 0.4).abs() < 1e-5, "{out:?}");
        assert!((out[2] - 0.16).abs() < 1e-5, "{out:?}");
    }

    #[test]
    fn brightening_sliders_keep_chromaticity() {
        // 局部曝光只乘亮度增益：线性通道比例（色相 / 饱和度）不能被改变。
        let mut image = RgbaImage::new(48, 48);
        for pixel in image.pixels_mut() {
            *pixel = image::Rgba([64, 32, 16, 255]);
        }
        let mut params = EditParams::default();
        params.light.shadows = 100.0;
        apply_local_tone(&mut image, &params);
        let after = image.get_pixel(24, 24).0;
        let before_ratio = srgb_to_linear(32.0 / 255.0) / srgb_to_linear(64.0 / 255.0);
        let after_ratio =
            srgb_to_linear(after[1] as f32 / 255.0) / srgb_to_linear(after[0] as f32 / 255.0);
        assert!(
            (before_ratio - after_ratio).abs() < 0.02,
            "线性通道比例应保持：{before_ratio} -> {after_ratio}（{after:?}）"
        );
    }

    #[test]
    fn local_highlights_target_bright_regions() {
        let mut image = split_image(64, 64, 32, 32, 224);
        let before_left = image.get_pixel(8, 32).0[0];
        let before_right = image.get_pixel(56, 32).0[0];
        let mut params = EditParams::default();
        params.light.highlights = -100.0;
        apply_local_tone(&mut image, &params);
        let after_left = image.get_pixel(8, 32).0[0];
        let after_right = image.get_pixel(56, 32).0[0];
        assert!(
            after_right + 12 < before_right,
            "高光 -100 应压暗亮区：{before_right} -> {after_right}"
        );
        assert!(
            after_left.abs_diff(before_left) <= 2,
            "高光 -100 不应明显改变暗区：{before_left} -> {after_left}"
        );
    }

    #[test]
    fn local_shadows_target_dark_regions() {
        let mut image = split_image(64, 64, 32, 32, 224);
        let before_left = image.get_pixel(8, 32).0[0];
        let before_right = image.get_pixel(56, 32).0[0];
        let mut params = EditParams::default();
        params.light.shadows = 100.0;
        apply_local_tone(&mut image, &params);
        let after_left = image.get_pixel(8, 32).0[0];
        let after_right = image.get_pixel(56, 32).0[0];
        assert!(
            after_left > before_left + 12,
            "阴影 +100 应提亮暗区：{before_left} -> {after_left}"
        );
        assert!(
            after_right.abs_diff(before_right) <= 4,
            "阴影 +100 不应明显改变亮区：{before_right} -> {after_right}"
        );
    }

    #[test]
    fn local_tone_leaves_midgray_and_black_alone() {
        let mut image = RgbaImage::new(32, 32);
        for pixel in image.pixels_mut() {
            *pixel = image::Rgba([128, 128, 128, 255]);
        }
        let mut params = EditParams::default();
        params.light.shadows = 100.0;
        params.light.highlights = -100.0;
        apply_local_tone(&mut image, &params);
        let mid = image.get_pixel(16, 16).0[0];
        assert!(mid.abs_diff(128) <= 2, "中灰不应被局部影调推走：{mid}");

        let mut black = RgbaImage::new(32, 32);
        for pixel in black.pixels_mut() {
            *pixel = image::Rgba([0, 0, 0, 255]);
        }
        apply_local_tone(&mut black, &params);
        assert_eq!(black.get_pixel(16, 16).0[0], 0, "纯黑乘增益仍为黑");
    }

    fn split_image(width: u32, height: u32, split: u32, dark: u8, bright: u8) -> RgbaImage {
        let mut image = RgbaImage::new(width, height);
        for (x, _, pixel) in image.enumerate_pixels_mut() {
            let value = if x < split { dark } else { bright };
            *pixel = image::Rgba([value, value, value, 255]);
        }
        image
    }

    #[test]
    fn local_base_tracks_brightness_regions() {
        let image = split_image(128, 128, 64, 24, 240);
        let base = LocalToneBase::build(&image).expect("base");
        let dark_ev = base.ev_at(8, 64);
        let bright_ev = base.ev_at(120, 64);
        assert!(dark_ev < -2.0, "暗区基底 EV 应明显为负：{dark_ev}");
        assert!(bright_ev > 1.0, "亮区基底 EV 应明显为正：{bright_ev}");
    }

    fn tone_out(stage: &ToneStage, y: f32) -> f32 {
        stage.apply([y; 3], 0.0)[0]
    }

    #[test]
    fn whites_and_blacks_follow_reference_curve() {
        let mut whites = EditParams::default();
        whites.light.whites = -100.0;
        let stage = ToneStage::from_params(&whites);
        assert!((tone_out(&stage, 1.0) - 1.0).abs() < 1e-5);
        assert!(tone_out(&stage, 0.75) < 0.75);
        assert!(tone_out(&stage, 0.5) < 0.5);

        let mut blacks = EditParams::default();
        blacks.light.blacks = 100.0;
        let stage = ToneStage::from_params(&blacks);
        assert!((tone_out(&stage, 0.0)).abs() < 1e-5);
        assert!(tone_out(&stage, 0.03) > 0.03);
        assert!(tone_out(&stage, 0.5) > 0.5);

        blacks.light.blacks = -100.0;
        let stage = ToneStage::from_params(&blacks);
        assert!(tone_out(&stage, 0.1) < 0.1);
        assert!((tone_out(&stage, 1.0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn reference_luts_are_monotone_at_every_stop() {
        for value in (-100..=100).step_by(25) {
            let value = value as f32;
            for curves in [
                &HIGHLIGHT_CURVES,
                &SHADOW_CURVES,
                &WHITE_CURVES,
                &BLACK_CURVES,
            ] {
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
