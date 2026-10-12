//! AI 修图接口与训练数据记录（v1.4.0 只定义接口，不实现具体修图 / 训练）。
//!
//! - [`RetouchEngine`]：后续修图模型接收「原图 + 标准蒙版 + 参数」，可独立替换；
//! - [`DataRecorder`] / [`JsonlRecorder`]：**用户明确授权后**才导出训练数据；
//!   默认不联网、不上传，数据只留在本地或用户指定位置；
//! - 导出布局兼容 DUTS / CelebAMask-HQ / COCO 风格（见 [`DatasetLayout`]）。

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{FdxmpError, Result};
use crate::smartmask::mask::{SmartMask, SmartMaskKind};
use crate::smartmask::traits::{Image, RetouchEngine};

/// 修图参数（未来接入磨皮 / 肤色统一 / 祛痘 / 祛青筋 / 瘦脸 / 换背景 / inpainting / relight）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RetouchParams {
    /// 修图类型（`smooth` / `whiten` / `inpaint` / `relight` …）。
    pub kind: String,
    /// 强度 0..1。
    pub strength: f32,
    /// 其它参数（各修图模型自定义）。
    pub extra: serde_json::Value,
}

impl Default for RetouchParams {
    fn default() -> Self {
        Self {
            kind: "none".to_string(),
            strength: 0.0,
            extra: serde_json::Value::Null,
        }
    }
}

/// 空修图引擎：返回原图副本（占位实现，验证接口连通性）。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopRetouchEngine;

impl RetouchEngine for NoopRetouchEngine {
    fn apply(&self, image: &Image, _mask: &SmartMask, _params: &RetouchParams) -> Result<Image> {
        Ok(image.clone())
    }
}

/// 一条训练样本（导出用；图片 / 蒙版以**路径**引用，避免内存复制）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainingSample {
    /// 样本 id（文件名主干）。
    pub id: String,
    /// 原图路径（本地）。
    pub image_path: String,
    /// 蒙版：类型 + 路径。
    pub masks: Vec<(SmartMaskKind, String)>,
    /// 来源模型。
    pub model: String,
    /// 模型版本。
    pub model_version: String,
    /// 生成参数（JSON）。
    pub params_json: String,
    /// 生成时间（Unix 毫秒）。
    pub created_at: u64,
}

/// 训练数据记录接口：用户授权后由调用方触发，默认绝不上传。
pub trait DataRecorder {
    /// 记录一条样本（实现方决定落盘位置与格式）。
    fn record(&mut self, sample: TrainingSample) -> Result<()>;

    /// 已记录样本数。
    fn len(&self) -> usize;

    /// 是否为空。
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 导出布局（兼容常见数据集目录约定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatasetLayout {
    /// DUTS：`images/` + `masks/`，同名文件。
    Duts,
    /// CelebAMask-HQ：`CelebA-HQ-img/` + `CelebAMask-HQ-mask-anno/<id>/`，每类一张 png。
    CelebAMaskHq,
    /// COCO 风格：`images/` + `annotations.json`（几何用包围盒，类别名为蒙版类型）。
    Coco,
}

/// 训练数据无网络上传：本模块不包含任何网络代码，导出只写本地文件。
///
/// JSONL 记录器：样本追加写入 `<dir>/samples.jsonl`；蒙版可另存 PNG（16 位灰度）。
#[derive(Debug, Clone)]
pub struct JsonlRecorder {
    dir: PathBuf,
    count: usize,
}

impl JsonlRecorder {
    /// 打开（或创建）记录目录。
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir, count: 0 })
    }

    /// 记录目录。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 另存一张蒙版为 16 位灰度 PNG（0..1 → 0..65535）。
    pub fn save_mask_png(&self, mask: &SmartMask, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let buffer: image::ImageBuffer<image::Luma<u16>, Vec<u16>> =
            image::ImageBuffer::from_fn(mask.width, mask.height, |x, y| {
                let value = mask.get(x, y).clamp(0.0, 1.0);
                image::Luma([(value * 65535.0).round() as u16])
            });
        buffer
            .save(path)
            .map_err(|err| FdxmpError::Unsupported(format!("写入蒙版 PNG 失败：{err}")))
    }
}

impl DataRecorder for JsonlRecorder {
    fn record(&mut self, sample: TrainingSample) -> Result<()> {
        let path = self.dir.join("samples.jsonl");
        let line = serde_json::to_string(&sample)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(file, "{line}")?;
        self.count += 1;
        Ok(())
    }

    fn len(&self) -> usize {
        self.count
    }
}

/// 按布局导出样本（本地目录；返回写入文件数）。
pub fn export_dataset(
    target: &Path,
    layout: DatasetLayout,
    samples: &[TrainingSample],
) -> Result<usize> {
    match layout {
        DatasetLayout::Duts => export_duts(target, samples),
        DatasetLayout::CelebAMaskHq => export_celebamask(target, samples),
        DatasetLayout::Coco => export_coco(target, samples),
    }
}

fn copy_image(source: &Path, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(source, target).map_err(|err| {
        FdxmpError::Unsupported(format!(
            "复制训练数据失败：{} → {}（{err}）",
            source.display(),
            target.display()
        ))
    })?;
    Ok(())
}

fn export_duts(target: &Path, samples: &[TrainingSample]) -> Result<usize> {
    let mut written = 0;
    for sample in samples {
        let image_target = target.join("images").join(format!("{}.png", sample.id));
        copy_image(Path::new(&sample.image_path), &image_target)?;
        written += 1;
        for (kind, path) in &sample.masks {
            let mask_target =
                target
                    .join("masks")
                    .join(format!("{}_{}.png", sample.id, kind.as_str()));
            copy_image(Path::new(path), &mask_target)?;
            written += 1;
        }
    }
    Ok(written)
}

fn export_celebamask(target: &Path, samples: &[TrainingSample]) -> Result<usize> {
    let mut written = 0;
    for sample in samples {
        copy_image(
            Path::new(&sample.image_path),
            &target
                .join("CelebA-HQ-img")
                .join(format!("{}.png", sample.id)),
        )?;
        written += 1;
        for (kind, path) in &sample.masks {
            copy_image(
                Path::new(path),
                &target
                    .join("CelebAMask-HQ-mask-anno")
                    .join(&sample.id)
                    .join(format!("{}.png", kind.as_str())),
            )?;
            written += 1;
        }
    }
    Ok(written)
}

fn export_coco(target: &Path, samples: &[TrainingSample]) -> Result<usize> {
    std::fs::create_dir_all(target)?;
    let mut images = Vec::new();
    let mut annotations = Vec::new();
    let mut categories: Vec<String> = Vec::new();
    let mut annotation_id = 1u64;
    for (index, sample) in samples.iter().enumerate() {
        let file_name = format!("{}.png", sample.id);
        copy_image(
            Path::new(&sample.image_path),
            &target.join("images").join(&file_name),
        )?;
        let image_id = index as u64 + 1;
        images.push(serde_json::json!({
            "id": image_id,
            "file_name": file_name,
        }));
        for (kind, _) in &sample.masks {
            let name = kind.as_str().to_string();
            if !categories.contains(&name) {
                categories.push(name.clone());
            }
            let category_id = categories.iter().position(|item| *item == name).unwrap() as u64 + 1;
            annotations.push(serde_json::json!({
                "id": annotation_id,
                "image_id": image_id,
                "category_id": category_id,
                "category": name,
                "mask": format!("masks/{}_{}.png", sample.id, name),
            }));
            annotation_id += 1;
        }
        for (kind, path) in &sample.masks {
            copy_image(
                Path::new(path),
                &target
                    .join("masks")
                    .join(format!("{}_{}.png", sample.id, kind.as_str())),
            )?;
        }
    }
    let manifest = serde_json::json!({
        "images": images,
        "annotations": annotations,
        "categories": categories,
    });
    let path = target.join("annotations.json");
    std::fs::write(&path, serde_json::to_string_pretty(&manifest)?)?;
    Ok(images.len() + annotations.len() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_engine_returns_copy() {
        let image = Image::new(2, 2);
        let mask = SmartMask::zeros(SmartMaskKind::Skin, 2, 2);
        let engine = NoopRetouchEngine;
        let out = engine
            .apply(&image, &mask, &RetouchParams::default())
            .unwrap();
        assert_eq!(out.dimensions(), image.dimensions());
    }

    #[test]
    fn recorder_writes_jsonl_and_mask_png() {
        let dir =
            std::env::temp_dir().join(format!("fdxmp-smartmask-recorder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut recorder = JsonlRecorder::open(&dir).unwrap();
        let sample = TrainingSample {
            id: "sample-1".to_string(),
            image_path: dir.join("image.png").display().to_string(),
            masks: vec![(
                SmartMaskKind::Skin,
                dir.join("mask_skin.png").display().to_string(),
            )],
            model: "mock".to_string(),
            model_version: "1.0.0".to_string(),
            params_json: "{}".to_string(),
            created_at: 1,
        };
        recorder.record(sample).unwrap();
        assert_eq!(recorder.len(), 1);
        let content = std::fs::read_to_string(dir.join("samples.jsonl")).unwrap();
        assert!(content.contains("sample-1"));
        assert!(content.contains("skin"));

        let mut mask = SmartMask::zeros(SmartMaskKind::Skin, 2, 2);
        mask.data[0] = 1.0;
        recorder
            .save_mask_png(&mask, &dir.join("mask.png"))
            .unwrap();
        let saved = image::open(dir.join("mask.png")).unwrap();
        assert_eq!(saved.width(), 2);
        assert_eq!(saved.height(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_coco_layout_writes_manifest() {
        let dir = std::env::temp_dir().join(format!("fdxmp-smartmask-coco-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let image_path = dir.join("image.png");
        let mask_path = dir.join("mask.png");
        std::fs::write(&image_path, b"png").unwrap();
        std::fs::write(&mask_path, b"png").unwrap();
        let samples = vec![TrainingSample {
            id: "a".to_string(),
            image_path: image_path.display().to_string(),
            masks: vec![(SmartMaskKind::Face, mask_path.display().to_string())],
            model: "mock".to_string(),
            model_version: "1".to_string(),
            params_json: "{}".to_string(),
            created_at: 2,
        }];
        let target = dir.join("out");
        let written = export_dataset(&target, DatasetLayout::Coco, &samples).unwrap();
        assert!(written >= 2);
        let manifest = std::fs::read_to_string(target.join("annotations.json")).unwrap();
        assert!(manifest.contains("\"face\""));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
