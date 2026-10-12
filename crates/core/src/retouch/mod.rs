//! AI 修图（v1.5.0）：一键修图的人像处理核心。
//!
//! 只做像素级修图，**不做调色**（调色仍在既有管线里，两者互不干扰）：
//!
//! | 功能 | 做法 |
//! | --- | --- |
//! | 去瑕疵 | 皮肤蒙版内的高频负向异常（痘印 / 斑点）软阈值收敛，保留毛孔 |
//! | 低/中/高频磨皮 | 三级频率分离（低频 = 光影 / 肤色不均，中频 = 纹理，高频 = 噪点），逐带按强度衰减 |
//! | 统一肤色 | 色度（Cb / Cr）大半径平滑 + 向肤色中位数收敛 |
//! | 脸部塑形 | FaceMesh 关键点驱动的局部变形（瘦脸 / 下巴 / 大眼 / 瘦鼻） |
//! | 身体塑形 | 人像蒙版轮廓驱动的腰腹收形 |
//!
//! 蒙版与关键点由智能蒙版模块提供（[`crate::smartmask`]）；本模块只消费，
//! 不依赖任何模型，可在没有 `ai` feature 的环境里用合成蒙版单独测试。

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// 修图参数（0..100 强度；缺省 0 = 不做任何处理）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RetouchParams {
    /// 去瑕疵（痘印 / 斑点）。
    pub blemish: f32,
    /// 低频磨皮（光影 / 肤色不均）。
    pub smooth_low: f32,
    /// 中频磨皮（纹理）。
    pub smooth_mid: f32,
    /// 高频磨皮（毛孔 / 噪点）。
    pub smooth_high: f32,
    /// 统一肤色。
    pub skin_tone: f32,
    /// 瘦脸（下颌收窄）。
    pub face_slim: f32,
    /// 下巴（拉长 / 收窄）。
    pub chin: f32,
    /// 大眼。
    pub eye_enlarge: f32,
    /// 瘦鼻。
    pub nose_slim: f32,
    /// 瘦身（腰腹收形）。
    pub body_slim: f32,
}

impl Default for RetouchParams {
    fn default() -> Self {
        Self {
            blemish: 0.0,
            smooth_low: 0.0,
            smooth_mid: 0.0,
            smooth_high: 0.0,
            skin_tone: 0.0,
            face_slim: 0.0,
            chin: 0.0,
            eye_enlarge: 0.0,
            nose_slim: 0.0,
            body_slim: 0.0,
        }
    }
}

impl RetouchParams {
    /// 是否完全不动像素（可以跳过整个修图）。
    pub fn is_identity(&self) -> bool {
        self == &Self::default()
    }

    /// 是否包含像素级修图（磨皮 / 去瑕疵 / 肤色）。
    pub fn has_pixel_pass(&self) -> bool {
        self.blemish > 0.0
            || self.smooth_low > 0.0
            || self.smooth_mid > 0.0
            || self.smooth_high > 0.0
            || self.skin_tone > 0.0
    }

    /// 是否包含塑形（几何变形）。
    pub fn has_warp_pass(&self) -> bool {
        self.face_slim > 0.0
            || self.chin > 0.0
            || self.eye_enlarge > 0.0
            || self.nose_slim > 0.0
            || self.body_slim > 0.0
    }
}

/// 单通道权重平面（与图像同尺寸，0..1）。
#[derive(Debug, Clone, PartialEq)]
pub struct MaskPlane {
    /// 宽度。
    pub width: u32,
    /// 高度。
    pub height: u32,
    /// 行优先权重。
    pub data: Vec<f32>,
}

impl MaskPlane {
    /// 从 `SmartMask` 的权重构建（尺寸不一致时双线性缩放）。
    pub fn from_smart(mask: &crate::smartmask::SmartMask, width: u32, height: u32) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            data: crate::smartmask::mask::resize_plane(
                &mask.data,
                mask.width.max(1),
                mask.height.max(1),
                width.max(1),
                height.max(1),
            ),
        }
    }

    /// 读取（越界返回 0）。
    pub fn get(&self, x: u32, y: u32) -> f32 {
        if x >= self.width || y >= self.height {
            return 0.0;
        }
        self.data[(y as usize) * (self.width as usize) + (x as usize)]
    }
}

/// 修图所需的人像信息（由智能蒙版 / FaceMesh 提供；都可以缺省）。
#[derive(Debug, Clone, Default)]
pub struct RetouchMasks {
    /// 皮肤（磨皮 / 去瑕疵 / 肤色统一的作用域）。
    pub skin: Option<MaskPlane>,
    /// 面部（脸部塑形的保底作用域）。
    pub face: Option<MaskPlane>,
    /// 人像（身体塑形）。
    pub person: Option<MaskPlane>,
    /// 脸部关键点（归一化原图坐标；MediaPipe FaceMesh 478 点）。
    pub landmarks: Vec<[f32; 2]>,
}

/// MediaPipe FaceMesh（478 点）常用下标。
pub mod landmark {
    /// 面部外轮廓（下颌线，从左耳根到右耳根）。
    pub const FACE_OVAL: [usize; 36] = [
        10, 338, 297, 332, 284, 251, 389, 356, 454, 323, 361, 288, 397, 365, 379, 378, 400, 377,
        152, 148, 176, 149, 150, 136, 172, 58, 132, 93, 234, 127, 162, 21, 54, 103, 67, 109,
    ];
    /// 左眼中心（虹膜中心）。
    pub const LEFT_IRIS: usize = 468;
    /// 右眼中心（虹膜中心）。
    pub const RIGHT_IRIS: usize = 473;
    /// 左眼轮廓。
    pub const LEFT_EYE: [usize; 8] = [33, 161, 160, 159, 158, 157, 173, 133];
    /// 右眼轮廓。
    pub const RIGHT_EYE: [usize; 8] = [362, 385, 386, 387, 388, 466, 263, 249];
    /// 鼻翼（左右）。
    pub const NOSE_WINGS: [usize; 2] = [129, 358];
    /// 鼻尖。
    pub const NOSE_TIP: usize = 1;
    /// 下巴尖。
    pub const CHIN: usize = 152;
    /// 额头中心。
    pub const FOREHEAD: usize = 10;
}

/// 修图主入口：按参数处理图像（顺序：像素级修图 → 塑形变形）。
pub fn apply(
    image: &image::RgbaImage,
    masks: &RetouchMasks,
    params: &RetouchParams,
) -> image::RgbaImage {
    if params.is_identity() {
        return image.clone();
    }
    let (width, height) = (image.width().max(1), image.height().max(1));
    let mut planes = to_planes(image);
    let skin = masks
        .skin
        .clone()
        .map(|plane| plane_to_size(&plane, width, height))
        .unwrap_or_else(|| MaskPlane {
            width,
            height,
            data: vec![1.0; (width as usize) * (height as usize)],
        });
    // 柔化蒙版边界：磨皮 / 去瑕疵 / 肤色都按权重渐变，避免蒙版轮廓出现硬边。
    let skin = soften_mask(&skin, (width.min(height) as f32 * 0.02).clamp(2.0, 24.0));

    if params.has_pixel_pass() {
        blemish_pass(&mut planes, &skin, width, height, params.blemish);
        frequency_smooth(&mut planes, &skin, width, height, params);
        skin_tone_unify(&mut planes, &skin, width, height, params.skin_tone);
    }

    let smoothed = from_planes(&planes, width, height);
    if params.has_warp_pass() {
        warp(&smoothed, masks, params)
    } else {
        smoothed
    }
}

/// 变形阶段单独入口（测试 / 复用）。
pub fn warp(
    image: &image::RgbaImage,
    masks: &RetouchMasks,
    params: &RetouchParams,
) -> image::RgbaImage {
    if !params.has_warp_pass() {
        return image.clone();
    }
    let (width, height) = (image.width().max(1), image.height().max(1));
    // 逆映射：对每个输出像素求源坐标（依次施加各变形的逆）。
    let source = image.as_raw().clone();
    let stride = (width as usize) * 4;
    let body = masks
        .person
        .as_ref()
        .map(|plane| BodyProfile::build(&plane_to_size(plane, width, height)));
    let landmarks = &masks.landmarks;

    let mut rows: Vec<Vec<u8>> = (0..height as usize)
        .into_par_iter()
        .map(|y| {
            let mut row = vec![0u8; stride];
            for x in 0..width as usize {
                let mut px = x as f32 + 0.5;
                let mut py = y as f32 + 0.5;

                // 身体（腰腹收形，先做；脸部在其后微调）。
                if params.body_slim > 0.0 {
                    if let Some(body) = body.as_ref() {
                        let (nx, ny) = slim_body(px, py, body, params.body_slim);
                        px = nx;
                        py = ny;
                    }
                }
                // 脸部塑形（基于关键点）。
                if !landmarks.is_empty() {
                    let (nx, ny) = warp_face(px, py, landmarks, params, width, height);
                    px = nx;
                    py = ny;
                }
                for channel in 0..4 {
                    row[x * 4 + channel] =
                        sample(&source, width, height, px - 0.5, py - 0.5, channel);
                }
            }
            row
        })
        .collect();
    let raw: Vec<u8> = rows.drain(..).flatten().collect();
    image::RgbaImage::from_raw(width, height, raw).unwrap_or_else(|| image.clone())
}

// ---------------------------------------------------------------------------
// 平面 / 像素工具
// ---------------------------------------------------------------------------

/// 三通道 f32 平面（每通道一个 `Vec<f32>`，0..1）。
struct Planes {
    r: Vec<f32>,
    g: Vec<f32>,
    b: Vec<f32>,
}

fn to_planes(image: &image::RgbaImage) -> Planes {
    let count = (image.width() as usize) * (image.height() as usize);
    let mut planes = Planes {
        r: vec![0.0; count],
        g: vec![0.0; count],
        b: vec![0.0; count],
    };
    for (index, pixel) in image.pixels().enumerate() {
        planes.r[index] = pixel.0[0] as f32 / 255.0;
        planes.g[index] = pixel.0[1] as f32 / 255.0;
        planes.b[index] = pixel.0[2] as f32 / 255.0;
    }
    planes
}

fn from_planes(planes: &Planes, width: u32, height: u32) -> image::RgbaImage {
    let mut buffer = image::RgbaImage::new(width, height);
    for (index, pixel) in buffer.pixels_mut().enumerate() {
        pixel.0 = [
            to_u8(planes.r[index]),
            to_u8(planes.g[index]),
            to_u8(planes.b[index]),
            255,
        ];
    }
    buffer
}

fn to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn plane_to_size(plane: &MaskPlane, width: u32, height: u32) -> MaskPlane {
    if plane.width == width && plane.height == height {
        return plane.clone();
    }
    MaskPlane {
        width,
        height,
        data: crate::smartmask::mask::resize_plane(
            &plane.data,
            plane.width.max(1),
            plane.height.max(1),
            width,
            height,
        ),
    }
}

/// 可分离盒式模糊（3 次 ≈ 高斯），就地作用于单个平面。
fn blur_plane(plane: &mut [f32], width: usize, height: usize, radius: f32) {
    let radius = radius.round().max(1.0) as usize;
    for _ in 0..3 {
        box_blur(plane, width, height, radius);
    }
}

fn box_blur(plane: &mut [f32], width: usize, height: usize, radius: usize) {
    if width == 0 || height == 0 || radius == 0 {
        return;
    }
    let mut temp = vec![0.0f32; plane.len()];
    for y in 0..height {
        let row = y * width;
        let mut sum = 0.0f32;
        let mut count = 0usize;
        let window = |index: isize| -> Option<f32> {
            if index < 0 || index >= width as isize {
                None
            } else {
                Some(plane[row + index as usize])
            }
        };
        for offset in -(radius as isize)..=radius as isize {
            if let Some(value) = window(offset) {
                sum += value;
                count += 1;
            }
        }
        for x in 0..width {
            temp[row + x] = sum / count.max(1) as f32;
            if let Some(value) = window(x as isize - radius as isize) {
                sum -= value;
                count = count.saturating_sub(1);
            }
            if let Some(value) = window(x as isize + radius as isize + 1) {
                sum += value;
                count += 1;
            }
        }
    }
    for x in 0..width {
        let mut sum = 0.0f32;
        let mut count = 0usize;
        let window = |index: isize| -> Option<f32> {
            if index < 0 || index >= height as isize {
                None
            } else {
                Some(temp[index as usize * width + x])
            }
        };
        for offset in -(radius as isize)..=radius as isize {
            if let Some(value) = window(offset) {
                sum += value;
                count += 1;
            }
        }
        for y in 0..height {
            plane[y * width + x] = sum / count.max(1) as f32;
            if let Some(value) = window(y as isize - radius as isize) {
                sum -= value;
                count = count.saturating_sub(1);
            }
            if let Some(value) = window(y as isize + radius as isize + 1) {
                sum += value;
                count += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 像素级修图
// ---------------------------------------------------------------------------

/// 去瑕疵：皮肤内的高频负向异常（比周围暗的痘印 / 斑点）软阈值收敛。
fn blemish_pass(planes: &mut Planes, skin: &MaskPlane, width: u32, height: u32, strength: f32) {
    let strength = (strength / 100.0).clamp(0.0, 1.0);
    if strength <= 0.0 {
        return;
    }
    let (w, h) = (width as usize, height as usize);
    let radius = (width.min(height) as f32 / 400.0).clamp(2.0, 6.0);
    // 用「亮度」找本地异常，再按比例回撤三个通道（保色）；多迭代几次让痘印收敛。
    let threshold = 0.03 * (1.0 - 0.5 * strength);
    for _ in 0..2 {
        let luma: Vec<f32> = (0..w * h)
            .map(|index| {
                0.299 * planes.r[index] + 0.587 * planes.g[index] + 0.114 * planes.b[index]
            })
            .collect();
        let mut base = luma.clone();
        blur_plane(&mut base, w, h, radius);
        for index in 0..w * h {
            let x = (index % w) as u32;
            let y = (index / w) as u32;
            let weight = skin.get(x, y) * strength;
            if weight <= 0.01 {
                continue;
            }
            let delta = luma[index] - base[index];
            if delta >= -threshold {
                continue; // 只修「比周围暗」的斑点
            }
            // 软阈值：把超出阈值的部分按强度回撤（保留边缘的自然过渡）。
            let excess = (delta + threshold).min(0.0);
            let correction = -excess * weight;
            planes.r[index] = (planes.r[index] + correction).clamp(0.0, 1.0);
            planes.g[index] = (planes.g[index] + correction).clamp(0.0, 1.0);
            planes.b[index] = (planes.b[index] + correction).clamp(0.0, 1.0);
        }
    }
}

/// 低 / 中 / 高频磨皮（频率分离）。
fn frequency_smooth(
    planes: &mut Planes,
    skin: &MaskPlane,
    width: u32,
    height: u32,
    params: &RetouchParams,
) {
    let low_strength = (params.smooth_low / 100.0).clamp(0.0, 1.0);
    let mid_strength = (params.smooth_mid / 100.0).clamp(0.0, 1.0);
    let high_strength = (params.smooth_high / 100.0).clamp(0.0, 1.0);
    if low_strength <= 0.0 && mid_strength <= 0.0 && high_strength <= 0.0 {
        return;
    }
    let (w, h) = (width as usize, height as usize);
    let long_edge = width.max(height) as f32;
    let sigma_low = (long_edge / 90.0).clamp(6.0, 40.0); // 光影
    let sigma_mid = (sigma_low / 3.0).max(2.0); // 纹理
    let sigma_smooth_low = sigma_low * 2.6; // 低频再平滑（肤色不均）

    for channel in 0..3 {
        let plane = match channel {
            0 => &mut planes.r,
            1 => &mut planes.g,
            _ => &mut planes.b,
        };
        let original = plane.clone();
        let mut low = original.clone();
        blur_plane(&mut low, w, h, sigma_low);
        let mut low_smooth = low.clone();
        blur_plane(&mut low_smooth, w, h, sigma_smooth_low);
        let mut mid_base = original.clone();
        blur_plane(&mut mid_base, w, h, sigma_mid);

        plane.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            for (x, slot) in row.iter_mut().enumerate() {
                let index = y * w + x;
                let weight = skin.get(x as u32, y as u32);
                if weight <= 0.01 {
                    continue;
                }
                let low_value = low[index];
                let mid = mid_base[index] - low_value;
                let high = original[index] - mid_base[index];

                let low_out = low_value + (low_smooth[index] - low_value) * low_strength * weight;
                let mid_out = mid * (1.0 - mid_strength * 0.85 * weight);
                // 高频：按强度衰减（80% 上限，保一点毛孔质感）。
                let high_out = high * (1.0 - high_strength * 0.8 * weight);

                *slot = (low_out + mid_out + high_out).clamp(0.0, 1.0);
            }
        });
    }
}

/// 统一肤色：色度平滑 + 向肤色中位数收敛。
fn skin_tone_unify(planes: &mut Planes, skin: &MaskPlane, width: u32, height: u32, strength: f32) {
    let strength = (strength / 100.0).clamp(0.0, 1.0);
    if strength <= 0.0 {
        return;
    }
    let (w, h) = (width as usize, height as usize);
    let count = w * h;
    // YCbCr（BT.601）。
    let mut cb: Vec<f32> = (0..count)
        .map(|index| -0.169 * planes.r[index] - 0.331 * planes.g[index] + 0.5 * planes.b[index])
        .collect();
    let mut cr: Vec<f32> = (0..count)
        .map(|index| 0.5 * planes.r[index] - 0.419 * planes.g[index] - 0.081 * planes.b[index])
        .collect();
    // 肤色区域中位数（用直方图近似，避免排序）。
    let mut histogram_cb = [0usize; 256];
    let mut histogram_cr = [0usize; 256];
    let mut skin_count = 0usize;
    for index in 0..count {
        let x = (index % w) as u32;
        let y = (index / w) as u32;
        if skin.get(x, y) > 0.5 {
            histogram_cb[((cb[index] + 0.5) * 255.0).clamp(0.0, 255.0) as usize] += 1;
            histogram_cr[((cr[index] + 0.5) * 255.0).clamp(0.0, 255.0) as usize] += 1;
            skin_count += 1;
        }
    }
    if skin_count < 64 {
        return;
    }
    let median = |histogram: &[usize; 256]| -> f32 {
        let half = skin_count / 2;
        let mut seen = 0usize;
        for (index, value) in histogram.iter().enumerate() {
            seen += value;
            if seen >= half {
                return index as f32 / 255.0 - 0.5;
            }
        }
        0.0
    };
    let (target_cb, target_cr) = (median(&histogram_cb), median(&histogram_cr));
    let radius = (width.max(height) as f32 / 32.0).clamp(8.0, 64.0);
    blur_plane(&mut cb, w, h, radius);
    blur_plane(&mut cr, w, h, radius);

    planes
        .r
        .par_iter_mut()
        .zip(planes.g.par_iter_mut())
        .zip(planes.b.par_iter_mut())
        .enumerate()
        .for_each(|(index, ((r, g), b))| {
            let x = (index % w) as u32;
            let y = (index / w) as u32;
            let weight = skin.get(x, y) * strength;
            if weight <= 0.01 {
                return;
            }
            let (rr, gg, bb) = (*r, *g, *b);
            let index_cb = -0.169 * rr - 0.331 * gg + 0.5 * bb;
            let index_cr = 0.5 * rr - 0.419 * gg - 0.081 * bb;
            // 平滑 + 向中位数收敛。
            let new_cb = (index_cb + (cb[index] - index_cb) * weight * 0.95)
                + (target_cb - index_cb) * weight * 0.5;
            let new_cr = (index_cr + (cr[index] - index_cr) * weight * 0.95)
                + (target_cr - index_cr) * weight * 0.5;
            // 反变换（Y 不变）。
            let y_value = 0.299 * rr + 0.587 * gg + 0.114 * bb;
            *r = (y_value + 1.402 * new_cr).clamp(0.0, 1.0);
            *g = (y_value - 0.344 * new_cb - 0.714 * new_cr).clamp(0.0, 1.0);
            *b = (y_value + 1.772 * new_cb).clamp(0.0, 1.0);
        });
}

// ---------------------------------------------------------------------------
// 塑形（几何变形）
// ---------------------------------------------------------------------------

fn sample(raw: &[u8], width: u32, height: u32, x: f32, y: f32, channel: usize) -> u8 {
    let (w, h) = (width as i32, height as i32);
    if w <= 0 || h <= 0 {
        return 0;
    }
    if x < -1.0 || y < -1.0 || x > w as f32 || y > h as f32 {
        return 0;
    }
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let at = |px: u32, py: u32| raw[((py * width + px) as usize) * 4 + channel] as f32;
    let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
    let bottom = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
    (top * (1.0 - ty) + bottom * ty).round().clamp(0.0, 255.0) as u8
}

/// 关键点 → 像素坐标（归一化 × 图像尺寸）。
fn to_pixels(point: [f32; 2], width: f32, height: f32) -> (f32, f32) {
    (point[0] * width, point[1] * height)
}

/// 局部「向中心轴收拢」形变（瘦脸 / 瘦鼻用）：
/// 以 `center` 为圆心、`radius` 为作用半径，把像素沿水平方向拉向 `axis_x`。
#[allow(clippy::too_many_arguments)]
fn pull_toward_axis(
    px: f32,
    py: f32,
    center: (f32, f32),
    axis_x: f32,
    radius: f32,
    strength: f32,
    vertical_scale: f32,
) -> (f32, f32) {
    let dx = px - center.0;
    let dy = (py - center.1) / vertical_scale.max(1e-3);
    let distance = (dx * dx + dy * dy).sqrt();
    if distance >= radius {
        return (px, py);
    }
    let falloff = (1.0 - (distance / radius).powi(2)).powi(2);
    let amount = strength * falloff;
    // 输出像素从更靠外的位置取样 → 视觉上向轴收拢。
    let offset = (px - axis_x) * amount;
    (px + offset, py)
}

/// 径向放大（大眼）。
fn magnify(px: f32, py: f32, center: (f32, f32), radius: f32, strength: f32) -> (f32, f32) {
    let dx = px - center.0;
    let dy = py - center.1;
    let distance = (dx * dx + dy * dy).sqrt();
    if distance >= radius || distance < 1e-3 {
        return (px, py);
    }
    let falloff = (1.0 - (distance / radius).powi(2)).powi(2);
    let scale = 1.0 - strength * falloff; // 采样点更靠近中心 → 放大
    (center.0 + dx * scale, center.1 + dy * scale)
}

/// 脸部塑形（关键点驱动；`face` 蒙版仅作保底限制）。
fn warp_face(
    px: f32,
    py: f32,
    landmarks: &[[f32; 2]],
    params: &RetouchParams,
    width: u32,
    height: u32,
) -> (f32, f32) {
    if landmarks.len() <= landmark::CHIN {
        return (px, py);
    }
    let (w, h) = (width as f32, height as f32);
    let oval_points: Vec<(f32, f32)> = landmark::FACE_OVAL
        .iter()
        .filter(|index| **index < landmarks.len())
        .map(|index| to_pixels(landmarks[*index], w, h))
        .collect();
    if oval_points.len() < 8 {
        return (px, py);
    }
    let chin = to_pixels(landmarks[landmark::CHIN], w, h);
    let forehead = to_pixels(landmarks[landmark::FOREHEAD], w, h);
    let face_height = (chin.1 - forehead.1).abs().max(1.0);
    let face_width = oval_points
        .iter()
        .map(|point| point.0)
        .fold((f32::MAX, f32::MIN), |acc, value| {
            (acc.0.min(value), acc.1.max(value))
        });
    let center_x = (face_width.0 + face_width.1) / 2.0;
    let half_width = ((face_width.1 - face_width.0) / 2.0).max(1.0);

    let (mut sx, mut sy) = (px, py);

    // 瘦脸：两颊（下颌中部）向中轴收拢。
    if params.face_slim > 0.0 {
        let strength = (params.face_slim / 100.0).clamp(0.0, 1.0) * 0.28;
        let cheek_y = forehead.1 + face_height * 0.62;
        for side in [-1.0f32, 1.0] {
            let center = (center_x + side * half_width * 0.72, cheek_y);
            let (nx, ny) = pull_toward_axis(
                sx,
                sy,
                center,
                center_x,
                half_width * 0.9,
                strength,
                face_height * 0.5,
            );
            sx = nx;
            sy = ny;
        }
    }

    // 下巴：轻微拉长 + 收窄。
    if params.chin > 0.0 {
        let strength = (params.chin / 100.0).clamp(0.0, 1.0);
        let radius = face_height * 0.35;
        let dx = px - chin.0;
        let dy = py - chin.1;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance < radius {
            let falloff = (1.0 - (distance / radius).powi(2)).powi(2);
            sy -= strength * falloff * face_height * 0.06; // 采样点上移 → 下巴拉长
            sx -= (px - center_x) * strength * falloff * 0.18; // 收窄
        }
    }

    // 大眼：以虹膜中心做径向放大。
    if params.eye_enlarge > 0.0 && landmarks.len() > landmark::RIGHT_IRIS {
        let strength = (params.eye_enlarge / 100.0).clamp(0.0, 1.0) * 0.30;
        for index in [landmark::LEFT_IRIS, landmark::RIGHT_IRIS] {
            let center = to_pixels(landmarks[index], w, h);
            let eye_width = if index == landmark::LEFT_IRIS {
                eye_extent(landmarks, &landmark::LEFT_EYE, w, h)
            } else {
                eye_extent(landmarks, &landmark::RIGHT_EYE, w, h)
            };
            let (nx, ny) = magnify(sx, sy, center, eye_width * 1.15, strength);
            sx = nx;
            sy = ny;
        }
    }

    // 瘦鼻：鼻翼向鼻梁收拢。
    if params.nose_slim > 0.0 && landmarks.len() > landmark::NOSE_WINGS[1] {
        let strength = (params.nose_slim / 100.0).clamp(0.0, 1.0) * 0.22;
        let left = to_pixels(landmarks[landmark::NOSE_WINGS[0]], w, h);
        let right = to_pixels(landmarks[landmark::NOSE_WINGS[1]], w, h);
        let nose_center = ((left.0 + right.0) / 2.0, (left.1 + right.1) / 2.0);
        let nose_width = (right.0 - left.0).abs().max(1.0);
        for side in [-1.0f32, 1.0] {
            let center = (nose_center.0 + side * nose_width * 0.5, nose_center.1);
            let (nx, ny) = pull_toward_axis(
                sx,
                sy,
                center,
                nose_center.0,
                nose_width * 0.9,
                strength,
                face_height * 0.2,
            );
            sx = nx;
            sy = ny;
        }
    }
    (sx, sy)
}

fn eye_extent(landmarks: &[[f32; 2]], indices: &[usize], width: f32, _height: f32) -> f32 {
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    for index in indices {
        if *index < landmarks.len() {
            let x = landmarks[*index][0] * width;
            min = min.min(x);
            max = max.max(x);
        }
    }
    if min > max {
        return 8.0;
    }
    ((max - min) / 2.0).max(4.0)
}

/// 人像轮廓剖面（预计算一次，避免逐像素扫描蒙版）。
struct BodyProfile {
    min_y: u32,
    body_height: f32,
    /// 每一行平滑后的 `(中轴, 半宽)`（无人像的行为空）。
    rows: Vec<Option<(f32, f32)>>,
}

impl BodyProfile {
    fn build(person: &MaskPlane) -> Self {
        let mut min_y = u32::MAX;
        let mut max_y = 0u32;
        let mut raw: Vec<Option<(f32, f32)>> = Vec::with_capacity(person.height as usize);
        for y in 0..person.height {
            let mut left = None;
            let mut right = None;
            for x in 0..person.width {
                if person.get(x, y) > 0.5 {
                    if left.is_none() {
                        left = Some(x);
                    }
                    right = Some(x);
                }
            }
            if let (Some(left), Some(right)) = (left, right) {
                min_y = min_y.min(y);
                max_y = max_y.max(y);
                raw.push(Some((
                    (left + right) as f32 / 2.0,
                    (right - left) as f32 / 2.0,
                )));
            } else {
                raw.push(None);
            }
        }
        // 竖直方向平滑轮廓：逐行突变（蒙版锯齿 / 手臂穿插）会让位移场出现硬接缝。
        let window = ((person.height as f32 * 0.02) as usize).clamp(2, 24);
        let mut rows: Vec<Option<(f32, f32)>> = Vec::with_capacity(raw.len());
        for y in 0..raw.len() {
            let start = y.saturating_sub(window);
            let end = (y + window + 1).min(raw.len());
            let mut sum_center = 0.0f32;
            let mut sum_half = 0.0f32;
            let mut count = 0usize;
            for item in raw[start..end].iter().flatten() {
                sum_center += item.0;
                sum_half += item.1;
                count += 1;
            }
            rows.push(if count == 0 {
                None
            } else {
                Some((sum_center / count as f32, sum_half / count as f32))
            });
        }
        Self {
            min_y: min_y.min(person.height.saturating_sub(1)),
            body_height: (max_y.saturating_sub(min_y)) as f32,
            rows,
        }
    }
}

/// 身体塑形：按人像蒙版轮廓，把腰腹两侧向中轴收拢。
fn slim_body(px: f32, py: f32, body: &BodyProfile, strength: f32) -> (f32, f32) {
    let strength = (strength / 100.0).clamp(0.0, 1.0);
    if strength <= 0.0 || body.body_height <= 0.0 {
        return (px, py);
    }
    let y = py.round().max(0.0) as u32;
    let Some(Some((center, half))) = body.rows.get(y as usize).copied() else {
        return (px, py);
    };
    if half <= 1.0 {
        return (px, py);
    }
    // 腰腹带：人像高度 42%..72%。
    let rel = (y.saturating_sub(body.min_y)) as f32 / body.body_height;
    if !(0.42..=0.72).contains(&rel) {
        return (px, py);
    }
    let band = 1.0 - ((rel - 0.57) / 0.15).abs().min(1.0);
    // 归一化横向位置：0 = 中轴，1 = 轮廓；>1 为轮廓外的过渡带（1.35 处归零）。
    let u = ((px - center).abs() / half).max(0.0);
    if u >= 1.35 {
        return (px, py);
    }
    // 权重：中轴 0（保住身体中轴内容）→ 轮廓处最大 → 轮廓外平滑归零（背景跟随、无硬接缝）。
    let weight = if u <= 1.0 {
        u
    } else {
        (1.0 - (u - 1.0) / 0.35).clamp(0.0, 1.0)
    };
    let amount = strength * band * weight * weight * 0.30;
    (px + (px - center) * amount, py)
}

/// 柔化蒙版边界（盒式模糊 + clamp）。
fn soften_mask(plane: &MaskPlane, radius: f32) -> MaskPlane {
    let mut data = plane.data.clone();
    blur_plane(
        &mut data,
        plane.width as usize,
        plane.height as usize,
        radius,
    );
    for value in &mut data {
        *value = value.clamp(0.0, 1.0);
    }
    MaskPlane {
        width: plane.width,
        height: plane.height,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skin_plane(width: u32, height: u32) -> MaskPlane {
        MaskPlane {
            width,
            height,
            data: vec![1.0; (width as usize) * (height as usize)],
        }
    }

    fn noisy_skin_image(width: u32, height: u32) -> image::RgbaImage {
        image::RgbaImage::from_fn(width, height, |x, y| {
            let base = 180.0 + 12.0 * ((x as f32 * 0.7).sin() + (y as f32 * 0.9).cos());
            let noise = ((x * 7919 + y * 104729) % 13) as f32 - 6.0;
            let value = (base + noise).clamp(0.0, 255.0) as u8;
            image::Rgba([
                value,
                (value as f32 * 0.82) as u8,
                (value as f32 * 0.75) as u8,
                255,
            ])
        })
    }

    fn high_frequency_energy(image: &image::RgbaImage, region: (u32, u32, u32, u32)) -> f32 {
        let mut energy = 0.0f32;
        let mut count = 0usize;
        for y in region.1..region.3 {
            for x in region.0..region.2 {
                let center = image.get_pixel(x, y).0[0] as f32;
                let right = image.get_pixel((x + 1).min(image.width() - 1), y).0[0] as f32;
                energy += (center - right).abs();
                count += 1;
            }
        }
        energy / count.max(1) as f32
    }

    #[test]
    fn identity_params_do_not_touch_pixels() {
        let image = noisy_skin_image(32, 32);
        let out = apply(&image, &RetouchMasks::default(), &RetouchParams::default());
        assert_eq!(out.as_raw(), image.as_raw());
    }

    #[test]
    fn smoothing_reduces_high_frequency_inside_skin_only() {
        let mut image = noisy_skin_image(64, 64);
        // 右半边标成非皮肤（蒙版 0）。
        let mut skin = skin_plane(64, 64);
        for y in 0..64 {
            for x in 32..64 {
                skin.data[(y * 64 + x) as usize] = 0.0;
            }
        }
        let before_left = high_frequency_energy(&image, (8, 8, 28, 56));
        let before_right = high_frequency_energy(&image, (36, 8, 56, 56));
        let params = RetouchParams {
            smooth_mid: 80.0,
            smooth_high: 70.0,
            ..RetouchParams::default()
        };
        let out = apply(
            &image,
            &RetouchMasks {
                skin: Some(skin),
                ..RetouchMasks::default()
            },
            &params,
        );
        let after_left = high_frequency_energy(&out, (8, 8, 28, 56));
        let after_right = high_frequency_energy(&out, (36, 8, 56, 56));
        assert!(
            after_left < before_left * 0.7,
            "皮肤区高频应显著下降：{before_left} → {after_left}"
        );
        assert!(
            (after_right - before_right).abs() < 0.5,
            "非皮肤区不应变化：{before_right} → {after_right}"
        );
        // 颜色关系保持（不发灰）：R > G > B 依旧。
        let pixel = out.get_pixel(16, 16).0;
        assert!(pixel[0] >= pixel[1] && pixel[1] >= pixel[2]);
        let _ = &mut image;
    }

    #[test]
    fn blemish_pass_removes_dark_spots_within_skin() {
        let mut image = image::RgbaImage::from_pixel(48, 48, image::Rgba([200, 168, 150, 255]));
        // 中心一个 5×5 的深色痘印。
        for y in 22..27 {
            for x in 22..27 {
                image.put_pixel(x, y, image::Rgba([150, 118, 104, 255]));
            }
        }
        let params = RetouchParams {
            blemish: 100.0,
            ..RetouchParams::default()
        };
        let out = apply(
            &image,
            &RetouchMasks {
                skin: Some(skin_plane(48, 48)),
                ..RetouchMasks::default()
            },
            &params,
        );
        let center = out.get_pixel(24, 24).0[0] as i32;
        let surround = out.get_pixel(10, 10).0[0] as i32;
        assert!(
            (center - surround).abs() <= 18,
            "痘印应被抹平：中心 {center}，周围 {surround}"
        );
    }

    #[test]
    fn skin_tone_unify_reduces_chroma_spread() {
        let mut image = image::RgbaImage::from_pixel(64, 64, image::Rgba([200, 170, 150, 255]));
        // 左半边偏红（色斑）。
        for y in 0..64 {
            for x in 0..32 {
                image.put_pixel(x, y, image::Rgba([205, 150, 130, 255]));
            }
        }
        // 用「两半的平均色度差」衡量色斑（比 min/max 更能反映整体收敛）。
        let chroma_gap = |img: &image::RgbaImage| -> f32 {
            let mean = |range: std::ops::Range<u32>| -> f32 {
                let mut sum = 0.0;
                let mut count = 0usize;
                for y in 8..56 {
                    for x in range.clone() {
                        let pixel = img.get_pixel(x, y).0;
                        sum += 0.5 * pixel[0] as f32
                            - 0.419 * pixel[1] as f32
                            - 0.081 * pixel[2] as f32;
                        count += 1;
                    }
                }
                sum / count.max(1) as f32
            };
            (mean(8..24) - mean(40..56)).abs()
        };
        let before = chroma_gap(&image);
        let params = RetouchParams {
            skin_tone: 90.0,
            ..RetouchParams::default()
        };
        let out = apply(
            &image,
            &RetouchMasks {
                skin: Some(skin_plane(64, 64)),
                ..RetouchMasks::default()
            },
            &params,
        );
        let after = chroma_gap(&out);
        assert!(
            after < before * 0.6,
            "肤色色度差异应收敛：{before} → {after}"
        );
    }

    fn face_landmarks(width: f32, height: f32) -> Vec<[f32; 2]> {
        // 合成一张「标准脸」：正椭圆外轮廓 + 眼 / 鼻 / 下巴。
        let mut points = vec![[0.5f32, 0.5f32]; 478];
        let (cx, cy) = (0.5, 0.5);
        let (rx, ry) = (0.18, 0.24);
        for (slot, index) in landmark::FACE_OVAL.iter().enumerate() {
            let angle =
                std::f32::consts::PI * (slot as f32 / (landmark::FACE_OVAL.len() - 1) as f32);
            let x = cx
                - (angle.cos() * rx)
                    * if angle > std::f32::consts::FRAC_PI_2 {
                        -1.0
                    } else {
                        1.0
                    };
            let y = cy + angle.sin() * ry;
            points[*index] = [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)];
        }
        points[landmark::FOREHEAD] = [cx, cy - ry];
        points[landmark::CHIN] = [cx, cy + ry];
        points[landmark::LEFT_IRIS] = [cx - 0.06, cy - 0.06];
        points[landmark::RIGHT_IRIS] = [cx + 0.06, cy - 0.06];
        for (slot, index) in landmark::LEFT_EYE.iter().enumerate() {
            let t = slot as f32 / landmark::LEFT_EYE.len() as f32 * std::f32::consts::TAU;
            points[*index] = [cx - 0.06 + 0.028 * t.cos(), cy - 0.06 + 0.012 * t.sin()];
        }
        for (slot, index) in landmark::RIGHT_EYE.iter().enumerate() {
            let t = slot as f32 / landmark::RIGHT_EYE.len() as f32 * std::f32::consts::TAU;
            points[*index] = [cx + 0.06 + 0.028 * t.cos(), cy - 0.06 + 0.012 * t.sin()];
        }
        points[landmark::NOSE_WINGS[0]] = [cx - 0.035, cy + 0.06];
        points[landmark::NOSE_WINGS[1]] = [cx + 0.035, cy + 0.06];
        points[landmark::NOSE_TIP] = [cx, cy + 0.07];
        let _ = (width, height);
        points
    }

    #[test]
    fn face_slim_narrows_jaw_region() {
        // 棋盘格便于观察位移方向。
        let image = image::RgbaImage::from_fn(200, 200, |x, y| {
            let value = if (x / 4 + y / 4) % 2 == 0 { 60 } else { 200 };
            image::Rgba([value, value, value, 255])
        });
        let masks = RetouchMasks {
            landmarks: face_landmarks(200.0, 200.0),
            face: Some(skin_plane(200, 200)),
            ..RetouchMasks::default()
        };
        let out = warp(
            &image,
            &masks,
            &RetouchParams {
                face_slim: 100.0,
                ..RetouchParams::default()
            },
        );
        assert_eq!(out.dimensions(), (200, 200));
        // 脸颊处（中心左右两侧）应发生位移（与原地不同）。
        let mut moved = 0usize;
        for y in 110..150 {
            for x in 30..70 {
                if image.get_pixel(x, y).0[0] != out.get_pixel(x, y).0[0] {
                    moved += 1;
                }
            }
        }
        assert!(moved > 200, "瘦脸应收窄脸颊区域（移动像素 {moved}）");
        // 远离面部（角落）不变。
        assert_eq!(image.get_pixel(5, 5).0, out.get_pixel(5, 5).0);
    }

    #[test]
    fn eye_enlarge_moves_pixels_around_iris() {
        let image = image::RgbaImage::from_fn(200, 200, |x, y| {
            let value = if (x / 3 + y / 3) % 2 == 0 { 70 } else { 190 };
            image::Rgba([value, value, value, 255])
        });
        let masks = RetouchMasks {
            landmarks: face_landmarks(200.0, 200.0),
            ..RetouchMasks::default()
        };
        let out = warp(
            &image,
            &masks,
            &RetouchParams {
                eye_enlarge: 100.0,
                ..RetouchParams::default()
            },
        );
        let mut moved = 0usize;
        for y in 80..100 {
            for x in 70..130 {
                if image.get_pixel(x, y).0[0] != out.get_pixel(x, y).0[0] {
                    moved += 1;
                }
            }
        }
        assert!(moved > 100, "大眼应改变眼周像素（移动 {moved}）");
    }

    #[test]
    fn body_slim_moves_waist_edges_inward() {
        let image = image::RgbaImage::from_fn(200, 300, |x, y| {
            let value = if (x / 4 + y / 4) % 2 == 0 { 80 } else { 180 };
            image::Rgba([value, value, value, 255])
        });
        let mut person = MaskPlane {
            width: 200,
            height: 300,
            data: vec![0.0; 200 * 300],
        };
        // 椭圆人像：腰腹从 40% 高度开始。
        for y in 40..280u32 {
            for x in 0..200u32 {
                let dx = (x as f32 - 100.0) / 55.0;
                let dy = (y as f32 - 160.0) / 130.0;
                if dx * dx + dy * dy <= 1.0 {
                    person.data[(y * 200 + x) as usize] = 1.0;
                }
            }
        }
        let params = RetouchParams {
            body_slim: 100.0,
            ..RetouchParams::default()
        };
        let out = warp(
            &image,
            &RetouchMasks {
                person: Some(person),
                ..RetouchMasks::default()
            },
            &params,
        );
        // 腰腹带（约 y=170）两侧边缘处应有位移。
        let mut moved = 0usize;
        for y in 150..200 {
            for x in 40..80 {
                if image.get_pixel(x, y).0[0] != out.get_pixel(x, y).0[0] {
                    moved += 1;
                }
            }
        }
        assert!(moved > 100, "瘦身应改变腰腹边缘（移动 {moved}）");
        // 头部区域不动。
        assert_eq!(image.get_pixel(100, 60).0, out.get_pixel(100, 60).0);
    }
}
