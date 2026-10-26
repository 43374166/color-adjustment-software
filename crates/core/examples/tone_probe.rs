//! 基本面板 / 参数曲线的响应探针（开发工具，不参与打包）。
//!
//! 对中性灰阶逐点跑当前 CPU 参考链路，输出“输入 sRGB → 输出 sRGB”，
//! 用来和 Lightroom 同参数导出做逐点对照，也用来防止后续改动悄悄改变端点。
//!
//! ```text
//! cargo run -p fdxmp-core --example tone_probe -- --csv
//! ```

use fdxmp_color::transfer::{linear_to_srgb, srgb_to_linear};
use fdxmp_core::params::EditParams;
use fdxmp_core::pipeline::apply_reference;
use fdxmp_core::tone::ToneStage;

const INPUTS: [u8; 21] = [
    0, 4, 8, 16, 24, 32, 48, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240, 248, 255,
];

struct Setting {
    name: &'static str,
    value: f32,
    apply: fn(&mut EditParams, f32),
}

fn settings() -> Vec<Setting> {
    vec![
        Setting {
            name: "exposure",
            value: -2.0,
            apply: |p, v| p.light.exposure = v,
        },
        Setting {
            name: "exposure",
            value: -1.0,
            apply: |p, v| p.light.exposure = v,
        },
        Setting {
            name: "exposure",
            value: 1.0,
            apply: |p, v| p.light.exposure = v,
        },
        Setting {
            name: "contrast",
            value: -100.0,
            apply: |p, v| p.light.contrast = v,
        },
        Setting {
            name: "contrast",
            value: -50.0,
            apply: |p, v| p.light.contrast = v,
        },
        Setting {
            name: "contrast",
            value: 50.0,
            apply: |p, v| p.light.contrast = v,
        },
        Setting {
            name: "contrast",
            value: 100.0,
            apply: |p, v| p.light.contrast = v,
        },
        Setting {
            name: "highlights",
            value: -100.0,
            apply: |p, v| p.light.highlights = v,
        },
        Setting {
            name: "highlights",
            value: -50.0,
            apply: |p, v| p.light.highlights = v,
        },
        Setting {
            name: "highlights",
            value: 50.0,
            apply: |p, v| p.light.highlights = v,
        },
        Setting {
            name: "highlights",
            value: 100.0,
            apply: |p, v| p.light.highlights = v,
        },
        Setting {
            name: "shadows",
            value: -100.0,
            apply: |p, v| p.light.shadows = v,
        },
        Setting {
            name: "shadows",
            value: -50.0,
            apply: |p, v| p.light.shadows = v,
        },
        Setting {
            name: "shadows",
            value: 50.0,
            apply: |p, v| p.light.shadows = v,
        },
        Setting {
            name: "shadows",
            value: 100.0,
            apply: |p, v| p.light.shadows = v,
        },
        Setting {
            name: "whites",
            value: -100.0,
            apply: |p, v| p.light.whites = v,
        },
        Setting {
            name: "whites",
            value: -50.0,
            apply: |p, v| p.light.whites = v,
        },
        Setting {
            name: "whites",
            value: 50.0,
            apply: |p, v| p.light.whites = v,
        },
        Setting {
            name: "whites",
            value: 100.0,
            apply: |p, v| p.light.whites = v,
        },
        Setting {
            name: "blacks",
            value: -100.0,
            apply: |p, v| p.light.blacks = v,
        },
        Setting {
            name: "blacks",
            value: -50.0,
            apply: |p, v| p.light.blacks = v,
        },
        Setting {
            name: "blacks",
            value: 50.0,
            apply: |p, v| p.light.blacks = v,
        },
        Setting {
            name: "blacks",
            value: 100.0,
            apply: |p, v| p.light.blacks = v,
        },
        Setting {
            name: "extended_tone",
            value: 0.0,
            apply: |_p, _v| {},
        },
    ]
}

/// 中性像素在当前链路中的 sRGB 输出（局部基底层按像素自身亮度，等价于大片均匀色块）。
fn render(params: &EditParams, input_srgb: u8) -> u8 {
    let linear = [srgb_to_linear(input_srgb as f32 / 255.0); 3];
    let base = apply_reference(linear, params);
    let stage = ToneStage::from_params(params);
    let base_ev = stage.self_base_ev(base);
    let out = stage.apply(base, base_ev);
    (linear_to_srgb(out[0].clamp(0.0, 1.0)) * 255.0).round() as u8
}

fn main() {
    let csv = std::env::args().any(|arg| arg == "--csv");
    if csv {
        print!("setting,value");
        for input in INPUTS {
            print!(",in{input}");
        }
        println!();
    } else {
        println!(
            "setting value | {}",
            INPUTS.map(|v| format!("{v:>3}")).join(" ")
        );
    }

    for setting in settings() {
        let mut params = EditParams::default();
        (setting.apply)(&mut params, setting.value);
        let outputs: Vec<u8> = INPUTS.iter().map(|&i| render(&params, i)).collect();
        if csv {
            print!("{},{},", setting.name, setting.value);
            for (index, out) in outputs.iter().enumerate() {
                if index > 0 {
                    print!(",");
                }
                print!("{out}");
            }
            println!();
        } else if setting.name == "extended_tone" {
            println!("(baseline: 无色调滑杆，恒等)");
        } else {
            println!(
                "{:<10} {:<6} | {}",
                setting.name,
                setting.value,
                outputs
                    .iter()
                    .map(|v| format!("{v:>3}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }
}
