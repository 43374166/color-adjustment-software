//! 阶段 1 解码管线：常见格式（image crate）+ RAW（rsraw / LibRaw）+ HEIF（libheif-rs）。
//!
//! 统一入口：
//!
//! ```text
//! decode(path)          完整质量解码（RAW 走 LibRaw 全尺寸解调）
//! decode_preview(path)  首屏快速路径（RAW 优先取内嵌 JPEG 预览 + 半尺寸解调）
//! decode_with(path, &DecodeOptions)  自定义策略
//! ```

use std::path::Path;
use std::time::Instant;

use image::ImageDecoder;
use serde::{Deserialize, Serialize};

use crate::error::{FdxmpError, Result};
use crate::histogram::Histogram;

/// 支持的输入格式类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    /// JPEG。
    Jpeg,
    /// PNG。
    Png,
    /// TIFF。
    Tiff,
    /// WebP。
    WebP,
    /// BMP。
    Bmp,
    /// GIF。
    Gif,
    /// HEIC / HEIF。
    Heif,
    /// 相机 RAW。
    Raw,
}

impl ImageFormat {
    /// 短名称。
    pub fn name(self) -> &'static str {
        match self {
            ImageFormat::Jpeg => "jpeg",
            ImageFormat::Png => "png",
            ImageFormat::Tiff => "tiff",
            ImageFormat::WebP => "webp",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Gif => "gif",
            ImageFormat::Heif => "heif",
            ImageFormat::Raw => "raw",
        }
    }

    /// 由扩展名推断。
    pub fn from_extension(ext: &str) -> Option<Self> {
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        if RAW_EXTENSIONS.contains(&ext.as_str()) {
            return Some(ImageFormat::Raw);
        }
        Some(match ext.as_str() {
            "jpg" | "jpeg" | "jpe" => ImageFormat::Jpeg,
            "png" => ImageFormat::Png,
            "tif" | "tiff" => ImageFormat::Tiff,
            "webp" => ImageFormat::WebP,
            "bmp" => ImageFormat::Bmp,
            "gif" => ImageFormat::Gif,
            "heic" | "heif" | "avif" => ImageFormat::Heif,
            _ => return None,
        })
    }
}

/// 相机 RAW 扩展名。
pub const RAW_EXTENSIONS: &[&str] = &[
    "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "dng", "raf", "orf", "rw2", "pef",
    "srw", "raw", "rwl", "3fr", "fff", "iiq", "mos", "mrw", "x3f",
];

/// 通过魔数识别格式（RAW 为 TIFF 容器，需结合扩展名判断）。
pub fn sniff_magic(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Some(ImageFormat::Jpeg);
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some(ImageFormat::Png);
    }
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        return Some(ImageFormat::Tiff);
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(ImageFormat::WebP);
    }
    if bytes.starts_with(b"BM") {
        return Some(ImageFormat::Bmp);
    }
    if bytes.starts_with(b"GIF8") {
        return Some(ImageFormat::Gif);
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        if matches!(
            brand,
            b"heic" | b"heix" | b"hevc" | b"mif1" | b"msf1" | b"avif"
        ) {
            return Some(ImageFormat::Heif);
        }
    }
    None
}

/// 解码来源（决定画质与耗时）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecodeSource {
    /// 常见格式一次解码。
    CommonFormat,
    /// RAW 内嵌 JPEG 预览（首屏最快）。
    EmbeddedPreview,
    /// LibRaw 完整解调（16bit）。
    FullRaw,
    /// LibRaw 半尺寸解调。
    HalfRaw,
}

impl DecodeSource {
    /// 中文标签（状态栏 / CLI 输出）。
    pub fn label_zh(self) -> &'static str {
        match self {
            DecodeSource::CommonFormat => "常见格式",
            DecodeSource::EmbeddedPreview => "内嵌预览",
            DecodeSource::FullRaw => "RAW 全尺寸",
            DecodeSource::HalfRaw => "RAW 半尺寸",
        }
    }
}

/// 解码策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecodeOptions {
    /// RAW 优先使用内嵌 JPEG 预览（首屏路径）。
    pub use_embedded_preview: bool,
    /// RAW 使用半尺寸解调（预览 / 低端设备）。
    pub half_size: bool,
    /// 使用相机白平衡（关闭则用 LibRaw 默认）。
    pub camera_white_balance: bool,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            use_embedded_preview: false,
            half_size: false,
            camera_white_balance: true,
        }
    }
}

/// RAW 元数据（画质与导出环节要用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RawMetadata {
    /// 厂商。
    pub make: String,
    /// 机型。
    pub model: String,
    /// ISO。
    pub iso: u32,
    /// 快门时间（秒）。
    pub shutter_seconds: f32,
    /// 光圈。
    pub aperture: f32,
    /// 焦距（mm）。
    pub focal_length_mm: f32,
    /// 拍摄时间（RFC 3339）。
    pub captured_at: Option<String>,
}

impl RawMetadata {
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "{} {} · ISO {} · 1/{:.0}s · f/{:.1} · {:.0}mm",
            self.make.trim(),
            self.model.trim(),
            self.iso,
            if self.shutter_seconds > 0.0 {
                1.0 / self.shutter_seconds
            } else {
                0.0
            },
            self.aperture,
            self.focal_length_mm
        )
    }
}

/// 探测结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageProbe {
    /// 文件路径。
    pub path: String,
    /// 推断格式。
    pub format: Option<ImageFormat>,
    /// 宽度（部分 RAW 在解调前也能给出）。
    pub width: u32,
    /// 高度。
    pub height: u32,
}

/// 解码结果。
#[derive(Debug, Clone)]
pub struct DecodedImage {
    /// 源格式。
    pub format: ImageFormat,
    /// 解码来源。
    pub source: DecodeSource,
    /// 宽度。
    pub width: u32,
    /// 高度。
    pub height: u32,
    /// 像素数据（RAW 为 16bit）。
    pub pixels: image::DynamicImage,
    /// RAW 元数据。
    pub metadata: Option<RawMetadata>,
    /// 耗时（毫秒）。
    pub elapsed_ms: u128,
}

impl DecodedImage {
    /// 百万像素。
    pub fn megapixels(&self) -> f32 {
        (self.width as f32 * self.height as f32) / 1_000_000.0
    }

    /// 屏幕显示用代理图（RGBA8，长边不超过 `max_edge`）。
    ///
    /// 45MP RAW 直接上传纹理要 ~180MB，桌面端先看代理图；
    /// 全分辨率分块渲染在阶段 6 实现。
    pub fn display_proxy(&self, max_edge: u32) -> image::RgbaImage {
        let longest = self.width.max(self.height);
        let pixels = if longest > max_edge && max_edge > 0 {
            self.pixels
                .resize(max_edge, max_edge, image::imageops::FilterType::Triangle)
        } else {
            self.pixels.clone()
        };
        pixels.to_rgba8()
    }

    /// 全分辨率线性 f32 RGBA（sRGB 解码到线性，0..1，不裁剪）。
    ///
    /// 供 GPU 以 `rgba16float` 上传，避免 16bit RAW 在解码 → 上传环节
    /// 被压成 8bit 后再进管线（渐变/暗部少一次量化）。
    pub fn pixels_linear(&self) -> image::Rgba32FImage {
        self.linear_region(0, 0, self.width, self.height)
    }

    /// 指定矩形区域的线性 f32 RGBA。
    ///
    /// 先在 16bit 原始数据上裁剪再解码，避免分块导出为整图保留线性副本
    /// （45MP 的 Rgba32F 约 720MB）。
    pub fn linear_region(&self, x: u32, y: u32, width: u32, height: u32) -> image::Rgba32FImage {
        if self.width == 0 || self.height == 0 {
            return image::Rgba32FImage::new(0, 0);
        }
        let x0 = x.min(self.width - 1);
        let y0 = y.min(self.height - 1);
        let width = width.max(1).min(self.width - x0);
        let height = height.max(1).min(self.height - y0);
        let rgba16 = self.pixels.crop_imm(x0, y0, width, height).to_rgba16();
        let mut output = image::Rgba32FImage::new(width, height);
        for (target, source) in output.pixels_mut().zip(rgba16.pixels()) {
            target.0 = [
                srgb_u16_to_linear(source.0[0]),
                srgb_u16_to_linear(source.0[1]),
                srgb_u16_to_linear(source.0[2]),
                source.0[3] as f32 / 65535.0,
            ];
        }
        output
    }

    /// 线性光代理（长边不超过 `max_edge`），供 GPU `rgba16float` 上传使用。
    ///
    /// 先在 16bit 域缩放再解码到线性：避免为整图建 Rgba32F 副本
    /// （45MP 约 720MB / 数百毫秒）。缩放发生在 sRGB 编码域（16bit），
    /// 线性域缩放只用于导出路径（`export::resize_srgb_linear`）。
    pub fn display_proxy_linear(&self, max_edge: u32) -> image::Rgba32FImage {
        let longest = self.width.max(self.height);
        let (width, height) = if longest > max_edge && max_edge > 0 {
            let scale = max_edge as f32 / longest as f32;
            (
                ((self.width as f32 * scale).round() as u32).max(1),
                ((self.height as f32 * scale).round() as u32).max(1),
            )
        } else {
            (self.width, self.height)
        };
        let resized = self
            .pixels
            .resize_exact(width, height, image::imageops::FilterType::Lanczos3)
            .to_rgba16();
        let mut output = image::Rgba32FImage::new(width, height);
        for (target, source) in output.pixels_mut().zip(resized.pixels()) {
            target.0 = [
                srgb_u16_to_linear(source.0[0]),
                srgb_u16_to_linear(source.0[1]),
                srgb_u16_to_linear(source.0[2]),
                source.0[3] as f32 / 65535.0,
            ];
        }
        output
    }

    /// 一次重采样同时给出"线性 f32 代理"与"其 sRGB 8bit 版本"。
    ///
    /// GPU 用线性版本（`rgba16float`），直方图 / 蒙版 / CPU 回退用 8bit 版本，
    /// 两者像素一一对应。
    pub fn display_proxy_pair(&self, max_edge: u32) -> (image::Rgba32FImage, image::RgbaImage) {
        let linear = self.display_proxy_linear(max_edge);
        let (width, height) = linear.dimensions();
        let mut encoded = image::RgbaImage::new(width, height);
        for (target, source) in encoded.pixels_mut().zip(linear.pixels()) {
            target.0 = [
                linear_to_srgb_u8(source.0[0]),
                linear_to_srgb_u8(source.0[1]),
                linear_to_srgb_u8(source.0[2]),
                (source.0[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
            ];
        }
        (linear, encoded)
    }

    /// 直方图（大图自动抽样）。
    pub fn histogram(&self) -> Histogram {
        Histogram::from_image(&self.pixels)
    }
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_u16_to_linear(value: u16) -> f32 {
    srgb_to_linear(value as f32 / 65535.0)
}

fn linear_to_srgb_u8(value: f32) -> u8 {
    let x = value.clamp(0.0, 1.0);
    let encoded = if x <= 0.003_130_8 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0 + 0.5) as u8
}

/// 探测文件格式与尺寸（不进行完整解码）。
pub fn probe(path: impl AsRef<Path>) -> Result<ImageProbe> {
    let path = path.as_ref();
    let mut file = std::fs::File::open(path)?;
    let mut head = [0u8; 16];
    use std::io::Read;
    let read = file.read(&mut head)?;

    let format = ImageFormat::from_extension(
        path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default(),
    )
    .or_else(|| sniff_magic(&head[..read]));

    let (width, height) = match format {
        Some(ImageFormat::Raw) => probe_raw_size(path).unwrap_or((0, 0)),
        Some(ImageFormat::Heif) => probe_heif_size(path).unwrap_or((0, 0)),
        Some(_) => image::ImageReader::open(path)?
            .with_guessed_format()?
            .into_dimensions()?,
        None => (0, 0),
    };

    Ok(ImageProbe {
        path: path.display().to_string(),
        format,
        width,
        height,
    })
}

/// 完整质量解码。
pub fn decode(path: impl AsRef<Path>) -> Result<DecodedImage> {
    decode_with(path, &DecodeOptions::default())
}

/// 首屏快速解码：RAW 优先内嵌预览 + 半尺寸。
pub fn decode_preview(path: impl AsRef<Path>) -> Result<DecodedImage> {
    decode_with(
        path,
        &DecodeOptions {
            use_embedded_preview: true,
            half_size: true,
            camera_white_balance: true,
        },
    )
}

/// 统一解码入口。
pub fn decode_with(path: impl AsRef<Path>, options: &DecodeOptions) -> Result<DecodedImage> {
    let path = path.as_ref();
    let started = Instant::now();
    let probe = probe(path)?;

    match probe.format {
        Some(ImageFormat::Raw) => decode_raw(path, options, started),
        Some(ImageFormat::Heif) => decode_heif(path, started),
        Some(format) => {
            let pixels = decode_common(path)?;
            Ok(DecodedImage {
                format,
                source: DecodeSource::CommonFormat,
                width: pixels.width(),
                height: pixels.height(),
                pixels,
                metadata: None,
                elapsed_ms: started.elapsed().as_millis(),
            })
        }
        None => Err(FdxmpError::Unsupported(format!(
            "无法识别格式：{}",
            path.display()
        ))),
    }
}

/// 常见格式解码：应用 EXIF 方向（Orientation）与嵌入 ICC（输入侧色彩管理）。
fn decode_common(path: &Path) -> Result<image::DynamicImage> {
    let reader = image::ImageReader::open(path)?
        .with_guessed_format()
        .map_err(|err| FdxmpError::Unsupported(format!("格式识别失败：{err}")))?;
    let mut decoder = reader
        .into_decoder()
        .map_err(|err| FdxmpError::Unsupported(format!("解码器创建失败：{err}")))?;
    let icc = decoder.icc_profile().ok().flatten();
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut pixels = image::DynamicImage::from_decoder(decoder)
        .map_err(|err| FdxmpError::Unsupported(format!("解码失败：{err}")))?;
    pixels.apply_orientation(orientation);
    match icc {
        Some(icc) => apply_input_icc(pixels, &icc),
        None => Ok(pixels),
    }
}

/// 把带非 sRGB ICC 的输入转换到 sRGB 工作空间。
///
/// 目前覆盖 8bit RGB/RGBA（JPEG / PNG / WebP / 8bit TIFF）；
/// 16bit 输入与 RAW 相机 ICC/DCP 记录在色彩修复台账里。
fn apply_input_icc(pixels: image::DynamicImage, icc: &[u8]) -> Result<image::DynamicImage> {
    match pixels {
        image::DynamicImage::ImageRgb8(rgb) => {
            let rgba = image::DynamicImage::ImageRgb8(rgb).to_rgba8();
            let converted = crate::color::convert_icc_to_srgb(icc, &rgba)?;
            Ok(image::DynamicImage::ImageRgba8(converted))
        }
        image::DynamicImage::ImageRgba8(rgba) => {
            let converted = crate::color::convert_icc_to_srgb(icc, &rgba)?;
            Ok(image::DynamicImage::ImageRgba8(converted))
        }
        image::DynamicImage::ImageRgb16(rgb) => {
            let rgba = image::DynamicImage::ImageRgb16(rgb).to_rgba16();
            let converted = crate::color::convert_icc_to_srgb_rgba16(icc, &rgba)?;
            Ok(image::DynamicImage::ImageRgba16(converted))
        }
        image::DynamicImage::ImageRgba16(rgba) => {
            let converted = crate::color::convert_icc_to_srgb_rgba16(icc, &rgba)?;
            Ok(image::DynamicImage::ImageRgba16(converted))
        }
        other => Ok(other),
    }
}

/// RAW 解码（rsraw / LibRaw）。
///
/// 色彩路径：相机白平衡（`use_camera_wb`）+ 相机内嵌色彩矩阵（`use_camera_matrix`）
/// → LibRaw 输出 sRGB 16bit。没有相机 ICC / DCP 风格（见修复台账 C6c）。
#[cfg(feature = "raw")]
pub fn decode_raw(path: &Path, options: &DecodeOptions, started: Instant) -> Result<DecodedImage> {
    let bytes = std::fs::read(path)?;
    let mut raw = rsraw::RawImage::open(&bytes)
        .map_err(|err| FdxmpError::Raw(format!("LibRaw 打开失败：{err}")))?;
    let metadata = raw_metadata(&raw);

    if options.use_embedded_preview {
        if let Some(pixels) = embedded_preview(&mut raw) {
            let (width, height) = (pixels.width(), pixels.height());
            return Ok(DecodedImage {
                format: ImageFormat::Raw,
                source: DecodeSource::EmbeddedPreview,
                width,
                height,
                pixels,
                metadata: Some(metadata),
                elapsed_ms: started.elapsed().as_millis(),
            });
        }
    }

    raw.set_use_camera_wb(options.camera_white_balance);
    // 显式启用相机内嵌色彩矩阵（LibRaw 默认即 3，这里写明意图，避免上游默认值变化）；
    // 注意：rsraw 0.1.1 未暴露 output_color / DCP 等接口，无法做"相机标准 / 中性"风格，
    // 归类见修复台账 C6c。
    raw.set_use_camera_matrix(true);
    if options.half_size {
        raw.set_half_size(true);
    }
    raw.unpack()
        .map_err(|err| FdxmpError::Raw(format!("LibRaw unpack 失败：{err}")))?;
    let processed = raw
        .process::<{ rsraw::BIT_DEPTH_16 }>()
        .map_err(|err| FdxmpError::Raw(format!("LibRaw 解调失败：{err}")))?;

    let width = processed.width();
    let height = processed.height();
    let colors = processed.colors();
    let data: &[u16] = &processed;

    let pixels = match colors {
        1 => image::DynamicImage::ImageLuma16(
            image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_raw(
                width,
                height,
                data.to_vec(),
            )
            .ok_or_else(|| FdxmpError::Raw("灰度数据长度与尺寸不匹配".to_string()))?,
        ),
        3 => image::DynamicImage::ImageRgb16(
            image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_raw(width, height, data.to_vec())
                .ok_or_else(|| FdxmpError::Raw("RGB 数据长度与尺寸不匹配".to_string()))?,
        ),
        4 => image::DynamicImage::ImageRgba16(
            image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(
                width,
                height,
                data.to_vec(),
            )
            .ok_or_else(|| FdxmpError::Raw("RGBA 数据长度与尺寸不匹配".to_string()))?,
        ),
        other => {
            return Err(FdxmpError::Raw(format!("不支持的通道数：{other}")));
        }
    };

    Ok(DecodedImage {
        format: ImageFormat::Raw,
        source: if options.half_size {
            DecodeSource::HalfRaw
        } else {
            DecodeSource::FullRaw
        },
        width,
        height,
        pixels,
        metadata: Some(metadata),
        elapsed_ms: started.elapsed().as_millis(),
    })
}

/// 未启用 `raw` feature 时的提示。
#[cfg(not(feature = "raw"))]
pub fn decode_raw(
    path: &Path,
    _options: &DecodeOptions,
    _started: Instant,
) -> Result<DecodedImage> {
    Err(FdxmpError::RawUnavailable(format!(
        "构建时启用 feature `raw` 后可用：{}",
        path.display()
    )))
}

/// HEIF 解码（libheif-rs，需要本地 libheif ≥ 1.23）。
#[cfg(feature = "heif")]
pub fn decode_heif(path: &Path, started: Instant) -> Result<DecodedImage> {
    let bytes = std::fs::read(path)?;
    let lib_heif = libheif_rs::LibHeif::new();
    let context = libheif_rs::HeifContext::read_from_bytes(&bytes)
        .map_err(|err| FdxmpError::Heif(format!("libheif 打开失败：{err}")))?;
    let handle = context
        .primary_image_handle()
        .map_err(|err| FdxmpError::Heif(format!("缺少主图像：{err}")))?;
    let image = lib_heif
        .decode(
            &handle,
            libheif_rs::ColorSpace::Rgb(libheif_rs::RgbChroma::Rgb),
            None,
        )
        .map_err(|err| FdxmpError::Heif(format!("libheif 解码失败：{err}")))?;

    let width = image.width();
    let height = image.height();
    let planes = image.planes();
    let plane = planes
        .interleaved
        .ok_or_else(|| FdxmpError::Heif("libheif 未返回交错 RGB 平面".to_string()))?;
    let stride = plane.stride;
    let data = plane.data;
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for row in 0..height as usize {
        let start = row * stride;
        rgb.extend_from_slice(&data[start..start + width as usize * 3]);
    }
    let pixels = image::DynamicImage::ImageRgb8(
        image::ImageBuffer::<image::Rgb<u8>, Vec<u8>>::from_raw(width, height, rgb)
            .ok_or_else(|| FdxmpError::Heif("RGB 数据长度与尺寸不匹配".to_string()))?,
    );

    Ok(DecodedImage {
        format: ImageFormat::Heif,
        source: DecodeSource::CommonFormat,
        width,
        height,
        pixels,
        metadata: None,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

/// 未启用 `heif` feature 时的提示。
#[cfg(not(feature = "heif"))]
pub fn decode_heif(path: &Path, _started: Instant) -> Result<DecodedImage> {
    Err(FdxmpError::HeifUnavailable(format!(
        "构建时启用 feature `heif` 后可用：{}",
        path.display()
    )))
}

/// RAW 尺寸探测（打开文件头即可，不需要完整解调）。
#[cfg(feature = "raw")]
fn probe_raw_size(path: &Path) -> Option<(u32, u32)> {
    let bytes = std::fs::read(path).ok()?;
    let raw = rsraw::RawImage::open(&bytes).ok()?;
    let (width, height) = (raw.width(), raw.height());
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

#[cfg(not(feature = "raw"))]
fn probe_raw_size(_path: &Path) -> Option<(u32, u32)> {
    None
}

/// HEIF 尺寸探测（只读容器信息，不解码像素）。
#[cfg(feature = "heif")]
fn probe_heif_size(path: &Path) -> Option<(u32, u32)> {
    let bytes = std::fs::read(path).ok()?;
    let context = libheif_rs::HeifContext::read_from_bytes(&bytes).ok()?;
    let handle = context.primary_image_handle().ok()?;
    let (width, height) = (handle.width(), handle.height());
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

#[cfg(not(feature = "heif"))]
fn probe_heif_size(_path: &Path) -> Option<(u32, u32)> {
    None
}

#[cfg(feature = "raw")]
fn raw_metadata(raw: &rsraw::RawImage) -> RawMetadata {
    RawMetadata {
        make: raw.make().to_string(),
        model: raw.model().to_string(),
        iso: raw.iso_speed(),
        shutter_seconds: raw.shutter(),
        aperture: raw.aperture(),
        focal_length_mm: raw.focal_len(),
        captured_at: raw.datetime().map(|stamp| stamp.to_rfc3339()),
    }
}

/// 取最大的内嵌 JPEG 预览。
#[cfg(feature = "raw")]
fn embedded_preview(raw: &mut rsraw::RawImage) -> Option<image::DynamicImage> {
    let thumbs = raw.extract_thumbs().ok()?;
    let best = thumbs
        .iter()
        .filter(|thumb| {
            matches!(thumb.format, rsraw::ThumbFormat::Jpeg)
                && thumb.width >= 640
                && thumb.height > 0
        })
        .max_by_key(|thumb| thumb.width * thumb.height)?;
    image::load_from_memory_with_format(&best.data, image::ImageFormat::Jpeg).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_sniffing() {
        assert_eq!(
            sniff_magic(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(
            sniff_magic(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A]),
            Some(ImageFormat::Png)
        );
        assert_eq!(sniff_magic(b"GIF89a"), Some(ImageFormat::Gif));
        assert_eq!(sniff_magic(b"RIFF____WEBP"), Some(ImageFormat::WebP));
        assert_eq!(sniff_magic(&[0, 0, 0, 0]), None);
    }

    #[test]
    fn extension_mapping() {
        assert_eq!(ImageFormat::from_extension(".CR3"), Some(ImageFormat::Raw));
        assert_eq!(ImageFormat::from_extension("heic"), Some(ImageFormat::Heif));
        assert_eq!(ImageFormat::from_extension("txt"), None);
    }

    #[cfg(not(feature = "raw"))]
    #[test]
    fn raw_error_mentions_feature() {
        let err = decode_raw(
            Path::new("a.cr3"),
            &DecodeOptions::default(),
            Instant::now(),
        )
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("raw"));
    }

    #[test]
    fn linear_region_matches_full_linear_crop() {
        let mut pixels = image::RgbImage::new(8, 6);
        for (x, y, pixel) in pixels.enumerate_pixels_mut() {
            *pixel = image::Rgb([(x * 30) as u8, (y * 40) as u8, 128]);
        }
        let decoded = DecodedImage {
            format: ImageFormat::Png,
            source: DecodeSource::CommonFormat,
            width: 8,
            height: 6,
            pixels: image::DynamicImage::ImageRgb8(pixels),
            metadata: None,
            elapsed_ms: 0,
        };
        let full = decoded.pixels_linear();
        let region = decoded.linear_region(2, 3, 4, 2);
        assert_eq!(region.dimensions(), (4, 2));
        for y in 0..2 {
            for x in 0..4 {
                let expected = full.get_pixel(x + 2, y + 3);
                let actual = region.get_pixel(x, y);
                for channel in 0..4 {
                    assert!(
                        (expected.0[channel] - actual.0[channel]).abs() < 1e-6,
                        "区域线性数据应与整图一致：({x},{y}) 通道 {channel}"
                    );
                }
            }
        }
        // 越界请求应夹取到图像内
        let clamped = decoded.linear_region(6, 5, 100, 100);
        assert_eq!(clamped.dimensions(), (2, 1));
    }

    #[test]
    fn display_proxy_linear_scales_without_full_copy() {
        let mut pixels = image::RgbImage::new(100, 50);
        for pixel in pixels.pixels_mut() {
            *pixel = image::Rgb([128, 64, 200]);
        }
        let decoded = DecodedImage {
            format: ImageFormat::Png,
            source: DecodeSource::CommonFormat,
            width: 100,
            height: 50,
            pixels: image::DynamicImage::ImageRgb8(pixels),
            metadata: None,
            elapsed_ms: 0,
        };
        let proxy = decoded.display_proxy_linear(25);
        assert_eq!(proxy.dimensions(), (25, 13));
        let full = decoded.pixels_linear();
        // 均匀色图：缩放后线性值应与整图一致
        for channel in 0..3 {
            let diff =
                (proxy.get_pixel(12, 6).0[channel] - full.get_pixel(50, 25).0[channel]).abs();
            assert!(
                diff < 0.01,
                "代理线性值应与整图一致：通道 {channel} 差 {diff}"
            );
        }
    }

    #[test]
    fn decode_options_defaults_are_quality_first() {
        let options = DecodeOptions::default();
        assert!(!options.use_embedded_preview);
        assert!(!options.half_size);
        assert!(options.camera_white_balance);
    }

    #[test]
    fn metadata_summary_formats() {
        let metadata = RawMetadata {
            make: "Nikon".to_string(),
            model: "Z 8".to_string(),
            iso: 400,
            shutter_seconds: 1.0 / 250.0,
            aperture: 4.0,
            focal_length_mm: 50.0,
            captured_at: None,
        };
        let summary = metadata.summary();
        assert!(summary.contains("Nikon Z 8"));
        assert!(summary.contains("ISO 400"));
    }
}
