//! 阶段 1 解码集成测试：常见格式 + RAW（需要样张，缺失时跳过）。

use std::path::PathBuf;

use fdxmp_core::decode::{decode, DecodeSource};
#[cfg(feature = "raw")]
use fdxmp_core::decode::{decode_preview, decode_with, DecodeOptions};

fn temp_png() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fdxmp-decode-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sample.png");
    let image = image::RgbImage::from_fn(320, 200, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });
    image.save(&path).unwrap();
    path
}

#[test]
fn decodes_common_format_with_metadata_free_result() {
    let path = temp_png();
    let decoded = decode(&path).unwrap();
    assert_eq!(decoded.width, 320);
    assert_eq!(decoded.height, 200);
    assert_eq!(decoded.source, DecodeSource::CommonFormat);
    assert!(decoded.metadata.is_none());
    assert!(decoded.elapsed_ms < 5_000);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn missing_file_reports_io_error() {
    let err = decode("不存在的文件.cr3").unwrap_err();
    assert!(matches!(err, fdxmp_core::error::FdxmpError::Io(_)));
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fdxmp-decode-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 在 JPEG 的 SOI 之后插入最小 EXIF APP1（只含 Orientation 标签）。
fn insert_exif_orientation(path: &std::path::Path, orientation: u16) {
    let mut jpeg = std::fs::read(path).unwrap();
    assert_eq!(&jpeg[0..2], &[0xFF, 0xD8], "应为 JPEG");
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II\x2A\x00"); // little-endian
    tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 偏移
    tiff.extend_from_slice(&1u16.to_le_bytes()); // 条目数
    tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
    tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    tiff.extend_from_slice(&1u32.to_le_bytes()); // count
    tiff.extend_from_slice(&orientation.to_le_bytes()); // 值
    tiff.extend_from_slice(&0u16.to_le_bytes()); // 值域填充
    tiff.extend_from_slice(&0u32.to_le_bytes()); // 下一个 IFD

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
    jpeg = output;
    std::fs::write(path, jpeg).unwrap();
}

#[test]
fn common_format_applies_exif_orientation() {
    let dir = temp_dir("orientation");
    let path = dir.join("rotated.jpg");
    // 8×4：左半白、右半黑；Orientation=6（顺时针 90°）后应变成 4×8、上半白。
    let source = image::RgbImage::from_fn(8, 4, |x, _| {
        if x < 4 {
            image::Rgb([255, 255, 255])
        } else {
            image::Rgb([0, 0, 0])
        }
    });
    source.save(&path).unwrap();
    insert_exif_orientation(&path, 6);

    let decoded = decode(&path).unwrap();
    assert_eq!(
        (decoded.width, decoded.height),
        (4, 8),
        "EXIF 方向应交换宽高"
    );
    let rgba = decoded.pixels.to_rgba8();
    assert!(rgba.get_pixel(0, 1).0[0] > 200, "旋转后上半应为白");
    assert!(rgba.get_pixel(0, 6).0[0] < 60, "旋转后下半应为黑");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn common_format_applies_embedded_icc() {
    use fdxmp_core::export::{export_rgba, ColorSpace, ExportFormat, ExportSettings};

    let dir = temp_dir("input-icc");
    let mut source = image::RgbaImage::new(3, 1);
    source.put_pixel(0, 0, image::Rgba([128, 128, 128, 255]));
    source.put_pixel(1, 0, image::Rgba([200, 60, 40, 255]));
    source.put_pixel(2, 0, image::Rgba([30, 90, 180, 255]));

    // 写出一张"Adobe RGB + ICC"的 JPEG，再解码：应自动转回 sRGB。
    let settings = ExportSettings {
        format: ExportFormat::Jpeg,
        quality: 100,
        color_space: ColorSpace::AdobeRgb,
        output_dir: dir.display().to_string(),
        ..ExportSettings::default()
    };
    let outcome = export_rgba(&source, "adobe", &settings).unwrap();
    let decoded = decode(&outcome.path).unwrap();
    let rgba = decoded.pixels.to_rgba8();
    for (index, original) in source.pixels().enumerate() {
        let back = rgba.get_pixel(index as u32, 0);
        for channel in 0..3 {
            let diff = original.0[channel] as i32 - back.0[channel] as i32;
            assert!(
                diff.abs() <= 6,
                "带 ICC 的输入应转换回 sRGB：{original:?} → {back:?}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "raw")]
fn sample_raw() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FDXMP_TEST_RAW") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let candidate = std::path::Path::new(&home)
        .join(".cache")
        .join("fdxmp-tools")
        .join("samples")
        .join("test-z8.NEF");
    candidate.is_file().then_some(candidate)
}

#[cfg(feature = "raw")]
#[test]
fn decodes_raw_preview_and_full() {
    let Some(path) = sample_raw() else {
        eprintln!(
            "跳过 RAW 集成测试：未找到样张。可设置 FDXMP_TEST_RAW，或放到 \
             %USERPROFILE%\\.cache\\fdxmp-tools\\samples\\test-z8.NEF"
        );
        return;
    };

    let preview = decode_preview(&path).unwrap();
    assert!(preview.width > 0 && preview.height > 0);
    assert!(preview.metadata.is_some());
    assert!(
        matches!(
            preview.source,
            DecodeSource::EmbeddedPreview | DecodeSource::HalfRaw
        ),
        "首屏路径应命中内嵌预览或半尺寸，实际：{:?}",
        preview.source
    );

    let full = decode(&path).unwrap();
    assert_eq!(full.source, DecodeSource::FullRaw);
    assert!(full.width >= preview.width);
    assert!(full.height >= preview.height);

    let metadata = full.metadata.as_ref().unwrap();
    eprintln!(
        "RAW 元数据：{} | 首屏 {}x{}（{}，{}ms）| 全尺寸 {}x{}（{}ms，{:.1}MP）",
        metadata.summary(),
        preview.width,
        preview.height,
        preview.source.label_zh(),
        preview.elapsed_ms,
        full.width,
        full.height,
        full.elapsed_ms,
        full.megapixels()
    );
}

#[cfg(feature = "raw")]
#[test]
fn raw_custom_options_are_honoured() {
    let Some(path) = sample_raw() else {
        return;
    };
    let decoded = decode_with(
        &path,
        &DecodeOptions {
            use_embedded_preview: false,
            half_size: true,
            camera_white_balance: false,
        },
    )
    .unwrap();
    assert_eq!(decoded.source, DecodeSource::HalfRaw);
}

/// DNG 样张（rsraw / LibRaw 内置 DNG 解码器；iPhone ProRAW 也走这条路径）。
#[cfg(feature = "raw")]
fn sample_dng() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FDXMP_TEST_DNG") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let candidate = std::path::Path::new(&home)
        .join(".cache")
        .join("fdxmp-tools")
        .join("samples")
        .join("exiftool-DNG.dng");
    candidate.is_file().then_some(candidate)
}

#[cfg(feature = "raw")]
#[test]
fn decodes_dng_via_libraw() {
    let Some(path) = sample_dng() else {
        eprintln!(
            "跳过 DNG 集成测试：未找到样张。可设置 FDXMP_TEST_DNG，或放到 \
             %USERPROFILE%\\.cache\\fdxmp-tools\\samples\\exiftool-DNG.dng"
        );
        return;
    };
    let decoded = decode(&path).unwrap();
    assert!(decoded.width > 1000 && decoded.height > 1000);
    assert!(decoded.metadata.is_some());
    eprintln!(
        "DNG 解码：{}x{}（{}ms）| {}",
        decoded.width,
        decoded.height,
        decoded.elapsed_ms,
        decoded.metadata.as_ref().unwrap().summary()
    );
}

#[cfg(feature = "raw")]
#[test]
fn decode_raw_rgba_from_bytes_decodes_and_scales() {
    // 完整样张优先（NEF 带真实图像数据；DNG 样张可能只有元数据）。
    let (bytes, complete) = if let Some(path) = sample_raw() {
        (std::fs::read(&path).unwrap(), true)
    } else if let Some(path) = sample_dng() {
        (std::fs::read(&path).unwrap(), false)
    } else {
        eprintln!("跳过 RAW 内存解码测试：未找到样张（FDXMP_TEST_RAW / FDXMP_TEST_DNG）");
        return;
    };
    let preview = fdxmp_core::preview::decode_raw_rgba(&bytes, 1024, false).unwrap();
    let longest = preview.width.max(preview.height);
    assert!(
        (512..=1024).contains(&longest),
        "长边应按 max_edge 缩放：{longest}"
    );
    assert_eq!(
        preview.rgba.len(),
        (preview.width as usize) * (preview.height as usize) * 4
    );
    // 完整样张：确认真实解调出像素而不是纯色占位。
    if complete {
        let first = &preview.rgba[..4];
        assert!(
            preview
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel.as_slice() != first),
            "解码结果不应为纯色"
        );
    }
}

#[cfg(not(feature = "raw"))]
#[test]
fn raw_without_feature_reports_hint() {
    let dir = std::env::temp_dir().join(format!("fdxmp-raw-hint-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sample.nef");
    std::fs::write(&path, b"II*\0placeholder raw payload").unwrap();

    let err = decode(&path).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("raw"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "heif")]
fn sample_heic() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FDXMP_TEST_HEIC") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let candidate = std::path::Path::new(&home)
        .join(".cache")
        .join("fdxmp-tools")
        .join("samples")
        .join("heic-test.heic");
    candidate.is_file().then_some(candidate)
}

#[cfg(feature = "heif")]
#[test]
fn decodes_heic_sample() {
    let Some(path) = sample_heic() else {
        eprintln!(
            "跳过 HEIC 集成测试：未找到样张。可设置 FDXMP_TEST_HEIC，或放到 \
             %USERPROFILE%\\.cache\\fdxmp-tools\\samples\\heic-test.heic"
        );
        return;
    };

    let probe = fdxmp_core::decode::probe(&path).unwrap();
    assert_eq!(probe.format, Some(fdxmp_core::decode::ImageFormat::Heif));
    assert!(probe.width > 0 && probe.height > 0, "HEIF 容器应给出尺寸");

    let decoded = decode(&path).unwrap();
    assert_eq!(decoded.format, fdxmp_core::decode::ImageFormat::Heif);
    assert_eq!(decoded.width, probe.width);
    assert_eq!(decoded.height, probe.height);
    assert!(decoded.elapsed_ms < 10_000);
    eprintln!(
        "HEIC：{}×{}，{}ms",
        decoded.width, decoded.height, decoded.elapsed_ms
    );
}
