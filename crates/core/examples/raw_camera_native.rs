//! Experimental LibRaw C API path: ask LibRaw for raw/camera RGB
//! (`output_color = 0`) instead of processed sRGB.
//!
//! Usage:
//!   cargo run -p fdxmp-core --example raw_camera_native --features raw -- in.ARW out.png

#[cfg(feature = "raw")]
fn main() -> Result<(), String> {
    use std::ffi::c_void;

    use rsraw_sys as sys;

    let mut args = std::env::args().skip(1);
    let input = args.next().ok_or("usage: raw_camera_native <in.ARW> <out.png>")?;
    let output = args.next().ok_or("usage: raw_camera_native <in.ARW> <out.png>")?;
    let bytes = std::fs::read(&input).map_err(|err| err.to_string())?;

    unsafe {
        let raw = sys::libraw_init(0);
        if raw.is_null() {
            return Err("libraw_init failed".to_string());
        }
        let result = (|| -> Result<(), String> {
            if sys::libraw_open_buffer(raw, bytes.as_ptr().cast::<c_void>(), bytes.len()) != 0 {
                return Err("libraw_open_buffer failed".to_string());
            }
            let params = &mut (*raw).params;
            // 0 = raw/camera RGB; no sRGB matrix, no gamma.
            params.output_color = 0;
            params.output_bps = 16;
            params.use_camera_wb = 1;
            params.use_camera_matrix = 0;
            params.no_auto_bright = 1;
            params.gamm[0] = 1.0;
            params.gamm[1] = 1.0;
            if sys::libraw_unpack(raw) != 0 {
                return Err("libraw_unpack failed".to_string());
            }
            // LibRaw's convert_to_rgb() copies rgb_cam into out_cam. Force it to
            // identity so the demosaiced output stays in camera RGB.
            for i in 0..3 {
                for j in 0..4 {
                    (*raw).color.rgb_cam[i][j] = if i == j { 1.0 } else { 0.0 };
                }
            }
            if sys::libraw_dcraw_process(raw) != 0 {
                return Err("libraw_dcraw_process failed".to_string());
            }
            let mut errc = 0;
            let image = sys::libraw_dcraw_make_mem_image(raw, &mut errc);
            if image.is_null() {
                return Err(format!("libraw_dcraw_make_mem_image failed: {errc}"));
            }
            let width = (*image).width as u32;
            let height = (*image).height as u32;
            let colors = (*image).colors as usize;
            let bits = (*image).bits as usize;
            let data_size = (*image).data_size as usize;
            let data = std::slice::from_raw_parts((*image).data.as_ptr(), data_size);
            print!("rgb_cam = [");
            for i in 0..3 {
                print!("[");
                for j in 0..3 {
                    print!("{},", sys::libraw_get_rgb_cam(raw, i, j));
                }
                print!("],");
            }
            println!("]");
            print!("cam_mul = [");
            for j in 0..4 {
                print!("{},", sys::libraw_get_cam_mul(raw, j));
            }
            println!("]");
            println!(
                "camera RGB: {width}x{height}, colors={colors}, bits={bits}, bytes={data_size}"
            );
            let dynamic = if colors == 3 && bits == 16 {
                let pixels: Vec<u16> = data
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_ne_bytes(*pair))
                    .collect();
                image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_raw(width, height, pixels)
                    .map(image::DynamicImage::ImageRgb16)
            } else {
                None
            }
            .ok_or_else(|| format!("unsupported output: colors={colors}, bits={bits}"))?;
            if output.ends_with(".bin") {
                use std::io::Write;
                let rgb = dynamic.as_rgb16().ok_or("not rgb16")?;
                let mut file = std::fs::File::create(&output).map_err(|err| err.to_string())?;
                let mut buffer = Vec::with_capacity(8 + rgb.as_raw().len() * 2);
                buffer.extend_from_slice(&width.to_le_bytes());
                buffer.extend_from_slice(&height.to_le_bytes());
                for value in rgb.as_raw() {
                    buffer.extend_from_slice(&value.to_le_bytes());
                }
                file.write_all(&buffer).map_err(|err| err.to_string())?;
            } else {
                dynamic.save(&output).map_err(|err| err.to_string())?;
            }
            sys::libraw_dcraw_clear_mem(image);
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
