//! Render a small synthetic colour chart through the CPU preview at slider
//! endpoints. Useful for eyeballing regressions (banding, inversion, clipping)
//! without opening the desktop UI.
//!
//!     cargo run -p fdxmp-core --example color_sweep -- <out-dir>

use std::path::PathBuf;

use fdxmp_core::params::{ColorBand, EditParams, GradingZone, HslBand, PointColorParams};
use fdxmp_core::preview::render_preview_image;
use image::{Rgba, RgbaImage};

fn chart(width: u32, height: u32) -> RgbaImage {
    let patches = [
        [24u8, 24, 24],
        [235, 235, 235],
        [190, 40, 35],
        [220, 150, 60],
        [230, 220, 70],
        [50, 170, 70],
        [35, 165, 175],
        [45, 80, 210],
        [135, 60, 175],
        [215, 70, 150],
        [60, 45, 35],
        [210, 175, 140],
    ];
    RgbaImage::from_fn(width, height, |x, y| {
        // Top half: 12 colour patches; bottom half: neutral ramp.
        if y < height / 2 {
            let idx = (x as usize * patches.len() / width as usize).min(patches.len() - 1);
            let c = patches[idx];
            Rgba([c[0], c[1], c[2], 255])
        } else {
            let v = (x * 255 / (width - 1).max(1)) as u8;
            Rgba([v, v, v, 255])
        }
    })
}

fn all_bands(params: &mut EditParams, field: fn(&mut HslBand) -> &mut f32, value: f32) {
    for band in [
        ColorBand::Red,
        ColorBand::Orange,
        ColorBand::Yellow,
        ColorBand::Green,
        ColorBand::Aqua,
        ColorBand::Blue,
        ColorBand::Purple,
        ColorBand::Magenta,
    ] {
        *field(params.hsl.band_mut(band)) = value;
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut next = 0;
    let input = args
        .first()
        .filter(|arg| std::path::Path::new(arg).is_file())
        .cloned();
    if input.is_some() {
        next = 1;
    }
    let out_dir = PathBuf::from(
        args.get(next)
            .cloned()
            .unwrap_or_else(|| "reports/color-sweep".into()),
    );
    std::fs::create_dir_all(&out_dir).unwrap();
    let source = if let Some(path) = &input {
        let image = image::open(path).unwrap().to_rgba8();
        let edge = image.width().max(image.height());
        if edge > 1600 {
            let scale = 1600.0 / edge as f32;
            image::imageops::resize(
                &image,
                (image.width() as f32 * scale) as u32,
                (image.height() as f32 * scale) as u32,
                image::imageops::FilterType::Triangle,
            )
        } else {
            image
        }
    } else {
        chart(768, 512)
    };

    let mut cases: Vec<(&str, EditParams)> = Vec::new();
    cases.push(("00-default", EditParams::default()));

    let mut p = EditParams::default();
    p.color.saturation = 100.0;
    cases.push(("01-saturation-plus100", p));

    let mut p = EditParams::default();
    p.color.saturation = -100.0;
    cases.push(("02-saturation-minus100", p));

    let mut p = EditParams::default();
    all_bands(&mut p, |b| &mut b.luminance, -100.0);
    cases.push(("03-hsl-luminance-minus100", p));

    let mut p = EditParams::default();
    all_bands(&mut p, |b| &mut b.luminance, 100.0);
    cases.push(("04-hsl-luminance-plus100", p));

    let mut p = EditParams::default();
    p.grading.shadows = GradingZone {
        hue: 220.0,
        saturation: 100.0,
        luminance: 100.0,
    };
    p.grading.midtones = GradingZone {
        hue: 40.0,
        saturation: 100.0,
        luminance: -100.0,
    };
    p.grading.highlights = GradingZone {
        hue: 300.0,
        saturation: 100.0,
        luminance: 100.0,
    };
    p.grading.blending = 100.0;
    p.grading.balance = 100.0;
    cases.push(("05-grading-extreme", p));

    let mut p = EditParams::default();
    p.color.temperature = -100.0;
    p.color.tint = 100.0;
    cases.push(("06-wb-cool-magenta", p));

    let mut p = EditParams::default();
    p.color.temperature = 100.0;
    p.color.tint = -100.0;
    cases.push(("07-wb-warm-green", p));

    let mut p = EditParams::default();
    p.point_colors.push(PointColorParams {
        lum: 0.0,
        chroma: 0.0,
        hue: 0.0,
        hue_shift: 100.0,
        sat_shift: 100.0,
        lum_shift: -100.0,
        variance: 100.0,
        range: 100.0,
        hue_range: 100.0,
        sat_range: 100.0,
        lum_range: 100.0,
    });
    cases.push(("08-point-extreme", p));

    let mut p = EditParams::default();
    p.calibration.red_hue = 100.0;
    p.calibration.red_sat = 100.0;
    p.calibration.green_hue = -100.0;
    p.calibration.green_sat = 100.0;
    p.calibration.blue_hue = 100.0;
    p.calibration.blue_sat = -100.0;
    p.calibration.shadows_tint = 100.0;
    cases.push(("09-calibration-extreme", p));

    for (name, params) in cases {
        let rendered = render_preview_image(&source, &params);
        let path = out_dir.join(format!("{name}.png"));
        rendered.save(&path).unwrap();
        println!("{}", path.display());
    }
}
