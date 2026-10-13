//! 双阶段蒙版接口：全部用 trait 定义，模型与平台实现可插拔。
//!
//! - 第一阶段只负责「看哪里」：[`Stage1FaceDetector`] / [`Stage1PersonSegmenter`]
//! - 第二阶段只负责「看清楚」：[`Stage2FaceParser`] / [`Stage2PersonMatting`]
//! - 后处理：[`MaskRefiner`]；消费方：[`MaskConsumer`] / [`RetouchEngine`]
//! - 训练数据：[`crate::smartmask::retouch::DataRecorder`]

use crate::error::Result;
use crate::smartmask::mask::{PolygonMask, SmartMask, SmartMaskKind};
use crate::smartmask::spec::ModelSpec;

/// 图像类型（RGBA8，坐标原点左上角）。
pub type Image = image::RgbaImage;

/// 一张人脸的检测结果（归一化坐标 0..1，原点左上角）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceDetection {
    /// 人脸框 `[x0, y0, x1, y1]`（归一化）。
    pub bbox: [f32; 4],
    /// 5 点关键点（左眼、右眼、鼻尖、左嘴角、右嘴角；归一化）。
    pub landmarks: [[f32; 2]; 5],
    /// 置信度。
    pub confidence: f32,
}

/// 人脸解析结果：一张脸区域里的各部位蒙版（对齐空间，坐标 >= 0）。
#[derive(Debug, Clone, PartialEq)]
pub struct FaceParseResult {
    /// 皮肤。
    pub skin: Option<SmartMask>,
    /// 头发。
    pub hair: Option<SmartMask>,
    /// 面部整体。
    pub face: Option<SmartMask>,
    /// 眼睛。
    pub eyes: Option<SmartMask>,
    /// 眉毛。
    pub eyebrows: Option<SmartMask>,
    /// 鼻子。
    pub nose: Option<SmartMask>,
    /// 嘴唇。
    pub lips: Option<SmartMask>,
    /// 牙齿（口腔内部近似）。
    pub teeth: Option<SmartMask>,
    /// 其它部位（新增类型不破坏旧代码）。
    pub extra: Vec<(SmartMaskKind, SmartMask)>,
}

impl FaceParseResult {
    /// 空结果。
    pub fn empty() -> Self {
        Self {
            skin: None,
            hair: None,
            face: None,
            eyes: None,
            eyebrows: None,
            nose: None,
            lips: None,
            teeth: None,
            extra: Vec::new(),
        }
    }

    /// 按类型取蒙版。
    pub fn get(&self, kind: SmartMaskKind) -> Option<&SmartMask> {
        match kind {
            SmartMaskKind::Skin => self.skin.as_ref(),
            SmartMaskKind::Hair => self.hair.as_ref(),
            SmartMaskKind::Face => self.face.as_ref(),
            SmartMaskKind::Eyes => self.eyes.as_ref(),
            SmartMaskKind::Eyebrows => self.eyebrows.as_ref(),
            SmartMaskKind::Nose => self.nose.as_ref(),
            SmartMaskKind::Lips => self.lips.as_ref(),
            SmartMaskKind::Teeth => self.teeth.as_ref(),
            other => self
                .extra
                .iter()
                .find(|(kind, _)| *kind == other)
                .map(|(_, mask)| mask),
        }
    }

    /// 全部部位。
    pub fn iter(&self) -> impl Iterator<Item = (SmartMaskKind, &SmartMask)> {
        let fixed = [
            (SmartMaskKind::Skin, self.skin.as_ref()),
            (SmartMaskKind::Hair, self.hair.as_ref()),
            (SmartMaskKind::Face, self.face.as_ref()),
            (SmartMaskKind::Eyes, self.eyes.as_ref()),
            (SmartMaskKind::Eyebrows, self.eyebrows.as_ref()),
            (SmartMaskKind::Nose, self.nose.as_ref()),
            (SmartMaskKind::Lips, self.lips.as_ref()),
            (SmartMaskKind::Teeth, self.teeth.as_ref()),
        ];
        fixed
            .into_iter()
            .filter_map(|(kind, mask)| mask.map(|mask| (kind, mask)))
            .chain(self.extra.iter().map(|(kind, mask)| (*kind, mask)))
    }
}

/// 模型输出：检测框或蒙版。
#[derive(Debug, Clone, PartialEq)]
pub enum MaskOrDetection {
    /// 权重蒙版。
    Mask(SmartMask),
    /// 检测结果。
    Detections(Vec<FaceDetection>),
}

/// 平面张量（预处理输出 / 后处理输入；与推理后端无关的中间表示）。
#[derive(Debug, Clone, PartialEq)]
pub struct PlaneTensor {
    /// 形状（NCHW / NHWC 由 `ModelSpec` 决定）。
    pub shape: Vec<usize>,
    /// 连续数据。
    pub data: Vec<f32>,
}

impl PlaneTensor {
    /// 新建。
    pub fn new(shape: Vec<usize>, data: Vec<f32>) -> Self {
        Self { shape, data }
    }

    /// 元素个数。
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// 预处理抽象：resize / padding / 颜色转换 / 归一化全部由 `ModelSpec` 驱动。
pub trait Preprocess {
    /// 图像 + 规格 → 模型输入张量。
    fn run(&self, image: &Image, spec: &ModelSpec) -> Result<PlaneTensor>;
}

/// 后处理抽象：sigmoid / softmax / argmax / NMS / resize / 阈值 / 形态学全部可配。
pub trait Postprocess {
    /// 模型输出 + 规格 + 原图尺寸 → 蒙版或检测框。
    fn run(
        &self,
        outputs: Vec<PlaneTensor>,
        spec: &ModelSpec,
        orig_size: (u32, u32),
    ) -> Result<MaskOrDetection>;
}

/// 第一阶段：快速、粗略定位人脸（框 + 5 点关键点）。
pub trait Stage1FaceDetector: Send + Sync {
    /// 检测人脸（无人脸返回空列表，不算错误）。
    fn detect(&self, image: &Image) -> Result<Vec<FaceDetection>>;

    /// 模型版本（参与缓存键；换模型时递增）。
    fn version_id(&self) -> String {
        "0.0.0".to_string()
    }
}

/// 第一阶段：人像粗分割（Alpha）。
pub trait Stage1PersonSegmenter: Send + Sync {
    /// 粗略人像 Alpha（与原图同尺寸，0..1）。
    fn segment_coarse(&self, image: &Image) -> Result<SmartMask>;

    /// 模型版本（参与缓存键）。
    fn version_id(&self) -> String {
        "0.0.0".to_string()
    }
}

/// 第二阶段：人脸对齐后的像素级解析（皮肤 / 五官 / 头发等）。
pub trait Stage2FaceParser: Send + Sync {
    /// 解析对齐后的人脸图。
    fn parse(&self, aligned_face: &Image) -> Result<FaceParseResult>;

    /// 模型版本（参与缓存键）。
    fn version_id(&self) -> String {
        "0.0.0".to_string()
    }
}

/// 第二阶段：人像精细抠图（Alpha）。
pub trait Stage2PersonMatting: Send + Sync {
    /// 精细抠图（输入为裁剪区域，输出与裁剪同尺寸）。
    fn matting_fine(&self, crop: &Image) -> Result<SmartMask>;

    /// 模型版本（参与缓存键）。
    fn version_id(&self) -> String {
        "0.0.0".to_string()
    }
}

/// 第二阶段：人脸 478 点关键点（FaceMesh；修图塑形用）。
pub trait Stage2FaceLandmarks: Send + Sync {
    /// 输入对齐后的人脸图，输出归一化（0..1，相对输入图）的 2D 关键点。
    ///
    /// MediaPipe FaceMesh v2 = 478 点（前 468 点为面部网格，末尾 10 点为双眼虹膜）。
    fn landmarks(&self, aligned_face: &Image) -> Result<Vec<[f32; 2]>>;

    /// 人脸置信度（0..1；用于 UI 判断「是否检测到人脸」）。
    fn confidence(&self, aligned_face: &Image) -> Result<f32> {
        self.landmarks(aligned_face).map(|_| 1.0)
    }

    /// 模型版本（参与缓存键）。
    fn version_id(&self) -> String {
        "0.0.0".to_string()
    }
}

/// 蒙版细化（边缘清理 / 引导滤波 / 形态学）。
pub trait MaskRefiner: Send + Sync {
    /// 细化（参考图为原图）。
    fn refine(&self, image: &Image, mask: &SmartMask) -> Result<SmartMask>;

    /// 版本（参与缓存键）。
    fn version_id(&self) -> String {
        "0.0.0".to_string()
    }
}

// ---------------------------------------------------------------------------
// AI 修图阶段（v1.6.0）：修复 / 恢复 / 超分 —— 均为可选模型，缺失时调用方走经典算法
// ---------------------------------------------------------------------------

/// AI 修复（LaMa）：对 `mask` 权重 > 0 的区域重绘，返回与输入同尺寸的结果。
pub trait StageInpaint: Send + Sync {
    /// `image` 与 `mask` 同尺寸；`mask` 为 0..1 软权重（已羽化 / 膨胀）。
    fn inpaint(&self, image: &Image, mask: &crate::retouch::MaskPlane) -> Result<Image>;

    /// 模型版本（参与缓存键）。
    fn version_id(&self) -> String {
        "0.0.0".to_string()
    }
}

/// AI 人脸恢复（GFPGAN）：输入对齐后人脸（任意尺寸，内部缩放到模型输入），
/// 返回与输入同尺寸的恢复结果。
pub trait StageFaceRestore: Send + Sync {
    /// 恢复一张对齐人脸。
    fn restore(&self, aligned_face: &Image) -> Result<Image>;

    /// 模型版本（参与缓存键）。
    fn version_id(&self) -> String {
        "0.0.0".to_string()
    }
}

/// AI 超分（Real-ESRGAN）：4 倍放大（内部按块推理，支持任意尺寸）。
pub trait StageSuperRes: Send + Sync {
    /// 放大 4 倍。
    fn upscale(&self, image: &Image) -> Result<Image>;

    /// 按倍率放大（1..4；默认实现退回 4 倍放大，供不支持逐块缩放的后端使用）。
    fn upscale_scaled(&self, image: &Image, scale: f32) -> Result<Image> {
        let _ = scale;
        self.upscale(image)
    }

    /// 模型版本（参与缓存键）。
    fn version_id(&self) -> String {
        "0.0.0".to_string()
    }
}

/// 蒙版消费方：修图流程之外的消费接口（导出、统计、预览等）。
pub trait MaskConsumer {
    /// 接收一张图的全部蒙版。
    fn on_masks(&mut self, image: &Image, masks: &[SmartMask]) -> Result<()>;
}

/// AI 修图引擎：接收「原图 + 标准蒙版 + 参数」，独立于蒙版模块替换。
pub trait RetouchEngine: Send + Sync {
    /// 应用修图（返回新图；不得修改入参）。
    fn apply(
        &self,
        image: &Image,
        mask: &SmartMask,
        params: &crate::smartmask::retouch::RetouchParams,
    ) -> Result<Image>;
}

/// 多边形提供者：平台（如 iOS Vision 五官关键点）把多边形交给核心光栅化。
pub trait PolygonProvider: Send + Sync {
    /// 取指定类型的多边形（正常化坐标，原点左上角）。
    fn polygons(&self, kind: SmartMaskKind) -> Result<PolygonMask>;
}
