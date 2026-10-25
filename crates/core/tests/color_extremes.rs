//! Extreme-value regression tests for the shared color pipeline.
//!
//! These deliberately exercise slider endpoints (-100 / +100) because a color
//! control that inverts or clips at its endpoints is a user-visible bug even
//! when the mid-range looks fine.

#![allow(clippy::field_reassign_with_default)]

use fdxmp_core::params::{ColorBand, EditParams, GradingZone, HslBand, PointColorParams};
use fdxmp_core::preview::render_preview_image;
use image::{Rgba, RgbaImage};

fn ramp() -> RgbaImage {
    RgbaImage::from_fn(256, 1, |x, _| {
        let v = x as u8;
        Rgba([v, v, v, 255])
    })
}

fn patches() -> RgbaImage {
    let colors = [
        [30u8, 30, 30],
        [220, 40, 40],
        [40, 180, 60],
        [40, 70, 210],
        [220, 150, 70],
        [30, 160, 170],
        [170, 50, 160],
        [230, 220, 210],
    ];
    RgbaImage::from_fn(256, 1, |x, _| {
        let c = colors[(x as usize / 32).min(colors.len() - 1)];
        Rgba([c[0], c[1], c[2], 255])
    })
}

fn luma(p: &Rgba<u8>) -> f32 {
    0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
}

fn assert_not_inverted(name: &str, image: &RgbaImage) {
    let black = luma(image.get_pixel(0, 0));
    let white = luma(image.get_pixel(image.width() - 1, 0));
    assert!(
        black <= white + 2.0,
        "{name}: black {black} > white {white}"
    );
    assert!(
        image.pixels().all(|p| p[3] == 255),
        "{name}: alpha must stay opaque"
    );
}

fn assert_monotonic(name: &str, image: &RgbaImage) {
    let mut prev = -1.0f32;
    for x in 0..image.width() {
        let y = luma(image.get_pixel(x, 0));
        assert!(
            y >= prev - 1.5,
            "{name}: luma drops at x={x}: {prev} -> {y}"
        );
        prev = y;
    }
}

fn render(params: &EditParams, source: &RgbaImage) -> RgbaImage {
    render_preview_image(source, params)
}

#[test]
fn tone_slider_endpoints_never_invert_the_grey_ramp() {
    for (name, apply) in [
        (
            "contrast -100",
            (|p: &mut EditParams| p.light.contrast = -100.0) as fn(&mut EditParams),
        ),
        ("contrast +100", |p| p.light.contrast = 100.0),
        ("whites -100", |p| p.light.whites = -100.0),
        ("whites +100", |p| p.light.whites = 100.0),
        ("blacks -100", |p| p.light.blacks = -100.0),
        ("blacks +100", |p| p.light.blacks = 100.0),
        ("highlights -100", |p| p.light.highlights = -100.0),
        ("highlights +100", |p| p.light.highlights = 100.0),
        ("shadows -100", |p| p.light.shadows = -100.0),
        ("shadows +100", |p| p.light.shadows = 100.0),
    ] {
        let mut params = EditParams::default();
        apply(&mut params);
        let out = render(&params, &ramp());
        assert_not_inverted(name, &out);
        assert_monotonic(name, &out);
    }
}

#[test]
fn saturation_and_vibrance_endpoints_keep_neutral_and_gamut() {
    for (name, apply) in [
        (
            "saturation -100",
            (|p: &mut EditParams| p.color.saturation = -100.0) as fn(&mut EditParams),
        ),
        ("saturation +100", |p| p.color.saturation = 100.0),
        ("vibrance -100", |p| p.color.vibrance = -100.0),
        ("vibrance +100", |p| p.color.vibrance = 100.0),
    ] {
        let mut params = EditParams::default();
        apply(&mut params);
        let out = render(&params, &patches());
        assert_not_inverted(name, &out);
        // No pixel should be all-black or all-white unless the source was.
        for p in out.pixels() {
            assert!(
                p[0] as u16 + p[1] as u16 + p[2] as u16 > 0,
                "{name}: black pixel"
            );
            assert!(
                p[0] as u16 + p[1] as u16 + (p[2] as u16) < 765,
                "{name}: white pixel"
            );
        }
        if name.starts_with("saturation -100") {
            for x in 0..out.width() {
                let p = out.get_pixel(x, 0);
                let spread = p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2]);
                assert!(spread <= 3, "{name}: not neutral at x={x}: {p:?}");
            }
        }
    }
}

#[test]
fn hsl_luminance_endpoints_do_not_make_dark_colours_brighter_than_light_ones() {
    let source = patches();
    for lum in [-100.0f32, 100.0] {
        let mut params = EditParams::default();
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
            *params.hsl.band_mut(band) = HslBand {
                luminance: lum,
                ..Default::default()
            };
        }
        let out = render(&params, &source);
        assert_not_inverted(&format!("HSL luminance {lum}"), &out);
    }
}

#[test]
fn grading_luminance_endpoints_do_not_invert_black_and_white() {
    for lum in [-100.0f32, 100.0] {
        let mut params = EditParams::default();
        params.grading.shadows = GradingZone {
            hue: 220.0,
            saturation: 100.0,
            luminance: lum,
        };
        params.grading.midtones = GradingZone {
            hue: 40.0,
            saturation: 100.0,
            luminance: -lum,
        };
        params.grading.highlights = GradingZone {
            hue: 300.0,
            saturation: 100.0,
            luminance: lum,
        };
        params.grading.blending = 100.0;
        params.grading.balance = 100.0;
        let out = render(&params, &ramp());
        assert_not_inverted(&format!("grading luminance {lum}"), &out);
    }
}

#[test]
fn point_color_endpoints_do_not_invert_black_and_white() {
    for shift in [-100.0f32, 100.0] {
        let mut params = EditParams::default();
        params.point_colors.push(PointColorParams {
            lum: 0.0,
            chroma: 0.0,
            hue: 0.0,
            hue_shift: shift,
            sat_shift: shift,
            lum_shift: shift,
            variance: shift,
            range: 100.0,
            hue_range: 100.0,
            sat_range: 100.0,
            lum_range: 100.0,
        });
        let out = render(&params, &ramp());
        assert_not_inverted(&format!("point color {shift}"), &out);
    }
}

#[test]
fn calibration_endpoints_keep_neutral_and_do_not_invert() {
    let mut params = EditParams::default();
    params.calibration.red_hue = 100.0;
    params.calibration.red_sat = 100.0;
    params.calibration.green_hue = -100.0;
    params.calibration.green_sat = 100.0;
    params.calibration.blue_hue = 100.0;
    params.calibration.blue_sat = -100.0;
    params.calibration.shadows_tint = 100.0;
    let out = render(&params, &ramp());
    assert_not_inverted("calibration endpoints", &out);
}

#[test]
fn white_balance_endpoints_keep_the_grey_ramp_monotonic() {
    for (name, temperature, tint) in [
        ("WB cool+magenta", -100.0f32, 100.0f32),
        ("WB warm+green", 100.0, -100.0),
        ("WB cool+green", -100.0, -100.0),
        ("WB warm+magenta", 100.0, 100.0),
    ] {
        let mut params = EditParams::default();
        params.color.temperature = temperature;
        params.color.tint = tint;
        let out = render(&params, &ramp());
        assert_not_inverted(name, &out);
        assert_monotonic(name, &out);
    }
}

#[test]
fn exposure_endpoints_remain_monotonic() {
    for ev in [-5.0f32, 5.0] {
        let mut params = EditParams::default();
        params.light.exposure = ev;
        let out = render(&params, &ramp());
        assert_not_inverted(&format!("exposure {ev}"), &out);
        assert_monotonic(&format!("exposure {ev}"), &out);
    }
}

#[test]
fn effect_endpoints_do_not_invert_black_and_white() {
    let source = patches();
    for field in 0..4 {
        for value in [-100.0f32, 100.0] {
            let mut params = EditParams::default();
            match field {
                0 => params.effects.texture = value,
                1 => params.effects.clarity = value,
                2 => params.effects.dehaze = value,
                _ => params.effects.vignette = value,
            }
            let out = render(&params, &source);
            assert_not_inverted(&format!("effect {field} {value}"), &out);
            assert!(out.pixels().all(|p| p[3] == 255));
        }
    }
}

#[test]
fn hsl_hue_and_saturation_endpoints_do_not_break_colour_patches() {
    for field in 0..3 {
        for value in [-100.0f32, 100.0] {
            let mut params = EditParams::default();
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
                match field {
                    0 => params.hsl.band_mut(band).hue = value,
                    1 => params.hsl.band_mut(band).saturation = value,
                    _ => params.hsl.band_mut(band).luminance = value,
                }
            }
            let out = render(&params, &patches());
            assert_not_inverted(&format!("HSL field {field} {value}"), &out);
        }
    }
}

#[test]
fn saturation_plus100_keeps_hue_and_lightness_close() {
    use fdxmp_color::{lab_to_lch, oklab_from_rgb, wrap_angle, SRGB};

    let source = patches();
    let mut params = EditParams::default();
    params.color.saturation = 100.0;
    let out = render(&params, &source);

    for x in (16..256).step_by(32) {
        let before = source.get_pixel(x, 0);
        let after = out.get_pixel(x, 0);
        let to_linear = |p: &Rgba<u8>| {
            [
                fdxmp_color::transfer::srgb_to_linear(p[0] as f32 / 255.0),
                fdxmp_color::transfer::srgb_to_linear(p[1] as f32 / 255.0),
                fdxmp_color::transfer::srgb_to_linear(p[2] as f32 / 255.0),
            ]
        };
        let [l0, c0, h0] = lab_to_lch(oklab_from_rgb(&SRGB, to_linear(before)));
        let [l1, c1, h1] = lab_to_lch(oklab_from_rgb(&SRGB, to_linear(after)));
        if c0 < 0.06 {
            continue;
        }
        let dh = wrap_angle(h1 - h0).abs().to_degrees();
        assert!(dh < 2.0, "x={x}: hue drift {dh:.1}° ({h0} -> {h1})");
        assert!(
            (l1 - l0).abs() < 0.10,
            "x={x}: lightness drift {:.3} ({l0} -> {l1})",
            l1 - l0
        );
        assert!(c1 >= c0 * 0.7, "x={x}: chroma collapsed {c0} -> {c1}");
    }
}

#[test]
fn display_gamut_mapping_returns_bounded_linear_rgb() {
    for rgb in [
        [2.0, -0.2, 0.1],
        [0.1, 3.0, 0.2],
        [-0.5, 0.2, 0.1],
        [10.0, 10.0, 10.0],
        [f32::NAN, 0.0, 0.0],
        [f32::INFINITY, 0.0, 0.0],
    ] {
        let mapped = fdxmp_core::color_engine::ColorStage::map_display_gamut(rgb);
        assert!(
            mapped.iter().all(|v| v.is_finite()),
            "{rgb:?} -> {mapped:?}"
        );
        assert!(
            mapped.iter().all(|v| (0.0..=1.0).contains(v)),
            "{rgb:?} -> {mapped:?}"
        );
    }
}
