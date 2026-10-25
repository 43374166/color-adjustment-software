//! Broad endpoint sweeps for the colour operators.
//!
//! The goal is not to pin a particular "look" but to guarantee that every
//! slider endpoint produces finite, display-mappable colour. A regression here
//! means a user can drag a slider to ±100 and get NaN, negative channels or a
//! hard colour inversion.

use fdxmp_color::{ColorOps, Grading, Hsl, Mixer, PointColor, Wheel, REC2020};

fn luma() -> [f32; 3] {
    REC2020.luma().map(|v| v as f32)
}

fn assert_displayable(name: &str, rgb: [f32; 3], out: [f32; 3]) {
    assert!(
        out.iter().all(|v| v.is_finite()),
        "{name}: {rgb:?} -> {out:?}"
    );
    let (mapped, _) = fdxmp_color::gamut::map_to_gamut(out, luma());
    assert!(
        mapped.iter().all(|v| (0.0..=1.0).contains(v)),
        "{name}: {rgb:?} -> {out:?} -> {mapped:?}"
    );
}

fn colours() -> [[f32; 3]; 10] {
    [
        [0.0, 0.0, 0.0],
        [0.001, 0.001, 0.001],
        [0.18, 0.18, 0.18],
        [1.0, 1.0, 1.0],
        [0.8, 0.02, 0.01],
        [0.02, 0.7, 0.02],
        [0.01, 0.02, 0.8],
        [0.35, 0.16, 0.09],
        [0.02, 0.5, 0.5],
        [2.0, 0.1, -0.2],
    ]
}

#[test]
fn every_mixer_endpoint_is_displayable() {
    for band in 0..8 {
        for endpoint in [-100.0f64, 100.0] {
            for field in 0..3 {
                let mut hsl = Hsl::default();
                match field {
                    0 => hsl.hue = endpoint,
                    1 => hsl.sat = endpoint,
                    _ => hsl.lum = endpoint,
                }
                let mut mixer = Mixer::default();
                mixer.bands[band] = hsl;
                let mut o = ColorOps::new(REC2020);
                o.mixer = Some(mixer);
                for rgb in colours() {
                    assert_displayable(
                        &format!("mixer band {band} field {field} {endpoint}"),
                        rgb,
                        o.apply(rgb),
                    );
                }
            }
        }
    }
}

#[test]
fn every_point_color_endpoint_is_displayable() {
    let base = fdxmp_color::lab_to_lch(fdxmp_color::oklab_from_rgb(&REC2020, [0.35, 0.16, 0.09]));
    let samples = [
        PointColor {
            lum: base[0] as f64,
            chroma: base[1] as f64,
            hue: (base[2] as f64).to_degrees(),
            ..Default::default()
        },
        PointColor {
            lum: 0.0,
            chroma: 0.0,
            hue: 0.0,
            ..Default::default()
        },
    ];
    for sample in samples {
        for endpoint in [-100.0f64, 100.0] {
            for field in 0..6 {
                let mut p = sample;
                match field {
                    0 => p.hue_shift = endpoint,
                    1 => p.sat_shift = endpoint,
                    2 => p.lum_shift = endpoint,
                    3 => p.variance = endpoint,
                    4 => p.range = endpoint,
                    _ => p.lum_range = endpoint,
                }
                let mut o = ColorOps::new(REC2020);
                o.points.push(p);
                for rgb in colours() {
                    assert_displayable(
                        &format!("point field {field} {endpoint}"),
                        rgb,
                        o.apply(rgb),
                    );
                }
            }
        }
    }
}

#[test]
fn every_grading_endpoint_is_displayable() {
    for zone in 0..3 {
        for endpoint in [-100.0f64, 100.0] {
            let wheel = Wheel {
                hue: 220.0,
                sat: if zone == 0 { endpoint } else { 100.0 },
                lum: if zone == 0 { 100.0 } else { endpoint },
            };
            let mut g = Grading {
                blending: 100.0,
                balance: endpoint,
                ..Default::default()
            };
            match zone {
                0 => g.shadows = wheel,
                1 => g.midtones = wheel,
                _ => g.highlights = wheel,
            }
            let mut o = ColorOps::new(REC2020);
            o.grading = Some(g);
            for rgb in colours() {
                assert_displayable(
                    &format!("grading zone {zone} {endpoint}"),
                    rgb,
                    o.apply(rgb),
                );
            }
        }
    }
}
