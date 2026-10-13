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
    if let Ok(mut guard) = platform_pose_cell().lock() {
        *guard = None;
    }
    if let Ok(mut guard) = platform_person_cell().lock() {
        *guard = None;
    }
    set_progress("");
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
    /// 检测到的瑕疵区域数（AI 修复用；0 = 没有 / 模型缺失）。
    #[serde(default)]
    pub blemishes: usize,
    /// AI 瑕疵修复模型（LaMa）是否可用。
    #[serde(default)]
    pub ai_repair: bool,
    /// AI 人脸增强模型（GFPGAN）是否可用。
    #[serde(default)]
    pub ai_restore: bool,
    /// AI 超分模型（Real-ESRGAN；导出增强）是否可用。
    #[serde(default)]
    pub ai_superres: bool,
    /// 是否拿到平台身体姿态（iOS Vision；身体塑形可用）。
    #[serde(default)]
    pub pose: bool,
    /// 提示（无脸 / 无人像等）。
    pub warnings: Vec<String>,
}

/// 人脸对齐计划（AI 增强：GFPGAN 恢复的输入 / 回贴几何）。
#[derive(Debug, Clone)]
struct FacePlan {
    /// 原图像素坐标 → 对齐空间（512）的正向仿射。
    map: crate::smartmask::mask::AffineMap,
    /// 对齐边长（当前固定 512）。
    align_size: u32,
}

/// 全局修图会话：最近一次识别结果（蒙版 + 关键点，与图像指纹绑定）。
struct RetouchSession {
    fingerprint: u64,
    size: (u32, u32),
    masks: crate::retouch::RetouchMasks,
    analysis: RetouchAnalysis,
    /// 瑕疵修复计划（AI LaMa；None = 无瑕疵 / 模型缺失）。
    blemish: Option<crate::retouch::BlemishPlan>,
    /// 人脸对齐计划（AI GFPGAN；None = 无脸）。
    face_plan: Option<FacePlan>,
    /// 恢复加权的全图权重（面部蒙版 ∩ 对齐覆盖，已羽化）。
    restore_weight: Option<crate::retouch::MaskPlane>,
    /// LaMa 修复缓存（整图；按会话指纹失效）。
    repair_cache: Option<Arc<image::RgbaImage>>,
    /// GFPGAN 恢复缓存（整图；按会话指纹失效）。
    restore_cache: Option<Arc<image::RgbaImage>>,
}

fn retouch_session_cell() -> &'static Mutex<Option<RetouchSession>> {
    static CELL: OnceLock<Mutex<Option<RetouchSession>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

/// 平台姿态（iOS Vision：人脸 + 身体关节）：按图像指纹缓存。
fn platform_pose_cell() -> &'static Mutex<Option<(u64, PlatformPose)>> {
    static CELL: OnceLock<Mutex<Option<(u64, PlatformPose)>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

/// 平台姿态数据（人脸关键点 + 身体关节；都可缺省）。
#[derive(Debug, Clone, Default)]
pub struct PlatformPose {
    /// Vision 人脸关键点（塑形回退）。
    pub face: Option<crate::retouch::VisionFace>,
    /// Vision 身体关节（姿态塑形）。
    pub body: Option<crate::retouch::BodyPose>,
}

/// 修图进度文本（阶段名；空 = 空闲）。UI 轮询显示加载动画文案。
fn progress_cell() -> &'static Mutex<String> {
    static CELL: OnceLock<Mutex<String>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(String::new()))
}

/// 当前修图进度文本（空 = 空闲）。
pub fn retouch_progress() -> String {
    progress_cell()
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

/// 设置进度文本（空 = 清除）。
fn set_progress(text: &str) {
    if let Ok(mut guard) = progress_cell().lock() {
        *guard = text.to_string();
    }
}

/// 提交平台姿态（iOS Vision）：人脸关键点 + 身体关节，绑定图像指纹。
///
/// `json` 形如 `{"face": {"contour": [[x,y], …], …}, "body": {"left_shoulder": [x,y,c], …}}`，
/// 坐标均为归一化（0..1，原点左上）。换图后自动失效。
pub fn submit_platform_pose(image: &Image, json: &str) -> std::result::Result<(), RuntimeError> {
    #[derive(serde::Deserialize, Default)]
    struct PoseJson {
        #[serde(default)]
        face: Option<crate::retouch::VisionFace>,
        #[serde(default)]
        body: Option<crate::retouch::BodyPose>,
    }
    let parsed: PoseJson = serde_json::from_str(json)
        .map_err(|err| RuntimeError::Failed(format!("姿态 JSON 解析失败：{err}")))?;
    let pose = PlatformPose {
        face: parsed.face.filter(|face| face.usable()),
        body: parsed.body,
    };
    if pose.face.is_none() && pose.body.is_none() {
        return Err(RuntimeError::Failed(
            "姿态数据为空（face / body 都缺失）".to_string(),
        ));
    }
    let mut guard = platform_pose_cell()
        .lock()
        .map_err(|_| RuntimeError::Failed("平台姿态锁失效".to_string()))?;
    *guard = Some((fingerprint(image), pose));
    Ok(())
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
    set_progress("AI 识别五官…");
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

    // 平台姿态（iOS Vision）：人脸关键点 + 身体关节（按指纹取用）。
    let platform_pose = platform_pose_cell()
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .filter(|(key, _)| *key == fingerprint(image))
        .map(|(_, pose)| pose);
    if let Some(pose) = platform_pose.as_ref() {
        masks.pose = pose.body.clone();
        masks.vision_face = pose.face.clone();
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

    // 人脸对齐计划（GFPGAN 增强用；最大人脸，512 对齐空间）。
    let face_plan = set.faces.first().and_then(|face| {
        crate::smartmask::pipeline::face_alignment(face, (width, height), 512).map(|map| FacePlan {
            map,
            align_size: 512,
        })
    });

    // 恢复加权（面部蒙版 ∩ 对齐覆盖，羽化）：GFPGAN 回贴的融合权重。
    let restore_weight = face_plan.as_ref().and_then(|plan| {
        let face = masks.face.as_ref()?;
        let softened = crate::retouch::feather(
            face,
            (width.min(height) as f32 * 0.02).clamp(2.0, 24.0),
        );
        let inverse = plan.map.invert()?;
        let align = plan.align_size as f32;
        let margin = 20.0f32;
        let mut data = vec![0.0f32; (width as usize) * (height as usize)];
        for y in 0..height {
            for x in 0..width {
                let face_weight = softened.get(x, y);
                if face_weight <= 0.001 {
                    continue;
                }
                let (u, v) = inverse.apply(x as f64 + 0.5, y as f64 + 0.5);
                if u < 0.0 || v < 0.0 || u >= align as f64 || v >= align as f64 {
                    continue;
                }
                let (u, v) = (u as f32, v as f32);
                let edge = (u.min(v).min(align - u).min(align - v) / margin).min(1.0);
                data[(y as usize) * (width as usize) + (x as usize)] = face_weight * edge;
            }
        }
        Some(crate::retouch::MaskPlane {
            width,
            height,
            data,
        })
    });

    // 瑕疵检测（AI LaMa 修复计划；模型缺失时不做，走经典去瑕疵）。
    let blemish = if runtime.stages.inpainter().is_some() {
        let plan = crate::retouch::detect_blemishes(image, masks.skin.as_ref());
        if plan.is_empty() {
            None
        } else {
            Some(plan)
        }
    } else {
        None
    };

    let analysis = RetouchAnalysis {
        faces: set.faces.len(),
        landmarks: landmark_count,
        skin: masks.skin.is_some(),
        person: masks.person.is_some(),
        blemishes: blemish.as_ref().map(|plan| plan.regions.len()).unwrap_or(0),
        ai_repair: runtime.stages.inpainter().is_some(),
        ai_restore: runtime.stages.restorer().is_some(),
        ai_superres: runtime.stages.superresolver().is_some(),
        pose: masks.pose.is_some() || masks.vision_face.is_some(),
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
        blemish,
        face_plan,
        restore_weight,
        repair_cache: None,
        restore_cache: None,
    });
    set_progress("");
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
    let runtime = runtime()?;
    let (width, height) = (image.width().max(1), image.height().max(1));
    let fingerprint = fingerprint(image);
    // 会话数据（蒙版 + 瑕疵计划 + 人脸计划 + 缓存）。
    let cached = {
        let guard = retouch_session_cell()
            .lock()
            .map_err(|_| RuntimeError::Failed("修图会话锁失效".to_string()))?;
        guard
            .as_ref()
            .filter(|session| session.fingerprint == fingerprint && session.size == (width, height))
            .map(|session| {
                (
                    session.masks.clone(),
                    session.blemish.clone(),
                    session.face_plan.clone(),
                    session.restore_weight.clone(),
                )
            })
    };
    let (masks, blemish, face_plan, restore_weight) = match cached {
        Some(data) => data,
        None => {
            retouch_analyze(image)?;
            let guard = retouch_session_cell()
                .lock()
                .map_err(|_| RuntimeError::Failed("修图会话锁失效".to_string()))?;
            let session = guard
                .as_ref()
                .ok_or_else(|| RuntimeError::Failed("修图识别结果缺失".to_string()))?;
            (
                session.masks.clone(),
                session.blemish.clone(),
                session.face_plan.clone(),
                session.restore_weight.clone(),
            )
        }
    };

    let mut working = image.clone();
    // ① AI 人脸增强（GFPGAN）：恢复缓存按图像指纹失效，只算一次。
    if params.has_restore() {
        if let (Some(restorer), Some(plan), Some(weight)) = (
            runtime.stages.restorer(),
            face_plan.as_ref(),
            restore_weight.as_ref(),
        ) {
            set_progress("AI 人脸增强…");
            if let Some(restored) = ensure_restore_cache(&runtime, image, plan, weight, restorer)?
            {
                crate::retouch::blend_weighted(
                    &mut working,
                    &restored,
                    weight,
                    params.restore / 100.0,
                );
            }
        } else if face_plan.is_none() {
            // 无脸：跳过增强（不打断其余修图）。
        }
    }
    // ② AI 瑕疵修复（LaMa）：区域局部重绘（缓存按图像指纹失效）。
    let mut ai_repaired = false;
    if params.blemish > 0.0 {
        if let (Some(inpainter), Some(plan)) = (runtime.stages.inpainter(), blemish.as_ref()) {
            set_progress("AI 修复瑕疵…");
            if let Some(repaired) = ensure_repair_cache(&runtime, image, plan, inpainter)? {
                crate::retouch::blend_weighted(
                    &mut working,
                    &repaired,
                    &plan.weight,
                    params.blemish / 100.0,
                );
                ai_repaired = true;
            }
        }
    }
    // ③ 经典像素级修图 + 塑形（AI 已处理的环节从经典参数里剔除，避免叠加）。
    let mut classical = params.clone();
    if ai_repaired {
        // AI 只修检测到的大斑点；经典去瑕疵以 60% 强度补扫细小斑点（不再触发软阈值大波动）。
        classical.blemish *= 0.6;
    }
    if classical.has_pixel_pass() {
        set_progress("皮肤美化…");
    }
    let output = crate::retouch::apply(&working, &masks, &classical);
    set_progress("");
    Ok(output)
}

/// 取 / 计算 GFPGAN 恢复缓存（整图；含对齐人脸恢复 + 逆映射回贴）。
fn ensure_restore_cache(
    runtime: &Arc<Runtime>,
    image: &Image,
    plan: &FacePlan,
    weight: &crate::retouch::MaskPlane,
    restorer: Arc<dyn crate::smartmask::traits::StageFaceRestore>,
) -> std::result::Result<Option<Arc<image::RgbaImage>>, RuntimeError> {
    let _ = runtime;
    let fingerprint = fingerprint(image);
    {
        let guard = retouch_session_cell()
            .lock()
            .map_err(|_| RuntimeError::Failed("修图会话锁失效".to_string()))?;
        if let Some(session) = guard.as_ref() {
            if session.fingerprint == fingerprint {
                if let Some(cache) = session.restore_cache.clone() {
                    return Ok(Some(cache));
                }
            }
        }
    }
    // 对齐裁剪 → 恢复 → 逆映射回贴。
    let crop = crate::smartmask::pipeline::warp_image(image, &plan.map, plan.align_size);
    let restored = restorer
        .restore(&crop)
        .map_err(|err| RuntimeError::Failed(format!("GFPGAN 恢复失败：{err}")))?;
    let Some(inverse) = plan.map.invert() else {
        return Ok(None);
    };
    let align = plan.align_size as f32;
    let (width, height) = (image.width().max(1), image.height().max(1));
    let mut out = image.clone();
    for y in 0..height {
        for x in 0..width {
            if weight.get(x, y) <= 0.001 {
                continue;
            }
            let (u, v) = inverse.apply(x as f64 + 0.5, y as f64 + 0.5);
            if u < 0.0 || v < 0.0 || u >= align as f64 || v >= align as f64 {
                continue;
            }
            let pixel = crate::smartmask::pipeline::sample_rgba(
                &restored,
                u as f32 - 0.5,
                v as f32 - 0.5,
            );
            let base = out.get_pixel(x, y).0;
            out.put_pixel(x, y, image::Rgba([pixel[0], pixel[1], pixel[2], base[3]]));
        }
    }
    let cache = Arc::new(out);
    if let Ok(mut guard) = retouch_session_cell().lock() {
        if let Some(session) = guard.as_mut() {
            if session.fingerprint == fingerprint {
                session.restore_cache = Some(cache.clone());
            }
        }
    }
    Ok(Some(cache))
}

/// 取 / 计算 LaMa 修复缓存（整图；逐区域裁剪重绘后回贴）。
fn ensure_repair_cache(
    runtime: &Arc<Runtime>,
    image: &Image,
    plan: &crate::retouch::BlemishPlan,
    inpainter: Arc<dyn crate::smartmask::traits::StageInpaint>,
) -> std::result::Result<Option<Arc<image::RgbaImage>>, RuntimeError> {
    let _ = runtime;
    let fingerprint = fingerprint(image);
    {
        let guard = retouch_session_cell()
            .lock()
            .map_err(|_| RuntimeError::Failed("修图会话锁失效".to_string()))?;
        if let Some(session) = guard.as_ref() {
            if session.fingerprint == fingerprint {
                if let Some(cache) = session.repair_cache.clone() {
                    return Ok(Some(cache));
                }
            }
        }
    }
    let (width, height) = (image.width().max(1), image.height().max(1));
    let mut out = image.clone();
    // 贪心打包：把相邻区域并入同一个 ≤512 的裁剪，减少 LaMa 调用次数。
    let pack_margin = 48u32;
    let union = |a: [u32; 4], b: [u32; 4]| -> [u32; 4] {
        let x0 = a[0].min(b[0]);
        let y0 = a[1].min(b[1]);
        let x1 = (a[0] + a[2]).max(b[0] + b[2]);
        let y1 = (a[1] + a[3]).max(b[1] + b[3]);
        [x0, y0, x1 - x0, y1 - y0]
    };
    let mut crops: Vec<[u32; 4]> = Vec::new();
    for region in &plan.regions {
        let rx = region[0].saturating_sub(pack_margin);
        let ry = region[1].saturating_sub(pack_margin);
        let rw = (region[2] + pack_margin * 2).min(width - rx).max(1);
        let rh = (region[3] + pack_margin * 2).min(height - ry).max(1);
        let candidate = [rx, ry, rw, rh];
        if let Some(crop) = crops.iter_mut().find(|crop| {
            let merged = union(**crop, candidate);
            merged[2] <= 512 && merged[3] <= 512
        }) {
            *crop = union(*crop, candidate);
        } else {
            crops.push(candidate);
        }
    }
    for crop_box in crops {
        // 裁剪框再加一层上下文；LaMa 适配器会把不足 512 的画布补齐。
        let [cx, cy, cw, ch] = crop_box;
        let expand = 64u32.min(512u32.saturating_sub(cw.max(ch)));
        let x0 = cx.saturating_sub(expand / 2);
        let y0 = cy.saturating_sub(expand / 2);
        let crop_w = (cw + expand).min(512).min(width - x0).max(1);
        let crop_h = (ch + expand).min(512).min(height - y0).max(1);
        let crop = image::imageops::crop_imm(image, x0, y0, crop_w, crop_h)
            .to_image();
        // 区域权重裁剪（同尺寸）。
        let mut mask_data = vec![0.0f32; (crop_w as usize) * (crop_h as usize)];
        for y in 0..crop_h {
            for x in 0..crop_w {
                let gx = x0 + x;
                let gy = y0 + y;
                mask_data[(y as usize) * (crop_w as usize) + (x as usize)] =
                    plan.weight.get(gx, gy);
            }
        }
        let mask = crate::retouch::MaskPlane {
            width: crop_w,
            height: crop_h,
            data: mask_data,
        };
        let repaired = inpainter
            .inpaint(&crop, &mask)
            .map_err(|err| RuntimeError::Failed(format!("LaMa 修复失败：{err}")))?;
        image::imageops::replace(&mut out, &repaired, x0 as i64, y0 as i64);
    }
    let cache = Arc::new(out);
    if let Ok(mut guard) = retouch_session_cell().lock() {
        if let Some(session) = guard.as_mut() {
            if session.fingerprint == fingerprint {
                session.repair_cache = Some(cache.clone());
            }
        }
    }
    Ok(Some(cache))
}

/// 导出增强：Real-ESRGAN 超分（分块）。`target_max_edge` ≤ 0 时按 4 倍输出。
///
/// 超大原图按「目标边长 / 原边长」计算倍率并逐块缩放输出，避免 4× 中间缓冲爆内存；
/// 输出上限 64 MP。
pub fn retouch_upscale(
    image: &Image,
    target_max_edge: u32,
) -> std::result::Result<Image, RuntimeError> {
    let runtime = runtime()?;
    let upscaler = runtime
        .stages
        .superresolver()
        .ok_or_else(|| RuntimeError::Unavailable("未随包超分模型（Real-ESRGAN）".to_string()))?;
    let (width, height) = (image.width().max(1), image.height().max(1));
    let max_edge = width.max(height);
    let target = if target_max_edge == 0 {
        max_edge.saturating_mul(4)
    } else {
        target_max_edge.max(max_edge)
    };
    let scale = (target as f32 / max_edge as f32).clamp(1.0, 4.0);
    let output_pixels =
        (width as f64 * scale as f64) * (height as f64 * scale as f64);
    if output_pixels > 64_000_000.0 {
        return Err(RuntimeError::Failed(format!(
            "增强输出过大（≈{:.0} MP，上限 64 MP）：请降低目标边长",
            output_pixels / 1e6
        )));
    }
    set_progress("高清增强（超分）…");
    let result = upscaler
        .upscale_scaled(image, scale)
        .map_err(|err| RuntimeError::Failed(format!("超分失败：{err}")));
    set_progress("");
    result
}

/// AI 修图模型是否就绪（LaMa / GFPGAN / Real-ESRGAN 任一可用）。
pub fn ai_retouch_ready() -> bool {
    runtime()
        .map(|runtime| {
            runtime.stages.inpainter().is_some()
                || runtime.stages.restorer().is_some()
                || runtime.stages.superresolver().is_some()
        })
        .unwrap_or(false)
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
