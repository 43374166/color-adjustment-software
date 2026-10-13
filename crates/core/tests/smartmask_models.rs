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
        stages.matting(),
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
    let registry = ModelRegistry::with_dir(dir.clone());
    let _ = ensure_models(&registry);
    fdxmp_core::ai::ensure_runtime().expect("ONNX Runtime 初始化失败");
    // 全部 .onnx（含尚未登记进注册表的候选模型），便于换模型时对齐规格。
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .map(|extension| extension == "onnx")
                .unwrap_or(false)
            {
                files.push(path);
            }
        }
    }
    files.sort();
    for path in files {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut builder = Session::builder().unwrap();
        let session = builder.commit_from_file(&path).unwrap();
        eprintln!("== {name}");
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
        .expect("需要人像粗分割模型")
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
            // 耳朵常被头发遮住：允许为空（只要求类别存在）。
            fdxmp_core::smartmask::SmartMaskKind::Ears => 0.0,
            _ => 0.002,
        };
        // minimum = 0 表示「允许为空」（如被头发遮住的耳朵）。
        if minimum > 0.0 {
            assert!(
                mask.mean() > minimum,
                "{kind:?} 蒙版几乎为空（mean={:.4}）",
                mask.mean()
            );
        }
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
    let fine = stages
        .matting()
        .expect("需要精细抠图模型")
        .matting_fine(&crop)
        .expect("精细抠图失败");
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

/// 诊断：MODNet 量化版（`_candidate_modnet_quantized.onnx`）与 fp32 版的蒙版一致性（IoU）。
#[test]
#[ignore]
fn probe_modnet_quantized() {
    use ort::session::Session;
    use ort::value::Tensor;

    let Some(portrait_env) = std::env::var_os("FDXMP_TEST_PORTRAIT") else {
        eprintln!("跳过：未设置 FDXMP_TEST_PORTRAIT");
        return;
    };
    let Some(dir) = model_dir() else {
        eprintln!("跳过：未找到模型目录");
        return;
    };
    let quantized = dir.join("_candidate_modnet_quantized.onnx");
    let fp32 = dir.join("person_coarse_modnet_fp32.onnx");
    if !quantized.is_file() || !fp32.is_file() {
        eprintln!("跳过：缺少模型");
        return;
    }
    if std::env::var_os("FDXMP_ORT_DLL").is_none() {
        let dll = dir.join("../onnxruntime.dll");
        if dll.is_file() {
            std::env::set_var("FDXMP_ORT_DLL", dll);
        }
    }
    fdxmp_core::ai::ensure_runtime().expect("ONNX Runtime 初始化失败");
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
    let size = 512u32;
    let resized =
        image::imageops::resize(&image, size, size, image::imageops::FilterType::Triangle);
    let pixels = (size as usize) * (size as usize);
    let mut data = vec![0.0f32; pixels * 3];
    for (index, pixel) in resized.pixels().enumerate() {
        for channel in 0..3 {
            let value = pixel.0[channel] as f32 / 255.0;
            data[channel * pixels + index] = (value - 0.5) / 0.5;
        }
    }

    let run = |path: &std::path::Path| -> Option<(Vec<usize>, Vec<f32>)> {
        let mut builder = Session::builder().unwrap();
        let mut session = builder.commit_from_file(path).ok()?;
        let input_name = session.inputs()[0].name().to_string();
        let tensor =
            Tensor::from_array((vec![1usize, 3, size as usize, size as usize], data.clone()))
                .unwrap();
        let outputs = session
            .run(ort::inputs![input_name.as_str() => tensor])
            .ok()?;
        let (name, value) = outputs
            .iter()
            .find(|(name, _)| name.contains("output"))
            .or_else(|| outputs.iter().next())?;
        let (shape, values) = value.try_extract_tensor::<f32>().ok()?;
        eprintln!(
            "{}: out={name} shape={:?}",
            path.file_name().unwrap().to_string_lossy(),
            shape
        );
        Some((
            shape.iter().map(|dim| *dim as usize).collect(),
            values.to_vec(),
        ))
    };

    let (shape_a, alpha_a) = run(&fp32).expect("fp32 推理失败");
    let (shape_b, alpha_b) = run(&quantized).expect("quantized 推理失败");
    let count = alpha_a.len().min(alpha_b.len());
    let mut intersection = 0usize;
    let mut union = 0usize;
    let mut mean_diff = 0.0f32;
    for index in 0..count {
        let a = alpha_a[index] >= 0.5;
        let b = alpha_b[index] >= 0.5;
        if a && b {
            intersection += 1;
        }
        if a || b {
            union += 1;
        }
        mean_diff += (alpha_a[index] - alpha_b[index]).abs();
    }
    let iou = intersection as f32 / union.max(1) as f32;
    eprintln!(
        "fp32 shape={shape_a:?} / quant shape={shape_b:?}；像素平均差={:.4}，IoU(0.5)={iou:.4}",
        mean_diff / count.max(1) as f32
    );
    // 导出一张（量化版）蒙版便于目测。
    if let Some(out_dir) = std::env::var_os("FDXMP_TEST_OUT") {
        let out_dir = std::path::PathBuf::from(out_dir);
        let _ = std::fs::create_dir_all(&out_dir);
        if shape_b.len() == 4 {
            let (height, width) = (shape_b[2], shape_b[3]);
            let buffer: image::ImageBuffer<image::Luma<u8>, Vec<u8>> =
                image::ImageBuffer::from_fn(width as u32, height as u32, |x, y| {
                    let value = alpha_b[(y as usize) * width + x as usize];
                    image::Luma([(value.clamp(0.0, 1.0) * 255.0) as u8])
                });
            let path = out_dir.join("candidate-modnet-quantized.png");
            let _ = buffer.save(&path);
            eprintln!("已导出 {}", path.display());
        }
    }
}

/// 诊断：候选抠图模型 PP-HumanSeg（`_candidate_humanseg.onnx`，5.9MB）的输入尺寸与蒙版质量。
#[test]
#[ignore]
fn probe_humanseg_candidate() {
    use ort::session::Session;
    use ort::value::Tensor;

    let Some(portrait_env) = std::env::var_os("FDXMP_TEST_PORTRAIT") else {
        eprintln!("跳过：未设置 FDXMP_TEST_PORTRAIT");
        return;
    };
    let Some(dir) = model_dir() else {
        eprintln!("跳过：未找到模型目录");
        return;
    };
    let path = dir.join("_candidate_humanseg.onnx");
    if !path.is_file() {
        eprintln!("跳过：缺少候选模型 {}", path.display());
        return;
    }
    if std::env::var_os("FDXMP_ORT_DLL").is_none() {
        let dll = dir.join("../onnxruntime.dll");
        if dll.is_file() {
            std::env::set_var("FDXMP_ORT_DLL", dll);
        }
    }
    fdxmp_core::ai::ensure_runtime().expect("ONNX Runtime 初始化失败");
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

    for size in [192u32, 512, 256, 320, 384] {
        let mut builder = Session::builder().unwrap();
        let mut session = match builder.commit_from_file(&path) {
            Ok(session) => session,
            Err(err) => {
                eprintln!("加载失败：{err}");
                return;
            }
        };
        let resized =
            image::imageops::resize(&image, size, size, image::imageops::FilterType::Triangle);
        let pixels = (size as usize) * (size as usize);
        let mut data = vec![0.0f32; pixels * 3];
        for (index, pixel) in resized.pixels().enumerate() {
            for channel in 0..3 {
                let value = pixel.0[channel] as f32 / 255.0;
                data[channel * pixels + index] = (value - 0.5) / 0.5;
            }
        }
        let input_name = session.inputs()[0].name().to_string();
        let tensor =
            Tensor::from_array((vec![1usize, 3, size as usize, size as usize], data)).unwrap();
        let outputs = match session.run(ort::inputs![input_name.as_str() => tensor]) {
            Ok(outputs) => outputs,
            Err(err) => {
                eprintln!(
                    "size {size}: 推理失败（{}）",
                    err.to_string().lines().next().unwrap_or("")
                );
                continue;
            }
        };
        let (name, value) = outputs.iter().next().unwrap();
        let (shape, values) = value.try_extract_tensor::<f32>().unwrap();
        let dims: Vec<usize> = shape.iter().map(|dim| *dim as usize).collect();
        let min = values.iter().copied().fold(f32::MAX, f32::min);
        let max = values.iter().copied().fold(f32::MIN, f32::max);
        eprintln!(
            "size {size}: in={input_name} out={name} shape={dims:?} min={min:.3} max={max:.3}"
        );
        if dims.len() != 4 || dims[0] != 1 {
            continue;
        }
        // [1,1,H,W]（概率）或 [1,2,H,W]（两类 logits）→ 前景 alpha。
        let (out_h, out_w) = (dims[2], dims[3]);
        let out_pixels = out_h * out_w;
        let mut alpha = vec![0.0f32; out_pixels];
        if dims[1] == 1 {
            for (index, slot) in alpha.iter_mut().enumerate() {
                *slot = values[index].clamp(0.0, 1.0);
            }
        } else if dims[1] == 2 {
            for index in 0..out_pixels {
                let foreground = values[out_pixels + index];
                let background = values[index];
                let probability = 1.0 / (1.0 + (-(foreground - background)).exp());
                alpha[index] = probability;
            }
        } else {
            continue;
        }
        let mean = alpha.iter().sum::<f32>() / alpha.len() as f32;
        let coverage =
            alpha.iter().filter(|value| **value > 0.5).count() as f32 / alpha.len() as f32;
        eprintln!("  alpha mean={mean:.3} coverage(0.5)={coverage:.3}");
        if let Some(out_dir) = std::env::var_os("FDXMP_TEST_OUT") {
            let out_dir = std::path::PathBuf::from(out_dir);
            let _ = std::fs::create_dir_all(&out_dir);
            let upscaled = fdxmp_core::smartmask::mask::resize_plane(
                &alpha,
                out_w as u32,
                out_h as u32,
                image.width(),
                image.height(),
            );
            let buffer: image::ImageBuffer<image::Luma<u8>, Vec<u8>> =
                image::ImageBuffer::from_fn(image.width(), image.height(), |x, y| {
                    let value = upscaled[(y * image.width() + x) as usize];
                    image::Luma([(value.clamp(0.0, 1.0) * 255.0) as u8])
                });
            let path = out_dir.join(format!("candidate-humanseg-{size}.png"));
            let _ = buffer.save(&path);
            eprintln!("  已导出 {}", path.display());
        }
        break;
    }
}

/// 诊断：候选人脸解析模型（BiSeNet-19，`_candidate_faceparser_sim.onnx`）的标签分布与可视化。
///
/// 需要 `FDXMP_TEST_PORTRAIT`；`FDXMP_TEST_OUT` 指定导出目录（可选）。
#[test]
#[ignore]
fn probe_faceparser_candidate() {
    use ort::session::Session;
    use ort::value::Tensor;

    let Some(portrait_env) = std::env::var_os("FDXMP_TEST_PORTRAIT") else {
        eprintln!("跳过：未设置 FDXMP_TEST_PORTRAIT");
        return;
    };
    let Some(dir) = model_dir() else {
        eprintln!("跳过：未找到模型目录");
        return;
    };
    let candidate = std::env::var("FDXMP_FACEPARSER_CANDIDATE")
        .unwrap_or_else(|_| "_candidate_faceparser_sim.onnx".to_string());
    let path = dir.join(candidate);
    if !path.is_file() {
        eprintln!("跳过：缺少候选模型 {}", path.display());
        return;
    }
    std::env::set_var("FDXMP_MODEL_DIR", &dir);
    if std::env::var_os("FDXMP_ORT_DLL").is_none() {
        let dll = dir.join("../onnxruntime.dll");
        if dll.is_file() {
            std::env::set_var("FDXMP_ORT_DLL", dll);
        }
    }
    fdxmp_core::ai::ensure_runtime().expect("ONNX Runtime 初始化失败");
    let registry = ModelRegistry::with_dir(dir.clone());
    let stages = OnnxStages::load(&registry).expect("加载模型失败");
    let image = image::open(std::path::PathBuf::from(portrait_env))
        .expect("打开测试人像失败")
        .to_rgba8();
    let faces = stages.detector().detect(&image).expect("人脸检测失败");
    assert!(!faces.is_empty());
    let transform = fdxmp_core::smartmask::pipeline::face_alignment(
        &faces[0],
        (image.width(), image.height()),
        512,
    )
    .unwrap();
    let crop = fdxmp_core::smartmask::pipeline::warp_image(&image, &transform, 512);
    if let Some(out_dir) = std::env::var_os("FDXMP_TEST_OUT") {
        let out_dir = std::path::PathBuf::from(out_dir);
        let _ = std::fs::create_dir_all(&out_dir);
        let _ = crop.save(out_dir.join("candidate-aligned-512.png"));
    }

    let mut builder = Session::builder().unwrap();
    let mut session = builder.commit_from_file(&path).unwrap();
    eprintln!("== candidate faceparser");
    for input in session.inputs() {
        eprintln!("  in  {}", input.name());
    }
    for output in session.outputs() {
        eprintln!("  out {}", output.name());
    }
    // zllrunning face-parsing：ImageNet 归一化、512×512、NCHW。
    let mean = [0.485f32, 0.456, 0.406];
    let std = [0.229f32, 0.224, 0.225];
    let mut data = vec![0.0f32; 512 * 512 * 3];
    for (index, pixel) in crop.pixels().enumerate() {
        for channel in 0..3 {
            let value = pixel.0[channel] as f32 / 255.0;
            data[channel * 512 * 512 + index] = (value - mean[channel]) / std[channel];
        }
    }
    let input_name = session.inputs()[0].name().to_string();
    let tensor = Tensor::from_array((vec![1usize, 3, 512, 512], data)).unwrap();
    let outputs = session
        .run(ort::inputs![input_name.as_str() => tensor])
        .unwrap();
    let (shape, values) = outputs[0].try_extract_tensor::<f32>().unwrap();
    eprintln!("  out0 shape={:?}", shape);
    let dims: Vec<usize> = shape.iter().map(|dim| *dim as usize).collect();
    let (classes, height, width) = match dims.as_slice() {
        [1, classes, height, width] => (*classes, *height, *width),
        _ => {
            eprintln!("  非预期输出形状");
            return;
        }
    };
    let pixels = height * width;
    let mut histogram = vec![0usize; classes];
    let mut labels = vec![0usize; pixels];
    for index in 0..pixels {
        let mut best = 0usize;
        let mut best_value = f32::MIN;
        for class in 0..classes {
            let value = values[class * pixels + index];
            if value > best_value {
                best_value = value;
                best = class;
            }
        }
        labels[index] = best;
        histogram[best] += 1;
    }
    let names = [
        "background",
        "skin",
        "nose",
        "eye_g",
        "l_eye",
        "r_eye",
        "l_brow",
        "r_brow",
        "l_ear",
        "r_ear",
        "mouth",
        "u_lip",
        "l_lip",
        "hair",
        "hat",
        "ear_r",
        "neck_l",
        "neck",
        "cloth",
    ];
    for (class, count) in histogram.iter().enumerate() {
        if *count > 0 {
            let name = names.get(class).copied().unwrap_or("?");
            eprintln!(
                "  class {class:>2} {name:<10} {:>6.2}%",
                *count as f32 / pixels as f32 * 100.0
            );
        }
    }
    if let Some(out_dir) = std::env::var_os("FDXMP_TEST_OUT") {
        let out_dir = std::path::PathBuf::from(out_dir);
        let _ = std::fs::create_dir_all(&out_dir);
        let palette: [[u8; 3]; 19] = [
            [0, 0, 0],
            [255, 200, 170],
            [200, 120, 90],
            [120, 120, 255],
            [80, 200, 255],
            [80, 200, 255],
            [255, 160, 60],
            [255, 160, 60],
            [230, 180, 140],
            [230, 180, 140],
            [180, 60, 60],
            [240, 90, 90],
            [200, 60, 60],
            [90, 70, 50],
            [200, 200, 0],
            [255, 255, 0],
            [160, 140, 120],
            [170, 150, 130],
            [60, 120, 200],
        ];
        let buffer: image::ImageBuffer<image::Rgb<u8>, Vec<u8>> =
            image::ImageBuffer::from_fn(width as u32, height as u32, |x, y| {
                let label = labels[(y as usize) * width + x as usize].min(18);
                image::Rgb(palette[label])
            });
        let path = out_dir.join("candidate-faceparser-labels.png");
        let _ = buffer.save(&path);
        eprintln!("  已导出 {}", path.display());
        // 关键类别单独导出（确认类别语义：皮肤 / 鼻子 / 眉毛 / 头发 / 脖子）。
        for (class, name) in [
            (1usize, "skin"),
            (2, "nose"),
            (6, "l_brow"),
            (13, "hair"),
            (17, "neck"),
        ] {
            let buffer: image::ImageBuffer<image::Luma<u8>, Vec<u8>> =
                image::ImageBuffer::from_fn(width as u32, height as u32, |x, y| {
                    let label = labels[(y as usize) * width + x as usize];
                    image::Luma([if label == class { 255 } else { 0 }])
                });
            let path = out_dir.join(format!("candidate-class-{class:02}-{name}.png"));
            let _ = buffer.save(&path);
        }
        eprintln!("  各类别掩膜已导出");
    }
}

/// AI 修图端到端（真实人像）：识别（五官 / 蒙版）→ 一键修图 → 导出前后对照。
///
/// 需要 `FDXMP_TEST_PORTRAIT`；`FDXMP_TEST_OUT` 指定导出目录（可选）。
#[test]
fn onnx_retouch_analyze_and_apply_on_real_portrait() {
    use fdxmp_core::retouch::RetouchParams;

    let Some(portrait_env) = std::env::var_os("FDXMP_TEST_PORTRAIT") else {
        eprintln!("跳过：未设置 FDXMP_TEST_PORTRAIT");
        return;
    };
    let Some(dir) = model_dir() else {
        eprintln!("跳过：未找到模型目录");
        return;
    };
    std::env::set_var("FDXMP_MODEL_DIR", &dir);
    let registry = ModelRegistry::with_dir(dir.clone());
    if !ensure_models(&registry) {
        return;
    }
    if std::env::var_os("FDXMP_ORT_DLL").is_none() {
        let dll = dir.join("../onnxruntime.dll");
        if dll.is_file() {
            std::env::set_var("FDXMP_ORT_DLL", dll);
        }
    }
    let image = image::open(std::path::PathBuf::from(portrait_env))
        .expect("打开测试人像失败")
        .to_rgba8();
    let scale = 1600.0 / image.width().max(image.height()) as f32;
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

    let analysis = fdxmp_core::smartmask::runtime::retouch_analyze(&image).expect("识别失败");
    eprintln!(
        "识别：faces={} landmarks={} skin={} person={} warnings={:?}",
        analysis.faces, analysis.landmarks, analysis.skin, analysis.person, analysis.warnings
    );
    assert!(analysis.faces >= 1, "真实人像应检出人脸");
    assert_eq!(analysis.landmarks, 478, "FaceMesh 应输出 478 点");
    assert!(analysis.skin && analysis.person);

    // 一键修图（默认强度）。
    let params = RetouchParams {
        blemish: 60.0,
        smooth_low: 40.0,
        smooth_mid: 55.0,
        smooth_high: 45.0,
        skin_tone: 50.0,
        restore: 45.0,
        face_slim: 35.0,
        chin: 20.0,
        eye_enlarge: 20.0,
        nose_slim: 15.0,
        body_slim: 30.0,
    };
    let start = std::time::Instant::now();
    let retouched =
        fdxmp_core::smartmask::runtime::retouch_apply(&image, &params).expect("修图失败");
    eprintln!(
        "修图耗时：{:.2}s（{}×{}）",
        start.elapsed().as_secs_f32(),
        image.width(),
        image.height()
    );
    assert_eq!(retouched.dimensions(), image.dimensions());
    let diff: f32 = retouched
        .as_raw()
        .iter()
        .zip(image.as_raw().iter())
        .map(|(a, b)| (*a as f32 - *b as f32).abs())
        .sum::<f32>()
        / retouched.as_raw().len() as f32;
    assert!(diff > 1.0, "修图应改变像素（平均差 {diff}）");

    if let Some(out_dir) = std::env::var_os("FDXMP_TEST_OUT") {
        let out_dir = std::path::PathBuf::from(out_dir);
        let _ = std::fs::create_dir_all(&out_dir);
        // JPEG 不支持 RGBA，用 PNG 导出对照图。
        let _ = image.save(out_dir.join("retouch-before.png"));
        let _ = retouched.save(out_dir.join("retouch-after.png"));
        eprintln!(
            "已导出 {} / {}",
            out_dir.join("retouch-before.png").display(),
            out_dir.join("retouch-after.png").display()
        );
    }
    fdxmp_core::masks::clear_ai_raster_cache();
}

/// 诊断：FaceMesh（468/478 点）候选模型的输出形状与取值范围。
#[test]
#[ignore]
fn probe_facemesh() {
    use ort::session::Session;
    use ort::value::Tensor;

    let Some(portrait_env) = std::env::var_os("FDXMP_TEST_PORTRAIT") else {
        eprintln!("跳过：未设置 FDXMP_TEST_PORTRAIT");
        return;
    };
    let Some(dir) = model_dir() else {
        eprintln!("跳过：未找到模型目录");
        return;
    };
    std::env::set_var("FDXMP_MODEL_DIR", &dir);
    if std::env::var_os("FDXMP_ORT_DLL").is_none() {
        let dll = dir.join("../onnxruntime.dll");
        if dll.is_file() {
            std::env::set_var("FDXMP_ORT_DLL", dll);
        }
    }
    fdxmp_core::ai::ensure_runtime().expect("ONNX Runtime 初始化失败");
    let image = image::open(std::path::PathBuf::from(portrait_env))
        .expect("打开测试人像失败")
        .to_rgba8();
    // 用 SCRFD 检出的第一个人脸做对齐裁剪。
    let registry = ModelRegistry::with_dir(dir.clone());
    let stages = OnnxStages::load(&registry).expect("加载模型失败");
    let faces = stages.detector().detect(&image).expect("人脸检测失败");
    assert!(!faces.is_empty(), "需要一张有脸的测试照片");
    let face = &faces[0];
    let transform =
        fdxmp_core::smartmask::pipeline::face_alignment(face, (image.width(), image.height()), 256)
            .unwrap();
    let crop = fdxmp_core::smartmask::pipeline::warp_image(&image, &transform, 256);

    for file in [
        "face_landmarks_facemesh_fp32.onnx",
        "face_landmarks_mediapipe_v2_fp32.onnx",
    ] {
        let path = dir.join(file);
        if !path.is_file() {
            continue;
        }
        let mut builder = Session::builder().unwrap();
        let mut session = builder.commit_from_file(&path).unwrap();
        // 布局 NHWC [1,256,256,3]，MediaPipe 归一化 (x-127.5)/127.5 → [-1,1]。
        let mut data = vec![0.0f32; 256 * 256 * 3];
        for (index, pixel) in crop.pixels().enumerate() {
            for channel in 0..3 {
                data[index * 3 + channel] = (pixel.0[channel] as f32 - 127.5) / 127.5;
            }
        }
        let input_name = session.inputs()[0].name().to_string();
        let tensor = Tensor::from_array((vec![1usize, 256, 256, 3], data)).unwrap();
        let outputs = session
            .run(ort::inputs![input_name.as_str() => tensor])
            .unwrap();
        for (name, value) in outputs.iter() {
            if let Ok((shape, values)) = value.try_extract_tensor::<f32>() {
                let min = values.iter().copied().fold(f32::MAX, f32::min);
                let max = values.iter().copied().fold(f32::MIN, f32::max);
                eprintln!(
                    "{file} out {name} shape={:?} len={} min={min:.3} max={max:.3}",
                    shape,
                    values.len()
                );
                if values.len() >= 12 {
                    eprintln!(
                        "   pts[0..4] = {:?}",
                        values[..12.min(values.len())]
                            .chunks(3)
                            .map(|c| [c[0], c[1]])
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }
}

/// 回归：切换图片后（清栅格缓存）重新生成，必须得到新图的蒙版，而不是上一张的。
#[test]
fn onnx_runtime_regenerates_after_photo_switch() {
    use fdxmp_core::params::{MaskKind, MaskParams, PeoplePart};

    let Some(dir) = model_dir() else {
        eprintln!("跳过：未找到模型目录");
        return;
    };
    let portrait_a = std::env::var_os("FDXMP_TEST_PORTRAIT").map(std::path::PathBuf::from);
    let portrait_b = std::env::var_os("FDXMP_TEST_PORTRAIT_B").map(std::path::PathBuf::from);
    let (Some(portrait_a), Some(portrait_b)) = (portrait_a, portrait_b) else {
        eprintln!("跳过：未设置 FDXMP_TEST_PORTRAIT / FDXMP_TEST_PORTRAIT_B");
        return;
    };
    std::env::set_var("FDXMP_MODEL_DIR", &dir);
    let registry = ModelRegistry::with_dir(dir);
    if !ensure_models(&registry) {
        return;
    }
    let load = |path: &std::path::PathBuf| image::open(path).expect("打开测试人像失败").to_rgba8();
    let image_a = load(&portrait_a);
    let image_b = load(&portrait_b);
    assert_ne!(image_a.as_raw(), image_b.as_raw(), "两张图必须不同");

    let make_mask = |id: &str| MaskParams {
        id: id.to_string(),
        kind: MaskKind::AiPeople {
            part: PeoplePart::Body,
        },
        ..MaskParams::default()
    };

    // 图片 A：第一次生成。
    let mask_a = make_mask("a");
    fdxmp_core::smartmask::runtime::generate_into_cache(&mask_a, &image_a).unwrap();
    let raster_a = fdxmp_core::masks::cached_ai_raster(&mask_a, 256, 256).unwrap();

    // 切图：iOS 会清栅格缓存（fdxmp_mask_raster_clear）。
    fdxmp_core::masks::clear_ai_raster_cache();
    assert!(!fdxmp_core::masks::ai_raster_ready(&mask_a));

    // 图片 B：同一类型蒙版必须重新推理出 B 的蒙版。
    let mask_b = make_mask("b");
    fdxmp_core::smartmask::runtime::generate_into_cache(&mask_b, &image_b).unwrap();
    let raster_b = fdxmp_core::masks::cached_ai_raster(&mask_b, 256, 256).unwrap();

    let diff: f32 = raster_a
        .weights
        .iter()
        .zip(raster_b.weights.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / raster_a.weights.len() as f32;
    assert!(diff > 0.01, "切图后蒙版不应与上一张相同（平均差异 {diff}）");
    fdxmp_core::masks::clear_ai_raster_cache();
}
