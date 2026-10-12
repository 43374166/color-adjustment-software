//! Mock 模型：不依赖任何模型文件与运行时，用来跑通双阶段调度、坐标映射与后处理。
//!
//! 结果**确定性**（同输入同输出），适合单元 / 集成测试与 UI 联调；
//! 真实模型（ONNX / Vision / CoreML）以同一组 trait 接入，上层零修改。

use crate::error::Result;
use crate::smartmask::mask::{SmartMask, SmartMaskKind};
use crate::smartmask::traits::{
    FaceDetection, FaceParseResult, Image, MaskRefiner, Stage1FaceDetector, Stage1PersonSegmenter,
    Stage2FaceParser, Stage2PersonMatting,
};

/// Mock 人脸检测：在画面中央偏上生成一张脸（框 + 5 点关键点）。
#[derive(Debug, Clone, Copy, Default)]
pub struct MockFaceDetector {
    /// 是否检测到人脸（用于覆盖「无人脸」分支）。
    pub face_present: bool,
}

impl Stage1FaceDetector for MockFaceDetector {
    fn detect(&self, _image: &Image) -> Result<Vec<FaceDetection>> {
        if !self.face_present {
            return Ok(Vec::new());
        }
        Ok(vec![FaceDetection {
            bbox: [0.3, 0.15, 0.7, 0.65],
            landmarks: [
                [0.4, 0.32],
                [0.6, 0.32],
                [0.5, 0.42],
                [0.42, 0.55],
                [0.58, 0.55],
            ],
            confidence: 0.99,
        }])
    }
}

/// Mock 人像粗分割：中央竖向椭圆。
#[derive(Debug, Clone, Copy, Default)]
pub struct MockPersonSegmenter;

impl MockPersonSegmenter {
    fn ellipse(width: u32, height: u32) -> SmartMask {
        let mut mask = SmartMask::zeros(SmartMaskKind::Portrait, width, height);
        let cx = width as f32 * 0.5;
        let cy = height as f32 * 0.5;
        let rx = width as f32 * 0.28;
        let ry = height as f32 * 0.4;
        for y in 0..height {
            for x in 0..width {
                let dx = (x as f32 + 0.5 - cx) / rx.max(1.0);
                let dy = (y as f32 + 0.5 - cy) / ry.max(1.0);
                if dx * dx + dy * dy <= 1.0 {
                    mask.data[(y * width + x) as usize] = 1.0;
                }
            }
        }
        mask.feather(2.0);
        mask
    }
}

impl Stage1PersonSegmenter for MockPersonSegmenter {
    fn segment_coarse(&self, image: &Image) -> Result<SmartMask> {
        Ok(Self::ellipse(image.width().max(1), image.height().max(1))
            .with_source("mock-segmenter", "1.0.0"))
    }
}

/// Mock 人脸解析：把对齐人脸图按几何切成皮肤 / 眼睛 / 眉毛 / 鼻子 / 嘴唇 / 牙齿。
#[derive(Debug, Clone, Copy, Default)]
pub struct MockFaceParser;

impl MockFaceParser {
    fn region(
        width: u32,
        height: u32,
        kind: SmartMaskKind,
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
    ) -> SmartMask {
        let mut mask = SmartMask::zeros(kind, width, height);
        for y in 0..height {
            for x in 0..width {
                let dx = (x as f32 + 0.5 - cx * width as f32) / (rx * width as f32).max(1.0);
                let dy = (y as f32 + 0.5 - cy * height as f32) / (ry * height as f32).max(1.0);
                if dx * dx + dy * dy <= 1.0 {
                    mask.data[(y * width + x) as usize] = 1.0;
                }
            }
        }
        mask.feather(1.0);
        mask
    }
}

impl Stage2FaceParser for MockFaceParser {
    fn parse(&self, aligned_face: &Image) -> Result<FaceParseResult> {
        let (width, height) = (aligned_face.width().max(1), aligned_face.height().max(1));
        Ok(FaceParseResult {
            skin: Some(Self::region(
                width,
                height,
                SmartMaskKind::Skin,
                0.5,
                0.55,
                0.34,
                0.44,
            )),
            hair: Some(Self::region(
                width,
                height,
                SmartMaskKind::Hair,
                0.5,
                0.16,
                0.36,
                0.20,
            )),
            face: Some(Self::region(
                width,
                height,
                SmartMaskKind::Face,
                0.5,
                0.48,
                0.36,
                0.45,
            )),
            eyes: Some(Self::region(
                width,
                height,
                SmartMaskKind::Eyes,
                0.5,
                0.38,
                0.26,
                0.05,
            )),
            eyebrows: Some(Self::region(
                width,
                height,
                SmartMaskKind::Eyebrows,
                0.5,
                0.31,
                0.28,
                0.045,
            )),
            nose: Some(Self::region(
                width,
                height,
                SmartMaskKind::Nose,
                0.5,
                0.52,
                0.08,
                0.12,
            )),
            lips: Some(Self::region(
                width,
                height,
                SmartMaskKind::Lips,
                0.5,
                0.65,
                0.16,
                0.07,
            )),
            teeth: Some(Self::region(
                width,
                height,
                SmartMaskKind::Teeth,
                0.5,
                0.65,
                0.10,
                0.03,
            )),
            extra: Vec::new(),
        })
    }
}

/// Mock 精细抠图：与粗分割一致的椭圆（测试里用于验证路径被调用）。
#[derive(Debug, Clone, Copy, Default)]
pub struct MockPersonMatting;

impl Stage2PersonMatting for MockPersonMatting {
    fn matting_fine(&self, crop: &Image) -> Result<SmartMask> {
        Ok(
            MockPersonSegmenter::ellipse(crop.width().max(1), crop.height().max(1))
                .with_source("mock-matting", "1.0.0"),
        )
    }
}

/// Mock 细化：按 1 像素做一次开运算（去毛刺），保证接口被真实调用。
#[derive(Debug, Clone, Copy, Default)]
pub struct MockRefiner;

impl MaskRefiner for MockRefiner {
    fn refine(&self, _image: &Image, mask: &SmartMask) -> Result<SmartMask> {
        let mut refined = mask.clone();
        refined.morphology(crate::smartmask::mask::MaskMorphology::Open, 1.0);
        Ok(refined)
    }
}
