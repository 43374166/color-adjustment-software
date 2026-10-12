//! 双阶段调度：`MaskPipeline` 负责并行、缓存、人脸对齐与坐标映射、蒙版装配。
//!
//! 调度策略（对应主提示词「三、双阶段流程」）：
//! 1. 第一阶段并行：人脸检测 + 人像粗分割（只负责「看哪里」）；
//! 2. 有人脸：第二阶段并行跑「人脸对齐 → 五官解析」与「人像精细抠图」；
//! 3. 无人脸：只保留人像路径（粗分割，必要时精细抠图）；
//! 4. 所有蒙版映射回原图坐标（仿射逆变换 + 边界 clamp），带元数据。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rayon::prelude::*;

use crate::error::{FdxmpError, Result};
use crate::smartmask::mask::{AffineMap, SmartMask, SmartMaskKind, MAX_MASK_EDGE};
use crate::smartmask::traits::{
    FaceDetection, Image, MaskRefiner, Stage1FaceDetector, Stage1PersonSegmenter, Stage2FaceParser,
    Stage2PersonMatting,
};

/// 双阶段流水线配置。
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineConfig {
    /// 最多处理的人脸数。
    pub max_faces: usize,
    /// 人脸对齐后的输入边长（像素）。
    pub align_size: u32,
    /// 精细抠图裁剪的外扩比例（相对人像包围盒）。
    pub matting_margin: f32,
    /// 无人脸时是否跳过精细抠图（纯人像也能直接出图，省一次推理）。
    pub skip_matting_without_face: bool,
    /// 装配蒙版的羽化半径（像素，0 = 不羽化）。
    pub feather_px: f32,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_faces: 4,
            align_size: 256,
            matting_margin: 0.12,
            skip_matting_without_face: false,
            feather_px: 1.5,
        }
    }
}

/// 一次双阶段推理产出的标准蒙版集合（全部与原图同坐标）。
#[derive(Debug, Clone)]
pub struct MaskSet {
    /// 全部蒙版（按类型排序去重）。
    pub masks: Vec<SmartMask>,
    /// 检测到的人脸（归一化坐标）。
    pub faces: Vec<FaceDetection>,
    /// 模型版本组合（缓存键的一部分，写入元数据）。
    pub versions: String,
    /// 生成时间（Unix 毫秒；0 = 未知）。
    pub generated_at: u64,
}

impl MaskSet {
    /// 按类型取蒙版。
    pub fn get(&self, kind: SmartMaskKind) -> Option<&SmartMask> {
        self.masks.iter().find(|mask| mask.kind == kind)
    }

    /// 是否包含某个类型。
    pub fn contains(&self, kind: SmartMaskKind) -> bool {
        self.get(kind).is_some()
    }
}

/// 双阶段蒙版流水线（组件全部可替换；模型缺失时用 Mock 或平台实现接入）。
pub struct MaskPipeline {
    detector: Arc<dyn Stage1FaceDetector>,
    segmenter: Arc<dyn Stage1PersonSegmenter>,
    parser: Option<Arc<dyn Stage2FaceParser>>,
    matting: Option<Arc<dyn Stage2PersonMatting>>,
    refiner: Option<Arc<dyn MaskRefiner>>,
    config: PipelineConfig,
    cache: Mutex<HashMap<u64, Arc<MaskSet>>>,
}

impl MaskPipeline {
    /// 组装流水线（第二阶段可缺省：缺省时只出人像 / 主体蒙版）。
    pub fn new(
        detector: Arc<dyn Stage1FaceDetector>,
        segmenter: Arc<dyn Stage1PersonSegmenter>,
        parser: Option<Arc<dyn Stage2FaceParser>>,
        matting: Option<Arc<dyn Stage2PersonMatting>>,
        refiner: Option<Arc<dyn MaskRefiner>>,
        config: PipelineConfig,
    ) -> Self {
        Self {
            detector,
            segmenter,
            parser,
            matting,
            refiner,
            config,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// 全部 Mock 的流水线（无模型也能跑通双阶段；测试 / UI 联调用）。
    pub fn mock(face_present: bool) -> Self {
        use crate::smartmask::mock::{
            MockFaceDetector, MockFaceParser, MockPersonMatting, MockPersonSegmenter, MockRefiner,
        };
        Self::new(
            Arc::new(MockFaceDetector { face_present }),
            Arc::new(MockPersonSegmenter),
            Some(Arc::new(MockFaceParser)),
            Some(Arc::new(MockPersonMatting)),
            Some(Arc::new(MockRefiner)),
            PipelineConfig::default(),
        )
    }

    /// 配置。
    pub fn config(&self) -> &PipelineConfig {
        &self.config
    }

    /// 模型版本组合（缓存键 / 元数据用）。
    pub fn versions(&self) -> String {
        format!(
            "detect:{};coarse:{};parse:{};matting:{};refine:{}",
            self.detector.version_id(),
            self.segmenter.version_id(),
            self.parser
                .as_ref()
                .map(|parser| parser.version_id())
                .unwrap_or_else(|| "-".to_string()),
            self.matting
                .as_ref()
                .map(|matting| matting.version_id())
                .unwrap_or_else(|| "-".to_string()),
            self.refiner
                .as_ref()
                .map(|refiner| refiner.version_id())
                .unwrap_or_else(|| "-".to_string()),
        )
    }

    /// 跑一次双阶段流水线（带缓存）。
    pub fn run(&self, image: &Image) -> Result<Arc<MaskSet>> {
        let key = self.cache_key(image);
        if let Ok(cache) = self.cache.lock() {
            if let Some(hit) = cache.get(&key) {
                return Ok(hit.clone());
            }
        }
        let set = Arc::new(self.run_uncached(image)?);
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() >= 16 {
                cache.clear();
            }
            cache.insert(key, set.clone());
        }
        Ok(set)
    }

    /// 清空缓存。
    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    fn cache_key(&self, image: &Image) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut mix = |byte: u8| {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        };
        // 像素抽样（长边每像素最多 4096 个采样点），避免大图逐字节哈希。
        let (width, height) = (image.width().max(1), image.height().max(1));
        let step = ((width as usize * height as usize) / 4096).max(1);
        let raw = image.as_raw();
        let mut index = 0usize;
        while index < raw.len() {
            mix(raw[index]);
            index += step.saturating_mul(4);
        }
        for byte in self.versions().as_bytes() {
            mix(*byte);
        }
        for byte in format!(
            "{:?}|{}|{}|{}",
            self.config,
            width,
            height,
            crate::CORE_VERSION
        )
        .as_bytes()
        {
            mix(*byte);
        }
        hash
    }

    /// 无缓存推理本体。
    pub fn run_uncached(&self, image: &Image) -> Result<MaskSet> {
        let (width, height) = (image.width().max(1), image.height().max(1));
        if width > MAX_MASK_EDGE || height > MAX_MASK_EDGE {
            return Err(FdxmpError::Unsupported(format!(
                "图像尺寸超过智能蒙版上限 {MAX_MASK_EDGE}：{width}×{height}"
            )));
        }

        // 第一阶段：人脸检测 + 人像粗分割，并行（只负责「看哪里」）。
        let (faces_result, coarse_result) = rayon::join(
            || self.detector.detect(image),
            || self.segmenter.segment_coarse(image),
        );
        let faces = match faces_result {
            Ok(mut faces) => {
                faces.sort_by(|a, b| {
                    b.confidence
                        .partial_cmp(&a.confidence)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                faces.truncate(self.config.max_faces);
                faces
            }
            Err(_) => Vec::new(),
        };
        let coarse = coarse_result.ok();
        if coarse.is_none() && faces.is_empty() {
            return Err(FdxmpError::Unsupported(
                "双阶段推理失败：人脸检测与人像粗分割都不可用".to_string(),
            ));
        }

        // 第二阶段：人脸路径与人像路径并行（只负责「看清楚」）。
        let face_path = || -> Result<Vec<SmartMask>> {
            let Some(parser) = self.parser.as_ref() else {
                return Ok(Vec::new());
            };
            let mut accumulated: Vec<SmartMask> = Vec::new();
            for face in &faces {
                let Some(transform) = face_alignment(face, (width, height), self.config.align_size)
                else {
                    continue;
                };
                let aligned = warp_image(image, &transform, self.config.align_size);
                let parsed = parser.parse(&aligned)?;
                for (kind, region) in parsed.iter() {
                    let projected = project_region(image, kind, region, &transform, &face.bbox);
                    merge_kind(&mut accumulated, projected);
                }
            }
            Ok(accumulated)
        };
        let person_path = || -> Result<Option<SmartMask>> {
            let Some(coarse) = coarse.as_ref() else {
                return Ok(None);
            };
            if self.config.skip_matting_without_face && faces.is_empty() {
                return Ok(Some(coarse.clone()));
            }
            let Some(matting) = self.matting.as_ref() else {
                return Ok(Some(coarse.clone()));
            };
            let Some(bbox) = coarse.bbox(0.5) else {
                return Ok(Some(coarse.clone()));
            };
            let margin = self.config.matting_margin.max(0.0);
            let (mx, my) = (
                ((bbox.2 - bbox.0) as f32 * margin) as u32,
                ((bbox.3 - bbox.1) as f32 * margin) as u32,
            );
            let x0 = bbox.0.saturating_sub(mx);
            let y0 = bbox.1.saturating_sub(my);
            let x1 = (bbox.2 + mx).min(width);
            let y1 = (bbox.3 + my).min(height);
            if x1 <= x0 || y1 <= y0 {
                return Ok(Some(coarse.clone()));
            }
            let crop = image::imageops::crop_imm(image, x0, y0, x1 - x0, y1 - y0).to_image();
            let refined = matting.matting_fine(&crop)?;
            // 精细抠图结果放回整图坐标（裁剪偏移 + 尺寸对齐）。
            let mut full = SmartMask::zeros(SmartMaskKind::Portrait, width, height);
            let resized = crate::smartmask::mask::resize_plane(
                &refined.data,
                refined.width.max(1),
                refined.height.max(1),
                x1 - x0,
                y1 - y0,
            );
            for y in 0..(y1 - y0) {
                for x in 0..(x1 - x0) {
                    full.data[((y0 + y) as usize) * (width as usize) + (x0 + x) as usize] =
                        resized[(y * (x1 - x0) + x) as usize];
                }
            }
            full.confidence = refined.confidence;
            full.source_model = refined.source_model.clone();
            full.model_version = refined.model_version.clone();
            Ok(Some(full))
        };

        let (face_masks, person_mask) = rayon::join(face_path, person_path);
        let mut masks = face_masks?;
        let person_mask = person_mask?;

        // 主体：优先精细抠图，退回粗分割。
        if let Some(person) = person_mask.as_ref() {
            let mut subject = person.clone();
            subject.kind = SmartMaskKind::Subject;
            masks.push(subject);
            let mut portrait = person.clone();
            portrait.kind = SmartMaskKind::Portrait;
            masks.push(portrait);
        }

        // 细化（边缘清理）：只作用于主体 / 人像这类软蒙版。
        if let Some(refiner) = self.refiner.as_ref() {
            let refined: Vec<SmartMask> = masks
                .par_iter()
                .map(|mask| {
                    if matches!(mask.kind, SmartMaskKind::Subject | SmartMaskKind::Portrait) {
                        refiner.refine(image, mask).unwrap_or_else(|_| mask.clone())
                    } else {
                        mask.clone()
                    }
                })
                .collect();
            masks = refined;
        }

        if self.config.feather_px > 0.0 {
            for mask in &mut masks {
                if matches!(mask.kind, SmartMaskKind::Subject | SmartMaskKind::Portrait) {
                    mask.feather(self.config.feather_px);
                }
            }
        }

        // 元数据：统一补上生成时间与来源版本。
        let now = now_unix_ms();
        let versions = self.versions();
        for mask in &mut masks {
            if mask.generated_at == 0 {
                mask.generated_at = now;
            }
            if mask.model_version.is_empty() {
                mask.model_version = versions.clone();
            }
        }
        dedup_kinds(&mut masks);
        Ok(MaskSet {
            masks,
            faces,
            versions,
            generated_at: now,
        })
    }
}

/// 当前 Unix 毫秒（不可用时返回 0）。
pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// 由 5 点关键点计算「原图像素坐标 → 对齐人脸」的相似变换（ArcFace 112 模板）。
///
/// 关键点为归一化坐标（0..1），传入原图尺寸换算到像素坐标。
pub fn face_alignment(
    face: &FaceDetection,
    image_size: (u32, u32),
    align_size: u32,
) -> Option<AffineMap> {
    // ArcFace 112×112 模板（左眼 / 右眼 / 鼻尖 / 左嘴角 / 右嘴角）。
    const TEMPLATE: [[f64; 2]; 5] = [
        [38.2946, 51.6963],
        [73.5318, 51.5014],
        [56.0252, 71.7366],
        [41.5493, 92.3655],
        [70.7299, 92.2041],
    ];
    let scale = align_size as f64 / 112.0;
    let template: Vec<[f64; 2]> = TEMPLATE
        .iter()
        .map(|point| [point[0] * scale, point[1] * scale])
        .collect();
    let source: Vec<[f64; 2]> = face
        .landmarks
        .iter()
        .map(|point| {
            [
                point[0] as f64 * image_size.0.max(1) as f64,
                point[1] as f64 * image_size.1.max(1) as f64,
            ]
        })
        .collect();
    AffineMap::similarity_from_points(&source, &template)
}

/// 按仿射映射把人脸区域从原图重采样到对齐空间。
///
/// `map` 为**正向**变换（原图像素坐标 → 对齐空间像素坐标）；越界采样为透明。
pub fn warp_image(image: &Image, map: &AffineMap, size: u32) -> Image {
    let size = size.max(1);
    let mut out = Image::new(size, size);
    let Some(inverse) = map.invert() else {
        return out;
    };
    for y in 0..size {
        for x in 0..size {
            let (sx, sy) = inverse.apply(x as f64 + 0.5, y as f64 + 0.5);
            let pixel = sample_rgba(image, sx as f32 - 0.5, sy as f32 - 0.5);
            out.put_pixel(x, y, image::Rgba(pixel));
        }
    }
    out
}

/// 双线性采样 RGBA（越界返回全透明）。
pub fn sample_rgba(image: &Image, x: f32, y: f32) -> [u8; 4] {
    let (width, height) = (image.width() as i32, image.height() as i32);
    if x < -1.0 || y < -1.0 || x > width as f32 || y > height as f32 || width <= 0 || height <= 0 {
        return [0, 0, 0, 0];
    }
    let x = x.clamp(0.0, (width - 1) as f32);
    let y = y.clamp(0.0, (height - 1) as f32);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(image.width() - 1);
    let y1 = (y0 + 1).min(image.height() - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let mut out = [0u8; 4];
    for (channel, value) in out.iter_mut().enumerate() {
        let at = |px: u32, py: u32| image.get_pixel(px, py).0[channel] as f32;
        let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
        let bottom = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
        *value = (top * (1.0 - ty) + bottom * ty).round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// 把对齐空间里的区域蒙版投影回原图坐标（限定在人脸框外扩范围内）。
fn project_region(
    image: &Image,
    kind: SmartMaskKind,
    region: &SmartMask,
    map: &AffineMap,
    face_bbox: &[f32; 4],
) -> SmartMask {
    let (width, height) = (image.width().max(1), image.height().max(1));
    let mut out = SmartMask::zeros(kind, width, height);
    out.confidence = region.confidence;
    out.source_model = region.source_model.clone();
    out.model_version = region.model_version.clone();
    // 人脸框外扩 25%（覆盖头发 / 下巴），归一化坐标映射回像素并 clamp。
    let (bw, bh) = (face_bbox[2] - face_bbox[0], face_bbox[3] - face_bbox[1]);
    let x0 = ((face_bbox[0] - 0.25 * bw) * width as f32).clamp(0.0, width as f32);
    let y0 = ((face_bbox[1] - 0.25 * bh) * height as f32).clamp(0.0, height as f32);
    let x1 = ((face_bbox[2] + 0.25 * bw) * width as f32).clamp(0.0, width as f32);
    let y1 = ((face_bbox[3] + 0.25 * bh) * height as f32).clamp(0.0, height as f32);
    let (xi0, yi0) = (x0.floor().max(0.0) as u32, y0.floor().max(0.0) as u32);
    let (xi1, yi1) = (
        (x1.ceil() as u32).min(width),
        (y1.ceil() as u32).min(height),
    );
    for y in yi0..yi1 {
        for x in xi0..xi1 {
            // map 是「原图像素坐标 → 对齐空间」，按原图像素采样对齐空间的区域蒙版。
            let (sx, sy) = map.apply(x as f64 + 0.5, y as f64 + 0.5);
            let weight = crate::smartmask::mask::sample_bilinear(
                &region.data,
                region.width,
                region.height,
                sx as f32 - 0.5,
                sy as f32 - 0.5,
            );
            if weight > 0.001 {
                out.data[(y as usize) * (width as usize) + (x as usize)] = weight.clamp(0.0, 1.0);
            }
        }
    }
    out
}

/// 同名蒙版按并集合并（多张脸时）。
fn merge_kind(accumulated: &mut Vec<SmartMask>, incoming: SmartMask) {
    if let Some(existing) = accumulated
        .iter_mut()
        .find(|mask| mask.kind == incoming.kind)
    {
        for (a, b) in existing.data.iter_mut().zip(incoming.data.iter()) {
            *a = (*a).max(*b);
        }
        existing.confidence = existing.confidence.max(incoming.confidence);
    } else {
        accumulated.push(incoming);
    }
}

/// 去掉同类型的重复蒙版（保留置信度更高的）。
fn dedup_kinds(masks: &mut Vec<SmartMask>) {
    let mut seen: Vec<SmartMaskKind> = Vec::new();
    masks.retain(|mask| {
        if seen.contains(&mask.kind) {
            false
        } else {
            seen.push(mask.kind);
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smartmask::mask::SmartMaskKind;

    fn test_image(width: u32, height: u32) -> Image {
        let mut image = Image::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let value = ((x + y) % 255) as u8;
                image.put_pixel(x, y, image::Rgba([value, 128, 255 - value, 255]));
            }
        }
        image
    }

    #[test]
    fn pipeline_with_face_produces_subject_portrait_and_features() {
        let pipeline = MaskPipeline::mock(true);
        let image = test_image(64, 48);
        let set = pipeline.run(&image).unwrap();
        assert_eq!(set.faces.len(), 1);
        for kind in [
            SmartMaskKind::Subject,
            SmartMaskKind::Portrait,
            SmartMaskKind::Skin,
            SmartMaskKind::Eyes,
            SmartMaskKind::Eyebrows,
            SmartMaskKind::Nose,
            SmartMaskKind::Lips,
            SmartMaskKind::Teeth,
            SmartMaskKind::Hair,
            SmartMaskKind::Face,
        ] {
            let mask = set.get(kind).unwrap_or_else(|| panic!("缺少 {kind:?}"));
            assert_eq!(
                (mask.width, mask.height),
                (64, 48),
                "{kind:?} 坐标应与原图一致"
            );
            assert!(mask.mean() > 0.0, "{kind:?} 不应为空");
            assert!(mask.generated_at > 0);
            assert!(!mask.model_version.is_empty());
        }
        // 五官蒙版应相互错开（眼睛在上、嘴唇在下）。
        let eyes = set.get(SmartMaskKind::Eyes).unwrap().bbox(0.5).unwrap();
        let lips = set.get(SmartMaskKind::Lips).unwrap().bbox(0.5).unwrap();
        assert!(eyes.1 < lips.1, "眼睛应在嘴唇上方");
    }

    #[test]
    fn pipeline_without_face_degrades_to_person_only() {
        let pipeline = MaskPipeline::mock(false);
        let image = test_image(32, 32);
        let set = pipeline.run(&image).unwrap();
        assert!(set.faces.is_empty());
        assert!(set.contains(SmartMaskKind::Portrait));
        assert!(set.contains(SmartMaskKind::Subject));
        assert!(!set.contains(SmartMaskKind::Eyes), "无人脸时不出五官");
    }

    #[test]
    fn pipeline_caches_identical_images() {
        let pipeline = MaskPipeline::mock(true);
        let image = test_image(24, 24);
        let first = pipeline.run(&image).unwrap();
        let second = pipeline.run(&image).unwrap();
        assert!(Arc::ptr_eq(&first, &second), "同图应命中缓存");
        let different = pipeline.run(&test_image(24, 25)).unwrap();
        assert!(!Arc::ptr_eq(&first, &different));
        pipeline.clear_cache();
        let third = pipeline.run(&image).unwrap();
        assert!(!Arc::ptr_eq(&first, &third));
    }

    #[test]
    fn face_alignment_maps_template_to_aligned_space() {
        // 关键点正好取 ArcFace 112 模板 → 变换应精确等于「缩放到 align_size」。
        let template: [[f32; 2]; 5] = [
            [38.2946, 51.6963],
            [73.5318, 51.5014],
            [56.0252, 71.7366],
            [41.5493, 92.3655],
            [70.7299, 92.2041],
        ];
        let detection = FaceDetection {
            bbox: [0.2, 0.2, 0.8, 0.8],
            landmarks: template.map(|point| [point[0] / 112.0, point[1] / 112.0]),
            confidence: 1.0,
        };
        let map = face_alignment(&detection, (112, 112), 256).unwrap();
        for (index, point) in template.iter().enumerate() {
            let (x, y) = map.apply(point[0] as f64, point[1] as f64);
            let expected = (
                point[0] as f64 / 112.0 * 256.0,
                point[1] as f64 / 112.0 * 256.0,
            );
            assert!(
                (x - expected.0).abs() < 1e-3 && (y - expected.1).abs() < 1e-3,
                "第 {index} 点 ({x:.3}, {y:.3}) ≠ ({:.3}, {:.3})",
                expected.0,
                expected.1
            );
        }

        // 形状略有差异的真实人脸（近似等距）应落在模板附近（±8px）。
        let detection = FaceDetection {
            bbox: [0.3, 0.2, 0.7, 0.8],
            landmarks: [
                [0.4, 0.35],
                [0.6, 0.35],
                [0.5, 0.5],
                [0.42, 0.65],
                [0.58, 0.65],
            ],
            confidence: 1.0,
        };
        let map = face_alignment(&detection, (1000, 1000), 256).unwrap();
        let (x, y) = map.apply(400.0, 350.0);
        let expected = (38.2946 / 112.0 * 256.0, 51.6963 / 112.0 * 256.0);
        assert!(
            (x - expected.0).abs() < 8.0,
            "x={x} expected {}",
            expected.0
        );
        assert!(
            (y - expected.1).abs() < 8.0,
            "y={y} expected {}",
            expected.1
        );
    }

    #[test]
    fn warp_image_uses_forward_map() {
        let image = test_image(16, 16);
        let aligned = warp_image(&image, &AffineMap::identity(), 16);
        for y in 0..16 {
            for x in 0..16 {
                assert_eq!(image.get_pixel(x, y), aligned.get_pixel(x, y));
            }
        }
        // 正向平移 +2：输出像素 (2,0) 采样原图 (0,0)；输出 (0,0) 越界为透明。
        let shifted = warp_image(&image, &AffineMap::translation(2.0, 0.0), 16);
        assert_eq!(shifted.get_pixel(2, 0), image.get_pixel(0, 0));
        assert_eq!(shifted.get_pixel(0, 0).0[3], 0, "越界采样应为透明");
    }
}
