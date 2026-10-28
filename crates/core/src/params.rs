//! 调色参数模型：完整覆盖 Lightroom 移动端编辑面板。
//!
//! 参数树与 `.fdxmp` 预设一一对应（`PresetFile::params`），
//! 所有字段都参与 serde 序列化，缺省字段自动回落到默认值。

use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;

/// 参数模型版本（写入 `.fdxmp` 的 `params.version`）。
pub const PARAM_VERSION: u32 = 1;

/// 全局编辑参数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditParams {
    /// 参数模型版本。
    pub version: u32,
    /// 光照。
    pub light: LightParams,
    /// 颜色。
    pub color: ColorParams,
    /// 效果。
    pub effects: EffectParams,
    /// 曲线。
    pub curves: CurveParams,
    /// HSL。
    pub hsl: HslParams,
    /// 颜色分级。
    pub grading: ColorGradingParams,
    /// 相机校准（Calibration 面板：三基色色相 / 饱和度 + 阴影色调）。
    pub calibration: CalibrationParams,
    /// 颜色均衡器（8 色相节点的饱和度 / 色相 / 亮度，dt UCS 空间，空间算子）。
    pub equalizer: EqualizerParams,
    /// 黑白混色（8 色带，仅在黑白模式下生效）。
    pub bw_mix: BwMixParams,
    /// 点颜色采样（Point Color，最多 4 个，与 GPU uniform 槽位一致）。
    pub point_colors: Vec<PointColorParams>,
    /// 细节（锐化 / 降噪）。
    pub detail: DetailParams,
    /// 镜头校正。
    pub lens: LensParams,
    /// 变换。
    pub transform: TransformParams,
    /// 裁剪。
    pub crop: CropParams,
    /// AI 修图（v1.5.0：去瑕疵 / 磨皮 / 肤色 / 塑形；与调色互不影响）。
    pub retouch: crate::retouch::RetouchParams,
    /// 蒙版列表（局部调整）。
    pub masks: Vec<MaskParams>,
}

impl Default for EditParams {
    fn default() -> Self {
        Self {
            version: PARAM_VERSION,
            light: LightParams::default(),
            color: ColorParams::default(),
            effects: EffectParams::default(),
            curves: CurveParams::default(),
            hsl: HslParams::default(),
            grading: ColorGradingParams::default(),
            calibration: CalibrationParams::default(),
            equalizer: EqualizerParams::default(),
            bw_mix: BwMixParams::default(),
            point_colors: Vec::new(),
            detail: DetailParams::default(),
            lens: LensParams::default(),
            transform: TransformParams::default(),
            crop: CropParams::default(),
            retouch: crate::retouch::RetouchParams::default(),
            masks: Vec::new(),
        }
    }
}

/// 光照面板（曝光 → 黑色）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LightParams {
    /// 曝光，单位 EV，范围 -5.0..=5.0。
    pub exposure: f32,
    /// 对比度，-100..=100。
    pub contrast: f32,
    /// 高光，-100..=100。
    pub highlights: f32,
    /// 阴影，-100..=100。
    pub shadows: f32,
    /// 白色，-100..=100。
    pub whites: f32,
    /// 黑色，-100..=100。
    pub blacks: f32,
}

/// 颜色面板（色温 / 色调 / 自然饱和度 / 饱和度）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorParams {
    /// 色温，-100..=100（相对值：0 = As Shot；暖端 1 单位 = 90K、冷端 23K，
    /// 与 XMP 导入的开尔文映射共用同一换算，见 `white_balance` 模块）。
    pub temperature: f32,
    /// 当前照片的 As Shot 参考色温（K）。0 = 5500K 兜底。
    /// 色温 0 时白平衡矩阵恰为恒等，因此打开照片不会改变画面。
    #[serde(default = "default_wb_reference")]
    pub wb_reference_kelvin: f32,
    /// 黑白模式：打开后按 `bw_mix` 的 8 色带权重转单色（Lightroom 的 B&W 处理方式），
    /// 色相 / 饱和度混色与点颜色在该模式下不生效。
    pub black_white: bool,
    /// 色调，-150..=150（正 = 洋红，负 = 绿色；与 LR 的 Tint 范围一致）。
    pub tint: f32,
    /// 自然饱和度，-100..=100。
    pub vibrance: f32,
    /// 饱和度，-100..=100。
    pub saturation: f32,
}

fn default_wb_reference() -> f32 {
    5500.0
}

/// 颜色均衡器：按色相分 8 段调整饱和度 / 色相 / 亮度（红橙黄绿青蓝紫品红）。
///
/// 数学实现见 `fdxmp_color::equalizer`（dt UCS 2022 + 周期 RBF + 导向滤波），
/// 与 darktable 5.6.2 对拍的平均误差 0.15/255。它是**空间算子**（需要邻域），
/// 不能塞进逐像素的 `ColorOps`，因此单独走整帧的一遍。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqualizerParams {
    /// 总开关（默认关闭，关闭时不产生任何开销）。
    pub enabled: bool,
    /// 低饱和抑制拐点，0..=0.3（默认 0.1 ≈ 6% 饱和度）。
    pub threshold: f32,
    /// 色相曲线过渡宽度，0.05..=2.0（越小越窄）。
    pub smoothing_hue: f32,
    /// 低饱和过渡软硬，-1..=1（越负越软）。
    pub contrast: f32,
    /// 白点亮度的 2 的幂，默认 1.0（漫反射白）。
    pub white_level: f32,
    /// 色度导向滤波半径（1..=10）。
    pub chroma_size: f32,
    /// 修正量导向滤波半径（1..=128）。
    pub param_size: f32,
    /// 是否启用两个导向滤波（关掉更快，但彩噪与光晕会回来）。
    pub use_filter: bool,
    /// 8 个节点整体旋转，-23..=23 度。
    pub hue_shift: f32,
    /// 饱和度增益（1.0 = 不变）。
    pub saturation: [f32; 8],
    /// 色相偏移（度）。
    pub hue: [f32; 8],
    /// 亮度增益（1.0 = 不变）。
    pub brightness: [f32; 8],
}

impl Default for EqualizerParams {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: 0.1,
            smoothing_hue: 1.0,
            contrast: 0.0,
            white_level: 1.0,
            chroma_size: 1.5,
            param_size: 1.0,
            // GPU 版（WGSL）目前只做逐像素 + 5x5 预滤波近似，关掉导向滤波可让
            // 预览、导出、CPU 参考三边逐像素一致（实测最大 1 个 8bit LSB）。
            // 需要 darktable 那种彩噪抑制时可在面板里打开（GPU 用近似实现）。
            use_filter: false,
            hue_shift: 0.0,
            saturation: [1.0; 8],
            hue: [0.0; 8],
            brightness: [1.0; 8],
        }
    }
}

impl EqualizerParams {
    /// 转成算法 crate 的参数（去掉 `enabled`）。
    pub fn to_engine(&self) -> fdxmp_color::EqualizerParams {
        fdxmp_color::EqualizerParams {
            threshold: self.threshold,
            smoothing_hue: self.smoothing_hue,
            contrast: self.contrast,
            white_level: self.white_level,
            chroma_size: self.chroma_size,
            param_size: self.param_size,
            use_filter: self.use_filter,
            saturation: self.saturation,
            hue: self.hue,
            brightness: self.brightness,
            hue_shift: self.hue_shift,
        }
    }

    /// 打开且参数不为恒等时才需要真的跑。
    pub fn is_active(&self) -> bool {
        self.enabled && !self.to_engine().is_identity()
    }

    /// 复位到默认（保留 `enabled`）。
    pub fn reset(&mut self) {
        let enabled = self.enabled;
        *self = Self::default();
        self.enabled = enabled;
    }
}

impl Default for ColorParams {
    fn default() -> Self {
        Self {
            temperature: 0.0,
            wb_reference_kelvin: default_wb_reference(),
            black_white: false,
            tint: 0.0,
            vibrance: 0.0,
            saturation: 0.0,
        }
    }
}

/// 效果面板（纹理 → 颗粒）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectParams {
    /// 纹理，-100..=100。
    pub texture: f32,
    /// 清晰度，-100..=100。
    pub clarity: f32,
    /// 去雾，-100..=100。
    pub dehaze: f32,
    /// 暗角，-100..=100。
    pub vignette: f32,
    /// 暗角中点，0..=100（越小，压暗范围越向画面中心扩展）。
    pub vignette_midpoint: f32,
    /// 暗角羽化，0..=100（越大过渡越柔和）。
    pub vignette_feather: f32,
    /// 颗粒，0..=100。
    pub grain: f32,
    /// 颗粒大小，0..=100（0 = 逐像素细颗粒，100 = 约 8px 粗颗粒）。
    pub grain_size: f32,
    /// 颗粒粗糙度，0..=100（0 = 平滑，100 = 逐像素粗糙）。
    pub grain_roughness: f32,
}

impl Default for EffectParams {
    fn default() -> Self {
        Self {
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
            vignette: 0.0,
            // 中点 / 羽化默认 50 与改造前观感一致
            vignette_midpoint: 50.0,
            vignette_feather: 50.0,
            grain: 0.0,
            grain_size: 0.0,
            grain_roughness: 50.0,
        }
    }
}

/// 曲线上的一个控制点（归一化坐标）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    /// 输入，0.0..=1.0。
    pub x: f32,
    /// 输出，0.0..=1.0。
    pub y: f32,
}

/// 单条色调曲线。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToneCurve {
    /// 控制点，按 x 升序。
    pub points: Vec<CurvePoint>,
}

impl Default for ToneCurve {
    fn default() -> Self {
        Self {
            points: vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.0, y: 1.0 }],
        }
    }
}

impl ToneCurve {
    /// 曲线是否为恒等映射（在 0 / 0.25 / 0.5 / 0.75 / 1 处采样判断）。
    ///
    /// 默认两点式曲线恒等；手绘的直线曲线也会被识别为恒等，
    /// 用于蒙版增量合成时避免无意义的串联。
    pub fn is_identity(&self) -> bool {
        const SAMPLES: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
        SAMPLES
            .iter()
            .all(|&x| (eval_curve(&self.points, x) - x).abs() < 1e-4)
    }
}

/// 在控制点曲线上取值（**单调三次 Hermite / Fritsch–Carlson**，与 GPU LUT 构建一致）。
///
/// 与线性插值的区别：曲线严格过控制点、一阶连续（控制点处不再有折线感）；
/// 结点斜率做过三倍限幅，不会过冲（色调曲线不会出现反相 / 光晕）。
/// `points` 为空时按恒等处理；`x` 小于首个控制点时取首点值，大于末点时取末点值。
pub fn eval_curve(points: &[CurvePoint], x: f32) -> f32 {
    match points.len() {
        0 => return x,
        1 => return points[0].y,
        _ => {}
    }
    if x <= points[0].x {
        return points[0].y;
    }
    let last = &points[points.len() - 1];
    if x >= last.x {
        return last.y;
    }
    // 定位区间：index 满足 points[index].x <= x < points[index + 1].x
    let mut index = 0usize;
    while index + 2 < points.len() && x > points[index + 1].x {
        index += 1;
    }
    let left = points[index];
    let right = points[index + 1];
    let span = (right.x - left.x).max(1e-6);
    let t = ((x - left.x) / span).clamp(0.0, 1.0);
    let m0 = curve_tangent(points, index);
    let m1 = curve_tangent(points, index + 1);
    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    let value = h00 * left.y + h10 * span * m0 + h01 * right.y + h11 * span * m1;
    value.clamp(0.0, 1.0)
}

/// Fritsch–Carlson 单调三次 Hermite 的结点斜率（含三倍限幅）。
fn curve_tangent(points: &[CurvePoint], index: usize) -> f32 {
    let secant = |i: usize| {
        let span = (points[i + 1].x - points[i].x).max(1e-6);
        (points[i + 1].y - points[i].y) / span
    };
    if index == 0 {
        return secant(0);
    }
    if index + 1 >= points.len() {
        return secant(points.len() - 2);
    }
    let previous = secant(index - 1);
    let next = secant(index);
    if previous * next <= 0.0 {
        // 极值点：斜率为 0，保证单调段不被跨过
        return 0.0;
    }
    let slope = (previous + next) * 0.5;
    let limit = 3.0 * previous.abs().min(next.abs());
    slope.clamp(-limit, limit)
}

/// 曲线串联：先全局后本地（`local(global(x))`）。
fn compose_tone_curve(global: &ToneCurve, local: &ToneCurve) -> ToneCurve {
    if local.is_identity() {
        return global.clone();
    }
    if global.is_identity() {
        return local.clone();
    }
    let points: Vec<CurvePoint> = (0..=256)
        .map(|index| {
            let x = index as f32 / 256.0;
            let y = eval_curve(&local.points, eval_curve(&global.points, x));
            CurvePoint {
                x,
                y: y.clamp(0.0, 1.0),
            }
        })
        .collect();
    ToneCurve { points }
}

/// 颜色分级单区合成：色相加法取模，饱和度 / 明亮度加法夹取。
fn compose_grading_zone(global: GradingZone, local: GradingZone) -> GradingZone {
    GradingZone {
        hue: (global.hue + local.hue).rem_euclid(360.0),
        saturation: (global.saturation + local.saturation).clamp(0.0, 100.0),
        luminance: (global.luminance + local.luminance).clamp(-100.0, 100.0),
    }
}

/// 参数曲线（Lightroom 四区：高光 / 亮调 / 暗调 / 阴影）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ParametricCurve {
    /// 高光，-100..=100。
    pub highlights: f32,
    /// 亮调，-100..=100。
    pub lights: f32,
    /// 暗调，-100..=100；旧预设的 `midtones` 反序列化到这里。
    #[serde(alias = "midtones")]
    pub darks: f32,
    /// 阴影，-100..=100。
    pub shadows: f32,
    /// 高光区起点，0.0..=1.0；0 表示默认 0.75。
    #[serde(alias = "highlights_split")]
    pub highlight_split: f32,
    /// 亮调 / 暗调分界，0.0..=1.0；0 表示默认 0.50。
    #[serde(alias = "midtones_split")]
    pub midtone_split: f32,
    /// 阴影区终点，0.0..=1.0；0 表示默认 0.25。
    pub shadow_split: f32,
}

/// 参数曲线四个区域在 ±100 时的最大位移（显示域，1.0 = 255 级）。
///
/// 由 LR 参考图 `param_*` 的峰值拟合：阴影 / 高光比亮调 / 暗调弱。
pub const PARAM_CURVE_AMOUNTS: [f32; 4] = [0.14, 0.30, 0.32, 0.16];

/// 参数曲线 LUT 条目数（与 GPU `CURVE_ENTRIES` 一致，x = i / (N-1)）。
pub const PARAM_CURVE_ENTRIES: usize = 256;

const PARAM_CURVE_POINTS: usize = 17;
const PARAM_IDENTITY_CURVE: [f32; PARAM_CURVE_POINTS] = [
    0.0000, 0.0625, 0.1250, 0.1875, 0.2500, 0.3125, 0.3750, 0.4375, 0.5000, 0.5625, 0.6250, 0.6875,
    0.7500, 0.8125, 0.8750, 0.9375, 1.0000,
];

/// LR 参考：参数曲线阴影 -100 / -50 / 0 / +50 / +100。
#[allow(clippy::approx_constant)]
pub const PARAM_SHADOWS_CURVES: [[f32; PARAM_CURVE_POINTS]; 5] = [
    [
        0.0000, 0.0002, 0.0139, 0.0654, 0.1400, 0.2336, 0.3326, 0.4201, 0.4955, 0.5609, 0.6251,
        0.6875, 0.7505, 0.8125, 0.8746, 0.9372, 1.0000,
    ],
    [
        0.0000, 0.0132, 0.0628, 0.1280, 0.2009, 0.2801, 0.3583, 0.4300, 0.4983, 0.5617, 0.6252,
        0.6875, 0.7505, 0.8125, 0.8746, 0.9372, 1.0000,
    ],
    PARAM_IDENTITY_CURVE,
    [
        0.0000, 0.0963, 0.1720, 0.2386, 0.2933, 0.3435, 0.3927, 0.4437, 0.5022, 0.5629, 0.6254,
        0.6874, 0.7505, 0.8125, 0.8746, 0.9371, 1.0000,
    ],
    [
        0.0000, 0.1310, 0.2261, 0.3044, 0.3536, 0.3855, 0.4152, 0.4525, 0.5047, 0.5636, 0.6254,
        0.6874, 0.7505, 0.8125, 0.8746, 0.9371, 1.0000,
    ],
];

/// LR 参考：参数曲线暗调 -100 / -50 / 0 / +50 / +100。
#[allow(clippy::approx_constant)]
pub const PARAM_DARKS_CURVES: [[f32; PARAM_CURVE_POINTS]; 5] = [
    [
        0.0000, 0.0000, 0.0015, 0.0074, 0.0267, 0.0700, 0.1351, 0.2111, 0.2974, 0.3892, 0.4828,
        0.5819, 0.6793, 0.7704, 0.8546, 0.9322, 1.0000,
    ],
    [
        0.0000, 0.0062, 0.0308, 0.0757, 0.1304, 0.1960, 0.2676, 0.3390, 0.4144, 0.4898, 0.5666,
        0.6432, 0.7203, 0.7943, 0.8659, 0.9348, 1.0000,
    ],
    PARAM_IDENTITY_CURVE,
    [
        0.0000, 0.0977, 0.1783, 0.2567, 0.3303, 0.4016, 0.4673, 0.5261, 0.5817, 0.6329, 0.6828,
        0.7305, 0.7794, 0.8295, 0.8827, 0.9391, 1.0000,
    ],
    [
        0.0000, 0.1313, 0.2311, 0.3298, 0.4234, 0.5127, 0.5889, 0.6499, 0.6978, 0.7337, 0.7636,
        0.7904, 0.8194, 0.8526, 0.8933, 0.9421, 1.0000,
    ],
];

/// LR 参考：参数曲线亮调 -100 / -50 / 0 / +50 / +100。
#[allow(clippy::approx_constant)]
pub const PARAM_LIGHTS_CURVES: [[f32; PARAM_CURVE_POINTS]; 5] = [
    [
        0.0000, 0.0612, 0.1181, 0.1680, 0.2072, 0.2395, 0.2664, 0.2917, 0.3226, 0.3597, 0.4051,
        0.4613, 0.5333, 0.6161, 0.7159, 0.8385, 1.0000,
    ],
    [
        0.0000, 0.0622, 0.1225, 0.1797, 0.2319, 0.2823, 0.3299, 0.3754, 0.4232, 0.4722, 0.5251,
        0.5824, 0.6480, 0.7190, 0.7995, 0.8923, 1.0000,
    ],
    PARAM_IDENTITY_CURVE,
    [
        0.0000, 0.0638, 0.1289, 0.1962, 0.2667, 0.3435, 0.4244, 0.5046, 0.5888, 0.6706, 0.7504,
        0.8227, 0.8852, 0.9347, 0.9692, 0.9908, 1.0000,
    ],
    [
        0.0000, 0.0648, 0.1335, 0.2062, 0.2890, 0.3860, 0.4953, 0.6052, 0.7163, 0.8129, 0.8943,
        0.9518, 0.9810, 0.9949, 0.9993, 0.9999, 1.0000,
    ],
];

/// LR 参考：参数曲线高光 -100 / -50 / 0 / +50 / +100。
#[allow(clippy::approx_constant)]
pub const PARAM_HIGHLIGHTS_CURVES: [[f32; PARAM_CURVE_POINTS]; 5] = [
    [
        0.0000, 0.0630, 0.1253, 0.1880, 0.2495, 0.3127, 0.3759, 0.4371, 0.4972, 0.5449, 0.5791,
        0.6045, 0.6319, 0.6723, 0.7407, 0.8445, 1.0000,
    ],
    [
        0.0000, 0.0630, 0.1253, 0.1879, 0.2494, 0.3127, 0.3759, 0.4371, 0.4991, 0.5548, 0.6056,
        0.6517, 0.6988, 0.7510, 0.8156, 0.8961, 1.0000,
    ],
    PARAM_IDENTITY_CURVE,
    [
        0.0000, 0.0630, 0.1253, 0.1878, 0.2491, 0.3125, 0.3759, 0.4372, 0.5018, 0.5699, 0.6460,
        0.7254, 0.8060, 0.8794, 0.9397, 0.9823, 1.0000,
    ],
    [
        0.0000, 0.0630, 0.1252, 0.1878, 0.2491, 0.3123, 0.3758, 0.4372, 0.5033, 0.5799, 0.6751,
        0.7790, 0.8767, 0.9498, 0.9861, 0.9989, 1.0000,
    ],
];

fn sample_param_curve(curve: &[f32; PARAM_CURVE_POINTS], x: f32) -> f32 {
    let position = x.clamp(0.0, 1.0) * (PARAM_CURVE_POINTS - 1) as f32;
    let index = (position.floor() as usize).min(PARAM_CURVE_POINTS - 1);
    let next = (index + 1).min(PARAM_CURVE_POINTS - 1);
    let fraction = position - index as f32;
    curve[index] * (1.0 - fraction) + curve[next] * fraction
}

fn param_reference_lut(
    curves: &[[f32; PARAM_CURVE_POINTS]; 5],
    value: f32,
) -> [f32; PARAM_CURVE_ENTRIES] {
    let position = ((value.clamp(-100.0, 100.0) + 100.0) / 50.0).clamp(0.0, 4.0);
    let low = position.floor() as usize;
    let high = (low + 1).min(4);
    let fraction = position - low as f32;
    std::array::from_fn(|index| {
        let x = index as f32 / (PARAM_CURVE_ENTRIES - 1) as f32;
        let a = sample_param_curve(&curves[low], x);
        let b = sample_param_curve(&curves[high], x);
        (a + (b - a) * fraction).clamp(0.0, 1.0)
    })
}

fn sample_param_lut(lut: &[f32; PARAM_CURVE_ENTRIES], x: f32) -> f32 {
    let position = x.clamp(0.0, 1.0) * (PARAM_CURVE_ENTRIES - 1) as f32;
    let index = (position.floor() as usize).min(PARAM_CURVE_ENTRIES - 1);
    let next = (index + 1).min(PARAM_CURVE_ENTRIES - 1);
    let fraction = position - index as f32;
    lut[index] * (1.0 - fraction) + lut[next] * fraction
}

impl ParametricCurve {
    /// 三个分界的实际值；0 表示默认 0.25 / 0.50 / 0.75。
    pub fn effective_splits(&self) -> [f32; 3] {
        let shadow = if self.shadow_split <= 0.0 {
            0.25
        } else {
            self.shadow_split.clamp(0.05, 0.7)
        };
        let midtone = if self.midtone_split <= 0.0 {
            0.5
        } else {
            self.midtone_split.clamp(shadow + 0.05, 0.85)
        };
        let highlight = if self.highlight_split <= 0.0 {
            0.75
        } else {
            self.highlight_split.clamp(midtone + 0.05, 0.95)
        };
        [shadow, midtone, highlight]
    }

    /// 四个区域滑杆全为 0（分界不改变恒等曲线）。
    pub fn is_identity(&self) -> bool {
        self.highlights == 0.0
            && self.lights == 0.0
            && self.darks == 0.0
            && self.shadows == 0.0
    }

    /// 分界是否为 LR 默认 0.25 / 0.50 / 0.75（含旧的 0 表示默认）。
    fn uses_default_splits(&self) -> bool {
        let near = |value: f32, default: f32| value <= 0.0 || (value - default).abs() < 1e-4;
        near(self.shadow_split, 0.25)
            && near(self.midtone_split, 0.5)
            && near(self.highlight_split, 0.75)
    }
}

/// 参数曲线 → 显示域 LUT。
///
/// 默认分界（0.25 / 0.50 / 0.75）下，直接使用 LR 参考的四个滑块 1D 曲线，
/// 依次复合；这是单滑块最准确的路径。用户改分界后回退到四区单调样条，
/// 保留“分界可调”的行为。
pub fn parametric_curve_lut(curve: &ParametricCurve) -> [f32; PARAM_CURVE_ENTRIES] {
    if !curve.uses_default_splits() {
        return parametric_curve_spline_lut(curve);
    }

    let mut lut: [f32; PARAM_CURVE_ENTRIES] = std::array::from_fn(|index| {
        index as f32 / (PARAM_CURVE_ENTRIES - 1) as f32
    });
    for (curves, amount) in [
        (&PARAM_SHADOWS_CURVES, curve.shadows),
        (&PARAM_DARKS_CURVES, curve.darks),
        (&PARAM_LIGHTS_CURVES, curve.lights),
        (&PARAM_HIGHLIGHTS_CURVES, curve.highlights),
    ] {
        let slider = param_reference_lut(curves, amount);
        lut = std::array::from_fn(|index| sample_param_lut(&slider, lut[index]));
    }
    lut
}

/// 四区控制点样条（自定义分界时使用）。
fn parametric_curve_spline_lut(curve: &ParametricCurve) -> [f32; PARAM_CURVE_ENTRIES] {
    let [shadow_split, midtone_split, highlight_split] = curve.effective_splits();
    let centers = [
        shadow_split * 0.5,
        (shadow_split + midtone_split) * 0.5,
        (midtone_split + highlight_split) * 0.5,
        highlight_split + (1.0 - highlight_split) * 0.5,
    ];
    let amounts = [curve.shadows, curve.darks, curve.lights, curve.highlights];
    let mut values = [0.0f32; 4];
    for index in 0..4 {
        values[index] = (centers[index]
            + amounts[index] / 100.0 * PARAM_CURVE_AMOUNTS[index])
            .clamp(0.0, 1.0);
    }
    // 不允许控制点纵向交叉，否则样条会出现局部反相。
    for index in 1..4 {
        values[index] = values[index].max(values[index - 1] + 0.002);
    }
    for index in (0..3).rev() {
        values[index] = values[index].min(values[index + 1] - 0.002);
    }
    let points = [
        CurvePoint { x: 0.0, y: 0.0 },
        CurvePoint {
            x: centers[0],
            y: values[0],
        },
        CurvePoint {
            x: centers[1],
            y: values[1],
        },
        CurvePoint {
            x: centers[2],
            y: values[2],
        },
        CurvePoint {
            x: centers[3],
            y: values[3],
        },
        CurvePoint { x: 1.0, y: 1.0 },
    ];
    std::array::from_fn(|index| {
        let x = index as f32 / (PARAM_CURVE_ENTRIES - 1) as f32;
        eval_curve(&points, x)
    })
}

/// 曲线面板。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CurveParams {
    /// RGB 主通道。
    pub rgb: ToneCurve,
    /// 红通道。
    pub red: ToneCurve,
    /// 绿通道。
    pub green: ToneCurve,
    /// 蓝通道。
    pub blue: ToneCurve,
    /// 参数曲线。
    pub parametric: ParametricCurve,
}

/// 8 个 HSL 颜色通道。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorBand {
    /// 红。
    Red,
    /// 橙。
    Orange,
    /// 黄。
    Yellow,
    /// 绿。
    Green,
    /// 青。
    Aqua,
    /// 蓝。
    Blue,
    /// 紫。
    Purple,
    /// 洋红。
    Magenta,
}

impl ColorBand {
    /// 文档规定的通道顺序。
    pub const ALL: [ColorBand; 8] = [
        ColorBand::Red,
        ColorBand::Orange,
        ColorBand::Yellow,
        ColorBand::Green,
        ColorBand::Aqua,
        ColorBand::Blue,
        ColorBand::Purple,
        ColorBand::Magenta,
    ];

    /// 中文标签。
    pub fn label_zh(self) -> &'static str {
        match self {
            ColorBand::Red => "红",
            ColorBand::Orange => "橙",
            ColorBand::Yellow => "黄",
            ColorBand::Green => "绿",
            ColorBand::Aqua => "青",
            ColorBand::Blue => "蓝",
            ColorBand::Purple => "紫",
            ColorBand::Magenta => "洋红",
        }
    }

    /// 英文标签。
    pub fn label_en(self) -> &'static str {
        match self {
            ColorBand::Red => "Red",
            ColorBand::Orange => "Orange",
            ColorBand::Yellow => "Yellow",
            ColorBand::Green => "Green",
            ColorBand::Aqua => "Aqua",
            ColorBand::Blue => "Blue",
            ColorBand::Purple => "Purple",
            ColorBand::Magenta => "Magenta",
        }
    }
}

/// 单通道 HSL。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct HslBand {
    /// 色相，-100..=100。
    pub hue: f32,
    /// 饱和度，-100..=100。
    pub saturation: f32,
    /// 明亮度，-100..=100。
    pub luminance: f32,
}

/// HSL 面板（Lightroom 8 段）。
///
/// 算法已换成颜色均衡器那套（dt UCS + Lightroom 色带中心的周期 RBF 曲线，
/// 逐像素、无空间滤波）；「连续色域」模式已删除，需要连续曲线请用颜色均衡器。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct HslParams {
    /// 红。
    pub red: HslBand,
    /// 橙。
    pub orange: HslBand,
    /// 黄。
    pub yellow: HslBand,
    /// 绿。
    pub green: HslBand,
    /// 青。
    pub aqua: HslBand,
    /// 蓝。
    pub blue: HslBand,
    /// 紫。
    pub purple: HslBand,
    /// 洋红。
    pub magenta: HslBand,
}

impl HslParams {
    /// 按 [`ColorBand`] 读取。
    pub fn band(&self, band: ColorBand) -> &HslBand {
        match band {
            ColorBand::Red => &self.red,
            ColorBand::Orange => &self.orange,
            ColorBand::Yellow => &self.yellow,
            ColorBand::Green => &self.green,
            ColorBand::Aqua => &self.aqua,
            ColorBand::Blue => &self.blue,
            ColorBand::Purple => &self.purple,
            ColorBand::Magenta => &self.magenta,
        }
    }

    /// 按 [`ColorBand`] 写入。
    pub fn band_mut(&mut self, band: ColorBand) -> &mut HslBand {
        match band {
            ColorBand::Red => &mut self.red,
            ColorBand::Orange => &mut self.orange,
            ColorBand::Yellow => &mut self.yellow,
            ColorBand::Green => &mut self.green,
            ColorBand::Aqua => &mut self.aqua,
            ColorBand::Blue => &mut self.blue,
            ColorBand::Purple => &mut self.purple,
            ColorBand::Magenta => &mut self.magenta,
        }
    }
}

/// 颜色分级的一个区域（阴影 / 中间调 / 高光）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GradingZone {
    /// 色相，0..=360。
    pub hue: f32,
    /// 饱和度，0..=100。
    pub saturation: f32,
    /// 明亮度，-100..=100。
    pub luminance: f32,
}

/// 颜色分级面板。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorGradingParams {
    /// 阴影。
    pub shadows: GradingZone,
    /// 中间调。
    pub midtones: GradingZone,
    /// 高光。
    pub highlights: GradingZone,
    /// 全局。
    pub global: GradingZone,
    /// 混合，0..=100。
    pub blending: f32,
    /// 平衡，-100..=100。
    pub balance: f32,
}

impl Default for ColorGradingParams {
    fn default() -> Self {
        Self {
            shadows: GradingZone::default(),
            midtones: GradingZone::default(),
            highlights: GradingZone::default(),
            global: GradingZone::default(),
            // LR 默认 ColorGradeBlending = 50；旧实现用 0，导致中间调权重被挤成 0。
            blending: 50.0,
            balance: 0.0,
        }
    }
}

/// 相机校准（Lightroom 的 Calibration 面板）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CalibrationParams {
    /// 红原色色相，-100..=100。
    pub red_hue: f32,
    /// 红原色饱和度，-100..=100。
    pub red_sat: f32,
    /// 绿原色色相，-100..=100。
    pub green_hue: f32,
    /// 绿原色饱和度，-100..=100。
    pub green_sat: f32,
    /// 蓝原色色相，-100..=100。
    pub blue_hue: f32,
    /// 蓝原色饱和度，-100..=100。
    pub blue_sat: f32,
    /// 阴影色调，-100..=100（正 = 洋红，负 = 绿）。
    pub shadows_tint: f32,
}

impl CalibrationParams {
    /// 是否全部为默认值（默认时 GPU / CPU 都跳过校准段）。
    pub fn is_identity(&self) -> bool {
        self.red_hue == 0.0
            && self.red_sat == 0.0
            && self.green_hue == 0.0
            && self.green_sat == 0.0
            && self.blue_hue == 0.0
            && self.blue_sat == 0.0
            && self.shadows_tint == 0.0
    }

    /// 转成色彩引擎的校准参数。
    pub fn to_engine(&self) -> fdxmp_color::Calibration {
        fdxmp_color::Calibration {
            red_hue: self.red_hue as f64,
            red_sat: self.red_sat as f64,
            green_hue: self.green_hue as f64,
            green_sat: self.green_sat as f64,
            blue_hue: self.blue_hue as f64,
            blue_sat: self.blue_sat as f64,
            shadows_tint: self.shadows_tint as f64,
        }
    }
}

/// 黑白混色（8 色带，-100..=100；正值提亮该色相）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BwMixParams {
    /// 红。
    pub red: f32,
    /// 橙。
    pub orange: f32,
    /// 黄。
    pub yellow: f32,
    /// 绿。
    pub green: f32,
    /// 青。
    pub aqua: f32,
    /// 蓝。
    pub blue: f32,
    /// 紫。
    pub purple: f32,
    /// 洋红。
    pub magenta: f32,
}

impl BwMixParams {
    /// 按 [`ColorBand`] 读取。
    pub fn band(&self, band: ColorBand) -> f32 {
        match band {
            ColorBand::Red => self.red,
            ColorBand::Orange => self.orange,
            ColorBand::Yellow => self.yellow,
            ColorBand::Green => self.green,
            ColorBand::Aqua => self.aqua,
            ColorBand::Blue => self.blue,
            ColorBand::Purple => self.purple,
            ColorBand::Magenta => self.magenta,
        }
    }

    /// 是否全部为默认值。
    pub fn is_neutral(&self) -> bool {
        ColorBand::ALL.iter().all(|band| self.band(*band) == 0.0)
    }

    /// 按带序展开成数组。
    pub fn bands(&self) -> [f32; 8] {
        ColorBand::ALL.map(|band| self.band(band))
    }
}

/// 点颜色采样（Point Color）。
///
/// 颜色字段（`lum` / `chroma` / `hue`）在选中时由取样写入，是 OkLab 值；
/// 其余是调整量与范围，语义与 LightCraft / Lightroom 的点颜色一致。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PointColorParams {
    /// 采样点的 OkLab 明度。
    pub lum: f32,
    /// 采样点的 OkLab 彩度。
    pub chroma: f32,
    /// 采样点的 OkLab 色相（度）。
    pub hue: f32,
    /// 色相偏移，-100..=100。
    pub hue_shift: f32,
    /// 饱和度偏移，-100..=100。
    pub sat_shift: f32,
    /// 明度偏移，-100..=100。
    pub lum_shift: f32,
    /// 向采样点压缩（负）/ 扩散（正），-100..=100。
    pub variance: f32,
    /// 总范围，0..=100。
    pub range: f32,
    /// 色相范围，0..=100。
    pub hue_range: f32,
    /// 饱和度范围，0..=100。
    pub sat_range: f32,
    /// 明度范围，0..=100。
    pub lum_range: f32,
}

impl PointColorParams {
    /// 是否中性（不影响画面）。
    pub fn is_neutral(&self) -> bool {
        self.hue_shift == 0.0
            && self.sat_shift == 0.0
            && self.lum_shift == 0.0
            && self.variance == 0.0
    }

    /// 转成色彩引擎的采样结构。
    pub fn to_engine(&self) -> fdxmp_color::PointColor {
        fdxmp_color::PointColor {
            lum: self.lum as f64,
            chroma: self.chroma as f64,
            hue: self.hue as f64,
            hue_shift: self.hue_shift as f64,
            sat_shift: self.sat_shift as f64,
            lum_shift: self.lum_shift as f64,
            variance: self.variance as f64,
            range: self.range as f64,
            hue_range: self.hue_range as f64,
            sat_range: self.sat_range as f64,
            lum_range: self.lum_range as f64,
        }
    }
}

/// 锐化。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SharpenParams {
    /// 数量，0..=150。
    pub amount: f32,
    /// 半径，0.5..=3.0。
    pub radius: f32,
    /// 细节，0..=100。
    pub detail: f32,
    /// 蒙版，0..=100。
    pub masking: f32,
}

/// 降噪。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct NoiseReductionParams {
    /// 亮度，0..=100。
    pub luminance: f32,
    /// 亮度细节，0..=100。
    pub luminance_detail: f32,
    /// 亮度对比度，0..=100。
    pub luminance_contrast: f32,
    /// 颜色，0..=100。
    pub color: f32,
    /// 颜色细节，0..=100。
    pub color_detail: f32,
    /// 颜色平滑度，0..=100。
    pub color_smoothness: f32,
}

/// 细节面板。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DetailParams {
    /// 锐化。
    pub sharpen: SharpenParams,
    /// 降噪。
    pub noise: NoiseReductionParams,
}

/// 几何校正。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GeometryParams {
    /// 自动校正。
    pub auto: bool,
    /// 畸变，-100..=100。
    pub distortion: f32,
    /// 垂直透视，-100..=100。
    pub vertical: f32,
    /// 水平透视，-100..=100。
    pub horizontal: f32,
    /// 旋转，-45.0..=45.0。
    pub rotate: f32,
    /// 缩放，0..=200。
    pub scale: f32,
}

/// 镜头校正面板。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LensParams {
    /// 启用配置文件校正。
    pub profile_correction: bool,
    /// 色差，-100..=100。
    pub chromatic_aberration: f32,
    /// 晕影（校正），-100..=100。
    pub vignette: f32,
    /// 几何。
    pub geometry: GeometryParams,
}

/// 变换面板。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TransformParams {
    /// 垂直透视，-100..=100。
    pub perspective_vertical: f32,
    /// 水平透视，-100..=100。
    pub perspective_horizontal: f32,
    /// 旋转，-45.0..=45.0。
    pub rotate: f32,
    /// 水平翻转。
    pub flip_horizontal: bool,
    /// 垂直翻转。
    pub flip_vertical: bool,
}

/// 裁剪比例。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AspectRatio {
    /// 自由裁剪。
    #[default]
    Free,
    /// 1:1。
    Square,
    /// 4:3。
    FourThree,
    /// 3:2。
    ThreeTwo,
    /// 16:9。
    SixteenNine,
    /// 原始比例。
    Original,
}

/// 裁剪辅助线。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CropGrid {
    /// 无。
    None,
    /// 三分线。
    #[default]
    Thirds,
    /// 对角线。
    Diagonal,
    /// 三角形。
    Triangle,
    /// 黄金比例。
    GoldenRatio,
    /// 网格。
    Grid,
}

/// 裁剪面板。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CropParams {
    /// 归一化裁剪矩形 [x, y, w, h]。
    pub rect: [f32; 4],
    /// 固定比例。
    pub aspect: AspectRatio,
    /// 旋转，-45.0..=45.0。
    pub rotation_deg: f32,
    /// 90° 整圈旋转次数（0..=3，顺时针）。
    ///
    /// 与 `rotation_deg`（拉直微调）分开：整圈旋转会真正交换输出宽高，
    /// 不靠缩放裁剪（否则横图转竖图会裁掉大半画面）。
    pub quarter_turn: i32,
    /// 水平翻转。
    pub flip_horizontal: bool,
    /// 垂直翻转。
    pub flip_vertical: bool,
    /// 辅助线。
    pub grid: CropGrid,
    /// 裁剪后自动调整构图。
    pub auto_compose: bool,
}

impl Default for CropParams {
    fn default() -> Self {
        Self {
            rect: [0.0, 0.0, 1.0, 1.0],
            aspect: AspectRatio::Free,
            rotation_deg: 0.0,
            quarter_turn: 0,
            flip_horizontal: false,
            flip_vertical: false,
            grid: CropGrid::Thirds,
            auto_compose: false,
        }
    }
}

/// 人物蒙版的细分部位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeoplePart {
    /// 面部。
    Face,
    /// 身体。
    Body,
    /// 皮肤。
    Skin,
    /// 头发。
    Hair,
    /// 衣服。
    Clothes,
    /// 眼睛（v1.4.0 五官）。
    Eyes,
    /// 眉毛（v1.4.0 五官）。
    Eyebrows,
    /// 鼻子（v1.4.0 五官）。
    Nose,
    /// 嘴唇（v1.4.0 五官）。
    Lips,
    /// 牙齿 / 口腔内部（v1.4.0 五官）。
    Teeth,
    /// 耳朵。
    Ears,
}

/// 一条画笔笔画：点序列 + 落笔时的笔刷参数 + 擦除标记。
///
/// - 点序列保存的是**归一化坐标**（相对原图），可二次编辑，不保存位图；
/// - 参数为落笔时的快照（`None` 表示沿用蒙版当前设置），改动面板参数
///   只影响后续笔画，已画好的笔画保持原样（与 Lightroom 一致）；
/// - 反序列化兼容 v0.3 的旧格式（纯点数组 `[[x, y], ...]` → 普通笔画）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "BrushStrokeRepr")]
pub struct BrushStroke {
    /// 笔画路径（归一化坐标；单点 = 点按一下）。
    pub points: Vec<[f32; 2]>,
    /// 擦除笔画：从已有蒙版中减掉（反向混合）。
    pub erase: bool,
    /// 落笔时的大小（长边百分比，2..=100）。
    pub size: Option<f32>,
    /// 落笔时的羽化，0..=100。
    pub feather: Option<f32>,
    /// 落笔时的流动，0..=100。
    pub flow: Option<f32>,
    /// 落笔时的密度，0..=100。
    pub density: Option<f32>,
}

impl BrushStroke {
    /// 普通笔画（沿用蒙版当前笔刷设置）。
    pub fn new(points: Vec<[f32; 2]>) -> Self {
        Self {
            points,
            erase: false,
            size: None,
            feather: None,
            flow: None,
            density: None,
        }
    }

    /// 擦除笔画。
    pub fn erase(points: Vec<[f32; 2]>) -> Self {
        Self {
            erase: true,
            ..Self::new(points)
        }
    }

    /// 是否没有点。
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

/// 笔画的反序列化中间表示（对象格式或 v0.3 的点数组格式）。
#[derive(Deserialize)]
#[serde(untagged)]
enum BrushStrokeRepr {
    /// 对象格式（v0.4+）。
    Object {
        /// 点序列。
        points: Vec<[f32; 2]>,
        /// 擦除标记。
        #[serde(default)]
        erase: bool,
        /// 大小快照。
        #[serde(default)]
        size: Option<f32>,
        /// 羽化快照。
        #[serde(default)]
        feather: Option<f32>,
        /// 流动快照。
        #[serde(default)]
        flow: Option<f32>,
        /// 密度快照。
        #[serde(default)]
        density: Option<f32>,
    },
    /// 旧格式：只存点序列。
    Legacy(Vec<[f32; 2]>),
}

impl From<BrushStrokeRepr> for BrushStroke {
    fn from(repr: BrushStrokeRepr) -> Self {
        match repr {
            BrushStrokeRepr::Object {
                points,
                erase,
                size,
                feather,
                flow,
                density,
            } => Self {
                points,
                erase,
                size,
                feather,
                flow,
                density,
            },
            BrushStrokeRepr::Legacy(points) => Self::new(points),
        }
    }
}

/// 画笔设置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BrushSettings {
    /// 大小（长边的百分比，2..=100）。
    pub size: f32,
    /// 羽化，0..=100。
    pub feather: f32,
    /// 流动，0..=100。
    pub flow: f32,
    /// 密度，0..=100。
    pub density: f32,
    /// 自动蒙版。
    pub auto_mask: bool,
    /// 笔画列表（每笔自带点序列与参数快照，支持擦除）。
    pub strokes: Vec<BrushStroke>,
}

/// 蒙版类型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum MaskKind {
    /// 画笔。
    Brush(BrushSettings),
    /// 线性渐变。
    LinearGradient {
        /// 起点（归一化）；边界线过该点、在像素空间垂直于 `start → end`。
        start: [f32; 2],
        /// 终点（归一化）。
        end: [f32; 2],
    },
    /// 径向渐变。
    RadialGradient {
        /// 中心（归一化）。
        center: [f32; 2],
        /// 半径 [x, y]（归一化）。
        radii: [f32; 2],
        /// 旋转角度。
        rotation: f32,
        /// 内圈缩放（0..=0.98，缺省 0.75；越小过渡越柔）。
        #[serde(default)]
        inner: Option<f32>,
    },
    /// 颜色范围。
    ColorRange {
        /// 目标颜色（线性 RGB）。
        color: [f32; 3],
        /// 范围，0..=100。
        range: f32,
        /// 平滑度，0..=100。
        smoothness: f32,
    },
    /// 亮度范围。
    LuminanceRange {
        /// 下界，0..=1。
        min: f32,
        /// 上界，0..=1。
        max: f32,
        /// 平滑度，0..=100。
        smoothness: f32,
    },
    /// 深度范围（预留）。
    DepthRange {
        /// 下界，0..=1。
        min: f32,
        /// 上界，0..=1。
        max: f32,
    },
    /// AI 主体。
    AiSubject,
    /// AI 天空。
    AiSky,
    /// AI 背景。
    AiBackground,
    /// AI 人物。
    AiPeople {
        /// 部位。
        part: PeoplePart,
    },
    /// AI 物体（点按选择）。
    AiObject {
        /// 点按位置（归一化）。
        point: [f32; 2],
    },
}

/// 蒙版叠加显示颜色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OverlayColor {
    /// 红。
    #[default]
    Red,
    /// 绿。
    Green,
    /// 蓝。
    Blue,
    /// 白。
    White,
    /// 黑。
    Black,
}

/// 单个蒙版：类型 + 显示属性 + 独立的全套调色参数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaskParams {
    /// 稳定 ID（用于复制 / 粘贴 / 引用）。
    pub id: String,
    /// 名称。
    pub name: String,
    /// 蒙版类型。
    pub kind: MaskKind,
    /// 是否可见（隐藏的蒙版不参与渲染；旧项目缺省为可见）。
    pub visible: bool,
    /// 反向。
    pub inverted: bool,
    /// 羽化，0..=100。
    pub feather: f32,
    /// 不透明度，0..=100。
    pub opacity: f32,
    /// 叠加显示颜色。
    pub overlay: OverlayColor,
    /// 该蒙版独立的所有调色参数（约定其 `masks` 为空）。
    pub adjustments: Box<EditParams>,
}

impl Default for MaskParams {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            kind: MaskKind::AiSubject,
            visible: true,
            inverted: false,
            feather: 0.0,
            opacity: 100.0,
            overlay: OverlayColor::default(),
            adjustments: Box::new(EditParams::default()),
        }
    }
}

/// 调色面板滑块 ID（顺序即文档规定顺序，自上而下 15 项）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SliderId {
    /// 曝光。
    Exposure,
    /// 对比度。
    Contrast,
    /// 高光。
    Highlights,
    /// 阴影。
    Shadows,
    /// 白色。
    Whites,
    /// 黑色。
    Blacks,
    /// 色温。
    Temperature,
    /// 色调。
    Tint,
    /// 自然饱和度。
    Vibrance,
    /// 饱和度。
    Saturation,
    /// 纹理。
    Texture,
    /// 清晰度。
    Clarity,
    /// 去雾。
    Dehaze,
    /// 暗角。
    Vignette,
    /// 颗粒。
    Grain,
}

impl SliderId {
    /// 文档规定的 15 个滑块顺序。
    pub const ALL: [SliderId; 15] = [
        SliderId::Exposure,
        SliderId::Contrast,
        SliderId::Highlights,
        SliderId::Shadows,
        SliderId::Whites,
        SliderId::Blacks,
        SliderId::Temperature,
        SliderId::Tint,
        SliderId::Vibrance,
        SliderId::Saturation,
        SliderId::Texture,
        SliderId::Clarity,
        SliderId::Dehaze,
        SliderId::Vignette,
        SliderId::Grain,
    ];

    /// 中文标签。
    pub fn label_zh(self) -> &'static str {
        match self {
            SliderId::Exposure => "曝光",
            SliderId::Contrast => "对比度",
            SliderId::Highlights => "高光",
            SliderId::Shadows => "阴影",
            SliderId::Whites => "白色",
            SliderId::Blacks => "黑色",
            SliderId::Temperature => "色温",
            SliderId::Tint => "色调",
            SliderId::Vibrance => "自然饱和度",
            SliderId::Saturation => "饱和度",
            SliderId::Texture => "纹理",
            SliderId::Clarity => "清晰度",
            SliderId::Dehaze => "去雾",
            SliderId::Vignette => "暗角",
            SliderId::Grain => "颗粒",
        }
    }

    /// 英文标签。
    pub fn label_en(self) -> &'static str {
        match self {
            SliderId::Exposure => "Exposure",
            SliderId::Contrast => "Contrast",
            SliderId::Highlights => "Highlights",
            SliderId::Shadows => "Shadows",
            SliderId::Whites => "Whites",
            SliderId::Blacks => "Blacks",
            SliderId::Temperature => "Temperature",
            SliderId::Tint => "Tint",
            SliderId::Vibrance => "Vibrance",
            SliderId::Saturation => "Saturation",
            SliderId::Texture => "Texture",
            SliderId::Clarity => "Clarity",
            SliderId::Dehaze => "Dehaze",
            SliderId::Vignette => "Vignette",
            SliderId::Grain => "Grain",
        }
    }

    /// 滑块取值范围（UI 直接使用）。
    pub fn range(self) -> RangeInclusive<f32> {
        match self {
            SliderId::Exposure => -5.0..=5.0,
            SliderId::Grain => 0.0..=100.0,
            SliderId::Tint => -150.0..=150.0,
            _ => -100.0..=100.0,
        }
    }
}

impl EditParams {
    /// 按强度 t（0..=1）把另一组参数混入当前参数（预设强度滑块）。
    ///
    /// - 数值参数线性插值；
    /// - 曲线：控制点数量一致时逐点插值，否则 t < 0.5 保留原曲线、否则采用预设曲线；
    /// - 蒙版保留当前图像的蒙版（预设只影响全局参数）。
    pub fn blend(&self, preset: &EditParams, t: f32) -> EditParams {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        let mut result = self.clone();

        result.light.exposure = lerp(self.light.exposure, preset.light.exposure);
        result.light.contrast = lerp(self.light.contrast, preset.light.contrast);
        result.light.highlights = lerp(self.light.highlights, preset.light.highlights);
        result.light.shadows = lerp(self.light.shadows, preset.light.shadows);
        result.light.whites = lerp(self.light.whites, preset.light.whites);
        result.light.blacks = lerp(self.light.blacks, preset.light.blacks);
        result.color.temperature = lerp(self.color.temperature, preset.color.temperature);
        result.color.tint = lerp(self.color.tint, preset.color.tint);
        result.color.vibrance = lerp(self.color.vibrance, preset.color.vibrance);
        result.color.saturation = lerp(self.color.saturation, preset.color.saturation);
        result.effects.texture = lerp(self.effects.texture, preset.effects.texture);
        result.effects.clarity = lerp(self.effects.clarity, preset.effects.clarity);
        result.effects.dehaze = lerp(self.effects.dehaze, preset.effects.dehaze);
        result.effects.vignette = lerp(self.effects.vignette, preset.effects.vignette);
        result.effects.vignette_midpoint = lerp(
            self.effects.vignette_midpoint,
            preset.effects.vignette_midpoint,
        );
        result.effects.vignette_feather = lerp(
            self.effects.vignette_feather,
            preset.effects.vignette_feather,
        );
        result.effects.grain = lerp(self.effects.grain, preset.effects.grain);
        result.effects.grain_size = lerp(self.effects.grain_size, preset.effects.grain_size);
        result.effects.grain_roughness =
            lerp(self.effects.grain_roughness, preset.effects.grain_roughness);

        result.detail.sharpen.amount =
            lerp(self.detail.sharpen.amount, preset.detail.sharpen.amount);
        result.detail.sharpen.radius =
            lerp(self.detail.sharpen.radius, preset.detail.sharpen.radius);
        result.detail.sharpen.detail =
            lerp(self.detail.sharpen.detail, preset.detail.sharpen.detail);
        result.detail.sharpen.masking =
            lerp(self.detail.sharpen.masking, preset.detail.sharpen.masking);
        result.detail.noise.luminance =
            lerp(self.detail.noise.luminance, preset.detail.noise.luminance);
        result.detail.noise.color = lerp(self.detail.noise.color, preset.detail.noise.color);

        result.grading.shadows.hue = lerp(self.grading.shadows.hue, preset.grading.shadows.hue);
        result.grading.shadows.saturation = lerp(
            self.grading.shadows.saturation,
            preset.grading.shadows.saturation,
        );
        result.grading.midtones.saturation = lerp(
            self.grading.midtones.saturation,
            preset.grading.midtones.saturation,
        );
        result.grading.highlights.saturation = lerp(
            self.grading.highlights.saturation,
            preset.grading.highlights.saturation,
        );
        result.grading.global.hue = lerp(self.grading.global.hue, preset.grading.global.hue);
        result.grading.global.saturation =
            lerp(self.grading.global.saturation, preset.grading.global.saturation);
        result.grading.global.luminance =
            lerp(self.grading.global.luminance, preset.grading.global.luminance);
        result.grading.blending = lerp(self.grading.blending, preset.grading.blending);
        result.grading.balance = lerp(self.grading.balance, preset.grading.balance);

        for band in ColorBand::ALL {
            let source = self.hsl.band(band).clone();
            let target = preset.hsl.band(band);
            let blended = HslBand {
                hue: lerp(source.hue, target.hue),
                saturation: lerp(source.saturation, target.saturation),
                luminance: lerp(source.luminance, target.luminance),
            };
            *result.hsl.band_mut(band) = blended;
        }

        if t < 0.5 {
            result.curves = self.curves.clone();
        } else {
            result.curves = preset.curves.clone();
        }
        result.masks = self.masks.clone();
        result
    }

    /// 把蒙版的本地增量合成到全局参数之上（Lightroom 语义：蒙版滑块是相对全局的增量）。
    ///
    /// 合成规则：
    /// - 数值参数：全局 + 本地，按各自合法范围夹取；
    /// - 曲线：本地曲线串联在全局之后（`local(global(x))`），本地为恒等时保持全局；
    /// - 锐化半径、参数曲线分界：本地非默认（> 0）时覆盖；
    /// - 裁剪 / 变换 / 镜头：只取全局值（蒙版不参与几何调整）；
    /// - 嵌套蒙版列表：忽略（蒙版不可嵌套）。
    pub fn compose_local(&self, local: &EditParams) -> EditParams {
        let clamp_slider = |value: f32| value.clamp(-100.0, 100.0);
        let mut out = self.clone();

        out.light.exposure = (self.light.exposure + local.light.exposure).clamp(-5.0, 5.0);
        out.light.contrast = clamp_slider(self.light.contrast + local.light.contrast);
        out.light.highlights = clamp_slider(self.light.highlights + local.light.highlights);
        out.light.shadows = clamp_slider(self.light.shadows + local.light.shadows);
        out.light.whites = clamp_slider(self.light.whites + local.light.whites);
        out.light.blacks = clamp_slider(self.light.blacks + local.light.blacks);

        out.color.temperature = clamp_slider(self.color.temperature + local.color.temperature);
        out.color.tint = clamp_slider(self.color.tint + local.color.tint);
        out.color.vibrance = clamp_slider(self.color.vibrance + local.color.vibrance);
        out.color.saturation = clamp_slider(self.color.saturation + local.color.saturation);

        out.effects.texture = clamp_slider(self.effects.texture + local.effects.texture);
        out.effects.clarity = clamp_slider(self.effects.clarity + local.effects.clarity);
        out.effects.dehaze = clamp_slider(self.effects.dehaze + local.effects.dehaze);
        out.effects.vignette = clamp_slider(self.effects.vignette + local.effects.vignette);
        out.effects.vignette_midpoint =
            (self.effects.vignette_midpoint + local.effects.vignette_midpoint).clamp(0.0, 100.0);
        out.effects.vignette_feather =
            (self.effects.vignette_feather + local.effects.vignette_feather).clamp(0.0, 100.0);
        out.effects.grain = (self.effects.grain + local.effects.grain).clamp(0.0, 100.0);
        out.effects.grain_size =
            (self.effects.grain_size + local.effects.grain_size).clamp(0.0, 100.0);
        out.effects.grain_roughness =
            (self.effects.grain_roughness + local.effects.grain_roughness).clamp(0.0, 100.0);

        out.curves.rgb = compose_tone_curve(&self.curves.rgb, &local.curves.rgb);
        out.curves.red = compose_tone_curve(&self.curves.red, &local.curves.red);
        out.curves.green = compose_tone_curve(&self.curves.green, &local.curves.green);
        out.curves.blue = compose_tone_curve(&self.curves.blue, &local.curves.blue);
        out.curves.parametric.highlights =
            clamp_slider(self.curves.parametric.highlights + local.curves.parametric.highlights);
        out.curves.parametric.lights =
            clamp_slider(self.curves.parametric.lights + local.curves.parametric.lights);
        out.curves.parametric.darks =
            clamp_slider(self.curves.parametric.darks + local.curves.parametric.darks);
        out.curves.parametric.shadows =
            clamp_slider(self.curves.parametric.shadows + local.curves.parametric.shadows);
        if local.curves.parametric.highlight_split > 0.0 {
            out.curves.parametric.highlight_split =
                local.curves.parametric.highlight_split.clamp(0.0, 1.0);
        }
        if local.curves.parametric.midtone_split > 0.0 {
            out.curves.parametric.midtone_split =
                local.curves.parametric.midtone_split.clamp(0.0, 1.0);
        }
        if local.curves.parametric.shadow_split > 0.0 {
            out.curves.parametric.shadow_split =
                local.curves.parametric.shadow_split.clamp(0.0, 1.0);
        }

        for band in ColorBand::ALL {
            let global_band = self.hsl.band(band).clone();
            let local_band = local.hsl.band(band).clone();
            *out.hsl.band_mut(band) = HslBand {
                hue: clamp_slider(global_band.hue + local_band.hue),
                saturation: clamp_slider(global_band.saturation + local_band.saturation),
                luminance: clamp_slider(global_band.luminance + local_band.luminance),
            };
        }

        out.grading.shadows =
            compose_grading_zone(self.grading.shadows.clone(), local.grading.shadows.clone());
        out.grading.midtones = compose_grading_zone(
            self.grading.midtones.clone(),
            local.grading.midtones.clone(),
        );
        out.grading.highlights = compose_grading_zone(
            self.grading.highlights.clone(),
            local.grading.highlights.clone(),
        );
        out.grading.global = compose_grading_zone(
            self.grading.global.clone(),
            local.grading.global.clone(),
        );
        out.grading.blending = (self.grading.blending + local.grading.blending).clamp(0.0, 100.0);
        out.grading.balance = clamp_slider(self.grading.balance + local.grading.balance);

        out.detail.sharpen.amount =
            (self.detail.sharpen.amount + local.detail.sharpen.amount).clamp(0.0, 150.0);
        if local.detail.sharpen.radius > 0.0 {
            out.detail.sharpen.radius = local.detail.sharpen.radius.clamp(0.5, 3.0);
        }
        out.detail.sharpen.detail =
            (self.detail.sharpen.detail + local.detail.sharpen.detail).clamp(0.0, 100.0);
        out.detail.sharpen.masking =
            (self.detail.sharpen.masking + local.detail.sharpen.masking).clamp(0.0, 100.0);

        out.detail.noise.luminance =
            (self.detail.noise.luminance + local.detail.noise.luminance).clamp(0.0, 100.0);
        out.detail.noise.luminance_detail = (self.detail.noise.luminance_detail
            + local.detail.noise.luminance_detail)
            .clamp(0.0, 100.0);
        out.detail.noise.luminance_contrast = (self.detail.noise.luminance_contrast
            + local.detail.noise.luminance_contrast)
            .clamp(0.0, 100.0);
        out.detail.noise.color =
            (self.detail.noise.color + local.detail.noise.color).clamp(0.0, 100.0);
        out.detail.noise.color_detail =
            (self.detail.noise.color_detail + local.detail.noise.color_detail).clamp(0.0, 100.0);
        out.detail.noise.color_smoothness = (self.detail.noise.color_smoothness
            + local.detail.noise.color_smoothness)
            .clamp(0.0, 100.0);

        out.masks = Vec::new();
        out
    }

    /// 读取滑块值。
    pub fn slider(&self, id: SliderId) -> f32 {
        match id {
            SliderId::Exposure => self.light.exposure,
            SliderId::Contrast => self.light.contrast,
            SliderId::Highlights => self.light.highlights,
            SliderId::Shadows => self.light.shadows,
            SliderId::Whites => self.light.whites,
            SliderId::Blacks => self.light.blacks,
            SliderId::Temperature => self.color.temperature,
            SliderId::Tint => self.color.tint,
            SliderId::Vibrance => self.color.vibrance,
            SliderId::Saturation => self.color.saturation,
            SliderId::Texture => self.effects.texture,
            SliderId::Clarity => self.effects.clarity,
            SliderId::Dehaze => self.effects.dehaze,
            SliderId::Vignette => self.effects.vignette,
            SliderId::Grain => self.effects.grain,
        }
    }

    /// 写入滑块值，并按文档范围夹取。
    pub fn set_slider(&mut self, id: SliderId, value: f32) {
        let range = id.range();
        let value = value.clamp(*range.start(), *range.end());
        match id {
            SliderId::Exposure => self.light.exposure = value,
            SliderId::Contrast => self.light.contrast = value,
            SliderId::Highlights => self.light.highlights = value,
            SliderId::Shadows => self.light.shadows = value,
            SliderId::Whites => self.light.whites = value,
            SliderId::Blacks => self.light.blacks = value,
            SliderId::Temperature => self.color.temperature = value,
            SliderId::Tint => self.color.tint = value,
            SliderId::Vibrance => self.color.vibrance = value,
            SliderId::Saturation => self.color.saturation = value,
            SliderId::Texture => self.effects.texture = value,
            SliderId::Clarity => self.effects.clarity = value,
            SliderId::Dehaze => self.effects.dehaze = value,
            SliderId::Vignette => self.effects.vignette = value,
            SliderId::Grain => self.effects.grain = value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_order_matches_spec() {
        assert_eq!(SliderId::ALL.len(), 15);
        assert_eq!(SliderId::ALL[0], SliderId::Exposure);
        assert_eq!(SliderId::ALL[14], SliderId::Grain);
        assert_eq!(SliderId::Vignette.label_zh(), "暗角");
    }

    #[test]
    fn slider_set_get_roundtrip_and_clamp() {
        let mut params = EditParams::default();
        params.set_slider(SliderId::Exposure, 1.25);
        assert_eq!(params.slider(SliderId::Exposure), 1.25);
        params.set_slider(SliderId::Exposure, 99.0);
        assert_eq!(params.slider(SliderId::Exposure), 5.0);
        params.set_slider(SliderId::Grain, -10.0);
        assert_eq!(params.slider(SliderId::Grain), 0.0);
    }

    #[test]
    fn serde_roundtrip_keeps_defaults_and_masks() {
        let mut params = EditParams::default();
        params.masks.push(MaskParams {
            id: "m1".to_string(),
            name: "天空".to_string(),
            kind: MaskKind::AiSky,
            ..MaskParams::default()
        });
        let json = serde_json::to_string(&params).unwrap();
        let back: EditParams = serde_json::from_str(&json).unwrap();
        assert_eq!(params, back);
        assert_eq!(back.hsl.band(ColorBand::Aqua).hue, 0.0);
    }

    #[test]
    fn missing_fields_fall_back_to_default() {
        let params: EditParams = serde_json::from_str(r#"{"light":{"exposure":2.0}}"#).unwrap();
        assert_eq!(params.light.exposure, 2.0);
        assert_eq!(params.light.contrast, 0.0);
        assert_eq!(params.version, PARAM_VERSION);
    }

    #[test]
    fn blend_mixes_values_and_keeps_masks() {
        let mut base = EditParams::default();
        base.light.exposure = 0.0;
        base.masks.push(MaskParams {
            id: "keep-me".to_string(),
            ..MaskParams::default()
        });

        let mut preset = EditParams::default();
        preset.light.exposure = 2.0;
        preset.color.saturation = 50.0;

        let half = base.blend(&preset, 0.5);
        assert!((half.light.exposure - 1.0).abs() < 1e-6);
        assert!((half.color.saturation - 25.0).abs() < 1e-6);
        assert_eq!(half.masks.len(), 1, "蒙版应保留");
        assert_eq!(half.masks[0].id, "keep-me");

        let full = base.blend(&preset, 1.0);
        assert!((full.light.exposure - 2.0).abs() < 1e-6);

        let none = base.blend(&preset, 0.0);
        assert!((none.light.exposure - 0.0).abs() < 1e-6);
    }

    #[test]
    fn compose_local_default_is_noop() {
        let mut global = EditParams::default();
        global.light.exposure = 2.0;
        global.color.saturation = 40.0;
        global.effects.clarity = 25.0;
        global.detail.sharpen.radius = 1.2;

        let composed = global.compose_local(&EditParams::default());
        assert_eq!(composed.light.exposure, 2.0);
        assert_eq!(composed.color.saturation, 40.0);
        assert_eq!(composed.effects.clarity, 25.0);
        assert_eq!(composed.detail.sharpen.radius, 1.2);
        assert_eq!(composed.curves, global.curves);
    }

    #[test]
    fn compose_local_stacks_and_clamps() {
        let mut global = EditParams::default();
        global.light.exposure = 1.0;
        global.color.saturation = 80.0;
        global.hsl.red.saturation = 50.0;

        let mut local = EditParams::default();
        local.light.exposure = 0.5;
        local.color.saturation = 50.0;
        local.hsl.red.saturation = 25.0;

        let composed = global.compose_local(&local);
        assert!((composed.light.exposure - 1.5).abs() < 1e-6);
        assert_eq!(composed.color.saturation, 100.0, "叠加后应夹取到 +100");
        assert_eq!(composed.hsl.red.saturation, 75.0);
        assert_eq!(composed.light.contrast, global.light.contrast);
    }

    #[test]
    fn compose_local_keeps_geometry_and_drops_nested_masks() {
        let mut global = EditParams::default();
        global.crop.rect = [0.1, 0.1, 0.5, 0.5];
        global.transform.rotate = 5.0;
        global.lens.vignette = 20.0;

        let mut local = EditParams::default();
        local.crop.rect = [0.0, 0.0, 1.0, 1.0];
        local.transform.rotate = 30.0;
        local.lens.vignette = -50.0;
        local.masks.push(MaskParams::default());

        let composed = global.compose_local(&local);
        assert_eq!(composed.crop.rect, [0.1, 0.1, 0.5, 0.5]);
        assert_eq!(composed.transform.rotate, 5.0);
        assert_eq!(composed.lens.vignette, 20.0);
        assert!(composed.masks.is_empty(), "蒙版不可嵌套");
    }

    #[test]
    fn compose_local_concatenates_curves() {
        let mut global = EditParams::default();
        global.curves.rgb = ToneCurve {
            points: vec![
                CurvePoint { x: 0.0, y: 0.25 },
                CurvePoint { x: 1.0, y: 1.0 },
            ],
        };
        let mut local = EditParams::default();
        local.curves.rgb = ToneCurve {
            points: vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.0, y: 0.5 }],
        };

        let composed = global.compose_local(&local);
        // global(0.5) = 0.625 → local(0.625) = 0.3125
        let mid = eval_curve(&composed.curves.rgb.points, 0.5);
        assert!(
            (mid - 0.3125).abs() < 1e-4,
            "曲线应按 local(global(x)) 串联：{mid}"
        );
    }

    #[test]
    fn curve_interpolation_is_smooth_and_monotone() {
        let points = vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 0.25, y: 0.1 },
            CurvePoint { x: 0.5, y: 0.9 },
            CurvePoint { x: 1.0, y: 1.0 },
        ];
        for point in &points {
            let value = eval_curve(&points, point.x);
            assert!(
                (value - point.y).abs() < 1e-4,
                "曲线必须过控制点：({}, {}) → {value}",
                point.x,
                point.y
            );
        }
        let mut previous = -1.0f32;
        for step in 0..=1000 {
            let x = step as f32 / 1000.0;
            let value = eval_curve(&points, x);
            assert!(
                value >= previous - 1e-4,
                "曲线应单调不减：x={x} {previous} → {value}"
            );
            assert!(
                (0.0..=1.0).contains(&value),
                "曲线不应过冲：x={x} → {value}"
            );
            previous = value;
        }
        // 控制点之间不再是折线
        let mid = eval_curve(&points, 0.375);
        let linear = (0.1 + 0.9) / 2.0;
        assert!(
            (mid - linear).abs() > 0.005,
            "应使用平滑插值而非线性折线：{mid} vs {linear}"
        );
    }

    #[test]
    fn curve_is_c1_at_control_points() {
        let points = vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 0.25, y: 0.1 },
            CurvePoint { x: 0.5, y: 0.9 },
            CurvePoint { x: 1.0, y: 1.0 },
        ];
        for knot in [0.25f32, 0.5] {
            let h = 1e-3;
            let left = (eval_curve(&points, knot) - eval_curve(&points, knot - h)) / h;
            let right = (eval_curve(&points, knot + h) - eval_curve(&points, knot)) / h;
            assert!(
                (left - right).abs() < 0.05,
                "控制点处一阶应连续：knot={knot} 左 {left} 右 {right}"
            );
        }
    }

    #[test]
    fn parametric_curve_is_identity_at_neutral() {
        let lut = parametric_curve_lut(&ParametricCurve::default());
        for (index, value) in lut.iter().enumerate() {
            let x = index as f32 / (PARAM_CURVE_ENTRIES - 1) as f32;
            assert!((value - x).abs() < 1e-5, "x={x} -> {value}");
        }
    }

    #[test]
    fn parametric_curve_regions_are_independent_and_endpoints_are_fixed() {
        let neutral = parametric_curve_lut(&ParametricCurve::default());
        for (field, index) in [
            ("shadows", 0usize),
            ("darks", 1),
            ("lights", 2),
            ("highlights", 3),
        ] {
            for amount in [-100.0f32, 100.0] {
                let mut curve = ParametricCurve::default();
                match index {
                    0 => curve.shadows = amount,
                    1 => curve.darks = amount,
                    2 => curve.lights = amount,
                    _ => curve.highlights = amount,
                }
                let lut = parametric_curve_lut(&curve);
                // 端点必须不动（LR 的参数曲线端点固定）。
                assert!((lut[0] - 0.0).abs() < 1e-5, "{field} {amount}: 黑场");
                assert!(
                    (lut[PARAM_CURVE_ENTRIES - 1] - 1.0).abs() < 1e-5,
                    "{field} {amount}: 白场"
                );
                // 只在该区域中心产生明显位移，其他区域中心基本不动。
                let centers = [0.125f32, 0.375, 0.625, 0.875];
                let middle = centers[index];
                let moved =
                    (lut[(middle * 255.0) as usize] - neutral[(middle * 255.0) as usize]).abs();
                assert!(moved > 0.05, "{field} {amount}: 区域中心只动了 {moved}");
                for (other, center) in centers.iter().enumerate() {
                    if other == index {
                        continue;
                    }
                    let delta =
                        (lut[(center * 255.0) as usize] - neutral[(center * 255.0) as usize]).abs();
                    assert!(
                        delta < 0.30,
                        "{field} {amount}: 串到区域 {other}，位移 {delta}"
                    );
                }
            }
        }
    }

    #[test]
    fn parametric_curve_stays_monotone_with_opposing_sliders() {
        let curve = ParametricCurve {
            shadows: 100.0,
            darks: -100.0,
            lights: 100.0,
            highlights: -100.0,
            ..ParametricCurve::default()
        };
        let lut = parametric_curve_lut(&curve);
        let mut previous = -1.0f32;
        for value in lut {
            assert!(value >= previous - 1e-5, "{value} < {previous}");
            assert!((0.0..=1.0).contains(&value), "{value}");
            previous = value;
        }
    }
}
