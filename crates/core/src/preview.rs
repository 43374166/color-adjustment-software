//! CPU 预览渲染：把编辑参数应用到 sRGB RGBA 图像。
//!
//! 移动端 / 无 GPU 环境用这条路径做实时预览；wgpu 着色器管线是最终形态。
//! 白平衡、曝光、对比度直接复用 [`crate::pipeline::apply_reference`]（与 WGSL
//! 同公式）；分区色调（高光 / 阴影 / 白色 / 黑色）、饱和度 / 自然饱和度、
//! 暗角与颗粒为 CPU 近似实现，纹理 / 清晰度 / 去雾暂未纳入。

use std::path::Path;

use image::RgbaImage;

use crate::error::Result;
use crate::params::{
    eval_curve, ColorGradingParams, CropParams, CurveParams, DetailParams, EditParams, HslParams,
    LensParams, ParametricCurve, ToneCurve,
};

/// CPU 预览渲染结果：尺寸 + RGBA8 像素（行优先）。
pub struct PreviewRgba {
    /// 图像宽度。
    pub width: u32,
    /// 图像高度。
    pub height: u32,
    /// RGBA8 像素数据，长度 = width × height × 4。
    pub rgba: Vec<u8>,
}

/// 解码图像（优先内嵌预览 / 半尺寸）并渲染到最长边 `max_edge` 的预览。
pub fn render_preview_rgba(path: &Path, params_json: &str, max_edge: u32) -> Result<PreviewRgba> {
    let params: EditParams = serde_json::from_str(params_json)?;
    let decoded = crate::decode::decode_preview(path)?;
    let proxy = decoded.display_proxy(max_edge.clamp(64, 4096));
    let rendered = render_preview_image(&proxy, &params);
    let corrected = apply_lens(&rendered, &params.lens);
    let mut geometry = apply_geometry(&corrected, &params.crop);
    apply_masks(&mut geometry, &params);
    Ok(PreviewRgba {
        width: geometry.width(),
        height: geometry.height(),
        rgba: geometry.into_raw(),
    })
}

/// 镜头校正 / 几何：畸变、透视、旋转、缩放 + 镜头暗角校正（双线性采样）。
pub fn apply_lens(source: &RgbaImage, lens: &LensParams) -> RgbaImage {
    let geometry = &lens.geometry;
    let distortion = (geometry.distortion / 100.0).clamp(-1.0, 1.0);
    let vertical = (geometry.vertical / 100.0).clamp(-1.0, 1.0);
    let horizontal = (geometry.horizontal / 100.0).clamp(-1.0, 1.0);
    let rotate = geometry.rotate.clamp(-45.0, 45.0);
    let scale = if geometry.scale <= 0.0 {
        1.0
    } else {
        geometry.scale.clamp(0.5, 2.0)
    };
    let profile = if lens.profile_correction { -0.06 } else { 0.0 };
    let k = distortion * 0.25 + profile;
    let vignette = (lens.vignette / 100.0).clamp(-1.0, 1.0);
    let identity = k.abs() < 1e-4
        && vertical.abs() < 1e-4
        && horizontal.abs() < 1e-4
        && rotate.abs() < 0.01
        && (scale - 1.0).abs() < 1e-4
        && vignette.abs() < 1e-4;
    if identity {
        return source.clone();
    }

    let (width, height) = source.dimensions();
    if width == 0 || height == 0 {
        return source.clone();
    }
    let half_w = (width as f32 - 1.0) * 0.5;
    let half_h = (height as f32 - 1.0) * 0.5;
    let max_half = half_w.max(half_h).max(1.0);
    let radians = rotate.to_radians();
    let (sin, cos) = radians.sin_cos();
    let mut out = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let mut nx = (x as f32 - half_w) / max_half;
            let mut ny = (y as f32 - half_h) / max_half;
            nx /= scale;
            ny /= scale;
            nx -= vertical * ny * 0.5;
            ny -= horizontal * nx * 0.5;
            let rx = nx * cos + ny * sin;
            let ry = -nx * sin + ny * cos;
            let radius_squared = rx * rx + ry * ry;
            let factor = 1.0 + k * radius_squared;
            let sx = half_w + rx * factor * max_half;
            let sy = half_h + ry * factor * max_half;
            let mut pixel = sample_bilinear(source, sx, sy);
            if vignette.abs() > 1e-4 {
                let distance =
                    radius_squared.sqrt() / std::f32::consts::SQRT_2;
                let weight = smoothstep(0.35, 1.0, distance);
                let correction = 1.0 + vignette * 0.45 * weight;
                for channel in 0..3 {
                    pixel.0[channel] = ((pixel.0[channel] as f32 * correction)
                        .clamp(0.0, 255.0)) as u8;
                }
            }
            out.put_pixel(x, y, pixel);
        }
    }
    out
}

/// 几何处理：翻转 → 旋转（自动放大避免黑角）→ 按归一化矩形裁剪。
pub fn apply_geometry(source: &RgbaImage, crop: &CropParams) -> RgbaImage {
    let mut image = source.clone();
    if crop.flip_horizontal {
        image = flip_horizontal(&image);
    }
    if crop.flip_vertical {
        image = flip_vertical(&image);
    }
    if crop.rotation_deg.abs() > 0.01 {
        image = rotate(&image, crop.rotation_deg);
    }
    crop_rect(&image, crop.rect)
}

fn flip_horizontal(source: &RgbaImage) -> RgbaImage {
    let (width, height) = (source.width(), source.height());
    let mut out = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            out.put_pixel(width - 1 - x, y, *source.get_pixel(x, y));
        }
    }
    out
}

fn flip_vertical(source: &RgbaImage) -> RgbaImage {
    let (width, height) = (source.width(), source.height());
    let mut out = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            out.put_pixel(x, height - 1 - y, *source.get_pixel(x, y));
        }
    }
    out
}

/// 旋转并自动放大到覆盖整帧（直线校正后不出现黑角）。
fn rotate(source: &RgbaImage, degrees: f32) -> RgbaImage {
    let width = source.width();
    let height = source.height();
    if width == 0 || height == 0 {
        return source.clone();
    }
    let radians = degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    let w = width as f32;
    let h = height as f32;
    // 旋转后包围盒 / 原尺寸的较大者，作为放大系数保证画面铺满。
    let zoom = ((w * cos.abs() + h * sin.abs()) / w)
        .max((w * sin.abs() + h * cos.abs()) / h)
        .max(1.0);
    let center_x = (width as f32 - 1.0) * 0.5;
    let center_y = (height as f32 - 1.0) * 0.5;
    let mut out = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let dx = (x as f32 - center_x) / zoom;
            let dy = (y as f32 - center_y) / zoom;
            let sx = center_x + dx * cos + dy * sin;
            let sy = center_y - dx * sin + dy * cos;
            out.put_pixel(x, y, sample_bilinear(source, sx, sy));
        }
    }
    out
}

fn sample_bilinear(source: &RgbaImage, x: f32, y: f32) -> image::Rgba<u8> {
    let max_x = source.width().saturating_sub(1) as f32;
    let max_y = source.height().saturating_sub(1) as f32;
    let x = x.clamp(0.0, max_x);
    let y = y.clamp(0.0, max_y);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(source.width().saturating_sub(1));
    let y1 = (y0 + 1).min(source.height().saturating_sub(1));
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;

    let mut channels = [0u8; 4];
    for channel in 0..4 {
        let p00 = source.get_pixel(x0, y0).0[channel] as f32;
        let p10 = source.get_pixel(x1, y0).0[channel] as f32;
        let p01 = source.get_pixel(x0, y1).0[channel] as f32;
        let p11 = source.get_pixel(x1, y1).0[channel] as f32;
        let top = p00 + (p10 - p00) * fx;
        let bottom = p01 + (p11 - p01) * fx;
        channels[channel] = (top + (bottom - top) * fy).round().clamp(0.0, 255.0) as u8;
    }
    image::Rgba(channels)
}

fn crop_rect(source: &RgbaImage, rect: [f32; 4]) -> RgbaImage {
    let width = source.width();
    let height = source.height();
    if width == 0 || height == 0 {
        return source.clone();
    }
    let x = rect[0].clamp(0.0, 0.99);
    let y = rect[1].clamp(0.0, 0.99);
    let w = rect[2].clamp(0.01, 1.0 - x.max(0.0));
    let h = rect[3].clamp(0.01, 1.0 - y.max(0.0));
    let left = (x * width as f32).round() as u32;
    let top = (y * height as f32).round() as u32;
    let crop_width = ((w * width as f32).round() as u32).clamp(1, width - left.min(width - 1));
    let crop_height = ((h * height as f32).round() as u32).clamp(1, height - top.min(height - 1));
    if left == 0 && top == 0 && crop_width == width && crop_height == height {
        return source.clone();
    }
    image::imageops::crop_imm(source, left, top, crop_width, crop_height).to_image()
}

/// 把编辑参数应用到 sRGB RGBA 图像。
pub fn render_preview_image(source: &RgbaImage, params: &EditParams) -> RgbaImage {
    let width = source.width().max(1) as f32;
    let height = source.height().max(1) as f32;
    let effects = &params.effects;
    let curves = CurveLuts::new(&params.curves);
    let mut out = source.clone();

    // 第一遍：逐像素颜色（基础 → 分区 → 曲线 → 饱和度 → HSL → 分级）。
    for pixel in out.pixels_mut() {
        let [r, g, b, a] = pixel.0;
        let linear = [
            srgb_to_linear(r as f32 / 255.0),
            srgb_to_linear(g as f32 / 255.0),
            srgb_to_linear(b as f32 / 255.0),
        ];
        let base = crate::pipeline::apply_reference(linear, params);
        let mut rgb = [
            linear_to_srgb(base[0]),
            linear_to_srgb(base[1]),
            linear_to_srgb(base[2]),
        ];
        rgb = adjust_pixel(rgb, params, &curves);
        pixel.0 = [to_u8(rgb[0]), to_u8(rgb[1]), to_u8(rgb[2]), a];
    }

    // 第二遍：细节（锐化 / 降噪）需要邻域。
    apply_detail(&mut out, &params.detail);

    // 第三遍：全局效果（暗角 / 颗粒）。
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let [r, g, b, a] = pixel.0;
        let mut rgb = [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0];
        rgb = apply_effects(rgb, effects, x, y, width, height);
        for channel in &mut rgb {
            *channel = channel.clamp(0.0, 1.0);
        }
        pixel.0 = [to_u8(rgb[0]), to_u8(rgb[1]), to_u8(rgb[2]), a];
    }

    out
}

/// 按蒙版对图像做局部调整（在全局调色之后、几何裁剪之前）。
pub fn apply_masks(image: &mut RgbaImage, params: &EditParams) {
    if params.masks.is_empty() {
        return;
    }
    let contexts: Vec<MaskContext> = params
        .masks
        .iter()
        .filter_map(|mask| {
            let raster =
                crate::masks::generate_mask(mask, image, image.width(), image.height()).ok()?;
            Some(MaskContext {
                raster,
                curves: CurveLuts::new(&mask.adjustments.curves),
                params: (*mask.adjustments).clone(),
            })
        })
        .collect();
    if contexts.is_empty() {
        return;
    }

    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let [r, g, b, a] = pixel.0;
        let mut rgb = [
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
        ];
        let mut changed = false;
        for context in &contexts {
            let weight = context.raster.get(x, y);
            if weight > 0.001 {
                let adjusted = adjust_with_reference(rgb, &context.params, &context.curves);
                for channel in 0..3 {
                    rgb[channel] += (adjusted[channel] - rgb[channel]) * weight;
                }
                changed = true;
            }
        }
        if changed {
            pixel.0 = [to_u8(rgb[0]), to_u8(rgb[1]), to_u8(rgb[2]), a];
        }
    }
}

/// 蒙版上下文：权重栅格 + 该蒙版的曲线 LUT 与调色参数。
struct MaskContext {
    raster: crate::masks::MaskRaster,
    curves: CurveLuts,
    params: EditParams,
}

/// 单像素调色：分区色调 → 曲线 → 饱和度 / 自然饱和度（局部调整复用）。
fn adjust_with_reference(rgb: [f32; 3], params: &EditParams, curves: &CurveLuts) -> [f32; 3] {
    // 局部调整同样包含白平衡 / 曝光 / 对比度（参考实现）。
    let linear = [
        srgb_to_linear(rgb[0]),
        srgb_to_linear(rgb[1]),
        srgb_to_linear(rgb[2]),
    ];
    let base = crate::pipeline::apply_reference(linear, params);
    adjust_pixel(
        [
            linear_to_srgb(base[0]),
            linear_to_srgb(base[1]),
            linear_to_srgb(base[2]),
        ],
        params,
        curves,
    )
}

fn adjust_pixel(mut rgb: [f32; 3], params: &EditParams, curves: &CurveLuts) -> [f32; 3] {
    let light = &params.light;
    let color = &params.color;

    // 分区色调：用平滑权重整体加减，保持通道比例。
    let luma = luminance(rgb);
    let highlights = light.highlights / 100.0;
    if highlights != 0.0 {
        let weight = smoothstep(0.5, 1.0, luma);
        for channel in &mut rgb {
            *channel += highlights * weight * 0.35;
        }
    }
    let shadows = light.shadows / 100.0;
    if shadows != 0.0 {
        let weight = 1.0 - smoothstep(0.0, 0.5, luma);
        for channel in &mut rgb {
            *channel += shadows * weight * 0.35;
        }
    }
    let whites = light.whites / 100.0;
    if whites != 0.0 {
        let weight = luma * luma;
        for channel in &mut rgb {
            *channel += whites * weight * 0.30;
        }
    }
    let blacks = light.blacks / 100.0;
    if blacks != 0.0 {
        let weight = (1.0 - luma) * (1.0 - luma);
        for channel in &mut rgb {
            *channel += blacks * weight * 0.30;
        }
    }
    for channel in &mut rgb {
        *channel = channel.clamp(0.0, 1.0);
    }

    // 曲线：参数曲线 → RGB → 单通道。
    rgb = curves.apply(rgb);

    // 饱和度 / 自然饱和度。
    let luminance_now = luminance(rgb);
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    let min = rgb[0].min(rgb[1]).min(rgb[2]);
    let pixel_saturation = if max <= f32::EPSILON {
        0.0
    } else {
        ((max - min) / max).clamp(0.0, 1.0)
    };
    let saturation_factor = 1.0 + color.saturation / 100.0;
    let vibrance_factor = 1.0 + (color.vibrance / 100.0) * (1.0 - pixel_saturation);
    let factor = saturation_factor * vibrance_factor;
    for channel in &mut rgb {
        *channel = luminance_now + (*channel - luminance_now) * factor;
    }

    // HSL 八色带。
    rgb = apply_hsl(rgb, &params.hsl);
    // 颜色分级（阴影 / 中间调 / 高光）。
    apply_grading(rgb, &params.grading)
}

/// 全局效果：暗角 + 颗粒。
fn apply_effects(
    mut rgb: [f32; 3],
    effects: &crate::params::EffectParams,
    x: u32,
    y: u32,
    width: f32,
    height: f32,
) -> [f32; 3] {
    // 暗角（负值提亮四角）。
    let vignette = effects.vignette / 100.0;
    if vignette != 0.0 {
        let nx = x as f32 / (width - 1.0).max(1.0) - 0.5;
        let ny = y as f32 / (height - 1.0).max(1.0) - 0.5;
        let distance = (nx * nx + ny * ny).sqrt() / std::f32::consts::FRAC_1_SQRT_2;
        let midpoint = (effects.vignette_midpoint / 100.0).clamp(0.0, 1.0);
        let feather = (effects.vignette_feather / 100.0).clamp(0.0, 1.0);
        let start = (midpoint * 0.85).clamp(0.0, 0.95);
        let transition = (0.12 + feather * 0.55).max(0.05);
        let weight = smoothstep(start, (start + transition).min(1.0), distance);
        let factor = (1.0 - vignette * 0.9 * weight).clamp(0.0, 2.0);
        for channel in &mut rgb {
            *channel *= factor;
        }
    }

    // 颗粒。
    let grain = effects.grain / 100.0;
    if grain != 0.0 {
        let noise = grain_noise(
            x,
            y,
            effects.grain_size,
            (effects.grain_roughness / 100.0).clamp(0.0, 1.0),
        );
        for channel in &mut rgb {
            *channel += noise * grain * 0.06;
        }
    }
    rgb
}

fn luminance(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}

/// HSL 八色带中心（色相圈 0..1）。
const HSL_BAND_CENTERS: [f32; 8] = [
    0.0,
    30.0 / 360.0,
    60.0 / 360.0,
    120.0 / 360.0,
    180.0 / 360.0,
    240.0 / 360.0,
    280.0 / 360.0,
    320.0 / 360.0,
];

/// HSL：按色相把 8 个色带的调整平滑混合后应用。
fn apply_hsl(rgb: [f32; 3], hsl: &HslParams) -> [f32; 3] {
    let bands = [
        &hsl.red,
        &hsl.orange,
        &hsl.yellow,
        &hsl.green,
        &hsl.aqua,
        &hsl.blue,
        &hsl.purple,
        &hsl.magenta,
    ];
    if bands
        .iter()
        .all(|band| band.hue == 0.0 && band.saturation == 0.0 && band.luminance == 0.0)
    {
        return rgb;
    }
    let (hue, saturation, value) = rgb_to_hsv(rgb);
    if saturation <= 0.02 {
        return rgb;
    }

    const RADIUS: f32 = 0.10;
    let mut hue_shift = 0.0f32;
    let mut saturation_shift = 0.0f32;
    let mut luminance_shift = 0.0f32;
    let mut weight_total = 0.0f32;
    for (index, center) in HSL_BAND_CENTERS.iter().enumerate() {
        let mut distance = (hue - center).abs();
        if distance > 0.5 {
            distance = 1.0 - distance;
        }
        let weight = (1.0 - distance / RADIUS).max(0.0);
        if weight > 0.0 {
            let band = bands[index];
            hue_shift += band.hue / 100.0 * weight;
            saturation_shift += band.saturation / 100.0 * weight;
            luminance_shift += band.luminance / 100.0 * weight;
            weight_total += weight;
        }
    }
    if weight_total <= 0.0 {
        return rgb;
    }
    hue_shift /= weight_total;
    saturation_shift /= weight_total;
    luminance_shift /= weight_total;

    let visible = smoothstep(0.02, 0.22, saturation);
    let new_hue = (hue + hue_shift * visible * 0.08).rem_euclid(1.0);
    let new_saturation = (saturation * (1.0 + saturation_shift * visible)).clamp(0.0, 1.0);
    let new_value = (value + luminance_shift * visible * 0.22).clamp(0.0, 1.0);
    hsv_to_rgb(new_hue, new_saturation, new_value)
}

/// 颜色分级：阴影 / 中间调 / 高光三个区域的色相、饱和度、明亮度。
fn apply_grading(rgb: [f32; 3], grading: &ColorGradingParams) -> [f32; 3] {
    let zones = [&grading.shadows, &grading.midtones, &grading.highlights];
    if zones
        .iter()
        .all(|zone| zone.saturation <= 0.01 && zone.luminance == 0.0)
    {
        return rgb;
    }
    let luma = luminance(rgb);
    let balanced = (luma + grading.balance / 100.0 * 0.15).clamp(0.0, 1.0);
    let blend = (grading.blending / 100.0).clamp(0.0, 1.0);
    let exponent = 1.0 + blend * 1.5;
    let weights = [
        (1.0 - smoothstep(0.0, 0.55, balanced)).powf(exponent),
        (1.0 - (balanced - 0.5).abs() * 2.0).max(0.0).powf(exponent),
        smoothstep(0.45, 1.0, balanced).powf(exponent),
    ];

    let mut out = rgb;
    for (zone, weight) in zones.iter().zip(weights) {
        if weight <= 0.001 {
            continue;
        }
        let strength = (zone.saturation / 100.0).clamp(0.0, 1.0) * weight;
        if strength > 0.0 {
            let tint = hsv_to_rgb((zone.hue / 360.0).rem_euclid(1.0), 1.0, 1.0);
            for channel in 0..3 {
                out[channel] += (tint[channel] - out[channel]) * strength * 0.5;
            }
        }
        if zone.luminance != 0.0 {
            let factor = 1.0 + (zone.luminance / 100.0) * weight * 0.25;
            for channel in &mut out {
                *channel *= factor;
            }
        }
    }
    out
}

/// 细节：USM 锐化 + 亮度 / 颜色降噪（颜色管线之后、效果之前）。
pub fn apply_detail(image: &mut RgbaImage, detail: &DetailParams) {
    let sharpen = &detail.sharpen;
    let noise = &detail.noise;
    let need_sharpen = sharpen.amount > 0.01;
    let need_luminance_nr = noise.luminance > 0.01;
    let need_color_nr = noise.color > 0.01;
    if !need_sharpen && !need_luminance_nr && !need_color_nr {
        return;
    }

    let radius = if need_sharpen {
        (sharpen.radius.max(0.5).round() as u32).clamp(1, 4)
    } else {
        2
    };
    let blurred = blur_rgb(image, radius);
    let (width, height) = image.dimensions();

    for y in 0..height {
        for x in 0..width {
            let index = (y as usize) * width as usize + x as usize;
            let source = image.get_pixel(x, y).0;
            let mut rgb = [
                source[0] as f32 / 255.0,
                source[1] as f32 / 255.0,
                source[2] as f32 / 255.0,
            ];
            let blur = blurred[index];

            if need_luminance_nr {
                let amount = (noise.luminance / 100.0).clamp(0.0, 1.0) * 0.85;
                let detail_factor = (0.4 + noise.luminance_detail / 100.0 * 0.6).clamp(0.2, 1.0);
                let weight = amount * detail_factor;
                for channel in 0..3 {
                    rgb[channel] += (blur[channel] - rgb[channel]) * weight;
                }
            }
            if need_color_nr {
                let amount = (noise.color / 100.0).clamp(0.0, 1.0) * 0.85;
                let detail_factor = (0.4 + noise.color_detail / 100.0 * 0.6).clamp(0.2, 1.0);
                let weight = amount * detail_factor;
                let luminance_original = luminance(rgb);
                let luminance_blurred = luminance(blur);
                for channel in 0..3 {
                    let chroma_original = rgb[channel] - luminance_original;
                    let chroma_blurred = blur[channel] - luminance_blurred;
                    rgb[channel] = luminance_original
                        + chroma_original
                        + (chroma_blurred - chroma_original) * weight;
                }
            }
            if need_sharpen {
                let amount = (sharpen.amount / 100.0).clamp(0.0, 1.5);
                let detail_factor = (0.5 + sharpen.detail / 100.0).clamp(0.5, 1.5);
                let masking = (sharpen.masking / 100.0).clamp(0.0, 1.0);
                let diff = [
                    rgb[0] - blur[0],
                    rgb[1] - blur[1],
                    rgb[2] - blur[2],
                ];
                let edge = (diff[0].abs() + diff[1].abs() + diff[2].abs()) / 3.0;
                let mask_weight = if masking <= 0.001 {
                    1.0
                } else {
                    let threshold = 0.004 + masking * 0.05;
                    smoothstep(0.0, threshold, edge)
                };
                let scale = amount * detail_factor * mask_weight;
                for channel in 0..3 {
                    rgb[channel] += diff[channel] * scale;
                }
            }

            for channel in &mut rgb {
                *channel = channel.clamp(0.0, 1.0);
            }
            image.put_pixel(
                x,
                y,
                image::Rgba([to_u8(rgb[0]), to_u8(rgb[1]), to_u8(rgb[2]), source[3]]),
            );
        }
    }
}

/// 可分离盒式模糊（两次横向 + 纵向，近似高斯）。
fn blur_rgb(image: &RgbaImage, radius: u32) -> Vec<[f32; 3]> {
    let (width, height) = image.dimensions();
    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let source: Vec<[f32; 3]> = image
        .pixels()
        .map(|pixel| {
            [
                pixel.0[0] as f32 / 255.0,
                pixel.0[1] as f32 / 255.0,
                pixel.0[2] as f32 / 255.0,
            ]
        })
        .collect();
    let radius = radius.max(1) as isize;
    let mut horizontal = vec![[0f32; 3]; source.len()];
    let mut out = vec![[0f32; 3]; source.len()];

    for y in 0..h {
        for x in 0..w {
            let mut sum = [0f32; 3];
            let mut count = 0f32;
            for offset in -radius..=radius {
                let sx = (x as isize + offset).clamp(0, w as isize - 1) as usize;
                let sample = source[y * w + sx];
                for channel in 0..3 {
                    sum[channel] += sample[channel];
                }
                count += 1.0;
            }
            horizontal[y * w + x] = [sum[0] / count, sum[1] / count, sum[2] / count];
        }
    }
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0f32; 3];
            let mut count = 0f32;
            for offset in -radius..=radius {
                let sy = (y as isize + offset).clamp(0, h as isize - 1) as usize;
                let sample = horizontal[sy * w + x];
                for channel in 0..3 {
                    sum[channel] += sample[channel];
                }
                count += 1.0;
            }
            out[y * w + x] = [sum[0] / count, sum[1] / count, sum[2] / count];
        }
    }
    out
}

fn rgb_to_hsv(rgb: [f32; 3]) -> (f32, f32, f32) {
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    let min = rgb[0].min(rgb[1]).min(rgb[2]);
    let delta = max - min;
    let hue = if delta <= f32::EPSILON {
        0.0
    } else if max == rgb[0] {
        ((rgb[1] - rgb[2]) / delta).rem_euclid(6.0) / 6.0
    } else if max == rgb[1] {
        (((rgb[2] - rgb[0]) / delta) + 2.0) / 6.0
    } else {
        (((rgb[0] - rgb[1]) / delta) + 4.0) / 6.0
    };
    let saturation = if max <= f32::EPSILON { 0.0 } else { delta / max };
    (hue.rem_euclid(1.0), saturation, max)
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> [f32; 3] {
    let h = hue.rem_euclid(1.0) * 6.0;
    let index = h.floor();
    let fraction = h - index;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * fraction);
    let t = value * (1.0 - saturation * (1.0 - fraction));
    match index as i32 % 6 {
        0 => [value, t, p],
        1 => [q, value, p],
        2 => [p, value, t],
        3 => [p, q, value],
        4 => [t, p, value],
        _ => [value, p, q],
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> f32 {
    let x = value.clamp(0.0, 1.0);
    if x <= 0.003_130_8 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

fn to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn grain_noise(x: u32, y: u32, size: f32, roughness: f32) -> f32 {
    let step = (1.0 + (size.clamp(0.0, 100.0) / 100.0) * 7.0).round() as u32;
    let coarse = hash_noise(x / step.max(1), y / step.max(1));
    let fine = hash_noise(x, y);
    coarse * (1.0 - roughness) + fine * roughness
}

fn hash_noise(x: u32, y: u32) -> f32 {
    let qx = x as u64;
    let qy = y as u64;
    let mut hash = qx.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ qy.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    hash ^= hash >> 33;
    (hash as f32 / u64::MAX as f32) * 2.0 - 1.0
}

/// 预设好的曲线 LUT：参数曲线 / RGB / 单通道，恒等映射时为 None。
struct CurveLuts {
    parametric: Option<[f32; 257]>,
    rgb: Option<[f32; 257]>,
    red: Option<[f32; 257]>,
    green: Option<[f32; 257]>,
    blue: Option<[f32; 257]>,
}

impl CurveLuts {
    fn new(curves: &CurveParams) -> Self {
        Self {
            parametric: build_parametric_lut(&curves.parametric),
            rgb: build_curve_lut(&curves.rgb),
            red: build_curve_lut(&curves.red),
            green: build_curve_lut(&curves.green),
            blue: build_curve_lut(&curves.blue),
        }
    }

    fn apply(&self, input: [f32; 3]) -> [f32; 3] {
        let mut rgb = input;
        if let Some(lut) = &self.parametric {
            for channel in &mut rgb {
                *channel = sample_lut(lut, *channel);
            }
        }
        if let Some(lut) = &self.rgb {
            for channel in &mut rgb {
                *channel = sample_lut(lut, *channel);
            }
        }
        if let Some(lut) = &self.red {
            rgb[0] = sample_lut(lut, rgb[0]);
        }
        if let Some(lut) = &self.green {
            rgb[1] = sample_lut(lut, rgb[1]);
        }
        if let Some(lut) = &self.blue {
            rgb[2] = sample_lut(lut, rgb[2]);
        }
        rgb
    }
}

fn build_curve_lut(curve: &ToneCurve) -> Option<[f32; 257]> {
    if curve.is_identity() {
        return None;
    }
    let mut lut = [0f32; 257];
    for (index, slot) in lut.iter_mut().enumerate() {
        *slot = eval_curve(&curve.points, index as f32 / 256.0);
    }
    Some(lut)
}

fn build_parametric_lut(curve: &ParametricCurve) -> Option<[f32; 257]> {
    if curve.highlights == 0.0 && curve.midtones == 0.0 && curve.shadows == 0.0 {
        return None;
    }
    let highlights_split = if curve.highlights_split <= 0.0 {
        0.75
    } else {
        curve.highlights_split.clamp(0.5, 1.0)
    };
    let midtones_split = if curve.midtones_split <= 0.0 {
        0.25
    } else {
        curve.midtones_split.clamp(0.0, 0.5)
    };
    let highlights_split = highlights_split.max(midtones_split + 0.1);

    let mut lut = [0f32; 257];
    for (index, slot) in lut.iter_mut().enumerate() {
        let x = index as f32 / 256.0;
        let weight_highlights = smoothstep(midtones_split, highlights_split, x);
        let weight_shadows = 1.0 - weight_highlights;
        let weight_midtones = (1.0 - weight_highlights - weight_shadows).max(0.0);
        let delta = (curve.highlights / 100.0) * weight_highlights * 0.35
            + (curve.midtones / 100.0) * weight_midtones * 0.35
            + (curve.shadows / 100.0) * weight_shadows * 0.35;
        *slot = (x + delta).clamp(0.0, 1.0);
    }
    Some(lut)
}

fn sample_lut(lut: &[f32; 257], value: f32) -> f32 {
    let position = value.clamp(0.0, 1.0) * 256.0;
    let index = (position.floor() as usize).min(256);
    let next = (index + 1).min(256);
    let fraction = position - index as f32;
    lut[index] * (1.0 - fraction) + lut[next] * fraction
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn default_params_are_identity() {
        let mut source = RgbaImage::new(4, 4);
        for (x, y, pixel) in source.enumerate_pixels_mut() {
            *pixel = Rgba([(x * 60) as u8, (y * 60) as u8, 128, 255]);
        }
        let rendered = render_preview_image(&source, &EditParams::default());
        for (before, after) in source.pixels().zip(rendered.pixels()) {
            for channel in 0..3 {
                let delta = (before.0[channel] as i32 - after.0[channel] as i32).abs();
                assert!(delta <= 1, "默认参数应接近恒等，偏差 {delta}");
            }
        }
    }

    #[test]
    fn exposure_brightens_image() {
        let mut source = RgbaImage::new(2, 2);
        for pixel in source.pixels_mut() {
            *pixel = Rgba([64, 64, 64, 255]);
        }
        let mut params = EditParams::default();
        params.light.exposure = 1.0;
        let rendered = render_preview_image(&source, &params);
        // sRGB 伽马：线性翻倍对应 64 → 约 90。
        assert!(rendered.get_pixel(0, 0).0[0] >= 80);
    }

    #[test]
    fn rgb_curve_darkens_midtones() {
        let mut source = RgbaImage::new(2, 2);
        for pixel in source.pixels_mut() {
            *pixel = Rgba([128, 128, 128, 255]);
        }
        let mut params = EditParams::default();
        params.curves.rgb.points = vec![
            crate::params::CurvePoint { x: 0.0, y: 0.0 },
            crate::params::CurvePoint { x: 0.5, y: 0.25 },
            crate::params::CurvePoint { x: 1.0, y: 1.0 },
        ];
        let rendered = render_preview_image(&source, &params);
        assert!(rendered.get_pixel(0, 0).0[0] < 110);
    }

    #[test]
    fn crop_rect_halves_image() {
        let source = RgbaImage::from_pixel(8, 8, Rgba([10, 20, 30, 255]));
        let mut crop = CropParams::default();
        crop.rect = [0.0, 0.0, 0.5, 0.5];
        let out = apply_geometry(&source, &crop);
        assert_eq!(out.width(), 4);
        assert_eq!(out.height(), 4);
    }

    #[test]
    fn rotation_keeps_size() {
        let source = RgbaImage::from_pixel(8, 8, Rgba([10, 20, 30, 255]));
        let mut crop = CropParams::default();
        crop.rotation_deg = 10.0;
        let out = apply_geometry(&source, &crop);
        assert_eq!(out.dimensions(), (8, 8));
    }

    #[test]
    fn flip_horizontal_mirrors_pixels() {
        let mut source = RgbaImage::new(2, 1);
        source.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        source.put_pixel(1, 0, Rgba([0, 255, 0, 255]));
        let mut crop = CropParams::default();
        crop.flip_horizontal = true;
        let out = apply_geometry(&source, &crop);
        assert_eq!(out.get_pixel(0, 0).0, [0, 255, 0, 255]);
    }

    #[test]
    fn linear_mask_brightens_masked_side() {
        let source = RgbaImage::from_pixel(4, 4, Rgba([64, 64, 64, 255]));
        let mut params = EditParams::default();
        let mut mask = crate::params::MaskParams::default();
        mask.kind = crate::params::MaskKind::LinearGradient {
            start: [0.0, 0.0],
            end: [0.0, 1.0],
        };
        let mut local = EditParams::default();
        local.light.exposure = 1.0;
        mask.adjustments = Box::new(local);
        params.masks = vec![mask];

        let mut out = render_preview_image(&source, &params);
        apply_masks(&mut out, &params);
        let top = out.get_pixel(0, 0).0[0];
        let bottom = out.get_pixel(0, 3).0[0];
        assert!(top > bottom + 10, "top={top} bottom={bottom}");
    }

    #[test]
    fn hsl_red_band_desaturates_red() {
        let source = RgbaImage::from_pixel(2, 2, Rgba([200, 40, 40, 255]));
        let mut params = EditParams::default();
        params.hsl.red.saturation = -100.0;
        let out = render_preview_image(&source, &params);
        let pixel = out.get_pixel(0, 0).0;
        assert!(
            (pixel[1] as i32 - pixel[0] as i32).abs() < 40,
            "红色饱和度降低后应更接近灰色：{pixel:?}"
        );
    }

    #[test]
    fn grading_tints_shadows_blue() {
        let source = RgbaImage::from_pixel(2, 2, Rgba([30, 30, 30, 255]));
        let mut params = EditParams::default();
        params.grading.shadows.hue = 240.0;
        params.grading.shadows.saturation = 100.0;
        let out = render_preview_image(&source, &params);
        let pixel = out.get_pixel(0, 0).0;
        assert!(pixel[2] > pixel[0], "阴影应偏蓝：{pixel:?}");
    }

    #[test]
    fn sharpen_increases_edge_contrast() {
        let mut source = RgbaImage::new(4, 1);
        for x in 0..4 {
            source.put_pixel(
                x,
                0,
                if x < 2 {
                    Rgba([80, 80, 80, 255])
                } else {
                    Rgba([140, 140, 140, 255])
                },
            );
        }
        let mut params = EditParams::default();
        params.detail.sharpen.amount = 100.0;
        params.detail.sharpen.radius = 1.0;
        let out = render_preview_image(&source, &params);
        let left = out.get_pixel(1, 0).0[0] as i32;
        let right = out.get_pixel(2, 0).0[0] as i32;
        assert!(right - left > 60, "锐化后边缘对比应增大：left={left} right={right}");
    }

    #[test]
    fn lens_scale_changes_corner_pixels() {
        let mut source = RgbaImage::new(4, 4);
        for (x, y, pixel) in source.enumerate_pixels_mut() {
            *pixel = Rgba([(x * 60) as u8, (y * 60) as u8, 0, 255]);
        }
        let mut lens = LensParams::default();
        lens.geometry.scale = 1.5;
        let out = apply_lens(&source, &lens);
        assert_ne!(out.get_pixel(0, 0).0, source.get_pixel(0, 0).0);
    }
}
