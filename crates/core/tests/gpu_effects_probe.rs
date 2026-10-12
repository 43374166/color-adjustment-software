//! 本机诊断（手动运行）：用 GPU 管线实际渲染，量化"效果 / 细节"滑块的像素影响。
//!
//! 运行：
//!   cargo test -p fdxmp-core --features gpu --test gpu_effects_probe -- --ignored --nocapture
#![cfg(feature = "gpu")]

use fdxmp_core::gpu::PreviewSession;
use fdxmp_core::params::EditParams;

fn test_image(width: u32, height: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    // 确定性 LCG：细碎纹理 + 色噪，模拟真实照片的高频含量。
    let mut state = 0x1234_5678u32;
    let mut next = move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((state >> 8) & 0xFFFF) as f32 / 65535.0
    };
    for y in 0..height {
        for x in 0..width {
            let index = ((y * width + x) * 4) as usize;
            // 斜向条纹 + 低频渐变：既有细节（锐化 / 纹理的用武之地），也有雾感底子。
            let stripe = ((x as f32 * 0.35 + y as f32 * 0.21).sin() * 0.5 + 0.5) * 60.0;
            let base = 90.0 + 70.0 * (x as f32 / width as f32);
            // 细碎高频纹理（棋盘 1px + 逐像素随机），让纹理 / 锐化 / 降噪都有事可做。
            let checker = if (x + y) % 2 == 0 { 10.0 } else { -10.0 };
            let speckle = (next() - 0.5) * 16.0;
            let value = base + stripe + checker + speckle;
            // 逐通道色噪（色度降噪的作用对象）。
            let chroma_r = (next() - 0.5) * 18.0;
            let chroma_b = (next() - 0.5) * 18.0;
            rgba[index] = (value + chroma_r).clamp(0.0, 255.0) as u8;
            rgba[index + 1] = value.clamp(0.0, 255.0) as u8;
            rgba[index + 2] = (value + chroma_b).clamp(0.0, 255.0) as u8;
            rgba[index + 3] = 255;
        }
    }
    rgba
}

/// 色度噪声强度（R-G 与 B-G 的标准差均值）：颜色降噪的效果指标。
fn chroma_noise(rgba: &[u8]) -> f64 {
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut count = 0.0f64;
    for pixel in rgba.chunks_exact(4) {
        let r = pixel[0] as f64;
        let g = pixel[1] as f64;
        let b = pixel[2] as f64;
        for value in [(r - g), (b - g)] {
            sum += value;
            sum_sq += value * value;
            count += 1.0;
        }
    }
    let mean = sum / count.max(1.0);
    ((sum_sq / count.max(1.0)) - mean * mean).max(0.0).sqrt()
}

fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
    let mut total = 0.0f64;
    let mut count = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        total += (*x as f64 - *y as f64).abs();
        count += 1.0;
    }
    total / count.max(1.0)
}

#[test]
#[ignore]
fn probe_effects_and_detail_strength() {
    let (width, height) = (320u32, 240u32);
    let rgba = test_image(width, height);
    let mut session = PreviewSession::new(&rgba, width, height).expect("GPU 初始化失败");
    println!("backend = {}", session.backend());

    let render = |session: &mut PreviewSession, mutate: &dyn Fn(&mut EditParams)| -> Vec<u8> {
        let mut params = EditParams::default();
        mutate(&mut params);
        let (_, _, pixels) = session.render(&params).expect("渲染失败");
        pixels
    };

    let baseline = render(&mut session, &|_| {});
    let cases: [(&str, &dyn Fn(&mut EditParams)); 9] = [
        ("texture+100", &|p| p.effects.texture = 100.0),
        ("texture-100", &|p| p.effects.texture = -100.0),
        ("clarity+100", &|p| p.effects.clarity = 100.0),
        ("dehaze+100", &|p| p.effects.dehaze = 100.0),
        ("sharpen(amount100)", &|p| p.detail.sharpen.amount = 100.0),
        ("nr_luma(100)", &|p| p.detail.noise.luminance = 100.0),
        ("nr_color(100)", &|p| p.detail.noise.color = 100.0),
        ("grain(100)", &|p| p.effects.grain = 100.0),
        ("vignette-100", &|p| p.effects.vignette = -100.0),
    ];
    for (name, mutate) in cases {
        let output = render(&mut session, mutate);
        let diff = mean_abs_diff(&baseline, &output);
        println!("{name:22} 平均像素差 = {diff:.3}");
    }
    println!(
        "色度噪声           基线 = {:.3}，颜色降噪(100) = {:.3}",
        chroma_noise(&baseline),
        chroma_noise(&render(&mut session, &|p| p.detail.noise.color = 100.0)),
    );
}

/// 色调分区（阴影 / 黑色）的 CPU ↔ GPU 一致性探针。
///
/// 同一张灰度台阶图分别走 CPU（`render_memory_rgba`）与 GPU 管线，
/// 打印同位置像素——两条路径必须逐格接近（同公式硬要求）。
#[test]
#[ignore]
fn probe_tone_cpu_gpu_parity() {
    let (width, height) = (256u32, 64u32);
    let steps: Vec<u8> = (0..width)
        .map(|x| (x as u32 * 255 / (width - 1)) as u8)
        .collect();
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..height {
        for &value in &steps {
            rgba.extend_from_slice(&[value, value, value, 255]);
        }
    }

    let params_json = r#"{"version":1,"light":{"shadows":100.0,"blacks":100.0}}"#;
    let cpu = fdxmp_core::preview::render_memory_rgba(&rgba, width, height, params_json)
        .expect("CPU 渲染失败");

    let mut params = EditParams::default();
    params.light.shadows = 100.0;
    params.light.blacks = 100.0;
    let mut session = PreviewSession::new(&rgba, width, height).expect("GPU 初始化失败");
    let (_, _, gpu) = session.render(&params).expect("GPU 渲染失败");

    println!("输入 | CPU | GPU （阴影 +100，黑色 +100）");
    let mut max_gap = 0i32;
    for index in [0usize, 1, 2, 5, 10, 20, 40, 80, 120, 200, 255] {
        let offset = index * 4;
        let gap = cpu.rgba[offset] as i32 - gpu[offset] as i32;
        max_gap = max_gap.max(gap.abs());
        println!(
            "{:3} | {:3} | {:3}  (Δ{})",
            steps[index], cpu.rgba[offset], gpu[offset], gap
        );
    }
    println!("最大差异 = {max_gap}（应 ≤ 2）");
    assert!(max_gap <= 2, "CPU / GPU 色调公式不一致：Δ={max_gap}");
}
