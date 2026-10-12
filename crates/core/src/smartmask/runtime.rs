//! 智能蒙版运行时：懒加载 ONNX 模型、按蒙版类型跑双阶段流水线、写入共享栅格缓存。
//!
//! Windows（`ai`，运行时加载 onnxruntime.dll）与 iOS（`ai-static`，静态链接 +
//! CoreML 后端）共用同一份实现；模型缺失 / 运行库不可用时返回
//! [`RuntimeError::Unavailable`]，调用方（如 iOS）可回退平台实现（Vision）。

use std::sync::{Arc, Mutex, OnceLock};

use crate::error::Result;
use crate::masks::{insert_ai_raster, MaskRaster};
use crate::params::{MaskKind, MaskParams, PeoplePart};
use crate::smartmask::mask::SmartMaskKind;
use crate::smartmask::pipeline::MaskPipeline;
use crate::smartmask::spec::ModelRegistry;
use crate::smartmask::traits::Image;

/// 运行时错误分类（决定调用方是否回退平台实现）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    /// 模型 / 运行库不可用（未随包 / 哈希不匹配 / 加载失败）。
    Unavailable(String),
    /// 该蒙版类型没有对应模型（如天空 / 物体提示选择）。
    Unsupported(String),
    /// 推理成功但不能产出该类型（如无人脸 → 五官）。
    NotFound(String),
    /// 其它错误。
    Failed(String),
}

impl RuntimeError {
    /// 错误提示文本。
    pub fn message(&self) -> &str {
        match self {
            RuntimeError::Unavailable(message)
            | RuntimeError::Unsupported(message)
            | RuntimeError::NotFound(message)
            | RuntimeError::Failed(message) => message,
        }
    }

    /// 是否应该回退平台实现（模型不可用 / 类型不支持）。
    pub fn should_fallback(&self) -> bool {
        matches!(
            self,
            RuntimeError::Unavailable(_) | RuntimeError::Unsupported(_)
        )
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

/// 全局流水线（懒加载；模型加载失败也会被缓存，避免反复读盘校验）。
fn pipeline_cell() -> &'static Mutex<Option<Arc<MaskPipeline>>> {
    static CELL: OnceLock<Mutex<Option<Arc<MaskPipeline>>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

/// 模型与运行库是否可用（iOS 打包了模型则返回 true）。
pub fn runtime_available() -> bool {
    pipeline().is_ok()
}

/// 取全局流水线（首次调用加载并校验全部模型）。
pub fn pipeline() -> std::result::Result<Arc<MaskPipeline>, RuntimeError> {
    let cell = pipeline_cell();
    {
        let guard = cell
            .lock()
            .map_err(|_| RuntimeError::Failed("智能蒙版运行时锁失效".to_string()))?;
        if let Some(pipeline) = guard.as_ref() {
            return Ok(pipeline.clone());
        }
    }
    let registry = ModelRegistry::new();
    let stages = crate::smartmask::onnx::OnnxStages::load(&registry)
        .map_err(|err| RuntimeError::Unavailable(err.to_string()))?;
    let pipeline = Arc::new(stages.pipeline());
    let mut guard = cell
        .lock()
        .map_err(|_| RuntimeError::Failed("智能蒙版运行时锁失效".to_string()))?;
    *guard = Some(pipeline.clone());
    Ok(pipeline)
}

/// 清空运行时（测试用；下次调用重新加载模型）。
pub fn reset_runtime() {
    if let Ok(mut guard) = pipeline_cell().lock() {
        *guard = None;
    }
}

/// 蒙版类型 → 流水线输出类型（None = 模型侧不支持，调用方可回退）。
fn output_kind(kind: &MaskKind) -> Option<SmartMaskKind> {
    match kind {
        MaskKind::AiSubject | MaskKind::AiBackground => Some(SmartMaskKind::Subject),
        MaskKind::AiPeople { part } => Some(match part {
            PeoplePart::Face => SmartMaskKind::Face,
            PeoplePart::Body => SmartMaskKind::Portrait,
            PeoplePart::Skin => SmartMaskKind::Skin,
            PeoplePart::Hair => SmartMaskKind::Hair,
            PeoplePart::Clothes => SmartMaskKind::Clothes,
            PeoplePart::Eyes => SmartMaskKind::Eyes,
            PeoplePart::Eyebrows => SmartMaskKind::Eyebrows,
            PeoplePart::Nose => SmartMaskKind::Nose,
            PeoplePart::Lips => SmartMaskKind::Lips,
            PeoplePart::Teeth => SmartMaskKind::Teeth,
            PeoplePart::Ears => SmartMaskKind::Ears,
        }),
        _ => None,
    }
}

/// 跑双阶段流水线并把目标蒙版写入共享栅格缓存。
///
/// - 同一张图上的多次调用会命中 [`MaskPipeline`] 内部缓存（只推理一次算法路径）；
/// - `AiBackground` 自动取主体反向；
/// - 来源模型 / 版本写入栅格元数据（透明度 / 羽化 / 反向仍由渲染侧统一后处理）。
pub fn generate_into_cache(
    mask: &MaskParams,
    image: &Image,
) -> std::result::Result<(), RuntimeError> {
    let Some(kind) = output_kind(&mask.kind) else {
        return Err(RuntimeError::Unsupported(format!(
            "该蒙版类型（{:?}）暂由平台实现提供",
            mask.kind
        )));
    };
    let pipeline = pipeline()?;
    let set = pipeline
        .run(image)
        .map_err(|err| RuntimeError::Failed(err.to_string()))?;
    let Some(produced) = set.get(kind) else {
        return Err(RuntimeError::NotFound(format!(
            "模型未产出该部位（{}）：可能没检测到人脸 / 人物",
            kind.as_str()
        )));
    };
    let mut raster: MaskRaster = produced.to_raster();
    if matches!(mask.kind, MaskKind::AiBackground) {
        for weight in &mut raster.weights {
            *weight = 1.0 - *weight;
        }
    }
    insert_ai_raster(mask, raster).map_err(|err| RuntimeError::Failed(err.to_string()))
}

/// 便捷包装：核心错误类型。
pub fn generate(mask: &MaskParams, image: &Image) -> Result<()> {
    generate_into_cache(mask, image)
        .map_err(|err| crate::error::FdxmpError::Unsupported(err.message().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::MaskParams;

    #[test]
    fn output_kind_maps_all_supported_kinds() {
        let cases = [
            (MaskKind::AiSubject, Some(SmartMaskKind::Subject)),
            (MaskKind::AiBackground, Some(SmartMaskKind::Subject)),
            (
                MaskKind::AiPeople {
                    part: PeoplePart::Eyes,
                },
                Some(SmartMaskKind::Eyes),
            ),
            (
                MaskKind::AiPeople {
                    part: PeoplePart::Ears,
                },
                Some(SmartMaskKind::Ears),
            ),
            (
                MaskKind::AiPeople {
                    part: PeoplePart::Body,
                },
                Some(SmartMaskKind::Portrait),
            ),
            (MaskKind::AiSky, None),
            (MaskKind::AiObject { point: [0.5, 0.5] }, None),
        ];
        for (kind, expected) in cases {
            assert_eq!(output_kind(&kind), expected, "{kind:?}");
        }
    }

    #[test]
    fn unsupported_kind_returns_fallbackable_error() {
        let mask = MaskParams {
            kind: MaskKind::AiSky,
            ..MaskParams::default()
        };
        let image = image::RgbaImage::new(4, 4);
        let error = generate_into_cache(&mask, &image).unwrap_err();
        assert!(error.should_fallback());
    }
}
