//! 阶段 3：蒙版系统。
//!
//! - 生成器：画笔 / 线性渐变 / 径向渐变 / 颜色范围 / 亮度范围 / 深度范围（预留）
//! - AI 蒙版（主体 / 天空 / 背景 / 人物 / 物体）在阶段 7 集成模型，本阶段返回明确提示
//! - 输出为 0..1 的权重栅格（每像素一个 f32），供 GPU 局部调整使用

use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::error::{FdxmpError, Result};
pub use crate::params::{
    BrushSettings, BrushStroke, MaskKind, MaskParams, OverlayColor, PeoplePart,
};

/// AI / 平台蒙版栅格缓存。
///
/// 键：
/// - **基础键**：模型一次推理产出的原始栅格（任意分辨率），
///   由 ONNX（Windows）或平台提供者（iOS Vision / CoreML / NNAPI）提交；
/// - **精确键** `mask_key(mask, width, height)`：按渲染请求分辨率重采样后的副本。
///
/// 基础键只取「类型」等会改变蒙版形状的字段：可见性 / 反向 / 羽化 / 不透明度
/// 由 [`generate_mask`] 在取用后统一后处理，调整这些参数不会让已生成的
/// 智能蒙版失效（无需重新推理）。
///
/// 渲染、叠加、导出在不同分辨率下都会命中（自动双线性重采样），
/// 切换图片 / 参数变化时由 [`clear_ai_raster_cache`] 或 mask_key 变化自然失效。
pub(crate) fn ai_raster_cache() -> &'static Mutex<HashMap<u64, Arc<MaskRaster>>> {
    static CACHE: OnceLock<Mutex<HashMap<u64, Arc<MaskRaster>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// AI / 平台蒙版基础键：只保留「形状来源」字段（kind），
/// 忽略 visible / inverted / feather / opacity（这些由 `generate_mask` 后处理）。
fn ai_base_key(mask: &MaskParams) -> u64 {
    let mut normalized = mask.clone();
    normalized.visible = true;
    normalized.inverted = false;
    normalized.feather = 0.0;
    normalized.opacity = 100.0;
    mask_key(&normalized, 0, 0)
}

/// 提交一份 AI / 平台蒙版栅格（按「基础键」存储，分辨率任意）。
///
/// 权重会被 clamp 到 0..1；尺寸非法 / 数据量不匹配会被拒绝。
pub fn insert_ai_raster(mask: &MaskParams, raster: MaskRaster) -> Result<()> {
    if raster.width == 0
        || raster.height == 0
        || raster.weights.len() != (raster.width as usize) * (raster.height as usize)
    {
        return Err(FdxmpError::Unsupported(
            "AI 蒙版栅格尺寸非法或数据量不匹配".to_string(),
        ));
    }
    if raster.weights.iter().any(|value| !value.is_finite()) {
        return Err(FdxmpError::Unsupported("AI 蒙版包含非有限数值".to_string()));
    }
    let mut weights = raster.weights;
    for value in &mut weights {
        *value = value.clamp(0.0, 1.0);
    }
    let base = MaskRaster {
        width: raster.width,
        height: raster.height,
        weights,
    };
    let key = ai_base_key(mask);
    let cache = ai_raster_cache();
    let mut map = cache
        .lock()
        .map_err(|_| FdxmpError::Unsupported("AI 蒙版缓存锁失效".to_string()))?;
    // 同一张图内的多个 AI 蒙版可能共享缓存；上限 24 份，超出整体清空（幂等重建）。
    if map.len() >= 24 && !map.contains_key(&key) {
        map.clear();
    }
    map.insert(key, Arc::new(base));
    Ok(())
}

/// 查询 AI / 平台蒙版栅格；命中时按请求分辨率重采样并回填精确键。
pub fn cached_ai_raster(mask: &MaskParams, width: u32, height: u32) -> Option<MaskRaster> {
    let (width, height) = (width.max(1), height.max(1));
    let base_key = ai_base_key(mask);
    let exact_key = mask_key(mask, width, height);
    let cache = ai_raster_cache();
    {
        let map = cache.lock().ok()?;
        if let Some(hit) = map.get(&exact_key) {
            return Some((**hit).clone());
        }
    }
    let base = {
        let map = cache.lock().ok()?;
        map.get(&base_key).cloned()
    }?;
    let resized = if base.width == width && base.height == height {
        (*base).clone()
    } else {
        MaskRaster {
            width,
            height,
            weights: crate::smartmask::mask::resize_plane(
                &base.weights,
                base.width,
                base.height,
                width,
                height,
            ),
        }
    };
    if let Ok(mut map) = cache.lock() {
        if map.len() < 24 {
            map.insert(exact_key, Arc::new(resized.clone()));
        }
    }
    Some(resized)
}

/// 是否已有某个蒙版的基础栅格（FFI 状态查询用；不做重采样）。
pub fn ai_raster_ready(mask: &MaskParams) -> bool {
    let key = ai_base_key(mask);
    ai_raster_cache()
        .lock()
        .map(|map| map.contains_key(&key))
        .unwrap_or(false)
}

/// 清空全部 AI / 平台蒙版栅格（切换图片 / 释放缓存时调用）。
pub fn clear_ai_raster_cache() {
    if let Ok(mut map) = ai_raster_cache().lock() {
        map.clear();
    }
}

/// 测试用：串行化访问全局栅格缓存的用例（cargo test 默认多线程并行）。
#[cfg(test)]
pub(crate) fn test_cache_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 蒙版栅格缓存键（只含影响栅格的字段 + 尺寸）。
///
/// GPU 权重纹理、叠加层与 AI 栅格缓存都用它判断是否需要重新生成。
/// **不包含 `adjustments` / `name` / `overlay`**：拖动局部调色滑块不应触发
/// 蒙版栅格重建（旧实现把整个 `MaskParams` 序列化进哈希，每次拖动都要重算
/// 画笔距离场 + 羽化模糊）。
pub fn mask_key(mask: &MaskParams, width: u32, height: u32) -> u64 {
    let json = serde_json::to_string(&(
        &mask.kind,
        mask.visible,
        mask.inverted,
        mask.feather,
        mask.opacity,
    ))
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
    // 隐藏的蒙版不产生任何覆盖（也不做距离场 / 模糊计算）。
    if !mask.visible {
        return Ok(MaskRaster::zeros(width, height));
    }
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
            // 栅格由模型（Windows ONNX）或平台提供者（iOS Vision / CoreML）提交进缓存；
            // 这里只取用并按请求分辨率重采样，不关心来源。
            cached_ai_raster(mask, width, height).ok_or_else(|| {
                FdxmpError::Unsupported(
                    "AI 蒙版尚未生成（请在蒙版面板点击智能蒙版按钮）".to_string(),
                )
            })?
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

/// 线性渐变：起点一侧权重 1 → 终点一侧 0，中间 smoothstep 平滑过渡。
///
/// `start` / `end` 是过渡带的两条边界线锚点（垂直于渐变方向的两条线
/// 分别过这两点）；50% 中心线过 `(start + end) / 2`。
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
                *weight = 1.0 - smoothstep(0.0, 1.0, t);
            }
        });
    raster
}

/// 径向渐变：内圈 1 → 外圈 0（椭圆 + 旋转，边缘 smoothstep 过渡）。
///
/// 旋转在**像素等比空间**里做（`radii` 仍是 `[x, y]` 相对宽高的归一化半径）：
/// 屏幕上看到的就是标准旋转椭圆，而不是归一化空间旋转后被非等比缩放斜切过的形状。
/// `inner` 是内圈相对外圈的缩放（0.98 = 硬边缘，越小过渡越宽）。
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
    // 像素空间的半轴与中心。
    let ax = (radii[0].max(1e-3) * width as f32).max(1e-3);
    let ay = (radii[1].max(1e-3) * height as f32).max(1e-3);
    let cx = center[0] * width as f32;
    let cy = center[1] * height as f32;
    let inner_scale = inner.unwrap_or(0.75).clamp(0.0, 0.98);
    raster
        .weights
        .par_chunks_mut(width as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let py = (y as f32 + 0.5) - cy;
            for (x, weight) in row.iter_mut().enumerate() {
                let px = (x as f32 + 0.5) - cx;
                let local_x = px * cos + py * sin;
                let local_y = -px * sin + py * cos;
                let distance = ((local_x / ax).powi(2) + (local_y / ay).powi(2)).sqrt();
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

/// 画笔：沿笔画路径做**径向渐变 stamp 累积**（大小 / 羽化 / 流动 / 密度 + 擦除）。
///
/// 渲染模型与 Lightroom 一致：
/// - 每条笔画按落笔时的参数快照（缺省沿用蒙版当前设置）沿折线按间距盖章；
/// - 同一笔画内 stamp 重叠按"流动"累积：`a += (1 - a) · coverage · flow`；
/// - 单次落笔的覆盖上限由"密度"约束；
/// - 擦除笔画反向混合：`a *= 1 - coverage · flow`；
/// - 只对 stamp 包围盒逐行并行盖章，长笔画先按 stamp 间距抽稀。
pub fn brush_mask(settings: &BrushSettings, width: u32, height: u32) -> MaskRaster {
    let mut raster = MaskRaster::zeros(width, height);
    if settings.strokes.is_empty() || width == 0 || height == 0 {
        return raster;
    }
    let long_edge = width.max(height) as f32;
    for stroke in &settings.strokes {
        if stroke.is_empty() {
            continue;
        }
        let size = stroke.size.unwrap_or(settings.size);
        let feather = stroke.feather.unwrap_or(settings.feather);
        let flow = stroke.flow.unwrap_or(settings.flow);
        let density = stroke.density.unwrap_or(settings.density);
        let radius = (size / 100.0).clamp(0.002, 0.5) * long_edge * 0.5;
        let flow_norm = (flow / 100.0).clamp(0.0, 1.0);
        let density_norm = (density / 100.0).clamp(0.0, 1.0);
        if radius <= 0.0 || flow_norm <= 0.0 || density_norm <= 0.0 {
            continue;
        }
        let inner = radius * (1.0 - (feather / 100.0).clamp(0.0, 1.0));
        for stamp in stroke_stamps(&stroke.points, width, height, radius) {
            stamp_brush(
                &mut raster,
                stamp[0],
                stamp[1],
                radius,
                inner,
                flow_norm,
                density_norm,
                stroke.erase,
            );
        }
    }
    raster
}

/// 把折线按 stamp 间距展开成像素坐标的落笔点（首点必落，末段不足间距则丢弃）。
fn stroke_stamps(points: &[[f32; 2]], width: u32, height: u32, radius: f32) -> Vec<[f32; 2]> {
    /// 单条笔画的 stamp 上限（防御性限幅，避免病态输入卡死）。
    const MAX_STAMPS: usize = 8192;
    let to_pixel = |point: &[f32; 2]| {
        [
            point[0].clamp(0.0, 1.0) * width as f32,
            point[1].clamp(0.0, 1.0) * height as f32,
        ]
    };
    let spacing = (radius * 0.25).max(1.0);
    let mut stamps = Vec::new();
    let first = to_pixel(&points[0]);
    stamps.push(first);
    let mut since = 0.0f32;
    for pair in points.windows(2) {
        let a = to_pixel(&pair[0]);
        let b = to_pixel(&pair[1]);
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let length = (dx * dx + dy * dy).sqrt();
        if length <= 1e-6 {
            continue;
        }
        let dir = [dx / length, dy / length];
        let mut travelled = 0.0f32;
        while since + (length - travelled) >= spacing {
            travelled += spacing - since;
            stamps.push([a[0] + dir[0] * travelled, a[1] + dir[1] * travelled]);
            since = 0.0;
            if stamps.len() >= MAX_STAMPS {
                return stamps;
            }
        }
        since += length - travelled;
    }
    stamps
}

/// 单个 stamp：在包围盒内按羽化径向渐变累积（或擦除）权重。
#[allow(clippy::too_many_arguments)]
fn stamp_brush(
    raster: &mut MaskRaster,
    cx: f32,
    cy: f32,
    radius: f32,
    inner: f32,
    flow: f32,
    density: f32,
    erase: bool,
) {
    let width = raster.width;
    let height = raster.height;
    if width == 0 || height == 0 {
        return;
    }
    let x0 = (cx - radius).floor().max(0.0) as u32;
    let y0 = (cy - radius).floor().max(0.0) as u32;
    let x1 = (cx + radius).ceil().min(width as f32 - 1.0).max(0.0) as u32;
    let y1 = (cy + radius).ceil().min(height as f32 - 1.0).max(0.0) as u32;
    if x1 < x0 || y1 < y0 {
        return;
    }
    let row_start = y0 as usize;
    let row_end = (y1 + 1) as usize;
    let column_start = x0 as usize;
    let column_end = (x1 + 1) as usize;
    let width_usize = width as usize;
    let area = (column_end - column_start) * (row_end - row_start);
    let splat = |row: &mut [f32], row_index: usize| {
        let y = row_start + row_index;
        let dy = (y as f32 + 0.5) - cy;
        for (x, cell) in row
            .iter_mut()
            .enumerate()
            .take(column_end)
            .skip(column_start)
        {
            let dx = (x as f32 + 0.5) - cx;
            let distance = (dx * dx + dy * dy).sqrt();
            if distance >= radius {
                continue;
            }
            let falloff = if distance <= inner {
                1.0
            } else {
                1.0 - smoothstep(inner, radius, distance)
            };
            let coverage = falloff * flow;
            if erase {
                *cell *= 1.0 - coverage;
            } else {
                *cell = (*cell + (1.0 - *cell) * coverage).min(density);
            }
        }
    };
    let region = &mut raster.weights[row_start * width_usize..row_end * width_usize];
    if area >= 4096 {
        region
            .par_chunks_mut(width_usize)
            .enumerate()
            .for_each(|(row_index, row)| splat(row, row_index));
    } else {
        for (row_index, row) in region.chunks_mut(width_usize).enumerate() {
            splat(row, row_index);
        }
    }
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
    fn radial_rotation_is_aspect_correct() {
        // 200×100：rx=0.25（像素半轴 50px）、ry=0.1（10px），旋转 90° 后长轴变成竖直。
        let raster = radial_mask([0.5, 0.5], [0.25, 0.1], 90.0, Some(0.9), 200, 100);
        // 中心上方 40px 在长轴（50px）以内 → 选中；正右方 20px 超过短轴（10px）→ 未选中。
        assert!(
            raster.get(100, 10) > 0.9,
            "旋转 90° 后竖直方向仍在椭圆内：{}",
            raster.get(100, 10)
        );
        assert!(
            raster.get(120, 50) < 0.1,
            "旋转 90° 后水平方向超出短轴：{}",
            raster.get(120, 50)
        );
        // 未旋转时相反：水平 40px 在长轴内，竖直 20px 超出短轴。
        let unrotated = radial_mask([0.5, 0.5], [0.25, 0.1], 0.0, Some(0.9), 200, 100);
        assert!(unrotated.get(140, 50) > 0.9);
        assert!(unrotated.get(100, 30) < 0.1);
    }

    #[test]
    fn radial_feather_transitions_between_inner_and_outer() {
        // inner=0.5：内圈（半径一半处）为 1，外圈为 0，中间平滑过渡。
        let raster = radial_mask([0.5, 0.5], [0.4, 0.4], 0.0, Some(0.5), 100, 100);
        assert!(raster.get(50, 50) > 0.99, "中心应完全选中");
        assert!(raster.get(50, 30) > 0.9, "内圈以内应接近 1"); // 半径 20/40 = 0.5
        assert!(raster.get(50, 10) < 0.1, "外圈附近应接近 0"); // 半径 40/40 = 1.0
        let mid = raster.get(50, 20); // 半径 30/40 = 0.75，内圈与外圈之间
        assert!(mid > 0.1 && mid < 0.9, "内外圈之间应平滑过渡：{mid}");
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
            strokes: vec![BrushStroke::new(vec![[0.2, 0.5], [0.8, 0.5]])],
        };
        let raster = brush_mask(&settings, 100, 100);
        assert!(raster.get(50, 50) > 0.9, "笔画中心应被选中");
        assert!(raster.get(50, 10) < 0.05, "远离笔画不应被选中");
    }

    #[test]
    fn brush_erase_stroke_subtracts_coverage() {
        let mut settings = BrushSettings {
            size: 20.0,
            feather: 100.0,
            flow: 100.0,
            density: 100.0,
            auto_mask: false,
            strokes: vec![BrushStroke::new(vec![[0.2, 0.5], [0.8, 0.5]])],
        };
        let painted = brush_mask(&settings, 100, 100);
        assert!(painted.get(50, 50) > 0.9);

        // 擦除笔画（从右往左擦回）应把中间擦掉，端点保留。
        settings
            .strokes
            .push(BrushStroke::erase(vec![[0.9, 0.5], [0.5, 0.5]]));
        let erased = brush_mask(&settings, 100, 100);
        assert!(erased.get(85, 50) < 0.1, "擦除笔画起点应完全擦掉");
        assert!(
            erased.get(50, 50) < painted.get(50, 50) * 0.5,
            "擦除后中心覆盖应显著下降：{} vs {}",
            erased.get(50, 50),
            painted.get(50, 50)
        );
        assert!(
            erased.get(30, 50) > 0.5,
            "未经过的笔画段应保留覆盖：{}",
            erased.get(30, 50)
        );
    }

    #[test]
    fn brush_flow_and_density_limit_coverage() {
        let stroke = BrushStroke::new(vec![[0.5, 0.5]]);
        // 密度 60、流动 50：单点落笔上限不超过密度。
        let settings = BrushSettings {
            size: 40.0,
            feather: 0.0,
            flow: 50.0,
            density: 60.0,
            auto_mask: false,
            strokes: vec![stroke.clone()],
        };
        let raster = brush_mask(&settings, 200, 200);
        let center = raster.get(100, 100);
        assert!(
            center > 0.25 && center <= 0.6 + 1e-3,
            "密度应封顶：{center}"
        );

        // 流动 100、密度 100：一笔应接近全选。
        let full = BrushSettings {
            size: 40.0,
            feather: 0.0,
            flow: 100.0,
            density: 100.0,
            auto_mask: false,
            strokes: vec![stroke],
        };
        assert!(brush_mask(&full, 200, 200).get(100, 100) > 0.95);
    }

    #[test]
    fn brush_stroke_parameter_snapshot_wins_over_mask_settings() {
        let mut stroke = BrushStroke::new(vec![[0.5, 0.5]]);
        stroke.density = Some(20.0);
        let settings = BrushSettings {
            size: 40.0,
            feather: 0.0,
            flow: 100.0,
            density: 100.0,
            auto_mask: false,
            strokes: vec![stroke],
        };
        let raster = brush_mask(&settings, 200, 200);
        assert!(raster.get(100, 100) <= 0.2 + 1e-3, "落笔参数快照应优先生效");
    }

    #[test]
    fn brush_strokes_parse_legacy_and_object_formats() {
        // v0.3 旧格式：纯点数组。
        let legacy = r#"{"size":30.0,"feather":50.0,"flow":100.0,"density":100.0,"auto_mask":false,"strokes":[[[0.1,0.2],[0.3,0.4]]]}"#;
        let settings: BrushSettings = serde_json::from_str(legacy).expect("旧格式应能解析");
        assert_eq!(settings.strokes.len(), 1);
        assert_eq!(settings.strokes[0].points.len(), 2);
        assert!(!settings.strokes[0].erase);

        // v0.4 对象格式：笔画自带参数与擦除标记。
        let modern = r#"{"size":30.0,"strokes":[{"points":[[0.1,0.2]],"erase":true,"size":12.0,"feather":40.0,"flow":80.0,"density":90.0}]}"#;
        let settings: BrushSettings = serde_json::from_str(modern).expect("对象格式应能解析");
        assert!(settings.strokes[0].erase);
        assert_eq!(settings.strokes[0].size, Some(12.0));

        // 序列化回去仍是对象格式。
        let json = serde_json::to_string(&settings).expect("应能序列化");
        assert!(json.contains("\"erase\":true"), "{json}");
    }

    #[test]
    fn invisible_mask_is_empty() {
        let image = gray_image(16, 16, 128);
        let mut params = mask(MaskKind::LuminanceRange {
            min: 0.3,
            max: 0.7,
            smoothness: 20.0,
        });
        params.visible = false;
        let raster = generate_mask(&params, &image, 16, 16).unwrap();
        assert!(raster.mean() < 1e-6, "隐藏的蒙版不应有任何覆盖");

        params.visible = true;
        assert!(generate_mask(&params, &image, 16, 16).unwrap().mean() > 0.9);
    }

    #[test]
    fn linear_transition_is_smoothstep() {
        let raster = linear_mask([0.0, 0.5], [1.0, 0.5], 101, 16);
        // 边界处为 100% / 0%，中心线为 50%。
        assert!(raster.get(0, 8) > 0.99, "起点边界应接近 1");
        assert!(raster.get(100, 8) < 0.01, "终点边界应接近 0");
        let middle = raster.get(50, 8);
        assert!(
            (middle - 0.5).abs() < 0.02,
            "中心线（50% 过渡线）应接近 0.5：{middle}"
        );
        // smoothstep 在靠近两端时切线为 0（比线性坡道更平），中段更陡：
        // 25% 处应高于线性坡道的 0.75。
        let quarter = raster.get(25, 8);
        assert!(
            (quarter - 0.841).abs() < 0.02,
            "过渡应是 smoothstep（25% 处 ≈0.841）而非线性 0.75：{quarter}"
        );
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
            message.contains("尚未生成"),
            "未生成时提示应说明原因：{message}"
        );
    }

    #[test]
    fn ai_masks_use_submitted_raster_and_resample() {
        let _guard = test_cache_lock();
        let image = gray_image(8, 8, 128);
        let mask = mask(MaskKind::AiSky);
        // 平台提供者提交 4×4 基础栅格 → 8×8 请求应自动重采样命中。
        let mut raster = MaskRaster::zeros(4, 4);
        for weight in raster.weights.iter_mut() {
            *weight = 0.75;
        }
        insert_ai_raster(&mask, raster).unwrap();
        assert!(ai_raster_ready(&mask));
        let generated = generate_mask(&mask, &image, 8, 8).unwrap();
        assert!((generated.mean() - 0.75).abs() < 1e-4);
        // 非法栅格必须被拒绝（NaN / 数据量不匹配）。
        assert!(insert_ai_raster(
            &mask,
            MaskRaster {
                width: 2,
                height: 2,
                weights: vec![0.0; 3],
            }
        )
        .is_err());
        assert!(insert_ai_raster(
            &mask,
            MaskRaster {
                width: 2,
                height: 2,
                weights: vec![f32::NAN; 4],
            }
        )
        .is_err());
        clear_ai_raster_cache();
        assert!(!ai_raster_ready(&mask));
        assert!(
            generate_mask(&mask, &image, 8, 8).is_err(),
            "清空后应回到未生成状态"
        );
    }

    #[test]
    fn ai_raster_survives_feather_opacity_and_visibility_changes() {
        let _guard = test_cache_lock();
        let image = gray_image(8, 8, 128);
        let base = mask(MaskKind::AiSubject);
        let mut raster = MaskRaster::zeros(4, 4);
        for weight in raster.weights.iter_mut() {
            *weight = 1.0;
        }
        insert_ai_raster(&base, raster).unwrap();
        // 调羽化 / 不透明度 / 反向 / 可见性：基础栅格不失效，后处理仍然生效。
        let mut adjusted = base.clone();
        adjusted.feather = 0.0;
        adjusted.opacity = 50.0;
        assert!(ai_raster_ready(&adjusted));
        let generated = generate_mask(&adjusted, &image, 8, 8).unwrap();
        assert!((generated.mean() - 0.5).abs() < 1e-3, "不透明度 50% 生效");
        let mut inverted = base.clone();
        inverted.inverted = true;
        let generated = generate_mask(&inverted, &image, 8, 8).unwrap();
        assert!(generated.mean() < 0.01, "反向生效");
        let mut hidden = base.clone();
        hidden.visible = false;
        let generated = generate_mask(&hidden, &image, 8, 8).unwrap();
        assert_eq!(generated.mean(), 0.0, "隐藏的蒙版不产生覆盖");
        // 换类型（形状来源不同）必须失效。
        let other = mask(MaskKind::AiSky);
        assert!(!ai_raster_ready(&other));
        clear_ai_raster_cache();
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
