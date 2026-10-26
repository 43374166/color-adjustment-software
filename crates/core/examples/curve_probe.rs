//! 曲线面板响应探针（开发工具，不参与打包）。
//!
//! 用 256 级灰阶跑 CPU 预览路径，输出参数曲线 / RGB 曲线 / 单通道曲线的
//! “输入 → 输出”，用于和 Lightroom 曲线导出以及 GPU 路径对照。
//!
//! ```text
//! cargo run -p fdxmp-core --example curve_probe
//! ```

use fdxmp_core::params::{CurvePoint, EditParams, ToneCurve};
use fdxmp_core::preview::render_preview_image;
use image::RgbaImage;

const INPUTS: [u8; 17] = [
    0, 16, 32, 48, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240, 255,
];

fn ramp() -> RgbaImage {
    let mut image = RgbaImage::new(256, 32);
    for y in 0..image.height() {
        for x in 0..image.width() {
            let value = x as u8;
            image.put_pixel(x, y, image::Rgba([value, value, value, 255]));
        }
    }
    image
}

fn render(params: &EditParams) -> Vec<u8> {
    let source = ramp();
    let out = render_preview_image(&source, params);
    INPUTS
        .iter()
        .map(|&x| out.get_pixel(x as u32, 16).0[0])
        .collect()
}

fn report(name: &str, params: &EditParams) {
    let out = render(params);
    println!(
        "{name:<28} | {}",
        out.iter()
            .map(|v| format!("{v:>3}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
}

fn main() {
    println!(
        "{:<28} | {}",
        "input",
        INPUTS
            .iter()
            .map(|v| format!("{v:>3}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    report("baseline", &EditParams::default());

    for (name, field) in [
        ("param shadows", 0usize),
        ("param darks", 1),
        ("param lights", 2),
        ("param highlights", 3),
    ] {
        for value in [-100.0f32, -50.0, 50.0, 100.0] {
            let mut params = EditParams::default();
            match field {
                0 => params.curves.parametric.shadows = value,
                1 => params.curves.parametric.darks = value,
                2 => params.curves.parametric.lights = value,
                _ => params.curves.parametric.highlights = value,
            }
            report(&format!("{name} {value:+.0}"), &params);
        }
    }

    for (name, value) in [
        ("rgb point 0.25->0.40", 0.40f32),
        ("rgb point 0.25->0.10", 0.10),
    ] {
        let mut params = EditParams::default();
        params.curves.rgb = ToneCurve {
            points: vec![
                CurvePoint { x: 0.0, y: 0.0 },
                CurvePoint { x: 0.25, y: value },
                CurvePoint { x: 1.0, y: 1.0 },
            ],
        };
        report(name, &params);
    }

    let mut params = EditParams::default();
    params.curves.red = ToneCurve {
        points: vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 0.5, y: 0.35 },
            CurvePoint { x: 1.0, y: 1.0 },
        ],
    };
    report("red channel midpoint -0.15", &params);
}
