//! 阶段 3：蒙版系统。
//!
//! - 生成器：画笔 / 线性渐变 / 径向渐变 / 颜色范围 / 亮度范围 / 深度范围（预留）
//! - AI 蒙版（主体 / 天空 / 背景 / 人物 / 物体）在阶段 7 集成模型，本阶段返回明确提示
//! - 输出为 0..1 的权重栅格（每像素一个 f32），供 GPU 局部调整使用

use rayon::prelude::*;

use crate::error::{FdxmpError, Result};
pub use crate::params::{BrushSettings, MaskKind, MaskParams, OverlayColor, PeoplePart};

/// 蒙版栅格缓存键（只含影响栅格的字段 + 尺寸）。
///
/// GPU 权重纹理、叠加层与 AI 栅格缓存都用它判断是否需要重新生成。
/// **不包含 `adjustments` / `name` / `overlay`**：拖动局部调色滑块不应触发
/// 蒙版栅格重建（旧实现把整个 `MaskParams` 序列化进哈希，每次拖动都要重算
/// 画笔距离场 + 羽化模糊）。
pub fn mask_key(mask: &MaskParams, width: u32, height: u32) -> u64 {
    let json = serde_json::to_string(&(&mask.kind, mask.inverted, mask.feather, mask.opacity))
        .unwrap_or_default();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in json
        .as_bytes()
        .iter()
        .chain(width.to_le_bytes().iter())
        .chain(height.to_le_bytes().iter())
    {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 蒙版权重栅格（0.0 = 未选中，1.0 = 完全选中）。
#[derive(Debug, Clone, PartialEq)]
pub struct MaskRaster {
    /// 宽度。
    pub width: u32,
    /// 高度。
    pub height: u32,
    /// 逐像素权重（行优先）。
    pub weights: Vec<f32>,
}

impl MaskRaster {
    /// 新建全零栅格。
    pub fn zeros(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            weights: vec![0.0; (width as usize) * (height as usize)],
        }
    }

    /// 读取权重（越界返回 0）。
    pub fn get(&self, x: u32, y: u32) -> f32 {
        if x >= self.width || y >= self.height {
            return 0.0;
        }
        self.weights[(y as usize) * (self.width as usize) + (x as usize)]
    }

    /// 平均权重（快速评估蒙版覆盖率）。
    pub fn mean(&self) -> f32 {
        if self.weights.is_empty() {
            return 0.0;
        }
        self.weights.iter().sum::<f32>() / self.weights.len() as f32
    }
}

/// 按蒙版参数生成权重栅格。
///
/// `image` 用于颜色范围 / 亮度范围（其它类型忽略）。
pub fn generate_mask(
    mask: &MaskParams,
    image: &image::RgbaImage,
    width: u32,
    height: u32,
) -> Result<MaskRaster> {
    let mut raster = match &mask.kind {
        MaskKind::Brush(settings) => brush_mask(settings, width, height),
        MaskKind::LinearGradient { start, end } => linear_mask(*start, *end, width, height),
        MaskKind::RadialGradient {
            center,
            radii,
            rotation,
            inner,
        } => radial_mask(*center, *radii, *rotation, *inner, width, height),
        MaskKind::ColorRange {
            color,
            range,
            smoothness,
        } => color_range_mask(image, *color, *range, *smoothness),
        MaskKind::LuminanceRange {
            min,
            max,
            smoothness,
        } => luminance_range_mask(image, *min, *max, *smoothness),
        MaskKind::DepthRange { .. } => {
            return Err(FdxmpError::Unsupported(
                "深度范围蒙版需要深度图（预留接口）".to_string(),
            ));
        }
        MaskKind::AiSubject
        | MaskKind::AiSky
        | MaskKind::AiBackground
        | MaskKind::AiPeople { .. }
        | MaskKind::AiObject { .. } => {
            #[cfg(feature = "ai")]
            {
                match crate::ai::cached_raster(mask, width, height) {
                    Some(raster) if raster.weights.len() == (width * height) as usize => {
                        MaskRaster {
                            width,
                            height,
                            weights: raster.weights.clone(),
                        }
                    }
                    _ => {
                        return Err(FdxmpError::Unsupported(
                            "AI 蒙版尚未生成（请在蒙版面板点击 AI 按钮）".to_string(),
                        ))
                    }
                }
            }
            #[cfg(not(feature = "ai"))]
            {
                return Err(FdxmpError::Unsupported(
                    "AI 蒙版需要启用 ai feature（Windows ONNX Runtime）".to_string(),
                ));
            }
        }
    };

    if mask.inverted {
        for weight in &mut raster.weights {
            *weight = 1.0 - *weight;
        }
    }
    let opacity = (mask.opacity / 100.0).clamp(0.0, 1.0);
    if opacity < 1.0 {
        for weight in &mut raster.weights {
            *weight *= opacity;
        }
    }
    let feather = (mask.feather / 100.0).clamp(0.0, 1.0);
    if feather > 0.001 {
        let radius = (feather * 0.05 * width.min(height) as f32).round() as usize;
        if radius > 0 {
            box_blur(&mut raster, radius);
        }
    }
    Ok(raster)
}

/// 线性渐变：起点权重 1 → 终点 0。
pub fn linear_mask(start: [f32; 2], end: [f32; 2], width: u32, height: u32) -> MaskRaster {
    let mut raster = MaskRaster::zeros(width, height);
    let dir = [end[0] - start[0], end[1] - start[1]];
    let length_squared = (dir[0] * dir[0] + dir[1] * dir[1]).max(1e-6);
    raster
        .weights
        .par_chunks_mut(width as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let py = (y as f32 + 0.5) / height as f32;
            for (x, weight) in row.iter_mut().enumerate() {
                let px = (x as f32 + 0.5) / width as f32;
                let t = ((px - start[0]) * dir[0] + (py - start[1]) * dir[1]) / length_squared;
                *weight = 1.0 - t.clamp(0.0, 1.0);
            }
        });
    raster
}

/// 径向渐变：中心 1 → 边缘 0（椭圆 + 旋转，边缘平滑）。
pub fn radial_mask(
    center: [f32; 2],
    radii: [f32; 2],
    rotation: f32,
    inner: Option<f32>,
    width: u32,
    height: u32,
) -> MaskRaster {
    let mut raster = MaskRaster::zeros(width, height);
    let (sin, cos) = rotation.to_radians().sin_cos();
    let rx = radii[0].max(1e-3);
    let ry = radii[1].max(1e-3);
    let inner_scale = inner.unwrap_or(0.75).clamp(0.0, 0.98);
    raster
        .weights
        .par_chunks_mut(width as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let py = (y as f32 + 0.5) / height as f32 - center[1];
            for (x, weight) in row.iter_mut().enumerate() {
                let px = (x as f32 + 0.5) / width as f32 - center[0];
                let local_x = px * cos + py * sin;
                let local_y = -px * sin + py * cos;
                let distance = ((local_x / rx).powi(2) + (local_y / ry).powi(2)).sqrt();
                *weight = 1.0 - smoothstep(inner_scale, 1.0, distance);
            }
        });
    raster
}

/// 颜色范围：与目标颜色的距离在范围内选中。
pub fn color_range_mask(
    image: &image::RgbaImage,
    color: [f32; 3],
    range: f32,
    smoothness: f32,
) -> MaskRaster {
    let (width, height) = image.dimensions();
    let mut raster = MaskRaster::zeros(width, height);
    let target = [
        color[0].clamp(0.0, 1.0),
        color[1].clamp(0.0, 1.0),
        color[2].clamp(0.0, 1.0),
    ];
    let inner = (range / 100.0).clamp(0.0, 1.0) * 0.6;
    let outer = (inner + (smoothness / 100.0).clamp(0.02, 1.0) * 0.5).max(inner + 0.01);
    raster
        .weights
        .par_chunks_mut(width as usize)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, weight) in row.iter_mut().enumerate() {
                let pixel = image.get_pixel(x as u32, y as u32);
                let rgb = [
                    pixel[0] as f32 / 255.0,
                    pixel[1] as f32 / 255.0,
                    pixel[2] as f32 / 255.0,
                ];
                let distance = ((rgb[0] - target[0]).powi(2)
                    + (rgb[1] - target[1]).powi(2)
                    + (rgb[2] - target[2]).powi(2))
                .sqrt()
                    / 3f32.sqrt();
                *weight = 1.0 - smoothstep(inner, outer, distance);
            }
        });
    raster
}

/// 亮度范围：选中亮度落在 [min, max] 的像素。
pub fn luminance_range_mask(
    image: &image::RgbaImage,
    min: f32,
    max: f32,
    smoothness: f32,
) -> MaskRaster {
    let (width, height) = image.dimensions();
    let mut raster = MaskRaster::zeros(width, height);
    let soft = ((smoothness / 100.0).clamp(0.0, 1.0) * 0.5).max(0.001);
    let low = min.clamp(0.0, 1.0);
    let high = max.clamp(0.0, 1.0);
    raster
        .weights
        .par_chunks_mut(width as usize)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, weight) in row.iter_mut().enumerate() {
                let pixel = image.get_pixel(x as u32, y as u32);
                let luminance = (0.2126 * pixel[0] as f32
                    + 0.7152 * pixel[1] as f32
                    + 0.0722 * pixel[2] as f32)
                    / 255.0;
                let lower = smoothstep(low - soft, low + soft, luminance);
                let upper = 1.0 - smoothstep(high - soft, high + soft, luminance);
                *weight = lower * upper;
            }
        });
    raster
}

/// 画笔：沿笔画路径以圆形笔刷覆盖（含羽化 / 流动 / 密度）。
pub fn brush_mask(settings: &BrushSettings, width: u32, height: u32) -> MaskRaster {
    let mut raster = MaskRaster::zeros(width, height);
    if settings.strokes.is_empty() {
        return raster;
    }
    let long_edge = width.max(height) as f32;
    let radius = (settings.size / 100.0).clamp(0.002, 0.5) * long_edge * 0.5;
    let feather = (settings.feather / 100.0).clamp(0.0, 1.0);
    let density = (settings.density / 100.0).clamp(0.0, 1.0);
    let inner = radius * (1.0 - feather).clamp(0.0, 1.0);

    for stroke in &settings.strokes {
        if stroke.is_empty() {
            continue;
        }
        // 只用笔画包围盒 + 半径范围内的像素参与计算
        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        for point in stroke {
            min_x = min_x.min(point[0]);
            max_x = max_x.max(point[0]);
            min_y = min_y.min(point[1]);
            max_y = max_y.max(point[1]);
        }
        let pad = radius / long_edge * 2.0 + 0.02;
        let x0 = (((min_x - pad) * width as f32).floor().max(0.0)) as u32;
        let x1 = (((max_x + pad) * width as f32)
            .ceil()
            .min(width as f32 - 1.0)) as u32;
        let y0 = (((min_y - pad) * height as f32).floor().max(0.0)) as u32;
        let y1 = (((max_y + pad) * height as f32)
            .ceil()
            .min(height as f32 - 1.0)) as u32;
        if x1 < x0 || y1 < y0 {
            continue;
        }
        let row_start = y0 as usize;
        let row_end = (y1 + 1) as usize;
        let column_start = x0 as usize;
        let column_end = (x1 + 1) as usize;
        let width_usize = width as usize;

        raster.weights[row_start * width_usize..row_end * width_usize]
            .par_chunks_mut(width_usize)
            .enumerate()
            .for_each(|(row_index, row)| {
                let y = row_start + row_index;
                let py = (y as f32 + 0.5) / height as f32;
                for (x, cell) in row
                    .iter_mut()
                    .enumerate()
                    .take(column_end)
                    .skip(column_start)
                {
                    let px = (x as f32 + 0.5) / width as f32;
                    let distance = distance_to_polyline(&[px, py], stroke);
                    let coverage = if distance <= inner {
                        1.0
                    } else if distance >= radius {
                        0.0
                    } else {
                        1.0 - (distance - inner) / (radius - inner).max(1e-3)
                    };
                    if coverage > cell.max(0.0) {
                        *cell = coverage * density;
                    }
                }
            });
    }
    raster
}

fn distance_to_polyline(point: &[f32; 2], stroke: &[[f32; 2]]) -> f32 {
    if stroke.len() == 1 {
        let dx = point[0] - stroke[0][0];
        let dy = point[1] - stroke[0][1];
        return (dx * dx + dy * dy).sqrt();
    }
    let mut best = f32::MAX;
    for pair in stroke.windows(2) {
        best = best.min(distance_to_segment(point, &pair[0], &pair[1]));
    }
    best
}

fn distance_to_segment(point: &[f32; 2], start: &[f32; 2], end: &[f32; 2]) -> f32 {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let length_squared = dx * dx + dy * dy;
    if length_squared <= 1e-9 {
        let px = point[0] - start[0];
        let py = point[1] - start[1];
        return (px * px + py * py).sqrt();
    }
    let t = (((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / length_squared)
        .clamp(0.0, 1.0);
    let cx = start[0] + t * dx;
    let cy = start[1] + t * dy;
    let px = point[0] - cx;
    let py = point[1] - cy;
    (px * px + py * py).sqrt()
}

fn box_blur(raster: &mut MaskRaster, radius: usize) {
    let width = raster.width as usize;
    let height = raster.height as usize;
    if width == 0 || height == 0 || radius == 0 {
        return;
    }
    let mut temp = vec![0.0f32; raster.weights.len()];
    // 水平：前缀和（O(w·h)，旧实现每个像素累加整窗，羽化半径大时是 O(w·h·r)）
    temp.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        let source = &raster.weights[y * width..(y + 1) * width];
        let mut prefix = vec![0.0f32; width + 1];
        for x in 0..width {
            prefix[x + 1] = prefix[x] + source[x];
        }
        for (x, value) in row.iter_mut().enumerate() {
            let start = x.saturating_sub(radius);
            let end = (x + radius + 1).min(width);
            *value = (prefix[end] - prefix[start]) / (end - start) as f32;
        }
    });
    // 垂直：逐列滑动窗口（O(w·h)，额外内存仅一行）
    let mut column_sums = vec![0.0f32; width];
    let initial_end = (radius + 1).min(height);
    for source_y in 0..initial_end {
        let row = &temp[source_y * width..(source_y + 1) * width];
        for (sum, value) in column_sums.iter_mut().zip(row) {
            *sum += *value;
        }
    }
    // 垂直趟按行顺序滑动（每行只加 / 减边缘样本，O(w·h)）。
    for y in 0..height {
        let start = y.saturating_sub(radius);
        let end = (y + radius + 1).min(height);
        let divisor = (end - start) as f32;
        let row = &mut raster.weights[y * width..(y + 1) * width];
        for (x, value) in row.iter_mut().enumerate() {
            *value = column_sums[x] / divisor;
        }
        let next_start = (y + 1).saturating_sub(radius);
        let next_end = (y + 1 + radius + 1).min(height);
        if next_start > start {
            let removed = &temp[start * width..(start + 1) * width];
            for (x, sum) in column_sums.iter_mut().enumerate() {
                *sum -= removed[x];
            }
        }
        if next_end > end {
            let added = &temp[(next_end - 1) * width..next_end * width];
            for (x, sum) in column_sums.iter_mut().enumerate() {
                *sum += added[x];
            }
        }
    }
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    if (edge1 - edge0).abs() < 1e-6 {
        return if value < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 蒙版叠加颜色 → RGBA。
pub fn overlay_rgba(color: OverlayColor) -> [f32; 4] {
    match color {
        OverlayColor::Red => [1.0, 0.0, 0.0, 0.5],
        OverlayColor::Green => [0.0, 1.0, 0.0, 0.5],
        OverlayColor::Blue => [0.0, 0.0, 1.0, 0.5],
        OverlayColor::White => [1.0, 1.0, 1.0, 0.5],
        OverlayColor::Black => [0.0, 0.0, 0.0, 0.5],
    }
}

/// 生成器接口（保留给自定义蒙版）。
pub trait MaskGenerator {
    /// 生成权重栅格。
    fn generate(&self, width: u32, height: u32) -> MaskRaster;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::MaskKind;

    fn gray_image(width: u32, height: u32, value: u8) -> image::RgbaImage {
        image::RgbaImage::from_pixel(width, height, image::Rgba([value, value, value, 255]))
    }

    fn mask(kind: MaskKind) -> MaskParams {
        MaskParams {
            id: "test".to_string(),
            name: "test".to_string(),
            kind,
            ..MaskParams::default()
        }
    }

    #[test]
    fn linear_gradient_runs_from_start_to_end() {
        let raster = linear_mask([0.0, 0.5], [1.0, 0.5], 64, 32);
        assert!((raster.get(0, 16) - 1.0).abs() < 0.05, "起点应接近 1");
        assert!(raster.get(63, 16) < 0.05, "终点应接近 0");
        assert!(raster.get(16, 16) > raster.get(48, 16), "应单调递减");
    }

    #[test]
    fn radial_gradient_is_centered() {
        let raster = radial_mask([0.5, 0.5], [0.25, 0.25], 0.0, None, 64, 64);
        assert!(raster.get(32, 32) > 0.95, "中心应为 1");
        assert!(raster.get(0, 0) < 0.05, "角落应为 0");
    }

    #[test]
    fn color_range_selects_similar_colors() {
        let mut image = gray_image(16, 16, 0);
        for pixel in image.pixels_mut() {
            *pixel = image::Rgba([20, 40, 220, 255]);
        }
        let raster = color_range_mask(&image, [0.08, 0.16, 0.86], 30.0, 20.0);
        assert!(raster.mean() > 0.9, "目标颜色应被选中");

        let raster_other = color_range_mask(&image, [0.9, 0.1, 0.1], 10.0, 10.0);
        assert!(raster_other.mean() < 0.05, "不同颜色不应被选中");
    }

    #[test]
    fn luminance_range_selects_midtones() {
        let image = gray_image(16, 16, 128);
        let raster = luminance_range_mask(&image, 0.3, 0.7, 20.0);
        assert!(raster.mean() > 0.95, "中灰应被选中");

        let dark = gray_image(16, 16, 10);
        assert!(luminance_range_mask(&dark, 0.3, 0.7, 20.0).mean() < 0.05);
    }

    #[test]
    fn brush_covers_stroke_path() {
        let settings = BrushSettings {
            size: 20.0,
            feather: 50.0,
            flow: 100.0,
            density: 100.0,
            auto_mask: false,
            strokes: vec![vec![[0.2, 0.5], [0.8, 0.5]]],
        };
        let raster = brush_mask(&settings, 100, 100);
        assert!(raster.get(50, 50) > 0.9, "笔画中心应被选中");
        assert!(raster.get(50, 10) < 0.05, "远离笔画不应被选中");
    }

    #[test]
    fn invert_and_opacity_are_applied() {
        let image = gray_image(16, 16, 128);
        let mut params = mask(MaskKind::LuminanceRange {
            min: 0.3,
            max: 0.7,
            smoothness: 20.0,
        });
        params.inverted = true;
        params.opacity = 50.0;
        let raster = generate_mask(&params, &image, 16, 16).unwrap();
        assert!(raster.mean() < 0.05, "中灰反向后应接近 0");

        let mut params = mask(MaskKind::LuminanceRange {
            min: 0.3,
            max: 0.7,
            smoothness: 20.0,
        });
        params.opacity = 50.0;
        let raster = generate_mask(&params, &image, 16, 16).unwrap();
        assert!((raster.mean() - 0.5).abs() < 0.05, "不透明度 50% 应减半");
    }

    #[test]
    fn ai_masks_require_generated_raster() {
        let image = gray_image(8, 8, 128);
        let err = generate_mask(&mask(MaskKind::AiSky), &image, 8, 8).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("尚未生成") || message.contains("需要启用 ai feature"),
            "未生成时提示应说明原因：{message}"
        );
    }

    #[cfg(feature = "ai")]
    #[test]
    fn ai_masks_use_cached_raster() {
        let image = gray_image(8, 8, 128);
        let mask = mask(MaskKind::AiSky);
        let mut raster = MaskRaster::zeros(8, 8);
        for weight in raster.weights.iter_mut() {
            *weight = 0.75;
        }
        crate::ai::insert_cached(&mask, 8, 8, raster);
        let generated = generate_mask(&mask, &image, 8, 8).unwrap();
        assert!((generated.mean() - 0.75).abs() < 1e-4);
        crate::ai::clear_cache();
    }

    #[test]
    fn overlay_colors_are_translucent() {
        for color in [
            OverlayColor::Red,
            OverlayColor::Green,
            OverlayColor::Blue,
            OverlayColor::White,
            OverlayColor::Black,
        ] {
            assert_eq!(overlay_rgba(color)[3], 0.5);
        }
    }

    #[test]
    fn mask_key_ignores_adjustments_but_tracks_raster_fields() {
        let mask = MaskParams {
            id: "m".to_string(),
            feather: 10.0,
            ..MaskParams::default()
        };
        let base = mask_key(&mask, 100, 100);

        let mut adjusted = mask.clone();
        adjusted.adjustments.light.exposure = 1.5;
        adjusted.name = "改名".to_string();
        adjusted.overlay = OverlayColor::Blue;
        assert_eq!(
            base,
            mask_key(&adjusted, 100, 100),
            "局部调色参数不应使蒙版栅格缓存失效"
        );

        let mut feathered = mask.clone();
        feathered.feather = 60.0;
        assert_ne!(base, mask_key(&feathered, 100, 100));

        let mut other_kind = mask.clone();
        other_kind.kind = MaskKind::AiSky;
        assert_ne!(base, mask_key(&other_kind, 100, 100));
    }

    #[test]
    fn raster_is_independent_of_adjustments() {
        // 这是 mask_key 忽略 adjustments 的前提：栅格只由 kind / feather / opacity / inverted 决定。
        let image = gray_image(64, 64, 128);
        let mask = MaskParams {
            id: "m".to_string(),
            kind: MaskKind::LinearGradient {
                start: [0.0, 0.0],
                end: [1.0, 1.0],
            },
            feather: 30.0,
            ..MaskParams::default()
        };
        let mut adjusted = mask.clone();
        adjusted.adjustments.light.exposure = 2.0;
        adjusted.adjustments.color.saturation = -50.0;
        adjusted.name = "改名".to_string();
        let base = generate_mask(&mask, &image, 64, 64).unwrap();
        let other = generate_mask(&adjusted, &image, 64, 64).unwrap();
        assert_eq!(base.weights, other.weights, "局部调色不应改变蒙版栅格");
    }

    /// 朴素两点盒式模糊（作为滑动窗口实现的参考）。
    fn naive_box_blur(raster: &mut MaskRaster, radius: usize) {
        let width = raster.width as usize;
        let height = raster.height as usize;
        let mut temp = vec![0.0f32; raster.weights.len()];
        for y in 0..height {
            for x in 0..width {
                let start = x.saturating_sub(radius);
                let end = (x + radius + 1).min(width);
                let sum: f32 = (start..end).map(|sx| raster.weights[y * width + sx]).sum();
                temp[y * width + x] = sum / (end - start) as f32;
            }
        }
        for y in 0..height {
            for x in 0..width {
                let start = y.saturating_sub(radius);
                let end = (y + radius + 1).min(height);
                let sum: f32 = (start..end).map(|sy| temp[sy * width + x]).sum();
                raster.weights[y * width + x] = sum / (end - start) as f32;
            }
        }
    }

    #[test]
    fn box_blur_matches_naive_reference() {
        let mut raster = MaskRaster::zeros(17, 13);
        for (index, value) in raster.weights.iter_mut().enumerate() {
            *value = ((index * 37) % 100) as f32 / 100.0;
        }
        let mut reference = raster.clone();
        naive_box_blur(&mut reference, 3);
        box_blur(&mut raster, 3);
        for (index, (fast, slow)) in raster
            .weights
            .iter()
            .zip(reference.weights.iter())
            .enumerate()
        {
            assert!(
                (fast - slow).abs() < 1e-5,
                "滑动窗口与朴素实现不一致（#{index}）：{fast} vs {slow}"
            );
        }
    }
}
