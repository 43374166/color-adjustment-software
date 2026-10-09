//! Windows AI 蒙版：ONNX Runtime（运行时动态加载）+ 分割模型。
//!
//! 模型（默认放在「exe 同目录 models/」或 `%LOCALAPPDATA%\fdxmp\models`，可用
//! 环境变量 `FDXMP_MODEL_DIR` 覆盖）：
//!
//! | 文件 | 用途 | 来源 |
//! | --- | --- | --- |
//! | `u2netp.onnx` | 主体 / 背景 / 物体（轻量显著目标，默认） | BritishWerewolf/U-2-Netp（Apache-2.0） |
//! | `birefnet_lite.onnx` | 主体 / 背景 / 物体（高质量，可选，213 MB） | onnx-community/BiRefNet_lite-ONNX（MIT） |
//! | `face_parsing_quantized.onnx` | 人物：面部 / 身体 / 皮肤 / 头发 / 衣服 | Xenova/face-parsing |
//! | `segformer_ade_b0.onnx` | 天空（ADE20K 语义分割） | lquint/segformer-b0-...-onnx |
//!
//! ONNX Runtime 以 `load-dynamic` 方式加载：优先 `FDXMP_ORT_DLL`，
//! 其次 exe 同目录 / 模型目录下的 `onnxruntime.dll`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use ort::session::Session;
use ort::value::Tensor;

use crate::error::{FdxmpError, Result};
use crate::masks::{mask_key, MaskRaster};
use crate::params::{MaskKind, MaskParams, PeoplePart};

/// 模型文件名与下载地址（hf-mirror 镜像）。
pub const MODEL_FILES: &[(&str, &str)] = &[
    (
        "u2netp.onnx",
        "https://hf-mirror.com/BritishWerewolf/U-2-Netp/resolve/main/onnx/model.onnx",
    ),
    (
        "face_parsing_quantized.onnx",
        "https://hf-mirror.com/Xenova/face-parsing/resolve/main/onnx/model_quantized.onnx",
    ),
    (
        "segformer_ade_b0.onnx",
        "https://hf-mirror.com/lquint/segformer-b0-finetuned-ade-512-512-onnx/resolve/main/onnx/model.onnx",
    ),
];

const MODEL_SUBJECT_LITE: &str = "u2netp.onnx";
const MODEL_SUBJECT_QUALITY: &str = "birefnet_lite.onnx";
const URL_SUBJECT_QUALITY: &str =
    "https://hf-mirror.com/onnx-community/BiRefNet_lite-ONNX/resolve/main/onnx/model.onnx";
const MODEL_FACE: &str = "face_parsing_quantized.onnx";
const MODEL_SKY: &str = "segformer_ade_b0.onnx";

/// 轻量主体模型（U2NetP）与高质量主体模型（BiRefNet-lite）输入边长。
const SUBJECT_LITE_SIZE: u32 = 320;
const SUBJECT_QUALITY_SIZE: u32 = 1024;
/// 语义分割模型输入边长。
const SEMANTIC_SIZE: u32 = 512;
/// ADE20K 中「天空」的类别下标。
const ADE_SKY_CLASS: usize = 2;

/// AI 运行时与模型状态。
#[derive(Debug, Clone)]
pub struct AiStatus {
    /// 是否找到 onnxruntime.dll（或可用默认搜索路径）。
    pub runtime: bool,
    /// 模型目录。
    pub model_dir: PathBuf,
    /// 主体 / 背景 / 物体模型是否存在。
    pub subject: bool,
    /// 是否已安装高质量主体模型（BiRefNet-lite）。
    pub subject_quality: bool,
    /// 人物部位模型是否存在。
    pub face: bool,
    /// 天空模型是否存在。
    pub sky: bool,
}

impl AiStatus {
    /// 是否全部模型就绪。
    pub fn models_ready(&self) -> bool {
        self.subject && self.face && self.sky
    }
}

/// 模型目录：`FDXMP_MODEL_DIR` → exe 同目录 `models/` → `%LOCALAPPDATA%\fdxmp\models`。
pub fn model_dir() -> PathBuf {
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
    std::env::temp_dir().join("fdxmp-models")
}

/// onnxruntime.dll 搜索路径（存在时返回）。
pub fn ort_dll_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FDXMP_ORT_DLL") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("onnxruntime.dll"));
        }
    }
    candidates.push(model_dir().join("onnxruntime.dll"));
    candidates.into_iter().find(|path| path.is_file())
}

/// 当前 AI 状态。
pub fn status() -> AiStatus {
    let dir = model_dir();
    AiStatus {
        runtime: ort_dll_path().is_some(),
        subject: dir.join(MODEL_SUBJECT_LITE).is_file()
            || dir.join(MODEL_SUBJECT_QUALITY).is_file(),
        subject_quality: dir.join(MODEL_SUBJECT_QUALITY).is_file(),
        face: dir.join(MODEL_FACE).is_file(),
        sky: dir.join(MODEL_SKY).is_file(),
        model_dir: dir,
    }
}

/// 初始化 ONNX Runtime（幂等）。
fn ensure_runtime() -> Result<()> {
    static RUNTIME: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    let result = RUNTIME.get_or_init(|| {
        let committed = match ort_dll_path() {
            Some(path) => ort::init_from(&path)
                .map_err(|err| format!("加载 {} 失败：{err}", path.display()))?
                .commit(),
            None => ort::init().commit(),
        };
        if committed || ort_dll_path().is_some() {
            Ok(())
        } else {
            Err("未找到 onnxruntime.dll（可放到 exe 目录，或设置 FDXMP_ORT_DLL）".to_string())
        }
    });
    result
        .clone()
        .map_err(|message| FdxmpError::Unsupported(format!("ONNX Runtime 初始化失败：{message}")))
}

/// 模型种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelKind {
    SubjectLite,
    SubjectQuality,
    Face,
    Sky,
}

impl ModelKind {
    fn file(self) -> &'static str {
        match self {
            ModelKind::SubjectLite => MODEL_SUBJECT_LITE,
            ModelKind::SubjectQuality => MODEL_SUBJECT_QUALITY,
            ModelKind::Face => MODEL_FACE,
            ModelKind::Sky => MODEL_SKY,
        }
    }
}

/// 惰性加载的会话集合。
#[derive(Default)]
struct Engine {
    subject_lite: Option<Session>,
    subject_quality: Option<Session>,
    face: Option<Session>,
    sky: Option<Session>,
}

impl Engine {
    fn session(&mut self, kind: ModelKind) -> Result<&mut Session> {
        let slot = match kind {
            ModelKind::SubjectLite => &mut self.subject_lite,
            ModelKind::SubjectQuality => &mut self.subject_quality,
            ModelKind::Face => &mut self.face,
            ModelKind::Sky => &mut self.sky,
        };
        if slot.is_none() {
            ensure_runtime()?;
            let path = model_dir().join(kind.file());
            if !path.is_file() {
                return Err(FdxmpError::Unsupported(format!(
                    "缺少模型文件 {}（可在蒙版面板下载）",
                    path.display()
                )));
            }
            let mut builder = Session::builder()
                .map_err(|err| FdxmpError::Unsupported(format!("ONNX 会话创建失败：{err}")))?;
            let session = builder.commit_from_file(&path).map_err(|err| {
                FdxmpError::Unsupported(format!("加载模型 {} 失败：{err}", path.display()))
            })?;
            *slot = Some(session);
        }
        Ok(slot.as_mut().expect("session 已加载"))
    }
}

fn engine() -> &'static Mutex<Engine> {
    static ENGINE: OnceLock<Mutex<Engine>> = OnceLock::new();
    ENGINE.get_or_init(|| Mutex::new(Engine::default()))
}

/// AI 蒙版栅格缓存：键为 [`mask_key`]（随蒙版参数与图像尺寸变化）。
fn raster_cache() -> &'static Mutex<HashMap<u64, Arc<MaskRaster>>> {
    static CACHE: OnceLock<Mutex<HashMap<u64, Arc<MaskRaster>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 查询已生成的 AI 蒙版栅格。
pub fn cached_raster(mask: &MaskParams, width: u32, height: u32) -> Option<Arc<MaskRaster>> {
    let key = mask_key(mask, width, height);
    raster_cache().lock().ok()?.get(&key).cloned()
}

/// 清空缓存（切换图片 / 重建代理时调用）。
pub fn clear_cache() {
    if let Ok(mut cache) = raster_cache().lock() {
        cache.clear();
    }
}

/// 生成 AI 蒙版并写入缓存（工作线程中调用）。
pub fn generate_into_cache(mask: &MaskParams, image: &image::RgbaImage) -> Result<()> {
    let (width, height) = (image.width().max(1), image.height().max(1));
    let key = mask_key(mask, width, height);
    let raster = generate(mask, image)?;
    if let Ok(mut cache) = raster_cache().lock() {
        cache.insert(key, Arc::new(raster));
        if cache.len() > 16 {
            cache.clear();
        }
    }
    Ok(())
}

/// 按蒙版类型生成基础权重栅格（不含反向 / 不透明度 / 羽化，由 `masks::generate_mask` 统一处理）。
pub fn generate(mask: &MaskParams, image: &image::RgbaImage) -> Result<MaskRaster> {
    match &mask.kind {
        MaskKind::AiSubject => run_subject(image),
        MaskKind::AiBackground => {
            let mut raster = run_subject(image)?;
            for weight in &mut raster.weights {
                *weight = 1.0 - *weight;
            }
            Ok(raster)
        }
        MaskKind::AiSky => run_sky(image),
        MaskKind::AiPeople { part } => run_face_part(image, *part),
        MaskKind::AiObject { point } => run_object(image, *point),
        _ => Err(FdxmpError::Unsupported(
            "该蒙版类型不需要 AI 推理".to_string(),
        )),
    }
}

/// 主体模型选择：默认 U2NetP（轻量）；设置 `FDXMP_AI_SUBJECT=hq` 且已下载
/// BiRefNet-lite 时改用高质量模型（更慢、内存占用更高）。
fn subject_model_choice() -> (ModelKind, u32) {
    let quality_available = model_dir().join(MODEL_SUBJECT_QUALITY).is_file();
    let preference = std::env::var("FDXMP_AI_SUBJECT").unwrap_or_default();
    let use_quality = match preference.to_ascii_lowercase().as_str() {
        "hq" | "quality" | "high" => quality_available,
        _ => false,
    };
    if use_quality {
        (ModelKind::SubjectQuality, SUBJECT_QUALITY_SIZE)
    } else {
        (ModelKind::SubjectLite, SUBJECT_LITE_SIZE)
    }
}

/// 主体（显著目标）：U2NetP（轻量）或 BiRefNet-lite（高质量，见 [`subject_model_choice`]）。
fn run_subject(image: &image::RgbaImage) -> Result<MaskRaster> {
    let (model, size) = subject_model_choice();
    let data = preprocess(image, size, [0.485, 0.456, 0.406], [0.229, 0.224, 0.225]);
    let (plane, plane_w, plane_h) = {
        let mut guard = engine()
            .lock()
            .map_err(|_| FdxmpError::Unsupported("AI 引擎锁失效".to_string()))?;
        let session = guard.session(model)?;
        let name = session
            .inputs()
            .first()
            .map(|input| input.name().to_string())
            .ok_or_else(|| FdxmpError::Unsupported("模型没有输入".to_string()))?;
        let tensor = Tensor::from_array(([1usize, 3, size as usize, size as usize], data))
            .map_err(|err| FdxmpError::Unsupported(format!("张量创建失败：{err}")))?;
        let outputs = session
            .run(ort::inputs![name.as_str() => tensor])
            .map_err(|err| FdxmpError::Unsupported(format!("推理失败：{err}")))?;
        let (shape, values) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|err| FdxmpError::Unsupported(format!("输出解析失败：{err}")))?;
        let dims: Vec<usize> = shape.iter().map(|dim| *dim as usize).collect();
        let (out_w, out_h) = match dims.as_slice() {
            [1, 1, h, w] => (*w, *h),
            [1, h, w] => (*w, *h),
            _ => {
                return Err(FdxmpError::Unsupported(format!(
                    "主体模型输出形状不支持：{dims:?}"
                )))
            }
        };
        (
            normalize_probabilities(values.to_vec()),
            out_w as u32,
            out_h as u32,
        )
    };
    let resized = resize_plane(&plane, plane_w, plane_h, image.width(), image.height());
    Ok(MaskRaster {
        width: image.width().max(1),
        height: image.height().max(1),
        weights: resized,
    })
}

/// 天空：ADE20K 语义分割的 sky 类别。
fn run_sky(image: &image::RgbaImage) -> Result<MaskRaster> {
    let weights = run_semantic(
        image,
        ModelKind::Sky,
        [0.485, 0.456, 0.406],
        [0.229, 0.224, 0.225],
        |labels, classes| {
            labels
                .iter()
                .map(|label| {
                    if *label == ADE_SKY_CLASS && classes > ADE_SKY_CLASS {
                        1.0f32
                    } else {
                        0.0f32
                    }
                })
                .collect()
        },
    )?;
    Ok(MaskRaster {
        width: image.width().max(1),
        height: image.height().max(1),
        weights,
    })
}

/// 人物部位：face-parsing（19 类）。
fn run_face_part(image: &image::RgbaImage, part: PeoplePart) -> Result<MaskRaster> {
    let weights = run_semantic(
        image,
        ModelKind::Face,
        [0.485, 0.456, 0.406],
        [0.229, 0.224, 0.225],
        |labels, _classes| {
            labels
                .iter()
                .map(|label| face_label_weight(*label, part))
                .collect()
        },
    )?;
    Ok(MaskRaster {
        width: image.width().max(1),
        height: image.height().max(1),
        weights,
    })
}

/// 物体（点按）：显著目标 + 点所在连通域。
fn run_object(image: &image::RgbaImage, point: [f32; 2]) -> Result<MaskRaster> {
    let base = run_subject(image)?;
    let width = base.width;
    let height = base.height;
    let px = ((point[0].clamp(0.0, 1.0)) * (width.saturating_sub(1)) as f32).round() as u32;
    let py = ((point[1].clamp(0.0, 1.0)) * (height.saturating_sub(1)) as f32).round() as u32;
    let component = connected_component_at(&base.weights, width, height, px, py, 0.5);
    let weights = base
        .weights
        .iter()
        .zip(component.iter())
        .map(|(weight, inside)| weight * inside)
        .collect();
    Ok(MaskRaster {
        width,
        height,
        weights,
    })
}

/// face-parsing 类别 → 部位权重。
///
/// 类别表（CelebAMask-HQ 19 类）：0 背景、1 皮肤、2 鼻子、3 眼镜、4/5 眼、6/7 眉、
/// 8/9 耳、10 嘴、11/12 唇、13 头发、14 帽子、15 耳环、16/17 脖子、18 衣服。
fn face_label_weight(label: usize, part: PeoplePart) -> f32 {
    let matches = match part {
        PeoplePart::Face => matches!(
            label,
            1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 15 | 16 | 17
        ),
        PeoplePart::Body => label != 0,
        PeoplePart::Skin => matches!(label, 1 | 8 | 9 | 15 | 16 | 17),
        PeoplePart::Hair => matches!(label, 13),
        PeoplePart::Clothes => matches!(label, 18),
    };
    if matches {
        1.0
    } else {
        0.0
    }
}

/// 语义分割通用流程：预处理 → 推理 → argmax → 自定义映射 → 缩放回图像尺寸。
fn run_semantic(
    image: &image::RgbaImage,
    kind: ModelKind,
    mean: [f32; 3],
    std: [f32; 3],
    map: impl Fn(&[usize], usize) -> Vec<f32>,
) -> Result<Vec<f32>> {
    let data = preprocess(image, SEMANTIC_SIZE, mean, std);
    let (labels, classes, out_w, out_h) = {
        let mut guard = engine()
            .lock()
            .map_err(|_| FdxmpError::Unsupported("AI 引擎锁失效".to_string()))?;
        let session = guard.session(kind)?;
        let name = session
            .inputs()
            .first()
            .map(|input| input.name().to_string())
            .ok_or_else(|| FdxmpError::Unsupported("模型没有输入".to_string()))?;
        let tensor = Tensor::from_array((
            [1usize, 3, SEMANTIC_SIZE as usize, SEMANTIC_SIZE as usize],
            data,
        ))
        .map_err(|err| FdxmpError::Unsupported(format!("张量创建失败：{err}")))?;
        let outputs = session
            .run(ort::inputs![name.as_str() => tensor])
            .map_err(|err| FdxmpError::Unsupported(format!("推理失败：{err}")))?;
        let (shape, values) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|err| FdxmpError::Unsupported(format!("输出解析失败：{err}")))?;
        let dims: Vec<usize> = shape.iter().map(|dim| *dim as usize).collect();
        let (classes, out_h, out_w) = match dims.as_slice() {
            [1, c, h, w] => (*c, *h, *w),
            _ => {
                return Err(FdxmpError::Unsupported(format!(
                    "语义分割输出形状不支持：{dims:?}"
                )))
            }
        };
        let mut labels = vec![0usize; out_h * out_w];
        for (index, label) in labels.iter_mut().enumerate() {
            let mut best = 0usize;
            let mut best_value = f32::MIN;
            for class in 0..classes {
                let value = values[class * out_h * out_w + index];
                if value > best_value {
                    best_value = value;
                    best = class;
                }
            }
            *label = best;
        }
        (labels, classes, out_w, out_h)
    };
    let mapped = map(&labels, classes);
    Ok(resize_plane(
        &mapped,
        out_w as u32,
        out_h as u32,
        image.width().max(1),
        image.height().max(1),
    ))
}

/// RGBA → NCHW f32（缩放到 `size`×`size`，按 mean/std 归一化）。
fn preprocess(image: &image::RgbaImage, size: u32, mean: [f32; 3], std: [f32; 3]) -> Vec<f32> {
    let resized = image::imageops::resize(image, size, size, image::imageops::FilterType::Triangle);
    let pixels = size as usize * size as usize;
    let mut data = vec![0.0f32; pixels * 3];
    for (index, pixel) in resized.pixels().enumerate() {
        for channel in 0..3 {
            let value = pixel.0[channel] as f32 / 255.0;
            data[channel * pixels + index] = (value - mean[channel]) / std[channel];
        }
    }
    data
}

/// 概率化输出：若数值已落在 0..1 则直接使用，否则做 sigmoid（兼容 logits 模型）。
fn normalize_probabilities(values: Vec<f32>) -> Vec<f32> {
    let min = values.iter().copied().fold(f32::MAX, f32::min);
    let max = values.iter().copied().fold(f32::MIN, f32::max);
    if min >= -0.01 && max <= 1.01 {
        values
    } else {
        values
            .into_iter()
            .map(|value| 1.0 / (1.0 + (-value).exp()))
            .collect()
    }
}

/// 双线性缩放单通道平面。
pub(crate) fn resize_plane(
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
    let mut output = vec![0.0f32; (dw * dh) as usize];
    for y in 0..dh {
        let fy = (y as f32 + 0.5) * sh as f32 / dh as f32 - 0.5;
        let y0 = fy.floor().clamp(0.0, (sh - 1) as f32) as u32;
        let y1 = (y0 + 1).min(sh - 1);
        let ty = (fy - y0 as f32).clamp(0.0, 1.0);
        for x in 0..dw {
            let fx = (x as f32 + 0.5) * sw as f32 / dw as f32 - 0.5;
            let x0 = fx.floor().clamp(0.0, (sw - 1) as f32) as u32;
            let x1 = (x0 + 1).min(sw - 1);
            let tx = (fx - x0 as f32).clamp(0.0, 1.0);
            let sample =
                |px: u32, py: u32| plane.get((py * sw + px) as usize).copied().unwrap_or(0.0);
            let top = sample(x0, y0) * (1.0 - tx) + sample(x1, y0) * tx;
            let bottom = sample(x0, y1) * (1.0 - tx) + sample(x1, y1) * tx;
            output[(y * dw + x) as usize] = top * (1.0 - ty) + bottom * ty;
        }
    }
    output
}

/// 取 (px, py) 所在连通域（8 连通，阈值以上），返回 0/1 权重。
pub(crate) fn connected_component_at(
    plane: &[f32],
    width: u32,
    height: u32,
    px: u32,
    py: u32,
    threshold: f32,
) -> Vec<f32> {
    let (w, h) = (width as usize, height as usize);
    let mut output = vec![0.0f32; w * h];
    if w == 0 || h == 0 || plane.len() < w * h {
        return output;
    }
    let index = |x: usize, y: usize| y * w + x;
    let start = index(px.min(w as u32 - 1) as usize, py.min(h as u32 - 1) as usize);
    if plane[start] < threshold {
        // 点按位置不在目标上：返回空蒙版（避免误选整张图）。
        return output;
    }
    let mut stack = vec![start];
    let mut visited = vec![false; w * h];
    visited[start] = true;
    while let Some(current) = stack.pop() {
        output[current] = 1.0;
        let x = current % w;
        let y = current / w;
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let neighbor = index(nx as usize, ny as usize);
                if !visited[neighbor] && plane[neighbor] >= threshold {
                    visited[neighbor] = true;
                    stack.push(neighbor);
                }
            }
        }
    }
    output
}

/// 下载缺失模型到模型目录（hf-mirror；`progress(文件名, 已下载字节, 总字节)`）。
pub fn download_models(mut progress: impl FnMut(&str, u64, u64)) -> Result<Vec<PathBuf>> {
    let dir = model_dir();
    std::fs::create_dir_all(&dir)?;
    let mut downloaded = Vec::new();
    for (file, url) in MODEL_FILES {
        let target = dir.join(file);
        if target.is_file() {
            continue;
        }
        download_file(url, &target, file, &mut progress)?;
        downloaded.push(target);
    }
    Ok(downloaded)
}

/// 下载高质量主体模型（BiRefNet-lite，约 213 MB，内存占用较高）。
pub fn download_quality_subject(mut progress: impl FnMut(&str, u64, u64)) -> Result<PathBuf> {
    let dir = model_dir();
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(MODEL_SUBJECT_QUALITY);
    if !target.is_file() {
        download_file(
            URL_SUBJECT_QUALITY,
            &target,
            MODEL_SUBJECT_QUALITY,
            &mut progress,
        )?;
    }
    Ok(target)
}

/// 单文件下载（先写 `.part` 再改名）。
fn download_file(
    url: &str,
    target: &Path,
    label: &str,
    progress: &mut impl FnMut(&str, u64, u64),
) -> Result<()> {
    progress(label, 0, 0);
    let mut response = ureq::get(url)
        .call()
        .map_err(|err| FdxmpError::Unsupported(format!("下载 {label} 失败：{err}")))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let bytes = response
        .body_mut()
        .read_to_vec()
        .map_err(|err| FdxmpError::Unsupported(format!("下载 {label} 失败：{err}")))?;
    let received = bytes.len() as u64;
    let temp = target.with_extension("part");
    std::fs::write(&temp, &bytes)?;
    std::fs::rename(&temp, target)?;
    progress(label, received, total.max(received));
    Ok(())
}

/// 把栅格插入缓存（测试用）。
#[cfg(test)]
pub(crate) fn insert_cached(mask: &MaskParams, width: u32, height: u32, raster: MaskRaster) {
    let key = mask_key(mask, width, height);
    if let Ok(mut cache) = raster_cache().lock() {
        cache.insert(key, Arc::new(raster));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_plane_keeps_constant_values() {
        let plane = vec![0.25f32; 4];
        let resized = resize_plane(&plane, 2, 2, 8, 6);
        assert_eq!(resized.len(), 48);
        assert!(resized.iter().all(|value| (value - 0.25).abs() < 1e-5));
    }

    #[test]
    fn connected_component_selects_region_containing_point() {
        // 两个分离的方块：左边 (0..2)×(0..2)，右边 (4..6)×(0..2)。
        let mut plane = vec![0.0f32; 8 * 4];
        for y in 0..2 {
            for x in 0..2 {
                plane[y * 8 + x] = 0.9;
            }
            for x in 4..6 {
                plane[y * 8 + x] = 0.9;
            }
        }
        let left = connected_component_at(&plane, 8, 4, 0, 0, 0.5);
        let right = connected_component_at(&plane, 8, 4, 5, 1, 0.5);
        assert_eq!(left.iter().filter(|value| **value > 0.5).count(), 4);
        assert_eq!(right.iter().filter(|value| **value > 0.5).count(), 4);
        assert!(left[4] < 0.5);
        assert!(right[0] < 0.5);
        // 点按到背景：空蒙版。
        let empty = connected_component_at(&plane, 8, 4, 3, 3, 0.5);
        assert!(empty.iter().all(|value| *value < 0.5));
    }

    #[test]
    fn face_label_mapping_covers_parts() {
        assert_eq!(face_label_weight(1, PeoplePart::Skin), 1.0);
        assert_eq!(face_label_weight(13, PeoplePart::Hair), 1.0);
        assert_eq!(face_label_weight(18, PeoplePart::Clothes), 1.0);
        assert_eq!(face_label_weight(0, PeoplePart::Body), 0.0);
        assert_eq!(face_label_weight(18, PeoplePart::Body), 1.0);
        assert_eq!(face_label_weight(13, PeoplePart::Face), 0.0);
    }

    #[test]
    fn probability_normalization_applies_sigmoid_for_logits() {
        let logits = vec![-2.0f32, 0.0, 2.0];
        let normalized = normalize_probabilities(logits);
        assert!(normalized[0] < 0.2 && normalized[2] > 0.8);
        let already = vec![0.1f32, 0.5, 0.9];
        assert_eq!(normalize_probabilities(already.clone()), already);
    }

    #[test]
    fn status_reports_missing_models_gracefully() {
        let status = status();
        // 不依赖模型是否下载：只要求路径与布尔值可读。
        assert!(!status.model_dir.as_os_str().is_empty());
    }
}
