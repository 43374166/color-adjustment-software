//! 用文件里的参数跑一遍颜色均衡器，输入输出都是裸 `f32` 交错 RGB（线性）。
//!
//! 这是给「与 darktable 对拍」用的开发工具：Python 负责解码/编码图片，
//! 这里只负责算法，避免给 `fdxmp-color` 引入图像解码依赖。
//!
//! ```text
//! cargo run --release -p fdxmp-color --example equalize_raw -- \
//!     in.raw out.raw 7008 4672 3 params.txt
//! ```
//!
//! `params.txt` 是 `key=value` 行，缺省项用默认值：
//!
//! ```text
//! space=rec2020          # srgb | rec2020，输入先转到该空间再算
//! threshold=0.1
//! smoothing_hue=1.0
//! contrast=0.0
//! white_level=1.0
//! chroma_size=1.5
//! param_size=1.0
//! use_filter=1
//! hue_shift=0.0
//! sat=1,1,1,1,1,1,1,1
//! hue=0,0,-49.23,0,0,0,0,0
//! bright=1,1,1,1,1,1,1,1
//! ```

use std::io::{Read, Write};
use std::time::Instant;

use fdxmp_color::{ColorEqualizer, EqualizerParams, RgbSpace, NODES, REC2020, SRGB};

fn parse_triplet(text: &str) -> [f32; NODES] {
    let mut out = [1.0f32; NODES];
    for (i, part) in text.split(',').take(NODES).enumerate() {
        out[i] = part.trim().parse().unwrap_or(1.0);
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 7 {
        eprintln!(
            "usage: {} <in.raw> <out.raw> <width> <height> <channels> <params.txt>",
            args[0]
        );
        std::process::exit(2);
    }

    let width: usize = args[3].parse().expect("width");
    let height: usize = args[4].parse().expect("height");
    let channels: usize = args[5].parse().expect("channels");
    let params_text = std::fs::read_to_string(&args[6]).expect("params file");
    // PowerShell 的 Set-Content 会带 BOM，这里容错。
    let params_text = params_text.trim_start_matches('\u{feff}');

    let mut params = EqualizerParams::default();
    let mut space_name = String::from("srgb");
    for line in params_text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "space" => space_name = value.to_string(),
            "threshold" => params.threshold = value.parse().unwrap_or(params.threshold),
            "smoothing_hue" => params.smoothing_hue = value.parse().unwrap_or(params.smoothing_hue),
            "contrast" => params.contrast = value.parse().unwrap_or(params.contrast),
            "white_level" => params.white_level = value.parse().unwrap_or(params.white_level),
            "chroma_size" => params.chroma_size = value.parse().unwrap_or(params.chroma_size),
            "param_size" => params.param_size = value.parse().unwrap_or(params.param_size),
            "use_filter" => params.use_filter = value != "0" && !value.eq_ignore_ascii_case("false"),
            "hue_shift" => params.hue_shift = value.parse().unwrap_or(params.hue_shift),
            "sat" => params.saturation = parse_triplet(value),
            "hue" => params.hue = parse_triplet(value),
            "bright" => params.brightness = parse_triplet(value),
            other => eprintln!("忽略未知参数 {other}"),
        }
    }

    let space: RgbSpace = if space_name.eq_ignore_ascii_case("rec2020") {
        REC2020
    } else {
        SRGB
    };

    let mut raw = vec![0u8; width * height * channels * 4];
    let mut file = std::fs::File::open(&args[1]).expect("open input");
    let read = file.read(&mut raw).expect("read input");
    if read != raw.len() {
        eprintln!(
            "输入只有 {read} 字节，按 {width}x{height}x{channels} 需要 {}（注意 width 是每行像素数，不要写反）",
            raw.len()
        );
        std::process::exit(3);
    }

    let mut pixels: Vec<f32> = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    drop(raw);

    // 线性 sRGB → 目标工作空间
    let to_space = SRGB.to_space(&space).to_f32();
    let from_space = space.to_space(&SRGB).to_f32();
    if space != SRGB {
        for px in pixels.chunks_exact_mut(channels) {
            let (r, g, b) = (px[0], px[1], px[2]);
            px[0] = to_space[0][0] * r + to_space[0][1] * g + to_space[0][2] * b;
            px[1] = to_space[1][0] * r + to_space[1][1] * g + to_space[1][2] * b;
            px[2] = to_space[2][0] * r + to_space[2][1] * g + to_space[2][2] * b;
        }
    }

    let eq = ColorEqualizer::new(params, &space);
    let start = Instant::now();
    eq.apply(&mut pixels, width, height, channels);
    eprintln!("equalize: {:?} ({}x{}, space={})", start.elapsed(), width, height, space.name);

    if space != SRGB {
        for px in pixels.chunks_exact_mut(channels) {
            let (r, g, b) = (px[0], px[1], px[2]);
            px[0] = from_space[0][0] * r + from_space[0][1] * g + from_space[0][2] * b;
            px[1] = from_space[1][0] * r + from_space[1][1] * g + from_space[1][2] * b;
            px[2] = from_space[2][0] * r + from_space[2][1] * g + from_space[2][2] * b;
        }
    }

    let mut out = Vec::with_capacity(pixels.len() * 4);
    for v in &pixels {
        out.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::File::create(&args[2])
        .expect("create output")
        .write_all(&out)
        .expect("write output");
}
