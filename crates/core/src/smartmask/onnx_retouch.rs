//! AI 修图 ONNX 适配器（`ai` / `ai-static` feature）：
//!
//! - [`LamaInpainter`]：瑕疵区域重绘（LaMa，512 固定画布，区域 + 上下文裁剪）；
//! - [`GfpganRestorer`]：对齐人脸恢复增强（GFPGAN v1.4，输入 / 输出 [-1,1]）；
//! - [`EsrganUpscaler`]：4 倍超分（Real-ESRGAN 紧凑版，动态尺寸 + 分块无缝拼接）。
//!
//! 模型路径、输入尺寸、归一化全部来自 [`ModelSpec`]（[`ModelRegistry::builtin`]），
//! 缺失时返回加载错误，调用方（runtime / FFI）优雅降级到经典算法。

use std::sync::Mutex;

use ort::session::Session;
use ort::value::Tensor;

use crate::error::{FdxmpError, Result};
use crate::retouch::MaskPlane;
use crate::smartmask::mask::resize_plane;
use crate::smartmask::onnx::load_session;
use crate::smartmask::spec::{ModelRegistry, ModelSpec, ModelTask};
use crate::smartmask::traits::{Image, StageFaceRestore, StageInpaint, StageSuperRes};

/// LaMa 固定画布边长（模型输入 512×512）。
const LAMA_CANVAS: u32 = 512;
/// Real-ESRGAN 分块边长与重叠（像素）。
const ESRGAN_TILE: u32 = 256;
const ESRGAN_OVERLAP: u32 = 16;
/// Real-ESRGAN 输入边长上限（防耗时失控；输出像素上限在 runtime 侧另控）。
const ESRGAN_MAX_EDGE: u32 = 4096;

// ---------------------------------------------------------------------------
// 张量工具
// ---------------------------------------------------------------------------

/// RGBA8 → 双线性重采样到 `(width, height)` 的 RGB f32 NCHW（按 spec 归一化）。
fn image_to_nchw(image: &Image, width: u32, height: u32, spec: &ModelSpec) -> Vec<f32> {
    let resized = if image.width() == width && image.height() == height {
        None
    } else {
        Some(image::imageops::resize(
            image,
            width,
            height,
            image::imageops::FilterType::Triangle,
        ))
    };
    let source = resized.as_ref().unwrap_or(image);
    let pixels = (width as usize) * (height as usize);
    let mut data = vec![0.0f32; pixels * 3];
    for (index, pixel) in source.pixels().enumerate() {
        for channel in 0..3 {
            data[channel * pixels + index] = spec.normalize(pixel.0[channel] as f32, channel);
        }
    }
    data
}

/// NCHW f32 → RGBA8（`value * scale + offset`，clamp 0..255；Alpha = 255）。
fn nchw_to_image(data: &[f32], width: u32, height: u32, scale: f32, offset: f32) -> Image {
    let pixels = (width as usize) * (height as usize);
    let mut raw = vec![255u8; pixels * 4];
    for index in 0..pixels {
        for channel in 0..4 {
            if channel == 3 {
                continue;
            }
            let value = data[channel * pixels + index] * scale + offset;
            raw[index * 4 + channel] = value.round().clamp(0.0, 255.0) as u8;
        }
    }
    Image::from_raw(width, height, raw).expect("尺寸与缓冲一致")
}

/// 运行单输入会话（返回第一路输出 f32 + 形状）。
fn run_single(session: &mut Session, shape: Vec<usize>, data: Vec<f32>) -> Result<(Vec<usize>, Vec<f32>)> {
    let input_name = session
        .inputs()
        .first()
        .map(|input| input.name().to_string())
        .ok_or_else(|| FdxmpError::Unsupported("模型没有输入".to_string()))?;
    let tensor = Tensor::from_array((shape, data))
        .map_err(|err| FdxmpError::Unsupported(format!("张量创建失败：{err}")))?;
    let outputs = session
        .run(ort::inputs![input_name.as_str() => tensor])
        .map_err(|err| FdxmpError::Unsupported(format!("推理失败：{err}")))?;
    let value = outputs
        .iter()
        .next()
        .map(|(_, value)| value)
        .ok_or_else(|| FdxmpError::Unsupported("模型没有输出".to_string()))?;
    let (shape, values) = value
        .try_extract_tensor::<f32>()
        .map_err(|err| FdxmpError::Unsupported(format!("输出提取失败：{err}")))?;
    Ok((
        shape.iter().map(|dim| *dim as usize).collect(),
        values.to_vec(),
    ))
}

// ---------------------------------------------------------------------------
// LaMa 修复
// ---------------------------------------------------------------------------

/// LaMa 重绘：把「带上下文裁剪 + 软掩码」放到 512 画布推理，再按掩码融合回原图。
pub struct LamaInpainter {
    spec: ModelSpec,
    session: Mutex<Session>,
}

impl LamaInpainter {
    /// 按注册表加载（校验文件与 SHA-256）。
    pub fn load(registry: &ModelRegistry) -> Result<Self> {
        let (spec, session) = load_session(registry, ModelTask::Inpaint)?;
        Ok(Self {
            spec,
            session: Mutex::new(session),
        })
    }

    /// 重绘：`mask` 与 `image` 同尺寸（0..1，已膨胀 / 羽化）。
    pub fn run(&self, image: &Image, mask: &MaskPlane) -> Result<Image> {
        let (width, height) = (image.width().max(1), image.height().max(1));
        if mask.width != width || mask.height != height {
            return Err(FdxmpError::Unsupported(
                "LaMa 掩码与图像尺寸不一致".to_string(),
            ));
        }
        // 画布缩放：大于画布时整体缩小（区域修复通常远小于画布，不触发）。
        let scale = (LAMA_CANVAS as f32 / width as f32)
            .min(LAMA_CANVAS as f32 / height as f32)
            .min(1.0);
        let draw_w = ((width as f32 * scale).round() as u32).clamp(1, LAMA_CANVAS);
        let draw_h = ((height as f32 * scale).round() as u32).clamp(1, LAMA_CANVAS);

        // 画布：边缘复制填充（LaMa 需要干净上下文），掩码区外为 0。
        let resized_image = if draw_w != width || draw_h != height {
            image::imageops::resize(image, draw_w, draw_h, image::imageops::FilterType::Triangle)
        } else {
            image.clone()
        };
        let resized_mask = resize_plane(&mask.data, width, height, draw_w, draw_h);
        let canvas = LAMA_CANVAS as usize;
        let pixels = canvas * canvas;
        let mut image_data = vec![0.0f32; pixels * 3];
        let mut mask_data = vec![0.0f32; pixels];
        for y in 0..canvas {
            let sy = y.min(draw_h as usize - 1);
            for x in 0..canvas {
                let sx = x.min(draw_w as usize - 1);
                let index = y * canvas + x;
                let source = sy * (draw_w as usize) + sx;
                let pixel = resized_image.get_pixel(sx as u32, sy as u32);
                for channel in 0..3 {
                    image_data[channel * pixels + index] =
                        self.spec.normalize(pixel.0[channel] as f32, channel);
                }
                mask_data[index] = if x < draw_w as usize && y < draw_h as usize {
                    resized_mask[source].clamp(0.0, 1.0)
                } else {
                    0.0
                };
            }
        }

        let (_, output) = {
            let mut session = self
                .session
                .lock()
                .map_err(|_| FdxmpError::Unsupported("LaMa 会话锁失效".to_string()))?;
            let input_names: Vec<String> = session
                .inputs()
                .iter()
                .map(|input| input.name().to_string())
                .collect();
            if input_names.len() < 2 {
                return Err(FdxmpError::Unsupported(
                    "LaMa 需要 image / mask 两路输入".to_string(),
                ));
            }
            let image_tensor = Tensor::from_array((
                vec![1, 3, canvas, canvas],
                image_data,
            ))
            .map_err(|err| FdxmpError::Unsupported(format!("张量创建失败：{err}")))?;
            let mask_tensor =
                Tensor::from_array((vec![1, 1, canvas, canvas], mask_data.clone()))
                    .map_err(|err| FdxmpError::Unsupported(format!("张量创建失败：{err}")))?;
            let outputs = session
                .run(ort::inputs![
                    input_names[0].as_str() => image_tensor,
                    input_names[1].as_str() => mask_tensor
                ])
                .map_err(|err| FdxmpError::Unsupported(format!("LaMa 推理失败：{err}")))?;
            let value = outputs
                .iter()
                .next()
                .map(|(_, value)| value)
                .ok_or_else(|| FdxmpError::Unsupported("LaMa 没有输出".to_string()))?;
            let (shape, values) = value
                .try_extract_tensor::<f32>()
                .map_err(|err| FdxmpError::Unsupported(format!("输出提取失败：{err}")))?;
            (
                shape.iter().map(|dim| *dim as usize).collect::<Vec<_>>(),
                values.to_vec(),
            )
        };
        if output.len() < pixels * 3 {
            return Err(FdxmpError::Unsupported(
                "LaMa 输出尺寸异常".to_string(),
            ));
        }
        // Carve 导出输出为 0..255；若在 [0,1] 则原样使用。
        let max_value = output.iter().cloned().fold(f32::MIN, f32::max);
        let (out_scale, out_offset) = if max_value > 1.5 {
            (1.0, 0.0)
        } else {
            (255.0, 0.0)
        };
        let generated = nchw_to_image(&output, LAMA_CANVAS, LAMA_CANVAS, out_scale, out_offset);
        // 融合：原图 ×(1−m) + 生成 ×m（画布坐标），再缩回原尺寸。
        let mut composited = Image::new(LAMA_CANVAS, LAMA_CANVAS);
        for y in 0..LAMA_CANVAS {
            for x in 0..LAMA_CANVAS {
                let index = (y as usize) * canvas + (x as usize);
                let weight = mask_data[index];
                let base = resized_image.get_pixel(
                    x.min(draw_w - 1),
                    y.min(draw_h - 1),
                );
                let generated_pixel = generated.get_pixel(x, y);
                let mut pixel = [0u8; 3];
                for (channel, value) in pixel.iter_mut().enumerate() {
                    let blended = base.0[channel] as f32 * (1.0 - weight)
                        + generated_pixel.0[channel] as f32 * weight;
                    *value = blended.round().clamp(0.0, 255.0) as u8;
                }
                composited.put_pixel(x, y, image::Rgba([pixel[0], pixel[1], pixel[2], base.0[3]]));
            }
        }
        let cropped = image::imageops::crop_imm(&composited, 0, 0, draw_w, draw_h).to_image();
        let result = if draw_w != width || draw_h != height {
            image::imageops::resize(
                &cropped,
                width,
                height,
                image::imageops::FilterType::Triangle,
            )
        } else {
            cropped
        };
        // 只保留掩码处变化（掩码外强制原像素，避免重采样引入差异）。
        let mut out = image.clone();
        for y in 0..height {
            for x in 0..width {
                let weight = mask.get(x, y).clamp(0.0, 1.0);
                if weight <= 0.001 {
                    continue;
                }
                let base = image.get_pixel(x, y);
                let fixed = result.get_pixel(x, y);
                let mut pixel = [0u8; 4];
                for (channel, value) in pixel.iter_mut().enumerate().take(3) {
                    let blended = base.0[channel] as f32 * (1.0 - weight)
                        + fixed.0[channel] as f32 * weight;
                    *value = blended.round().clamp(0.0, 255.0) as u8;
                }
                pixel[3] = base.0[3];
                out.put_pixel(x, y, image::Rgba(pixel));
            }
        }
        Ok(out)
    }
}

impl StageInpaint for LamaInpainter {
    fn inpaint(&self, image: &Image, mask: &MaskPlane) -> Result<Image> {
        self.run(image, mask)
    }

    fn version_id(&self) -> String {
        self.spec.version.clone()
    }
}

// ---------------------------------------------------------------------------
// GFPGAN 人脸恢复
// ---------------------------------------------------------------------------

/// GFPGAN v1.4：512 对齐人脸 → 恢复（输入 / 输出 [-1,1]）。
pub struct GfpganRestorer {
    spec: ModelSpec,
    session: Mutex<Session>,
}

impl GfpganRestorer {
    /// 按注册表加载。
    pub fn load(registry: &ModelRegistry) -> Result<Self> {
        let (spec, session) = load_session(registry, ModelTask::FaceRestore)?;
        Ok(Self {
            spec,
            session: Mutex::new(session),
        })
    }

    /// 恢复一张对齐人脸（返回与输入同尺寸；内部按需重采样到 512）。
    pub fn run(&self, face: &Image) -> Result<Image> {
        let (width, height) = (face.width().max(1), face.height().max(1));
        let input = image_to_nchw(face, 512, 512, &self.spec);
        let (_, output) = {
            let mut session = self
                .session
                .lock()
                .map_err(|_| FdxmpError::Unsupported("GFPGAN 会话锁失效".to_string()))?;
            run_single(&mut session, vec![1, 3, 512, 512], input)?
        };
        if output.len() < 512 * 512 * 3 {
            return Err(FdxmpError::Unsupported("GFPGAN 输出尺寸异常".to_string()));
        }
        // 输出 [-1,1] → 0..255。
        let restored = nchw_to_image(&output, 512, 512, 127.5, 127.5);
        if width != 512 || height != 512 {
            Ok(image::imageops::resize(
                &restored,
                width,
                height,
                image::imageops::FilterType::Triangle,
            ))
        } else {
            Ok(restored)
        }
    }
}

impl StageFaceRestore for GfpganRestorer {
    fn restore(&self, aligned_face: &Image) -> Result<Image> {
        self.run(aligned_face)
    }

    fn version_id(&self) -> String {
        self.spec.version.clone()
    }
}

// ---------------------------------------------------------------------------
// Real-ESRGAN 超分
// ---------------------------------------------------------------------------

/// Real-ESRGAN 紧凑版：4 倍放大，动态尺寸按块推理、重叠带线性融合。
pub struct EsrganUpscaler {
    spec: ModelSpec,
    session: Mutex<Session>,
}

impl EsrganUpscaler {
    /// 按注册表加载。
    pub fn load(registry: &ModelRegistry) -> Result<Self> {
        let (spec, session) = load_session(registry, ModelTask::SuperRes)?;
        Ok(Self {
            spec,
            session: Mutex::new(session),
        })
    }

    /// 4 倍放大（输入任意尺寸，输出 = 4×）。
    pub fn run(&self, image: &Image) -> Result<Image> {
        self.run_scaled(image, 4.0)
    }

    /// 按倍率放大：内部始终以 4× 推理，再逐块缩放到目标倍率拼接
    /// （大图导出增强时避免 4× 中间缓冲爆内存）。`scale ∈ [1, 4]`。
    pub fn run_scaled(&self, image: &Image, scale: f32) -> Result<Image> {
        let scale = scale.clamp(1.0, 4.0);
        let (width, height) = (image.width().max(1), image.height().max(1));
        if width > ESRGAN_MAX_EDGE || height > ESRGAN_MAX_EDGE {
            return Err(FdxmpError::Unsupported(format!(
                "增强输入过大（{width}×{height}，单边 ≤ {ESRGAN_MAX_EDGE}）"
            )));
        }
        let out_width = ((width as f32 * scale).round() as u32).max(1);
        let out_height = ((height as f32 * scale).round() as u32).max(1);
        let mut out = vec![0.0f32; (out_width as usize) * (out_height as usize) * 3];
        let mut weight = vec![0.0f32; (out_width as usize) * (out_height as usize)];

        let tile = ESRGAN_TILE;
        let overlap = ESRGAN_OVERLAP;
        let step = tile - overlap * 2;
        let mut offsets_y = Vec::new();
        let mut y = 0u32;
        loop {
            offsets_y.push(y.min(height.saturating_sub(tile)));
            if y + tile >= height {
                break;
            }
            y = (y + step).min(height.saturating_sub(tile));
        }
        let mut offsets_x = Vec::new();
        let mut x = 0u32;
        loop {
            offsets_x.push(x.min(width.saturating_sub(tile)));
            if x + tile >= width {
                break;
            }
            x = (x + step).min(width.saturating_sub(tile));
        }

        let mut session = self
            .session
            .lock()
            .map_err(|_| FdxmpError::Unsupported("Real-ESRGAN 会话锁失效".to_string()))?;
        for &oy in &offsets_y {
            for &ox in &offsets_x {
                // 边缘复制填满 tile（动态尺寸模型也接受更小输入，这里统一尺寸减少会话重编译）。
                let mut data = vec![0.0f32; (tile as usize) * (tile as usize) * 3];
                let pixels = (tile as usize) * (tile as usize);
                for ty in 0..tile {
                    for tx in 0..tile {
                        let sx = (ox + tx).min(width - 1);
                        let sy = (oy + ty).min(height - 1);
                        let pixel = image.get_pixel(sx, sy);
                        let index = (ty as usize) * (tile as usize) + (tx as usize);
                        for channel in 0..3 {
                            data[channel * pixels + index] =
                                self.spec.normalize(pixel.0[channel] as f32, channel);
                        }
                    }
                }
                let (_, output) =
                    run_single(&mut session, vec![1, 3, tile as usize, tile as usize], data)?;
                let out_tile = (tile * 4) as usize;
                if output.len() < out_tile * out_tile * 3 {
                    return Err(FdxmpError::Unsupported("超分输出尺寸异常".to_string()));
                }
                // 每个 4× tile 先缩放到目标倍率，再写入重叠带加权融合。
                let dest_tile = ((tile as f32 * scale).round() as usize).max(1);
                let mut scaled_tile = vec![0.0f32; dest_tile * dest_tile * 3];
                for ty in 0..dest_tile {
                    let sy = ((ty as f32 + 0.5) * out_tile as f32 / dest_tile as f32 - 0.5)
                        .clamp(0.0, out_tile as f32 - 1.0);
                    let y0 = sy.floor() as usize;
                    let y1 = (y0 + 1).min(out_tile - 1);
                    let fy = sy - y0 as f32;
                    for tx in 0..dest_tile {
                        let sx = ((tx as f32 + 0.5) * out_tile as f32 / dest_tile as f32 - 0.5)
                            .clamp(0.0, out_tile as f32 - 1.0);
                        let x0 = sx.floor() as usize;
                        let x1 = (x0 + 1).min(out_tile - 1);
                        let fx = sx - x0 as f32;
                        for channel in 0..3 {
                            let plane = channel * out_tile * out_tile;
                            let top = output[plane + y0 * out_tile + x0] * (1.0 - fx)
                                + output[plane + y0 * out_tile + x1] * fx;
                            let bottom = output[plane + y1 * out_tile + x0] * (1.0 - fx)
                                + output[plane + y1 * out_tile + x1] * fx;
                            scaled_tile[channel * dest_tile * dest_tile + ty * dest_tile + tx] =
                                (top * (1.0 - fy) + bottom * fy) * 255.0;
                        }
                    }
                }
                for ty in 0..dest_tile {
                    let gy = (oy as f32 * scale).round() as usize + ty;
                    if gy >= out_height as usize {
                        break;
                    }
                    // 纵向权重（在 tile 内靠近重叠边缘处渐入）。
                    let in_tile_y = ty as f32 / scale;
                    let wy = ramp(in_tile_y, oy, overlap);
                    for tx in 0..dest_tile {
                        let gx = (ox as f32 * scale).round() as usize + tx;
                        if gx >= out_width as usize {
                            break;
                        }
                        let in_tile_x = tx as f32 / scale;
                        let wx = ramp(in_tile_x, ox, overlap);
                        let w = wy * wx;
                        if w <= 0.0 {
                            continue;
                        }
                        let src = ty * dest_tile + tx;
                        let dst = gy * (out_width as usize) + gx;
                        for channel in 0..3 {
                            out[dst * 3 + channel] += scaled_tile
                                [channel * dest_tile * dest_tile + src]
                                .clamp(0.0, 255.0)
                                * w;
                        }
                        weight[dst] += w;
                    }
                }
            }
        }
        let mut raw = vec![255u8; (out_width as usize) * (out_height as usize) * 4];
        for index in 0..(out_width as usize) * (out_height as usize) {
            let w = weight[index];
            for channel in 0..4 {
                if channel == 3 {
                    continue;
                }
                let value = if w > 0.0001 {
                    out[index * 3 + channel] / w
                } else {
                    // 理论不可达（tile 覆盖全图）；兜底取最近邻。
                    let pixel = image.get_pixel(
                        ((index % out_width as usize) as f32 / scale) as u32,
                        ((index / out_width as usize) as f32 / scale) as u32,
                    );
                    pixel.0[channel] as f32
                };
                raw[index * 4 + channel] = value.round().clamp(0.0, 255.0) as u8;
            }
        }
        Image::from_raw(out_width, out_height, raw)
            .ok_or_else(|| FdxmpError::Unsupported("超分输出缓冲异常".to_string()))
    }
}

/// 重叠带渐入权重：tile 内坐标 `local`（0..tile），起点 `offset`（原图）。
/// 在图像左 / 上边界不做渐入（权重 1），否则前 `overlap` 像素线性渐入。
fn ramp(local: f32, offset: u32, overlap: u32) -> f32 {
    if offset == 0 {
        return 1.0;
    }
    if local < overlap as f32 {
        (local + 0.5) / overlap as f32
    } else {
        1.0
    }
}

impl StageSuperRes for EsrganUpscaler {
    fn upscale(&self, image: &Image) -> Result<Image> {
        self.run(image)
    }

    fn upscale_scaled(&self, image: &Image, scale: f32) -> Result<Image> {
        self.run_scaled(image, scale)
    }

    fn version_id(&self) -> String {
        self.spec.version.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_keeps_borders_full_weight() {
        assert!((ramp(0.0, 0, 16) - 1.0).abs() < 1e-6);
        assert!(ramp(0.0, 100, 16) < 0.2);
        assert!((ramp(32.0, 100, 16) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn image_roundtrip_preserves_size() {
        let image = Image::from_pixel(8, 6, image::Rgba([10, 20, 30, 255]));
        let data = image_to_nchw(&image, 8, 6, &ModelSpec::default());
        assert_eq!(data.len(), 8 * 6 * 3);
        let back = nchw_to_image(&data, 8, 6, 255.0, 0.0);
        assert_eq!(back.get_pixel(0, 0).0, [10, 20, 30, 255]);
    }
}
