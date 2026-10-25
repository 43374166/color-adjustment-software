//! Experimental: read LibRaw's `imgdata.image` (camera RGB, before color matrix).
//!
//! Usage:
//!   cargo run -p fdxmp-core --example raw_camera_image --features raw -- in.ARW out.png

#[cfg(feature = "raw")]
fn main() -> Result<(), String> {
    use std::ffi::c_void;

    use rsraw_sys as sys;

    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .ok_or("usage: raw_camera_image <in.ARW> <out.png>")?;
    let output = args
        .next()
        .ok_or("usage: raw_camera_image <in.ARW> <out.png>")?;
    let bytes = std::fs::read(&input).map_err(|err| err.to_string())?;

    unsafe {
        let raw = sys::libraw_init(0);
        if raw.is_null() {
            return Err("libraw_init failed".into());
        }
        let result = (|| -> Result<(), String> {
            if sys::libraw_open_buffer(raw, bytes.as_ptr().cast::<c_void>(), bytes.len()) != 0 {
                return Err("libraw_open_buffer failed".into());
            }
            (*raw).params.use_camera_wb = 1;
            (*raw).params.use_camera_matrix = 0;
            (*raw).params.no_auto_bright = 1;
            if sys::libraw_unpack(raw) != 0 {
                return Err("libraw_unpack failed".into());
            }
            if sys::libraw_raw2image(raw) != 0 {
                return Err("libraw_raw2image failed".into());
            }
            let sizes = &(*raw).sizes;
            let width = sizes.iwidth as u32;
            let height = sizes.iheight as u32;
            let colors = (*raw).idata.colors.clamp(3, 4) as usize;
            let image = (*raw).image;
            if image.is_null() {
                return Err("raw image is null".into());
            }
            let maximum = (*raw).color.maximum as f32;
            println!(
                "cdesc={:?} filters={:#x}",
                (*raw).idata.cdesc, (*raw).idata.filters
            );
            print!("black={}, cblack=[", (*raw).color.black);
            for i in 0..4 {
                print!("{},", (*raw).color.cblack[i]);
            }
            println!("], maximum={maximum}");
            print!("pre_mul=[");
            for i in 0..4 {
                print!("{},", (*raw).color.pre_mul[i]);
            }
            println!("]");
            print!("cam_xyz=[");
            for i in 0..4 {
                print!("[");
                for j in 0..3 {
                    print!("{},", (*raw).color.cam_xyz[i][j]);
                }
                print!("],");
            }
            println!("]");
            print!("cam_mul=[");
            for i in 0..4 {
                print!("{},", sys::libraw_get_cam_mul(raw, i));
            }
            println!("]; maximum={maximum}; size={width}x{height}, colors={colors}");
            let count = width as usize * height as usize;
            let src = std::slice::from_raw_parts(image, count);
            let rgb = fdxmp_core::demosaic::demosaic_rggb(
                src,
                width as usize,
                height as usize,
                (*raw).color.black as f32,
                maximum,
            );
            let mut pixels = Vec::with_capacity(count * 3);
            for pixel in rgb {
                for value in pixel {
                    pixels.push((value * 65535.0 + 0.5) as u16);
                }
            }
            let dynamic =
                image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_raw(width, height, pixels)
                    .map(image::DynamicImage::ImageRgb16)
                    .ok_or("buffer size mismatch")?;
            if output.ends_with(".bin") {
                use std::io::Write;
                let mut file = std::fs::File::create(&output).map_err(|err| err.to_string())?;
                let raw = dynamic.as_rgb16().ok_or("not rgb16")?;
                let mut buffer = Vec::with_capacity(8 + raw.as_raw().len() * 2);
                buffer.extend_from_slice(&width.to_le_bytes());
                buffer.extend_from_slice(&height.to_le_bytes());
                for value in raw.as_raw() {
                    buffer.extend_from_slice(&value.to_le_bytes());
                }
                file.write_all(&buffer).map_err(|err| err.to_string())?;
            } else {
                dynamic.save(&output).map_err(|err| err.to_string())?;
            }
            sys::libraw_free_image(raw);
            Ok(())
        })();
        sys::libraw_close(raw);
        result?;
    }
    println!("wrote {output}");
    Ok(())
}

#[cfg(not(feature = "raw"))]
fn main() {
    eprintln!("build with --features raw");
}
