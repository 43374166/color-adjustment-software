//! 色彩管线：阶段顺序 + CPU 参考实现。
//!
//! 阶段 2 会用 wgpu compute shader 实现完整管线；这里的 CPU 参考实现
//! 用于单元测试与色彩正确性比对（golden image 基准）。

use serde::{Deserialize, Serialize};

use crate::params::EditParams;

/// 管线阶段（顺序与文档一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
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

/// 阶段执行顺序（文档规定的 15 项，自上而下）。
pub const STAGE_ORDER: [Stage; 15] = [
    Stage::Exposure,
    Stage::Contrast,
    Stage::Highlights,
    Stage::Shadows,
    Stage::Whites,
    Stage::Blacks,
    Stage::Temperature,
    Stage::Tint,
    Stage::Vibrance,
    Stage::Saturation,
    Stage::Texture,
    Stage::Clarity,
    Stage::Dehaze,
    Stage::Vignette,
    Stage::Grain,
];

/// CPU 参考实现：基础调色段（白平衡 → 曝光 → 对比度），与 GPU 着色器同公式。
///
/// 对应 `shaders/color_pipeline.wgsl::pipeline_base` 的前四步：
/// Bradford 色适应矩阵、`2^曝光`、`1+1.5a` / `1/(1-1.5a)`（pivot 0.18）。
/// 色调分区、曲线、HSL、分级等阶段不在本函数范围内。
///
/// 输入输出均为线性 RGB（允许 > 1.0 的高光余量）。
pub fn apply_reference(mut rgb: [f32; 3], params: &EditParams) -> [f32; 3] {
    // 白平衡（与 GPU 共用同一色适应矩阵实现）
    let matrix =
        crate::white_balance::adaptation_matrix(params.color.temperature, params.color.tint);
    rgb = crate::white_balance::apply_matrix(&matrix, rgb);

    let exposure = 2f32.powf(params.light.exposure);
    for channel in &mut rgb {
        *channel *= exposure;
    }

    // 对比度：正半轴线性放大、负半轴平滑压平（任意取值都不反相）
    let amount = (params.light.contrast / 100.0).clamp(-1.0, 1.0);
    let factor = if amount >= 0.0 {
        1.0 + 1.5 * amount
    } else {
        1.0 / (1.0 - 1.5 * amount)
    };
    for channel in &mut rgb {
        *channel = ((*channel - 0.18) * factor + 0.18).max(0.0);
    }
    rgb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_order_matches_spec() {
        assert_eq!(STAGE_ORDER.len(), 15);
        assert_eq!(STAGE_ORDER[0], Stage::Exposure);
        assert_eq!(STAGE_ORDER[14], Stage::Grain);
    }

    #[test]
    fn default_params_are_identity() {
        let params = EditParams::default();
        let rgb = apply_reference([0.25, 0.5, 0.75], &params);
        // 白平衡矩阵在 0/0 时是浮点意义上的恒等
        for (actual, expected) in rgb.iter().zip([0.25, 0.5, 0.75]) {
            assert!(
                (actual - expected).abs() < 1e-5,
                "默认参数应为恒等：{rgb:?}"
            );
        }
    }

    #[test]
    fn plus_one_ev_doubles_linear_value() {
        let mut params = EditParams::default();
        params.light.exposure = 1.0;
        let rgb = apply_reference([0.25, 0.0, 1.0], &params);
        assert!((rgb[0] - 0.5).abs() < 1e-6);
        assert!((rgb[2] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn negative_contrast_matches_shader_and_does_not_invert() {
        let mut params = EditParams::default();
        params.light.contrast = -100.0;
        let black = apply_reference([0.0, 0.0, 0.0], &params);
        let white = apply_reference([1.0, 1.0, 1.0], &params);
        // 系数 1/(1+1.5) = 0.4：黑 → 0.108，白 → 0.508（与着色器一致，且不反相）
        assert!(
            (black[0] - 0.108).abs() < 1e-4,
            "黑场应被压平到中灰附近：{black:?}"
        );
        assert!((white[0] - 0.508).abs() < 1e-4, "白场应被压平：{white:?}");
        assert!(black[0] < white[0], "对比度 -100 不得反相");
    }

    #[test]
    fn white_balance_reference_warms_and_cools() {
        let mut warm_params = EditParams::default();
        warm_params.color.temperature = 100.0;
        let warm = apply_reference([0.18, 0.18, 0.18], &warm_params);
        assert!(warm[0] > warm[2], "温度 +100 应偏暖：{warm:?}");

        let mut cool_params = EditParams::default();
        cool_params.color.temperature = -100.0;
        let cool = apply_reference([0.18, 0.18, 0.18], &cool_params);
        assert!(cool[2] > cool[0], "温度 -100 应偏冷：{cool:?}");
    }
}
