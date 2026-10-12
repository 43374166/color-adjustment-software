//! 智能蒙版真实模型集成测试（需要 `ai` feature 与本地模型文件）。
//!
//! 运行：`cargo test -p fdxmp-core --features ai --test smartmask_models -- --nocapture`
//!
//! 模型目录：`FDXMP_MODEL_DIR`，未设置时尝试 `dist/windows/models`；
//! 模型缺失时自动跳过（CI 无模型也能通过）。

#![cfg(feature = "ai")]

use std::path::PathBuf;

use fdxmp_core::smartmask::onnx::OnnxStages;
use fdxmp_core::smartmask::spec::ModelRegistry;
use fdxmp_core::smartmask::{MaskPipeline, PipelineConfig};

/// 定位模型目录（环境变量 → dist/windows/models）。
fn model_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("FDXMP_MODEL_DIR") {
        let dir = PathBuf::from(dir);
        if dir.is_dir() {
            return Some(dir);
        }
    }
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from("dist/windows/models"));
    candidates.push(PathBuf::from("../../dist/windows/models"));
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("dist/windows/models"));
        if let Some(parent) = cwd.parent() {
            candidates.push(parent.join("dist/windows/models"));
        }
    }
    candidates.into_iter().find(|dir| dir.is_dir())
}

fn ensure_models(registry: &ModelRegistry) -> bool {
    for spec in registry.specs() {
        if !registry.path_of(spec).is_file() {
            eprintln!("跳过：缺少模型 {}", spec.path);
            return false;
        }
    }
    // ONNX Runtime：优先 FDXMP_ORT_DLL，其次模型目录同级 / 上级的 onnxruntime.dll。
    if std::env::var_os("FDXMP_ORT_DLL").is_none() {
        let candidates = [
            registry.dir().join("onnxruntime.dll"),
            registry.dir().join("../onnxruntime.dll"),
        ];
        match candidates.iter().find(|path| path.is_file()) {
            Some(path) => std::env::set_var("FDXMP_ORT_DLL", path),
            None => {
                eprintln!("跳过：未找到 onnxruntime.dll");
                return false;
            }
        }
    }
    true
}

/// 1×1 白色测试图（真实模型对内容不敏感，只验证形状 / 解码路径）。
fn white_pixel() -> image::RgbaImage {
    image::RgbaImage::from_pixel(64, 48, image::Rgba([255, 255, 255, 255]))
}

#[test]
fn onnx_stages_load_verify_and_run_all_four_models() {
    let Some(dir) = model_dir() else {
        eprintln!("跳过：未找到模型目录");
        return;
    };
    let registry = ModelRegistry::with_dir(dir);
    if !ensure_models(&registry) {
        return;
    }
    // 哈希校验：四个模型都必须通过（拒绝篡改）。
    for spec in registry.specs() {
        let status = registry.verify(spec);
        assert_eq!(status.hash_state, "ok", "{} 哈希校验失败", spec.name);
    }

    let stages = OnnxStages::load(&registry).expect("加载 ONNX 模型失败");
    let pipeline = MaskPipeline::new(
        stages.detector(),
        stages.segmenter(),
        Some(stages.parser()),
        Some(stages.matting()),
        None,
        PipelineConfig::default(),
    );
    // 白色空图：不要求检出人脸，但必须跑通全流程且不报错。
    let image = white_pixel();
    let set = pipeline.run(&image).expect("流水线执行失败");
    eprintln!(
        "真实模型流水线：faces={} masks={:?} versions={}",
        set.faces.len(),
        set.masks.iter().map(|mask| mask.kind).collect::<Vec<_>>(),
        set.versions
    );
    assert!(
        set.contains(fdxmp_core::smartmask::SmartMaskKind::Portrait)
            || set.contains(fdxmp_core::smartmask::SmartMaskKind::Subject)
    );
    // 主体 / 人像蒙版必须与原图同尺寸、权重合法。
    for mask in &set.masks {
        assert_eq!((mask.width, mask.height), (64, 48));
        assert!(mask.data.iter().all(|value| (0.0..=1.0).contains(value)));
    }
}

#[test]
fn onnx_face_detector_finds_synthetic_face() {
    let Some(dir) = model_dir() else {
        eprintln!("跳过：未找到模型目录");
        return;
    };
    let registry = ModelRegistry::with_dir(dir);
    if !ensure_models(&registry) {
        return;
    }
    // 合成一张「类人脸」图（肤色椭圆 + 双眼 + 嘴），不强求检出；
    // 只验证解码输出结构合理（分数在 0..1、框在图像范围内）。
    let mut image = image::RgbaImage::from_pixel(640, 480, image::Rgba([40, 40, 40, 255]));
    for y in 90..390 {
        for x in 170..470 {
            let dx = (x as f32 - 320.0) / 150.0;
            let dy = (y as f32 - 240.0) / 150.0;
            if dx * dx + dy * dy <= 1.0 {
                image.put_pixel(x, y, image::Rgba([225, 190, 170, 255]));
            }
        }
    }
    for y in 200..230 {
        for x in 230..290 {
            image.put_pixel(x, y, image::Rgba([50, 50, 60, 255]));
        }
        for x in 350..410 {
            image.put_pixel(x, y, image::Rgba([50, 50, 60, 255]));
        }
    }
    for y in 310..330 {
        for x in 280..360 {
            image.put_pixel(x, y, image::Rgba([160, 60, 60, 255]));
        }
    }
    let stages = OnnxStages::load(&registry).expect("加载 ONNX 模型失败");
    let faces = stages.detector().detect(&image).expect("人脸检测失败");
    for face in &faces {
        assert!((0.0..=1.0).contains(&face.confidence));
        assert!(face.bbox[0] >= 0.0 && face.bbox[2] <= 1.0);
        assert!(face.bbox[1] >= 0.0 && face.bbox[3] <= 1.0);
        assert!(face.bbox[2] > face.bbox[0] && face.bbox[3] > face.bbox[1]);
    }
    eprintln!("合成人脸检出：{} 张", faces.len());
}

/// 诊断：打印四个模型的输入 / 输出名称与形状（换模型时用它对齐 `ModelSpec`）。
///
/// 运行：`cargo test -p fdxmp-core --features ai --test smartmask_models -- --ignored --nocapture dump_model_io`
#[test]
#[ignore]
fn dump_model_io() {
    use ort::session::Session;

    let Some(dir) = model_dir() else {
        eprintln!("跳过：未找到模型目录");
        return;
    };
    let registry = ModelRegistry::with_dir(dir);
    if !ensure_models(&registry) {
        return;
    }
    fdxmp_core::ai::ensure_runtime().expect("ONNX Runtime 初始化失败");
    for spec in registry.specs() {
        let path = registry.path_of(spec);
        let mut builder = Session::builder().unwrap();
        let session = builder.commit_from_file(&path).unwrap();
        eprintln!("== {}（{}）", spec.name, spec.path);
        for input in session.inputs() {
            eprintln!("  in  {}", input.name());
        }
        for output in session.outputs() {
            eprintln!("  out {}", output.name());
        }
    }
}

/// 真实人像端到端：检测 → 对齐 → 五官解析 → 精细抠图。
///
/// 需要 `FDXMP_TEST_PORTRAIT=<照片路径>`（真实人像照片）；未设置时跳过。
/// 运行：
/// `FDXMP_TEST_PORTRAIT=xxx.jpg cargo test -p fdxmp-core --features ai --test smartmask_models -- --nocapture onnx_face_path`
#[test]
fn onnx_face_path_on_real_portrait() {
    let Some(portrait_env) = std::env::var_os("FDXMP_TEST_PORTRAIT") else {
        eprintln!("跳过：未设置 FDXMP_TEST_PORTRAIT");
        return;
    };
    let Some(dir) = model_dir() else {
        eprintln!("跳过：未找到模型目录");
        return;
    };
    let registry = ModelRegistry::with_dir(dir);
    if !ensure_models(&registry) {
        return;
    }
    let image = image::open(std::path::PathBuf::from(portrait_env))
        .expect("打开测试人像失败")
        .to_rgba8();
    // 缩到 1024 长边，模拟实际预览尺寸。
    let scale = 1024.0 / image.width().max(image.height()) as f32;
    let image = if scale < 1.0 {
        image::imageops::resize(
            &image,
            (image.width() as f32 * scale) as u32,
            (image.height() as f32 * scale) as u32,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };
    let stages = OnnxStages::load(&registry).expect("加载 ONNX 模型失败");

    // 1) 人脸检测（关键点必须齐）。
    let faces = stages.detector().detect(&image).expect("人脸检测失败");
    eprintln!(
        "检测到 {} 张人脸；最高分 {:.3}；框 {:?}",
        faces.len(),
        faces.first().map(|face| face.confidence).unwrap_or(0.0),
        faces.first().map(|face| face.bbox)
    );
    assert!(!faces.is_empty(), "真实人像必须检出人脸");
    let face = &faces[0];
    for point in face.landmarks {
        assert!(point[0] > 0.0 && point[0] < 1.0 && point[1] > 0.0 && point[1] < 1.0);
    }

    // 2) 人像粗分割。
    let coarse = stages
        .segmenter()
        .segment_coarse(&image)
        .expect("人像粗分割失败");
    assert!(coarse.mean() > 0.01, "人像粗分割不应为空");
    eprintln!("人像粗分割覆盖率：{:.3}", coarse.coverage(0.5));

    // 3) 人脸对齐 + 五官解析。
    use fdxmp_core::smartmask::pipeline::{face_alignment, warp_image};
    let transform =
        face_alignment(face, (image.width(), image.height()), 256).expect("人脸对齐失败");
    let aligned = warp_image(&image, &transform, 256);
    if let Some(out_dir) = std::env::var_os("FDXMP_TEST_OUT") {
        let out_dir = std::path::PathBuf::from(out_dir);
        let _ = std::fs::create_dir_all(&out_dir);
        let _ = aligned.save(out_dir.join("00-aligned-face.png"));
        eprintln!("已导出 {}", out_dir.join("00-aligned-face.png").display());
    }
    let parsed = stages.parser().parse(&aligned).expect("人脸解析失败");
    let mut summary = Vec::new();
    for (kind, mask) in parsed.iter() {
        summary.push(format!("{kind:?}={:.3}", mask.mean()));
    }
    eprintln!("五官解析：{}", summary.join(" "));
    for (kind, mask) in parsed.iter() {
        // 眼睛 / 眉毛等本身就很小（几百到几千像素），阈值按部位区分。
        let minimum = match kind {
            fdxmp_core::smartmask::SmartMaskKind::Skin
            | fdxmp_core::smartmask::SmartMaskKind::Hair
            | fdxmp_core::smartmask::SmartMaskKind::Face => 0.02,
            _ => 0.002,
        };
        assert!(
            mask.mean() > minimum,
            "{kind:?} 蒙版几乎为空（mean={:.4}）",
            mask.mean()
        );
    }

    // 4) 精细抠图（用粗分割包围盒裁剪）。
    let bbox = coarse.bbox(0.5).expect("粗分割为空");
    let crop = image::imageops::crop_imm(
        &image,
        bbox.0,
        bbox.1,
        (bbox.2 - bbox.0).max(1),
        (bbox.3 - bbox.1).max(1),
    )
    .to_image();
    let fine = stages.matting().matting_fine(&crop).expect("精细抠图失败");
    assert!(fine.mean() > 0.01, "精细抠图不应为空");
    eprintln!("精细抠图覆盖率：{:.3}", fine.coverage(0.5));

    // 5) 导出可视化（FDXMP_TEST_OUT 指定目录）。
    if let Some(out_dir) = std::env::var_os("FDXMP_TEST_OUT") {
        let out_dir = std::path::PathBuf::from(out_dir);
        let _ = std::fs::create_dir_all(&out_dir);
        let save = |name: &str, mask: &fdxmp_core::smartmask::SmartMask| {
            let buffer: image::ImageBuffer<image::Luma<u8>, Vec<u8>> =
                image::ImageBuffer::from_fn(mask.width, mask.height, |x, y| {
                    image::Luma([(mask.get(x, y).clamp(0.0, 1.0) * 255.0).round() as u8])
                });
            let path = out_dir.join(format!("{name}.png"));
            let _ = buffer.save(&path);
            eprintln!("已导出 {}", path.display());
        };
        save("01-subject", &coarse);
        for (kind, mask) in parsed.iter() {
            save(&format!("02-{}", kind.as_str()), mask);
        }
        aligned.save(out_dir.join("00-aligned-face.png")).ok();
    }
}

/// 诊断：MODNet / U²-Net 的原始输出统计（排查归一化与激活）。
#[test]
#[ignore]
fn probe_segmenter_stats() {
    use ort::session::Session;
    use ort::value::Tensor;

    let Some(dir) = model_dir() else {
        eprintln!("跳过：未找到模型目录");
        return;
    };
    let registry = ModelRegistry::with_dir(dir);
    if !ensure_models(&registry) {
        return;
    }
    let Some(portrait_env) = std::env::var_os("FDXMP_TEST_PORTRAIT") else {
        eprintln!("跳过：未设置 FDXMP_TEST_PORTRAIT");
        return;
    };
    fdxmp_core::ai::ensure_runtime().expect("ONNX Runtime 初始化失败");
    let image = image::open(std::path::PathBuf::from(portrait_env))
        .expect("打开测试人像失败")
        .to_rgba8();
    for spec in registry.specs() {
        if !matches!(
            spec.task,
            fdxmp_core::smartmask::ModelTask::PersonCoarse
                | fdxmp_core::smartmask::ModelTask::PersonMatting
        ) {
            continue;
        }
        let path = registry.path_of(spec);
        let mut builder = Session::builder().unwrap();
        let mut session = builder.commit_from_file(&path).unwrap();
        let (width, height) = spec.input_size;
        let resized =
            image::imageops::resize(&image, width, height, image::imageops::FilterType::Triangle);
        let pixels = (width as usize) * (height as usize);
        let mut data = vec![0.0f32; pixels * 3];
        for (index, pixel) in resized.pixels().enumerate() {
            for channel in 0..3 {
                data[channel * pixels + index] = spec.normalize(pixel.0[channel] as f32, channel);
            }
        }
        let input_name = session.inputs()[0].name().to_string();
        let tensor =
            Tensor::from_array((vec![1usize, 3, height as usize, width as usize], data)).unwrap();
        let outputs = session
            .run(ort::inputs![input_name.as_str() => tensor])
            .unwrap();
        let (name, value) = outputs.iter().next().unwrap();
        let (shape, values) = value.try_extract_tensor::<f32>().unwrap();
        let min = values.iter().copied().fold(f32::MAX, f32::min);
        let max = values.iter().copied().fold(f32::MIN, f32::max);
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        let above_half =
            values.iter().filter(|value| **value > 0.5).count() as f32 / values.len() as f32;
        eprintln!(
            "{}: out={} shape={:?} min={min:.3} max={max:.3} mean={mean:.3} >0.5={above_half:.3}",
            spec.name, name, shape,
        );
    }
}

/// 运行时（FFI `fdxmp_smartmask_generate` 背后的实现）：按蒙版类型生成并写入缓存。
///
/// 需要 `FDXMP_TEST_PORTRAIT=<照片路径>`；未设置时跳过。
#[test]
fn onnx_runtime_generates_and_caches_each_mask_kind() {
    use fdxmp_core::params::{MaskKind, MaskParams, PeoplePart};

    let Some(portrait_env) = std::env::var_os("FDXMP_TEST_PORTRAIT") else {
        eprintln!("跳过：未设置 FDXMP_TEST_PORTRAIT");
        return;
    };
    let Some(dir) = model_dir() else {
        eprintln!("跳过：未找到模型目录");
        return;
    };
    // 运行时从默认目录读模型（FFI 在 iOS 上就是从 App 包里的 models/ 读）。
    std::env::set_var("FDXMP_MODEL_DIR", &dir);
    let registry = ModelRegistry::with_dir(dir.clone());
    if !ensure_models(&registry) {
        return;
    }
    let image = image::open(std::path::PathBuf::from(portrait_env))
        .expect("打开测试人像失败")
        .to_rgba8();
    let scale = 1024.0 / image.width().max(image.height()) as f32;
    let image = if scale < 1.0 {
        image::imageops::resize(
            &image,
            (image.width() as f32 * scale) as u32,
            (image.height() as f32 * scale) as u32,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };

    assert!(
        fdxmp_core::smartmask::runtime::runtime_available(),
        "模型应可用（随包场景）"
    );
    let kinds = [
        MaskKind::AiSubject,
        MaskKind::AiBackground,
        MaskKind::AiPeople {
            part: PeoplePart::Body,
        },
        MaskKind::AiPeople {
            part: PeoplePart::Face,
        },
        MaskKind::AiPeople {
            part: PeoplePart::Skin,
        },
        MaskKind::AiPeople {
            part: PeoplePart::Hair,
        },
        MaskKind::AiPeople {
            part: PeoplePart::Eyes,
        },
        MaskKind::AiPeople {
            part: PeoplePart::Lips,
        },
    ];
    for kind in kinds {
        let mask = MaskParams {
            id: format!("m-{}", kind_name(&kind)),
            kind: kind.clone(),
            ..MaskParams::default()
        };
        fdxmp_core::smartmask::runtime::generate_into_cache(&mask, &image)
            .unwrap_or_else(|err| panic!("{kind:?} 生成失败：{err}"));
        assert!(
            fdxmp_core::masks::ai_raster_ready(&mask),
            "{kind:?} 栅格未写入缓存"
        );
        // 渲染侧能取到（按任意分辨率重采样）。
        assert!(
            fdxmp_core::masks::cached_ai_raster(&mask, 640, 480).is_some(),
            "{kind:?} 按预览分辨率取不到"
        );
    }
    // 背景 ≈ 主体反向：权重互补。
    let subject = MaskParams {
        id: "s".to_string(),
        kind: MaskKind::AiSubject,
        ..MaskParams::default()
    };
    let background = MaskParams {
        id: "b".to_string(),
        kind: MaskKind::AiBackground,
        ..MaskParams::default()
    };
    let subject_raster = fdxmp_core::masks::cached_ai_raster(&subject, 256, 256).unwrap();
    let background_raster = fdxmp_core::masks::cached_ai_raster(&background, 256, 256).unwrap();
    let sum: f32 = subject_raster
        .weights
        .iter()
        .zip(background_raster.weights.iter())
        .map(|(a, b)| (a + b - 1.0).abs())
        .sum::<f32>()
        / subject_raster.weights.len() as f32;
    assert!(sum < 0.01, "主体 + 背景应互补（平均偏差 {sum}）");
    fdxmp_core::masks::clear_ai_raster_cache();
}

fn kind_name(kind: &fdxmp_core::params::MaskKind) -> String {
    format!("{kind:?}").replace(['(', ')', '{', '}', ' ', ':', ','], "")
}
