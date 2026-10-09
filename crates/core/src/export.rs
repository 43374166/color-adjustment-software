//! 阶段 4：导出管线。
//!
//! - 格式：JPEG / PNG / TIFF / WebP（image crate）+ HEIC（libheif-rs，`heif` feature）
//! - 尺寸：原始 / 长边 / 短边 / 百分比
//! - 元数据：保留 / 删除（重编码；删除为默认，保留在阶段 6 接入 EXIF 写回）
//! - 水印：文字（内置 5×7 点阵字体，ASCII）/ 图片（PNG 叠加，含不透明度）
//! - 批处理：`export_batch` 顺序导出多个文件

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{FdxmpError, Result};

/// 导出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// JPEG（有损）。
    #[default]
    Jpeg,
    /// PNG（无损）。
    Png,
    /// TIFF（无损）。
    Tiff,
    /// HEIC（需要 `heif` feature）。
    Heic,
    /// WebP。
    WebP,
}

impl ExportFormat {
    /// 扩展名。
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Jpeg => "jpg",
            ExportFormat::Png => "png",
            ExportFormat::Tiff => "tiff",
            ExportFormat::Heic => "heic",
            ExportFormat::WebP => "webp",
        }
    }

    /// 中文标签。
    pub fn label_zh(self) -> &'static str {
        match self {
            ExportFormat::Jpeg => "JPEG",
            ExportFormat::Png => "PNG",
            ExportFormat::Tiff => "TIFF",
            ExportFormat::Heic => "HEIC",
            ExportFormat::WebP => "WebP",
        }
    }
}

/// 导出色彩空间。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColorSpace {
    /// sRGB。
    #[default]
    Srgb,
    /// Display P3。
    DisplayP3,
    /// Adobe RGB。
    AdobeRgb,
    /// ProPhoto RGB。
    ProPhotoRgb,
}

impl ColorSpace {
    /// 中文标签。
    pub fn label_zh(self) -> &'static str {
        match self {
            ColorSpace::Srgb => "sRGB",
            ColorSpace::DisplayP3 => "Display P3",
            ColorSpace::AdobeRgb => "Adobe RGB",
            ColorSpace::ProPhotoRgb => "ProPhoto RGB",
        }
    }
}

/// 尺寸策略。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", content = "value", rename_all = "snake_case")]
pub enum ResizeSpec {
    /// 原始尺寸。
    #[default]
    Original,
    /// 长边像素。
    LongEdge(u32),
    /// 短边像素。
    ShortEdge(u32),
    /// 百分比。
    Percent(f32),
}

/// 元数据处理。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MetadataPolicy {
    /// 保留（阶段 6 写回 EXIF）。
    Keep,
    /// 删除。
    #[default]
    Strip,
}

/// 水印。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Watermark {
    /// 无水印。
    #[default]
    None,
    /// 文字水印（ASCII 点阵字体）。
    Text {
        /// 文本。
        text: String,
        /// 不透明度，0..=100。
        opacity: f32,
        /// 字号（像素）。
        size: u32,
    },
    /// 图片水印。
    Image {
        /// PNG 路径（带 alpha）。
        path: String,
        /// 不透明度，0..=100。
        opacity: f32,
    },
}

/// 导出设置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    /// 格式。
    pub format: ExportFormat,
    /// 质量，1..=100（JPEG / HEIC 生效）。
    ///
    /// WebP 目前走 `image` crate 的无损编码器，质量参数不生效
    /// （有损 WebP 需要额外编码器，见修复台账 C15c）。
    pub quality: u8,
    /// 尺寸。
    pub resize: ResizeSpec,
    /// 色彩空间。
    pub color_space: ColorSpace,
    /// 元数据。
    pub metadata: MetadataPolicy,
    /// 水印。
    pub watermark: Watermark,
    /// 输出目录。
    pub output_dir: String,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            format: ExportFormat::Jpeg,
            quality: 90,
            resize: ResizeSpec::Original,
            color_space: ColorSpace::Srgb,
            metadata: MetadataPolicy::Strip,
            watermark: Watermark::None,
            output_dir: String::new(),
        }
    }
}

/// 按设置计算目标尺寸。
pub fn target_size(width: u32, height: u32, resize: ResizeSpec) -> (u32, u32) {
    match resize {
        ResizeSpec::Original => (width, height),
        ResizeSpec::LongEdge(edge) => {
            let edge = edge.max(16);
            if width >= height {
                let new_height = ((height as f32) * (edge as f32) / (width as f32)).round();
                (edge, new_height.max(1.0) as u32)
            } else {
                let new_width = ((width as f32) * (edge as f32) / (height as f32)).round();
                (new_width.max(1.0) as u32, edge)
            }
        }
        ResizeSpec::ShortEdge(edge) => {
            let edge = edge.max(16);
            if width <= height {
                let new_height = ((height as f32) * (edge as f32) / (width as f32)).round();
                (edge, new_height.max(1.0) as u32)
            } else {
                let new_width = ((width as f32) * (edge as f32) / (height as f32)).round();
                (new_width.max(1.0) as u32, edge)
            }
        }
        ResizeSpec::Percent(percent) => {
            let factor = (percent / 100.0).clamp(0.01, 4.0);
            (
                ((width as f32 * factor).round() as u32).max(1),
                ((height as f32 * factor).round() as u32).max(1),
            )
        }
    }
}

/// 导出结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOutcome {
    /// 输出路径。
    pub path: PathBuf,
    /// 输出尺寸。
    pub width: u32,
    /// 输出高度。
    pub height: u32,
    /// 文件字节数。
    pub bytes: u64,
}

/// 导出单张图片（输入为 sRGB 编码的 RGBA8）。
pub fn export_rgba(
    rgba: &image::RgbaImage,
    source_stem: &str,
    settings: &ExportSettings,
) -> Result<ExportOutcome> {
    export_rgba_impl(rgba, source_stem, settings, None)
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

/// 线性光缩放：在 sRGB 编码值上直接重采样会让边缘 / 中间调产生 gamma 偏差，
/// 这里先解码到线性 f32、用 Lanczos3 重采样、再编码回 sRGB。
fn resize_srgb_linear(rgba: &image::RgbaImage, width: u32, height: u32) -> image::RgbaImage {
    let mut linear = image::Rgba32FImage::new(rgba.width(), rgba.height());
    for (target, source) in linear.pixels_mut().zip(rgba.pixels()) {
        target.0 = [
            srgb_to_linear(source.0[0] as f32 / 255.0),
            srgb_to_linear(source.0[1] as f32 / 255.0),
            srgb_to_linear(source.0[2] as f32 / 255.0),
            source.0[3] as f32 / 255.0,
        ];
    }
    let resized = image::imageops::resize(
        &linear,
        width.max(1),
        height.max(1),
        image::imageops::FilterType::Lanczos3,
    );
    let mut output = image::RgbaImage::new(width.max(1), height.max(1));
    for (target, source) in output.pixels_mut().zip(resized.pixels()) {
        target.0 = [
            (linear_to_srgb(source.0[0]) * 255.0 + 0.5) as u8,
            (linear_to_srgb(source.0[1]) * 255.0 + 0.5) as u8,
            (linear_to_srgb(source.0[2]) * 255.0 + 0.5) as u8,
            (source.0[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        ];
    }
    output
}

/// 导出主体：色彩空间转换 → 编码 → ICC 写回 → 元数据写回。
fn export_rgba_impl(
    rgba: &image::RgbaImage,
    source_stem: &str,
    settings: &ExportSettings,
    source_file: Option<&Path>,
) -> Result<ExportOutcome> {
    let (width, height) = target_size(rgba.width(), rgba.height(), settings.resize);
    let resized = if (width, height) == (rgba.width(), rgba.height()) {
        rgba.clone()
    } else {
        resize_srgb_linear(rgba, width, height)
    };
    let watermarked = apply_watermark(&resized, &settings.watermark)?;
    // 色彩空间：非 sRGB 时做 ICC 转换，并记录需要写回的配置文件。
    let (pixels, icc_profile) = if settings.color_space == ColorSpace::Srgb {
        (watermarked, None)
    } else {
        match crate::color::convert_from_srgb(&watermarked, settings.color_space) {
            Ok(conversion) => (conversion.pixels, Some(conversion.icc)),
            // 转换失败时保持 sRGB 并继续导出（不阻断出图）。
            Err(_) => (watermarked, None),
        }
    };

    let directory = if settings.output_dir.trim().is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(&settings.output_dir)
    };
    std::fs::create_dir_all(&directory)?;
    let file_name = format!("{source_stem}.{}", settings.format.extension());
    let path = directory.join(file_name);

    // 元数据要在编码前取（HEIC 需要编码时写入容器，其余格式编码后写回）
    let source_metadata = if settings.metadata == MetadataPolicy::Keep {
        source_file.map(extract_source_metadata).unwrap_or_default()
    } else {
        (None, Vec::new())
    };

    match settings.format {
        ExportFormat::Jpeg => {
            let rgb = image::DynamicImage::ImageRgba8(pixels).to_rgb8();
            let file = std::fs::File::create(&path)?;
            let mut writer = std::io::BufWriter::new(file);
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut writer,
                settings.quality.clamp(1, 100),
            );
            encoder
                .encode(&rgb, width, height, image::ExtendedColorType::Rgb8)
                .map_err(|err| FdxmpError::Unsupported(format!("JPEG 编码失败：{err}")))?;
        }
        ExportFormat::Png => {
            image::DynamicImage::ImageRgba8(pixels)
                .save_with_format(&path, image::ImageFormat::Png)
                .map_err(|err| FdxmpError::Unsupported(format!("PNG 编码失败：{err}")))?;
        }
        ExportFormat::Tiff => {
            let dynamic = image::DynamicImage::ImageRgba8(pixels);
            dynamic
                .save_with_format(&path, image::ImageFormat::Tiff)
                .map_err(|err| FdxmpError::Unsupported(format!("TIFF 编码失败：{err}")))?;
        }
        ExportFormat::WebP => {
            image::DynamicImage::ImageRgba8(pixels)
                .save_with_format(&path, image::ImageFormat::WebP)
                .map_err(|err| FdxmpError::Unsupported(format!("WebP 编码失败：{err}")))?;
        }
        ExportFormat::Heic => {
            export_heic(
                &pixels,
                &path,
                settings.quality,
                source_metadata.0.as_deref(),
                &source_metadata.1,
            )?;
        }
    }

    // ICC / 元数据写回（尽力而为）。WebP 需要一次重写 RIFF 容器，
    // HEIC 已在编码阶段写入容器（此处跳过），其余格式分步写。
    if settings.format == ExportFormat::WebP {
        let _ = insert_webp_metadata(
            &path,
            icc_profile.as_deref(),
            source_metadata.0.as_deref(),
            &source_metadata.1,
        );
    } else if settings.format != ExportFormat::Heic {
        if let Some(icc) = icc_profile.as_deref() {
            let _ = insert_icc_profile(&path, icc, settings.format, width, height);
        }
        if settings.metadata == MetadataPolicy::Keep {
            if let Some(source) = source_file {
                let _ = preserve_metadata(&path, source, settings.format);
            }
        }
    }

    let bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    Ok(ExportOutcome {
        path,
        width,
        height,
        bytes,
    })
}

/// 批处理导出（顺序执行，避免占满内存）。
pub fn export_batch(
    images: &[(String, image::RgbaImage)],
    settings: &ExportSettings,
) -> Result<Vec<ExportOutcome>> {
    let mut outcomes = Vec::with_capacity(images.len());
    for (stem, image) in images {
        outcomes.push(export_rgba(image, stem, settings)?);
    }
    Ok(outcomes)
}

/// 导出单张图片，并在 `metadata == Keep` 时把源文件的 EXIF / XMP 写回输出。
///
/// 支持：JPEG（APP1 段）与 PNG（eXIf 块）。源文件可以是 JPEG / PNG，
/// 也可以是 TIFF 系 RAW（扫描其内嵌 JPEG 预览的 APP1 段）。
pub fn export_rgba_with_source(
    rgba: &image::RgbaImage,
    source_stem: &str,
    settings: &ExportSettings,
    source: Option<&Path>,
) -> Result<ExportOutcome> {
    export_rgba_impl(rgba, source_stem, settings, source)
}

/// 读取源文件字节（超过 256 MB 直接跳过，避免占用过多内存）。
fn read_source_bytes(path: &Path) -> Option<Vec<u8>> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > 256 * 1024 * 1024 {
        return None;
    }
    std::fs::read(path).ok()
}

/// 扫描字节流中的 JPEG APP1 段，返回 (EXIF TIFF 负载, XMP 段原始字节)。
///
/// 对 TIFF 系 RAW（NEF / CR2 / ARW 等）同样有效：它们内嵌的 JPEG 预览带 APP1。
fn scan_jpeg_app1(bytes: &[u8]) -> (Option<Vec<u8>>, Vec<Vec<u8>>) {
    const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
    let mut exif: Option<Vec<u8>> = None;
    let mut xmp: Vec<Vec<u8>> = Vec::new();
    let mut index = 0usize;
    while index + 4 < bytes.len() {
        if bytes[index] == 0xFF && bytes[index + 1] == 0xE1 {
            let length = u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]) as usize;
            let payload_start = index + 4;
            let payload_end = (index + 2 + length).min(bytes.len());
            if length >= 2 && payload_start < payload_end {
                let payload = &bytes[payload_start..payload_end];
                if payload.len() > 6 && &payload[..6] == b"Exif\0\0" {
                    exif.get_or_insert_with(|| payload[6..].to_vec());
                } else if payload.len() > XMP_HEADER.len()
                    && &payload[..XMP_HEADER.len()] == XMP_HEADER
                {
                    xmp.push(bytes[index..payload_end].to_vec());
                }
            }
            index = payload_end.max(index + 4);
            continue;
        }
        index += 1;
    }
    (exif, xmp)
}

/// PNG 签名判断。
fn is_png(bytes: &[u8]) -> bool {
    bytes.len() > 8 && &bytes[..8] == b"\x89PNG\r\n\x1a\n"
}

/// 解析 PNG 块，取出 eXIf（TIFF 负载）。
fn scan_png_exif(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut index = 8usize;
    while index + 8 <= bytes.len() {
        let length = u32::from_be_bytes([
            bytes[index],
            bytes[index + 1],
            bytes[index + 2],
            bytes[index + 3],
        ]) as usize;
        let kind = &bytes[index + 4..index + 8];
        let data_start = index + 8;
        let data_end = data_start + length;
        if data_end + 4 > bytes.len() {
            break;
        }
        if kind == b"eXIf" {
            return Some(bytes[data_start..data_end].to_vec());
        }
        index = data_end + 4;
    }
    None
}

/// CRC32（IEEE，PNG 块校验）。
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// 在 PNG 的 IHDR 之后插入一个标准块（长度 + 类型 + 数据 + CRC）。
fn insert_png_chunk(path: &Path, kind: &[u8; 4], payload: &[u8]) -> Result<()> {
    let bytes = std::fs::read(path)?;
    if !is_png(&bytes) || bytes.len() < 33 {
        return Ok(());
    }
    let ihdr_length = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    let insert_at = 8 + 8 + ihdr_length + 4;
    if insert_at > bytes.len() {
        return Ok(());
    }
    let mut chunk = Vec::with_capacity(payload.len() + 12);
    chunk.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    chunk.extend_from_slice(kind);
    chunk.extend_from_slice(payload);
    let mut crc_input = Vec::with_capacity(payload.len() + 4);
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(payload);
    chunk.extend_from_slice(&crc32(&crc_input).to_be_bytes());

    let mut output = Vec::with_capacity(bytes.len() + chunk.len());
    output.extend_from_slice(&bytes[..insert_at]);
    output.extend_from_slice(&chunk);
    output.extend_from_slice(&bytes[insert_at..]);
    std::fs::write(path, output)?;
    Ok(())
}

/// 在 PNG 的 IHDR 之后插入 eXIf 块。
fn insert_png_exif(path: &Path, payload: &[u8]) -> Result<()> {
    insert_png_chunk(path, b"eXIf", payload)
}

/// JPEG：把 ICC 配置文件写成 APP2 段（超过 65519 字节时按规范分片）。
fn insert_jpeg_icc(path: &Path, icc: &[u8]) -> Result<()> {
    if icc.is_empty() {
        return Ok(());
    }
    let bytes = std::fs::read(path)?;
    if bytes.len() < 2 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Ok(());
    }
    const CHUNK: usize = 65519;
    let chunks: Vec<&[u8]> = icc.chunks(CHUNK).collect();
    if chunks.is_empty() || chunks.len() > 255 {
        return Ok(());
    }
    let total = chunks.len() as u8;
    let mut prefix = Vec::new();
    for (index, chunk) in chunks.iter().enumerate() {
        let length = chunk.len() + 14;
        prefix.extend_from_slice(&[0xFF, 0xE2]);
        prefix.extend_from_slice(&(length as u16).to_be_bytes());
        prefix.extend_from_slice(b"ICC_PROFILE\0");
        prefix.push((index + 1) as u8);
        prefix.push(total);
        prefix.extend_from_slice(chunk);
    }
    let mut output = Vec::with_capacity(bytes.len() + prefix.len());
    output.extend_from_slice(&bytes[..2]);
    output.extend_from_slice(&prefix);
    output.extend_from_slice(&bytes[2..]);
    std::fs::write(path, output)?;
    Ok(())
}

/// 把 ICC 配置文件写入输出文件（JPEG APP2 / PNG iCCP；WebP 走 [`insert_webp_metadata`]）。
fn insert_icc_profile(
    path: &Path,
    icc: &[u8],
    format: ExportFormat,
    _width: u32,
    _height: u32,
) -> Result<()> {
    match format {
        ExportFormat::Jpeg => insert_jpeg_icc(path, icc),
        ExportFormat::Png => {
            use flate2::write::ZlibEncoder;
            use flate2::Compression;
            use std::io::Write;

            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(icc)?;
            let compressed = encoder.finish()?;
            let mut payload = Vec::with_capacity(compressed.len() + 16);
            payload.extend_from_slice(b"ICC profile\0");
            payload.push(0); // 压缩方法：deflate
            payload.extend_from_slice(&compressed);
            insert_png_chunk(path, b"iCCP", &payload)
        }
        _ => Ok(()),
    }
}

/// 追加一个 RIFF 块（FourCC + 小端长度 + 数据 + 偶数对齐填充）。
fn push_riff_chunk(output: &mut Vec<u8>, fourcc: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(fourcc);
    output.extend_from_slice(&(data.len() as u32).to_le_bytes());
    output.extend_from_slice(data);
    if data.len() % 2 == 1 {
        output.push(0);
    }
}

/// 把 ICC / EXIF / XMP 写入 WebP：转换/补齐为扩展格式（VP8X + 元数据块 + 原图像块）。
///
/// 规范要求 ICCP / EXIF / XMP 块位于 VP8X 扩展头之后、图像数据之前；
/// 这里重写整个 RIFF 容器（保留原图像块，去掉可能存在的旧扩展块）并回填长度与标志位。
fn insert_webp_metadata(
    path: &Path,
    icc: Option<&[u8]>,
    exif: Option<&[u8]>,
    xmp_segments: &[Vec<u8>],
) -> Result<()> {
    const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
    let icc = icc.filter(|data| !data.is_empty());
    let exif = exif.filter(|data| !data.is_empty());
    // JPEG APP1 里保存的是完整段（含 0xFFE1 标记与 XMP 头），WebP 需要裸 XMP 包
    let xmp_packets: Vec<&[u8]> = xmp_segments
        .iter()
        .filter_map(|segment| {
            segment
                .windows(XMP_HEADER.len())
                .position(|window| window == XMP_HEADER)
                .map(|at| &segment[at + XMP_HEADER.len()..])
        })
        .collect();
    if icc.is_none() && exif.is_none() && xmp_packets.is_empty() {
        return Ok(());
    }
    let bytes = std::fs::read(path)?;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err(FdxmpError::Unsupported(
            "WebP 元数据写回失败：文件不是有效的 RIFF/WEBP".to_string(),
        ));
    }
    // WebP 的 VP8X 画布尺寸字段（显式给出，避免解析 VP8/VP8L 头）
    let (width, height) = webp_canvas_size(&bytes)
        .ok_or_else(|| FdxmpError::Unsupported("WebP 画布尺寸读取失败".to_string()))?;
    let mut image_chunks = Vec::new();
    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        let fourcc = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes(
            bytes[offset + 4..offset + 8]
                .try_into()
                .map_err(|_| FdxmpError::Unsupported("WebP 块长度读取失败".to_string()))?,
        ) as usize;
        let payload_end = (offset + 8 + size).min(bytes.len());
        if !matches!(fourcc, b"VP8X" | b"ICCP" | b"EXIF" | b"XMP ") {
            image_chunks.extend_from_slice(&bytes[offset..payload_end]);
            if size % 2 == 1 && payload_end < bytes.len() {
                image_chunks.push(0);
            }
        }
        offset = payload_end + (size & 1);
    }

    let mut output = Vec::with_capacity(bytes.len() + icc.map(|data| data.len()).unwrap_or(0) + 32);
    output.extend_from_slice(b"RIFF");
    output.extend_from_slice(&0u32.to_le_bytes()); // 稍后回填
    output.extend_from_slice(b"WEBP");
    let mut flags = 0u8;
    if icc.is_some() {
        flags |= 0x20;
    }
    if exif.is_some() {
        flags |= 0x08;
    }
    if !xmp_packets.is_empty() {
        flags |= 0x04;
    }
    let mut vp8x = Vec::with_capacity(10);
    vp8x.push(flags);
    vp8x.extend_from_slice(&[0, 0, 0]);
    vp8x.extend_from_slice(&width.saturating_sub(1).to_le_bytes()[0..3]);
    vp8x.extend_from_slice(&height.saturating_sub(1).to_le_bytes()[0..3]);
    push_riff_chunk(&mut output, b"VP8X", &vp8x);
    if let Some(icc) = icc {
        push_riff_chunk(&mut output, b"ICCP", icc);
    }
    if let Some(exif) = exif {
        push_riff_chunk(&mut output, b"EXIF", exif);
    }
    for packet in &xmp_packets {
        push_riff_chunk(&mut output, b"XMP ", packet);
    }
    output.extend_from_slice(&image_chunks);
    let riff_size = (output.len() - 8) as u32;
    output[4..8].copy_from_slice(&riff_size.to_le_bytes());
    std::fs::write(path, output)?;
    Ok(())
}

/// 读取 WebP 画布尺寸：优先取 VP8X 扩展头，否则解析 VP8 / VP8L 图像头。
fn webp_canvas_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        let fourcc = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
        let payload = bytes.get(offset + 8..(offset + 8 + size).min(bytes.len()))?;
        match fourcc {
            b"VP8X" if payload.len() >= 10 => {
                let width = 1 + u32::from_le_bytes([payload[4], payload[5], payload[6], 0]);
                let height = 1 + u32::from_le_bytes([payload[7], payload[8], payload[9], 0]);
                return Some((width, height));
            }
            b"VP8L" if payload.len() >= 5 && payload[0] == 0x2F => {
                // 14 位宽 / 高，各减一后拼成 +1
                let bits = u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
                let width = (bits & 0x3FFF) + 1;
                let height = ((bits >> 14) & 0x3FFF) + 1;
                return Some((width, height));
            }
            b"VP8 " if payload.len() >= 10 => {
                // 关键帧头：3 字节帧标签 + 3 字节同步码 + 2 字节宽 + 2 字节高
                let width = u16::from_le_bytes([payload[6], payload[7]]) as u32 & 0x3FFF;
                let height = u16::from_le_bytes([payload[8], payload[9]]) as u32 & 0x3FFF;
                return Some((width, height));
            }
            _ => {}
        }
        offset += 8 + size + (size & 1);
    }
    None
}

/// 在 JPEG 的 SOI 之后插入 EXIF APP1 段与 XMP APP1 段。
fn insert_jpeg_app1(path: &Path, exif: Option<&[u8]>, xmp: &[Vec<u8>]) -> Result<()> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < 2 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Ok(());
    }
    let mut prefix = Vec::new();
    if let Some(payload) = exif {
        let length = payload.len() + 8;
        if length <= u16::MAX as usize {
            prefix.extend_from_slice(&[0xFF, 0xE1]);
            prefix.extend_from_slice(&(length as u16).to_be_bytes());
            prefix.extend_from_slice(b"Exif\0\0");
            prefix.extend_from_slice(payload);
        }
    }
    for segment in xmp {
        prefix.extend_from_slice(segment);
    }
    if prefix.is_empty() {
        return Ok(());
    }
    let mut output = Vec::with_capacity(bytes.len() + prefix.len());
    output.extend_from_slice(&bytes[..2]);
    output.extend_from_slice(&prefix);
    output.extend_from_slice(&bytes[2..]);
    std::fs::write(path, output)?;
    Ok(())
}

/// 从源文件提取并写回元数据（JPEG / PNG；其余格式暂不写回）。
fn preserve_metadata(output: &Path, source: &Path, format: ExportFormat) -> Result<()> {
    let (exif, xmp) = extract_source_metadata(source);
    if exif.is_none() && xmp.is_empty() {
        return Ok(());
    }
    match format {
        ExportFormat::Jpeg => insert_jpeg_app1(output, exif.as_deref(), &xmp),
        ExportFormat::Png => {
            if let Some(payload) = exif.as_deref() {
                insert_png_exif(output, payload)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// 从源文件提取 EXIF / XMP（JPEG APP1 与 PNG eXIf）。
fn extract_source_metadata(source: &Path) -> (Option<Vec<u8>>, Vec<Vec<u8>>) {
    let Some(bytes) = read_source_bytes(source) else {
        return (None, Vec::new());
    };
    if is_png(&bytes) {
        (scan_png_exif(&bytes), Vec::new())
    } else {
        scan_jpeg_app1(&bytes)
    }
}

/// HEIC 编码（libheif-rs）。
#[cfg(feature = "heif")]
fn export_heic(
    rgba: &image::RgbaImage,
    path: &Path,
    quality: u8,
    exif: Option<&[u8]>,
    xmp_segments: &[Vec<u8>],
) -> Result<()> {
    use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};

    use libheif_rs::Channel;

    let lib_heif = LibHeif::new();
    let mut context = HeifContext::new()
        .map_err(|err| FdxmpError::Heif(format!("libheif 上下文创建失败：{err}")))?;
    let mut encoder = lib_heif
        .encoder_for_format(libheif_rs::CompressionFormat::Hevc)
        .map_err(|err| FdxmpError::Heif(format!("HEVC 编码器不可用：{err}")))?;
    encoder
        .set_quality(libheif_rs::EncoderQuality::Lossy(quality.clamp(1, 100)))
        .map_err(|err| FdxmpError::Heif(format!("HEIC 质量设置失败：{err}")))?;

    let (width, height) = (rgba.width(), rgba.height());
    let mut image = libheif_rs::Image::new(width, height, ColorSpace::Rgb(RgbChroma::Rgb))
        .map_err(|err| FdxmpError::Heif(format!("libheif 图像创建失败：{err}")))?;
    // libheif 不会自动分配平面内存，需要显式创建交错 RGB 平面（8bit/通道）
    image
        .create_plane(Channel::Interleaved, width, height, 8)
        .map_err(|err| FdxmpError::Heif(format!("libheif 平面创建失败：{err}")))?;
    {
        let mut planes = image.planes_mut();
        let plane = planes
            .interleaved
            .as_mut()
            .ok_or_else(|| FdxmpError::Heif("libheif 未提供交错 RGB 平面".to_string()))?;
        let stride = plane.stride;
        let data = &mut plane.data;
        for row in 0..height as usize {
            let source_row =
                &rgba.as_raw()[row * width as usize * 4..(row + 1) * width as usize * 4];
            let target = &mut data[row * stride..row * stride + width as usize * 3];
            for column in 0..width as usize {
                target[column * 3] = source_row[column * 4];
                target[column * 3 + 1] = source_row[column * 4 + 1];
                target[column * 3 + 2] = source_row[column * 4 + 2];
            }
        }
    }
    let handle = context
        .encode_image(&image, &mut encoder, None)
        .map_err(|err| FdxmpError::Heif(format!("HEIC 编码失败：{err}")))?;
    // 元数据（EXIF 负载为 TIFF 头起始、XMP 为裸包，与 WebP 写入口径一致）
    if let Some(exif) = exif.filter(|data| !data.is_empty()) {
        context
            .add_exif_metadata(&handle, exif)
            .map_err(|err| FdxmpError::Heif(format!("HEIC EXIF 写入失败：{err}")))?;
    }
    for segment in xmp_segments {
        const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
        let Some(at) = segment
            .windows(XMP_HEADER.len())
            .position(|window| window == XMP_HEADER)
        else {
            continue;
        };
        context
            .add_xmp_metadata(&handle, &segment[at + XMP_HEADER.len()..])
            .map_err(|err| FdxmpError::Heif(format!("HEIC XMP 写入失败：{err}")))?;
    }
    let path_text = path.to_string_lossy().to_string();
    context
        .write_to_file(&path_text)
        .map_err(|err| FdxmpError::Heif(format!("HEIC 写入失败：{err}")))?;
    Ok(())
}

#[cfg(not(feature = "heif"))]
fn export_heic(
    _rgba: &image::RgbaImage,
    _path: &Path,
    _quality: u8,
    _exif: Option<&[u8]>,
    _xmp_segments: &[Vec<u8>],
) -> Result<()> {
    Err(FdxmpError::HeifUnavailable(
        "构建时启用 feature `heif` 后可用".to_string(),
    ))
}

/// 叠加水印（文字 / 图片，右下角）。
pub fn apply_watermark(rgba: &image::RgbaImage, watermark: &Watermark) -> Result<image::RgbaImage> {
    match watermark {
        Watermark::None => Ok(rgba.clone()),
        Watermark::Text {
            text,
            opacity,
            size,
        } => {
            let mut output = rgba.clone();
            draw_text_watermark(&mut output, text, *opacity, *size);
            Ok(output)
        }
        Watermark::Image { path, opacity } => {
            let mark = image::open(path)
                .map_err(|err| FdxmpError::Unsupported(format!("水印图片读取失败：{err}")))?
                .to_rgba8();
            let mut output = rgba.clone();
            let scale =
                (output.width() as f32 * 0.18 / mark.width().max(1) as f32).clamp(0.02, 1.0);
            let mark_width = ((mark.width() as f32 * scale).round() as u32).max(1);
            let mark_height = ((mark.height() as f32 * scale).round() as u32).max(1);
            let mark = image::imageops::resize(
                &mark,
                mark_width,
                mark_height,
                image::imageops::FilterType::Triangle,
            );
            let alpha = (opacity / 100.0).clamp(0.0, 1.0);
            let offset_x = output.width().saturating_sub(mark_width + 24);
            let offset_y = output.height().saturating_sub(mark_height + 24);
            for (x, y, pixel) in mark.enumerate_pixels() {
                let target = (offset_x + x, offset_y + y);
                if target.0 >= output.width() || target.1 >= output.height() {
                    continue;
                }
                let base = output.get_pixel(target.0, target.1);
                let mark_alpha = pixel[3] as f32 / 255.0 * alpha;
                let blended = [
                    (base[0] as f32 * (1.0 - mark_alpha) + pixel[0] as f32 * mark_alpha) as u8,
                    (base[1] as f32 * (1.0 - mark_alpha) + pixel[1] as f32 * mark_alpha) as u8,
                    (base[2] as f32 * (1.0 - mark_alpha) + pixel[2] as f32 * mark_alpha) as u8,
                    255,
                ];
                output.put_pixel(target.0, target.1, image::Rgba(blended));
            }
            Ok(output)
        }
    }
}

/// 5×7 ASCII 点阵（仅支持可打印 ASCII；中文水印需要字体引擎，阶段 6 接入）。
const FONT_5X7: &[(char, [u8; 7])] = &[
    (' ', [0b00000, 0, 0, 0, 0, 0, 0]),
    ('!', [0b00100, 0b00100, 0b00100, 0b00100, 0, 0b00100, 0]),
    (
        '0',
        [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
    ),
    (
        '1',
        [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
    ),
    (
        '2',
        [
            0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
        ],
    ),
    (
        '3',
        [
            0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
        ],
    ),
    (
        '4',
        [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
    ),
    (
        '5',
        [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
    ),
    (
        '6',
        [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        '7',
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
    ),
    (
        '8',
        [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        '9',
        [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
    ),
    (
        'A',
        [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'B',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
    ),
    (
        'C',
        [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
    ),
    (
        'D',
        [
            0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100,
        ],
    ),
    (
        'E',
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
    ),
    (
        'F',
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        'G',
        [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
    ),
    (
        'H',
        [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'I',
        [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
    ),
    (
        'J',
        [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
    ),
    (
        'K',
        [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
    ),
    (
        'L',
        [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
    ),
    (
        'M',
        [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'N',
        [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'O',
        [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        'P',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        'Q',
        [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
    ),
    (
        'R',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
    ),
    (
        'S',
        [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
    ),
    (
        'T',
        [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        'U',
        [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        'V',
        [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
    ),
    (
        'W',
        [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
    ),
    (
        'X',
        [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
    ),
    (
        'Y',
        [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        'Z',
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
    ),
];

fn font_glyph(character: char) -> [u8; 7] {
    FONT_5X7
        .iter()
        .find(|(candidate, _)| *candidate == character.to_ascii_uppercase())
        .map(|(_, glyph)| *glyph)
        .unwrap_or([
            0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
        ])
}

/// 尝试加载系统字体（优先中文字体；找不到时返回 None）。
///
/// 说明：这里读取系统已装字体，避免把字体文件打包进仓库；
/// 发布版建议改为随包分发开源中文字体（如 Noto Sans SC）。
pub fn load_system_font() -> Option<ab_glyph::FontVec> {
    const CANDIDATES: &[(&str, u32)] = &[
        ("C:/Windows/Fonts/msyh.ttc", 0),
        ("C:/Windows/Fonts/msyhbd.ttc", 0),
        ("C:/Windows/Fonts/simhei.ttf", 0),
        ("C:/Windows/Fonts/simsun.ttc", 0),
        ("C:/Windows/Fonts/Deng.ttf", 0),
        ("C:/Windows/Fonts/arial.ttf", 0),
    ];
    for (path, index) in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = ab_glyph::FontVec::try_from_vec_and_index(bytes, *index) {
                return Some(font);
            }
        }
    }
    None
}

/// 用 TrueType 字体绘制文字水印（支持中文）。
fn draw_text_with_font(
    rgba: &mut image::RgbaImage,
    font: &ab_glyph::FontVec,
    text: &str,
    alpha: f32,
    size: u32,
) -> bool {
    use ab_glyph::{Font, PxScale, ScaleFont};

    let scale = PxScale::from(size.max(8) as f32);
    let scaled = font.as_scaled(scale);
    let mut width = 0.0f32;
    let mut previous: Option<ab_glyph::GlyphId> = None;
    for character in text.chars() {
        let id = font.glyph_id(character);
        if let Some(previous) = previous {
            width += scaled.kern(previous, id);
        }
        width += scaled.h_advance(id);
        previous = Some(id);
    }
    let text_width = width.ceil();
    let descent = scaled.descent();
    let margin = 24.0f32;
    let start_x = (rgba.width() as f32 - text_width - margin).max(0.0);
    let baseline = (rgba.height() as f32 - margin + descent.min(0.0)).max(0.0);

    let mut pen_x = start_x;
    let mut previous: Option<ab_glyph::GlyphId> = None;
    for character in text.chars() {
        let id = font.glyph_id(character);
        if let Some(previous) = previous {
            pen_x += scaled.kern(previous, id);
        }
        let glyph = id.with_scale_and_position(scale, ab_glyph::point(pen_x, baseline));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px < 0 || py < 0 || px >= rgba.width() as i32 || py >= rgba.height() as i32 {
                    return;
                }
                let blend = (coverage * alpha).clamp(0.0, 1.0);
                if blend <= 0.002 {
                    return;
                }
                let base = rgba.get_pixel(px as u32, py as u32);
                rgba.put_pixel(
                    px as u32,
                    py as u32,
                    image::Rgba([
                        (base[0] as f32 * (1.0 - blend) + 255.0 * blend) as u8,
                        (base[1] as f32 * (1.0 - blend) + 255.0 * blend) as u8,
                        (base[2] as f32 * (1.0 - blend) + 255.0 * blend) as u8,
                        255,
                    ]),
                );
            });
        }
        pen_x += scaled.h_advance(id);
        previous = Some(id);
    }
    true
}

/// 在右下角绘制文字水印（优先 TrueType 字体，支持中文；找不到字体时回退 5×7 点阵）。
pub fn draw_text_watermark(rgba: &mut image::RgbaImage, text: &str, opacity: f32, size: u32) {
    let alpha = (opacity / 100.0).clamp(0.0, 1.0);
    if alpha <= 0.0 || text.is_empty() {
        return;
    }
    if let Some(font) = load_system_font() {
        if draw_text_with_font(rgba, &font, text, alpha, size) {
            return;
        }
    }
    draw_text_watermark_bitmap(rgba, text, opacity, size);
}

/// 回退实现：5×7 ASCII 点阵（点阵放大 + 不透明度）。
fn draw_text_watermark_bitmap(rgba: &mut image::RgbaImage, text: &str, opacity: f32, size: u32) {
    let alpha = (opacity / 100.0).clamp(0.0, 1.0);
    if alpha <= 0.0 || text.is_empty() {
        return;
    }
    let scale = (size.max(8) / 7).max(1);
    let glyph_width = 6 * scale;
    let text_width = glyph_width * text.chars().count() as u32;
    let text_height = 7 * scale;
    let margin = 24;
    let start_x = rgba.width().saturating_sub(text_width + margin);
    let start_y = rgba.height().saturating_sub(text_height + margin);

    for (index, character) in text.chars().enumerate() {
        let glyph = font_glyph(character);
        let glyph_x = start_x + index as u32 * glyph_width;
        for (row, bits) in glyph.iter().enumerate() {
            for column in 0..5u32 {
                let on = bits & (1 << (4 - column)) != 0;
                if !on {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let x = glyph_x + column * scale + dx;
                        let y = start_y + row as u32 * scale + dy;
                        if x >= rgba.width() || y >= rgba.height() {
                            continue;
                        }
                        let base = rgba.get_pixel(x, y);
                        let blended = [
                            (base[0] as f32 * (1.0 - alpha) + 255.0 * alpha) as u8,
                            (base[1] as f32 * (1.0 - alpha) + 255.0 * alpha) as u8,
                            (base[2] as f32 * (1.0 - alpha) + 255.0 * alpha) as u8,
                            255,
                        ];
                        rgba.put_pixel(x, y, image::Rgba(blended));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fdxmp-export-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn gradient(width: u32, height: u32) -> image::RgbaImage {
        image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255])
        })
    }

    #[test]
    fn downscale_averages_in_linear_light() {
        // 4×1 黑白交替：线性光平均 ≈ 0.5（sRGB ≈ 188）；
        // 若在 sRGB 编码值上平均则得到 ≈ 128（gamma 错误）。
        let mut source = image::RgbaImage::new(4, 1);
        for (index, value) in [0u8, 255, 0, 255].iter().enumerate() {
            source.put_pixel(index as u32, 0, image::Rgba([*value, *value, *value, 255]));
        }
        let settings = ExportSettings {
            format: ExportFormat::Png,
            resize: ResizeSpec::Percent(50.0),
            output_dir: temp_dir("linear-resize").display().to_string(),
            ..ExportSettings::default()
        };
        let outcome = export_rgba(&source, "linear", &settings).unwrap();
        assert_eq!((outcome.width, outcome.height), (2, 1));
        let decoded = image::open(&outcome.path).unwrap().to_rgba8();
        let sample = decoded.get_pixel(0, 0).0[0];
        assert!(
            sample > 150,
            "线性光缩放应得到 ≈188 的灰，而不是 sRGB 平均的 ≈128：{sample}"
        );
        assert!(sample < 235, "线性光缩放结果过高：{sample}");
    }

    #[test]
    fn resize_modes_compute_expected_sizes() {
        assert_eq!(target_size(4000, 3000, ResizeSpec::Original), (4000, 3000));
        assert_eq!(
            target_size(4000, 3000, ResizeSpec::LongEdge(2000)),
            (2000, 1500)
        );
        assert_eq!(
            target_size(4000, 3000, ResizeSpec::ShortEdge(1500)),
            (2000, 1500)
        );
        assert_eq!(
            target_size(1000, 500, ResizeSpec::Percent(50.0)),
            (500, 250)
        );
    }

    #[test]
    fn exports_jpeg_png_tiff_webp() {
        let dir = temp_dir("formats");
        let image = gradient(320, 200);
        for format in [
            ExportFormat::Jpeg,
            ExportFormat::Png,
            ExportFormat::Tiff,
            ExportFormat::WebP,
        ] {
            let settings = ExportSettings {
                format,
                quality: 85,
                output_dir: dir.display().to_string(),
                ..ExportSettings::default()
            };
            let outcome =
                export_rgba(&image, &format!("test-{}", format.extension()), &settings).unwrap();
            assert!(outcome.path.is_file(), "{format:?} 应该生成文件");
            assert!(outcome.bytes > 0);
            let reopened = image::open(&outcome.path).unwrap();
            assert_eq!(reopened.width(), 320);
            assert_eq!(reopened.height(), 200);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_applies_resize_and_text_watermark() {
        let dir = temp_dir("watermark");
        let image = gradient(800, 600);
        let settings = ExportSettings {
            format: ExportFormat::Png,
            resize: ResizeSpec::LongEdge(400),
            watermark: Watermark::Text {
                text: "FDXMP 2026".to_string(),
                opacity: 70.0,
                size: 21,
            },
            output_dir: dir.display().to_string(),
            ..ExportSettings::default()
        };
        let outcome = export_rgba(&image, "watermarked", &settings).unwrap();
        assert_eq!((outcome.width, outcome.height), (400, 300));
        let reopened = image::open(&outcome.path).unwrap().to_rgba8();
        // 右下角区域应出现水印提亮的像素（不少于 20 个）。
        let mut bright = 0usize;
        for y in 240..300 {
            for x in 200..400 {
                if reopened.get_pixel(x, y)[0] > 200 {
                    bright += 1;
                }
            }
        }
        assert!(bright >= 20, "水印区域应被提亮（亮像素 {bright} 个）");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch_export_reports_each_file() {
        let dir = temp_dir("batch");
        let images = vec![
            ("a".to_string(), gradient(64, 64)),
            ("b".to_string(), gradient(64, 64)),
        ];
        let settings = ExportSettings {
            format: ExportFormat::Png,
            output_dir: dir.display().to_string(),
            ..ExportSettings::default()
        };
        let outcomes = export_batch(&images, &settings).unwrap();
        assert_eq!(outcomes.len(), 2);
        assert!(outcomes.iter().all(|outcome| outcome.path.is_file()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_watermark_renders_cjk_when_font_available() {
        let mut image = image::RgbaImage::new(240, 120);
        for pixel in image.pixels_mut() {
            *pixel = image::Rgba([0, 0, 0, 255]);
        }
        draw_text_watermark(&mut image, "中文测试水印", 100.0, 28);
        // 不 panic、不改变尺寸；有系统字体时中文应真的渲染出来。
        assert_eq!(image.width(), 240);
        let bright = image.pixels().filter(|pixel| pixel.0[0] > 180).count();
        if load_system_font().is_some() {
            assert!(
                bright > 50,
                "系统字体可用时应渲染中文水印（亮像素 {bright}）"
            );
        }
    }

    #[cfg(feature = "heif")]
    #[test]
    fn exports_heic_and_decodes_back() {
        let dir = temp_dir("heic");
        let image = gradient(320, 200);
        // 带 EXIF 的源文件 + 保留元数据：验证 HEIC 容器写入元数据后仍可解码
        let source = dir.join("source.jpg");
        let exif_payload: Vec<u8> = b"II*\0\x08\0\0\0\x01\0\x0f\x01\x02\0\x04\0\0\0Test".to_vec();
        let mut source_bytes = vec![0xFF, 0xD8];
        let length = exif_payload.len() + 8;
        source_bytes.extend_from_slice(&[0xFF, 0xE1]);
        source_bytes.extend_from_slice(&(length as u16).to_be_bytes());
        source_bytes.extend_from_slice(b"Exif\0\0");
        source_bytes.extend_from_slice(&exif_payload);
        source_bytes.extend_from_slice(&[0xFF, 0xD9]);
        std::fs::write(&source, &source_bytes).unwrap();
        let settings = ExportSettings {
            format: ExportFormat::Heic,
            quality: 80,
            metadata: MetadataPolicy::Keep,
            output_dir: dir.display().to_string(),
            ..ExportSettings::default()
        };
        let outcome =
            export_rgba_with_source(&image, "heic-out", &settings, Some(&source)).unwrap();
        assert!(outcome.path.is_file(), "应生成 .heic 文件");
        let decoded = crate::decode::decode(&outcome.path).unwrap();
        assert_eq!((decoded.width, decoded.height), (320, 200));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keep_metadata_writes_exif_to_jpeg_and_png() {
        let dir = temp_dir("metadata");
        let source = dir.join("source.jpg");
        // 构造带 EXIF APP1 的源 JPEG（SOI + APP1 + EOI）。
        let exif_payload: Vec<u8> = b"II*\0\x08\0\0\0\x01\0\x0f\x01\x02\0\x04\0\0\0Test".to_vec();
        let mut source_bytes = vec![0xFF, 0xD8];
        let length = exif_payload.len() + 8;
        source_bytes.extend_from_slice(&[0xFF, 0xE1]);
        source_bytes.extend_from_slice(&(length as u16).to_be_bytes());
        source_bytes.extend_from_slice(b"Exif\0\0");
        source_bytes.extend_from_slice(&exif_payload);
        source_bytes.extend_from_slice(&[0xFF, 0xD9]);
        std::fs::write(&source, &source_bytes).unwrap();

        let image = gradient(48, 32);
        let mut settings = ExportSettings {
            metadata: MetadataPolicy::Keep,
            output_dir: dir.display().to_string(),
            ..ExportSettings::default()
        };
        let outcome =
            export_rgba_with_source(&image, "with-exif", &settings, Some(&source)).unwrap();
        let jpeg = std::fs::read(&outcome.path).unwrap();
        assert!(
            jpeg.windows(6).any(|window| window == b"Exif\0\0"),
            "JPEG 输出应包含 EXIF APP1 段"
        );
        // EXIF 段应紧跟 SOI。
        assert_eq!(&jpeg[2..4], &[0xFF, 0xE1]);

        // PNG 输出：写入 eXIf 块且 CRLF 之前的 IHDR 仍在最前。
        settings.format = ExportFormat::Png;
        let outcome =
            export_rgba_with_source(&image, "with-exif-png", &settings, Some(&source)).unwrap();
        let png = std::fs::read(&outcome.path).unwrap();
        assert!(
            png.windows(4).any(|window| window == b"eXIf"),
            "PNG 输出应包含 eXIf 块"
        );
        // eXIf 必须出现在 IDAT 之前（PNG 规范要求）。
        let exif_at = png.windows(4).position(|w| w == b"eXIf").unwrap();
        let idat_at = png.windows(4).position(|w| w == b"IDAT").unwrap();
        assert!(exif_at < idat_at, "eXIf 块应位于 IDAT 之前");

        // Strip 策略下不应写入元数据。
        settings.format = ExportFormat::Jpeg;
        settings.metadata = MetadataPolicy::Strip;
        let outcome =
            export_rgba_with_source(&image, "stripped", &settings, Some(&source)).unwrap();
        let stripped = std::fs::read(&outcome.path).unwrap();
        assert!(
            !stripped.windows(6).any(|window| window == b"Exif\0\0"),
            "Strip 策略下输出不应包含 EXIF"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exports_embed_icc_profile_for_non_srgb() {
        let dir = temp_dir("icc");
        let image = gradient(64, 48);
        let mut settings = ExportSettings {
            format: ExportFormat::Jpeg,
            color_space: ColorSpace::DisplayP3,
            output_dir: dir.display().to_string(),
            ..ExportSettings::default()
        };
        let outcome = export_rgba(&image, "p3", &settings).unwrap();
        let jpeg = std::fs::read(&outcome.path).unwrap();
        assert!(
            jpeg.windows(12).any(|window| window == b"ICC_PROFILE\0"),
            "JPEG 输出应包含 ICC_PROFILE APP2 段"
        );
        assert!(
            jpeg.windows(4).any(|window| window == b"acsp"),
            "ICC 配置文件应包含 acsp 签名"
        );

        settings.format = ExportFormat::Png;
        settings.color_space = ColorSpace::AdobeRgb;
        let outcome = export_rgba(&image, "adobergb", &settings).unwrap();
        let png = std::fs::read(&outcome.path).unwrap();
        assert!(
            png.windows(4).any(|window| window == b"iCCP"),
            "PNG 输出应包含 iCCP 块"
        );
        let iccp_at = png.windows(4).position(|w| w == b"iCCP").unwrap();
        let idat_at = png.windows(4).position(|w| w == b"IDAT").unwrap();
        assert!(iccp_at < idat_at, "iCCP 块应位于 IDAT 之前");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn webp_export_embeds_icc_and_stays_decodable() {
        use image::ImageDecoder;

        let dir = temp_dir("webp-icc");
        let image = gradient(32, 24);
        // 源 JPEG 带 EXIF Orientation=6，验证 WebP 也能写回元数据
        let source_path = dir.join("source.jpg");
        image::DynamicImage::ImageRgba8(image.clone())
            .to_rgb8()
            .save(&source_path)
            .unwrap();
        insert_test_exif_orientation(&source_path, 6);

        let settings = ExportSettings {
            format: ExportFormat::WebP,
            color_space: ColorSpace::DisplayP3,
            metadata: MetadataPolicy::Keep,
            output_dir: dir.display().to_string(),
            ..ExportSettings::default()
        };
        let outcome =
            export_rgba_with_source(&image, "p3-webp", &settings, Some(&source_path)).unwrap();
        let bytes = std::fs::read(&outcome.path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
        assert_eq!(&bytes[12..16], b"VP8X", "应转换为扩展格式");
        assert!(
            bytes.windows(4).any(|window| window == b"ICCP"),
            "WebP 输出应包含 ICCP 块"
        );
        assert!(
            bytes.windows(4).any(|window| window == b"EXIF"),
            "WebP 输出应包含 EXIF 块"
        );
        // 用解码器独立验证：ICC / EXIF 方向能读回、图像仍可正常解码
        let reader = image::ImageReader::open(&outcome.path)
            .unwrap()
            .with_guessed_format()
            .unwrap();
        let mut decoder = reader.into_decoder().unwrap();
        let icc = decoder.icc_profile().unwrap().expect("应能读回 ICC");
        assert_eq!(&icc[36..40], b"acsp", "ICC 配置无效");
        assert_eq!(
            decoder.orientation().unwrap(),
            image::metadata::Orientation::Rotate90,
            "EXIF 方向应能读回"
        );
        let decoded = image::open(&outcome.path).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (32, 24));
        assert_eq!(
            outcome.bytes,
            std::fs::metadata(&outcome.path).unwrap().len()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 在 JPEG 的 SOI 之后插入最小 EXIF APP1（只含 Orientation 标签）。
    fn insert_test_exif_orientation(path: &Path, orientation: u16) {
        let jpeg = std::fs::read(path).unwrap();
        assert_eq!(&jpeg[0..2], &[0xFF, 0xD8], "应为 JPEG");
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II\x2A\x00");
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&orientation.to_le_bytes());
        tiff.extend_from_slice(&0u16.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());

        let mut payload = Vec::new();
        payload.extend_from_slice(b"Exif\x00\x00");
        payload.extend_from_slice(&tiff);
        let length = (payload.len() + 2) as u16;
        let mut segment = vec![0xFF, 0xE1];
        segment.extend_from_slice(&length.to_be_bytes());
        segment.extend_from_slice(&payload);

        let mut output = Vec::with_capacity(jpeg.len() + segment.len());
        output.extend_from_slice(&jpeg[0..2]);
        output.extend_from_slice(&segment);
        output.extend_from_slice(&jpeg[2..]);
        std::fs::write(path, output).unwrap();
    }
}
