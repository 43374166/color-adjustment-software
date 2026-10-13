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

    // GFPGAN 生效性：restore=0 与 restore=50 的输出应明显不同（防「静默失效」回归）。
    let mut no_restore = params.clone();
    no_restore.restore = 0.0;
    let without = fdxmp_core::smartmask::runtime::retouch_apply(&image, &no_restore)
        .expect("AI 修图（无增强）");
    without
        .save(out_dir.join("ai-retouch-no-restore.png"))
        .expect("保存无增强结果");
    let mut changed = 0usize;
    let mut max_diff = 0i64;
    let (without_px, _) = without.as_raw().as_chunks::<4>();
    let (output_px, _) = output.as_raw().as_chunks::<4>();
    for (a, b) in without_px.iter().zip(output_px.iter()) {
        let d = (0..3)
            .map(|c| (a[c] as i64 - b[c] as i64).abs())
            .max()
            .unwrap_or(0);
        if d > 2 {
            changed += 1;
        }
        max_diff = max_diff.max(d);
    }
    let total_pixels = (image.width() as usize) * (image.height() as usize);
    eprintln!(
        "GFPGAN 生效：改变像素 {changed}（{:.2}%），最大差 {max_diff}",
        changed as f64 / total_pixels as f64 * 100.0
    );
    assert!(
        changed as f64 >= total_pixels as f64 * 0.003,
        "GFPGAN 增强应改变面部区域像素（{changed}/{total_pixels}）"
    );
    assert!(max_diff >= 8, "GFPGAN 应有可见变化（最大差 {max_diff}）");

    // 单独看 GFPGAN：只开 restore（便于目检增强效果）。
    let restore_only = fdxmp_core::retouch::RetouchParams {
        restore: 90.0,
        ..Default::default()
    };
    let restored = fdxmp_core::smartmask::runtime::retouch_apply(&image, &restore_only)
        .expect("AI 人脸增强");
    restored
        .save(out_dir.join("ai-restore-only.png"))
        .expect("保存增强结果");

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
