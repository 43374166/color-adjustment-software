//! ONNX Runtime 适配器（`ai` feature）：显式实现双阶段 trait，全部由 [`ModelSpec`] 驱动。
//!
//! - [`ScrfdDetector`]：SCRFD-500M 人脸检测（框 + 5 点关键点），NMS 解码；
//! - [`ModnetSegmenter`]：MODNet 人像粗分割；
//! - [`BiseNetParser`]：BiSeNet 19 类人脸解析（皮肤 / 五官 / 头发）；
//! - [`U2NetMatting`]：U²-Net 精细抠图。
//!
//! 模型路径、输入尺寸、归一化、阈值、输出名全部来自注册表配置；
//! 换模型只需替换 ONNX 文件 + 更新 [`ModelSpec`]，上层零修改。
//! 模型缺失时 [`OnnxStages::load`] 返回明确错误，调用方可退回 Mock / 平台实现。

use std::sync::{Arc, Mutex};

use ort::session::Session;
use ort::value::Tensor;

use crate::error::{FdxmpError, Result};
use crate::smartmask::mask::{SmartMask, SmartMaskKind};
use crate::smartmask::spec::{Activation, ModelRegistry, ModelSpec, ModelTask};
use crate::smartmask::traits::{
    FaceDetection, FaceParseResult, Image, Stage1FaceDetector, Stage1PersonSegmenter,
    Stage2FaceParser, Stage2PersonMatting,
};

/// 一组加载完成的 ONNX 阶段（四个模型）。
pub struct OnnxStages {
    detector: Arc<ScrfdDetector>,
    segmenter: Arc<ModnetSegmenter>,
    parser: Arc<BiseNetParser>,
    matting: Arc<U2NetMatting>,
}

impl OnnxStages {
    /// 按注册表加载并校验（SHA-256）全部模型。
    pub fn load(registry: &ModelRegistry) -> Result<Self> {
        let detector = ScrfdDetector::load(registry)?;
        let segmenter = ModnetSegmenter::load(registry)?;
        let parser = BiseNetParser::load(registry)?;
        let matting = U2NetMatting::load(registry)?;
        Ok(Self {
            detector: Arc::new(detector),
            segmenter: Arc::new(segmenter),
            parser: Arc::new(parser),
            matting: Arc::new(matting),
        })
    }

    /// 第一阶段人脸检测。
    pub fn detector(&self) -> Arc<dyn Stage1FaceDetector> {
        self.detector.clone()
    }

    /// 第一阶段人像粗分割。
    pub fn segmenter(&self) -> Arc<dyn Stage1PersonSegmenter> {
        self.segmenter.clone()
    }

    /// 第二阶段人脸解析。
    pub fn parser(&self) -> Arc<dyn Stage2FaceParser> {
        self.parser.clone()
    }

    /// 第二阶段精细抠图。
    pub fn matting(&self) -> Arc<dyn Stage2PersonMatting> {
        self.matting.clone()
    }

    /// 组装完整双阶段流水线（人脸路径 + 人像路径）。
    pub fn pipeline(&self) -> crate::smartmask::pipeline::MaskPipeline {
        crate::smartmask::pipeline::MaskPipeline::new(
            self.detector(),
            self.segmenter(),
            Some(self.parser()),
            Some(self.matting()),
            None,
            crate::smartmask::pipeline::PipelineConfig::default(),
        )
    }
}

/// 加载会话（校验模型存在与哈希，初始化 ONNX Runtime）。
fn load_session(registry: &ModelRegistry, task: ModelTask) -> Result<(ModelSpec, Session)> {
    let spec = registry
        .spec_for(task)
        .cloned()
        .ok_or_else(|| FdxmpError::Unsupported(format!("注册表缺少任务 {task:?} 的模型")))?;
    let path = registry.verified_path(&spec)?;
    crate::ai::ensure_runtime()?;
    let mut builder = Session::builder()
        .map_err(|err| FdxmpError::Unsupported(format!("ONNX 会话创建失败：{err}")))?;
    let session = builder.commit_from_file(&path).map_err(|err| {
        FdxmpError::Unsupported(format!("加载模型 {} 失败：{err}", path.display()))
    })?;
    Ok((spec, session))
}

/// 一张模型输出（形状 + f32 数据）。
struct Output {
    name: String,
    shape: Vec<usize>,
    data: Vec<f32>,
}

impl Output {
    fn element_count(&self) -> usize {
        self.shape.iter().product()
    }
}

/// 运行一次推理（输入名取会话第一个输入；输出逐个提取为 f32）。
fn run_session(session: &mut Session, shape: Vec<usize>, data: Vec<f32>) -> Result<Vec<Output>> {
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
    let mut result = Vec::new();
    for (index, (name, value)) in outputs.iter().enumerate() {
        let name = if name.is_empty() {
            format!("output_{index}")
        } else {
            name.to_string()
        };
        if let Ok((shape, values)) = value.try_extract_tensor::<f32>() {
            result.push(Output {
                name,
                shape: shape.iter().map(|dim| *dim as usize).collect(),
                data: values.to_vec(),
            });
            continue;
        }
        return Err(FdxmpError::Unsupported(format!(
            "输出 {name} 类型不支持（当前只提取 f32）"
        )));
    }
    Ok(result)
}

/// 预处理：缩放到规格尺寸 → NCHW f32（`(v·scale − mean) / std`）。
fn preprocess_nchw(image: &Image, spec: &ModelSpec) -> (Vec<usize>, Vec<f32>) {
    let (width, height) = (spec.input_size.0.max(1), spec.input_size.1.max(1));
    let resized =
        image::imageops::resize(image, width, height, image::imageops::FilterType::Triangle);
    let pixels = (width as usize) * (height as usize);
    let mut data = vec![0.0f32; pixels * 3];
    for (index, pixel) in resized.pixels().enumerate() {
        for channel in 0..3 {
            let source = if matches!(spec.input_color, crate::smartmask::spec::ColorSpace::Bgr) {
                match channel {
                    0 => pixel.0[2],
                    1 => pixel.0[1],
                    _ => pixel.0[0],
                }
            } else {
                pixel.0[channel]
            };
            data[channel * pixels + index] = spec.normalize(source as f32, channel);
        }
    }
    (vec![1, 3, height as usize, width as usize], data)
}

/// 单通道平面 → 蒙版（resize 到目标尺寸，clamp 到 0..1）。
fn plane_to_mask(
    kind: SmartMaskKind,
    values: &[f32],
    plane_width: u32,
    plane_height: u32,
    mask_width: u32,
    mask_height: u32,
    activation: Activation,
) -> Result<SmartMask> {
    let plane: Vec<f32> = values
        .iter()
        .map(|value| match activation {
            Activation::Sigmoid => 1.0 / (1.0 + (-value).exp()),
            Activation::Softmax | Activation::None => *value,
        })
        .collect();
    let resized = crate::smartmask::mask::resize_plane(
        &plane,
        plane_width,
        plane_height,
        mask_width.max(1),
        mask_height.max(1),
    );
    SmartMask::new(kind, mask_width.max(1), mask_height.max(1), resized)
}

/// 一条解码后的检测：置信度 + 归一化框 + 5 点关键点。
type DecodedDetection = (f32, [f32; 4], [[f32; 2]; 5]);

/// SCRFD-500M 人脸检测器。
pub struct ScrfdDetector {
    spec: ModelSpec,
    session: Mutex<Session>,
}

impl ScrfdDetector {
    /// 加载模型。
    pub fn load(registry: &ModelRegistry) -> Result<Self> {
        let (spec, session) = load_session(registry, ModelTask::FaceDetect)?;
        Ok(Self {
            spec,
            session: Mutex::new(session),
        })
    }

    /// 解码一个 stride 的检测输出。
    fn decode_stride(
        &self,
        outputs: &[Output],
        stride: usize,
        image_size: (u32, u32),
        detections: &mut Vec<DecodedDetection>,
    ) {
        let input_w = self.spec.input_size.0 as usize;
        let input_h = self.spec.input_size.1 as usize;
        let grid_h = input_h / stride;
        let grid_w = input_w / stride;
        let expected = grid_h * grid_w;
        // 按名称或缺省顺序取该 stride 的 score / bbox / kps。
        let suffix = stride.to_string();
        let find = |keyword: &str| -> Option<&Output> {
            outputs
                .iter()
                .find(|output| output.name.contains(keyword) && output.name.contains(&suffix))
        };
        let score = find("score");
        let bbox = find("bbox");
        let kps = find("kps");
        let (score, bbox, kps) = match (score, bbox, kps) {
            (Some(score), Some(bbox), Some(kps)) => (score, bbox, kps),
            _ => return,
        };
        let anchors = (score.element_count() / expected).max(1);
        if bbox.element_count() < expected * anchors * 4
            || kps.element_count() < expected * anchors * 10
        {
            return;
        }
        let threshold = self.spec.postprocess.score_threshold;
        let (scale_x, scale_y) = (
            image_size.0 as f32 / input_w as f32,
            image_size.1 as f32 / input_h as f32,
        );
        for index in 0..(expected * anchors) {
            let raw_score = score.data[index];
            let score_value = if (0.0..=1.0).contains(&raw_score) {
                raw_score
            } else {
                1.0 / (1.0 + (-raw_score).exp())
            };
            if score_value < threshold {
                continue;
            }
            let anchor = index / anchors;
            let (col, row) = (anchor % grid_w, anchor / grid_w);
            let cx = (col * stride) as f32;
            let cy = (row * stride) as f32;
            let distance = |offset: usize, factor: f32| bbox.data[index * 4 + offset] * factor;
            let x1 = (cx - distance(0, stride as f32)) * scale_x / image_size.0 as f32;
            let y1 = (cy - distance(1, stride as f32)) * scale_y / image_size.1 as f32;
            let x2 = (cx + distance(2, stride as f32)) * scale_x / image_size.0 as f32;
            let y2 = (cy + distance(3, stride as f32)) * scale_y / image_size.1 as f32;
            let mut landmarks = [[0.0f32; 2]; 5];
            for (point, landmark) in landmarks.iter_mut().enumerate() {
                let kx = cx + kps.data[index * 10 + point * 2] * stride as f32;
                let ky = cy + kps.data[index * 10 + point * 2 + 1] * stride as f32;
                *landmark = [
                    kx * scale_x / image_size.0 as f32,
                    ky * scale_y / image_size.1 as f32,
                ];
            }
            detections.push((
                score_value,
                [
                    x1.clamp(0.0, 1.0),
                    y1.clamp(0.0, 1.0),
                    x2.clamp(0.0, 1.0),
                    y2.clamp(0.0, 1.0),
                ],
                landmarks,
            ));
        }
    }
}

impl Stage1FaceDetector for ScrfdDetector {
    fn detect(&self, image: &Image) -> Result<Vec<FaceDetection>> {
        let (shape, data) = preprocess_nchw(image, &self.spec);
        let outputs = {
            let mut session = self
                .session
                .lock()
                .map_err(|_| FdxmpError::Unsupported("ONNX 会话锁失效".to_string()))?;
            run_session(&mut session, shape, data)?
        };
        let size = (image.width().max(1), image.height().max(1));
        let mut detections: Vec<DecodedDetection> = Vec::new();
        for stride in [8usize, 16, 32] {
            self.decode_stride(&outputs, stride, size, &mut detections);
        }
        // 按分数排序 + NMS（IoU 阈值来自规格）。
        detections.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let iou_threshold = self.spec.postprocess.nms_iou;
        let mut kept: Vec<DecodedDetection> = Vec::new();
        for candidate in detections {
            let keep = kept
                .iter()
                .all(|existing| iou(&candidate.1, &existing.1) < iou_threshold);
            if keep {
                kept.push(candidate);
            }
        }
        Ok(kept
            .into_iter()
            .map(|(confidence, bbox, landmarks)| FaceDetection {
                bbox,
                landmarks,
                confidence,
            })
            .collect())
    }

    fn version_id(&self) -> String {
        format!("scrfd/{}", self.spec.version)
    }
}

/// 交并比（归一化框）。
fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let (x0, y0) = (a[0].max(b[0]), a[1].max(b[1]));
    let (x1, y1) = (a[2].min(b[2]), a[3].min(b[3]));
    let intersection = (x1 - x0).max(0.0) * (y1 - y0).max(0.0);
    let area_a = (a[2] - a[0]).max(0.0) * (a[3] - a[1]).max(0.0);
    let area_b = (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    let union = area_a + area_b - intersection;
    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}

/// MODNet 人像粗分割。
pub struct ModnetSegmenter {
    spec: ModelSpec,
    session: Mutex<Session>,
}

impl ModnetSegmenter {
    /// 加载模型。
    pub fn load(registry: &ModelRegistry) -> Result<Self> {
        let (spec, session) = load_session(registry, ModelTask::PersonCoarse)?;
        Ok(Self {
            spec,
            session: Mutex::new(session),
        })
    }
}

impl Stage1PersonSegmenter for ModnetSegmenter {
    fn segment_coarse(&self, image: &Image) -> Result<SmartMask> {
        let (shape, data) = preprocess_nchw(image, &self.spec);
        let outputs = {
            let mut session = self
                .session
                .lock()
                .map_err(|_| FdxmpError::Unsupported("ONNX 会话锁失效".to_string()))?;
            run_session(&mut session, shape, data)?
        };
        let primary = outputs
            .iter()
            .find(|output| output.name.contains("output"))
            .or_else(|| outputs.first())
            .ok_or_else(|| FdxmpError::Unsupported("MODNet 没有输出".to_string()))?;
        let dims = &primary.shape;
        let (plane_w, plane_h) = match dims.len() {
            4 => (dims[3] as u32, dims[2] as u32),
            3 => (dims[2] as u32, 1),
            _ => {
                return Err(FdxmpError::Unsupported(format!(
                    "MODNet 输出形状不支持：{dims:?}"
                )))
            }
        };
        let mut mask = plane_to_mask(
            SmartMaskKind::Portrait,
            &primary.data,
            plane_w,
            plane_h,
            image.width().max(1),
            image.height().max(1),
            self.spec.postprocess.activation,
        )?;
        mask.source_model = self.spec.name.clone();
        mask.model_version = self.spec.version.clone();
        mask.confidence = 0.9;
        Ok(mask)
    }

    fn version_id(&self) -> String {
        format!("modnet/{}", self.spec.version)
    }
}

/// BiSeNet 19 类人脸解析（CelebAMask-HQ 类别表）。
pub struct BiseNetParser {
    spec: ModelSpec,
    session: Mutex<Session>,
}

impl BiseNetParser {
    /// 加载模型。
    pub fn load(registry: &ModelRegistry) -> Result<Self> {
        let (spec, session) = load_session(registry, ModelTask::FaceParse)?;
        Ok(Self {
            spec,
            session: Mutex::new(session),
        })
    }
}

/// 19 类标签 → 标准蒙版类型（None = 不属于该类型）。
fn face_label_kind(label: usize) -> Option<SmartMaskKind> {
    match label {
        1 => Some(SmartMaskKind::Skin),
        2 => Some(SmartMaskKind::Nose),
        4 | 5 => Some(SmartMaskKind::Eyes),
        6 | 7 => Some(SmartMaskKind::Eyebrows),
        8 | 9 | 15 => Some(SmartMaskKind::Ears),
        10 => Some(SmartMaskKind::Teeth),
        11 | 12 => Some(SmartMaskKind::Lips),
        13 => Some(SmartMaskKind::Hair),
        _ => None,
    }
}

impl Stage2FaceParser for BiseNetParser {
    fn parse(&self, aligned_face: &Image) -> Result<FaceParseResult> {
        let (shape, data) = preprocess_nchw(aligned_face, &self.spec);
        let outputs = {
            let mut session = self
                .session
                .lock()
                .map_err(|_| FdxmpError::Unsupported("ONNX 会话锁失效".to_string()))?;
            run_session(&mut session, shape, data)?
        };
        let primary = outputs
            .iter()
            .find(|output| output.shape.len() == 4)
            .or_else(|| outputs.first())
            .ok_or_else(|| FdxmpError::Unsupported("BiSeNet 没有输出".to_string()))?;
        let dims = &primary.shape;
        let (classes, plane_h, plane_w) = match dims.as_slice() {
            [1, classes, height, width] => (*classes, *height as u32, *width as u32),
            _ => {
                return Err(FdxmpError::Unsupported(format!(
                    "BiSeNet 输出形状不支持：{dims:?}"
                )))
            }
        };
        let pixels = (plane_h as usize) * (plane_w as usize);
        if primary.data.len() < pixels * classes {
            return Err(FdxmpError::Unsupported("BiSeNet 输出数据不足".to_string()));
        }
        // argmax → 类别图；按类别累积到对应标准蒙版。
        let mut labels = vec![0usize; pixels];
        for (index, label) in labels.iter_mut().enumerate() {
            let mut best = 0usize;
            let mut best_value = f32::MIN;
            for class in 0..classes {
                let value = primary.data[class * pixels + index];
                if value > best_value {
                    best_value = value;
                    best = class;
                }
            }
            *label = best;
        }
        let mask_width = aligned_face.width().max(1);
        let mask_height = aligned_face.height().max(1);
        let mut result = FaceParseResult::empty();
        let kinds = [
            SmartMaskKind::Skin,
            SmartMaskKind::Nose,
            SmartMaskKind::Eyes,
            SmartMaskKind::Eyebrows,
            SmartMaskKind::Ears,
            SmartMaskKind::Teeth,
            SmartMaskKind::Lips,
            SmartMaskKind::Hair,
        ];
        for kind in kinds {
            let mut plane = vec![0.0f32; pixels];
            for (index, label) in labels.iter().enumerate() {
                if face_label_kind(*label) == Some(kind) {
                    plane[index] = 1.0;
                }
            }
            // Face = 皮肤 + 五官（不含头发 / 衣服 / 背景）。
            if kind == SmartMaskKind::Skin {
                for (index, label) in labels.iter().enumerate() {
                    if matches!(label, 1 | 2 | 4 | 5 | 6 | 7 | 10 | 11 | 12) {
                        plane[index] = 1.0;
                    }
                }
            }
            let mut mask = plane_to_mask(
                kind,
                &plane,
                plane_w,
                plane_h,
                mask_width,
                mask_height,
                Activation::None,
            )?;
            mask.source_model = self.spec.name.clone();
            mask.model_version = self.spec.version.clone();
            mask.confidence = 0.85;
            match kind {
                SmartMaskKind::Skin => {
                    // 单独再给「面部」：由皮肤平面派生（五官已在其中）。
                    let mut face = mask.clone();
                    face.kind = SmartMaskKind::Face;
                    result.face = Some(face);
                    result.skin = Some(mask);
                }
                SmartMaskKind::Nose => result.nose = Some(mask),
                SmartMaskKind::Eyes => result.eyes = Some(mask),
                SmartMaskKind::Eyebrows => result.eyebrows = Some(mask),
                SmartMaskKind::Teeth => result.teeth = Some(mask),
                SmartMaskKind::Lips => result.lips = Some(mask),
                SmartMaskKind::Hair => result.hair = Some(mask),
                SmartMaskKind::Ears => result.extra.push((SmartMaskKind::Ears, mask)),
                _ => {}
            }
        }
        Ok(result)
    }

    fn version_id(&self) -> String {
        format!("bisenet/{}", self.spec.version)
    }
}

/// U²-Net 精细抠图。
pub struct U2NetMatting {
    spec: ModelSpec,
    session: Mutex<Session>,
}

impl U2NetMatting {
    /// 加载模型。
    pub fn load(registry: &ModelRegistry) -> Result<Self> {
        let (spec, session) = load_session(registry, ModelTask::PersonMatting)?;
        Ok(Self {
            spec,
            session: Mutex::new(session),
        })
    }
}

impl Stage2PersonMatting for U2NetMatting {
    fn matting_fine(&self, crop: &Image) -> Result<SmartMask> {
        let (shape, data) = preprocess_nchw(crop, &self.spec);
        let outputs = {
            let mut session = self
                .session
                .lock()
                .map_err(|_| FdxmpError::Unsupported("ONNX 会话锁失效".to_string()))?;
            run_session(&mut session, shape, data)?
        };
        // U²-Net 有多个侧输出（1959..1965）：最后一个是融合结果（d0），优先取它。
        let primary = outputs
            .iter()
            .rev()
            .find(|output| output.shape.len() >= 3 && output.shape[1] == 1)
            .or_else(|| outputs.last())
            .ok_or_else(|| FdxmpError::Unsupported("U²-Net 没有输出".to_string()))?;
        let dims = &primary.shape;
        let (plane_w, plane_h) = match dims.len() {
            4 => (dims[3] as u32, dims[2] as u32),
            3 => (dims[2] as u32, 1),
            _ => {
                return Err(FdxmpError::Unsupported(format!(
                    "U²-Net 输出形状不支持：{dims:?}"
                )))
            }
        };
        let mut mask = plane_to_mask(
            SmartMaskKind::Portrait,
            &primary.data,
            plane_w,
            plane_h,
            crop.width().max(1),
            crop.height().max(1),
            self.spec.postprocess.activation,
        )?;
        mask.source_model = self.spec.name.clone();
        mask.model_version = self.spec.version.clone();
        mask.confidence = 0.9;
        Ok(mask)
    }

    fn version_id(&self) -> String {
        format!("u2net/{}", self.spec.version)
    }
}
