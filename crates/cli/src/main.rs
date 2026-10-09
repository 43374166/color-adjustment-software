//! fdxmp 命令行测试工具。
//!
//! ```text
//! fdxmp-cli version
//! fdxmp-cli probe <image>
//! fdxmp-cli decode <image> [--preview]
//! fdxmp-cli render <in> <out.png> [--exposure 1.0 --saturation -30 ...]   # 需要 gpu feature
//! fdxmp-cli preset-new <out.fdxmp> <name> [author]
//! fdxmp-cli preset-info <preset.fdxmp>
//! fdxmp-cli cache-size <dir>
//! fdxmp-cli cache-clear <dir>
//! ```

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use fdxmp_core::cache::CacheManager;
use fdxmp_core::params::EditParams;
use fdxmp_core::preset::PresetFile;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("错误：{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let (command, rest) = args.split_first().ok_or_else(usage)?;
    match command.as_str() {
        "version" => {
            println!("fdxmp-core {}", fdxmp_core::version());
            println!("ABI {}", fdxmp_core::ABI_VERSION);
            Ok(())
        }
        "probe" => {
            let path = rest.first().ok_or_else(usage)?;
            let probe = fdxmp_core::decode::probe(path).map_err(|err| err.to_string())?;
            let json = serde_json::to_string_pretty(&probe).map_err(|err| err.to_string())?;
            println!("{json}");
            Ok(())
        }
        "decode" => {
            let path = rest.first().ok_or_else(usage)?;
            let preview = rest.iter().any(|arg| arg == "--preview");
            let decoded = if preview {
                fdxmp_core::decode::decode_preview(path)
            } else {
                fdxmp_core::decode::decode(path)
            }
            .map_err(|err| err.to_string())?;

            println!("格式：{}", decoded.format.name());
            println!("来源：{}", decoded.source.label_zh());
            println!(
                "尺寸：{}×{}（{:.1} MP）",
                decoded.width,
                decoded.height,
                decoded.megapixels()
            );
            println!("像素类型：{:?}", decoded.pixels.color());
            println!("耗时：{} ms", decoded.elapsed_ms);
            if let Some(metadata) = &decoded.metadata {
                println!("元数据：{}", metadata.summary());
                if let Some(captured) = &metadata.captured_at {
                    println!("拍摄时间：{captured}");
                }
            }
            Ok(())
        }
        "render" => render_command(rest),
        "preset-builtins" => {
            let dir = rest
                .first()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("presets"));
            std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
            for preset in fdxmp_core::preset::builtin_presets() {
                let path = dir.join(format!("{}.fdxmp", preset.name));
                preset.save(&path).map_err(|err| err.to_string())?;
                println!("已生成 {}", path.display());
            }
            Ok(())
        }
        "preset-new" => {
            let out = rest.first().ok_or_else(usage)?;
            let name = rest.get(1).ok_or_else(usage)?;
            let author = rest.get(2).cloned().unwrap_or_else(|| "fdxmp".to_string());
            let preset = PresetFile::new(name.clone(), author, EditParams::default());
            preset
                .save(PathBuf::from(out))
                .map_err(|err| err.to_string())?;
            println!("已生成预设：{out}");
            Ok(())
        }
        "preset-info" => {
            let path = rest.first().ok_or_else(usage)?;
            let preset = PresetFile::load(path).map_err(|err| err.to_string())?;
            println!("名称：{}", preset.name);
            println!("作者：{}", preset.author);
            println!("创建时间：{}", preset.created_at);
            println!("schema_version：{}", preset.schema_version);
            println!("内嵌 LUT：{}", preset.luts.len());
            let json =
                serde_json::to_string_pretty(&preset.params).map_err(|err| err.to_string())?;
            println!("{json}");
            Ok(())
        }
        "cache-size" => {
            let dir = rest.first().ok_or_else(usage)?;
            let cache = CacheManager::new(dir);
            let total = cache.total_size().map_err(|err| err.to_string())?;
            println!("{}（{} 字节）", CacheManager::human_size(total), total);
            for kind in fdxmp_core::cache::CacheKind::ALL {
                let size = cache.size_of(kind).map_err(|err| err.to_string())?;
                println!("  {}：{}", kind.label_zh(), CacheManager::human_size(size));
            }
            Ok(())
        }
        "cache-clear" => {
            let dir = rest.first().ok_or_else(usage)?;
            let cache = CacheManager::new(dir);
            let freed = cache.clear_all().map_err(|err| err.to_string())?;
            println!("已清除：{}", CacheManager::human_size(freed));
            Ok(())
        }
        "--help" | "-h" | "help" => Err(usage()),
        other => Err(format!("未知命令 `{other}`\n\n{}", usage())),
    }
}

fn usage() -> String {
    "用法：
  fdxmp-cli version
  fdxmp-cli probe <image>
  fdxmp-cli decode <image> [--preview]
  fdxmp-cli render <in> <out.png> [--exposure 1.0 --contrast 20 --saturation -30 ...]
  fdxmp-cli preset-new <out.fdxmp> <name> [author]
  fdxmp-cli preset-info <preset.fdxmp>
  fdxmp-cli cache-size <dir>
  fdxmp-cli cache-clear <dir>"
        .to_string()
}

/// 阶段 2：headless GPU 渲染（验证色彩管线 / 生成对比图）。
#[cfg(feature = "gpu")]
fn render_command(args: &[String]) -> Result<(), String> {
    use fdxmp_core::gpu::GpuContext;
    use fdxmp_core::gpu_pipeline::ColorPipeline;
    use fdxmp_core::params::EditParams;

    let input = args.first().ok_or_else(usage)?;
    let output = args.get(1).ok_or_else(usage)?;

    let mut params = EditParams::default();
    let mut index = 2;
    while index + 1 < args.len() {
        let key = args[index].trim_start_matches("--");
        let value: f32 = args[index + 1]
            .parse()
            .map_err(|_| format!("参数 {} 需要数字，得到 {}", key, args[index + 1]))?;
        apply_slider(&mut params, key, value).ok_or_else(|| format!("未知参数 `--{key}`"))?;
        index += 2;
    }

    let decoded = fdxmp_core::decode::decode(input).map_err(|err| err.to_string())?;
    let proxy = decoded.display_proxy(2048);
    let context = GpuContext::new_blocking().map_err(|err| err.to_string())?;
    let mut pipeline = ColorPipeline::new(&context.device);
    let image = pipeline
        .upload(
            &context.device,
            &context.queue,
            proxy.as_raw(),
            proxy.width(),
            proxy.height(),
        )
        .map_err(|err| err.to_string())?;

    let started = std::time::Instant::now();
    pipeline.render(&context.device, &context.queue, &image, &params);
    let elapsed = started.elapsed();
    let pixels = pipeline
        .readback(&context.device, &context.queue, &image)
        .map_err(|err| err.to_string())?;
    let rendered = image::RgbaImage::from_raw(proxy.width(), proxy.height(), pixels)
        .ok_or_else(|| "渲染结果尺寸不匹配".to_string())?;
    rendered.save(output).map_err(|err| err.to_string())?;

    println!(
        "已渲染 {}×{} → {}（GPU {}，后端 {}，耗时 {:.1} ms）",
        proxy.width(),
        proxy.height(),
        output,
        context.adapter_info.name,
        context.backend_name(),
        elapsed.as_secs_f64() * 1000.0
    );
    Ok(())
}

#[cfg(not(feature = "gpu"))]
fn render_command(_args: &[String]) -> Result<(), String> {
    Err(
        "render 需要启用 gpu feature：cargo run -p fdxmp-cli --features gpu -- render ..."
            .to_string(),
    )
}

/// 命令行参数 → 调色参数。
#[cfg(feature = "gpu")]
fn apply_slider(params: &mut fdxmp_core::params::EditParams, key: &str, value: f32) -> Option<()> {
    use fdxmp_core::params::SliderId;
    let slider = match key {
        "exposure" => SliderId::Exposure,
        "contrast" => SliderId::Contrast,
        "highlights" => SliderId::Highlights,
        "shadows" => SliderId::Shadows,
        "whites" => SliderId::Whites,
        "blacks" => SliderId::Blacks,
        "temperature" => SliderId::Temperature,
        "tint" => SliderId::Tint,
        "vibrance" => SliderId::Vibrance,
        "saturation" => SliderId::Saturation,
        "texture" => SliderId::Texture,
        "clarity" => SliderId::Clarity,
        "dehaze" => SliderId::Dehaze,
        "vignette" => SliderId::Vignette,
        "grain" => SliderId::Grain,
        "sharpen" => {
            params.detail.sharpen.amount = value;
            return Some(());
        }
        "nr-luminance" => {
            params.detail.noise.luminance = value;
            return Some(());
        }
        "nr-color" => {
            params.detail.noise.color = value;
            return Some(());
        }
        "grading-blending" => {
            params.grading.blending = value;
            return Some(());
        }
        "grading-balance" => {
            params.grading.balance = value;
            return Some(());
        }
        _ => return None,
    };
    params.set_slider(slider, value);
    Some(())
}
