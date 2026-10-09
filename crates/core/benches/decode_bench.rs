//! 阶段 1 解码基准：24MP 常见格式 + RAW 首屏 / 全尺寸（RAW 需要样张，缺失时跳过）。

use std::path::PathBuf;
#[cfg(feature = "raw")]
use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion};

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join("fdxmp-decode-bench")
}

/// 生成 6000×4000（24MP）JPEG 作为常见格式基准样本。
fn ensure_common_sample() -> PathBuf {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sample-24mp.jpg");
    if !path.is_file() {
        let image = image::RgbImage::from_fn(6000, 4000, |x, y| {
            image::Rgb([
                (x % 256) as u8,
                (y % 256) as u8,
                ((x / 4 + y / 4) % 256) as u8,
            ])
        });
        image.save(&path).unwrap();
    }
    path
}

#[cfg(feature = "raw")]
fn raw_sample() -> Option<PathBuf> {
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

fn bench_decode_common(c: &mut Criterion) {
    let path = ensure_common_sample();
    c.bench_function("decode_24mp_jpeg", |b| {
        b.iter(|| fdxmp_core::decode::decode(&path).unwrap())
    });
    c.bench_function("decode_24mp_jpeg_preview", |b| {
        b.iter(|| fdxmp_core::decode::decode_preview(&path).unwrap())
    });
}

#[cfg(feature = "raw")]
fn bench_decode_raw(c: &mut Criterion) {
    let Some(path) = raw_sample() else {
        eprintln!("跳过 RAW 基准：未找到样张（可设置 FDXMP_TEST_RAW）");
        return;
    };
    let mut group = c.benchmark_group("raw_45mp");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(20));
    group.bench_function("embedded_preview", |b| {
        b.iter(|| fdxmp_core::decode::decode_preview(&path).unwrap())
    });
    group.bench_function("full_demosaic_16bit", |b| {
        b.iter(|| fdxmp_core::decode::decode(&path).unwrap())
    });
    group.finish();
}

#[cfg(not(feature = "raw"))]
fn bench_decode_raw(_c: &mut Criterion) {}

criterion_group!(benches, bench_decode_common, bench_decode_raw);
criterion_main!(benches);
