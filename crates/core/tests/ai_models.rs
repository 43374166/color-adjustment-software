//! Windows AI 蒙版集成测试：使用真实 ONNX 模型跑推理（模型缺失时跳过）。
//!
//! 运行前准备（仓库内已就绪）：`dist/windows/models/*.onnx`（hf-mirror 下载）
//! 与 `.tmp/tools/onnxruntime/onnxruntime.dll`；也可用环境变量
//! `FDXMP_MODEL_DIR` / `FDXMP_ORT_DLL` 指定。

#![cfg(feature = "ai")]

use std::path::PathBuf;
use std::time::Instant;

use fdxmp_core::params::{MaskKind, MaskParams, PeoplePart};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// 找到模型目录（仓库 dist 或环境变量）；缺失时返回 None。
fn prepare() -> Option<PathBuf> {
    let models = std::env::var_os("FDXMP_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("dist/windows/models"));
    if !models.join("u2netp.onnx").is_file()
        || !models.join("face_parsing_quantized.onnx").is_file()
        || !models.join("segformer_ade_b0.onnx").is_file()
    {
        eprintln!("跳过 AI 测试：缺少模型（{}）", models.display());
        return None;
    }
    std::env::set_var("FDXMP_MODEL_DIR", &models);
    // 集成测试走轻量主体模型（BiRefNet-lite 另见下方 ignore 测试）。
    std::env::set_var("FDXMP_AI_SUBJECT", "lite");
    if std::env::var_os("FDXMP_ORT_DLL").is_none() {
        let dll = repo_root().join(".tmp/tools/onnxruntime/onnxruntime.dll");
        if dll.is_file() {
            std::env::set_var("FDXMP_ORT_DLL", dll);
        }
    }
    Some(models)
}

/// 造一张「上蓝下绿 + 中间肤色椭圆」的合成图，用于跑通三类模型。
fn synthetic_photo(width: u32, height: u32) -> image::RgbaImage {
    let mut image = image::RgbaImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let vertical = y as f32 / height as f32;
        let (r, g, b) = if vertical < 0.45 {
            (90.0, 150.0, 235.0) // 天空
        } else {
            (70.0, 110.0, 60.0) // 草地
        };
        // 脸部椭圆（BGR 肤色）
        let cx = width as f32 * 0.5;
        let cy = height as f32 * 0.62;
        let dx = (x as f32 - cx) / (width as f32 * 0.16);
        let dy = (y as f32 - cy) / (height as f32 * 0.26);
        let inside_face = dx * dx + dy * dy < 1.0;
        let (r, g, b) = if inside_face {
            (225.0, 180.0, 150.0)
        } else {
            (r, g, b)
        };
        *pixel = image::Rgba([r as u8, g as u8, b as u8, 255]);
    }
    image
}

fn mask(kind: MaskKind) -> MaskParams {
    MaskParams {
        id: "ai-test".to_string(),
        name: "AI 测试".to_string(),
        kind,
        ..MaskParams::default()
    }
}

#[test]
fn ai_models_run_inference_on_windows() {
    let Some(_models) = prepare() else {
        return;
    };
    let image = synthetic_photo(256, 192);

    let cases: [(&str, MaskKind); 5] = [
        ("主体", MaskKind::AiSubject),
        ("背景", MaskKind::AiBackground),
        ("天空", MaskKind::AiSky),
        (
            "人物-面部",
            MaskKind::AiPeople {
                part: PeoplePart::Face,
            },
        ),
        ("物体（点按）", MaskKind::AiObject { point: [0.5, 0.62] }),
    ];

    for (label, kind) in cases {
        let started = Instant::now();
        let raster = fdxmp_core::ai::generate(&mask(kind), &image)
            .unwrap_or_else(|err| panic!("{label} 推理失败：{err}"));
        let elapsed = started.elapsed().as_secs_f32() * 1000.0;
        assert_eq!(
            (raster.width, raster.height),
            (image.width(), image.height()),
            "{label} 输出尺寸应与图像一致"
        );
        assert_eq!(
            raster.weights.len(),
            (image.width() * image.height()) as usize,
            "{label} 权重长度应匹配"
        );
        let mean = raster.mean();
        assert!(
            mean.is_finite() && (0.0..=1.0).contains(&mean),
            "{label} 平均权重异常：{mean}"
        );
        eprintln!("AI 蒙版 {label}：{elapsed:.0} ms，平均权重 {mean:.3}");
    }
}

/// 高质量主体模型（BiRefNet-lite）：默认不跑（CPU 慢、内存高），
/// 需要时执行 `cargo test -p fdxmp-core --features ai --test ai_models -- --ignored`。
#[test]
#[ignore = "BiRefNet-lite 在 CPU 上较慢且内存占用较高"]
fn ai_quality_subject_model_runs() {
    let Some(models) = prepare() else {
        return;
    };
    if !models.join("birefnet_lite.onnx").is_file() {
        eprintln!("跳过：未下载 birefnet_lite.onnx");
        return;
    }
    std::env::set_var("FDXMP_AI_SUBJECT", "hq");
    let image = synthetic_photo(256, 192);
    let started = Instant::now();
    let raster = fdxmp_core::ai::generate(&mask(MaskKind::AiSubject), &image).unwrap();
    eprintln!(
        "高质量主体模型（BiRefNet-lite）：{:.0} ms，平均权重 {:.3}",
        started.elapsed().as_secs_f32() * 1000.0,
        raster.mean()
    );
}
