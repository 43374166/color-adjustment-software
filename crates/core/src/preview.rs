//! CPU 预览渲染：把编辑参数应用到 sRGB RGBA 图像。
//!
//! 移动端 / 无 GPU 环境用这条路径做实时预览；wgpu 着色器管线是最终形态。
//! 白平衡、曝光、对比度直接复用 [`crate::pipeline::apply_reference`]（与 WGSL
//! 同公式）；分区色调（高光 / 阴影 / 白色 / 黑色）、饱和度 / 自然饱和度、
//! 暗角与颗粒为 CPU 近似实现，纹理 / 清晰度 / 去雾暂未纳入。

use std::path::Path;

use image::RgbaImage;

use crate::error::Result;
use crate::params::EditParams;

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
    Ok(PreviewRgba {
        width: rendered.width(),
        height: rendered.height(),
        rgba: rendered.into_raw(),
    })
}

/// 把编辑参数应用到 sRGB RGBA 图像。
pub fn render_preview_image(source: &RgbaImage, params: &EditParams) -> RgbaImage {
    let width = source.width().max(1) as f32;
    let height = source.height().max(1) as f32;
    let light = &params.light;
    let color = &params.color;
    let effects = &params.effects;
    let mut out = source.clone();

    for (x, y, pixel) in out.enumerate_pixels_mut() {
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

        // 暗角（负值提亮四角）。
        let vignette = effects.vignette / 100.0;
        if vignette != 0.0 {
            let nx = x as f32 / (width - 1.0).max(1.0) - 0.5;
            let ny = y as f32 / (height - 1.0).max(1.0) - 0.5;
            let distance = (nx * nx + ny * ny).sqrt() / std::f32::consts::FRAC_1_SQRT_2;
            let weight = smoothstep(0.4, 1.0, distance);
            let factor = (1.0 - vignette * 0.8 * weight).clamp(0.0, 2.0);
            for channel in &mut rgb {
                *channel *= factor;
            }
        }

        // 颗粒。
        let grain = effects.grain / 100.0;
        if grain != 0.0 {
            let noise = grain_noise(x, y, effects.grain_size);
            for channel in &mut rgb {
                *channel += noise * grain * 0.06;
            }
        }

        for channel in &mut rgb {
            *channel = channel.clamp(0.0, 1.0);
        }
        pixel.0 = [
            to_u8(rgb[0]),
            to_u8(rgb[1]),
            to_u8(rgb[2]),
            a,
        ];
    }

    out
}

fn luminance(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
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

fn grain_noise(x: u32, y: u32, size: f32) -> f32 {
    let step = size.max(1.0) as u32;
    let qx = (x / step) as u64;
    let qy = (y / step) as u64;
    let mut hash = qx.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ qy.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    hash ^= hash >> 33;
    (hash as f32 / u64::MAX as f32) * 2.0 - 1.0
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
}
