//! 智能蒙版模块（v1.4.0）：双阶段、模型可插拔、安全优先、为 AI 修图与自训练预留接口。
//!
//! 本模块只生产蒙版，不消费蒙版（消费方实现 [`traits::MaskConsumer`] /
//! [`traits::RetouchEngine`]）。整体分四层：
//!
//! | 层 | 文件 | 职责 |
//! | --- | --- | --- |
//! | 规格 | [`spec`] | [`spec::ModelSpec`] / [`spec::ModelRegistry`]：模型清单、SHA-256、白名单、量化与执行提供者 |
//! | 数据 | [`mask`] | [`mask::SmartMask`]：f32 0..1 权重 + 元数据；羽化 / 形态学 / 组合 / 坐标映射 |
//! | 接口 | [`traits`] | 双阶段 trait、预 / 后处理抽象、`RetouchEngine`、`DataRecorder` |
//! | 调度 | [`pipeline`] | [`pipeline::MaskPipeline`]：双阶段并行调度、缓存、蒙版装配 |
//!
//! 平台：
//! - Windows / 桌面：`ai` feature 下用 ONNX Runtime 跑 [`spec::ModelRegistry::builtin`] 里的模型
//!   （见 [`onnx`]），模型缺失时优雅降级；
//! - iOS / 移动：由平台提供者（Vision / CoreML / NNAPI）把权重栅格或五官多边形提交进
//!   [`mask::raster`] 的共享缓存（FFI `fdxmp_mask_raster_submit` / `fdxmp_mask_polygons_submit`），
//!   渲染、叠加、导出自动复用同一套坐标与后处理。
//!
//! 换模型只改 [`spec`] 的配置或新增一个 trait 适配器，上层业务零修改。

pub mod mask;
pub mod mock;
pub mod pipeline;
pub mod retouch;
pub mod spec;
pub mod traits;

#[cfg(any(feature = "ai", feature = "ai-static"))]
pub mod onnx;

#[cfg(any(feature = "ai", feature = "ai-static"))]
pub mod onnx_retouch;

#[cfg(any(feature = "ai", feature = "ai-static"))]
pub mod runtime;

pub use mask::{MaskMorphology, PolygonMask, SmartMask, SmartMaskKind};
pub use pipeline::{MaskPipeline, MaskSet, PipelineConfig};
pub use retouch::{DataRecorder, JsonlRecorder, NoopRetouchEngine, RetouchParams, TrainingSample};
#[cfg(any(feature = "ai", feature = "ai-static"))]
pub use runtime::{generate_into_cache, runtime_available, RuntimeError};
pub use spec::{
    ColorSpace, ExecutionProvider, InputLayout, ModelRegistry, ModelSpec, ModelTask, Precision,
};
pub use traits::{
    FaceDetection, FaceParseResult, MaskConsumer, MaskOrDetection, MaskRefiner, PlaneTensor,
    Postprocess, Preprocess, RetouchEngine, Stage1FaceDetector, Stage1PersonSegmenter,
    Stage2FaceLandmarks, Stage2FaceParser, Stage2PersonMatting, StageFaceRestore, StageInpaint,
    StageSuperRes,
};
