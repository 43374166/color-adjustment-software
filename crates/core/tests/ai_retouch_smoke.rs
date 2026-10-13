//! AI 修图整链路冒烟（默认忽略；需要本机模型目录 + onnxruntime.dll + 测试人像）。
//!
//! 运行示例（PowerShell，仓库根目录）：
//!
//! ```powershell
//! $env:FDXMP_ORT_DLL = '…\onnxruntime.dll'
//! $env:FDXMP_MODEL_DIR = '…\models-app'
//! $env:FDXMP_TEST_IMAGE = '…\portrait.png'
//! $env:FDXMP_TEST_OUT = '…\out'
//! cargo test -p fdxmp-core --features ai --test ai_retouch_smoke -- --ignored --nocapture
//! ```
#![cfg(any(feature = "ai", feature = "ai-static"))]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

#[test]
#[ignore = "需要 FDXMP_ORT_DLL / FDXMP_MODEL_DIR / FDXMP_TEST_IMAGE / FDXMP_TEST_OUT"]
fn ai_retouch_full_chain_smoke() {
    let Some(image_path) = env("FDXMP_TEST_IMAGE") else {
        eprintln!("跳过：未设置 FDXMP_TEST_IMAGE");
        return;
    };
    let out_dir = env("FDXMP_TEST_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("fdxmp-ai-retouch"));
    std::fs::create_dir_all(&out_dir).expect("创建输出目录");

    let image = image::open(&image_path)
        .expect("读取测试图")
        .to_rgba8();
    eprintln!("测试图：{}×{}", image.width(), image.height());

    // 分析（蒙版 + 关键点 + 瑕疵检测）
    let started = Instant::now();
    let analysis = fdxmp_core::smartmask::runtime::retouch_analyze(&image).expect("AI 分析");
    eprintln!(
        "分析耗时 {:?}：{}",
        started.elapsed(),
        serde_json::to_string(&analysis).unwrap_or_default()
    );

    // 修图（模拟「写真」一键：修复 + 磨皮 + 肤色 + 增强 + 塑形）
    let params = fdxmp_core::retouch::RetouchParams {
        blemish: 45.0,
        smooth_low: 30.0,
        smooth_mid: 28.0,
        smooth_high: 22.0,
        skin_tone: 30.0,
        restore: 50.0,
        face_slim: 18.0,
        chin: 8.0,
        eye_enlarge: 8.0,
        nose_slim: 6.0,
        body_slim: 0.0,
    };
    assert!(!params.is_identity());

    // 后台打印进度（验证 UI 轮询文案）。
    let stop = Arc::new(AtomicBool::new(false));
    let progress_stop = stop.clone();
    let progress_thread = std::thread::spawn(move || {
        while !progress_stop.load(Ordering::Relaxed) {
            let text = fdxmp_core::smartmask::runtime::retouch_progress();
            if !text.is_empty() {
                eprintln!("[进度] {text}");
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    });

    let started = Instant::now();
    let output =
        fdxmp_core::smartmask::runtime::retouch_apply(&image, &params).expect("AI 修图");
    eprintln!("修图耗时（首帧，含 AI 缓存） {:?}", started.elapsed());
    output
        .save(out_dir.join("ai-retouch-after.png"))
        .expect("保存修图结果");

    // 二次调用应命中 AI 缓存（明显更快）。
    let started = Instant::now();
    let second =
        fdxmp_core::smartmask::runtime::retouch_apply(&image, &params).expect("AI 修图（缓存）");
    eprintln!("修图耗时（缓存） {:?}", started.elapsed());
    assert_eq!(second.dimensions(), output.dimensions());

    stop.store(true, Ordering::Relaxed);
    let _ = progress_thread.join();

    // 导出增强：Real-ESRGAN 4 倍超分（裁剪小区域验证分块路径）。
    let crop_w = output.width().min(320);
    let crop_h = output.height().min(240);
    let crop = image::imageops::crop_imm(&output, 0, 0, crop_w, crop_h).to_image();
    let started = Instant::now();
    let target = crop_w.max(crop_h) * 4;
    let upscaled =
        fdxmp_core::smartmask::runtime::retouch_upscale(&crop, target).expect("超分");
    eprintln!(
        "超分耗时 {:?} → {}×{}",
        started.elapsed(),
        upscaled.width(),
        upscaled.height()
    );
    assert_eq!((upscaled.width(), upscaled.height()), (crop_w * 4, crop_h * 4));
    upscaled.save(out_dir.join("ai-upscale.png")).expect("保存超分结果");
}
