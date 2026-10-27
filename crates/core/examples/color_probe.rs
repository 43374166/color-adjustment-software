//! 颜色面板 / 8 色带 HSL 的响应探针（开发工具，不参与打包）。
//!
//! 输出几个代表色在单个滑杆 ±100 时的 OkLCh 变化（ΔL / ΔC / Δh），
//! 以及灰阶在白平衡滑块下的 RGB 偏移。用于和 Lightroom 同参数导出对照。
//!
//! ```text
//! cargo run -p fdxmp-core --example color_probe
//! ```

use fdxmp_color::transfer::{linear_to_srgb, srgb_to_linear};
use fdxmp_color::{lab_to_lch, oklab_from_rgb, ColorOps, SRGB};
use fdxmp_core::params::{ColorBand, EditParams};
use fdxmp_core::pipeline::apply_reference;

const PATCHES: [(&str, [u8; 3]); 8] = [
    ("grey50", [128, 128, 128]),
    ("skin", [224, 172, 140]),
    ("red", [220, 40, 35]),
    ("orange", [230, 140, 40]),
    ("yellow", [230, 220, 60]),
    ("green", [60, 170, 80]),
    ("blue", [45, 95, 210]),
    ("magenta", [200, 60, 180]),
];

fn patch_linear(rgb: [u8; 3]) -> [f32; 3] {
    rgb.map(|v| srgb_to_linear(v as f32 / 255.0))
}

fn oklch(rgb: [f32; 3]) -> [f32; 3] {
    lab_to_lch(oklab_from_rgb(&SRGB, rgb))
}

fn params_to_ops(params: &EditParams) -> ColorOps {
    let mut ops = ColorOps::new(SRGB);
    ops.vibrance = params.color.vibrance as f64 / 100.0;
    ops.saturation = params.color.saturation as f64 / 100.0;
    let hsl = params.hsl.band(ColorBand::Red);
    if [hsl.hue, hsl.saturation, hsl.luminance]
        .iter()
        .any(|v| *v != 0.0)
    {
        let bands = ColorBand::ALL.map(|band| {
            let b = params.hsl.band(band);
            fdxmp_color::Hsl {
                hue: b.hue as f64,
                sat: b.saturation as f64,
                lum: b.luminance as f64,
            }
        });
        ops.mixer = Some(fdxmp_color::Mixer { bands });
    }
    ops
}

fn main() {
    println!("== 饱和度 / 自然饱和度（OkLCh C 变化）==");
    for slider in ["saturation", "vibrance"] {
        for value in [-100.0f32, 100.0] {
            let mut params = EditParams::default();
            if slider == "saturation" {
                params.color.saturation = value;
            } else {
                params.color.vibrance = value;
            }
            println!("-- {slider} {value:+.0}");
            for (name, patch) in PATCHES {
                let linear = patch_linear(patch);
                let before = oklch(linear);
                let after = oklch(params_to_ops(&params).apply(linear));
                println!(
                    "{name:<8} C {:.3} -> {:.3}  ({:+.0}%)   L {:.3} -> {:.3}",
                    before[1],
                    after[1],
                    (after[1] / before[1].max(1e-6) - 1.0) * 100.0,
                    before[0],
                    after[0]
                );
            }
        }
    }

    println!("\n== 色温 / 色调（中灰 128 的线性 RGB 与 sRGB8）==");
    for (label, temp, tint) in [
        ("temp -100", -100.0, 0.0),
        ("temp +100", 100.0, 0.0),
        ("tint -100", 0.0, -100.0),
        ("tint +100", 0.0, 100.0),
    ] {
        let mut params = EditParams::default();
        params.color.temperature = temp;
        params.color.tint = tint;
        let linear = patch_linear([128, 128, 128]);
        let out = apply_reference(linear, &params);
        println!(
            "{label:<10} linear [{:.4}, {:.4}, {:.4}]  sRGB8 {:?}",
            out[0],
            out[1],
            out[2],
            out.map(|v| (linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round() as u8)
        );
    }

    println!("\n== 8 色带 HSL：红带 ±100 对纯红 (220,40,35) 的影响 ==");
    for (field, values) in [
        ("hue", [-100.0f32, 100.0]),
        ("saturation", [-100.0, 100.0]),
        ("luminance", [-100.0, 100.0]),
    ] {
        for value in values {
            let mut params = EditParams::default();
            let band = params.hsl.band_mut(ColorBand::Red);
            match field {
                "hue" => band.hue = value,
                "saturation" => band.saturation = value,
                _ => band.luminance = value,
            }
            let linear = patch_linear([220, 40, 35]);
            let before = oklch(linear);
            let after = oklch(params_to_ops(&params).apply(linear));
            println!(
                "{field:<10} {value:+.0}: L {:.3} -> {:.3}  C {:.3} -> {:.3}  h {:.1}° -> {:.1}°",
                before[0],
                after[0],
                before[1],
                after[1],
                before[2].to_degrees(),
                after[2].to_degrees()
            );
        }
    }
}
