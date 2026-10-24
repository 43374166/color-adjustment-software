//! 调色段：把 [`EditParams`] 接到 `fdxmp-color`，让 CPU（导出 / 移动端预览 / 测试）
//! 与 GPU（`color_pipeline.wgsl`）跑同一份数学。
//!
//! - [`ColorStage`]：CPU 侧实现（相机校准 → 色彩算子）；
//! - [`pack_color_slots`]：把这些面板打包进 GPU uniform（槽位定义在
//!   [`crate::gpu_pipeline::slot`]，着色器按同一顺序还原）。
//!
//! 色彩算子全部在 **OkLab / OkLCh** 上运算（自然饱和度的低饱和加权与肤色保护、
//! 8 色带分割权重、点颜色的软范围、三分级的混合 / 平衡掩码、黑白混色），
//! 算法来源与许可见 `crates/color/README.md`。

use fdxmp_color::{ColorOps, Grading, Hsl, Mixer, Wheel, SRGB};

use crate::params::{ColorBand, EditParams};

/// 工作空间：与 `color_pipeline.wgsl` 一致（线性 sRGB / Rec.709）。
pub const WORKSPACE: fdxmp_color::RgbSpace = SRGB;

/// GPU 支持的点颜色样本数（与 `slot::POINT_START` 后的槽位一致）。
pub const POINT_SAMPLES: usize = 4;

/// 8 色带中心的 OkLCh 色相（弧度）——CPU 打包给 GPU 的同一张表。
///
/// 工具与测试用它定位色带（例如取相邻两带的正中点）。
pub fn band_hues() -> &'static [f32; 8] {
    fdxmp_color::bands::band_hues()
}

/// OkLCh → 工作空间线性 RGB（工具 / 测试用；与调色段同一套换算）。
pub fn oklch_to_linear(l: f32, chroma: f32, hue_rad: f32) -> [f32; 3] {
    fdxmp_color::rgb_from_oklab(&WORKSPACE, fdxmp_color::lch_to_lab([l, chroma, hue_rad]))
}

/// 8bit sRGB → OkLCh（`[L, C, 色相(弧度)]`），供四端"点颜色"取色器使用。
///
/// 与调色段同一套矩阵：取到的样本直接就是 `PointColorParams` 的
/// `lum` / `chroma` / `hue` 三个字段。
pub fn oklch_from_srgb8(r: u8, g: u8, b: u8) -> [f32; 3] {
    let linear = [
        fdxmp_color::transfer::srgb_to_linear(r as f32 / 255.0),
        fdxmp_color::transfer::srgb_to_linear(g as f32 / 255.0),
        fdxmp_color::transfer::srgb_to_linear(b as f32 / 255.0),
    ];
    fdxmp_color::lab_to_lch(fdxmp_color::oklab_from_rgb(&WORKSPACE, linear))
}

/// 一个画面要用的全部调色参数（构建一次，逐像素应用）。
#[derive(Clone, Debug)]
pub struct ColorStage {
    ops: ColorOps,
    calibration: Option<[[f32; 3]; 3]>,
    shadow_tint: f32,
}

impl ColorStage {
    /// 从编辑参数构建。
    pub fn from_params(params: &EditParams) -> Self {
        let monochrome = params.color.black_white;
        let bands = ColorBand::ALL.map(|band| {
            let hsl = params.hsl.band(band);
            Hsl {
                hue: hsl.hue as f64,
                sat: hsl.saturation as f64,
                lum: hsl.luminance as f64,
            }
        });
        let mixer = Mixer { bands };
        let zone = |zone: &crate::params::GradingZone| Wheel {
            hue: zone.hue as f64,
            sat: zone.saturation as f64,
            lum: zone.luminance as f64,
        };
        let grading = Grading {
            shadows: zone(&params.grading.shadows),
            midtones: zone(&params.grading.midtones),
            highlights: zone(&params.grading.highlights),
            global: Wheel::default(),
            blending: params.grading.blending as f64,
            balance: params.grading.balance as f64,
        };

        let mut ops = ColorOps::new(WORKSPACE);
        // 黑白模式：色相 / 饱和度混色与点颜色让位给 8 色带黑白混色（Lightroom 语义）。
        // 注意单位：滑杆是 -100..100，`ColorOps` 的这两个字段是归一化值（-1..1）。
        ops.vibrance = if monochrome {
            0.0
        } else {
            params.color.vibrance as f64 / 100.0
        };
        ops.saturation = if monochrome {
            0.0
        } else {
            params.color.saturation as f64 / 100.0
        };
        ops.mixer = (!monochrome && !mixer.is_neutral()).then_some(mixer);
        ops.points = if monochrome {
            Vec::new()
        } else {
            params
                .point_colors
                .iter()
                .take(POINT_SAMPLES)
                .map(|point| point.to_engine())
                .collect()
        };
        ops.bw = monochrome.then(|| params.bw_mix.bands().map(|value| value as f64));
        ops.grading = Some(grading);

        let calibration =
            fdxmp_color::calibration_matrix(&params.calibration.to_engine(), WORKSPACE);
        let shadow_tint = (params.calibration.shadows_tint / 100.0).clamp(-1.0, 1.0);
        Self {
            ops,
            calibration,
            shadow_tint,
        }
    }

    /// 全部为默认值时（GPU / CPU 都可跳过整个调色段）。
    pub fn is_identity(&self) -> bool {
        self.ops.is_identity() && self.calibration.is_none() && self.shadow_tint == 0.0
    }

    /// 相机校准（场景线性：白平衡之后、曝光之前）。
    pub fn apply_calibration(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.calibration.is_none() && self.shadow_tint == 0.0 {
            return rgb;
        }
        fdxmp_color::calibrate(
            rgb,
            self.calibration.as_ref(),
            self.shadow_tint,
            WORKSPACE.luma().map(|value| value as f32),
        )
    }

    /// 色彩算子（自然饱和度 / 8 色带 / 点颜色 / 黑白 / 分级）。
    pub fn apply_color(&self, rgb: [f32; 3]) -> [f32; 3] {
        self.ops.apply(rgb)
    }

    /// 校准 + 色彩算子（测试与导出用；GPU 里两段分别在不同位置应用）。
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        self.apply_color(self.apply_calibration(rgb))
    }
}

/// 把调色参数写进 GPU uniform（槽位见 [`crate::gpu_pipeline::slot`]）。
#[cfg(feature = "gpu")]
pub fn pack_color_slots(data: &mut [f32], params: &EditParams) {
    use crate::gpu_pipeline::slot;

    // 8 色带中心（OkLCh 色相，弧度）：GPU 侧不做 HSV→OkLCh 换算。
    for (index, hue) in fdxmp_color::bands::band_hues().iter().enumerate() {
        data[slot::BAND_HUES + index] = *hue;
    }

    // 黑白混色。
    data[slot::BW_ENABLE] = if params.color.black_white { 1.0 } else { 0.0 };
    for (index, value) in params.bw_mix.bands().iter().enumerate() {
        data[slot::BW_MIX + index] = *value;
    }

    // 相机校准（基色矩阵 + 阴影色调）。
    let calibration = fdxmp_color::calibration_matrix(&params.calibration.to_engine(), WORKSPACE);
    let tint = (params.calibration.shadows_tint / 100.0).clamp(-1.0, 1.0);
    data[slot::CALIB_ENABLE] = if calibration.is_some() || tint != 0.0 {
        1.0
    } else {
        0.0
    };
    if let Some(matrix) = calibration {
        for row in 0..3 {
            for col in 0..3 {
                data[slot::CALIB_MATRIX + row * 3 + col] = matrix[row][col];
            }
        }
    }
    data[slot::CALIB_TINT] = tint;

    // 点颜色（中性样本不占槽位）。
    let points: Vec<_> = params
        .point_colors
        .iter()
        .take(POINT_SAMPLES)
        .filter(|point| !point.is_neutral())
        .collect();
    data[slot::POINT_COUNT] = points.len() as f32;
    let stride = fdxmp_color::ops::POINT_WORDS;
    for (index, point) in points.iter().enumerate() {
        let words = fdxmp_color::ops::point_slots(&point.to_engine());
        for (word, value) in words.iter().enumerate() {
            data[slot::POINT_START + index * stride + word] = *value;
        }
    }

    // 分级色轮：色相在 CPU 侧换算成 OkLCh 的 a / b 向量，着色器只做加法。
    let wheel = |zone: &crate::params::GradingZone| {
        let hue = fdxmp_color::bands::oklch_hue_of_srgb_hue(zone.hue as f64);
        let sat = (zone.saturation as f64 / 100.0) as f32 * 0.09;
        (
            sat * hue.cos(),
            sat * hue.sin(),
            (zone.luminance as f64 / 100.0) as f32 * 0.12,
        )
    };
    let zones = [
        wheel(&params.grading.shadows),
        wheel(&params.grading.midtones),
        wheel(&params.grading.highlights),
    ];
    for (index, (a, b, lum)) in zones.iter().enumerate() {
        data[slot::GRADE_VEC + index * 3] = *a;
        data[slot::GRADE_VEC + index * 3 + 1] = *b;
        data[slot::GRADE_VEC + index * 3 + 2] = *lum;
    }
    // 全局轮（当前 UI 未开放，保留槽位给后续"全局"色轮）。
    for index in 9..12 {
        data[slot::GRADE_VEC + index] = 0.0;
    }
    let balance = (params.grading.balance / 100.0).clamp(-1.0, 1.0);
    let blending = (params.grading.blending / 100.0).clamp(0.0, 1.0);
    data[slot::GRADE_M] = 0.5 - balance * 0.25;
    data[slot::GRADE_WIDTH] = 0.15 + blending * 0.5;
    data[slot::SKIN_HUE] = fdxmp_color::bands::oklch_hue_of_srgb_hue(25.0);
    data[slot::COLOR_ACTIVE] = if ColorStage::from_params(params).is_identity() {
        0.0
    } else {
        1.0
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::PointColorParams;

    fn stage(params: &EditParams) -> ColorStage {
        ColorStage::from_params(params)
    }

    #[test]
    fn default_params_are_identity() {
        let params = EditParams::default();
        let stage = stage(&params);
        assert!(stage.is_identity());
        let rgb = [0.2, 0.35, 0.6];
        let out = stage.apply(rgb);
        for (actual, expected) in out.iter().zip(rgb) {
            assert!((actual - expected).abs() < 1e-6, "{out:?}");
        }
    }

    #[test]
    fn saturation_minus_hundred_removes_colour() {
        let mut params = EditParams::default();
        params.color.saturation = -100.0;
        let out = stage(&params).apply([0.4, 0.1, 0.05]);
        assert!(
            (out[0] - out[1]).abs() < 3e-3 && (out[1] - out[2]).abs() < 3e-3,
            "{out:?}"
        );
    }

    #[test]
    fn cross_band_mixer_targets_only_its_band() {
        let mut params = EditParams::default();
        params.hsl.blue.saturation = -100.0;
        let stage = stage(&params);
        let blue = stage.apply([0.02, 0.05, 0.4]);
        let red = stage.apply([0.4, 0.03, 0.02]);
        let chroma = |c: [f32; 3]| {
            let lab = fdxmp_color::oklab_from_rgb(&WORKSPACE, c);
            fdxmp_color::lab_to_lch(lab)[1]
        };
        assert!(chroma(blue) < 0.02, "{}", chroma(blue));
        assert!((chroma(red) - chroma([0.4, 0.03, 0.02])).abs() < 0.01);
    }

    #[test]
    fn grading_tints_shadows_blue() {
        let mut params = EditParams::default();
        params.grading.shadows.hue = 240.0;
        params.grading.shadows.saturation = 100.0;
        let out = stage(&params).apply([0.01, 0.01, 0.01]);
        assert!(out[2] > out[0], "{out:?}");
    }

    #[test]
    fn point_colour_only_touches_its_range() {
        let target = [0.35, 0.16, 0.09];
        let lab = fdxmp_color::oklab_from_rgb(&WORKSPACE, target);
        let lch = fdxmp_color::lab_to_lch(lab);
        let mut params = EditParams::default();
        params.point_colors.push(PointColorParams {
            lum: lch[0],
            chroma: lch[1],
            hue: lch[2].to_degrees(),
            hue_shift: 60.0,
            sat_shift: -50.0,
            ..Default::default()
        });
        let stage = stage(&params);
        let out = stage.apply(target);
        let after = fdxmp_color::lab_to_lch(fdxmp_color::oklab_from_rgb(&WORKSPACE, out));
        assert!(after[1] < lch[1] * 0.8, "{} -> {}", lch[1], after[1]);
        let far = [0.02, 0.05, 0.4];
        let untouched = stage.apply(far);
        for index in 0..3 {
            assert!(
                (untouched[index] - far[index]).abs() < 1e-4,
                "{untouched:?}"
            );
        }
    }

    #[test]
    fn monochrome_mix_brightens_its_band() {
        let blue = [0.05, 0.1, 0.5];
        let mut params = EditParams::default();
        params.color.black_white = true;
        let grey = stage(&params).apply(blue);
        assert!((grey[0] - grey[2]).abs() < 3e-3, "应为单色：{grey:?}");
        params.bw_mix.blue = 80.0;
        let lifted = stage(&params).apply(blue);
        assert!(lifted[1] > grey[1], "{grey:?} -> {lifted:?}");
    }

    #[test]
    fn calibration_keeps_neutral_and_rotates_primaries() {
        let mut params = EditParams::default();
        params.calibration.red_hue = 60.0;
        params.calibration.blue_sat = -50.0;
        let stage = stage(&params);
        assert!(!stage.is_identity());
        let grey = stage.apply_calibration([0.3, 0.3, 0.3]);
        for value in grey {
            assert!((value - 0.3).abs() < 5e-3, "{grey:?}");
        }
        let red = [0.4, 0.05, 0.03];
        let before = fdxmp_color::lab_to_lch(fdxmp_color::oklab_from_rgb(&WORKSPACE, red))[2];
        let after = fdxmp_color::lab_to_lch(fdxmp_color::oklab_from_rgb(
            &WORKSPACE,
            stage.apply_calibration(red),
        ))[2];
        assert!(after - before > 0.05, "{before} -> {after}");
    }

    #[test]
    fn colour_picker_sample_matches_the_stage() {
        // 纯蓝样本：取色器给出的 OkLCh 必须与调色段看到的同一颜色一致。
        let sample = oklch_from_srgb8(40, 80, 200);
        let linear = [
            fdxmp_color::transfer::srgb_to_linear(40.0 / 255.0),
            fdxmp_color::transfer::srgb_to_linear(80.0 / 255.0),
            fdxmp_color::transfer::srgb_to_linear(200.0 / 255.0),
        ];
        let again = fdxmp_color::lab_to_lch(fdxmp_color::oklab_from_rgb(&WORKSPACE, linear));
        for index in 0..3 {
            assert!(
                (sample[index] - again[index]).abs() < 1e-6,
                "{sample:?} vs {again:?}"
            );
        }
        let hue = sample[2].to_degrees().rem_euclid(360.0);
        assert!(
            (230.0..290.0).contains(&hue),
            "蓝色样本色相应在蓝带附近：{hue}"
        );
        // 中性灰没有彩度。
        let grey = oklch_from_srgb8(128, 128, 128);
        assert!(grey[1] < 1e-4, "{grey:?}");
    }

    #[test]
    fn ios_json_payload_maps_into_the_stage() {
        // iOS `EditJSON.build` 写出的键名必须能被 Rust 侧解析（跨语言契约）：
        // 每个端新增参数时，这个测试是唯一的自动检查点。
        let json = r#"{
            "version": 1,
            "light": {"exposure": 0, "contrast": 0, "highlights": 0, "shadows": 0, "whites": 0, "blacks": 0},
            "color": {"temperature": 0, "tint": 0, "vibrance": 0, "saturation": 0, "black_white": true},
            "calibration": {"red_hue": 25, "red_sat": 0, "green_hue": 0, "green_sat": 0, "blue_hue": 0, "blue_sat": -30, "shadows_tint": 15},
            "bw_mix": {"red": 0, "orange": 20, "yellow": 0, "green": 0, "aqua": 0, "blue": -45, "purple": 0, "magenta": 0},
            "point_colors": [{"lum": 0.45, "chroma": 0.15, "hue": 264, "hue_shift": 40, "sat_shift": -40, "lum_shift": 0, "variance": 0, "range": 50, "hue_range": 50, "sat_range": 50, "lum_range": 50}]
        }"#;
        let params: EditParams = serde_json::from_str(json).expect("iOS 参数 JSON 应可解析");
        assert!(params.color.black_white);
        assert_eq!(params.calibration.red_hue, 25.0);
        assert_eq!(params.calibration.blue_sat, -30.0);
        assert_eq!(params.calibration.shadows_tint, 15.0);
        assert_eq!(params.bw_mix.blue, -45.0);
        assert_eq!(params.point_colors.len(), 1);
        assert_eq!(params.point_colors[0].hue, 264.0);

        let stage = ColorStage::from_params(&params);
        assert!(!stage.is_identity());
        // 黑白模式下无论校准怎么改，输出都是单色（分级未开启）。
        let out = stage.apply([0.2, 0.35, 0.6]);
        assert!((out[0] - out[2]).abs() < 0.02, "{out:?}");
    }
}
