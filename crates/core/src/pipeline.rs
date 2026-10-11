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
/// Bradford 色适应矩阵、`2^曝光`、显示空间锚定端点的对比度 S 曲线。
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

    // 对比度：Lightroom 式显示空间 S 曲线——端点锚定（0 → 0、1 → 1，
    // 不压死暗部、不裁白），中段斜率随强度增加（±100 → 中段约 ±0.8× 混合，
    // 即 +100 中段斜率 ≈ 1.4×）；逐通道在 sRGB 空间做，超出 1 的高光余量原样保留。
    // 与着色器 `pipeline_base` 同公式。
    let amount = (params.light.contrast / 100.0).clamp(-1.0, 1.0) * 0.8;
    if amount.abs() > 1e-4 {
        for channel in &mut rgb {
            let headroom = (*channel - 1.0).max(0.0);
            let s = linear_to_srgb(*channel);
            let smooth = s * s * (3.0 - 2.0 * s);
            let curved = s + amount * (smooth - s);
            *channel = srgb_to_linear(curved) + headroom;
        }
    }
    rgb
}

/// 线性 → sRGB（0..1；超出 1 的部分由调用方单独保留）。
fn linear_to_srgb(value: f32) -> f32 {
    let x = value.clamp(0.0, 1.0);
    if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB → 线性。
fn srgb_to_linear(value: f32) -> f32 {
    let x = value.clamp(0.0, 1.0);
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
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
    fn contrast_anchors_endpoints() {
        // Lightroom 式 S 曲线：黑 / 白两端锚定（0 → 0、1 → 1，不裁切），
        // 与着色器 `pipeline_base` 同公式。
        for contrast in [-100.0f32, 100.0] {
            let mut params = EditParams::default();
            params.light.contrast = contrast;
            let black = apply_reference([0.0, 0.0, 0.0], &params);
            let white = apply_reference([1.0, 1.0, 1.0], &params);
            assert!(black[0].abs() < 1e-5, "黑场应保持 0：{black:?}");
            assert!((white[0] - 1.0).abs() < 1e-5, "白场应保持 1：{white:?}");
        }
    }

    #[test]
    fn positive_contrast_keeps_shadows_and_highlights_alive() {
        let mut params = EditParams::default();
        params.light.contrast = 100.0;
        // 中灰锚定：sRGB 0.5（线性 ≈ 0.2140）基本不动。
        let mid = apply_reference([0.2140, 0.2140, 0.2140], &params);
        assert!((mid[0] - 0.2140).abs() < 1e-3, "中灰应锚定：{mid:?}");
        // 暗部压暗但**不压死**（旧实现会裁到 0）。
        let shadow = apply_reference([0.02, 0.02, 0.02], &params);
        assert!(shadow[0] > 0.004, "暗部不应被压到黑：{shadow:?}");
        assert!(shadow[0] < 0.02, "暗部应被压暗：{shadow:?}");
        // 高光提亮但不裁白。
        let highlight = apply_reference([0.9, 0.9, 0.9], &params);
        assert!(highlight[0] > 0.9, "高光应被提亮：{highlight:?}");
        assert!(highlight[0] < 1.0, "高光不应被裁白：{highlight:?}");
        // 高光余量（> 1）原样保留。
        let headroom = apply_reference([1.4, 1.4, 1.4], &params);
        assert!(
            (headroom[0] - 1.4).abs() < 1e-4,
            ">1 的高光余量应保持不变：{headroom:?}"
        );
    }

    #[test]
    fn negative_contrast_lifts_shadows_without_inverting() {
        let mut params = EditParams::default();
        params.light.contrast = -100.0;
        let shadow = apply_reference([0.02, 0.02, 0.02], &params);
        let black = apply_reference([0.0, 0.0, 0.0], &params);
        let white = apply_reference([1.0, 1.0, 1.0], &params);
        assert!(shadow[0] > 0.02, "对比度 -100 应提亮暗部：{shadow:?}");
        assert!(black[0] < white[0], "对比度 -100 不得反相");
        assert!(shadow[0] < 1.0, "提亮不应越界：{shadow:?}");
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
