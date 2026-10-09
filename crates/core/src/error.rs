//! 统一错误类型。

use thiserror::Error;

/// 核心层统一 `Result`。
pub type Result<T> = std::result::Result<T, FdxmpError>;

/// fdxmp 核心错误。
#[derive(Debug, Error)]
pub enum FdxmpError {
    /// 文件 / 目录 IO 错误。
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON 序列化错误。
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    /// .fdxmp（ZIP）归档错误。
    #[error("archive error: {0}")]
    Zip(#[from] zip::result::ZipError),
    /// 常见格式解码错误。
    #[error("image decode error: {0}")]
    Image(#[from] image::ImageError),
    /// 预设内容不符合规范。
    #[error("preset error: {0}")]
    Preset(String),
    /// 当前构建不支持该格式。
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// 未启用 `raw` feature（rsraw）。
    #[error("raw decoding requires the `raw` feature (rsraw/LibRaw): {0}")]
    RawUnavailable(String),
    /// RAW 解码失败（文件损坏 / 相机不支持）。
    #[error("raw decode error: {0}")]
    Raw(String),
    /// 未启用 `heif` feature（libheif-rs）。
    #[error("heif decoding requires the `heif` feature (libheif-rs): {0}")]
    HeifUnavailable(String),
    /// HEIF 解码失败。
    #[error("heif decode error: {0}")]
    Heif(String),
    /// GPU 初始化 / 运行错误。
    #[error("gpu error: {0}")]
    Gpu(String),
    /// 参数非法。
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}
