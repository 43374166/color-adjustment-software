//! 模型清单与规格（配置驱动；禁止在业务代码里硬编码模型路径 / 尺寸 / 归一化）。
//!
//! 换模型步骤：
//! 1. 把新 ONNX 放进模型目录（命名规范 `{task}_{model}_{precision}.onnx`）；
//! 2. 在 [`ModelRegistry::builtin`]（或外部 JSON 配置）新增 / 修改 [`ModelSpec`]；
//! 3. 输入输出结构不同则新增一个 trait 适配器（见 [`crate::smartmask::traits`]）；
//! 4. 上层 [`crate::smartmask::pipeline::MaskPipeline`] 只认 trait，零修改。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{FdxmpError, Result};

/// 单个模型文件的大小上限（1 GiB）：拒绝超大输入（防 DoS）。
pub const MAX_MODEL_BYTES: u64 = 1024 * 1024 * 1024;

/// 允许下载模型的域名白名单（默认全部本地 / 随包分发，不联网）。
pub const DOWNLOAD_HOST_WHITELIST: &[&str] = &[
    "hf-mirror.com",
    "huggingface.co",
    "github.com",
    "objects.githubusercontent.com",
];

/// 模型任务。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTask {
    /// 第一阶段：人脸检测（框 + 5 点关键点）。
    FaceDetect,
    /// 第一阶段：人像粗分割（Alpha）。
    PersonCoarse,
    /// 第二阶段：人脸解析（皮肤 / 五官 / 头发等类别）。
    FaceParse,
    /// 第二阶段：人像精细抠图（Alpha）。
    PersonMatting,
}

impl ModelTask {
    /// 稳定的字符串标识（配置 / 日志用）。
    pub fn as_str(self) -> &'static str {
        match self {
            ModelTask::FaceDetect => "face_detect",
            ModelTask::PersonCoarse => "person_coarse",
            ModelTask::FaceParse => "face_parse",
            ModelTask::PersonMatting => "person_matting",
        }
    }
}

/// 张量布局。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputLayout {
    /// NCHW（PyTorch / 多数 ONNX 模型）。
    Nchw,
    /// NHWC（MediaPipe / TFLite 风格）。
    Nhwc,
}

/// 输入颜色空间。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSpace {
    /// 红绿蓝。
    Rgb,
    /// 蓝绿红。
    Bgr,
}

/// 推理执行提供者（后端可插拔）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProvider {
    /// CPU（默认，全平台可用）。
    Cpu,
    /// NVIDIA CUDA。
    Cuda,
    /// Apple CoreML（iOS / macOS）。
    CoreMl,
    /// Android NNAPI。
    Nnapi,
    /// Windows DirectML。
    DirectMl,
    /// NVIDIA TensorRT。
    TensorRt,
}

/// 数值精度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Precision {
    /// 32 位浮点（起步精度）。
    Fp32,
    /// 16 位浮点（移动端建议）。
    Fp16,
    /// 8 位整型（需校准；量化后 IoU 下降 > 2% 应回退）。
    Int8,
}

/// 输出激活函数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activation {
    /// 原样输出。
    None,
    /// sigmoid（logits → 概率）。
    Sigmoid,
    /// softmax（多分类 logits → 概率）。
    Softmax,
}

/// 缩放回原图的插值方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResizeMode {
    /// 双线性（蒙版默认）。
    Bilinear,
    /// 最近邻（类别图默认）。
    Nearest,
}

/// 后处理规格（全部由配置驱动）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PostprocessSpec {
    /// 激活函数。
    pub activation: Activation,
    /// 检测置信度阈值（人脸框等）。
    pub score_threshold: f32,
    /// 检测 NMS IoU 阈值。
    pub nms_iou: f32,
    /// 蒙版二值化 / 归一化参考阈值（0 = 不做阈值化）。
    pub mask_threshold: f32,
    /// 缩放回原图方式。
    pub resize: ResizeMode,
}

impl Default for PostprocessSpec {
    fn default() -> Self {
        Self {
            activation: Activation::None,
            score_threshold: 0.5,
            nms_iou: 0.4,
            mask_threshold: 0.0,
            resize: ResizeMode::Bilinear,
        }
    }
}

/// 单个模型的完整规格（对应主提示词「二点五、模型清单与规格」的 `ModelSpec`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelSpec {
    /// 模型名（日志 / UI 展示）。
    pub name: String,
    /// 任务类型。
    pub task: ModelTask,
    /// 文件名（相对模型目录；命名规范 `{task}_{model}_{precision}.onnx`）。
    pub path: String,
    /// 期望 SHA-256（小写十六进制；空串 = 不校验，仅用于开发期）。
    pub sha256: String,
    /// 固定输入尺寸 `(W, H)`（避免动态 shape 导致移动端性能抖动）。
    pub input_size: (u32, u32),
    /// 张量布局。
    pub input_layout: InputLayout,
    /// 输入颜色空间。
    pub input_color: ColorSpace,
    /// 归一化均值（`(像素 * input_scale - mean) / std`）。
    pub input_mean: [f32; 3],
    /// 归一化标准差。
    pub input_std: [f32; 3],
    /// 输入缩放（通常 1/255；SCRFD 等 0..255 输入的模型用 1.0）。
    pub input_scale: f32,
    /// ONNX 输出名（空 = 按顺序取全部输出）。
    pub output_names: Vec<String>,
    /// ONNX 输出形状（`-1` 表示动态维；仅作校验参考）。
    pub output_shapes: Vec<Vec<i64>>,
    /// 后处理规格。
    pub postprocess: PostprocessSpec,
    /// 执行提供者。
    pub provider: ExecutionProvider,
    /// 数值精度。
    pub precision: Precision,
    /// 许可证。
    pub license: String,
    /// 来源地址（下载白名单校验用）。
    pub source_url: String,
    /// 是否可用自有数据微调（为自训练留口）。
    pub trainable: bool,
    /// 训练代码 / 仓库（可空）。
    pub training_repo: Option<String>,
    /// 模型版本号（缓存键与元数据用；换模型记得递增）。
    pub version: String,
}

impl Default for ModelSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            task: ModelTask::FaceDetect,
            path: String::new(),
            sha256: String::new(),
            input_size: (0, 0),
            input_layout: InputLayout::Nchw,
            input_color: ColorSpace::Rgb,
            input_mean: [0.0; 3],
            input_std: [1.0; 3],
            input_scale: 1.0 / 255.0,
            output_names: Vec::new(),
            output_shapes: Vec::new(),
            postprocess: PostprocessSpec::default(),
            provider: ExecutionProvider::Cpu,
            precision: Precision::Fp32,
            license: String::new(),
            source_url: String::new(),
            trainable: false,
            training_repo: None,
            version: "1.0.0".to_string(),
        }
    }
}

impl ModelSpec {
    /// 归一化一个像素（0..255）为模型输入值：`(v * scale - mean) / std`。
    pub fn normalize(&self, value: f32, channel: usize) -> f32 {
        let mean = self.input_mean[channel.min(2)];
        let std = self.input_std[channel.min(2)].max(1e-6);
        (value * self.input_scale - mean) / std
    }
}

/// 单个模型文件的状态（UI / FFI 展示用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelStatus {
    /// 模型名。
    pub name: String,
    /// 任务。
    pub task: ModelTask,
    /// 文件名。
    pub path: String,
    /// 文件是否存在。
    pub present: bool,
    /// 文件大小（字节；不存在为 0）。
    pub bytes: u64,
    /// 是否登记了 SHA-256。
    pub hash_registered: bool,
    /// 校验结果：`ok` / `mismatch` / `missing` / `unregistered` / `error`。
    pub hash_state: String,
    /// 许可证。
    pub license: String,
    /// 是否可训练（自训练留口）。
    pub trainable: bool,
}

/// 模型注册表：统一管理加载路径、版本、哈希与下载白名单。
#[derive(Debug, Clone)]
pub struct ModelRegistry {
    dir: PathBuf,
    specs: Vec<ModelSpec>,
}

impl ModelRegistry {
    /// 默认模型目录：`FDXMP_MODEL_DIR` → exe 同目录 `models/` → `%LOCALAPPDATA%\fdxmp\models`
    /// （非 Windows 退回 `$XDG_DATA_HOME/fdxmp/models` 或用户目录）。
    pub fn default_dir() -> PathBuf {
        if let Some(dir) = std::env::var_os("FDXMP_MODEL_DIR") {
            return PathBuf::from(dir);
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let candidate = parent.join("models");
                if candidate.is_dir() {
                    return candidate;
                }
            }
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("fdxmp").join("models");
        }
        if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(data).join("fdxmp").join("models");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".local/share/fdxmp/models");
        }
        std::env::temp_dir().join("fdxmp-models")
    }

    /// 用默认目录 + 内置清单创建注册表。
    pub fn new() -> Self {
        Self::with_dir(Self::default_dir())
    }

    /// 用指定目录 + 内置清单创建注册表。
    pub fn with_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            specs: Self::builtin(),
        }
    }

    /// 模型目录。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 全部模型规格。
    pub fn specs(&self) -> &[ModelSpec] {
        &self.specs
    }

    /// 追加 / 覆盖一个规格（换模型只改配置）。
    pub fn upsert(&mut self, spec: ModelSpec) {
        if let Some(existing) = self.specs.iter_mut().find(|item| item.task == spec.task) {
            *existing = spec;
        } else {
            self.specs.push(spec);
        }
    }

    /// 按任务取规格。
    pub fn spec_for(&self, task: ModelTask) -> Option<&ModelSpec> {
        self.specs.iter().find(|spec| spec.task == task)
    }

    /// 按名称取规格。
    pub fn spec_by_name(&self, name: &str) -> Option<&ModelSpec> {
        self.specs.iter().find(|spec| spec.name == name)
    }

    /// 模型文件路径（不校验存在性）。
    pub fn path_of(&self, spec: &ModelSpec) -> PathBuf {
        self.dir.join(&spec.path)
    }

    /// 计算文件 SHA-256（小写十六进制，流式）。
    pub fn sha256_of_file(path: &Path) -> Result<String> {
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher)?;
        Ok(hex_lower(&hasher.finalize()))
    }

    /// 校验单个模型：存在、大小上限、SHA-256（登记时）。
    pub fn verify(&self, spec: &ModelSpec) -> ModelStatus {
        let path = self.path_of(spec);
        let metadata = std::fs::metadata(&path).ok();
        let present = metadata
            .as_ref()
            .map(|meta| meta.is_file())
            .unwrap_or(false);
        let bytes = metadata.map(|meta| meta.len()).unwrap_or(0);
        let mut status = ModelStatus {
            name: spec.name.clone(),
            task: spec.task,
            path: spec.path.clone(),
            present,
            bytes,
            hash_registered: !spec.sha256.is_empty(),
            hash_state: if present { "unregistered" } else { "missing" }.to_string(),
            license: spec.license.clone(),
            trainable: spec.trainable,
        };
        if !present {
            return status;
        }
        if bytes > MAX_MODEL_BYTES {
            status.hash_state = "error".to_string();
            return status;
        }
        if spec.sha256.is_empty() {
            return status;
        }
        status.hash_state = match Self::sha256_of_file(&path) {
            Ok(hash) if hash.eq_ignore_ascii_case(&spec.sha256) => "ok".to_string(),
            Ok(_) => "mismatch".to_string(),
            Err(_) => "error".to_string(),
        };
        status
    }

    /// 校验全部模型状态。
    pub fn status(&self) -> Vec<ModelStatus> {
        self.specs.iter().map(|spec| self.verify(spec)).collect()
    }

    /// 校验并返回可用模型路径（哈希不匹配直接拒绝）。
    pub fn verified_path(&self, spec: &ModelSpec) -> Result<PathBuf> {
        let path = self.path_of(spec);
        let metadata = std::fs::metadata(&path).map_err(|_| {
            FdxmpError::Unsupported(format!(
                "缺少模型文件 {}（可在蒙版面板下载）",
                path.display()
            ))
        })?;
        if !metadata.is_file() {
            return Err(FdxmpError::Unsupported(format!(
                "模型路径不是文件：{}",
                path.display()
            )));
        }
        if metadata.len() > MAX_MODEL_BYTES {
            return Err(FdxmpError::Unsupported(format!(
                "模型文件超过 {} GiB 上限，已拒绝加载：{}",
                MAX_MODEL_BYTES / (1024 * 1024 * 1024),
                path.display()
            )));
        }
        if !spec.sha256.is_empty() {
            let hash = Self::sha256_of_file(&path)?;
            if !hash.eq_ignore_ascii_case(&spec.sha256) {
                return Err(FdxmpError::Unsupported(format!(
                    "模型 {} 哈希不匹配（期望 {}，实际 {}），已拒绝加载",
                    spec.name, spec.sha256, hash
                )));
            }
        }
        Ok(path)
    }

    /// 下载白名单校验：只允许白名单域名 + HTTPS + 指定来源地址。
    pub fn download_allowed(url: &str) -> Result<()> {
        let rest = url
            .strip_prefix("https://")
            .ok_or_else(|| FdxmpError::Unsupported("仅允许 HTTPS 下载模型".to_string()))?;
        let host = rest.split('/').next().unwrap_or_default();
        if DOWNLOAD_HOST_WHITELIST
            .iter()
            .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
        {
            Ok(())
        } else {
            Err(FdxmpError::Unsupported(format!(
                "下载来源不在白名单：{host}"
            )))
        }
    }

    /// 内置模型清单（对应主提示词「模型清单与规格」；换模型改这里或 `upsert`）。
    ///
    /// 说明：
    /// - 所有模型固定输入尺寸；
    /// - 优先选择有训练代码、可微调的模型（`trainable` / `training_repo`）；
    /// - `sha256` 为随包 / 镜像下载的固定版本哈希，加载前校验。
    pub fn builtin() -> Vec<ModelSpec> {
        vec![
            ModelSpec {
                name: "scrfd-2.5g-bnkps".to_string(),
                task: ModelTask::FaceDetect,
                path: "face_detector_scrfd2p5g_bnkps_fp32.onnx".to_string(),
                sha256: "3f1ac54e769cb5fd76eda11ac3c088eed78d1f51a935a839d04d49b0e770219e"
                    .to_string(),
                input_size: (640, 640),
                input_layout: InputLayout::Nchw,
                input_color: ColorSpace::Rgb,
                input_mean: [127.5, 127.5, 127.5],
                input_std: [128.0, 128.0, 128.0],
                input_scale: 1.0,
                output_names: vec![
                    "score_8".to_string(),
                    "score_16".to_string(),
                    "score_32".to_string(),
                    "bbox_8".to_string(),
                    "bbox_16".to_string(),
                    "bbox_32".to_string(),
                    "kps_8".to_string(),
                    "kps_16".to_string(),
                    "kps_32".to_string(),
                ],
                output_shapes: vec![
                    vec![1, -1, 1],
                    vec![1, -1, 1],
                    vec![1, -1, 1],
                    vec![1, -1, 4],
                    vec![1, -1, 4],
                    vec![1, -1, 4],
                    vec![1, -1, 10],
                    vec![1, -1, 10],
                    vec![1, -1, 10],
                ],
                postprocess: PostprocessSpec {
                    activation: Activation::None,
                    score_threshold: 0.5,
                    nms_iou: 0.4,
                    mask_threshold: 0.0,
                    resize: ResizeMode::Bilinear,
                },
                provider: ExecutionProvider::Cpu,
                precision: Precision::Fp32,
                license: "Apache-2.0".to_string(),
                source_url:
                    "https://hf-mirror.com/RuteNL/SCRFD-face-detection-ONNX/resolve/main/2.5g_bnkps.onnx"
                        .to_string(),
                trainable: true,
                training_repo: Some("https://github.com/deepinsight/insightface".to_string()),
                version: "1.0.0".to_string(),
            },
            ModelSpec {
                name: "modnet".to_string(),
                task: ModelTask::PersonCoarse,
                path: "person_coarse_modnet_fp32.onnx".to_string(),
                sha256: "07c308cf0fc7e6e8b2065a12ed7fc07e1de8febb7dc7839d7b7f15dd66584df9"
                    .to_string(),
                input_size: (512, 512),
                input_layout: InputLayout::Nchw,
                input_color: ColorSpace::Rgb,
                input_mean: [0.5, 0.5, 0.5],
                input_std: [0.5, 0.5, 0.5],
                input_scale: 1.0 / 255.0,
                output_names: Vec::new(),
                output_shapes: vec![vec![1, 1, -1, -1]],
                postprocess: PostprocessSpec {
                    // MODNet 导出已内置 sigmoid（输出即 Alpha 0..1），不重复激活。
                    activation: Activation::None,
                    score_threshold: 0.5,
                    nms_iou: 0.4,
                    mask_threshold: 0.0,
                    resize: ResizeMode::Bilinear,
                },
                provider: ExecutionProvider::Cpu,
                precision: Precision::Fp32,
                license: "Apache-2.0".to_string(),
                source_url: "https://hf-mirror.com/Xenova/modnet/resolve/main/onnx/model.onnx"
                    .to_string(),
                trainable: true,
                training_repo: Some("https://github.com/ZHKKKe/MODNet".to_string()),
                version: "1.0.0".to_string(),
            },
            ModelSpec {
                name: "bisenet-face-parsing".to_string(),
                task: ModelTask::FaceParse,
                path: "face_parsing_quantized.onnx".to_string(),
                sha256: "5bab9bfb3cb979f3098ac3b934b1641dbf87f835e0b03c2ca6d88dcf18c83d27"
                    .to_string(),
                input_size: (512, 512),
                input_layout: InputLayout::Nchw,
                input_color: ColorSpace::Rgb,
                input_mean: [0.485, 0.456, 0.406],
                input_std: [0.229, 0.224, 0.225],
                input_scale: 1.0 / 255.0,
                output_names: Vec::new(),
                output_shapes: vec![vec![1, 19, 512, 512]],
                postprocess: PostprocessSpec {
                    activation: Activation::Softmax,
                    score_threshold: 0.5,
                    nms_iou: 0.4,
                    mask_threshold: 0.0,
                    resize: ResizeMode::Nearest,
                },
                provider: ExecutionProvider::Cpu,
                precision: Precision::Int8,
                license: "MIT".to_string(),
                source_url:
                    "https://hf-mirror.com/Xenova/face-parsing/resolve/main/onnx/model_quantized.onnx"
                        .to_string(),
                trainable: true,
                training_repo: Some("https://github.com/zllrunning/face-parsing.PyTorch".to_string()),
                version: "1.0.0".to_string(),
            },
            ModelSpec {
                name: "u2net-portrait-lite".to_string(),
                task: ModelTask::PersonMatting,
                path: "u2netp.onnx".to_string(),
                sha256: "309c8469258dda742793dce0ebea8e6dd393174f89934733ecc8b14c76f4ddd8"
                    .to_string(),
                input_size: (320, 320),
                input_layout: InputLayout::Nchw,
                input_color: ColorSpace::Rgb,
                input_mean: [0.485, 0.456, 0.406],
                input_std: [0.229, 0.224, 0.225],
                input_scale: 1.0 / 255.0,
                output_names: Vec::new(),
                output_shapes: vec![vec![1, 1, 320, 320]],
                postprocess: PostprocessSpec {
                    activation: Activation::None,
                    score_threshold: 0.5,
                    nms_iou: 0.4,
                    mask_threshold: 0.0,
                    resize: ResizeMode::Bilinear,
                },
                provider: ExecutionProvider::Cpu,
                precision: Precision::Fp32,
                license: "Apache-2.0".to_string(),
                source_url:
                    "https://hf-mirror.com/BritishWerewolf/U-2-Netp/resolve/main/onnx/model.onnx"
                        .to_string(),
                trainable: true,
                training_repo: Some("https://github.com/xuebinqin/U-2-Net".to_string()),
                version: "1.0.0".to_string(),
            },
        ]
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 字节 → 小写十六进制。
pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_specs_cover_all_tasks_and_are_consistent() {
        let specs = ModelRegistry::builtin();
        assert_eq!(specs.len(), 4, "四个任务各一个模型");
        let mut tasks = Vec::new();
        for spec in &specs {
            assert!(!spec.name.is_empty());
            assert!(spec.path.ends_with(".onnx"));
            assert!(spec.input_size.0 > 0 && spec.input_size.1 > 0);
            assert!(spec.input_std.iter().all(|value| *value > 0.0));
            assert!(!spec.license.is_empty(), "{} 缺少许可证", spec.name);
            assert!(!spec.source_url.is_empty());
            assert!(!spec.sha256.is_empty(), "{} 缺少哈希", spec.name);
            assert_eq!(spec.sha256.len(), 64, "{} 哈希格式错误", spec.name);
            tasks.push(spec.task);
        }
        for task in [
            ModelTask::FaceDetect,
            ModelTask::PersonCoarse,
            ModelTask::FaceParse,
            ModelTask::PersonMatting,
        ] {
            assert!(tasks.contains(&task), "缺少任务 {task:?}");
        }
    }

    #[test]
    fn normalization_follows_scale_mean_std() {
        let spec = ModelSpec {
            input_scale: 1.0 / 255.0,
            input_mean: [0.5, 0.5, 0.5],
            input_std: [0.5, 0.5, 0.5],
            ..ModelSpec::default()
        };
        // 纯白 → (1 - 0.5) / 0.5 = 1；纯黑 → -1。
        assert!((spec.normalize(255.0, 0) - 1.0).abs() < 1e-6);
        assert!((spec.normalize(0.0, 1) + 1.0).abs() < 1e-6);
        // SCRFD 风格：(x - 127.5) / 128。
        let scrfd = ModelSpec {
            input_scale: 1.0,
            input_mean: [127.5, 127.5, 127.5],
            input_std: [128.0, 128.0, 128.0],
            ..ModelSpec::default()
        };
        assert!((scrfd.normalize(127.5, 0)).abs() < 1e-6);
        assert!((scrfd.normalize(255.0, 0) - 0.996_093_75).abs() < 1e-6);
    }

    #[test]
    fn download_whitelist_accepts_https_known_hosts_only() {
        assert!(ModelRegistry::download_allowed(
            "https://hf-mirror.com/BritishWerewolf/U-2-Netp/resolve/main/onnx/model.onnx"
        )
        .is_ok());
        assert!(ModelRegistry::download_allowed(
            "https://github.com/xuebinqin/U-2-Net/releases/download/v1.0/u2net.pth"
        )
        .is_ok());
        assert!(ModelRegistry::download_allowed("http://hf-mirror.com/model.onnx").is_err());
        assert!(ModelRegistry::download_allowed("https://evil.example.com/model.onnx").is_err());
    }

    #[test]
    fn verify_reports_missing_and_hash_mismatch() {
        let dir = std::env::temp_dir().join(format!("fdxmp-smartmask-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let registry = ModelRegistry::with_dir(&dir);
        let spec = ModelSpec {
            name: "test".to_string(),
            task: ModelTask::FaceDetect,
            path: "missing.onnx".to_string(),
            sha256: "00".repeat(32),
            ..ModelSpec::default()
        };
        let status = registry.verify(&spec);
        assert!(!status.present);
        assert_eq!(status.hash_state, "missing");

        // 写入错误内容 → 哈希不匹配。
        let path = dir.join("missing.onnx");
        std::fs::write(&path, b"not a model").unwrap();
        let status = registry.verify(&spec);
        assert!(status.present);
        assert_eq!(status.hash_state, "mismatch");

        // 写入真实内容并登记正确哈希 → ok。
        let good = b"onnx-bytes".to_vec();
        std::fs::write(&path, &good).unwrap();
        let registry = ModelRegistry::with_dir(&dir);
        let mut hasher = Sha256::new();
        hasher.update(&good);
        let spec_ok = ModelSpec {
            sha256: hex_lower(&hasher.finalize()),
            ..spec.clone()
        };
        assert_eq!(registry.verify(&spec_ok).hash_state, "ok");
        assert!(registry.verified_path(&spec_ok).is_ok());
        assert!(
            registry.verified_path(&spec).is_err(),
            "哈希不匹配必须拒绝加载"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
