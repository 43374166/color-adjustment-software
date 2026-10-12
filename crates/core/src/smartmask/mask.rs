//! 标准蒙版数据与后处理：羽化 / 形态学 / 组合 / 坐标映射 / 多边形光栅化。
//!
//! 所有蒙版都是 **f32 0..1 权重 + 与原图一致的坐标**，带元数据
//! （类型、置信度、来源模型、模型版本、生成时间）。可羽化、可组合、可缓存。

use serde::{Deserialize, Serialize};

use crate::error::{FdxmpError, Result};
use crate::masks::MaskRaster;

/// 坐标 / 尺寸上限（防 DoS）：单边最多 8192 像素。
pub const MAX_MASK_EDGE: u32 = 8192;

/// 标准蒙版类型（后续新增类型只扩展本枚举，不破坏旧代码）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartMaskKind {
    /// 主体（显著目标）。
    Subject,
    /// 天空。
    Sky,
    /// 背景。
    Background,
    /// 人像（整人）。
    Portrait,
    /// 皮肤。
    Skin,
    /// 头发。
    Hair,
    /// 头部。
    Head,
    /// 面部。
    Face,
    /// 眼睛。
    Eyes,
    /// 眉毛。
    Eyebrows,
    /// 鼻子。
    Nose,
    /// 嘴唇。
    Lips,
    /// 牙齿。
    Teeth,
    /// 耳朵。
    Ears,
    /// 衣服。
    Clothes,
}

impl SmartMaskKind {
    /// 稳定的字符串标识。
    pub fn as_str(self) -> &'static str {
        match self {
            SmartMaskKind::Subject => "subject",
            SmartMaskKind::Sky => "sky",
            SmartMaskKind::Background => "background",
            SmartMaskKind::Portrait => "portrait",
            SmartMaskKind::Skin => "skin",
            SmartMaskKind::Hair => "hair",
            SmartMaskKind::Head => "head",
            SmartMaskKind::Face => "face",
            SmartMaskKind::Eyes => "eyes",
            SmartMaskKind::Eyebrows => "eyebrows",
            SmartMaskKind::Nose => "nose",
            SmartMaskKind::Lips => "lips",
            SmartMaskKind::Teeth => "teeth",
            SmartMaskKind::Ears => "ears",
            SmartMaskKind::Clothes => "clothes",
        }
    }
}

/// 蒙版组合方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskCombine {
    /// 并集（软取 `max`）。
    Union,
    /// 交集（软取 `min`）。
    Intersection,
    /// 差集（`a × (1 - b)`）。
    Difference,
    /// 排除（对称差 `|a - b|`）。
    Exclude,
}

/// 形态学运算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskMorphology {
    /// 膨胀。
    Dilate,
    /// 腐蚀。
    Erode,
    /// 开运算（先腐蚀后膨胀，去噪点）。
    Open,
    /// 闭运算（先膨胀后腐蚀，补空洞）。
    Close,
}

/// 标准蒙版：权重栅格 + 元数据。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartMask {
    /// 类型。
    pub kind: SmartMaskKind,
    /// 宽度（像素）。
    pub width: u32,
    /// 高度（像素）。
    pub height: u32,
    /// 逐像素权重（行优先，0..1）。
    pub data: Vec<f32>,
    /// 置信度 0..1。
    pub confidence: f32,
    /// 来源模型（注册表里的模型名）。
    pub source_model: String,
    /// 模型版本（参与缓存键）。
    pub model_version: String,
    /// 生成时间（Unix 毫秒；0 = 未知）。
    pub generated_at: u64,
}

impl SmartMask {
    /// 新建蒙版：尺寸 / 数据量校验 + 权重 clamp 到 0..1。
    pub fn new(kind: SmartMaskKind, width: u32, height: u32, mut data: Vec<f32>) -> Result<Self> {
        let expected = (width as usize).checked_mul(height as usize);
        let Some(expected) = expected else {
            return Err(FdxmpError::Unsupported("蒙版尺寸溢出".to_string()));
        };
        if width == 0 || height == 0 || width > MAX_MASK_EDGE || height > MAX_MASK_EDGE {
            return Err(FdxmpError::Unsupported(format!(
                "蒙版尺寸非法：{width}×{height}（上限 {MAX_MASK_EDGE}）"
            )));
        }
        if data.len() != expected {
            return Err(FdxmpError::Unsupported(format!(
                "蒙版权重数量不匹配：{} != {width}×{height}",
                data.len()
            )));
        }
        if data.iter().any(|value| !value.is_finite()) {
            return Err(FdxmpError::Unsupported("蒙版包含非有限数值".to_string()));
        }
        for value in &mut data {
            *value = value.clamp(0.0, 1.0);
        }
        Ok(Self {
            kind,
            width,
            height,
            data,
            confidence: 1.0,
            source_model: String::new(),
            model_version: String::new(),
            generated_at: 0,
        })
    }

    /// 全零蒙版。
    pub fn zeros(kind: SmartMaskKind, width: u32, height: u32) -> Self {
        Self {
            kind,
            width,
            height,
            data: vec![0.0; (width as usize) * (height as usize)],
            confidence: 0.0,
            source_model: String::new(),
            model_version: String::new(),
            generated_at: 0,
        }
    }

    /// 补上来源模型 / 版本元数据（构建器风格）。
    pub fn with_source(mut self, model: impl Into<String>, version: impl Into<String>) -> Self {
        self.source_model = model.into();
        self.model_version = version.into();
        self
    }

    /// 补上置信度。
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// 记录生成时间（Unix 毫秒；供 `DataRecorder` 与元数据使用）。
    pub fn with_timestamp(mut self, unix_ms: u64) -> Self {
        self.generated_at = unix_ms;
        self
    }

    /// 像素数。
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// 读取权重（越界返回 0）。
    pub fn get(&self, x: u32, y: u32) -> f32 {
        if x >= self.width || y >= self.height {
            return 0.0;
        }
        self.data[(y as usize) * (self.width as usize) + (x as usize)]
    }

    /// 平均权重。
    pub fn mean(&self) -> f32 {
        if self.data.is_empty() {
            return 0.0;
        }
        self.data.iter().sum::<f32>() / self.data.len() as f32
    }

    /// 阈值以上的覆盖率（0..1）。
    pub fn coverage(&self, threshold: f32) -> f32 {
        if self.data.is_empty() {
            return 0.0;
        }
        let count = self
            .data
            .iter()
            .filter(|value| **value >= threshold)
            .count();
        count as f32 / self.data.len() as f32
    }

    /// 阈值以上的包围盒 `(x0, y0, x1, y1)`（右下开区间）；空蒙版返回 `None`。
    pub fn bbox(&self, threshold: f32) -> Option<(u32, u32, u32, u32)> {
        let mut min_x = self.width;
        let mut min_y = self.height;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        let mut any = false;
        for y in 0..self.height {
            for x in 0..self.width {
                if self.get(x, y) >= threshold {
                    any = true;
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        any.then_some((min_x, min_y, max_x + 1, max_y + 1))
    }

    /// 反转权重。
    pub fn invert(&mut self) {
        for value in &mut self.data {
            *value = 1.0 - *value;
        }
    }

    /// 与另一个蒙版按方式组合（尺寸需一致）。
    pub fn combine(&mut self, other: &SmartMask, op: MaskCombine) -> Result<()> {
        if self.width != other.width || self.height != other.height {
            return Err(FdxmpError::Unsupported(format!(
                "蒙版尺寸不同，无法组合：{}×{} vs {}×{}",
                self.width, self.height, other.width, other.height
            )));
        }
        for (a, b) in self.data.iter_mut().zip(other.data.iter()) {
            let (x, y) = (*a, *b);
            *a = match op {
                MaskCombine::Union => x.max(y),
                MaskCombine::Intersection => x.min(y),
                MaskCombine::Difference => x * (1.0 - y),
                MaskCombine::Exclude => (x - y).abs(),
            }
            .clamp(0.0, 1.0);
        }
        Ok(())
    }

    /// 可分离盒式模糊（羽化），半径以像素计（3 次迭代近似高斯，O(1)/像素）。
    pub fn feather(&mut self, radius_px: f32) {
        let radius = radius_px.round().clamp(0.0, 512.0) as usize;
        if radius == 0 || self.data.is_empty() {
            return;
        }
        for _ in 0..3 {
            box_blur_plane(
                &mut self.data,
                self.width as usize,
                self.height as usize,
                radius,
            );
        }
    }

    /// 形态学运算（可分离 min/max 滤波，半径以像素计）。
    pub fn morphology(&mut self, op: MaskMorphology, radius_px: f32) {
        let radius = radius_px.round().clamp(0.0, 256.0) as usize;
        if radius == 0 || self.data.is_empty() {
            return;
        }
        match op {
            MaskMorphology::Dilate => self.dilate(radius),
            MaskMorphology::Erode => self.erode(radius),
            MaskMorphology::Open => {
                self.erode(radius);
                self.dilate(radius);
            }
            MaskMorphology::Close => {
                self.dilate(radius);
                self.erode(radius);
            }
        }
    }

    /// 膨胀（局部取最大；分离式两趟）。
    pub fn dilate(&mut self, radius: usize) {
        separable_extreme(
            &mut self.data,
            self.width as usize,
            self.height as usize,
            radius,
            true,
        );
    }

    /// 腐蚀（局部取最小；分离式两趟）。
    pub fn erode(&mut self, radius: usize) {
        separable_extreme(
            &mut self.data,
            self.width as usize,
            self.height as usize,
            radius,
            false,
        );
    }

    /// 双线性缩放（保持权重为 0..1）。
    pub fn resize_to(&self, width: u32, height: u32) -> Result<SmartMask> {
        let resized = resize_plane(&self.data, self.width, self.height, width, height);
        Ok(SmartMask {
            kind: self.kind,
            width,
            height,
            data: resized,
            confidence: self.confidence,
            source_model: self.source_model.clone(),
            model_version: self.model_version.clone(),
            generated_at: self.generated_at,
        })
    }

    /// 按仿射映射重采样（把对齐空间里的蒙版映射回原图坐标）。
    ///
    /// `map` 为源坐标 → 目标坐标的变换；输出为目标空间的蒙版。
    pub fn resample_with(&self, map: &AffineMap, width: u32, height: u32) -> Result<SmartMask> {
        let inverse = map
            .invert()
            .ok_or_else(|| FdxmpError::Unsupported("仿射变换不可逆（行列式为 0）".to_string()))?;
        let mut data = vec![0.0f32; (width as usize) * (height as usize)];
        for y in 0..height {
            for x in 0..width {
                let (sx, sy) = inverse.apply(x as f64 + 0.5, y as f64 + 0.5);
                data[(y as usize) * (width as usize) + (x as usize)] = sample_bilinear(
                    &self.data,
                    self.width,
                    self.height,
                    sx as f32 - 0.5,
                    sy as f32 - 0.5,
                );
            }
        }
        Ok(SmartMask {
            kind: self.kind,
            width,
            height,
            data,
            confidence: self.confidence,
            source_model: self.source_model.clone(),
            model_version: self.model_version.clone(),
            generated_at: self.generated_at,
        })
    }

    /// 转为核心渲染用的权重栅格。
    pub fn to_raster(&self) -> MaskRaster {
        MaskRaster {
            width: self.width,
            height: self.height,
            weights: self.data.clone(),
        }
    }

    /// 从核心权重栅格构建（元数据由调用方补齐）。
    pub fn from_raster(kind: SmartMaskKind, raster: &MaskRaster) -> Result<SmartMask> {
        SmartMask::new(kind, raster.width, raster.height, raster.weights.clone())
    }
}

/// 多边形蒙版（归一化坐标）：五官等由关键点围出的区域。
///
/// `include` 为并集填充，`exclude` 在其上抠除；坐标 `(x, y)` ∈ 0..1，
/// 原点在左上角，与图像坐标一致（Vision 的 y-up 需调用方翻转）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PolygonMask {
    /// 并入填充的多边形列表。
    pub include: Vec<Vec<[f32; 2]>>,
    /// 从填充中扣除的多边形列表。
    pub exclude: Vec<Vec<[f32; 2]>>,
}

impl PolygonMask {
    /// 是否包含任何多边形。
    pub fn is_empty(&self) -> bool {
        self.include.iter().all(|polygon| polygon.len() < 3)
            && self.exclude.iter().all(|polygon| polygon.len() < 3)
    }

    /// 光栅化为 `width × height` 的蒙版；`feather_px` 为羽化半径（像素）。
    pub fn rasterize(
        &self,
        kind: SmartMaskKind,
        width: u32,
        height: u32,
        feather_px: f32,
    ) -> Result<SmartMask> {
        if width == 0 || height == 0 || width > MAX_MASK_EDGE || height > MAX_MASK_EDGE {
            return Err(FdxmpError::Unsupported(format!(
                "多边形蒙版尺寸非法：{width}×{height}（上限 {MAX_MASK_EDGE}）"
            )));
        }
        if self.is_empty() {
            return Err(FdxmpError::Unsupported("多边形蒙版为空".to_string()));
        }
        let mut data = vec![0.0f32; (width as usize) * (height as usize)];
        for polygon in &self.include {
            fill_polygon(&mut data, width, height, polygon, 1.0);
        }
        for polygon in &self.exclude {
            fill_polygon(&mut data, width, height, polygon, 0.0);
        }
        let mut mask = SmartMask::new(kind, width, height, data)?;
        mask.feather(feather_px);
        Ok(mask)
    }
}

/// 仿射变换（2×3 矩阵）：`[a b c; d e f]`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AffineMap {
    /// 线性项（2×2）。
    pub linear: [[f64; 2]; 2],
    /// 平移项。
    pub offset: [f64; 2],
}

impl AffineMap {
    /// 恒等变换。
    pub fn identity() -> Self {
        Self {
            linear: [[1.0, 0.0], [0.0, 1.0]],
            offset: [0.0, 0.0],
        }
    }

    /// 平移变换。
    pub fn translation(dx: f64, dy: f64) -> Self {
        Self {
            linear: [[1.0, 0.0], [0.0, 1.0]],
            offset: [dx, dy],
        }
    }

    /// 相似变换（缩放 + 旋转 + 平移）：`p → s · R · p + t`。
    pub fn similarity(scale: f64, rotation_rad: f64, tx: f64, ty: f64) -> Self {
        let (sin, cos) = rotation_rad.sin_cos();
        Self {
            linear: [[scale * cos, -scale * sin], [scale * sin, scale * cos]],
            offset: [tx, ty],
        }
    }

    /// 最小二乘相似变换：把 `from` 点集对齐到 `to` 点集（≥ 2 对点）。
    ///
    /// 人脸对齐的标准做法（5 点关键点 → 模板）。
    pub fn similarity_from_points(from: &[[f64; 2]], to: &[[f64; 2]]) -> Option<Self> {
        if from.len() != to.len() || from.len() < 2 {
            return None;
        }
        let n = from.len() as f64;
        let mean = |points: &[[f64; 2]]| {
            let sum = points.iter().fold([0.0f64; 2], |acc, point| {
                [acc[0] + point[0], acc[1] + point[1]]
            });
            [sum[0] / n, sum[1] / n]
        };
        let (ca, cb) = (mean(from), mean(to));
        let mut dot = 0.0;
        let mut cross = 0.0;
        let mut norm_a = 0.0;
        for (a, b) in from.iter().zip(to.iter()) {
            let ax = a[0] - ca[0];
            let ay = a[1] - ca[1];
            let bx = b[0] - cb[0];
            let by = b[1] - cb[1];
            dot += ax * bx + ay * by;
            cross += ax * by - ay * bx;
            norm_a += ax * ax + ay * ay;
        }
        if norm_a <= f64::EPSILON {
            return None;
        }
        let (sin, cos) = cross.atan2(dot).sin_cos();
        let scale = (dot * dot + cross * cross).sqrt() / norm_a;
        let rotation = Self {
            linear: [[scale * cos, -scale * sin], [scale * sin, scale * cos]],
            offset: [0.0, 0.0],
        };
        let rotated = rotation.apply(ca[0], ca[1]);
        Some(Self {
            linear: rotation.linear,
            offset: [cb[0] - rotated.0, cb[1] - rotated.1],
        })
    }

    /// 应用变换。
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.linear[0][0] * x + self.linear[0][1] * y + self.offset[0],
            self.linear[1][0] * x + self.linear[1][1] * y + self.offset[1],
        )
    }

    /// 逆变换（行列式为 0 时返回 `None`）。
    pub fn invert(&self) -> Option<Self> {
        let [[a, b], [c, d]] = self.linear;
        let det = a * d - b * c;
        if det.abs() < 1e-12 {
            return None;
        }
        let inv = [[d / det, -b / det], [-c / det, a / det]];
        let (tx, ty) = (
            -(inv[0][0] * self.offset[0] + inv[0][1] * self.offset[1]),
            -(inv[1][0] * self.offset[0] + inv[1][1] * self.offset[1]),
        );
        Some(Self {
            linear: inv,
            offset: [tx, ty],
        })
    }
}

/// 双线性缩放单通道平面（公开给 FFI / 平台提供者复用）。
pub fn resize_plane(
    plane: &[f32],
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> Vec<f32> {
    let (sw, sh) = (src_width.max(1), src_height.max(1));
    let (dw, dh) = (dst_width.max(1), dst_height.max(1));
    if sw == dw && sh == dh && plane.len() >= (sw * sh) as usize {
        return plane[..(sw * sh) as usize].to_vec();
    }
    let mut out = vec![0.0f32; (dw * dh) as usize];
    for y in 0..dh {
        let fy = (y as f32 + 0.5) * sh as f32 / dh as f32 - 0.5;
        for x in 0..dw {
            let fx = (x as f32 + 0.5) * sw as f32 / dw as f32 - 0.5;
            out[(y * dw + x) as usize] = sample_bilinear(plane, sw, sh, fx, fy);
        }
    }
    out
}

/// 双线性采样（越界按边缘 clamp）。
pub fn sample_bilinear(plane: &[f32], width: u32, height: u32, x: f32, y: f32) -> f32 {
    if plane.is_empty() || width == 0 || height == 0 {
        return 0.0;
    }
    if plane.len() < (width as usize) * (height as usize) {
        return 0.0;
    }
    let x = x.clamp(0.0, (width - 1) as f32);
    let y = y.clamp(0.0, (height - 1) as f32);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let at = |px: u32, py: u32| plane[(py * width + px) as usize];
    let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
    let bottom = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
    top * (1.0 - ty) + bottom * ty
}

/// 可分离盒式模糊（就地，水平 + 垂直两趟）。
fn box_blur_plane(plane: &mut [f32], width: usize, height: usize, radius: usize) {
    if width == 0 || height == 0 || radius == 0 {
        return;
    }
    let mut temp = vec![0.0f32; plane.len()];
    // 水平趟。
    for y in 0..height {
        let row = y * width;
        let mut sum = 0.0f32;
        let mut count = 0usize;
        let window = |i: isize| -> Option<f32> {
            if i < 0 || i >= width as isize {
                None
            } else {
                Some(plane[row + i as usize])
            }
        };
        for i in -(radius as isize)..=radius as isize {
            if let Some(value) = window(i) {
                sum += value;
                count += 1;
            }
        }
        for x in 0..width {
            temp[row + x] = sum / count.max(1) as f32;
            if let Some(value) = window(x as isize - radius as isize) {
                sum -= value;
                count -= 1;
            }
            if let Some(value) = window(x as isize + radius as isize + 1) {
                sum += value;
                count += 1;
            }
        }
    }
    // 垂直趟。
    for x in 0..width {
        let mut sum = 0.0f32;
        let mut count = 0usize;
        let window = |i: isize| -> Option<f32> {
            if i < 0 || i >= height as isize {
                None
            } else {
                Some(temp[i as usize * width + x])
            }
        };
        for i in -(radius as isize)..=radius as isize {
            if let Some(value) = window(i) {
                sum += value;
                count += 1;
            }
        }
        for y in 0..height {
            plane[y * width + x] = sum / count.max(1) as f32;
            if let Some(value) = window(y as isize - radius as isize) {
                sum -= value;
                count -= 1;
            }
            if let Some(value) = window(y as isize + radius as isize + 1) {
                sum += value;
                count += 1;
            }
        }
    }
}

/// 可分离极值滤波（膨胀 = max，腐蚀 = min）。
fn separable_extreme(plane: &mut [f32], width: usize, height: usize, radius: usize, dilate: bool) {
    if width == 0 || height == 0 || radius == 0 {
        return;
    }
    let pick = |a: f32, b: f32| if dilate { a.max(b) } else { a.min(b) };
    let extreme = |values: &[f32]| -> f32 {
        let mut iter = values.iter().copied();
        let first = iter.next().unwrap_or(0.0);
        iter.fold(first, pick)
    };
    let mut temp = vec![0.0f32; plane.len()];
    for y in 0..height {
        let row = y * width;
        for x in 0..width {
            let start = x.saturating_sub(radius);
            let end = (x + radius + 1).min(width);
            temp[row + x] = extreme(&plane[row + start..row + end]);
        }
    }
    for x in 0..width {
        for y in 0..height {
            let start = y.saturating_sub(radius);
            let end = (y + radius + 1).min(height);
            let mut best = temp[start * width + x];
            for row in (start + 1)..end {
                best = pick(best, temp[row * width + x]);
            }
            plane[y * width + x] = best;
        }
    }
}

/// 扫描线填充多边形（4 次子扫描抗锯齿；`value` 为覆盖值 1.0 / 0.0）。
fn fill_polygon(data: &mut [f32], width: u32, height: u32, polygon: &[[f32; 2]], value: f32) {
    if polygon.len() < 3 || width == 0 || height == 0 {
        return;
    }
    let points: Vec<(f64, f64)> = polygon
        .iter()
        .map(|point| {
            (
                (point[0].clamp(-1.0, 2.0) as f64) * width as f64,
                (point[1].clamp(-1.0, 2.0) as f64) * height as f64,
            )
        })
        .collect();
    const SUB: usize = 4;
    let mut row_coverage = vec![0.0f32; width as usize];
    for y in 0..height {
        row_coverage.iter_mut().for_each(|value| *value = 0.0);
        let mut covered = false;
        for sub in 0..SUB {
            let sample_y = y as f64 + (sub as f64 + 0.5) / SUB as f64;
            let mut crossings: Vec<f64> = Vec::with_capacity(points.len());
            for index in 0..points.len() {
                let (x0, y0) = points[index];
                let (x1, y1) = points[(index + 1) % points.len()];
                if (y0 <= sample_y && y1 > sample_y) || (y1 <= sample_y && y0 > sample_y) {
                    let t = (sample_y - y0) / (y1 - y0);
                    crossings.push(x0 + t * (x1 - x0));
                }
            }
            if crossings.len() < 2 {
                continue;
            }
            crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            for pair in crossings.as_chunks::<2>().0 {
                let (mut start, end) = (pair[0], pair[1]);
                if end <= 0.0 || start >= width as f64 {
                    continue;
                }
                start = start.max(0.0);
                let end = end.min(width as f64);
                let first = start.floor() as usize;
                let last = (end.ceil() as usize).min(width as usize);
                for (index, coverage) in row_coverage.iter_mut().enumerate().take(last).skip(first)
                {
                    let pixel_start = (index as f64).max(start);
                    let pixel_end = ((index + 1) as f64).min(end);
                    if pixel_end > pixel_start {
                        *coverage += ((pixel_end - pixel_start) as f32) / SUB as f32;
                        covered = true;
                    }
                }
            }
        }
        if !covered {
            continue;
        }
        let row = (y as usize) * (width as usize);
        for (x, coverage) in row_coverage.iter().enumerate() {
            let coverage = coverage.clamp(0.0, 1.0);
            if coverage > 0.0 {
                let target = &mut data[row + x];
                *target = if value >= 0.5 {
                    (*target).max(coverage.min(1.0_f32))
                } else {
                    (*target).min(1.0 - coverage)
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot_mask(kind: SmartMaskKind, size: u32) -> SmartMask {
        let mut mask = SmartMask::zeros(kind, size, size);
        let center = size as f32 / 2.0;
        for y in 0..size {
            for x in 0..size {
                let dx = x as f32 + 0.5 - center;
                let dy = y as f32 + 0.5 - center;
                if (dx * dx + dy * dy).sqrt() <= 3.0 {
                    mask.data[(y * size + x) as usize] = 1.0;
                }
            }
        }
        mask
    }

    #[test]
    fn new_validates_and_clamps() {
        assert!(SmartMask::new(SmartMaskKind::Face, 2, 2, vec![0.5; 4]).is_ok());
        assert!(SmartMask::new(SmartMaskKind::Face, 2, 2, vec![0.5; 3]).is_err());
        assert!(SmartMask::new(SmartMaskKind::Face, 0, 2, Vec::new()).is_err());
        assert!(SmartMask::new(SmartMaskKind::Face, 2, 2, vec![f32::NAN; 4]).is_err());
        let mask = SmartMask::new(SmartMaskKind::Face, 1, 1, vec![7.0]).unwrap();
        assert_eq!(mask.data[0], 1.0, "越界权重被 clamp");
    }

    #[test]
    fn combine_ops_follow_soft_mask_math() {
        let mut a = SmartMask::new(SmartMaskKind::Eyes, 2, 1, vec![0.8, 0.2]).unwrap();
        let b = SmartMask::new(SmartMaskKind::Lips, 2, 1, vec![0.5, 0.5]).unwrap();
        let mut union = a.clone();
        union.combine(&b, MaskCombine::Union).unwrap();
        assert!((union.data[0] - 0.8).abs() < 1e-6);
        assert!((union.data[1] - 0.5).abs() < 1e-6);

        let mut intersection = a.clone();
        intersection.combine(&b, MaskCombine::Intersection).unwrap();
        assert!((intersection.data[0] - 0.5).abs() < 1e-6);
        assert!((intersection.data[1] - 0.2).abs() < 1e-6);

        let mut difference = a.clone();
        difference.combine(&b, MaskCombine::Difference).unwrap();
        assert!((difference.data[0] - 0.4).abs() < 1e-6, "0.8 × (1-0.5)");

        let mut exclude = a.clone();
        exclude.combine(&b, MaskCombine::Exclude).unwrap();
        assert!((exclude.data[0] - 0.3).abs() < 1e-6);
        assert!((exclude.data[1] - 0.3).abs() < 1e-6);

        let wide = SmartMask::zeros(SmartMaskKind::Face, 3, 1);
        assert!(
            a.combine(&wide, MaskCombine::Union).is_err(),
            "尺寸不同必须报错"
        );
    }

    #[test]
    fn morphology_dilate_erode_and_close_open() {
        let mut mask = dot_mask(SmartMaskKind::Skin, 16);
        let before = mask.coverage(0.5);
        let mut dilated = mask.clone();
        dilated.morphology(MaskMorphology::Dilate, 1.0);
        assert!(dilated.coverage(0.5) > before, "膨胀后覆盖变大");
        let mut eroded = mask.clone();
        eroded.morphology(MaskMorphology::Erode, 1.0);
        assert!(eroded.coverage(0.5) < before, "腐蚀后覆盖变小");
        // 闭运算补空洞：中心挖孔后应被填回。
        mask.data[(8 * 16 + 8) as usize] = 0.0;
        let mut closed = mask.clone();
        closed.morphology(MaskMorphology::Close, 1.0);
        assert!(closed.get(8, 8) > 0.5);
        // 开运算去孤立噪点。
        let mut noisy = dot_mask(SmartMaskKind::Skin, 16);
        noisy.data[0] = 1.0;
        let mut opened = noisy.clone();
        opened.morphology(MaskMorphology::Open, 1.0);
        assert!(opened.get(0, 0) < 0.5, "孤立噪点被清除");
        assert!(opened.get(8, 8) > 0.5, "主体保留");
    }

    #[test]
    fn feather_softens_hard_edge() {
        let mut mask = SmartMask::zeros(SmartMaskKind::Face, 16, 16);
        for y in 0..16 {
            for x in 0..8 {
                mask.data[(y * 16 + x) as usize] = 1.0;
            }
        }
        mask.feather(2.0);
        let edge = mask.get(7, 8);
        assert!(
            edge > 0.15 && edge < 0.95,
            "边界像素应为过渡值（0.15..0.95）：{edge}"
        );
        assert!(mask.get(2, 8) > 0.9, "远离边界处保持不透明");
        assert!(mask.get(14, 8) < 0.1, "远离边界处保持透明");
        // 羽化半径越大，过渡带越宽（边界像素越接近 0.5）。
        let mut wide = SmartMask::zeros(SmartMaskKind::Face, 16, 16);
        for y in 0..16 {
            for x in 0..8 {
                wide.data[(y * 16 + x) as usize] = 1.0;
            }
        }
        wide.feather(4.0);
        assert!(
            (wide.get(7, 8) - 0.5).abs() <= (edge - 0.5).abs() + 0.35,
            "更大半径的边界值更平缓：{} vs {edge}",
            wide.get(7, 8)
        );
    }

    #[test]
    fn resize_keeps_constant_plane() {
        let plane = vec![0.25f32; 4];
        let resized = resize_plane(&plane, 2, 2, 8, 6);
        assert_eq!(resized.len(), 48);
        assert!(resized.iter().all(|value| (value - 0.25).abs() < 1e-5));
    }

    #[test]
    fn polygon_rasterizes_include_minus_exclude() {
        let polygons = PolygonMask {
            include: vec![vec![[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]]],
            exclude: vec![vec![[0.4, 0.4], [0.6, 0.4], [0.6, 0.6], [0.4, 0.6]]],
        };
        let mask = polygons
            .rasterize(SmartMaskKind::Face, 100, 100, 0.0)
            .unwrap();
        assert!(mask.get(20, 20) > 0.9, "外圈属于 include");
        assert!(mask.get(50, 50) < 0.1, "中心被 exclude 抠除");
        assert!(mask.get(2, 2) < 0.1, "多边形外为空");
        // 羽化后边界变软。
        let soft = polygons
            .rasterize(SmartMaskKind::Face, 100, 100, 3.0)
            .unwrap();
        assert!(soft.get(10, 50) > 0.05 && soft.get(10, 50) < 0.95);
    }

    #[test]
    fn polygon_empty_and_oversized_are_rejected() {
        let empty = PolygonMask::default();
        assert!(empty.rasterize(SmartMaskKind::Face, 8, 8, 0.0).is_err());
        let square = PolygonMask {
            include: vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]],
            exclude: Vec::new(),
        };
        assert!(square.rasterize(SmartMaskKind::Face, 0, 8, 0.0).is_err());
        assert!(square
            .rasterize(SmartMaskKind::Face, MAX_MASK_EDGE + 1, 8, 0.0)
            .is_err());
    }

    #[test]
    fn affine_inverse_round_trips_and_similarity_recovers_transform() {
        let map = AffineMap::similarity(2.0, std::f64::consts::FRAC_PI_2, 10.0, -5.0);
        let (x, y) = map.apply(3.0, 4.0);
        let inverse = map.invert().unwrap();
        let (rx, ry) = inverse.apply(x, y);
        assert!((rx - 3.0).abs() < 1e-9 && (ry - 4.0).abs() < 1e-9);

        // 用 5 个点反解相似变换（缩放 0.5、旋转 30°、平移）。
        let source = [
            [0.0, 0.0],
            [10.0, 0.0],
            [0.0, 10.0],
            [5.0, 5.0],
            [10.0, 10.0],
        ];
        let rotation = std::f64::consts::PI / 6.0;
        let truth = AffineMap::similarity(0.5, rotation, 7.0, -3.0);
        let target: Vec<[f64; 2]> = source
            .iter()
            .map(|point| {
                let (x, y) = truth.apply(point[0], point[1]);
                [x, y]
            })
            .collect();
        let solved = AffineMap::similarity_from_points(&source, &target).unwrap();
        for point in source {
            let (tx, ty) = truth.apply(point[0], point[1]);
            let (sx, sy) = solved.apply(point[0], point[1]);
            assert!((tx - sx).abs() < 1e-6 && (ty - sy).abs() < 1e-6);
        }
        assert!(AffineMap::similarity_from_points(&source[..1], &target[..1]).is_none());
    }

    #[test]
    fn resample_with_identity_and_translation() {
        let mask = dot_mask(SmartMaskKind::Eyes, 9);
        let same = mask.resample_with(&AffineMap::identity(), 9, 9).unwrap();
        assert!((same.mean() - mask.mean()).abs() < 1e-6);

        // 平移半个像素以内：中心权重仍在。
        let shifted = mask
            .resample_with(&AffineMap::translation(1.0, 0.0), 9, 9)
            .unwrap();
        assert!(shifted.get(5, 4) > 0.9);

        // 缩放 2 倍（画布同样放大）：覆盖率基本不变，但包围盒翻倍。
        let scaled = mask
            .resample_with(&AffineMap::similarity(2.0, 0.0, 0.0, 0.0), 18, 18)
            .unwrap();
        assert!((scaled.coverage(0.5) - mask.coverage(0.5)).abs() < 0.05);
        let (sx0, _, sx1, _) = scaled.bbox(0.5).unwrap();
        let (ox0, _, ox1, _) = mask.bbox(0.5).unwrap();
        assert!(sx1 - sx0 >= (ox1 - ox0) * 2 - 2, "缩放后包围盒翻倍");

        let degenerate = AffineMap {
            linear: [[0.0, 0.0], [0.0, 0.0]],
            offset: [0.0, 0.0],
        };
        assert!(mask.resample_with(&degenerate, 9, 9).is_err());
    }

    #[test]
    fn bbox_and_coverage_report_regions() {
        let mask = dot_mask(SmartMaskKind::Lips, 16);
        let (x0, y0, x1, y1) = mask.bbox(0.5).unwrap();
        assert!(x0 >= 4 && y0 >= 4 && x1 <= 12 && y1 <= 12);
        assert!(mask.coverage(0.5) > 0.0);
        let empty = SmartMask::zeros(SmartMaskKind::Nose, 4, 4);
        assert!(empty.bbox(0.5).is_none());
    }
}
