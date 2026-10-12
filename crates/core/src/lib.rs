//! # fdxmp-core
//!
//! 图像处理核心：解码、色彩管线、蒙版、预设（.fdxmp）、缓存与历史记录。
//!
//! 本 crate 不依赖任何 UI 框架，四端（Windows / Android / HarmonyOS / iOS）
//! 通过 `fdxmp-ffi` 暴露的 C ABI 或各平台桥接层调用。

// 默认拒绝 unsafe；零拷贝上屏（gpu feature 的 apple 目标）需要极少量 FFI/图形 API
// 裸指针操作，在对应项上单独 `#[allow(unsafe_code)]` 并写明 Safety 注释。
#![deny(unsafe_code)]
#![warn(missing_docs)]

#[cfg(feature = "ai")]
pub mod ai;
pub mod cache;
pub mod color;
pub mod decode;
pub mod error;
pub mod export;
pub mod geometry;
pub mod histogram;
pub mod history;
pub mod lens_db;
pub mod masks;
pub mod params;
pub mod pipeline;
pub mod preset;
pub mod preview;
pub mod smartmask;
pub mod white_balance;

#[cfg(feature = "gpu")]
pub mod gpu;

#[cfg(feature = "gpu")]
pub mod gpu_pipeline;

use std::path::PathBuf;

use cache::CacheManager;
use error::Result;
use history::History;
use params::EditParams;

/// 核心语义版本号（与 crate 版本一致）。
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// C ABI 版本号：任何破坏性 ABI 变更都必须递增。
pub const ABI_VERSION: u32 = 1;

/// 返回核心版本号。
pub fn version() -> &'static str {
    CORE_VERSION
}

/// 缓存根目录：优先 `FDXMP_CACHE_DIR`，其次平台默认缓存路径。
pub fn default_cache_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("FDXMP_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("fdxmp").join("cache");
        }
    }
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(xdg).join("fdxmp");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".cache").join("fdxmp");
    }
    std::env::temp_dir().join("fdxmp-cache")
}

/// 引擎：持有当前编辑参数、历史记录与缓存管理器。
///
/// FFI 层的 `fdxmp_engine_*` 函数围绕该对象工作。
#[derive(Debug)]
pub struct Engine {
    params: EditParams,
    history: History,
    cache: CacheManager,
}

impl Engine {
    /// 使用默认缓存目录创建引擎。
    pub fn new() -> Self {
        Self::with_cache_root(default_cache_root())
    }

    /// 使用指定缓存目录创建引擎。
    pub fn with_cache_root(root: impl Into<PathBuf>) -> Self {
        let params = EditParams::default();
        let history = History::new("打开图片", params.clone());
        Self {
            params,
            history,
            cache: CacheManager::new(root),
        }
    }

    /// 当前编辑参数。
    pub fn params(&self) -> &EditParams {
        &self.params
    }

    /// 写入一组新的编辑参数并记录历史。
    pub fn apply(&mut self, label: impl Into<String>, params: EditParams) -> &EditParams {
        self.params = params;
        self.history.push(label, self.params.clone());
        &self.params
    }

    /// 实时调整参数（不写历史）。
    ///
    /// 滑块拖动过程中调用，配合 [`Engine::commit_history`] 在松手时记录一步，
    /// 避免一次拖动产生几十条历史。
    pub fn set_params_live(&mut self, params: EditParams) {
        self.params = params;
    }

    /// 把当前参数写入历史（交互结束时调用）。
    pub fn commit_history(&mut self, label: impl Into<String>) {
        let params = self.params.clone();
        self.history.push(label, params);
    }

    /// 创建历史快照（历史面板可跳转；不改变当前参数）。
    pub fn snapshot(&mut self, label: impl Into<String>) {
        let params = self.params.clone();
        self.history.snapshot(label, params);
    }

    /// 当前编辑参数的 JSON 表示。
    pub fn params_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&self.params)?)
    }

    /// 从 JSON 载入编辑参数并记录历史。
    pub fn apply_json(&mut self, label: impl Into<String>, json: &str) -> Result<()> {
        let params: EditParams = serde_json::from_str(json)?;
        self.apply(label, params);
        Ok(())
    }

    /// 撤销，返回是否发生变化。
    pub fn undo(&mut self) -> bool {
        match self.history.undo() {
            Some(params) => {
                self.params = params.clone();
                true
            }
            None => false,
        }
    }

    /// 重做，返回是否发生变化。
    pub fn redo(&mut self) -> bool {
        match self.history.redo() {
            Some(params) => {
                self.params = params.clone();
                true
            }
            None => false,
        }
    }

    /// 跳转到指定历史步骤（历史面板点击跳转），返回是否发生变化。
    pub fn jump_to(&mut self, index: usize) -> bool {
        match self.history.jump_to(index) {
            Some(params) => {
                self.params = params.clone();
                true
            }
            None => false,
        }
    }

    /// 历史记录只读访问。
    pub fn history(&self) -> &History {
        &self.history
    }

    /// 缓存管理器只读访问。
    pub fn cache(&self) -> &CacheManager {
        &self.cache
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_json_roundtrip_and_undo() {
        let mut engine = Engine::new();
        let mut params = engine.params().clone();
        params.light.exposure = 1.5;
        engine
            .apply_json("曝光", &serde_json::to_string(&params).unwrap())
            .unwrap();
        assert_eq!(engine.params().light.exposure, 1.5);

        assert!(engine.undo());
        assert_eq!(engine.params().light.exposure, 0.0);
        assert!(engine.redo());
        assert_eq!(engine.params().light.exposure, 1.5);
    }

    #[test]
    fn version_is_exposed() {
        assert!(!version().is_empty());
        let abi = std::hint::black_box(ABI_VERSION);
        assert!(abi >= 1);
    }

    #[test]
    fn engine_jump_to_history_step() {
        let mut engine = Engine::new();
        let mut params = engine.params().clone();
        params.light.contrast = 30.0;
        engine.apply("对比度", params);
        assert_eq!(engine.history().index(), 1);
        assert!(engine.jump_to(0));
        assert_eq!(engine.params().light.contrast, 0.0);
        assert!(!engine.jump_to(99));
    }

    #[test]
    fn live_edit_defers_history_until_commit() {
        let mut engine = Engine::new();
        let mut params = engine.params().clone();
        for step in 1..=5 {
            params.light.exposure = step as f32 * 0.1;
            engine.set_params_live(params.clone());
        }
        assert_eq!(engine.history().len(), 1, "拖动过程不写历史");
        assert!((engine.params().light.exposure - 0.5).abs() < 1e-6);

        engine.commit_history("曝光");
        assert_eq!(engine.history().len(), 2, "松手后只记一步");
        assert!(engine.undo());
        assert_eq!(engine.params().light.exposure, 0.0);
    }
}
