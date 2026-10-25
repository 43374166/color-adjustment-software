//! Experimental: decode a RAW without LibRaw's camera matrix/WB so a DCP
//! profile can be applied in camera-native RGB.
//!
//! Usage:
//!   cargo run -p fdxmp-core --example raw_camera --features raw -- in.ARW out.png

#[cfg(feature = "raw")]
fn main() -> Result<(), String> {
    use std::path::PathBuf;

    let mut args = std::env::args().skip(1);
    let input = args.next().ok_or("usage: raw_camera <in.ARW> <out.png>")?;
    let output = args.next().ok_or("usage: raw_camera <in.ARW> <out.png>")?;
    let bytes = std::fs::read(&input).map_err(|err| err.to_string())?;
    let mut raw = rsraw::RawImage::open(&bytes).map_err(|err| err.to_string())?;
    raw.set_use_camera_wb(false);
    raw.set_use_camera_matrix(false);
    raw.unpack().map_err(|err| err.to_string())?;
    let processed = raw
        .process::<{ rsraw::BIT_DEPTH_16 }>()
        .map_err(|err| err.to_string())?;
    let width = processed.width();
    let height = processed.height();
    let colors = processed.colors();
    let data: &[u16] = &processed;
    let image = match colors {
        3 => image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_raw(width, height, data.to_vec())
            .map(image::DynamicImage::ImageRgb16),
        4 => image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(width, height, data.to_vec())
            .map(image::DynamicImage::ImageRgba16),
        other => return Err(format!("unsupported channels: {other}")),
    }
    .ok_or("image buffer size mismatch")?;
    image.save(&output).map_err(|err| err.to_string())?;
    println!("wrote {width}x{height} -> {output}");
    let _ = PathBuf::from(output);
    Ok(())
}

#[cfg(not(feature = "raw"))]
fn main() {
    eprintln!("build with --features raw");
}
