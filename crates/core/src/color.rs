//! ICC 色彩管理：sRGB → Display P3 / Adobe RGB / ProPhoto RGB 转换与配置文件输出。
//!
//! 依赖 Little CMS（`lcms2`，静态编译）。转换结果供导出时写回
//! JPEG APP2 / PNG iCCP，实现规格要求的色彩空间导出。

use lcms2::{CIExyY, CIExyYTRIPLE, Flags, Intent, PixelFormat, Profile, ToneCurve, Transform};

use crate::error::{FdxmpError, Result};
use crate::export::ColorSpace;

/// 色彩空间转换结果：目标空间像素与对应 ICC 配置文件。
pub struct ColorConversion {
    /// 转换后的 RGBA8 像素（目标色彩空间编码）。
    pub pixels: image::RgbaImage,
    /// 目标空间 ICC 配置文件字节。
    pub icc: Vec<u8>,
}

fn d65() -> CIExyY {
    CIExyY {
        x: 0.3127,
        y: 0.3290,
        Y: 1.0,
    }
}

fn d50() -> CIExyY {
    CIExyY {
        x: 0.3457,
        y: 0.3585,
        Y: 1.0,
    }
}

fn primaries(red: (f64, f64), green: (f64, f64), blue: (f64, f64)) -> CIExyYTRIPLE {
    let point = |(x, y): (f64, f64)| CIExyY { x, y, Y: 1.0 };
    CIExyYTRIPLE {
        Red: point(red),
        Green: point(green),
        Blue: point(blue),
    }
}

/// sRGB 分段传递函数（IEC 61966-2.1）。
///
/// LittleCMS 参数曲线类型 4：`Y = (aX + b)^g`（`X ≥ d`）/ `Y = cX`（`X < d`）。
/// Display P3 与 sRGB 使用同一传递函数；纯 gamma 2.2 近似会让中间调产生可见偏差。
fn srgb_tone_curve() -> Result<ToneCurve> {
    const CURVE_TYPE_SRGB: i16 = 4;
    ToneCurve::new_parametric(
        CURVE_TYPE_SRGB,
        &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045],
    )
    .map_err(|err| FdxmpError::Unsupported(format!("sRGB 传递函数构建失败：{err}")))
}

/// 目标色彩空间的 ICC 配置文件。
pub fn target_profile(space: ColorSpace) -> Result<Profile> {
    let map = |err| FdxmpError::Unsupported(format!("ICC 配置文件创建失败：{err}"));
    match space {
        ColorSpace::Srgb => Ok(Profile::new_srgb()),
        ColorSpace::DisplayP3 => {
            // Display P3：D65 白点，P3 原色，sRGB 分段传递函数。
            let curve = srgb_tone_curve()?;
            let p = primaries((0.680, 0.320), (0.265, 0.690), (0.150, 0.060));
            Profile::new_rgb(&d65(), &p, &[&curve, &curve, &curve]).map_err(map)
        }
        ColorSpace::AdobeRgb => {
            // Adobe RGB (1998)：D65，gamma 563/256 ≈ 2.19921875。
            let curve = ToneCurve::new(563.0 / 256.0);
            let p = primaries((0.640, 0.330), (0.210, 0.710), (0.150, 0.060));
            Profile::new_rgb(&d65(), &p, &[&curve, &curve, &curve]).map_err(map)
        }
        ColorSpace::ProPhotoRgb => {
            // ProPhoto RGB：D50，gamma 1.8，原色范围超出可见光。
            let curve = ToneCurve::new(1.8);
            let p = primaries(
                (0.734_699, 0.265_301),
                (0.159_597, 0.840_404),
                (0.036_599, 0.000_098),
            );
            Profile::new_rgb(&d50(), &p, &[&curve, &curve, &curve]).map_err(map)
        }
    }
}

/// 把 sRGB 编码的 RGBA8 图像转换到目标色彩空间，并返回可写回的 ICC 配置文件。
pub fn convert_from_srgb(rgba: &image::RgbaImage, space: ColorSpace) -> Result<ColorConversion> {
    let source = Profile::new_srgb();
    let target = target_profile(space)?;
    // 照片转换用相对比色 + 黑点补偿（矩阵型配置文件下 Perceptual 会退化为
    // 相对比色且不补偿黑点）。
    let transform = Transform::new_flags(
        &source,
        PixelFormat::RGBA_8,
        &target,
        PixelFormat::RGBA_8,
        Intent::RelativeColorimetric,
        Flags::BLACKPOINT_COMPENSATION,
    )
    .map_err(|err| FdxmpError::Unsupported(format!("ICC 转换创建失败：{err}")))?;
    let mut destination = vec![0u8; rgba.as_raw().len()];
    transform.transform_pixels(rgba.as_raw(), &mut destination);
    let pixels = image::RgbaImage::from_raw(rgba.width(), rgba.height(), destination)
        .ok_or_else(|| FdxmpError::Unsupported("ICC 转换输出尺寸不匹配".to_string()))?;
    let icc = target
        .icc()
        .map_err(|err| FdxmpError::Unsupported(format!("ICC 配置文件导出失败：{err}")))?;
    Ok(ColorConversion { pixels, icc })
}

/// 输入侧色彩管理：把带嵌入 ICC 的 8bit RGBA 图像转换到 sRGB。
///
/// 用于 JPEG / PNG / WebP / 8bit TIFF 等容器里携带的非 sRGB 配置文件；
/// RAW 相机 ICC/DCP 仍待后续任务。
pub fn convert_icc_to_srgb(icc: &[u8], rgba: &image::RgbaImage) -> Result<image::RgbaImage> {
    let map = |err| FdxmpError::Unsupported(format!("输入 ICC 转换失败：{err}"));
    let source = Profile::new_icc(icc).map_err(map)?;
    let target = Profile::new_srgb();
    let transform = Transform::new_flags(
        &source,
        PixelFormat::RGBA_8,
        &target,
        PixelFormat::RGBA_8,
        Intent::RelativeColorimetric,
        Flags::BLACKPOINT_COMPENSATION,
    )
    .map_err(map)?;
    let mut destination = vec![0u8; rgba.as_raw().len()];
    transform.transform_pixels(rgba.as_raw(), &mut destination);
    image::RgbaImage::from_raw(rgba.width(), rgba.height(), destination)
        .ok_or_else(|| FdxmpError::Unsupported("输入 ICC 转换输出尺寸不匹配".to_string()))
}

/// 输入侧色彩管理：16bit RGBA 版本（16bit TIFF / PNG / RAW 输出）。
///
/// 用 lcms 的 `RGBA_16` 像素格式做整段转换，保持 16bit 精度。
pub fn convert_icc_to_srgb_rgba16(
    icc: &[u8],
    rgba: &image::ImageBuffer<image::Rgba<u16>, Vec<u16>>,
) -> Result<image::ImageBuffer<image::Rgba<u16>, Vec<u16>>> {
    let map = |err| FdxmpError::Unsupported(format!("输入 ICC 转换失败：{err}"));
    let source = Profile::new_icc(icc).map_err(map)?;
    let target = Profile::new_srgb();
    let transform: Transform<[u16; 4], [u16; 4]> = Transform::new_flags(
        &source,
        PixelFormat::RGBA_16,
        &target,
        PixelFormat::RGBA_16,
        Intent::RelativeColorimetric,
        Flags::BLACKPOINT_COMPENSATION,
    )
    .map_err(map)?;
    let input: &[[u16; 4]] = bytemuck::cast_slice(rgba.as_raw());
    let mut output = vec![[0u16; 4]; rgba.len()];
    transform.transform_pixels(input, &mut output);
    let raw: Vec<u16> = bytemuck::cast_slice(&output).to_vec();
    image::ImageBuffer::from_raw(rgba.width(), rgba.height(), raw)
        .ok_or_else(|| FdxmpError::Unsupported("输入 ICC 转换输出尺寸不匹配".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_srgb_red_to_display_p3() {
        let mut source = image::RgbaImage::new(2, 1);
        source.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        source.put_pixel(1, 0, image::Rgba([0, 128, 255, 255]));
        let conversion = convert_from_srgb(&source, ColorSpace::DisplayP3).unwrap();
        // P3 色域更大：sRGB 纯红在 P3 中不再饱和到 255。
        let red = conversion.pixels.get_pixel(0, 0).0;
        assert!(red[0] < 250, "sRGB 红在 P3 中应重新编码：{red:?}");
        // ICC 头部：前 4 字节是配置文件长度，36..40 是 `acsp` 签名。
        assert!(conversion.icc.len() > 128);
        assert_eq!(&conversion.icc[36..40], b"acsp");
    }

    #[test]
    fn all_target_spaces_produce_profiles() {
        for space in [
            ColorSpace::Srgb,
            ColorSpace::DisplayP3,
            ColorSpace::AdobeRgb,
            ColorSpace::ProPhotoRgb,
        ] {
            let profile = target_profile(space).unwrap();
            let icc = profile.icc().unwrap();
            assert_eq!(&icc[36..40], b"acsp", "{space:?} 配置无效");
        }
    }

    #[test]
    fn display_p3_roundtrips_back_to_srgb() {
        let mut source = image::RgbaImage::new(4, 1);
        source.put_pixel(0, 0, image::Rgba([128, 128, 128, 255]));
        source.put_pixel(1, 0, image::Rgba([200, 120, 60, 255]));
        source.put_pixel(2, 0, image::Rgba([40, 80, 160, 255]));
        source.put_pixel(3, 0, image::Rgba([250, 250, 250, 255]));

        let to_p3 = convert_from_srgb(&source, ColorSpace::DisplayP3).unwrap();
        let restored = convert_icc_to_srgb(&to_p3.icc, &to_p3.pixels).unwrap();
        for (original, back) in source.pixels().zip(restored.pixels()) {
            for channel in 0..3 {
                let diff = original.0[channel] as i32 - back.0[channel] as i32;
                assert!(
                    diff.abs() <= 2,
                    "sRGB → P3 → sRGB 往返偏差过大：{original:?} → {back:?}"
                );
            }
        }
    }

    #[test]
    fn input_icc_conversion_restores_srgb() {
        // 模拟一张"Adobe RGB + 对应 ICC"的输入图（如带配置文件的 JPEG / TIFF）。
        let mut source = image::RgbaImage::new(3, 1);
        source.put_pixel(0, 0, image::Rgba([128, 128, 128, 255]));
        source.put_pixel(1, 0, image::Rgba([200, 60, 40, 255]));
        source.put_pixel(2, 0, image::Rgba([30, 90, 180, 255]));

        let adobe = convert_from_srgb(&source, ColorSpace::AdobeRgb).unwrap();
        let restored = convert_icc_to_srgb(&adobe.icc, &adobe.pixels).unwrap();
        for (original, back) in source.pixels().zip(restored.pixels()) {
            for channel in 0..3 {
                let diff = original.0[channel] as i32 - back.0[channel] as i32;
                assert!(
                    diff.abs() <= 2,
                    "Adobe RGB → sRGB 输入转换偏差过大：{original:?} → {back:?}"
                );
            }
        }
    }

    #[test]
    fn input_icc_conversion_supports_16bit() {
        let mut source = image::RgbaImage::new(3, 1);
        source.put_pixel(0, 0, image::Rgba([128, 128, 128, 255]));
        source.put_pixel(1, 0, image::Rgba([200, 60, 40, 255]));
        source.put_pixel(2, 0, image::Rgba([30, 90, 180, 255]));
        let adobe = convert_from_srgb(&source, ColorSpace::AdobeRgb).unwrap();
        let reference = convert_icc_to_srgb(&adobe.icc, &adobe.pixels).unwrap();

        // 8bit → 16bit（左对齐）后走 16bit ICC 通路，结果应与 8bit 通路一致
        let mut wide: image::ImageBuffer<image::Rgba<u16>, Vec<u16>> =
            image::ImageBuffer::new(3, 1);
        for (target, pixel) in wide.pixels_mut().zip(adobe.pixels.pixels()) {
            target.0 = [
                pixel.0[0] as u16 * 257,
                pixel.0[1] as u16 * 257,
                pixel.0[2] as u16 * 257,
                65535,
            ];
        }
        let converted = convert_icc_to_srgb_rgba16(&adobe.icc, &wide).unwrap();
        for (index, expected) in reference.pixels().enumerate() {
            let actual = converted.get_pixel(index as u32, 0);
            for channel in 0..3 {
                let value = (actual.0[channel] >> 8) as i32;
                let diff = value - expected.0[channel] as i32;
                assert!(
                    diff.abs() <= 2,
                    "16bit 输入 ICC 转换应与 8bit 一致：{expected:?} vs {actual:?}"
                );
            }
        }
    }
}
