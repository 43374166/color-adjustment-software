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

/// 全局运行时（懒加载；模型加载失败也会被缓存，避免反复读盘校验）。
struct Runtime {
    stages: Arc<crate::smartmask::onnx::OnnxStages>,
    pipeline: Arc<MaskPipeline>,
}

fn runtime_cell() -> &'static Mutex<Option<Arc<Runtime>>> {
    static CELL: OnceLock<Mutex<Option<Arc<Runtime>>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

/// 模型与运行库是否可用（iOS 打包了模型则返回 true）。
pub fn runtime_available() -> bool {
    pipeline().is_ok()
}

/// 取全局流水线（首次调用加载并校验全部模型）。
pub fn pipeline() -> std::result::Result<Arc<MaskPipeline>, RuntimeError> {
    runtime().map(|runtime| runtime.pipeline.clone())
}

/// 取全局运行时（含 ONNX 各阶段，供关键点等额外模型复用）。
fn runtime() -> std::result::Result<Arc<Runtime>, RuntimeError> {
    let cell = runtime_cell();
    {
        let guard = cell
            .lock()
            .map_err(|_| RuntimeError::Failed("智能蒙版运行时锁失效".to_string()))?;
        if let Some(runtime) = guard.as_ref() {
            return Ok(runtime.clone());
        }
    }
    let registry = ModelRegistry::new();
    let stages = crate::smartmask::onnx::OnnxStages::load(&registry)
        .map_err(|err| RuntimeError::Unavailable(err.to_string()))?;
    let stages = Arc::new(stages);
    let runtime = Arc::new(Runtime {
        pipeline: Arc::new(stages.pipeline()),
        stages,
    });
    let mut guard = cell
        .lock()
        .map_err(|_| RuntimeError::Failed("智能蒙版运行时锁失效".to_string()))?;
    *guard = Some(runtime.clone());
    Ok(runtime)
}

/// 清空运行时（测试用；下次调用重新加载模型）。
pub fn reset_runtime() {
    if let Ok(mut guard) = runtime_cell().lock() {
        *guard = None;
    }
    if let Ok(mut guard) = retouch_session_cell().lock() {
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

// ---------------------------------------------------------------------------
// AI 修图（v1.5.0）：识别（蒙版 + 478 点关键点）→ 应用
// ---------------------------------------------------------------------------

/// 修图识别结果（UI 展示 + 决定可用功能）。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RetouchAnalysis {
    /// 检出的人脸数。
    pub faces: usize,
    /// 关键点数量（0 = 无 FaceMesh / 未检出）。
    pub landmarks: usize,
    /// 是否拿到皮肤蒙版。
    pub skin: bool,
    /// 是否拿到人像蒙版。
    pub person: bool,
    /// 提示（无脸 / 无人像等）。
    pub warnings: Vec<String>,
}

/// 全局修图会话：最近一次识别结果（蒙版 + 关键点，与图像指纹绑定）。
struct RetouchSession {
    fingerprint: u64,
    size: (u32, u32),
    masks: crate::retouch::RetouchMasks,
    analysis: RetouchAnalysis,
}

fn retouch_session_cell() -> &'static Mutex<Option<RetouchSession>> {
    static CELL: OnceLock<Mutex<Option<RetouchSession>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

/// 平台人像蒙版（iOS Vision 人像分割）：按图像指纹缓存，补上不随包 MODNet 的缺口。
fn platform_person_cell() -> &'static Mutex<Option<(u64, crate::retouch::MaskPlane)>> {
    static CELL: OnceLock<Mutex<Option<(u64, crate::retouch::MaskPlane)>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

/// 提交平台人像蒙版（iOS：Vision 人像分割 α，与图像指纹绑定）。
///
/// `retouch_analyze` / 智能蒙版人像路径会在缺模型时自动取用；换图后自动失效。
pub fn submit_platform_person_mask(
    image: &Image,
    plane_width: u32,
    plane_height: u32,
    weights: Vec<f32>,
) -> std::result::Result<(), RuntimeError> {
    let expected = (plane_width as usize).saturating_mul(plane_height as usize);
    if plane_width == 0
        || plane_height == 0
        || plane_width > 4096
        || plane_height > 4096
        || weights.len() != expected
        || weights.iter().any(|value| !value.is_finite())
    {
        return Err(RuntimeError::Failed(
            "平台人像蒙版尺寸非法或包含非法数值".to_string(),
        ));
    }
    let resized = crate::retouch::MaskPlane {
        width: image.width().max(1),
        height: image.height().max(1),
        data: crate::smartmask::mask::resize_plane(
            &weights,
            plane_width,
            plane_height,
            image.width().max(1),
            image.height().max(1),
        ),
    };
    let mut guard = platform_person_cell()
        .lock()
        .map_err(|_| RuntimeError::Failed("平台人像蒙版锁失效".to_string()))?;
    *guard = Some((fingerprint(image), resized));
    Ok(())
}

/// 平台人像蒙版是否可用（UI 用来提示"身体塑形可用"）。
pub fn platform_person_ready() -> bool {
    platform_person_cell()
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false)
}

/// 图像指纹（抽样；与图片内容绑定）。
fn fingerprint(image: &Image) -> u64 {
    let raw = image.as_raw();
    if raw.is_empty() {
        return 0;
    }
    let step = (raw.len() / 1024).max(1);
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut index = 0usize;
    while index < raw.len() {
        hash ^= raw[index] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        index += step;
    }
    hash
}

/// 识别（一键修图的第一步）：跑智能蒙版流水线 + FaceMesh，缓存结果供 [`retouch_apply`] 使用。
pub fn retouch_analyze(image: &Image) -> std::result::Result<RetouchAnalysis, RuntimeError> {
    let runtime = runtime()?;
    let (width, height) = (image.width().max(1), image.height().max(1));
    let set = runtime
        .pipeline
        .run(image)
        .map_err(|err| RuntimeError::Failed(err.to_string()))?;
    let mut warnings = Vec::new();

    let mut masks = crate::retouch::RetouchMasks::default();
    if let Some(skin) = set.get(SmartMaskKind::Skin) {
        masks.skin = Some(crate::retouch::MaskPlane::from_smart(skin, width, height));
    } else {
        warnings.push("未识别到皮肤（磨皮 / 去瑕疵 / 肤色将作用于整张图）".to_string());
    }
    if let Some(face) = set.get(SmartMaskKind::Face) {
        masks.face = Some(crate::retouch::MaskPlane::from_smart(face, width, height));
    }
    if let Some(person) = set
        .get(SmartMaskKind::Portrait)
        .or_else(|| set.get(SmartMaskKind::Subject))
    {
        masks.person = Some(crate::retouch::MaskPlane::from_smart(person, width, height));
    } else {
        // iOS：MODNet 不随包，人像蒙版由 Vision 提交（按图像指纹取用）。
        let platform = platform_person_cell()
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .filter(|(key, _)| *key == fingerprint(image))
            .map(|(_, plane)| crate::retouch::MaskPlane {
                width: plane.width,
                height: plane.height,
                data: crate::smartmask::mask::resize_plane(
                    &plane.data,
                    plane.width.max(1),
                    plane.height.max(1),
                    width,
                    height,
                ),
            });
        if let Some(person) = platform {
            masks.person = Some(person);
        } else {
            warnings.push("未识别到人像（身体塑形不可用）".to_string());
        }
    }

    // FaceMesh：取最大的人脸对齐后跑 478 点，映射回原图坐标。
    let mut landmark_count = 0usize;
    if let Some(landmarks_model) = runtime.stages.face_landmarks() {
        if let Some(face) = set.faces.first() {
            if let Some(transform) =
                crate::smartmask::pipeline::face_alignment(face, (width, height), 256)
            {
                let crop = crate::smartmask::pipeline::warp_image(image, &transform, 256);
                match landmarks_model.landmarks(&crop) {
                    Ok(points) => {
                        if let Some(inverse) = transform.invert() {
                            masks.landmarks = points
                                .iter()
                                .map(|point| {
                                    let (x, y) = inverse
                                        .apply(point[0] as f64 * 256.0, point[1] as f64 * 256.0);
                                    [
                                        (x as f32 / width as f32).clamp(0.0, 1.0),
                                        (y as f32 / height as f32).clamp(0.0, 1.0),
                                    ]
                                })
                                .collect();
                            landmark_count = masks.landmarks.len();
                        }
                    }
                    Err(err) => warnings.push(format!("五官识别失败：{err}")),
                }
            }
        } else {
            warnings.push("未检测到人脸（脸部塑形不可用）".to_string());
        }
    } else {
        warnings.push("未随包 FaceMesh 模型（脸部塑形不可用）".to_string());
    }

    let analysis = RetouchAnalysis {
        faces: set.faces.len(),
        landmarks: landmark_count,
        skin: masks.skin.is_some(),
        person: masks.person.is_some(),
        warnings,
    };
    let mut guard = retouch_session_cell()
        .lock()
        .map_err(|_| RuntimeError::Failed("修图会话锁失效".to_string()))?;
    *guard = Some(RetouchSession {
        fingerprint: fingerprint(image),
        size: (width, height),
        masks,
        analysis: analysis.clone(),
    });
    Ok(analysis)
}

/// 应用修图（一键修图的第二步 / 滑杆实时预览）：返回修图后的图像。
///
/// 会话缺失或图像变化时会自动先做一次 [`retouch_analyze`]。
pub fn retouch_apply(
    image: &Image,
    params: &crate::retouch::RetouchParams,
) -> std::result::Result<Image, RuntimeError> {
    if params.is_identity() {
        return Ok(image.clone());
    }
    let (width, height) = (image.width().max(1), image.height().max(1));
    let fingerprint = fingerprint(image);
    let cached = {
        let guard = retouch_session_cell()
            .lock()
            .map_err(|_| RuntimeError::Failed("修图会话锁失效".to_string()))?;
        guard
            .as_ref()
            .filter(|session| session.fingerprint == fingerprint && session.size == (width, height))
            .map(|session| session.masks.clone())
    };
    let masks = match cached {
        Some(masks) => masks,
        None => {
            retouch_analyze(image)?;
            let guard = retouch_session_cell()
                .lock()
                .map_err(|_| RuntimeError::Failed("修图会话锁失效".to_string()))?;
            guard
                .as_ref()
                .map(|session| session.masks.clone())
                .ok_or_else(|| RuntimeError::Failed("修图识别结果缺失".to_string()))?
        }
    };
    Ok(crate::retouch::apply(image, &masks, params))
}

/// 最近一次识别结果（无则 `None`）。
pub fn retouch_analysis() -> Option<RetouchAnalysis> {
    retouch_session_cell()
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|session| session.analysis.clone()))
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
