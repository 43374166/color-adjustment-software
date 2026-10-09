//! 阶段 2 GPU 管线集成测试（无可用适配器时自动跳过）。
//!
//! 覆盖：恒等往返、曝光、饱和度、暗角、曲线、HSL、实时性能冒烟。

#![cfg(feature = "gpu")]

use std::time::Instant;

use image::RgbaImage as RawImage;

use fdxmp_core::gpu::GpuContext;
use fdxmp_core::gpu_pipeline::ColorPipeline;
use fdxmp_core::params::{ColorBand, CurvePoint, EditParams, MaskKind, MaskParams};

fn context() -> Option<GpuContext> {
    match GpuContext::new_blocking() {
        Ok(context) => Some(context),
        Err(err) => {
            eprintln!("跳过 GPU 测试：{err}");
            None
        }
    }
}

/// 均匀灰底 + 四角彩色块 + 水平渐变，覆盖曲线 / HSL / 暗角的判定。
fn test_image(width: u32, height: u32) -> Vec<u8> {
    let mut data = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let index = ((y * width + x) * 4) as usize;
            let ramp = (x * 255 / width.max(1)) as u8;
            let (r, g, b) = if x < width / 3 && y < height / 3 {
                (40, 80, 200) // 蓝块
            } else if x > width * 2 / 3 && y < height / 3 {
                (200, 80, 40) // 橙块
            } else {
                (ramp, ramp, ramp)
            };
            data[index] = r;
            data[index + 1] = g;
            data[index + 2] = b;
            data[index + 3] = 255;
        }
    }
    data
}

/// 在 [`test_image`] 上叠加确定性色度噪声（R/B 反向抖动）。
///
/// 颜色降噪（细节 / 平滑度）作用的是局部色度，干净合成图上几乎没有可降的
/// 色噪，差异会被压到阈值以下；这里提供一张真正含色噪的图用于该组用例。
fn test_image_with_color_noise(width: u32, height: u32) -> Vec<u8> {
    let mut data = test_image(width, height);
    let mut seed = 0x1234_5678u32;
    for chunk in data.as_chunks_mut::<4>().0 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = ((seed >> 16) & 0x1F) as i32 - 16;
        chunk[0] = (chunk[0] as i32 + noise).clamp(0, 255) as u8;
        chunk[2] = (chunk[2] as i32 - noise).clamp(0, 255) as u8;
    }
    data
}

/// 指定 HSV 的纯色测试图（hue 单位：度）。
fn hsv_test_image(width: u32, height: u32, hue: f32, saturation: f32, value: f32) -> Vec<u8> {
    let h = hue.rem_euclid(360.0) / 60.0;
    let sector = h.floor();
    let f = h - sector;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * f);
    let t = value * (1.0 - saturation * (1.0 - f));
    let (r, g, b) = match sector as i32 % 6 {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    };
    let mut data = vec![0u8; (width * height * 4) as usize];
    for chunk in data.as_chunks_mut::<4>().0 {
        chunk[0] = (r * 255.0 + 0.5) as u8;
        chunk[1] = (g * 255.0 + 0.5) as u8;
        chunk[2] = (b * 255.0 + 0.5) as u8;
        chunk[3] = 255;
    }
    data
}

fn max_channel_diff(a: &[u8], b: &[u8]) -> i32 {
    a.iter()
        .zip(b.iter())
        .map(|(left, right)| (*left as i32 - *right as i32).abs())
        .max()
        .unwrap_or(0)
}

fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
    let index = ((y * width + x) * 4) as usize;
    [rgba[index], rgba[index + 1], rgba[index + 2]]
}

fn srgb_to_linear(value: u8) -> f32 {
    let v = value as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

#[test]
fn gpu_identity_roundtrip() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (96u32, 64u32);
    let source = test_image(width, height);
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();
    pipeline.render(
        &context.device,
        &context.queue,
        &image,
        &EditParams::default(),
    );
    let output = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();

    let mut max_diff = 0i32;
    let mut worst = (0usize, [0u8; 3], [0u8; 3]);
    for (index, (a, b)) in source.iter().zip(output.iter()).enumerate() {
        if index % 4 == 3 {
            continue;
        }
        let diff = (*a as i32 - *b as i32).abs();
        if diff > max_diff {
            max_diff = diff;
            let pixel_index = index / 4;
            let channel = index % 4;
            worst.0 = pixel_index;
            worst.1[channel] = *a;
            worst.2[channel] = *b;
        }
    }
    assert!(
        max_diff <= 4,
        "恒等管线最大偏差 {max_diff} (>4)，像素 #{} 源 {:?} → 输出 {:?}",
        worst.0,
        worst.1,
        worst.2
    );
}

/// 线性 f32 上传（16bit / RAW 通路）应与 8bit sRGB 上传渲染结果一致。
#[test]
fn gpu_linear_upload_matches_srgb_upload() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (96u32, 64u32);
    let source = test_image(width, height);

    let image8 = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();
    pipeline.render(
        &context.device,
        &context.queue,
        &image8,
        &EditParams::default(),
    );
    let out8 = pipeline
        .readback(&context.device, &context.queue, &image8)
        .unwrap();

    let linear: Vec<f32> = source
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| {
            [
                srgb_to_linear(pixel[0]),
                srgb_to_linear(pixel[1]),
                srgb_to_linear(pixel[2]),
                pixel[3] as f32 / 255.0,
            ]
        })
        .collect();
    let image_linear = pipeline
        .upload_linear_f32(&context.device, &context.queue, &linear, width, height)
        .unwrap();
    pipeline.render(
        &context.device,
        &context.queue,
        &image_linear,
        &EditParams::default(),
    );
    let out_linear = pipeline
        .readback(&context.device, &context.queue, &image_linear)
        .unwrap();

    let mut max_diff = 0i32;
    for (left, right) in out8.iter().zip(out_linear.iter()) {
        max_diff = max_diff.max((*left as i32 - *right as i32).abs());
    }
    assert!(
        max_diff <= 2,
        "线性 f32 上传与 8bit sRGB 上传结果偏差过大：{max_diff}"
    );
}

#[test]
fn gpu_exposure_doubles_linear_light() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (64u32, 64u32);
    let source = test_image(width, height);
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    let mut params = EditParams::default();
    params.light.exposure = 1.0;
    pipeline.render(&context.device, &context.queue, &image, &params);
    let output = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();

    let sample = (width / 2, height - 1); // 渐变中段灰阶（避免高光截断）
    let before = pixel(&source, width, sample.0, sample.1);
    let after = pixel(&output, width, sample.0, sample.1);
    let ratio = srgb_to_linear(after[0]) / srgb_to_linear(before[0]).max(1e-4);
    assert!(
        (ratio - 2.0).abs() < 0.25,
        "曝光 +1EV 的线性倍率应 ≈2，实际 {ratio:.3}"
    );
}

#[test]
fn gpu_saturation_minus_hundred_is_gray() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (64u32, 64u32);
    let source = test_image(width, height);
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    let mut params = EditParams::default();
    params.color.saturation = -100.0;
    pipeline.render(&context.device, &context.queue, &image, &params);
    let output = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();

    let sample = pixel(&output, width, 4, 4); // 蓝块内部
    let spread = sample.iter().max().unwrap() - sample.iter().min().unwrap();
    assert!(spread <= 6, "饱和度 -100 后应为灰色，实际 {sample:?}");
}

#[test]
fn gpu_vignette_darkens_corners() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (64u32, 64u32);
    // 均匀灰底，排除内容差异
    let mut source = vec![0u8; (width * height * 4) as usize];
    for chunk in source.as_chunks_mut::<4>().0 {
        *chunk = [140, 140, 140, 255];
    }
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    let mut params = EditParams::default();
    params.effects.vignette = 100.0;
    pipeline.render(&context.device, &context.queue, &image, &params);
    let output = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();

    let center = pixel(&output, width, width / 2, height / 2)[0];
    let corner = pixel(&output, width, 0, 0)[0];
    assert!(
        corner + 20 < center,
        "暗角应压暗四角：中心 {center}，角落 {corner}"
    );
}

#[test]
fn gpu_curve_and_hsl_change_output() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (64u32, 64u32);
    let source = test_image(width, height);
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    let identity = {
        pipeline.render(
            &context.device,
            &context.queue,
            &image,
            &EditParams::default(),
        );
        pipeline
            .readback(&context.device, &context.queue, &image)
            .unwrap()
    };

    let mut params = EditParams::default();
    params.curves.rgb.points = vec![
        CurvePoint { x: 0.0, y: 0.0 },
        CurvePoint { x: 0.5, y: 0.7 },
        CurvePoint { x: 1.0, y: 1.0 },
    ];
    params.hsl.band_mut(ColorBand::Blue).saturation = -80.0;
    pipeline.render(&context.device, &context.queue, &image, &params);
    let output = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();

    let middle = (width / 2, height / 2);
    let before = pixel(&identity, width, middle.0, middle.1);
    let after = pixel(&output, width, middle.0, middle.1);
    assert!(
        after[0] > before[0] + 10,
        "S 曲线中点应提亮：{before:?} → {after:?}"
    );

    let blue_before = pixel(&identity, width, 4, 4);
    let blue_after = pixel(&output, width, 4, 4);
    let spread_before = blue_before.iter().max().unwrap() - blue_before.iter().min().unwrap();
    let spread_after = blue_after.iter().max().unwrap() - blue_after.iter().min().unwrap();
    assert!(
        spread_after < spread_before,
        "HSL 蓝色饱和度 -80 应降低蓝色块彩色度：{spread_before} → {spread_after}"
    );
}

#[test]
fn gpu_mask_blend_applies_locally() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (64u32, 64u32);
    let mut source = vec![0u8; (width * height * 4) as usize];
    for chunk in source.as_chunks_mut::<4>().0 {
        *chunk = [120, 120, 120, 255];
    }
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    // 全局恒等
    let mut params = EditParams::default();
    pipeline.render(&context.device, &context.queue, &image, &params);
    let global = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();

    // 蒙版：从左到右的线性渐变，局部 +2EV
    let mut mask = MaskParams {
        id: "m1".to_string(),
        name: "左侧".to_string(),
        kind: MaskKind::LinearGradient {
            start: [0.0, 0.5],
            end: [0.5, 0.5],
        },
        ..MaskParams::default()
    };
    mask.adjustments.light.exposure = 2.0;
    params.masks.push(mask);

    pipeline.render(&context.device, &context.queue, &image, &params);
    let masked = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();

    let left = pixel(&masked, width, 4, 32)[0] as i32;
    let right = pixel(&masked, width, width - 4, 32)[0] as i32;
    let global_left = pixel(&global, width, 4, 32)[0] as i32;
    let global_right = pixel(&global, width, width - 4, 32)[0] as i32;
    assert!(
        left > global_left + 40,
        "蒙版区域应变亮：{global_left} → {left}"
    );
    assert!(
        (right - global_right).abs() <= 4,
        "蒙版外应保持不变：{global_right} → {right}"
    );
}

/// GPU 基础调色段（白平衡 / 曝光 / 对比度）应与 CPU 参考实现一致。
#[test]
fn gpu_base_stage_matches_cpu_reference() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (16u32, 16u32);
    let mut source = vec![0u8; (width * height * 4) as usize];
    for chunk in source.as_chunks_mut::<4>().0 {
        *chunk = [140, 120, 100, 255];
    }
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    let mut params = EditParams::default();
    params.color.temperature = 40.0;
    params.color.tint = -25.0;
    params.light.exposure = 0.4;
    params.light.contrast = 30.0;
    pipeline.render(&context.device, &context.queue, &image, &params);
    let output = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();

    let expected = fdxmp_core::pipeline::apply_reference(
        [
            srgb_to_linear(140),
            srgb_to_linear(120),
            srgb_to_linear(100),
        ],
        &params,
    );
    let gpu_pixel = pixel(&output, width, 8, 8);
    for channel in 0..3 {
        let reference = fdxmp_core::gpu_pipeline::linear_to_srgb_u8(expected[channel]) as i32;
        let diff = gpu_pixel[channel] as i32 - reference;
        assert!(
            diff.abs() <= 2,
            "GPU 基础段与 CPU 参考不一致（通道 {channel}）：GPU {} vs CPU {reference}",
            gpu_pixel[channel]
        );
    }
}

/// 回归：对比度 -100 只压平画面，不得反相（旧实现系数 -0.5 会把黑白颠倒）。
#[test]
fn gpu_negative_contrast_does_not_invert() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (64u32, 32u32);
    let mut source = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let index = ((y * width + x) * 4) as usize;
            let value = if x < width / 2 { 0u8 } else { 255u8 };
            source[index] = value;
            source[index + 1] = value;
            source[index + 2] = value;
            source[index + 3] = 255;
        }
    }
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    let mut params = EditParams::default();
    params.light.contrast = -100.0;
    pipeline.render(&context.device, &context.queue, &image, &params);
    let output = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();

    let black = pixel(&output, width, 4, 16)[0] as i32;
    let white = pixel(&output, width, width - 4, 16)[0] as i32;
    assert!(
        black < white,
        "对比度 -100 不得反相：黑 {black} / 白 {white}"
    );
    assert!(black > 0, "负对比度应向中灰压平黑场：{black}");
    assert!(white < 255, "负对比度应向中灰压平白场：{white}");
}

/// 回归：蒙版参数是相对全局的增量，零增量蒙版不得抹掉全局调色。
#[test]
fn gpu_empty_mask_keeps_global_edits() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (64u32, 64u32);
    let mut source = vec![0u8; (width * height * 4) as usize];
    for chunk in source.as_chunks_mut::<4>().0 {
        *chunk = [120, 120, 120, 255];
    }
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    // 全局 +1EV
    let mut params = EditParams::default();
    params.light.exposure = 1.0;
    pipeline.render(&context.device, &context.queue, &image, &params);
    let global = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();

    // 覆盖左半的渐变蒙版，但不设置任何局部参数（增量全 0）
    params.masks.push(MaskParams {
        id: "noop".to_string(),
        name: "空蒙版".to_string(),
        kind: MaskKind::LinearGradient {
            start: [0.0, 0.5],
            end: [0.5, 0.5],
        },
        ..MaskParams::default()
    });
    pipeline.render(&context.device, &context.queue, &image, &params);
    let masked = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();

    let global_left = pixel(&global, width, 4, 32)[0] as i32;
    let masked_left = pixel(&masked, width, 4, 32)[0] as i32;
    assert!(
        (masked_left - global_left).abs() <= 2,
        "零增量蒙版不应改变全局结果：{global_left} → {masked_left}"
    );
    assert!(
        masked_left > 140,
        "蒙版区域应保留全局 +1EV，而不是回到原图：{masked_left}"
    );
}

/// 回归：细节链路的模糊参考必须与曝光 / 白平衡同域。
///
/// 均匀灰面上没有高频细节：曝光 +1EV 后，纹理 / 清晰度 / 降噪（细节参数）
/// 都不应该改变画面亮度。旧实现用未调色的原图做模糊参考，
/// `c - blurred` 含巨大直流分量，会在均匀面上凭空提亮 / 压暗。
#[test]
fn gpu_detail_reference_shares_exposure() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (64u32, 64u32);
    let mut source = vec![0u8; (width * height * 4) as usize];
    for chunk in source.as_chunks_mut::<4>().0 {
        *chunk = [120, 120, 120, 255];
    }
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    fn render(
        pipeline: &mut ColorPipeline,
        context: &GpuContext,
        image: &fdxmp_core::gpu_pipeline::PipelineImage,
        params: &EditParams,
    ) -> Vec<u8> {
        pipeline.render(&context.device, &context.queue, image, params);
        pipeline
            .readback(&context.device, &context.queue, image)
            .unwrap()
    }

    let mut base = EditParams::default();
    base.light.exposure = 1.0;
    let plain = render(&mut pipeline, &context, &image, &base);

    for (name, params) in [
        ("纹理", {
            let mut p = base.clone();
            p.effects.texture = 100.0;
            p
        }),
        ("清晰度", {
            let mut p = base.clone();
            p.effects.clarity = 100.0;
            p
        }),
        ("亮度降噪", {
            let mut p = base.clone();
            p.detail.noise.luminance = 100.0;
            p
        }),
        ("锐化", {
            let mut p = base.clone();
            p.detail.sharpen.amount = 100.0;
            p
        }),
    ] {
        let out = render(&mut pipeline, &context, &image, &params);
        let mut max_diff = 0i32;
        for (left, right) in plain.iter().zip(out.iter()) {
            max_diff = max_diff.max((*left as i32 - *right as i32).abs());
        }
        assert!(
            max_diff <= 2,
            "均匀灰面上 {name} 不应改变亮度（最大差 {max_diff}）：模糊参考与曝光不同域"
        );
    }
}

/// 回归：HSL 色带分区必须完备——90°（黄绿）不能是"无响应区"。
#[test]
fn gpu_hsl_bands_cover_all_hues() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (32u32, 32u32);
    let source = hsv_test_image(width, height, 90.0, 1.0, 0.8);
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    let identity = {
        pipeline.render(
            &context.device,
            &context.queue,
            &image,
            &EditParams::default(),
        );
        pipeline
            .readback(&context.device, &context.queue, &image)
            .unwrap()
    };

    // 90° 正好是黄（60°）与绿（120°）的中点：两个色带都必须有响应，
    // 旧实现在这里是权重和 0，滑杆完全无效。
    for (name, band) in [("黄", ColorBand::Yellow), ("绿", ColorBand::Green)] {
        let mut params = EditParams::default();
        params.hsl.band_mut(band).saturation = -100.0;
        pipeline.render(&context.device, &context.queue, &image, &params);
        let output = pipeline
            .readback(&context.device, &context.queue, &image)
            .unwrap();
        let diff = max_channel_diff(&identity, &output);
        assert!(diff >= 4, "{name}色带在 90° 处应有响应（最大差 {diff}）");
    }

    // 权重和恒为 1：黄 -100 与绿 +100 在中点应基本抵消。
    let mut params = EditParams::default();
    params.hsl.yellow.saturation = -100.0;
    params.hsl.green.saturation = 100.0;
    pipeline.render(&context.device, &context.queue, &image, &params);
    let cancelled = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();
    let diff = max_channel_diff(&identity, &cancelled);
    assert!(
        diff <= 2,
        "相邻色带权重和应为 1（-100/+100 应抵消），实际最大差 {diff}"
    );
}

/// 回归：色调分区互不污染；白色 / 黑色是端点偏移（能抬黑 / 压白）。
#[test]
fn gpu_tone_zones_are_separated() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (96u32, 32u32);
    let mut source = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let value = if x < width / 3 {
                0u8
            } else if x < width * 2 / 3 {
                128u8
            } else {
                255u8
            };
            let index = ((y * width + x) * 4) as usize;
            source[index] = value;
            source[index + 1] = value;
            source[index + 2] = value;
            source[index + 3] = 255;
        }
    }
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    fn render(
        pipeline: &mut ColorPipeline,
        context: &GpuContext,
        image: &fdxmp_core::gpu_pipeline::PipelineImage,
        params: &EditParams,
    ) -> Vec<u8> {
        pipeline.render(&context.device, &context.queue, image, params);
        pipeline
            .readback(&context.device, &context.queue, image)
            .unwrap()
    }

    let plain = render(&mut pipeline, &context, &image, &EditParams::default());
    let black_x = 8;
    let mid_x = width / 2;
    let white_x = width - 8;
    let black = pixel(&plain, width, black_x, 16)[0];
    let mid = pixel(&plain, width, mid_x, 16)[0];
    let white = pixel(&plain, width, white_x, 16)[0];
    assert_eq!(
        (black, mid, white),
        (0, 128, 255),
        "测试图应为黑/中灰/白三段"
    );

    // 黑色 +100：抬起纯黑（乘性增益对 0 无效，旧实现纹丝不动）
    let mut params = EditParams::default();
    params.light.blacks = 100.0;
    let out = render(&mut pipeline, &context, &image, &params);
    let lifted = pixel(&out, width, black_x, 16)[0];
    assert!(
        lifted >= black + 8,
        "黑色 +100 应抬起纯黑：{black} → {lifted}"
    );
    let mid_after = pixel(&out, width, mid_x, 16)[0];
    assert!(
        (mid_after as i32 - mid as i32).abs() <= 3,
        "黑色滑块不应污染中灰：{mid} → {mid_after}"
    );

    // 白色 -100：压下纯白，且不碰中灰
    let mut params = EditParams::default();
    params.light.whites = -100.0;
    let out = render(&mut pipeline, &context, &image, &params);
    let pulled = pixel(&out, width, white_x, 16)[0];
    assert!(
        pulled + 8 <= white,
        "白色 -100 应压下纯白：{white} → {pulled}"
    );
    let mid_after = pixel(&out, width, mid_x, 16)[0];
    assert!(
        (mid_after as i32 - mid as i32).abs() <= 3,
        "白色滑块不应污染中灰：{mid} → {mid_after}"
    );

    // 阴影 -100：对纯黑无影响（证明黑场只由"黑色"控制）
    let mut params = EditParams::default();
    params.light.shadows = -100.0;
    let out = render(&mut pipeline, &context, &image, &params);
    let shadowed_black = pixel(&out, width, black_x, 16)[0];
    assert!(
        shadowed_black <= black + 2,
        "阴影 -100 不应改变纯黑：{black} → {shadowed_black}"
    );
}

/// 回归：白平衡是色适应矩阵——0/0 恒等、正温度变暖、负温度变冷。
#[test]
fn gpu_white_balance_warms_and_cools() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (32u32, 32u32);
    let mut source = vec![0u8; (width * height * 4) as usize];
    for chunk in source.as_chunks_mut::<4>().0 {
        *chunk = [128, 128, 128, 255];
    }
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    fn render(
        pipeline: &mut ColorPipeline,
        context: &GpuContext,
        image: &fdxmp_core::gpu_pipeline::PipelineImage,
        params: &EditParams,
    ) -> [u8; 3] {
        pipeline.render(&context.device, &context.queue, image, params);
        let output = pipeline
            .readback(&context.device, &context.queue, image)
            .unwrap();
        pixel(&output, 32, 16, 16)
    }

    let neutral = render(&mut pipeline, &context, &image, &EditParams::default());

    let mut warm_params = EditParams::default();
    warm_params.color.temperature = 100.0;
    let warm = render(&mut pipeline, &context, &image, &warm_params);
    assert!(
        warm[0] > warm[2] + 10,
        "温度 +100 应偏暖（R > B）：{neutral:?} → {warm:?}"
    );

    let mut cool_params = EditParams::default();
    cool_params.color.temperature = -100.0;
    let cool = render(&mut pipeline, &context, &image, &cool_params);
    assert!(
        cool[2] > cool[0] + 10,
        "温度 -100 应偏冷（B > R）：{neutral:?} → {cool:?}"
    );

    let mut magenta_params = EditParams::default();
    magenta_params.color.tint = 100.0;
    let magenta = render(&mut pipeline, &context, &image, &magenta_params);
    assert!(
        magenta[1] < neutral[1],
        "色调 +100 应压低绿色：{neutral:?} → {magenta:?}"
    );
}

/// 回归：颜色分级——上色保持亮度、只作用于对应分区、明亮度滑杆是本区增益。
#[test]
fn gpu_grading_preserves_luminance_and_zones() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (96u32, 32u32);
    let mut source = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let value = if x < width / 3 {
                40u8
            } else if x < width * 2 / 3 {
                128u8
            } else {
                220u8
            };
            let index = ((y * width + x) * 4) as usize;
            source[index] = value;
            source[index + 1] = value;
            source[index + 2] = value;
            source[index + 3] = 255;
        }
    }
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    fn render(
        pipeline: &mut ColorPipeline,
        context: &GpuContext,
        image: &fdxmp_core::gpu_pipeline::PipelineImage,
        params: &EditParams,
    ) -> Vec<u8> {
        pipeline.render(&context.device, &context.queue, image, params);
        pipeline
            .readback(&context.device, &context.queue, image)
            .unwrap()
    }

    fn luma(pixel: [u8; 3]) -> f32 {
        0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32
    }

    let plain = render(&mut pipeline, &context, &image, &EditParams::default());
    let dark_x = 8;
    let mid_x = width / 2;
    let bright_x = width - 8;
    let dark = pixel(&plain, width, dark_x, 16);
    let mid = pixel(&plain, width, mid_x, 16);
    let bright = pixel(&plain, width, bright_x, 16);

    // 阴影区上蓝（色相 210、饱和度 100）
    let mut params = EditParams::default();
    params.grading.shadows = fdxmp_core::params::GradingZone {
        hue: 210.0,
        saturation: 100.0,
        luminance: 0.0,
    };
    params.grading.blending = 50.0;
    let graded = render(&mut pipeline, &context, &image, &params);
    let dark_after = pixel(&graded, width, dark_x, 16);
    let mid_after = pixel(&graded, width, mid_x, 16);
    let bright_after = pixel(&graded, width, bright_x, 16);
    assert!(
        (dark_after[2] as i32 - dark_after[0] as i32) > (dark[2] as i32 - dark[0] as i32) + 10,
        "阴影应被染蓝：{dark:?} → {dark_after:?}"
    );
    assert!(
        (luma(dark_after) - luma(dark)).abs() <= 8.0,
        "上色应保持亮度：{} → {}",
        luma(dark),
        luma(dark_after)
    );
    assert!(
        max_channel_diff(&mid, &mid_after) <= 3 && max_channel_diff(&bright, &bright_after) <= 3,
        "分级不应污染其他分区：中灰 {mid:?} → {mid_after:?}，高光 {bright:?} → {bright_after:?}"
    );

    // 分区明亮度 +100：只提亮阴影区
    let mut params = EditParams::default();
    params.grading.shadows.luminance = 100.0;
    params.grading.blending = 50.0;
    let lifted = render(&mut pipeline, &context, &image, &params);
    let dark_lifted = pixel(&lifted, width, dark_x, 16);
    let bright_lifted = pixel(&lifted, width, bright_x, 16);
    assert!(
        dark_lifted[0] >= dark[0] + 4,
        "阴影明亮度 +100 应提亮暗部：{dark:?} → {dark_lifted:?}"
    );
    assert!(
        max_channel_diff(&bright, &bright_lifted) <= 3,
        "阴影明亮度不应影响高光：{bright:?} → {bright_lifted:?}"
    );
}

#[test]
fn gpu_preview_frame_time_smoke_test() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (2048u32, 1365u32); // 显示代理尺寸
    let source = test_image(width, height);
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    let mut params = EditParams::default();
    params.effects.clarity = 25.0;
    params.detail.noise.color = 20.0;

    // 预热
    for _ in 0..3 {
        pipeline.render(&context.device, &context.queue, &image, &params);
    }
    let _ = context.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });

    let iterations = 30;
    let started = Instant::now();
    for _ in 0..iterations {
        pipeline.render(&context.device, &context.queue, &image, &params);
    }
    let _ = context.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    let average_ms = started.elapsed().as_secs_f64() * 1000.0 / iterations as f64;
    eprintln!(
        "GPU 管线（2048×1365，含两趟模糊）：平均 {average_ms:.2} ms/帧 ≈ {:.0} fps",
        1000.0 / average_ms
    );
    assert!(
        average_ms < 33.0,
        "实时预览目标 ≥30fps，实际 {average_ms:.2} ms/帧"
    );

    // 带 2 个蒙版（线性 + 径向）的帧时间
    for (index, kind) in [
        MaskKind::LinearGradient {
            start: [0.1, 0.5],
            end: [0.6, 0.5],
        },
        MaskKind::RadialGradient {
            center: [0.7, 0.4],
            radii: [0.2, 0.25],
            rotation: 0.0,
        },
    ]
    .into_iter()
    .enumerate()
    {
        let mut mask = MaskParams {
            id: format!("m{index}"),
            name: format!("mask-{index}"),
            kind,
            ..MaskParams::default()
        };
        mask.adjustments.color.saturation = 25.0;
        params.masks.push(mask);
    }
    // 预热（首次会生成蒙版权重纹理）
    pipeline.render(&context.device, &context.queue, &image, &params);
    let started = Instant::now();
    for _ in 0..iterations {
        pipeline.render(&context.device, &context.queue, &image, &params);
    }
    let _ = context.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    let masked_ms = started.elapsed().as_secs_f64() * 1000.0 / iterations as f64;
    eprintln!(
        "GPU 管线（2048×1365，+2 个蒙版）：平均 {masked_ms:.2} ms/帧 ≈ {:.0} fps",
        1000.0 / masked_ms
    );
    assert!(
        masked_ms < 33.0,
        "带蒙版实时预览目标 ≥30fps，实际 {masked_ms:.2} ms/帧"
    );

    // 几何（旋转 5° + 裁剪 80%）帧时间：验证 cs_geometry 趟开销
    let mut geometry_params = EditParams::default();
    geometry_params.effects.clarity = 25.0;
    geometry_params.detail.noise.color = 20.0;
    geometry_params.crop.rotation_deg = 5.0;
    geometry_params.crop.rect = [0.1, 0.1, 0.8, 0.8];
    pipeline.render(&context.device, &context.queue, &image, &geometry_params);
    let started = Instant::now();
    for _ in 0..iterations {
        pipeline.render(&context.device, &context.queue, &image, &geometry_params);
    }
    let _ = context.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    let geometry_ms = started.elapsed().as_secs_f64() * 1000.0 / iterations as f64;
    eprintln!(
        "GPU 管线（2048×1365 旋转 5° + 裁剪 80%）：平均 {geometry_ms:.2} ms/帧 ≈ {:.0} fps",
        1000.0 / geometry_ms
    );
    assert!(
        geometry_ms < 33.0,
        "几何实时预览目标 ≥30fps，实际 {geometry_ms:.2} ms/帧"
    );

    // 细节全开（锐化半径/细节/蒙版 + 亮度/颜色降噪）帧时间
    let mut detail_params = EditParams::default();
    detail_params.detail.sharpen.amount = 80.0;
    detail_params.detail.sharpen.radius = 1.5;
    detail_params.detail.sharpen.detail = 60.0;
    detail_params.detail.sharpen.masking = 50.0;
    detail_params.detail.noise.luminance = 40.0;
    detail_params.detail.noise.luminance_detail = 50.0;
    detail_params.detail.noise.luminance_contrast = 30.0;
    detail_params.detail.noise.color = 30.0;
    detail_params.detail.noise.color_detail = 40.0;
    detail_params.detail.noise.color_smoothness = 40.0;
    pipeline.render(&context.device, &context.queue, &image, &detail_params);
    let started = Instant::now();
    for _ in 0..iterations {
        pipeline.render(&context.device, &context.queue, &image, &detail_params);
    }
    let _ = context.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    let detail_ms = started.elapsed().as_secs_f64() * 1000.0 / iterations as f64;
    eprintln!(
        "GPU 管线（2048×1365，细节全开）：平均 {detail_ms:.2} ms/帧 ≈ {:.0} fps",
        1000.0 / detail_ms
    );
    assert!(
        detail_ms < 12.0,
        "细节全开实时预览目标 ≤12ms，实际 {detail_ms:.2} ms/帧"
    );
}

#[test]
fn gpu_debug_gray_mapping() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let grays: [u8; 10] = [0, 16, 32, 64, 96, 128, 160, 192, 224, 255];
    let width = grays.len() as u32;
    let mut source = Vec::new();
    for gray in grays {
        source.extend_from_slice(&[gray, gray, gray, 255]);
    }
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, 1)
        .unwrap();
    pipeline.render(
        &context.device,
        &context.queue,
        &image,
        &EditParams::default(),
    );
    let output = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();
    let mapping: Vec<String> = grays
        .iter()
        .enumerate()
        .map(|(index, gray)| format!("{gray}->{}", output[index * 4]))
        .collect();
    eprintln!("恒等灰度映射：{}", mapping.join(" "));
}

/// 阶段 6：GPU 几何管线（裁剪 / 旋转 / 翻转）与 CPU 参考实现比对。
fn compare_rgba(a: &[u8], b: &[u8], tolerance: i32) -> i32 {
    assert_eq!(a.len(), b.len(), "对比的两张图尺寸不一致");
    a.iter()
        .zip(b.iter())
        .enumerate()
        .filter(|(index, _)| index % 4 != 3)
        .map(|(_, (left, right))| (*left as i32 - *right as i32).abs())
        .max()
        .unwrap_or(0)
        .max(0)
        .min(tolerance.max(0) + 1)
}

#[test]
fn gpu_crop_matches_cpu_reference() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (96u32, 64u32);
    let source = test_image(width, height);
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    let mut params = EditParams::default();
    params.crop.rect = [0.25, 0.25, 0.5, 0.5];
    pipeline.render(&context.device, &context.queue, &image, &params);
    assert_eq!(pipeline.target_size(), Some((48, 32)));
    let output = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();
    assert_eq!(output.len(), (48 * 32 * 4) as usize);

    let source_image = RawImage::from_raw(width, height, source.clone()).unwrap();
    let reference = fdxmp_core::geometry::render_geometry(
        &source_image,
        &params,
        fdxmp_core::geometry::GeometryMode::Crop,
    )
    .unwrap();
    let max_diff = compare_rgba(&output, reference.as_raw(), 8);
    assert!(
        max_diff <= 8,
        "GPU 裁剪与 CPU 参考最大偏差 {max_diff}（>8）"
    );
}

#[test]
fn gpu_rotation_matches_cpu_reference() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (80u32, 60u32);
    let source = test_image(width, height);
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    let mut params = EditParams::default();
    params.crop.rotation_deg = 12.5;
    pipeline.render(&context.device, &context.queue, &image, &params);
    let output = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();
    let source_image = RawImage::from_raw(width, height, source.clone()).unwrap();
    let reference = fdxmp_core::geometry::render_geometry(
        &source_image,
        &params,
        fdxmp_core::geometry::GeometryMode::Crop,
    )
    .unwrap();
    let max_diff = compare_rgba(&output, reference.as_raw(), 24);
    // f16 中间纹理 + sRGB 量化在高对比边缘会有个位数差异，平均值必须很小。
    let sample_count = ((output.len() / 4) * 3) as f64;
    let mean_diff: f64 = output
        .iter()
        .zip(reference.as_raw().iter())
        .enumerate()
        .filter(|(index, _)| index % 4 != 3)
        .map(|(_, (left, right))| (*left as f64 - *right as f64).abs())
        .sum::<f64>()
        / sample_count.max(1.0);
    assert!(
        max_diff <= 24,
        "GPU 旋转与 CPU 参考最大偏差 {max_diff}（>24）"
    );
    assert!(
        mean_diff < 2.0,
        "GPU 旋转与 CPU 参考平均偏差 {mean_diff:.3}（应 <2）"
    );
}

#[test]
fn gpu_flip_horizontal_mirrors() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (64u32, 32u32);
    let mut source = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let index = ((y * width + x) * 4) as usize;
            let value = if x < width / 4 { 240u8 } else { 20u8 };
            source[index] = value;
            source[index + 1] = value;
            source[index + 2] = value;
            source[index + 3] = 255;
        }
    }
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();
    let mut params = EditParams::default();
    params.crop.flip_horizontal = true;
    pipeline.render(&context.device, &context.queue, &image, &params);
    let output = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();
    // 左侧亮块翻转后应出现在右侧。
    assert!(pixel(&output, width, width - 2, 10)[0] > 200);
    assert!(pixel(&output, width, 1, 10)[0] < 60);
}

#[test]
fn gpu_crop_keeps_mask_aligned_to_source() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (64u32, 64u32);
    let mut source = vec![120u8; (width * height * 4) as usize];
    for chunk in source.as_chunks_mut::<4>().0 {
        chunk[3] = 255;
    }
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();
    let mut params = EditParams::default();
    params.crop.rect = [0.0, 0.0, 0.5, 1.0];
    let mut mask = MaskParams {
        id: "left".to_string(),
        name: "左侧提亮".to_string(),
        kind: MaskKind::LinearGradient {
            start: [0.0, 0.5],
            end: [0.5, 0.5],
        },
        ..MaskParams::default()
    };
    mask.adjustments.light.exposure = 1.0;
    params.masks.push(mask);
    pipeline.render(&context.device, &context.queue, &image, &params);
    assert_eq!(pipeline.target_size(), Some((32, 64)));
    let output = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();
    // 裁剪左半：左缘仍在被蒙版覆盖的源图左侧（更亮），右缘接近源图 x=0.5（较暗）。
    let left = pixel(&output, 32, 1, 32)[0] as i32;
    let right = pixel(&output, 32, 30, 32)[0] as i32;
    assert!(
        left - right >= 15,
        "蒙版在裁剪后应仍对齐源图内容：左 {left} 右 {right}"
    );
}

/// 阶段 6：细节从属参数必须真实参与计算（每个参数单独变化都要有可测差异）。
#[test]
fn gpu_detail_subparameters_change_output() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (160u32, 120u32);
    let source = test_image(width, height);
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    fn render_case(
        pipeline: &mut ColorPipeline,
        context: &GpuContext,
        image: &fdxmp_core::gpu_pipeline::PipelineImage,
        params: &EditParams,
    ) -> Vec<u8> {
        pipeline.render(&context.device, &context.queue, image, params);
        pipeline
            .readback(&context.device, &context.queue, image)
            .unwrap()
    }

    fn mean_rgb_diff(a: &[u8], b: &[u8]) -> f32 {
        let mut sum = 0f64;
        let mut count = 0usize;
        for (index, (left, right)) in a.iter().zip(b.iter()).enumerate() {
            if index % 4 == 3 {
                continue;
            }
            sum += (*left as f64 - *right as f64).abs();
            count += 1;
        }
        (sum / count.max(1) as f64) as f32
    }

    // 第四项：是否使用带色度噪声的输入（颜色降噪用例需要真实色噪）。
    let mut cases: Vec<(&str, EditParams, EditParams, bool)> = Vec::new();
    let make = |sharpen: f32, luma: f32, color: f32| {
        let mut params = EditParams::default();
        params.detail.sharpen.amount = sharpen;
        params.detail.noise.luminance = luma;
        params.detail.noise.color = color;
        params
    };
    let mut low = make(60.0, 0.0, 0.0);
    let mut high = low.clone();
    low.detail.sharpen.radius = 0.5;
    high.detail.sharpen.radius = 3.0;
    cases.push(("锐化半径", low, high, false));
    let mut low = make(60.0, 0.0, 0.0);
    let mut high = low.clone();
    low.detail.sharpen.detail = 0.0;
    high.detail.sharpen.detail = 100.0;
    cases.push(("锐化细节", low, high, false));
    let mut low = make(60.0, 0.0, 0.0);
    let mut high = low.clone();
    low.detail.sharpen.masking = 0.0;
    high.detail.sharpen.masking = 80.0;
    cases.push(("锐化蒙版", low, high, false));
    let mut low = make(0.0, 80.0, 0.0);
    let mut high = low.clone();
    low.detail.noise.luminance_detail = 0.0;
    high.detail.noise.luminance_detail = 100.0;
    cases.push(("亮度降噪细节", low, high, false));
    let mut low = make(0.0, 80.0, 0.0);
    let mut high = low.clone();
    low.detail.noise.luminance_contrast = 0.0;
    high.detail.noise.luminance_contrast = 100.0;
    cases.push(("亮度降噪对比度", low, high, false));
    let mut low = make(0.0, 0.0, 80.0);
    let mut high = low.clone();
    low.detail.noise.color_detail = 0.0;
    high.detail.noise.color_detail = 100.0;
    cases.push(("颜色降噪细节", low, high, true));
    let mut low = make(0.0, 0.0, 80.0);
    let mut high = low.clone();
    low.detail.noise.color_smoothness = 0.0;
    high.detail.noise.color_smoothness = 100.0;
    cases.push(("颜色降噪平滑度", low, high, true));

    let noisy_source = test_image_with_color_noise(width, height);
    let noisy_image = pipeline
        .upload(
            &context.device,
            &context.queue,
            &noisy_source,
            width,
            height,
        )
        .unwrap();

    for (name, low, high, use_noisy) in cases {
        let target = if use_noisy { &noisy_image } else { &image };
        let out_low = render_case(&mut pipeline, &context, target, &low);
        let out_high = render_case(&mut pipeline, &context, target, &high);
        let diff = mean_rgb_diff(&out_low, &out_high);
        assert!(diff > 0.2, "{name} 未参与计算（平均差异 {diff:.3}）");
    }
}

/// 阶段 6：GPU 蒙版叠加层在裁剪后仍与画面内容对齐。
#[test]
fn gpu_overlay_follows_crop() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (64u32, 64u32);
    let mut source = vec![0u8; (width * height * 4) as usize];
    for chunk in source.as_chunks_mut::<4>().0 {
        chunk[0] = 128;
        chunk[1] = 128;
        chunk[2] = 128;
        chunk[3] = 255;
    }
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    let mut params = EditParams::default();
    params.crop.rect = [0.5, 0.0, 0.5, 1.0]; // 只保留源图右半
    let mut mask = MaskParams {
        id: "gradient".to_string(),
        name: "渐变".to_string(),
        kind: MaskKind::LinearGradient {
            start: [0.0, 0.5],
            end: [1.0, 0.5],
        },
        ..MaskParams::default()
    };
    mask.adjustments.light.exposure = 0.5;
    params.masks.push(mask);

    pipeline.render(&context.device, &context.queue, &image, &params);
    let before = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();
    pipeline.render_overlay(
        &context.device,
        &context.queue,
        &image,
        &params,
        0,
        fdxmp_core::gpu_pipeline::OverlayStyle {
            color: [1.0, 0.0, 0.0],
            alpha: 0.5,
        },
    );
    let after = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();
    assert_eq!(pipeline.target_size(), Some((32, 64)));

    // 裁剪后左缘对应源图 x=0.5（蒙版权重高），右缘对应 x≈1（权重低）。
    let left = pixel(&after, 32, 1, 32);
    let right = pixel(&after, 32, 30, 32);
    assert!(
        left[0] > right[0] + 12,
        "叠加层未跟随裁剪：左 R {} 右 R {}",
        left[0],
        right[0]
    );
    assert!(
        left[2] + 12 < right[2],
        "叠加层颜色异常：左 B {} 右 B {}",
        left[2],
        right[2]
    );
    // 未叠加区域（右侧）应与渲染结果一致。
    let before_right = pixel(&before, 32, 30, 32);
    assert!((before_right[0] as i32 - right[0] as i32).abs() <= 2);
}

/// P3 前置：分块渲染（1:1 视图）与整图渲染在重叠区域逐像素一致。
#[test]
fn gpu_tile_matches_full_render_region() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (128u32, 96u32);
    let source = test_image(width, height);
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    let mut params = EditParams::default();
    params.crop.rotation_deg = 6.0;
    params.effects.clarity = 20.0;
    params.effects.vignette = 40.0;
    params.effects.grain = 30.0;
    params.masks.push(MaskParams {
        id: "tile-mask".to_string(),
        name: "分块蒙版".to_string(),
        kind: MaskKind::LinearGradient {
            start: [0.0, 0.0],
            end: [1.0, 1.0],
        },
        feather: 20.0,
        ..MaskParams::default()
    });
    params.masks[0].adjustments.light.exposure = 0.5;
    pipeline.render(&context.device, &context.queue, &image, &params);
    let full = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();

    // 分块：整图输出的 (40, 30) 起点，56×48 大小；源图按 App 的方式裁剪 + 边距，
    // 并把分块原点 / 整图尺寸传给管线（暗角、颗粒按整图坐标计算）。
    let geometry = fdxmp_core::geometry::Geometry::from_params(&params);
    let full_matrix = fdxmp_core::geometry::output_to_source_matrix(
        &geometry,
        params.crop.rect,
        width,
        height,
        width,
        height,
    );
    let (tile_x, tile_y, tile_w, tile_h) = (40i64, 30i64, 56i64, 48i64);
    let mut min = (f32::MAX, f32::MAX);
    let mut max = (f32::MIN, f32::MIN);
    for corner in [
        [tile_x as f32, tile_y as f32],
        [(tile_x + tile_w) as f32, tile_y as f32],
        [tile_x as f32, (tile_y + tile_h) as f32],
        [(tile_x + tile_w) as f32, (tile_y + tile_h) as f32],
    ] {
        let mapped = fdxmp_core::geometry::apply(&full_matrix, corner);
        min = (min.0.min(mapped[0]), min.1.min(mapped[1]));
        max = (max.0.max(mapped[0]), max.1.max(mapped[1]));
    }
    // 与 App 一致：覆盖细节模糊抽头范围（半径 × 缩放上限 + 几何采样）。
    let margin =
        fdxmp_core::gpu_pipeline::BLUR_RADIUS * fdxmp_core::gpu_pipeline::BLUR_SCALE_MAX + 4.0;
    let sx0 = (min.0 - margin).max(0.0) as u32;
    let sy0 = (min.1 - margin).max(0.0) as u32;
    let sx1 = ((max.0 + margin).min(width as f32)) as u32;
    let sy1 = ((max.1 + margin).min(height as f32)) as u32;

    let source_image = image::RgbaImage::from_raw(width, height, source.clone()).unwrap();
    let sub = image::imageops::crop_imm(&source_image, sx0, sy0, sx1 - sx0, sy1 - sy0).to_image();
    let mut tile_pipeline = ColorPipeline::new(&context.device);
    let mut tile_image = tile_pipeline
        .upload(
            &context.device,
            &context.queue,
            sub.as_raw(),
            sub.width(),
            sub.height(),
        )
        .unwrap();
    // 与 App 的 1:1 视图一致：蒙版按整图代理生成，源坐标是裁剪图坐标，
    // 通过 origin 加回整图（scale = 1 时即为整图分辨率）。
    tile_image.set_mask_reference(
        std::sync::Arc::new(source_image.clone()),
        [1.0, 1.0],
        [sx0 as f32, sy0 as f32],
    );
    let tile = fdxmp_core::geometry::tile_matrix(
        &full_matrix,
        [tile_x as f32, tile_y as f32],
        [sx0 as f32, sy0 as f32],
    );
    tile_pipeline.render_tile(
        &context.device,
        &context.queue,
        &tile_image,
        &params,
        &fdxmp_core::gpu_pipeline::RenderPlan {
            matrix: tile,
            width: 56,
            height: 48,
            geometry_active: true,
            tile_origin: [tile_x as f32, tile_y as f32],
            full_size: [width as f32, height as f32],
        },
    );
    assert_eq!(tile_pipeline.target_size(), Some((56, 48)));
    let tiled = tile_pipeline
        .readback(&context.device, &context.queue, &tile_image)
        .unwrap();

    let mut max_diff = 0i32;
    for y in 0..48u32 {
        for x in 0..56u32 {
            let full_pixel = pixel(&full, width, x + tile_x as u32, y + tile_y as u32);
            let tile_pixel = pixel(&tiled, 56, x, y);
            for channel in 0..3 {
                max_diff =
                    max_diff.max((full_pixel[channel] as i32 - tile_pixel[channel] as i32).abs());
            }
        }
    }
    assert!(max_diff <= 6, "分块与整图渲染最大偏差 {max_diff}（>6）");
}

/// 颗粒（大小 / 粗糙度）与暗角（中点 / 羽化）子参数必须真实参与计算。
#[test]
fn gpu_grain_and_vignette_subparams_change_output() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (96u32, 72u32);
    let mut source = vec![0u8; (width * height * 4) as usize];
    for chunk in source.as_chunks_mut::<4>().0 {
        *chunk = [128, 128, 128, 255];
    }
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    let make = |grain: f32, size: f32, roughness: f32, vignette: f32, mid: f32, feather: f32| {
        let mut params = EditParams::default();
        params.effects.grain = grain;
        params.effects.grain_size = size;
        params.effects.grain_roughness = roughness;
        params.effects.vignette = vignette;
        params.effects.vignette_midpoint = mid;
        params.effects.vignette_feather = feather;
        params
    };
    let render = |pipeline: &mut ColorPipeline, params: &EditParams| -> Vec<u8> {
        pipeline.render(&context.device, &context.queue, &image, params);
        pipeline
            .readback(&context.device, &context.queue, &image)
            .unwrap()
    };

    for (name, low, high) in [
        (
            "颗粒大小",
            make(80.0, 0.0, 50.0, 0.0, 50.0, 50.0),
            make(80.0, 100.0, 50.0, 0.0, 50.0, 50.0),
        ),
        (
            "颗粒粗糙度",
            make(80.0, 0.0, 0.0, 0.0, 50.0, 50.0),
            make(80.0, 0.0, 100.0, 0.0, 50.0, 50.0),
        ),
        (
            "暗角中点",
            make(0.0, 0.0, 50.0, 60.0, 10.0, 50.0),
            make(0.0, 0.0, 50.0, 60.0, 90.0, 50.0),
        ),
        (
            "暗角羽化",
            make(0.0, 0.0, 50.0, 60.0, 50.0, 0.0),
            make(0.0, 0.0, 50.0, 60.0, 50.0, 100.0),
        ),
    ] {
        let low_out = render(&mut pipeline, &low);
        let high_out = render(&mut pipeline, &high);
        let diff = max_channel_diff(&low_out, &high_out);
        assert!(diff >= 2, "{name} 未参与计算（最大差 {diff}）");
    }

    // 中点越小 → 压暗范围越向中心扩展（取 3/4 宽度处：半径 ≈ 0.4）
    let wide_vignette = render(&mut pipeline, &make(0.0, 0.0, 50.0, 60.0, 10.0, 50.0));
    let narrow_vignette = render(&mut pipeline, &make(0.0, 0.0, 50.0, 60.0, 90.0, 50.0));
    let probe = (width as f32 * 0.75) as u32;
    let wide = pixel(&wide_vignette, width, probe, height / 2)[0] as i32;
    let narrow = pixel(&narrow_vignette, width, probe, height / 2)[0] as i32;
    assert!(
        wide < narrow,
        "暗角中点越小，压暗范围应越大：中点 10 → {wide}，中点 90 → {narrow}"
    );
}

/// 细节半径按整图分辨率放大：半径 > 6 时走宽抽头模糊（1:1 / 全分辨率导出路径）。
#[test]
fn gpu_detail_radius_scales_with_full_size() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (64u32, 64u32);
    let source = test_image(width, height);
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    let mut params = EditParams::default();
    params.effects.clarity = 100.0;

    let plan = |full_size: [f32; 2]| fdxmp_core::gpu_pipeline::RenderPlan {
        matrix: fdxmp_core::geometry::IDENTITY,
        width,
        height,
        geometry_active: false,
        tile_origin: [0.0, 0.0],
        full_size,
    };
    // 代理尺度：半径 4 → 窄抽头（13）
    pipeline.render_tile(
        &context.device,
        &context.queue,
        &image,
        &params,
        &plan([0.0, 0.0]),
    );
    let narrow = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();
    // 1:1（整图 8192）：半径 16 → 宽抽头（49）
    pipeline.render_tile(
        &context.device,
        &context.queue,
        &image,
        &params,
        &plan([8192.0, 5461.0]),
    );
    let wide = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();

    let diff = max_channel_diff(&narrow, &wide);
    assert!(
        diff >= 3,
        "细节半径应随整图分辨率放大（宽 / 窄抽头结果应有差异）：最大差 {diff}"
    );
}

/// P5：镜头校正（畸变 / 色差）在 GPU 几何趟生效。
#[test]
fn gpu_lens_distortion_and_ca_change_output() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (128u32, 96u32);
    let source = test_image(width, height);
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();

    pipeline.render(
        &context.device,
        &context.queue,
        &image,
        &EditParams::default(),
    );
    let base = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();

    let mut distorted = EditParams::default();
    distorted.lens.geometry.distortion = 80.0;
    pipeline.render(&context.device, &context.queue, &image, &distorted);
    let warped = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();
    let max_diff = compare_rgba(&base, &warped, 255);
    assert!(max_diff > 15, "畸变校正未生效（最大差 {max_diff}）");

    // 灰度渐变图：默认 R=G=B，色差校正后红蓝采样位置不同。
    let mut gray = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let index = ((y * width + x) * 4) as usize;
            let value = (x * 255 / width.max(1)) as u8;
            gray[index] = value;
            gray[index + 1] = value;
            gray[index + 2] = value;
            gray[index + 3] = 255;
        }
    }
    let gray_image = pipeline
        .upload(&context.device, &context.queue, &gray, width, height)
        .unwrap();
    let mut chromatic = EditParams::default();
    chromatic.lens.chromatic_aberration = 100.0;
    pipeline.render(&context.device, &context.queue, &gray_image, &chromatic);
    let corrected = pipeline
        .readback(&context.device, &context.queue, &gray_image)
        .unwrap();
    let mut gap = 0i32;
    for chunk in corrected.as_chunks::<4>().0 {
        gap = gap.max((chunk[0] as i32 - chunk[2] as i32).abs());
    }
    assert!(gap > 5, "色差校正未生效（红蓝通道最大差 {gap}）");
}

/// P5：镜头暗角校正提亮四角（GPU）。
#[test]
fn gpu_lens_vignette_brightens_corners() {
    let Some(context) = context() else {
        return;
    };
    let mut pipeline = ColorPipeline::new(&context.device);
    let (width, height) = (96u32, 72u32);
    let mut source = vec![0u8; (width * height * 4) as usize];
    for chunk in source.as_chunks_mut::<4>().0 {
        chunk[0] = 120;
        chunk[1] = 120;
        chunk[2] = 120;
        chunk[3] = 255;
    }
    let image = pipeline
        .upload(&context.device, &context.queue, &source, width, height)
        .unwrap();
    let mut params = EditParams::default();
    params.lens.vignette = 100.0;
    pipeline.render(&context.device, &context.queue, &image, &params);
    let corrected = pipeline
        .readback(&context.device, &context.queue, &image)
        .unwrap();
    let corner = pixel(&corrected, width, 1, 1)[0] as i32;
    let center = pixel(&corrected, width, width / 2, height / 2)[0] as i32;
    assert!(
        corner > center + 8,
        "暗角校正未提亮四角：角 {corner} 中心 {center}"
    );
}
