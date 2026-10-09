//! 缓存管理：缩略图 / 预览 / AI 模型。
//!
//! 「清除缓存」是「我的」Tab 的核心功能，这里提供大小统计与清除实现。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// 缓存类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheKind {
    /// 缩略图缓存。
    Thumbnails,
    /// 预览缓存。
    Previews,
    /// AI 模型缓存。
    AiModels,
}

impl CacheKind {
    /// 全部类别。
    pub const ALL: [CacheKind; 3] = [
        CacheKind::Thumbnails,
        CacheKind::Previews,
        CacheKind::AiModels,
    ];

    /// 子目录名。
    pub fn dir_name(self) -> &'static str {
        match self {
            CacheKind::Thumbnails => "thumbnails",
            CacheKind::Previews => "previews",
            CacheKind::AiModels => "ai_models",
        }
    }

    /// 中文标签。
    pub fn label_zh(self) -> &'static str {
        match self {
            CacheKind::Thumbnails => "缩略图缓存",
            CacheKind::Previews => "预览缓存",
            CacheKind::AiModels => "AI 模型缓存",
        }
    }
}

/// 缓存目录管理器。
#[derive(Debug, Clone)]
pub struct CacheManager {
    root: PathBuf,
}

impl CacheManager {
    /// 指定缓存根目录。
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 缓存根目录。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 某个类别的目录。
    pub fn dir_for(&self, kind: CacheKind) -> PathBuf {
        self.root.join(kind.dir_name())
    }

    /// 创建全部缓存目录。
    pub fn ensure_dirs(&self) -> Result<()> {
        for kind in CacheKind::ALL {
            std::fs::create_dir_all(self.dir_for(kind))?;
        }
        Ok(())
    }

    /// 某个类别当前占用字节数。
    pub fn size_of(&self, kind: CacheKind) -> Result<u64> {
        dir_size(&self.dir_for(kind))
    }

    /// 全部缓存占用字节数。
    pub fn total_size(&self) -> Result<u64> {
        let mut total = 0;
        for kind in CacheKind::ALL {
            total += self.size_of(kind)?;
        }
        Ok(total)
    }

    /// 清除某个类别，返回释放的字节数。
    pub fn clear(&self, kind: CacheKind) -> Result<u64> {
        let dir = self.dir_for(kind);
        let freed = dir_size(&dir)?;
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        Ok(freed)
    }

    /// 清除全部缓存，返回释放的字节数。
    pub fn clear_all(&self) -> Result<u64> {
        let mut freed = 0;
        for kind in CacheKind::ALL {
            freed += self.clear(kind)?;
        }
        Ok(freed)
    }

    /// 人类可读大小（B / KB / MB / GB）。
    pub fn human_size(bytes: u64) -> String {
        const KB: f64 = 1024.0;
        const MB: f64 = KB * 1024.0;
        const GB: f64 = MB * 1024.0;
        let value = bytes as f64;
        if value >= GB {
            format!("{:.2} GB", value / GB)
        } else if value >= MB {
            format!("{:.1} MB", value / MB)
        } else if value >= KB {
            format!("{:.0} KB", value / KB)
        } else {
            format!("{bytes} B")
        }
    }

    /// 缩略图缓存文件路径。
    ///
    /// key = FNV-1a(规范化路径 + 文件大小 + 修改时间)，源文件变化时自动失效。
    pub fn thumbnail_path(&self, source: &Path) -> PathBuf {
        let canonical = std::fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
        let metadata = std::fs::metadata(source).ok();
        let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
        let modified = metadata
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);

        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut feed = |bytes: &[u8]| {
            for byte in bytes {
                hash ^= *byte as u64;
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        feed(canonical.to_string_lossy().as_bytes());
        feed(&size.to_le_bytes());
        feed(&modified.to_le_bytes());

        self.root
            .join(CacheKind::Thumbnails.dir_name())
            .join(format!("{hash:016x}.png"))
    }
}

/// 递归统计目录大小（不存在的目录计 0）。
pub fn dir_size(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                stack.push(entry.path());
            } else {
                total += metadata.len();
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let unique = format!(
            "fdxmp-test-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn size_and_clear_roundtrip() {
        let root = temp_root("cache");
        let cache = CacheManager::new(&root);
        cache.ensure_dirs().unwrap();
        std::fs::write(
            cache.dir_for(CacheKind::Thumbnails).join("a.bin"),
            b"1234567",
        )
        .unwrap();
        std::fs::write(cache.dir_for(CacheKind::Previews).join("b.bin"), b"123").unwrap();

        assert_eq!(cache.size_of(CacheKind::Thumbnails).unwrap(), 7);
        assert_eq!(cache.total_size().unwrap(), 10);
        assert_eq!(cache.clear_all().unwrap(), 10);
        assert_eq!(cache.total_size().unwrap(), 0);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn human_size_formats() {
        assert_eq!(CacheManager::human_size(512), "512 B");
        assert_eq!(CacheManager::human_size(2048), "2 KB");
        assert_eq!(CacheManager::human_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn thumbnail_path_is_stable_and_versioned() {
        let root = temp_root("thumb");
        let cache = CacheManager::new(&root);
        let source = root.join("photo.jpg");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&source, b"v1").unwrap();

        let first = cache.thumbnail_path(&source);
        assert_eq!(
            first,
            cache.thumbnail_path(&source),
            "同一文件应得到相同 key"
        );
        assert!(first.to_string_lossy().ends_with(".png"));

        std::fs::write(&source, b"v2-longer").unwrap();
        assert_ne!(
            first,
            cache.thumbnail_path(&source),
            "文件变化后 key 必须失效"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
