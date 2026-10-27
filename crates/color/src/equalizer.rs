//! 颜色均衡器（color equalizer）：按色相调整饱和度、色相与亮度。
//!
//! 算法结构（与参考实现等价，代码为独立实现）：
//!
//! 1. 线性 RGB → XYZ D65 → CIE xy → dt UCS 的 `U'V'` 与 `L*`；
//! 2. 用 `(max-min)/max` 估一个“饱和度”，高斯模糊后作为“是否有彩色”的判据；
//! 3. 可选：以 `U'V'` 自为导向图做 2 通道导向滤波，压掉色度噪点；
//! 4. 由像素色相查三张 LUT，得到色相偏移、饱和度增益、亮度增益；
//!    低饱和像素用 logistic 权重衰减，避免把灰区染色；
//! 5. 可选：以 `U'V'` 为导向图平滑修正量，并用 Scharr 梯度在强边缘处
//!    压掉亮度修正（防光晕）；
//! 6. 施加修正 → 色域软裁剪 → 回 XYZ → 回线性 RGB。
//!
//! 三张 LUT 由 8 个色相节点（红/橙/黄/绿/青/蓝/紫/品红）经**周期 RBF 插值**
//! 生成：节点值被展开到整圈色相上，保证 0°/360° 处连续、过渡可调。
//!
//! 用法：
//! ```no_run
//! use fdxmp_color::{ColorEqualizer, EqualizerParams, SRGB};
//!
//! let mut eq = ColorEqualizer::new(EqualizerParams::default(), &SRGB);
//! let (w, h) = (1920, 1080);
//! let mut rgb = vec![0.2f32; w * h * 3];
//! eq.apply(&mut rgb, w, h, 3);
//! ```
//!
//! 参数全为默认值时 [`ColorEqualizer::apply`] 直接返回，不改动任何像素。

use crate::guided::{
    gaussian_blur, guided_filter_2ch, par_rows_mut, par_rows_mut2, scharr_gradient,
};
use crate::math::RgbSpace;
use crate::ucs::{
    hsb_to_xyz, jch_to_hsb, l_star_from_y, l_white_of, luv_to_jch, xy_to_uv, xyz_to_xyy, Gamut,
    LUT_ELEM,
};

/// 色相节点数（8 个：红橙黄绿青蓝紫品红，每 45° 一个）。
pub const NODES: usize = 8;

/// 饱和度修正的视觉放大系数。
const SAT_EFFECT: f32 = 2.0;
/// 亮度修正的视觉放大系数。
const BRIGHT_EFFECT: f32 = 8.0;
/// logistic 权重表的一半长度。
const SATSIZE: usize = 4096;
/// 低于此值视为“无彩色”（2^-16，与参考实现一致）。
const NORM_MIN: f32 = 1.525_878_9e-5;
/// sRGB 红在 dt UCS 中的色相角，用于把 GUI 的 0° 对到红色。
const ANGLE_SHIFT: f32 = 20.0;

/// 颜色均衡器参数。
///
/// `saturation` / `hue` / `brightness` 各 8 个节点，顺序固定为
/// **红、橙、黄、绿、青、蓝、紫、品红**。饱和与亮度是增益（1.0 为不变），
/// 色相是角度偏移（度）。
#[derive(Clone, Debug, PartialEq)]
pub struct EqualizerParams {
    /// 低饱和抑制的拐点（0..0.3，默认 0.1 → 实际拐点约 6% 饱和度）。
    pub threshold: f32,
    /// 色相曲线的过渡宽度（0.05..2.0，越小越窄）。
    pub smoothing_hue: f32,
    /// 低饱和过渡的软硬（-1..1，越负越软）。
    pub contrast: f32,
    /// 白点亮度的 2 的幂（默认 1.0 = 漫反射白）。
    pub white_level: f32,
    /// 色相分析半径（导向滤波 σ 的一半）。
    pub chroma_size: f32,
    /// 修正量导向滤波半径（σ 的一半）。
    pub param_size: f32,
    /// 是否启用两个导向滤波（关掉更快，但彩噪与光晕会回来）。
    pub use_filter: bool,
    /// 饱和度增益，8 个色相节点。
    pub saturation: [f32; NODES],
    /// 色相偏移（度），8 个色相节点。
    pub hue: [f32; NODES],
    /// 亮度增益，8 个色相节点。
    pub brightness: [f32; NODES],
    /// 节点整体旋转（度，-23..23）。
    pub hue_shift: f32,
}

impl Default for EqualizerParams {
    fn default() -> Self {
        Self {
            threshold: 0.1,
            smoothing_hue: 1.0,
            contrast: 0.0,
            white_level: 1.0,
            chroma_size: 1.5,
            param_size: 1.0,
            use_filter: true,
            saturation: [1.0; NODES],
            hue: [0.0; NODES],
            brightness: [1.0; NODES],
            hue_shift: 0.0,
        }
    }
}

impl EqualizerParams {
    /// 是否等价于恒等变换（忽略只影响空间滤波的阈值类参数）。
    pub fn is_identity(&self) -> bool {
        let one = |v: &f32| (v - 1.0).abs() < 1e-7;
        let zero = |v: &f32| v.abs() < 1e-7;
        self.saturation.iter().all(one)
            && self.brightness.iter().all(one)
            && self.hue.iter().all(zero)
    }
}

/// 预编译好的颜色均衡器：三张色相 LUT + 色域边界表 + logistic 权重表。
///
/// 构造一次可反复使用；参数变化时调用 [`ColorEqualizer::set_params`]
/// 重新烘焙 LUT（8×8 线性方程 + 512 次采样，微秒级）。
pub struct ColorEqualizer {
    params: EqualizerParams,
    l_white: f32,
    lut_hue: [f32; LUT_ELEM],
    lut_saturation: [f32; LUT_ELEM],
    lut_brightness: [f32; LUT_ELEM],
    sat_weights: Vec<f32>,
    gamut: Gamut,
    matrix_in: [[f32; 3]; 3],
    matrix_out: [[f32; 3]; 3],
    space: RgbSpace,
}

impl core::fmt::Debug for ColorEqualizer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ColorEqualizer")
            .field("params", &self.params)
            .field("l_white", &self.l_white)
            .field("space", &self.space.name)
            .finish_non_exhaustive()
    }
}

impl ColorEqualizer {
    /// 由参数与目标 RGB 空间构造（空间用于色域边界表与 RGB↔XYZ 矩阵）。
    pub fn new(params: EqualizerParams, space: &RgbSpace) -> Self {
        let matrix = crate::ucs::rgb_to_xyz_d65(space);
        let matrix_in = matrix.to_f32();
        let matrix_out = matrix
            .inverse()
            .unwrap_or(crate::math::Mat3::IDENTITY)
            .to_f32();

        let mut this = Self {
            l_white: 1.0,
            lut_hue: [0.0; LUT_ELEM],
            lut_saturation: [1.0; LUT_ELEM],
            lut_brightness: [1.0; LUT_ELEM],
            sat_weights: Vec::new(),
            gamut: Gamut::from_rgb_to_xyz(&matrix_in),
            matrix_in,
            matrix_out,
            space: *space,
            params: params.clone(),
        };
        this.bake(&params);
        this
    }

    /// 当前参数。
    pub fn params(&self) -> &EqualizerParams {
        &self.params
    }

    /// 目标 RGB 空间。
    pub fn space(&self) -> &RgbSpace {
        &self.space
    }

    /// 白点明度 `L_white`。
    pub fn l_white(&self) -> f32 {
        self.l_white
    }

    /// 三张 512 点色相 LUT 与色域边界表（顺序：色相偏移、饱和度增益、亮度增益、边界 `M²`），
    /// 供热更新到 GPU uniform。
    pub fn luts(&self) -> (
        &[f32; LUT_ELEM],
        &[f32; LUT_ELEM],
        &[f32; LUT_ELEM],
        &[f32; LUT_ELEM],
    ) {
        (
            &self.lut_hue,
            &self.lut_saturation,
            &self.lut_brightness,
            self.gamut.values(),
        )
    }

    /// 换参数并重新烘焙 LUT。
    pub fn set_params(&mut self, params: &EqualizerParams) {
        self.params = params.clone();
        self.bake(params);
    }

    /// 换目标 RGB 空间（重建矩阵与色域表，保留参数）。
    pub fn set_space(&mut self, space: &RgbSpace) {
        let matrix = crate::ucs::rgb_to_xyz_d65(space);
        self.matrix_in = matrix.to_f32();
        self.matrix_out = matrix
            .inverse()
            .unwrap_or(crate::math::Mat3::IDENTITY)
            .to_f32();
        self.gamut = Gamut::from_rgb_to_xyz(&self.matrix_in);
        self.space = *space;
    }

    fn bake(&mut self, p: &EqualizerParams) {
        self.l_white = l_star_from_y(p.white_level.exp2());

        // 饱和度与亮度用固定平滑度 π；色相曲线的宽度由 smoothing_hue 控制。
        let sat_nodes = p.saturation;
        let bright_nodes = p.brightness;
        let hue_offsets: [f32; NODES] = core::array::from_fn(|k| p.hue[k].to_radians());

        self.lut_saturation = periodic_rbf(sat_nodes, core::f32::consts::PI, p, true);
        self.lut_brightness = periodic_rbf(bright_nodes, core::f32::consts::PI, p, true);
        self.lut_hue = periodic_rbf(
            hue_offsets,
            core::f32::consts::PI / p.smoothing_hue.clamp(0.05, 8.0),
            p,
            false,
        );

        // 低饱和抑制的 logistic 权重：w(s) = 1 / (1 + exp(factor·s/2))。
        let factor = -60.0 - 40.0 * p.contrast;
        self.sat_weights = (0..=2 * SATSIZE)
            .map(|i| {
                let s = 0.5 / SATSIZE as f32 * (i as f32 - SATSIZE as f32);
                1.0 / (1.0 + (factor * s).exp())
            })
            .collect();
    }

    /// logistic 权重查表（线性插值）。
    #[inline]
    fn sat_weight(&self, saturation: f32) -> f32 {
        let limit = 1.0 - 1.0 / SATSIZE as f32;
        let s = saturation.clamp(-1.0, limit);
        let position = SATSIZE as f32 * (1.0 + s);
        let base = position.floor();
        let i = base as usize;
        self.sat_weights[i] + (position - base) * (self.sat_weights[i + 1] - self.sat_weights[i])
    }

    /// 就地处理一张图（`channels` 为 3 或 4，alpha 保持不变）。
    pub fn apply(&self, pixels: &mut [f32], width: usize, height: usize, channels: usize) {
        self.apply_scaled(pixels, width, height, channels, 1.0);
    }

    /// 同 [`ColorEqualizer::apply`]，`scale` 为相对全尺寸的采样比
    ///（预览用小图时传 `preview_width / full_width`，让半径按比例缩小）。
    pub fn apply_scaled(
        &self,
        pixels: &mut [f32],
        width: usize,
        height: usize,
        channels: usize,
        scale: f32,
    ) {
        let pixels_count = width * height;
        if pixels_count == 0
            || channels < 3
            || pixels.len() < pixels_count * channels
            || self.params.is_identity()
        {
            return;
        }

        let p = &self.params;
        let l_white = self.l_white;
        let scale = scale.clamp(0.01, 4.0);

        // ---- 派生量 ----------------------------------------------------
        // threshold 滑杆 → 低饱和拐点：默认 0.1 → 0.06。
        let sat_shift = -0.015 + 0.3 * (5.0 * p.threshold).powi(2);
        let max_brightness = p.brightness.iter().fold(1.0f32, |acc, v| acc.max(*v));
        let max_brightness_shift = 0.01 * max_brightness;
        let corr_max_brightness_shift =
            max_brightness_shift * 5.0f32.min(p.param_size.max(0.0).sqrt());
        let bright_shift = sat_shift + corr_max_brightness_shift;

        let gradient_amp = 4.0 * max_brightness.sqrt() * scale * scale;
        let hue_sigma = 0.5 * p.chroma_size * scale;
        let par_sigma = 0.5 * p.param_size * scale;
        let sat_sigma = scale.max(0.5);
        let scharr_sigma = scale.max(0.5);

        // ---- STEP 1：RGB → UCS（UV + L*）与饱和度 ----------------------
        let mut saturation = vec![0.0f32; pixels_count];
        {
            let src = &pixels[..];
            par_rows_mut(&mut saturation, width, height, |y, row| {
                for (x, slot) in row.iter_mut().enumerate() {
                    let base = (y * width + x) * channels;
                    let (r, g, b) = (src[base], src[base + 1], src[base + 2]);
                    let max = r.max(g).max(b);
                    let min = r.min(g).min(b);
                    let delta = max - min;
                    *slot = if max > NORM_MIN && delta > NORM_MIN {
                        delta / max
                    } else {
                        0.0
                    };
                }
            });
        }

        let mut uv = vec![0.0f32; pixels_count * 2];
        let mut l_star = vec![0.0f32; pixels_count];
        {
            let src = &pixels[..];
            let m = &self.matrix_in;
            par_rows_mut2(
                &mut uv,
                width * 2,
                &mut l_star,
                width,
                height,
                |y, uv_row, l_row| {
                    for x in 0..width {
                        let base = (y * width + x) * channels;
                        let (r, g, b) = (src[base], src[base + 1], src[base + 2]);

                        let xyz = [
                            m[0][0] * r + m[0][1] * g + m[0][2] * b,
                            m[1][0] * r + m[1][1] * g + m[1][2] * b,
                            m[2][0] * r + m[2][1] * g + m[2][2] * b,
                        ];
                        let xyy = xyz_to_xyy(xyz);
                        let u = xy_to_uv([xyy[0], xyy[1]]);
                        uv_row[x * 2] = u[0];
                        uv_row[x * 2 + 1] = u[1];
                        l_row[x] = l_star_from_y(xyy[2]);
                    }
                },
            );
        }

        gaussian_blur(&mut saturation, width, height, 1, sat_sigma);

        // ---- STEP 2：平滑色度，压掉色度噪点 ---------------------------
        if p.use_filter {
            let smoothed = guided_filter_2ch(&uv, &uv, width, height, hue_sigma, 1e-5, 1.0);
            par_rows_mut(&mut uv, width * 2, height, |y, row| {
                for x in 0..width {
                    let k = y * width + x;
                    // 高饱和处用滤波结果，接近中性处保留原值。
                    let weight = self.sat_weight(saturation[k] - sat_shift);
                    let u = row[x * 2];
                    let v = row[x * 2 + 1];
                    row[x * 2] = weight * (smoothed[k * 2] - u) + u;
                    row[x * 2 + 1] = weight * (smoothed[k * 2 + 1] - v) + v;
                }
            });
        }

        // ---- STEP 3：查 LUT 得修正量，并算饱和度梯度 ------------------
        // corrections 交错保存 [饱和度增益, 亮度修正]。
        let mut corrections = vec![0.0f32; pixels_count * 2];
        let mut gradient = vec![0.0f32; pixels_count];
        let compute_gradient = p.use_filter && width >= 3 && height >= 3;

        par_rows_mut2(
            &mut corrections,
            width * 2,
            &mut gradient,
            width,
            height,
            |y, corr_row, grad_row| {
                for x in 0..width {
                    let k = y * width + x;
                    let jch = luv_to_jch(l_star[k], l_white, [uv[k * 2], uv[k * 2 + 1]]);
                    let hsb = jch_to_hsb(jch);

                    let (mut gain, mut bright) = (1.0f32, 0.0f32);
                    if jch[1] > NORM_MIN {
                        gain = lookup(&self.lut_saturation, hsb[0]);
                        bright = hsb[1] * (lookup(&self.lut_brightness, hsb[0]) - 1.0);
                    }
                    corr_row[x * 2] = gain;
                    corr_row[x * 2 + 1] = bright;

                    if compute_gradient {
                        // 取内点，边界像素复用最近的内部梯度。
                        let vrow = y.clamp(1, height - 2);
                        let vcol = x.clamp(1, width - 2);
                        let g = scharr_gradient(&saturation, width, vcol, vrow);
                        grad_row[x] = gradient_amp * (g - 0.02).max(0.0).powi(2);
                    }
                }
            },
        );

        // ---- STEP 4：导向滤波平滑修正量 + 边缘抑制光晕 ----------------
        if p.use_filter {
            gaussian_blur(&mut gradient, width, height, 1, scharr_sigma);
            let filtered =
                guided_filter_2ch(&uv, &corrections, width, height, par_sigma, 1e-6, 0.1);

            par_rows_mut(&mut corrections, width * 2, height, |y, row| {
                for x in 0..width {
                    let k = y * width + x;
                    let sat_gate = self.sat_weight(saturation[k] - sat_shift);
                    let bright_gate = self.sat_weight(saturation[k] - bright_shift);
                    let edge_gate = 1.0 - gradient[k].clamp(0.0, 1.0);
                    row[x * 2] = 1.0 + (filtered[k * 2] - 1.0) * sat_gate;
                    row[x * 2 + 1] = filtered[k * 2 + 1] * edge_gate * bright_gate;
                }
            });
        }

        // ---- STEP 5：施加修正 → 色域软裁剪 → 回 RGB -------------------
        let out_m = &self.matrix_out;
        par_rows_mut(pixels, width * channels, height, |y, row| {
            for x in 0..width {
                let k = y * width + x;
                let jch = luv_to_jch(l_star[k], l_white, [uv[k * 2], uv[k * 2 + 1]]);
                let mut hsb = jch_to_hsb(jch);
                let hue = hsb[0];

                hsb[0] = hue + lookup(&self.lut_hue, hue);
                hsb[1] = (hsb[1] * (1.0 + SAT_EFFECT * (corrections[k * 2] - 1.0))).max(0.0);
                hsb[2] = (hsb[2] * (1.0 + BRIGHT_EFFECT * corrections[k * 2 + 1])).max(0.0);

                self.gamut.map_hsb(&mut hsb, l_white);
                let xyz = hsb_to_xyz(hsb, l_white);

                let base = x * channels;
                row[base] = out_m[0][0] * xyz[0] + out_m[0][1] * xyz[1] + out_m[0][2] * xyz[2];
                row[base + 1] = out_m[1][0] * xyz[0] + out_m[1][1] * xyz[1] + out_m[1][2] * xyz[2];
                row[base + 2] = out_m[2][0] * xyz[0] + out_m[2][1] * xyz[1] + out_m[2][2] * xyz[2];
            }
        });
    }
}

/// 色相 LUT 查表（循环，线性插值）。`hue` 为弧度。
#[inline]
fn lookup(lut: &[f32; LUT_ELEM], hue: f32) -> f32 {    use core::f32::consts::PI;
    let two_pi = 2.0 * PI;

    // 折回 [-π, π)，避免 atan2 边界外的输入。
    let h = if hue >= PI || hue < -PI {
        hue - two_pi * ((hue + PI) / two_pi).floor()
    } else {
        hue
    };

    let x = LUT_ELEM as f32 * (h + PI) / two_pi;
    let x_prev = x.floor();
    let x_next = x.ceil();
    let xi = (x_prev as i32) & (LUT_ELEM as i32 - 1);
    let xii = (x_next as i32) & (LUT_ELEM as i32 - 1);
    let y_prev = lut[xi as usize];
    if xi == xii {
        y_prev
    } else {
        y_prev + (x - x_prev) * (lut[xii as usize] - y_prev)
    }
}

/// Lightroom 8 个色带的中心色相（度，常规色相：红 0°、品红 315°）。
pub const LR_BAND_HUES: [f32; NODES] = [0.0, 30.0, 60.0, 120.0, 180.0, 225.0, 270.0, 315.0];



/// 周期分段线性插值，输出 512 点 LUT（域为 UCS 色相 `[-π, π)`）。
///
/// `angles_deg` 是常规色相（升序），LUT 采样点按 UCS 色相排布（含 20° 偏移）。
fn periodic_linear(values: [f32; NODES], angles_deg: [f32; NODES]) -> [f32; LUT_ELEM] {
    let mut lut = [0.0f32; LUT_ELEM];
    for (index, slot) in lut.iter_mut().enumerate() {
        let deg = index as f32 * 360.0 / LUT_ELEM as f32;

        let mut k = NODES - 1;
        for (j, angle) in angles_deg.iter().enumerate() {
            if *angle <= deg {
                k = j;
            }
        }
        let a0 = angles_deg[k];
        let a1 = if k == NODES - 1 {
            angles_deg[0] + 360.0
        } else {
            angles_deg[k + 1]
        };
        let d = if k == NODES - 1 {
            deg + 360.0 - a0
        } else {
            deg - a0
        };
        let t = if a1 > a0 {
            (d / (a1 - a0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let v0 = values[k];
        let v1 = values[(k + 1) % NODES];
        *slot = v0 + (v1 - v0) * t;
    }
    lut
}

/// HSL 面板（Lightroom 8 段）的三条曲线，坐标系与均衡器一致（dt UCS）。
#[derive(Clone, Debug)]
pub struct HslCurves {
    /// 色相偏移（弧度）。
    pub hue: [f32; LUT_ELEM],
    /// 饱和度增益。
    pub saturation: [f32; LUT_ELEM],
    /// 亮度增益。
    pub brightness: [f32; LUT_ELEM],
}

impl HslCurves {
    /// 由 8 段滑杆值（-100..100）烘焙三条曲线。
    ///
    /// 行程沿用旧 HSL 面板：色相 ±30°、饱和 +100 ≈ ×1.6 / −100 → ×0、亮度 ±18%。
    ///
    /// 注意：Lightroom 的 8 个色带**不是等距**（0/30/60/120/180/225/270/315），
    /// 周期 RBF 在这种疏密不均的节点上会过冲（实测节点值 6° 能冲到 37°），
    /// 所以这里用**周期分段线性**插值——与 darktable 的 color zones 一致；
    /// 等距节点的颜色均衡器仍然用 RBF。
    pub fn from_bands(hue: [f32; NODES], saturation: [f32; NODES], luminance: [f32; NODES]) -> Self {
        let hue_nodes: [f32; NODES] = core::array::from_fn(|k| {
            (hue[k] / 100.0 * 30.0 * crate::bands::HSL_HUE_SCALE[k]).to_radians()
        });
        let sat_nodes: [f32; NODES] = core::array::from_fn(|k| {
            if saturation[k] >= 0.0 {
                1.0 + saturation[k] / 100.0 * 0.6
            } else {
                1.0 + saturation[k] / 100.0
            }
        });
        let bright_nodes: [f32; NODES] =
            core::array::from_fn(|k| 1.0 + luminance[k] / 100.0 * 0.12);

        Self {
            hue: periodic_linear(hue_nodes, LR_BAND_HUES),
            saturation: periodic_linear(sat_nodes, LR_BAND_HUES),
            brightness: periodic_linear(bright_nodes, LR_BAND_HUES),
        }
    }

    /// 三条曲线是否都是恒等（全平）。
    pub fn is_neutral(&self) -> bool {
        self.hue.iter().all(|v| v.abs() < 1e-6)
            && self.saturation.iter().all(|v| (v - 1.0).abs() < 1e-6)
            && self.brightness.iter().all(|v| (v - 1.0).abs() < 1e-6)
    }
}

/// HSL 面板的执行器：在 dt UCS 里按三条曲线调整 H/S/B，再色域软裁剪回 RGB。
///
/// 与 [`ColorEqualizer`] 的区别只有两点：节点角度用 Lightroom 的 8 个色带中心，
/// 且**不做空间滤波**（逐像素，便宜，留在原来的管线位置）。
#[derive(Clone, Debug)]
pub struct HslStage {
    matrix_in: [[f32; 3]; 3],
    matrix_out: [[f32; 3]; 3],
    /// 工作空间线性 RGB → 线性 sRGB（用 sRGB HSL 色相选带）。
    to_srgb: [[f32; 3]; 3],
    l_white: f32,
    curves: HslCurves,
    gamut: Gamut,
    neutral: bool,
}

impl HslStage {
    /// 由目标 RGB 空间与三条曲线构造。
    pub fn new(space: &RgbSpace, curves: HslCurves) -> Self {
        let matrix = crate::ucs::rgb_to_xyz_d65(space);
        let matrix_in = matrix.to_f32();
        let matrix_out = matrix
            .inverse()
            .unwrap_or(crate::math::Mat3::IDENTITY)
            .to_f32();
        let neutral = curves.is_neutral();
        Self {
            matrix_in,
            matrix_out,
            to_srgb: space.to_space(&crate::math::SRGB).to_f32(),
            l_white: l_white_of(1.0),
            curves,
            gamut: Gamut::from_rgb_to_xyz(&matrix_in),
            neutral,
        }
    }

    /// 当前曲线。
    pub fn curves(&self) -> &HslCurves {
        &self.curves
    }

    /// 是否恒等（可以整段跳过）。
    pub fn is_neutral(&self) -> bool {
        self.neutral
    }

    /// 逐像素应用（无空间滤波）。
    pub fn map(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.neutral || !rgb.iter().all(|value| value.is_finite()) {
            return rgb;
        }
        let m = &self.matrix_in;
        let xyz = [
            m[0][0] * rgb[0] + m[0][1] * rgb[1] + m[0][2] * rgb[2],
            m[1][0] * rgb[0] + m[1][1] * rgb[1] + m[1][2] * rgb[2],
            m[2][0] * rgb[0] + m[2][1] * rgb[1] + m[2][2] * rgb[2],
        ];
        let xyy = xyz_to_xyy(xyz);
        let uv = xy_to_uv([xyy[0], xyy[1]]);
        let l_star = l_star_from_y(xyy[2]);
        let jch = luv_to_jch(l_star, self.l_white, uv);
        let mut hsb = jch_to_hsb(jch);
        if jch[1] > NORM_MIN {
            // 色带按 Lightroom 的方式用 **sRGB HSL 色相** 选（曲线节点就是 LR 的 8 个中心），
            // 修正仍然在 dt UCS 里施加 —— 与 darktable color zones 的“按 HSL 选带、按 Lab 调”同构。
            let srgb_linear = [
                self.to_srgb[0][0] * rgb[0] + self.to_srgb[0][1] * rgb[1] + self.to_srgb[0][2] * rgb[2],
                self.to_srgb[1][0] * rgb[0] + self.to_srgb[1][1] * rgb[1] + self.to_srgb[1][2] * rgb[2],
                self.to_srgb[2][0] * rgb[0] + self.to_srgb[2][1] * rgb[1] + self.to_srgb[2][2] * rgb[2],
            ];
            let encoded = [
                crate::transfer::linear_to_srgb(srgb_linear[0].clamp(0.0, 1.0)),
                crate::transfer::linear_to_srgb(srgb_linear[1].clamp(0.0, 1.0)),
                crate::transfer::linear_to_srgb(srgb_linear[2].clamp(0.0, 1.0)),
            ];
            let band_hue = crate::perceptual::rgb_to_hsl(encoded)[0] * 360.0;
            let hue = hsb[0];
            hsb[0] = hue + lookup_hsl(&self.curves.hue, band_hue);
            hsb[1] = (hsb[1] * lookup_hsl(&self.curves.saturation, band_hue)).max(0.0);
            hsb[2] = (hsb[2] * lookup_hsl(&self.curves.brightness, band_hue)).max(0.0);
            self.gamut.map_hsb(&mut hsb, self.l_white);
        }
        let out = hsb_to_xyz(hsb, self.l_white);
        let mo = &self.matrix_out;
        [
            mo[0][0] * out[0] + mo[0][1] * out[1] + mo[0][2] * out[2],
            mo[1][0] * out[0] + mo[1][1] * out[1] + mo[1][2] * out[2],
            mo[2][0] * out[0] + mo[2][1] * out[1] + mo[2][2] * out[2],
        ]
    }
}

/// 查表：`deg` 是 **sRGB HSL 色相**（度，0..360），线性插值、周期回绕。
#[inline]
fn lookup_hsl(lut: &[f32; LUT_ELEM], deg: f32) -> f32 {
    let x = deg.rem_euclid(360.0) / 360.0 * LUT_ELEM as f32;
    let base = x.floor();
    let index = (base as usize) % LUT_ELEM;
    let next = (index + 1) % LUT_ELEM;
    lut[index] + (x - base) * (lut[next] - lut[index])
}

/// 颜色均衡器的三个通道（给 UI 画曲线用）。#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// 色相偏移（弧度）。
    Hue,
    /// 饱和度增益。
    Saturation,
    /// 亮度增益。
    Brightness,
}

/// 单通道 512 点色相曲线（定义域 `[-π, π)`），与 [`ColorEqualizer`] 内部烘焙的 LUT 完全一致。
///
/// 只跑一次 8 节点周期 RBF，**不建色域边界表**，所以可以在 UI 里每帧调用（微秒级）。
pub fn curve(params: &EqualizerParams, channel: Channel) -> [f32; LUT_ELEM] {
    use core::f32::consts::PI;
    match channel {
        Channel::Saturation => periodic_rbf(params.saturation, PI, params, true),
        Channel::Brightness => periodic_rbf(params.brightness, PI, params, true),
        Channel::Hue => {
            let nodes: [f32; NODES] = core::array::from_fn(|k| params.hue[k].to_radians());
            periodic_rbf(
                nodes,
                PI / params.smoothing_hue.clamp(0.05, 8.0),
                params,
                false,
            )
        }
    }
}

/// 8 节点周期 RBF 插值，生成 512 点色相 LUT。
///
/// 核 `k(l) = exp(-l²/smoothing)`，特征 `φ(Δ) = exp(Σ_l k(l)·cos(l·Δ))`；
/// 先解 `AᵀA λ = Aᵀy`（最小二乘，与参考实现一致），再用同一组基展开整圈。
#[allow(clippy::needless_range_loop)]
fn periodic_rbf(
    nodes: [f32; NODES],
    smoothing: f32,
    params: &EqualizerParams,
    clip: bool,
) -> [f32; LUT_ELEM] {
    let angles: [f64; NODES] = core::array::from_fn(|k| {
        ((k as f32) * 360.0 / NODES as f32 + params.hue_shift + ANGLE_SHIFT).to_radians() as f64
    });
    periodic_rbf_at(nodes, smoothing, angles, clip)
}

/// 用显式节点角度（**常规色相，度**）烘焙一条曲线；内部自动加 20° 的 UCS 偏移。
///
/// HSL 面板用它把 Lightroom 的 8 个色带中心（0/30/60/120/180/225/270/315）
/// 变成一条连续曲线，和均衡器共用同一套 RBF。
pub fn bake_curve(
    values: [f32; NODES],
    angles_deg: [f32; NODES],
    smoothing: f32,
    clip: bool,
) -> [f32; LUT_ELEM] {
    let angles: [f64; NODES] =
        core::array::from_fn(|k| (angles_deg[k] + ANGLE_SHIFT).to_radians() as f64);
    periodic_rbf_at(values, smoothing, angles, clip)
}

/// 同 [`periodic_rbf`]，但节点角度已经换算成 UCS 弧度。
#[allow(clippy::needless_range_loop)]
fn periodic_rbf_at(
    nodes: [f32; NODES],
    smoothing: f32,
    angles: [f64; NODES],
    clip: bool,
) -> [f32; LUT_ELEM] {
    use core::f32::consts::PI;

    let terms = (3.0 * smoothing.sqrt()).ceil().max(1.0) as usize;
    let kernel: Vec<f64> = (0..terms)
        .map(|l| (-((l * l) as f64) / smoothing as f64).exp())
        .collect();

    // A[i][j] = exp(Σ_l k(l)·cos(l·|θi − θj|))
    let mut a = [[0.0f64; NODES]; NODES];
    for i in 0..NODES {
        for j in 0..NODES {
            let mut acc = 0.0;
            for (l, kl) in kernel.iter().enumerate() {
                acc += kl * ((l as f64) * (angles[i] - angles[j]).abs()).cos();
            }
            a[i][j] = acc.exp();
        }
    }

    // 正规方程 AᵀA λ = Aᵀy
    let mut normal = [[0.0f64; NODES]; NODES];
    for i in 0..NODES {
        for j in 0..NODES {
            let mut acc = 0.0;
            for k in 0..NODES {
                acc += a[k][i] * a[k][j];
            }
            normal[i][j] = acc;
        }
    }
    let mut rhs = [0.0f64; NODES];
    for i in 0..NODES {
        let mut acc = 0.0;
        for k in 0..NODES {
            acc += a[k][i] * nodes[k] as f64;
        }
        rhs[i] = acc;
    }
    let lambda = solve8(normal, rhs).unwrap_or([0.0; NODES]);

    let mut lut = [0.0f32; LUT_ELEM];
    for (i, slot) in lut.iter_mut().enumerate() {
        let hue = ((i as f32) * 360.0 / LUT_ELEM as f32).to_radians() - PI;
        let mut value = 0.0f64;
        for k in 0..NODES {
            let mut acc = 0.0f64;
            for (l, kl) in kernel.iter().enumerate() {
                acc += kl * ((l as f64) * (hue as f64 - angles[k]).abs()).cos();
            }
            value += lambda[k] * acc.exp();
        }
        let mut v = value as f32;
        if clip {
            v = v.max(0.0);
        }
        *slot = v;
    }
    lut
}

/// 8×8 线性方程组，部分主元高斯消元（f64）。
#[allow(clippy::needless_range_loop)]
fn solve8(mut m: [[f64; NODES]; NODES], mut rhs: [f64; NODES]) -> Option<[f64; NODES]> {
    for col in 0..NODES {
        let pivot = (col..NODES).max_by(|&i, &j| m[i][col].abs().total_cmp(&m[j][col].abs()))?;
        if m[pivot][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, pivot);
        rhs.swap(col, pivot);

        for row in col + 1..NODES {
            let factor = m[row][col] / m[col][col];
            if factor == 0.0 {
                continue;
            }
            for k in col..NODES {
                m[row][k] -= factor * m[col][k];
            }
            rhs[row] -= factor * rhs[col];
        }
    }

    let mut x = [0.0f64; NODES];
    for i in (0..NODES).rev() {
        let mut acc = rhs[i];
        for j in i + 1..NODES {
            acc -= m[i][j] * x[j];
        }
        x[i] = acc / m[i][i];
    }
    Some(x)
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::math::SRGB;

    fn image(width: usize, height: usize) -> Vec<f32> {
        let mut rgb = vec![0.0f32; width * height * 3];
        for y in 0..height {
            for x in 0..width {
                let k = (y * width + x) * 3;
                rgb[k] = 0.2 + 0.7 * (x as f32 / width as f32);
                rgb[k + 1] = 0.2 + 0.7 * (y as f32 / height as f32);
                rgb[k + 2] = 0.4;
            }
        }
        rgb
    }

    #[test]
    fn default_params_are_identity() {
        let (w, h) = (24, 16);
        let mut eq = ColorEqualizer::new(EqualizerParams::default(), &SRGB);
        let src = image(w, h);
        let mut out = src.clone();
        eq.apply(&mut out, w, h, 3);
        assert_eq!(src, out);

        // 即使改了阈值类参数，节点全是默认值时仍是恒等。
        let mut p = EqualizerParams::default();
        p.threshold = 0.25;
        p.contrast = -0.8;
        eq.set_params(&p);
        let mut out = src.clone();
        eq.apply(&mut out, w, h, 3);
        assert_eq!(src, out);
    }

    #[test]
    fn neutral_pixels_stay_neutral() {
        let mut p = EqualizerParams::default();
        p.saturation = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        p.brightness = [2.0; NODES];
        p.hue = [60.0; NODES];

        let eq = ColorEqualizer::new(p, &SRGB);
        let src = [0.35f32, 0.35, 0.35, 0.6, 0.6, 0.6];
        let mut out = src;
        eq.apply(&mut out, 2, 1, 3);
        for (i, v) in out.iter().enumerate() {
            assert!((v - src[i]).abs() < 1e-6, "{out:?}");
        }
    }

    #[test]
    fn saturation_boost_is_hue_selective() {
        let mut p = EqualizerParams::default();
        p.use_filter = false;
        p.saturation[0] = 2.0; // 红
        p.saturation[5] = 1.0; // 蓝不变

        let eq = ColorEqualizer::new(p, &SRGB);
        let mut out = vec![0.5f32, 0.05, 0.02, 0.02, 0.05, 0.5];
        eq.apply(&mut out, 2, 1, 3);

        let red_before = 0.5f32;
        let blue_before = 0.5f32;
        // RBF 的过渡是连续的，邻近色带会有少量渗出；关键是红比蓝动得多。
        assert!(out[0] > red_before, "red should be more saturated: {out:?}");
        assert!(
            out[0] - red_before > (out[5] - blue_before).abs(),
            "{out:?}"
        );
        assert!(
            (out[5] - blue_before).abs() < 0.1,
            "blue moved too much: {out:?}"
        );
    }

    #[test]
    fn hue_offset_rotates_only_its_band() {
        let mut p = EqualizerParams::default();
        p.use_filter = false;
        p.hue[0] = 25.0;
        let eq = ColorEqualizer::new(p, &SRGB);

        let mut out = vec![0.45f32, 0.03, 0.02];
        eq.apply(&mut out, 1, 1, 3);
        // 红被推向橙：绿通道上升。
        assert!(out[1] > 0.05, "{out:?}");
    }

    #[test]
    fn lookup_is_periodic() {
        let lut: [f32; LUT_ELEM] = core::array::from_fn(|i| (i as f32 * 0.01).sin());
        use core::f32::consts::PI;
        for h in [-3.0f32, -1.0, 0.0, 1.5, 3.1] {
            let a = lookup(&lut, h);
            let b = lookup(&lut, h + 2.0 * PI);
            assert!((a - b).abs() < 1e-4, "{h}: {a} vs {b}");
        }
        // 端点连续（0° 与 360° 相接）。
        assert!((lookup(&lut, -PI) - lookup(&lut, PI - 1e-4)).abs() < 0.02);
    }

    #[test]
    fn rbf_reproduces_constant_nodes() {
        let p = EqualizerParams::default();
        let lut = periodic_rbf([0.75; NODES], core::f32::consts::PI, &p, true);
        for v in lut {
            // 最小二乘 + 正规方程会引入千分之一量级的偏差。
            assert!((v - 0.75).abs() < 2e-3, "{v}");
        }
    }

    #[test]
    fn rbf_nodes_land_on_their_angles() {
        let p = EqualizerParams::default();
        let mut nodes = [0.0f32; NODES];
        nodes[0] = 1.0;
        let lut = periodic_rbf(nodes, core::f32::consts::PI, &p, false);
        let peak = lut
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        let peak_deg = peak as f32 * 360.0 / LUT_ELEM as f32 - 180.0;
        assert!((peak_deg - 20.0).abs() < 8.0, "peak at {peak_deg}°");
    }

    #[test]
    fn tiny_images_do_not_panic() {
        for (w, h) in [(1, 1), (2, 2), (3, 1)] {
            let mut p = EqualizerParams::default();
            p.saturation = [1.5; NODES];
            let eq = ColorEqualizer::new(p, &SRGB);
            let mut img = vec![0.3f32; w * h * 4];
            eq.apply(&mut img, w, h, 4);
        }
    }

    #[test]
    fn spatial_filter_smooths_isolated_color_noise() {
        // 一片中性色中夹一个高饱和像素；开滤波后，导向滤波会把修正量摊开，
        // 输出不应出现单像素突变。
        let (w, h) = (17, 17);
        let mut img = vec![0.18f32; w * h * 3];
        let center = (8 * w + 8) * 3;
        img[center] = 0.8;
        img[center + 1] = 0.1;
        img[center + 2] = 0.1;

        let mut p = EqualizerParams::default();
        p.saturation = [2.0; NODES];
        let eq = ColorEqualizer::new(p.clone(), &SRGB);

        let mut filtered = img.clone();
        eq.apply(&mut filtered, w, h, 3);

        p.use_filter = false;
        let raw = ColorEqualizer::new(p, &SRGB);
        let mut unfiltered = img.clone();
        raw.apply(&mut unfiltered, w, h, 3);

        let center_out = filtered[center];
        let neighbor = filtered[center + 3];
        assert!(
            (center_out - neighbor).abs() <= (unfiltered[center] - unfiltered[center + 3]).abs(),
            "guided filter should not increase the local contrast"
        );
    }

    #[test]
    fn warm_look_is_selective_and_stays_finite() {
        // 红/橙提饱和、蓝降饱和、黄提亮：一张“暖调”风格。
        let mut p = EqualizerParams::default();
        p.saturation = [1.5, 1.3, 1.0, 1.0, 1.0, 0.6, 0.8, 1.0];
        p.brightness = [1.05, 1.05, 1.15, 1.0, 1.0, 0.95, 1.0, 1.0];
        p.hue = [5.0, -5.0, 0.0, 0.0, 0.0, -8.0, 0.0, 0.0];

        let eq = ColorEqualizer::new(p, &SRGB);
        let (w, h) = (32, 32);
        let mut img = vec![0.0f32; w * h * 3];
        // 左半红、右半蓝、最下 4 行中性灰。
        for y in 0..h {
            for x in 0..w {
                let k = (y * w + x) * 3;
                let (r, g, b) = if y >= h - 4 {
                    (0.4, 0.4, 0.4)
                } else if x < w / 2 {
                    (0.55, 0.08, 0.05)
                } else {
                    (0.05, 0.12, 0.55)
                };
                img[k] = r;
                img[k + 1] = g;
                img[k + 2] = b;
            }
        }
        eq.apply(&mut img, w, h, 3);

        for v in &img {
            assert!(v.is_finite() && *v >= -0.01 && *v <= 1.01, "{v}");
        }

        let red = 8 * 3; // 第一行左侧（红区）
        let blue = (16 * w + 24) * 3;
        let grey = ((h - 1) * w + 8) * 3;

        // 中性灰不动（取离色块 3 像素处，避开 1~2 像素的滤波过渡带）。
        for c in 0..3 {
            assert!(
                (img[grey + c] - 0.4).abs() < 1e-3,
                "grey out={:?}",
                &img[grey..grey + 3]
            );
        }
        // 红被提饱和：R 与 G 的差距变大。
        assert!(
            img[red] - img[red + 1] > 0.55 - 0.08,
            "red contrast {}",
            img[red] - img[red + 1]
        );
        // 蓝被降饱和：B 与 R 的差距变小。
        assert!(
            img[blue + 2] - img[blue] < 0.55 - 0.05,
            "blue contrast {}",
            img[blue + 2] - img[blue]
        );
    }
}

