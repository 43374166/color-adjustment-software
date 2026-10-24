//! 场景线性色调段：局部基底层 → 胶片色调映射 → 色域映射。
//!
//! 与 GPU `color_pipeline.wgsl` 的 `pipeline_tone` 同公式：
//!
//! 1. **局部色调（高光 / 阴影）**：权重取自**基底层**（大半径模糊后的场景亮度），
//!    而不是像素自己的亮度——小面积高光不会被"顺手"压暗，大面积天空才会被恢复，
//!    这是无光晕的关键（LightCraft 用引导滤波，这里用大半径模糊 + 下采样双边近似）；
//! 2. **胶片色调映射**：`fdxmp_color::ToneMap::film`（对比度作用于对数域、
//!    白色移动肩部、黑色是软膝趾部），按亮度比缩放 RGB（保持色相与通道比例）；
//! 3. **色域映射**：`fdxmp_color::gamut::map_to_gamut`（保色相去饱和）。
//!
//! 常量与权重形状参考 LightCraft（MIT OR Apache-2.0）的 `crates/pipeline/src/{tone,finish}.rs`。

use fdxmp_color::gamut::map_to_gamut;
use fdxmp_color::{luminance, ToneMap};

use crate::params::EditParams;

/// 场景中灰（线性）。
pub const GREY: f32 = 0.18;

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

#[inline]
fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 场景线性的色调段（构建一次，逐像素应用）。
#[derive(Clone, Debug)]
pub struct ToneStage {
    map: ToneMap,
    highlights: f32,
    shadows: f32,
    blacks: f32,
    active: bool,
    luma: [f32; 3],
}

impl ToneStage {
    /// 从编辑参数构建（对比度 / 白色 / 黑色 → 胶片曲线；高光 / 阴影 → 局部色调）。
    pub fn from_params(params: &EditParams) -> Self {
        let light = &params.light;
        let map = ToneMap::film(
            light.contrast as f64,
            light.whites as f64,
            light.blacks as f64,
        );
        let highlights = (light.highlights / 100.0).clamp(-1.0, 1.0);
        let shadows = (light.shadows / 100.0).clamp(-1.0, 1.0);
        let active = highlights != 0.0
            || shadows != 0.0
            || light.contrast != 0.0
            || light.whites != 0.0
            || light.blacks != 0.0;
        Self {
            map,
            highlights,
            shadows,
            blacks: (light.blacks / 100.0).clamp(-1.0, 1.0),
            active,
            luma: crate::color_engine::WORKSPACE
                .luma()
                .map(|value| value as f32),
        }
    }

    /// 全部为默认值时（GPU / CPU 可跳过整个色调段）。
    pub fn is_identity(&self) -> bool {
        !self.active
    }

    /// 胶片曲线采样表（打包给 GPU 用，与 CPU 查表完全一致）。
    pub fn lut(&self) -> &[f32] {
        self.map.lut()
    }

    /// 场景线性 RGB → 场景线性 RGB。
    ///
    /// `base_ev` 是该像素处基底层的场景 EV（相对中灰；没有基底层时传像素自身的 EV，
    /// 退化为全局权重）。
    pub fn apply(&self, rgb: [f32; 3], base_ev: f32) -> [f32; 3] {
        if !self.active {
            return rgb;
        }
        let mut c = rgb;

        // 1) 局部色调：权重来自基底层（大半径），位移是乘性 EV（纯黑保持纯黑）。
        if self.highlights != 0.0 || self.shadows != 0.0 {
            let mut delta = 0.0f32;
            if self.shadows != 0.0 {
                let w = 1.0 - smooth(-4.8, 0.3, base_ev);
                delta += self.shadows * 1.7 * w * w.sqrt();
            }
            if self.highlights != 0.0 {
                let w = smooth(-1.0, 2.8, base_ev);
                delta += self.highlights * 1.7 * w;
            }
            let gain = delta.exp2();
            c = c.map(|value| value * gain);
        }

        // 2) 胶片色调映射：按亮度比缩放（保持色相 / 通道比例，>1 的余量随曲线收敛）。
        let y = luminance(c, self.luma).max(0.0);
        if y > 1e-6 {
            let mapped = self.map.apply(y);
            let k = mapped / y;
            c = c.map(|value| value * k);
        } else if self.blacks != 0.0 {
            // 纯黑：黑点被抬起时按曲线的黑场值输出（Lightroom 的"黑色 +"语义）。
            let floor = self.map.apply(0.0);
            c = [floor; 3];
        }

        // 3) 色域映射：保色相回到 0..1。
        let (out, _) = map_to_gamut(c, self.luma);
        out
    }

    /// 像素自身的场景 EV（没有基底层时的退化权重）。
    pub fn self_base_ev(&self, rgb: [f32; 3]) -> f32 {
        (luminance(rgb, self.luma).max(1e-6) / GREY).log2()
    }
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

    fn luma() -> [f32; 3] {
        crate::color_engine::WORKSPACE
            .luma()
            .map(|value| value as f32)
    }

    #[test]
    fn neutral_is_identity() {
        let stage = ToneStage::from_params(&EditParams::default());
        assert!(stage.is_identity());
        let rgb = [0.2, 0.35, 0.6];
        assert_eq!(stage.apply(rgb, 0.0), rgb);
    }

    #[test]
    fn film_curve_keeps_grey_and_rolls_off_highlights() {
        let mut params = EditParams::default();
        params.light.whites = -30.0; // 白点上移：高光收敛（留余量）
        let stage = ToneStage::from_params(&params);
        assert!(!stage.is_identity());
        // 中灰基本不动（曲线的锚点）。
        let grey = stage.apply([GREY, GREY, GREY], 0.0);
        assert!((grey[0] - GREY).abs() < 0.03, "{grey:?}");
        // 高光平滑收敛（肩部，不是硬切）。
        let bright = stage.apply([1.0, 1.0, 1.0], 0.0);
        assert!(bright[0] < 0.95 && bright[0] > 0.7, "{bright:?}");
        // 超出显示白点的场景余量被曲线收回显示范围。
        let hot = stage.apply([4.0, 4.0, 4.0], 0.0);
        assert!(hot[0] <= 1.0 && hot[0] > bright[0], "{hot:?}");
    }

    #[test]
    fn contrast_works_in_log_domain() {
        let mut params = EditParams::default();
        params.light.contrast = 60.0;
        let stage = ToneStage::from_params(&params);
        let base = ToneStage::from_params(&EditParams::default());
        let (dark, bright) = ([0.03f32; 3], [0.6f32; 3]);
        assert!(
            stage.apply(dark, base.self_base_ev(dark))[0]
                < base.apply(dark, base.self_base_ev(dark))[0],
            "正对比度应压暗暗部"
        );
        assert!(
            stage.apply(bright, base.self_base_ev(bright))[0]
                > base.apply(bright, base.self_base_ev(bright))[0],
            "正对比度应提亮亮部"
        );
    }

    #[test]
    fn highlights_use_the_local_base_not_the_pixel() {
        // 单个像素的高光落在中灰背景上：基底层仍接近中灰 → 权重小，高光被保留；
        // 同一亮度但面积很大的高光：基底层是亮的 → 权重接近 1，被明显压暗。
        let mut params = EditParams::default();
        params.light.highlights = -100.0;
        let stage = ToneStage::from_params(&params);
        let luma = luma();

        let (width, height) = (128usize, 128usize);
        let mid = [GREY, GREY, GREY];
        let bright = [1.2f32, 1.2, 1.2];
        let mut small = vec![mid; width * height];
        small[64 * width + 64] = bright;
        let mut large = vec![mid; width * height];
        for y in 0..height {
            for x in 0..width {
                if (34..94).contains(&x) && (34..94).contains(&y) {
                    large[y * width + x] = bright;
                }
            }
        }

        let radius = BASE_RADIUS;
        let small_base = ToneBase::build(&small, width, height, radius);
        let large_base = ToneBase::build(&large, width, height, radius);
        let small_center = small_base.ev_at(64, 64, luma);
        let large_center = large_base.ev_at(64, 64, luma);
        assert!(
            small_center < 1.0,
            "小高光处的基底应仍是中间调：{small_center}"
        );
        assert!(large_center > 2.0, "大高光处的基底应是亮的：{large_center}");
        assert!(
            large_center - small_center > 1.0,
            "基底层必须区分小 / 大面积：{small_center} vs {large_center}"
        );

        // 应用后：小高光被保留的亮度明显多于大高光。
        let small_out = stage.apply(bright, small_center);
        let large_out = stage.apply(bright, large_center);
        assert!(
            small_out[0] > large_out[0] + 0.1,
            "{small_out:?} vs {large_out:?}"
        );
        // 大面积高光确实被压暗（恢复）。
        assert!(large_out[0] < bright[0] * 0.92, "{large_out:?}");
    }

    #[test]
    fn shadows_are_multiplicative_and_keep_black() {
        let mut params = EditParams::default();
        params.light.shadows = 100.0;
        let stage = ToneStage::from_params(&params);
        let black = stage.apply([0.0, 0.0, 0.0], -6.0);
        assert!(black.iter().all(|value| *value == 0.0), "{black:?}");
        let dark = stage.apply([0.01, 0.01, 0.01], -4.0);
        assert!(dark[0] > 0.01, "{dark:?}");
        // 亮部不受阴影滑块影响。
        let bright = [0.7f32, 0.7, 0.7];
        let out = stage.apply(bright, 2.0);
        assert!((out[0] - bright[0]).abs() < 0.05, "{out:?}");
    }

    #[test]
    fn output_stays_in_gamut() {
        let mut params = EditParams::default();
        params.light.contrast = 100.0;
        params.light.highlights = -100.0;
        params.light.shadows = 100.0;
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
