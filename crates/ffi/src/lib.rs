//! # fdxmp-ffi
//!
//! 四端共用的 C ABI 导出层。约定：
//!
//! - 所有指针参数在为空时返回错误（写入 thread-local 错误信息），不会崩溃；
//! - Rust panic 在边界处被捕获，转换为错误返回值；
//! - 返回的字符串 / 字节缓冲区必须用 `fdxmp_string_free` / `fdxmp_bytes_free` 释放；
//! - `fdxmp_version` 返回静态字符串，**不要**释放；
//! - ABI 版本由 [`fdxmp_abi_version`] 返回，破坏性变更时递增。
//!
//! 对应头文件：`crates/ffi/include/fdxmp.h`。

use std::any::Any;
use std::cell::RefCell;
use std::ffi::{c_char, c_void, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::sync::OnceLock;

use fdxmp_core::cache::CacheManager;
use fdxmp_core::preset::PresetFile;
use fdxmp_core::Engine;

/// 错误返回值：通用失败。
pub const FDXMP_ERR: i32 = -1;
/// 错误返回值：成功。
pub const FDXMP_OK: i32 = 0;
/// u64 错误哨兵值。
pub const FDXMP_U64_ERR: u64 = u64::MAX;

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

fn clear_last_error() {
    LAST_ERROR.with(|slot| *slot.borrow_mut() = None);
}

fn set_last_error(message: impl Into<String>) {
    let sanitized = message.into().replace('\0', " ");
    LAST_ERROR.with(|slot| *slot.borrow_mut() = CString::new(sanitized).ok());
}

fn panic_payload_to_string(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "panic in fdxmp ffi boundary".to_string()
    }
}

fn guard<R>(fallback: R, body: impl FnOnce() -> R) -> R {
    clear_last_error();
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(payload) => {
            set_last_error(panic_payload_to_string(&*payload));
            fallback
        }
    }
}

fn version_cstring() -> &'static CString {
    static VERSION: OnceLock<CString> = OnceLock::new();
    VERSION.get_or_init(|| CString::new(fdxmp_core::version()).unwrap_or_default())
}

fn into_c_string(value: String) -> *mut c_char {
    match CString::new(value) {
        Ok(text) => text.into_raw(),
        Err(_) => {
            set_last_error("内部错误：返回值包含 NUL 字符");
            ptr::null_mut()
        }
    }
}

/// # Safety
///
/// `ptr` 必须是以 NUL 结尾的合法 C 字符串，或为 NULL。
unsafe fn cstr_to_str<'a>(ptr: *const c_char, name: &str) -> std::result::Result<&'a str, String> {
    if ptr.is_null() {
        return Err(format!("参数 `{name}` 为空指针"));
    }
    CStr::from_ptr(ptr)
        .to_str()
        .map_err(|err| format!("参数 `{name}` 不是合法 UTF-8：{err}"))
}

/// # Safety
///
/// `ptr` 必须指向长度为 `len` 的连续字节，或为 NULL（此时返回 `None`）。
unsafe fn bytes_from_raw<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if ptr.is_null() {
        None
    } else {
        Some(std::slice::from_raw_parts(ptr, len))
    }
}

/// 返回 ABI 版本号。
#[no_mangle]
pub extern "C" fn fdxmp_abi_version() -> u32 {
    fdxmp_core::ABI_VERSION
}

/// 返回核心版本号（静态字符串，不要释放）。
#[no_mangle]
pub extern "C" fn fdxmp_version() -> *const c_char {
    version_cstring().as_ptr()
}

/// 8bit sRGB → OkLCh（`out` 指向至少 3 个 float：L、C、色相弧度）。
///
/// "点颜色"取色器用：样本直接是 `PointColorParams` 的 `lum` / `chroma` / `hue`，
/// 与调色段共用同一套色彩矩阵（四端一致）。
///
/// # Safety
///
/// `out` 必须指向可写、长度 ≥ 3 的 `float` 数组；为 NULL 时返回错误。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_color_oklch_from_srgb8(r: u8, g: u8, b: u8, out: *mut f32) -> i32 {
    if out.is_null() {
        set_last_error("参数 `out` 为空指针");
        return FDXMP_ERR;
    }
    let lch = fdxmp_core::color_engine::oklch_from_srgb8(r, g, b);
    // SAFETY: 调用方保证 out 指向 ≥3 个 float 的可写数组（见函数文档）。
    std::ptr::copy_nonoverlapping(lch.as_ptr(), out, 3);
    FDXMP_OK
}

/// 颜色均衡器曲线：把 8 个节点 + 选项算成 512 点 LUT（与渲染同一份算法）。
///
/// `channel`：0 色相（弧度）/ 1 饱和度增益 / 2 亮度增益。`params_json` 是整份
/// 渲染参数，只读其中的 `equalizer` 字段（缺失按默认值）。
///
/// # Safety
///
/// `params_json` 必须是有效的 UTF-8 C 字符串，`out` 必须指向可写、长度 ≥ 512 的
/// `float` 数组；任一为 NULL 时返回错误。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_equalizer_curve(
    channel: u32,
    params_json: *const c_char,
    out: *mut f32,
) -> i32 {
    if out.is_null() || params_json.is_null() {
        set_last_error("参数为空指针");
        return FDXMP_ERR;
    }
    // SAFETY: 调用方保证是有效 C 字符串（见函数文档）。
    let json = match CStr::from_ptr(params_json).to_str() {
        Ok(text) => text,
        Err(_) => {
            set_last_error("参数 JSON 不是有效的 UTF-8");
            return FDXMP_ERR;
        }
    };
    let root: serde_json::Value = match serde_json::from_str(json) {
        Ok(value) => value,
        Err(error) => {
            set_last_error(format!("参数 JSON 解析失败：{error}"));
            return FDXMP_ERR;
        }
    };
    let params: fdxmp_core::params::EqualizerParams = root
        .get("equalizer")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    let channel = match channel {
        0 => fdxmp_core::Channel::Hue,
        1 => fdxmp_core::Channel::Saturation,
        _ => fdxmp_core::Channel::Brightness,
    };
    let lut = fdxmp_core::equalizer_curve(&params, channel);
    // SAFETY: 调用方保证 out 指向 ≥512 个 float 的可写数组（见函数文档）。
    std::ptr::copy_nonoverlapping(lut.as_ptr(), out, lut.len());
    FDXMP_OK
}

/// 返回最近一次错误的 UTF-8 消息（调用方用 `fdxmp_string_free` 释放）；无错误返回 NULL。
#[no_mangle]
pub extern "C" fn fdxmp_last_error_message() -> *mut c_char {
    LAST_ERROR.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|text| text.clone().into_raw())
            .unwrap_or(ptr::null_mut())
    })
}

/// 释放由本库返回的字符串。
///
/// # Safety
///
/// `ptr` 必须是本库返回、且尚未释放的字符串指针，或为 NULL。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

/// 释放由本库返回的字节缓冲区。
///
/// # Safety
///
/// `ptr`/`len` 必须来自本库的字节缓冲区返回值，且未被释放。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_bytes_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(Vec::from_raw_parts(ptr, len, len));
    }
}

/// 创建引擎实例，失败返回 NULL。
#[no_mangle]
pub extern "C" fn fdxmp_engine_new() -> *mut c_void {
    guard(ptr::null_mut(), || {
        Box::into_raw(Box::new(Engine::new())).cast::<c_void>()
    })
}

/// 使用指定缓存根目录创建引擎实例，失败返回 NULL。
///
/// # Safety
///
/// `cache_root` 必须是以 NUL 结尾的合法 UTF-8 字符串，或为 NULL。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_engine_new_with_cache_root(
    cache_root: *const c_char,
) -> *mut c_void {
    guard(ptr::null_mut(), || {
        let root = match cstr_to_str(cache_root, "cache_root") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        Box::into_raw(Box::new(Engine::with_cache_root(root))).cast::<c_void>()
    })
}

/// 释放引擎实例。
///
/// # Safety
///
/// `engine` 必须来自 `fdxmp_engine_new*` 且尚未释放，或为 NULL。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_engine_free(engine: *mut c_void) {
    if !engine.is_null() {
        drop(Box::from_raw(engine as *mut Engine));
    }
}

/// 返回当前参数的 JSON 字符串（调用方释放），失败返回 NULL。
///
/// # Safety
///
/// `engine` 必须来自 `fdxmp_engine_new*` 且仍然有效。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_engine_params_json(engine: *const c_void) -> *mut c_char {
    guard(ptr::null_mut(), || {
        let Some(engine) = (engine as *const Engine).as_ref() else {
            set_last_error("engine 为空指针");
            return ptr::null_mut();
        };
        match engine.params_json() {
            Ok(json) => into_c_string(json),
            Err(err) => {
                set_last_error(err.to_string());
                ptr::null_mut()
            }
        }
    })
}

/// 应用一组参数 JSON，并写入历史记录。返回 0 成功 / -1 失败。
///
/// # Safety
///
/// `engine` 必须有效；`label`、`json` 必须为合法 C 字符串。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_engine_apply_params_json(
    engine: *mut c_void,
    label: *const c_char,
    json: *const c_char,
) -> i32 {
    guard(FDXMP_ERR, || {
        let Some(engine) = (engine as *mut Engine).as_mut() else {
            set_last_error("engine 为空指针");
            return FDXMP_ERR;
        };
        let label = match cstr_to_str(label, "label") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return FDXMP_ERR;
            }
        };
        let json = match cstr_to_str(json, "json") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return FDXMP_ERR;
            }
        };
        match engine.apply_json(label, json) {
            Ok(()) => FDXMP_OK,
            Err(err) => {
                set_last_error(err.to_string());
                FDXMP_ERR
            }
        }
    })
}

/// 撤销；返回 1 表示参数已变化，0 表示没有可撤销步骤。
///
/// # Safety
///
/// `engine` 必须来自 `fdxmp_engine_new*` 且仍然有效。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_engine_undo(engine: *mut c_void) -> i32 {
    guard(0, || {
        let Some(engine) = (engine as *mut Engine).as_mut() else {
            set_last_error("engine 为空指针");
            return 0;
        };
        i32::from(engine.undo())
    })
}

/// 重做；返回 1 表示参数已变化，0 表示没有可重做步骤。
///
/// # Safety
///
/// `engine` 必须来自 `fdxmp_engine_new*` 且仍然有效。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_engine_redo(engine: *mut c_void) -> i32 {
    guard(0, || {
        let Some(engine) = (engine as *mut Engine).as_mut() else {
            set_last_error("engine 为空指针");
            return 0;
        };
        i32::from(engine.redo())
    })
}

/// 历史步骤数量。
///
/// # Safety
///
/// `engine` 必须来自 `fdxmp_engine_new*` 且仍然有效。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_engine_history_len(engine: *const c_void) -> usize {
    guard(0, || match (engine as *const Engine).as_ref() {
        Some(engine) => engine.history().len(),
        None => {
            set_last_error("engine 为空指针");
            0
        }
    })
}

/// 当前历史下标。
///
/// # Safety
///
/// `engine` 必须来自 `fdxmp_engine_new*` 且仍然有效。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_engine_history_index(engine: *const c_void) -> usize {
    guard(0, || match (engine as *const Engine).as_ref() {
        Some(engine) => engine.history().index(),
        None => {
            set_last_error("engine 为空指针");
            0
        }
    })
}

/// 从 `.fdxmp` 字节流读取预设，返回 params JSON（调用方释放），失败返回 NULL。
///
/// # Safety
///
/// `bytes`/`len` 必须描述一段有效字节，或 `bytes` 为 NULL。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_preset_params_json_from_bytes(
    bytes: *const u8,
    len: usize,
) -> *mut c_char {
    guard(ptr::null_mut(), || {
        let Some(bytes) = bytes_from_raw(bytes, len) else {
            set_last_error("bytes 为空指针");
            return ptr::null_mut();
        };
        match PresetFile::from_bytes(bytes)
            .and_then(|preset| Ok(serde_json::to_string(&preset.params)?))
        {
            Ok(json) => into_c_string(json),
            Err(err) => {
                set_last_error(err.to_string());
                ptr::null_mut()
            }
        }
    })
}

/// 由 params JSON 打包 `.fdxmp` 字节流；返回缓冲区指针并写入 `out_len`，失败返回 NULL。
///
/// # Safety
///
/// `json`、`name`、`author` 必须为合法 C 字符串；`out_len` 必须为非空可写指针。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_preset_bytes_from_params_json(
    json: *const c_char,
    name: *const c_char,
    author: *const c_char,
    out_len: *mut usize,
) -> *mut u8 {
    guard(ptr::null_mut(), || {
        if out_len.is_null() {
            set_last_error("out_len 为空指针");
            return ptr::null_mut();
        }
        let json = match cstr_to_str(json, "json") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        let name = match cstr_to_str(name, "name") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        let author = match cstr_to_str(author, "author") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };

        let result = serde_json::from_str(json)
            .map_err(fdxmp_core::error::FdxmpError::from)
            .and_then(|params| PresetFile::new(name, author, params).to_bytes());
        match result {
            Ok(bytes) => {
                let mut boxed = bytes.into_boxed_slice();
                *out_len = boxed.len();
                let pointer = boxed.as_mut_ptr();
                std::mem::forget(boxed);
                pointer
            }
            Err(err) => {
                set_last_error(err.to_string());
                ptr::null_mut()
            }
        }
    })
}

/// 解析 `.fdxmp` 字节流，返回预设 JSON（`name` / `author` / `description` / `params`），
/// 失败返回 NULL（调用方释放返回值）。
///
/// # Safety
///
/// `bytes`/`len` 必须描述一段有效字节，或 `bytes` 为 NULL。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_preset_json_from_bytes(bytes: *const u8, len: usize) -> *mut c_char {
    guard(ptr::null_mut(), || {
        let Some(bytes) = bytes_from_raw(bytes, len) else {
            set_last_error("bytes 为空指针");
            return ptr::null_mut();
        };
        match PresetFile::from_bytes(bytes) {
            Ok(preset) => {
                let payload = serde_json::json!({
                    "name": preset.name,
                    "author": preset.author,
                    "description": preset.description,
                    "params": preset.params,
                });
                into_c_string(payload.to_string())
            }
            Err(err) => {
                set_last_error(err.to_string());
                ptr::null_mut()
            }
        }
    })
}

/// 解析 Lightroom `.xmp` 预设文本，返回预设 JSON（`name` / `author` / `params`），
/// 失败返回 NULL（调用方释放返回值）。
///
/// # Safety
///
/// `xmp` 必须为合法 C 字符串。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_preset_json_from_xmp(xmp: *const c_char) -> *mut c_char {
    guard(ptr::null_mut(), || {
        let text = match cstr_to_str(xmp, "xmp") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        match PresetFile::from_lightroom_xmp(text) {
            Ok(preset) => {
                let payload = serde_json::json!({
                    "name": preset.name,
                    "author": preset.author,
                    "params": preset.params,
                });
                into_c_string(payload.to_string())
            }
            Err(err) => {
                set_last_error(err.to_string());
                ptr::null_mut()
            }
        }
    })
}

/// 探测图像格式与尺寸，返回 JSON（调用方释放），失败返回 NULL。
///
/// # Safety
///
/// `path` 必须为合法 C 字符串。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_probe_json(path: *const c_char) -> *mut c_char {
    guard(ptr::null_mut(), || {
        let path = match cstr_to_str(path, "path") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        match fdxmp_core::decode::probe(path).and_then(|probe| Ok(serde_json::to_string(&probe)?)) {
            Ok(json) => into_c_string(json),
            Err(err) => {
                set_last_error(err.to_string());
                ptr::null_mut()
            }
        }
    })
}

/// 解码图像并按 `params_json` 渲染预览，返回 RGBA8 像素缓冲区
/// （调用方用 `fdxmp_bytes_free(ptr, len)` 释放）；失败返回 NULL。
///
/// 缓冲区前 8 字节为大端 `width` / `height`，其后为行优先 RGBA8 像素。
/// `params_json` 为 `EditParams` 的 JSON（缺省字段自动回落默认值），
/// `max_edge` 为预览最长边（会限制在 64..=4096）。
///
/// # Safety
///
/// `path`、`params_json` 必须为合法 C 字符串；`out_width`、`out_height`、
/// `out_len` 必须为非空可写指针，且未被释放。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_render_preview_rgba(
    path: *const c_char,
    params_json: *const c_char,
    max_edge: u32,
    out_width: *mut u32,
    out_height: *mut u32,
    out_len: *mut usize,
) -> *mut u8 {
    guard(ptr::null_mut(), || {
        if out_width.is_null() || out_height.is_null() || out_len.is_null() {
            set_last_error("out_width / out_height / out_len 不能为空指针");
            return ptr::null_mut();
        }
        let path = match cstr_to_str(path, "path") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        let params_json = match cstr_to_str(params_json, "params_json") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        match fdxmp_core::preview::render_preview_rgba(
            std::path::Path::new(path),
            params_json,
            max_edge,
        ) {
            Ok(preview) => {
                let mut payload = Vec::with_capacity(8 + preview.rgba.len());
                payload.extend_from_slice(&preview.width.to_be_bytes());
                payload.extend_from_slice(&preview.height.to_be_bytes());
                payload.extend_from_slice(&preview.rgba);
                *out_width = preview.width;
                *out_height = preview.height;
                let mut boxed = payload.into_boxed_slice();
                *out_len = boxed.len();
                let pointer = boxed.as_mut_ptr();
                std::mem::forget(boxed);
                pointer
            }
            Err(err) => {
                set_last_error(err.to_string());
                ptr::null_mut()
            }
        }
    })
}

/// 从内存字节解码相机 RAW（LibRaw / rsraw）并返回 RGBA8 缓冲区
/// （前 8 字节大端 width / height，其后行优先 RGBA8；用 `fdxmp_bytes_free` 释放）。
///
/// `full_size = 0` 走半尺寸解调（预览）；非 0 走全尺寸解调（导出）。
/// 输出按 `max_edge` 等比缩小（不放大）。未启用 `raw` feature 时返回 NULL 并给出提示。
///
/// # Safety
///
/// `bytes` 必须指向至少 `len` 字节的可读缓冲区；`out_width`、`out_height`、
/// `out_len` 必须为非空可写指针。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_decode_raw_rgba8(
    bytes: *const u8,
    len: usize,
    max_edge: u32,
    full_size: u32,
    out_width: *mut u32,
    out_height: *mut u32,
    out_len: *mut usize,
) -> *mut u8 {
    guard(ptr::null_mut(), || {
        if bytes.is_null() || out_width.is_null() || out_height.is_null() || out_len.is_null() {
            set_last_error("bytes / out_width / out_height / out_len 不能为空指针");
            return ptr::null_mut();
        }
        if len == 0 {
            set_last_error("RAW 数据为空");
            return ptr::null_mut();
        }
        // SAFETY: 调用方保证缓冲区至少 `len` 字节。
        let data = std::slice::from_raw_parts(bytes, len);
        match fdxmp_core::preview::decode_raw_rgba(data, max_edge, full_size != 0) {
            Ok(decoded) => {
                let mut payload = Vec::with_capacity(8 + decoded.rgba.len());
                payload.extend_from_slice(&decoded.width.to_be_bytes());
                payload.extend_from_slice(&decoded.height.to_be_bytes());
                payload.extend_from_slice(&decoded.rgba);
                *out_width = decoded.width;
                *out_height = decoded.height;
                let mut boxed = payload.into_boxed_slice();
                *out_len = boxed.len();
                let pointer = boxed.as_mut_ptr();
                std::mem::forget(boxed);
                pointer
            }
            Err(err) => {
                set_last_error(err.to_string());
                ptr::null_mut()
            }
        }
    })
}

/// 对调用方提供的内存 RGBA8 图像应用编辑参数，返回 RGBA8 像素缓冲区
/// （调用方用 `fdxmp_bytes_free(ptr, len)` 释放）；失败返回 NULL。
///
/// 用于图像已由系统解码的场景（如 iOS PhotoKit）：`rgba` 为行优先 RGBA8，
/// 长度至少 `width × height × 4`；返回格式与 `fdxmp_render_preview_rgba` 相同
/// （前 8 字节大端 `width` / `height`，其后为行优先 RGBA8 像素）。
///
/// # Safety
///
/// `rgba` 必须指向至少 `width * height * 4` 字节的可读缓冲区；
/// `params_json` 必须为合法 C 字符串；`out_width`、`out_height`、`out_len`
/// 必须为非空可写指针。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_render_memory_rgba(
    rgba: *const u8,
    width: u32,
    height: u32,
    params_json: *const c_char,
    out_width: *mut u32,
    out_height: *mut u32,
    out_len: *mut usize,
) -> *mut u8 {
    guard(ptr::null_mut(), || {
        if rgba.is_null() || out_width.is_null() || out_height.is_null() || out_len.is_null() {
            set_last_error("rgba / out_width / out_height / out_len 不能为空指针");
            return ptr::null_mut();
        }
        let params_json = match cstr_to_str(params_json, "params_json") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4));
        let Some(expected) = expected else {
            set_last_error("图像尺寸溢出");
            return ptr::null_mut();
        };
        // SAFETY: 调用方保证缓冲区至少 `width * height * 4` 字节。
        let buffer = std::slice::from_raw_parts(rgba, expected);
        match fdxmp_core::preview::render_memory_rgba(buffer, width, height, params_json) {
            Ok(preview) => {
                let mut payload = Vec::with_capacity(8 + preview.rgba.len());
                payload.extend_from_slice(&preview.width.to_be_bytes());
                payload.extend_from_slice(&preview.height.to_be_bytes());
                payload.extend_from_slice(&preview.rgba);
                *out_width = preview.width;
                *out_height = preview.height;
                let mut boxed = payload.into_boxed_slice();
                *out_len = boxed.len();
                let pointer = boxed.as_mut_ptr();
                std::mem::forget(boxed);
                pointer
            }
            Err(err) => {
                set_last_error(err.to_string());
                ptr::null_mut()
            }
        }
    })
}

/// 只渲染整帧的一块区域（放大 1:1 看细节；暗角 / 颗粒 / 蒙版按整帧坐标算）。
///
/// 入参是**区域像素**（区域 = 可见矩形向外扩余量后夹到整帧内），
/// 返回缓冲区格式同 `fdxmp_render_memory_rgba`（8 字节大端宽高 + RGBA8），
/// 内容为 `visible_*` 指定的可见矩形，必须用 `fdxmp_bytes_free` 释放。
/// 要求几何 / 镜头为单位变换、且蒙版不依赖图像内容，否则返回 NULL（调用方回退整帧渲染）。
///
/// # Safety
///
/// `rgba` 指向至少 `region_width * region_height * 4` 字节；`params_json` 为合法 C 字符串；
/// `out_width`、`out_height`、`out_len` 为非空可写指针。
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn fdxmp_render_region_rgba(
    rgba: *const u8,
    region_width: u32,
    region_height: u32,
    params_json: *const c_char,
    region_x: u32,
    region_y: u32,
    visible_x: u32,
    visible_y: u32,
    visible_width: u32,
    visible_height: u32,
    full_width: u32,
    full_height: u32,
    out_width: *mut u32,
    out_height: *mut u32,
    out_len: *mut usize,
) -> *mut u8 {
    guard(ptr::null_mut(), || {
        if rgba.is_null() || out_width.is_null() || out_height.is_null() || out_len.is_null() {
            set_last_error("rgba / out_width / out_height / out_len 不能为空指针");
            return ptr::null_mut();
        }
        let params_json = match cstr_to_str(params_json, "params_json") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        let expected = (region_width as usize)
            .checked_mul(region_height as usize)
            .and_then(|pixels| pixels.checked_mul(4));
        let Some(expected) = expected else {
            set_last_error("区域尺寸溢出");
            return ptr::null_mut();
        };
        // SAFETY: 调用方保证缓冲区至少 region_width * region_height * 4 字节。
        let buffer = std::slice::from_raw_parts(rgba, expected);
        match fdxmp_core::preview::render_memory_rgba_region(
            buffer,
            region_width,
            region_height,
            params_json,
            region_x,
            region_y,
            visible_x,
            visible_y,
            visible_width,
            visible_height,
            full_width,
            full_height,
        ) {
            Ok(preview) => {
                let mut payload = Vec::with_capacity(8 + preview.rgba.len());
                payload.extend_from_slice(&preview.width.to_be_bytes());
                payload.extend_from_slice(&preview.height.to_be_bytes());
                payload.extend_from_slice(&preview.rgba);
                *out_width = preview.width;
                *out_height = preview.height;
                let mut boxed = payload.into_boxed_slice();
                *out_len = boxed.len();
                let pointer = boxed.as_mut_ptr();
                std::mem::forget(boxed);
                pointer
            }
            Err(err) => {
                set_last_error(err.to_string());
                ptr::null_mut()
            }
        }
    })
}

/// 生成单个蒙版的叠加层（红色半透明，预乘 RGBA8），返回缓冲区
/// （调用方用 `fdxmp_bytes_free(ptr, len)` 释放）；失败返回 NULL。
///
/// 缓冲区前 8 字节为大端 `width` / `height`，其后为预乘 RGBA8 像素，
/// 可直接贴到预览图上（移动端蒙版编辑器的红色覆盖层 / 蒙版列表缩略图）。
/// 只支持几何型蒙版（画笔 / 线性 / 径向）；隐藏的蒙版返回全透明叠加。
///
/// # Safety
///
/// `params_json`、`mask_id` 必须为合法 C 字符串；`out_width`、`out_height`、
/// `out_len` 必须为非空可写指针，且未被释放。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_mask_overlay_rgba(
    params_json: *const c_char,
    mask_id: *const c_char,
    width: u32,
    height: u32,
    out_width: *mut u32,
    out_height: *mut u32,
    out_len: *mut usize,
) -> *mut u8 {
    guard(ptr::null_mut(), || {
        if out_width.is_null() || out_height.is_null() || out_len.is_null() {
            set_last_error("out_width / out_height / out_len 不能为空指针");
            return ptr::null_mut();
        }
        let params_json = match cstr_to_str(params_json, "params_json") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        let mask_id = match cstr_to_str(mask_id, "mask_id") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        match fdxmp_core::preview::render_mask_overlay_rgba(params_json, mask_id, width, height) {
            Ok(overlay) => {
                let mut payload = Vec::with_capacity(8 + overlay.rgba.len());
                payload.extend_from_slice(&overlay.width.to_be_bytes());
                payload.extend_from_slice(&overlay.height.to_be_bytes());
                payload.extend_from_slice(&overlay.rgba);
                *out_width = overlay.width;
                *out_height = overlay.height;
                let mut boxed = payload.into_boxed_slice();
                *out_len = boxed.len();
                let pointer = boxed.as_mut_ptr();
                std::mem::forget(boxed);
                pointer
            }
            Err(err) => {
                set_last_error(err.to_string());
                ptr::null_mut()
            }
        }
    })
}

// MARK: - 智能蒙版（v1.4.0）

/// 提交栅格的像素数上限（4096×4096，约 64 MB f32）。
const SMARTMASK_MAX_PIXELS: usize = 4096 * 4096;

/// 智能蒙版返回值：模型不可用 / 该类型不支持 —— 调用方应回退平台实现（如 iOS Vision）。
pub const FDXMP_SMARTMASK_FALLBACK: i32 = 2;

/// RGBA8 指针 + 尺寸 → 图像（统一做尺寸 / 溢出校验）。
///
/// # Safety
///
/// `rgba` 至少 `width × height × 4` 字节可读。
unsafe fn rgba_to_image(
    rgba: *const u8,
    width: u32,
    height: u32,
) -> std::result::Result<image::RgbaImage, String> {
    let expected = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    if width == 0 || height == 0 || width > 4096 || height > 4096 || rgba.is_null() || expected == 0
    {
        return Err(format!("图像尺寸非法：{width}×{height}（单边 ≤ 4096）"));
    }
    // SAFETY: 调用方保证至少 expected 字节可读。
    let buffer = std::slice::from_raw_parts(rgba, expected);
    image::RgbaImage::from_raw(width, height, buffer.to_vec())
        .ok_or_else(|| "RGBA 缓冲尺寸不匹配".to_string())
}

/// AI 修图识别（一键修图第一步）：蒙版 + 478 点五官关键点分析。
///
/// 返回 JSON：`{faces, landmarks, skin, person, warnings[]}`（调用方用
/// `fdxmp_string_free` 释放）；失败返回 NULL（原因见 `fdxmp_last_error_message`）。
///
/// # Safety
///
/// `rgba` 至少 `width × height × 4` 字节可读。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_retouch_analyze(
    rgba: *const u8,
    width: u32,
    height: u32,
) -> *mut c_char {
    guard(ptr::null_mut(), || {
        #[cfg(any(feature = "ai", feature = "ai-static"))]
        {
            let image = match rgba_to_image(rgba, width, height) {
                Ok(image) => image,
                Err(message) => {
                    set_last_error(message);
                    return ptr::null_mut();
                }
            };
            match fdxmp_core::smartmask::runtime::retouch_analyze(&image) {
                Ok(analysis) => match serde_json::to_string(&analysis) {
                    Ok(text) => into_c_string(text),
                    Err(err) => {
                        set_last_error(format!("序列化识别结果失败：{err}"));
                        ptr::null_mut()
                    }
                },
                Err(err) => {
                    set_last_error(err.message().to_string());
                    ptr::null_mut()
                }
            }
        }
        #[cfg(not(any(feature = "ai", feature = "ai-static")))]
        {
            let _ = (rgba, width, height);
            set_last_error("当前构建未启用 AI 修图（ai / ai-static）");
            ptr::null_mut()
        }
    })
}

/// AI 修图应用：按 `params_json`（RetouchParams）处理图像。
///
/// 返回与 `fdxmp_render_*` 相同的载荷（8 字节大端宽高 + RGBA8），
/// 调用方用 `fdxmp_bytes_free(ptr, len)` 释放；失败返回 NULL。
///
/// # Safety
///
/// `params_json` 必须为合法 C 字符串；`rgba` 至少 `width × height × 4` 字节可读；
/// `out_width` / `out_height` / `out_len` 为非空可写指针。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_retouch_apply(
    params_json: *const c_char,
    rgba: *const u8,
    width: u32,
    height: u32,
    out_width: *mut u32,
    out_height: *mut u32,
    out_len: *mut usize,
) -> *mut u8 {
    guard(ptr::null_mut(), || {
        if out_width.is_null() || out_height.is_null() || out_len.is_null() {
            set_last_error("out_width / out_height / out_len 不能为空指针");
            return ptr::null_mut();
        }
        let params_json = match cstr_to_str(params_json, "params_json") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        let params: fdxmp_core::retouch::RetouchParams = match serde_json::from_str(params_json) {
            Ok(params) => params,
            Err(err) => {
                set_last_error(format!("retouch params 解析失败：{err}"));
                return ptr::null_mut();
            }
        };
        #[cfg(any(feature = "ai", feature = "ai-static"))]
        {
            let image = match rgba_to_image(rgba, width, height) {
                Ok(image) => image,
                Err(message) => {
                    set_last_error(message);
                    return ptr::null_mut();
                }
            };
            match fdxmp_core::smartmask::runtime::retouch_apply(&image, &params) {
                Ok(output) => {
                    let mut payload = Vec::with_capacity(8 + output.as_raw().len());
                    payload.extend_from_slice(&output.width().to_be_bytes());
                    payload.extend_from_slice(&output.height().to_be_bytes());
                    payload.extend_from_slice(output.as_raw());
                    *out_width = output.width();
                    *out_height = output.height();
                    let mut boxed = payload.into_boxed_slice();
                    *out_len = boxed.len();
                    let pointer = boxed.as_mut_ptr();
                    std::mem::forget(boxed);
                    pointer
                }
                Err(err) => {
                    set_last_error(err.message().to_string());
                    ptr::null_mut()
                }
            }
        }
        #[cfg(not(any(feature = "ai", feature = "ai-static")))]
        {
            let _ = (params, rgba, width, height);
            set_last_error("当前构建未启用 AI 修图（ai / ai-static）");
            ptr::null_mut()
        }
    })
}

/// 打包图像为「8 字节大端宽高 + RGBA8」载荷（供上述接口返回）。
fn image_to_payload(
    image: &image::RgbaImage,
    out_width: *mut u32,
    out_height: *mut u32,
    out_len: *mut usize,
) -> *mut u8 {
    let mut payload = Vec::with_capacity(8 + image.as_raw().len());
    payload.extend_from_slice(&image.width().to_be_bytes());
    payload.extend_from_slice(&image.height().to_be_bytes());
    payload.extend_from_slice(image.as_raw());
    // SAFETY: 调用方（本文件内）已校验非空。
    unsafe {
        *out_width = image.width();
        *out_height = image.height();
    }
    let mut boxed = payload.into_boxed_slice();
    // SAFETY: 调用方（本文件内）已校验非空。
    unsafe {
        *out_len = boxed.len();
    }
    let pointer = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    pointer
}

/// 提交平台人像蒙版（iOS：Vision 人像分割 α，f32 0..1）——MODNet 不随包时补位。
///
/// 与图像内容（指纹）绑定；换图自动失效。返回 [`FDXMP_OK`] / [`FDXMP_ERR`]。
///
/// # Safety
///
/// `rgba` 至少 `width × height × 4` 字节可读；`plane` 至少 `plane_width × plane_height` 个 f32。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_retouch_submit_person_mask(
    rgba: *const u8,
    width: u32,
    height: u32,
    plane: *const f32,
    plane_width: u32,
    plane_height: u32,
) -> i32 {
    guard(FDXMP_ERR, || {
        #[cfg(any(feature = "ai", feature = "ai-static"))]
        {
            let image = match rgba_to_image(rgba, width, height) {
                Ok(image) => image,
                Err(message) => {
                    set_last_error(message);
                    return FDXMP_ERR;
                }
            };
            let expected = (plane_width as usize).saturating_mul(plane_height as usize);
            if plane.is_null() || expected == 0 || plane_width > 4096 || plane_height > 4096 {
                set_last_error("人像蒙版尺寸非法");
                return FDXMP_ERR;
            }
            // SAFETY: 调用方保证 plane 至少 expected 个 f32 可读。
            let weights = std::slice::from_raw_parts(plane, expected).to_vec();
            match fdxmp_core::smartmask::runtime::submit_platform_person_mask(
                &image,
                plane_width,
                plane_height,
                weights,
            ) {
                Ok(()) => FDXMP_OK,
                Err(err) => {
                    set_last_error(err.message().to_string());
                    FDXMP_ERR
                }
            }
        }
        #[cfg(not(any(feature = "ai", feature = "ai-static")))]
        {
            let _ = (rgba, width, height, plane, plane_width, plane_height);
            set_last_error("当前构建未启用智能蒙版（ai / ai-static）");
            FDXMP_ERR
        }
    })
}

/// 污点修复（手动祛痘笔）：`x` / `y` 归一化坐标，`radius` 像素，`strength` 0..1。
///
/// 返回修好的图像载荷（8 字节大端宽高 + RGBA8，用 `fdxmp_bytes_free` 释放）。
/// 纯算法，不依赖模型。
///
/// # Safety
///
/// `rgba` 至少 `width × height × 4` 字节可读；`out_*` 非空。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_heal_apply(
    rgba: *const u8,
    width: u32,
    height: u32,
    x: f32,
    y: f32,
    radius: f32,
    strength: f32,
    out_width: *mut u32,
    out_height: *mut u32,
    out_len: *mut usize,
) -> *mut u8 {
    guard(ptr::null_mut(), || {
        if out_width.is_null() || out_height.is_null() || out_len.is_null() {
            set_last_error("out_width / out_height / out_len 不能为空指针");
            return ptr::null_mut();
        }
        let mut image = match rgba_to_image(rgba, width, height) {
            Ok(image) => image,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        if !x.is_finite() || !y.is_finite() || !radius.is_finite() || !strength.is_finite() {
            set_last_error("修复参数包含非有限数值");
            return ptr::null_mut();
        }
        fdxmp_core::retouch::spot_heal(
            &mut image,
            x.clamp(0.0, 1.0) * width as f32,
            y.clamp(0.0, 1.0) * height as f32,
            radius.clamp(1.0, 200.0),
            strength.clamp(0.0, 1.0),
        );
        image_to_payload(&image, out_width, out_height, out_len)
    })
}

/// 液化：`strokes_json` = `[{"points":[[x,y],...],"radius":0.06,"strength":0.8}, ...]`。
///
/// 返回处理后的图像载荷（8 字节大端宽高 + RGBA8，用 `fdxmp_bytes_free` 释放）。纯算法。
///
/// # Safety
///
/// `rgba` 至少 `width × height × 4` 字节可读；`strokes_json` 合法 C 字符串；`out_*` 非空。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_liquify_apply(
    rgba: *const u8,
    width: u32,
    height: u32,
    strokes_json: *const c_char,
    out_width: *mut u32,
    out_height: *mut u32,
    out_len: *mut usize,
) -> *mut u8 {
    guard(ptr::null_mut(), || {
        if out_width.is_null() || out_height.is_null() || out_len.is_null() {
            set_last_error("out_width / out_height / out_len 不能为空指针");
            return ptr::null_mut();
        }
        let strokes_json = match cstr_to_str(strokes_json, "strokes_json") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        let image = match rgba_to_image(rgba, width, height) {
            Ok(image) => image,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        let strokes: Vec<fdxmp_core::retouch::LiquifyStroke> =
            match serde_json::from_str(strokes_json) {
                Ok(strokes) => strokes,
                Err(err) => {
                    set_last_error(format!("strokes_json 解析失败：{err}"));
                    return ptr::null_mut();
                }
            };
        if strokes.len() > 512 {
            set_last_error("液化笔画过多（≤ 512）");
            return ptr::null_mut();
        }
        let output = fdxmp_core::retouch::liquify(&image, &strokes);
        image_to_payload(&output, out_width, out_height, out_len)
    })
}

/// 提交平台姿态（iOS Vision）：人脸关键点 + 身体关节，绑定图像指纹（换图自动失效）。
///
/// `pose_json` 形如
/// `{"face":{"contour":[[x,y],…],"left_eye":[[x,y],…],…},"body":{"left_shoulder":[x,y,c],…}}`，
/// 坐标为归一化 0..1（原点左上）。返回 [`FDXMP_OK`] / [`FDXMP_ERR`]。
///
/// # Safety
///
/// `pose_json` 必须为合法 C 字符串；`rgba` 至少 `width × height × 4` 字节可读。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_retouch_submit_pose(
    rgba: *const u8,
    width: u32,
    height: u32,
    pose_json: *const c_char,
) -> i32 {
    guard(FDXMP_ERR, || {
        #[cfg(any(feature = "ai", feature = "ai-static"))]
        {
            let pose_json = match cstr_to_str(pose_json, "pose_json") {
                Ok(value) => value,
                Err(message) => {
                    set_last_error(message);
                    return FDXMP_ERR;
                }
            };
            let image = match rgba_to_image(rgba, width, height) {
                Ok(image) => image,
                Err(message) => {
                    set_last_error(message);
                    return FDXMP_ERR;
                }
            };
            match fdxmp_core::smartmask::runtime::submit_platform_pose(&image, pose_json) {
                Ok(()) => FDXMP_OK,
                Err(err) => {
                    set_last_error(err.message().to_string());
                    FDXMP_ERR
                }
            }
        }
        #[cfg(not(any(feature = "ai", feature = "ai-static")))]
        {
            let _ = (rgba, width, height, pose_json);
            set_last_error("当前构建未启用 AI 修图（ai / ai-static）");
            FDXMP_ERR
        }
    })
}

/// AI 修图进度文本（阶段名；空串 = 空闲）。UI 在修图任务运行期间轮询显示加载文案。
///
/// 返回的字符串用 `fdxmp_string_free` 释放；失败返回 NULL。
#[no_mangle]
pub extern "C" fn fdxmp_retouch_progress() -> *mut c_char {
    guard(ptr::null_mut(), || {
        #[cfg(any(feature = "ai", feature = "ai-static"))]
        {
            into_c_string(fdxmp_core::smartmask::runtime::retouch_progress())
        }
        #[cfg(not(any(feature = "ai", feature = "ai-static")))]
        {
            into_c_string(String::new())
        }
    })
}

/// AI 修图模型是否就绪（LaMa / GFPGAN / Real-ESRGAN 任一可用 → 1）。
#[no_mangle]
pub extern "C" fn fdxmp_retouch_ai_ready() -> i32 {
    guard(0, || {
        #[cfg(any(feature = "ai", feature = "ai-static"))]
        {
            if fdxmp_core::smartmask::runtime::ai_retouch_ready() {
                1
            } else {
                0
            }
        }
        #[cfg(not(any(feature = "ai", feature = "ai-static")))]
        {
            0
        }
    })
}

/// 导出增强：Real-ESRGAN 超分（分块推理；输入单边 ≤ 4096，输出 ≤ 64 MP）。
///
/// `target_max_edge` = 目标最长边（0 = 4 倍输出；倍率 = target / 原最长边，上限 4×）。
///
/// 返回放大后的图像载荷（8 字节大端宽高 + RGBA8，用 `fdxmp_bytes_free` 释放）。
///
/// # Safety
///
/// `rgba` 至少 `width × height × 4` 字节可读；`out_*` 非空可写。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_retouch_upscale(
    rgba: *const u8,
    width: u32,
    height: u32,
    target_max_edge: u32,
    out_width: *mut u32,
    out_height: *mut u32,
    out_len: *mut usize,
) -> *mut u8 {
    guard(ptr::null_mut(), || {
        if out_width.is_null() || out_height.is_null() || out_len.is_null() {
            set_last_error("out_width / out_height / out_len 不能为空指针");
            return ptr::null_mut();
        }
        let image = match rgba_to_image(rgba, width, height) {
            Ok(image) => image,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        #[cfg(any(feature = "ai", feature = "ai-static"))]
        {
            match fdxmp_core::smartmask::runtime::retouch_upscale(&image, target_max_edge) {
                Ok(output) => image_to_payload(&output, out_width, out_height, out_len),
                Err(err) => {
                    set_last_error(err.message().to_string());
                    ptr::null_mut()
                }
            }
        }
        #[cfg(not(any(feature = "ai", feature = "ai-static")))]
        {
            let _ = (image, target_max_edge);
            set_last_error("当前构建未启用 AI 修图（ai / ai-static）");
            ptr::null_mut()
        }
    })
}

/// 用随包 ONNX 模型生成智能蒙版（双阶段流水线），并写入共享栅格缓存。
///
/// 返回值：
/// - [`FDXMP_OK`]：已生成（后续渲染 / 叠加 / 导出直接复用）；
/// - [`FDXMP_SMARTMASK_FALLBACK`]：模型 / 运行库不可用，或该蒙版类型没有对应模型
///   —— 调用方回退平台实现（iOS Vision / 其它）；
/// - [`FDXMP_ERR`]：推理失败（无人脸 / 无人像等），原因见 `fdxmp_last_error_message`。
///
/// # Safety
///
/// `params_json`、`mask_id` 必须为合法 C 字符串；`rgba` 至少 `width × height × 4` 字节可读。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_smartmask_generate(
    params_json: *const c_char,
    mask_id: *const c_char,
    rgba: *const u8,
    width: u32,
    height: u32,
) -> i32 {
    guard(FDXMP_ERR, || {
        let params_json = match cstr_to_str(params_json, "params_json") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return FDXMP_ERR;
            }
        };
        let mask_id = match cstr_to_str(mask_id, "mask_id") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return FDXMP_ERR;
            }
        };
        let expected = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || rgba.is_null()
            || expected == 0
        {
            set_last_error(format!("图像尺寸非法：{width}×{height}"));
            return FDXMP_ERR;
        }
        let mask = match find_mask_params(params_json, mask_id) {
            Ok(mask) => mask,
            Err(message) => {
                set_last_error(message);
                return FDXMP_ERR;
            }
        };
        #[cfg(any(feature = "ai", feature = "ai-static"))]
        {
            // SAFETY: 调用方保证 rgba 至少 width × height × 4 字节可读。
            let buffer = std::slice::from_raw_parts(rgba, expected);
            let image = match image::RgbaImage::from_raw(width, height, buffer.to_vec()) {
                Some(image) => image,
                None => {
                    set_last_error("RGBA 缓冲尺寸不匹配");
                    return FDXMP_ERR;
                }
            };
            match fdxmp_core::smartmask::runtime::generate_into_cache(&mask, &image) {
                Ok(()) => FDXMP_OK,
                Err(error) => {
                    set_last_error(error.message().to_string());
                    if error.should_fallback() {
                        FDXMP_SMARTMASK_FALLBACK
                    } else {
                        FDXMP_ERR
                    }
                }
            }
        }
        #[cfg(not(any(feature = "ai", feature = "ai-static")))]
        {
            let _ = (mask, rgba, expected);
            set_last_error("当前构建未启用 ONNX 模型（ai / ai-static）");
            FDXMP_SMARTMASK_FALLBACK
        }
    })
}

/// 随包 ONNX 模型是否可用：1 = 就绪，0 = 不可用。
#[no_mangle]
pub extern "C" fn fdxmp_smartmask_runtime_ready() -> i32 {
    guard(0, || {
        #[cfg(any(feature = "ai", feature = "ai-static"))]
        {
            i32::from(fdxmp_core::smartmask::runtime::runtime_available())
        }
        #[cfg(not(any(feature = "ai", feature = "ai-static")))]
        {
            0
        }
    })
}

/// 解析参数 JSON 并取出指定蒙版。
fn find_mask_params(
    params_json: &str,
    mask_id: &str,
) -> std::result::Result<fdxmp_core::params::MaskParams, String> {
    let params: fdxmp_core::params::EditParams =
        serde_json::from_str(params_json).map_err(|err| format!("params_json 解析失败：{err}"))?;
    params
        .masks
        .iter()
        .find(|mask| mask.id == mask_id)
        .cloned()
        .ok_or_else(|| format!("找不到蒙版 {mask_id}"))
}

/// 提交平台提供者生成的蒙版权重栅格（智能蒙版）。
///
/// - `weights`：行优先 f32，取值 0..1，长度必须等于 `width × height`；
/// - 栅格按「蒙版参数」缓存（与尺寸无关），预览 / 叠加 / 导出会按各自
///   分辨率自动重采样复用；换图或参数变化后旧栅格自然失效；
/// - 单边上限 4096、总像素上限 16777216，超限拒绝（防 DoS）；
/// - 返回 [`FDXMP_OK`] / [`FDXMP_ERR`]。
///
/// # Safety
///
/// `params_json`、`mask_id` 必须为合法 C 字符串；`weights` 在 `len` 范围内可读。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_mask_raster_submit(
    params_json: *const c_char,
    mask_id: *const c_char,
    width: u32,
    height: u32,
    weights: *const f32,
    len: usize,
) -> i32 {
    guard(FDXMP_ERR, || {
        let params_json = match cstr_to_str(params_json, "params_json") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return FDXMP_ERR;
            }
        };
        let mask_id = match cstr_to_str(mask_id, "mask_id") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return FDXMP_ERR;
            }
        };
        let expected = (width as usize).saturating_mul(height as usize);
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || expected > SMARTMASK_MAX_PIXELS
        {
            set_last_error(format!(
                "蒙版栅格尺寸非法：{width}×{height}（单边 ≤ 4096，总像素 ≤ {SMARTMASK_MAX_PIXELS}）"
            ));
            return FDXMP_ERR;
        }
        if weights.is_null() || len != expected {
            set_last_error(format!(
                "权重长度不匹配：len={len}，期望 {width}×{height}={expected}"
            ));
            return FDXMP_ERR;
        }
        let mask = match find_mask_params(params_json, mask_id) {
            Ok(mask) => mask,
            Err(message) => {
                set_last_error(message);
                return FDXMP_ERR;
            }
        };
        // SAFETY: 调用方保证 weights 至少 len 个 f32 可读。
        let data = std::slice::from_raw_parts(weights, len);
        let raster = fdxmp_core::masks::MaskRaster {
            width,
            height,
            weights: data.to_vec(),
        };
        match fdxmp_core::masks::insert_ai_raster(&mask, raster) {
            Ok(()) => FDXMP_OK,
            Err(err) => {
                set_last_error(err.to_string());
                FDXMP_ERR
            }
        }
    })
}

/// 查询蒙版栅格是否已生成：1 = 就绪，0 = 未生成（参数非法时返回 [`FDXMP_ERR`]）。
///
/// # Safety
///
/// `params_json`、`mask_id` 必须为合法 C 字符串。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_mask_raster_ready(
    params_json: *const c_char,
    mask_id: *const c_char,
) -> i32 {
    guard(FDXMP_ERR, || {
        let params_json = match cstr_to_str(params_json, "params_json") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return FDXMP_ERR;
            }
        };
        let mask_id = match cstr_to_str(mask_id, "mask_id") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return FDXMP_ERR;
            }
        };
        match find_mask_params(params_json, mask_id) {
            Ok(mask) => i32::from(fdxmp_core::masks::ai_raster_ready(&mask)),
            Err(message) => {
                set_last_error(message);
                FDXMP_ERR
            }
        }
    })
}

/// 清空 AI / 平台蒙版栅格缓存（切换图片时调用）。返回 [`FDXMP_OK`]。
#[no_mangle]
pub extern "C" fn fdxmp_mask_raster_clear() -> i32 {
    guard(FDXMP_ERR, || {
        fdxmp_core::masks::clear_ai_raster_cache();
        FDXMP_OK
    })
}

/// 提交多边形蒙版（五官关键点 → 核心光栅化 + 羽化 + 缓存）。
///
/// `polygons_json` 形如：
/// ```json
/// {"include": [[[x, y], ...]], "exclude": [[[x, y], ...]]}
/// ```
/// 坐标为归一化 0..1（原点左上角，与图像坐标一致）。返回 [`FDXMP_OK`] / [`FDXMP_ERR`]。
///
/// # Safety
///
/// `params_json`、`mask_id`、`polygons_json` 必须为合法 C 字符串。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_mask_polygons_submit(
    params_json: *const c_char,
    mask_id: *const c_char,
    width: u32,
    height: u32,
    polygons_json: *const c_char,
    feather_px: f32,
) -> i32 {
    guard(FDXMP_ERR, || {
        let params_json = match cstr_to_str(params_json, "params_json") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return FDXMP_ERR;
            }
        };
        let mask_id = match cstr_to_str(mask_id, "mask_id") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return FDXMP_ERR;
            }
        };
        let polygons_json = match cstr_to_str(polygons_json, "polygons_json") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return FDXMP_ERR;
            }
        };
        let expected = (width as usize).saturating_mul(height as usize);
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || expected > SMARTMASK_MAX_PIXELS
        {
            set_last_error(format!("多边形蒙版尺寸非法：{width}×{height}"));
            return FDXMP_ERR;
        }
        if !feather_px.is_finite() || !(0.0..=128.0).contains(&feather_px) {
            set_last_error("feather_px 必须在 0..=128");
            return FDXMP_ERR;
        }
        let polygons: fdxmp_core::smartmask::PolygonMask = match serde_json::from_str(polygons_json)
        {
            Ok(value) => value,
            Err(err) => {
                set_last_error(format!("polygons_json 解析失败：{err}"));
                return FDXMP_ERR;
            }
        };
        // 规模与数值校验（防 DoS / NaN）。
        let total_points: usize = polygons
            .include
            .iter()
            .chain(polygons.exclude.iter())
            .map(|polygon| polygon.len())
            .sum();
        if polygons.include.len() > 64 || polygons.exclude.len() > 64 || total_points > 4096 {
            set_last_error("多边形数量过多（≤ 64 个 / ≤ 4096 点）");
            return FDXMP_ERR;
        }
        if polygons
            .include
            .iter()
            .chain(polygons.exclude.iter())
            .flat_map(|polygon| polygon.iter())
            .any(|point| !point[0].is_finite() || !point[1].is_finite())
        {
            set_last_error("多边形包含非有限坐标");
            return FDXMP_ERR;
        }
        let mask = match find_mask_params(params_json, mask_id) {
            Ok(mask) => mask,
            Err(message) => {
                set_last_error(message);
                return FDXMP_ERR;
            }
        };
        let rasterized = match polygons.rasterize(
            fdxmp_core::smartmask::SmartMaskKind::Face,
            width,
            height,
            feather_px,
        ) {
            Ok(mask) => mask,
            Err(err) => {
                set_last_error(err.to_string());
                return FDXMP_ERR;
            }
        };
        match fdxmp_core::masks::insert_ai_raster(&mask, rasterized.to_raster()) {
            Ok(()) => FDXMP_OK,
            Err(err) => {
                set_last_error(err.to_string());
                FDXMP_ERR
            }
        }
    })
}

/// 智能蒙版模型清单与状态（JSON 字符串，调用方用 `fdxmp_string_free` 释放）。
///
/// 返回 `{"dir": "...", "models": [{name, task, path, present, bytes,
/// hash_state, license, trainable, version, input_size, provider, precision,
/// source_url}]}`；失败返回 NULL。
#[no_mangle]
pub extern "C" fn fdxmp_smartmask_models_json() -> *mut c_char {
    guard(ptr::null_mut(), || {
        let registry = fdxmp_core::smartmask::ModelRegistry::new();
        let models: Vec<serde_json::Value> = registry
            .status()
            .into_iter()
            .zip(registry.specs().iter())
            .map(|(status, spec)| {
                serde_json::json!({
                    "name": status.name,
                    "task": spec.task,
                    "path": status.path,
                    "present": status.present,
                    "bytes": status.bytes,
                    "hash_state": status.hash_state,
                    "hash_registered": status.hash_registered,
                    "license": status.license,
                    "trainable": status.trainable,
                    "version": spec.version,
                    "input_size": [spec.input_size.0, spec.input_size.1],
                    "provider": spec.provider,
                    "precision": spec.precision,
                    "source_url": spec.source_url,
                })
            })
            .collect();
        let payload = serde_json::json!({
            "dir": registry.dir().display().to_string(),
            "models": models,
        });
        match serde_json::to_string(&payload) {
            Ok(text) => into_c_string(text),
            Err(err) => {
                set_last_error(format!("序列化模型状态失败：{err}"));
                ptr::null_mut()
            }
        }
    })
}

/// 缓存总大小（字节）；失败返回 [`FDXMP_U64_ERR`]。
///
/// # Safety
///
/// `cache_root` 必须为合法 C 字符串。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_cache_total_size(cache_root: *const c_char) -> u64 {
    guard(FDXMP_U64_ERR, || {
        let root = match cstr_to_str(cache_root, "cache_root") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return FDXMP_U64_ERR;
            }
        };
        match CacheManager::new(root).total_size() {
            Ok(size) => size,
            Err(err) => {
                set_last_error(err.to_string());
                FDXMP_U64_ERR
            }
        }
    })
}

/// 清除全部缓存，返回释放的字节数；失败返回 [`FDXMP_U64_ERR`]。
///
/// # Safety
///
/// `cache_root` 必须为合法 C 字符串。
#[no_mangle]
pub unsafe extern "C" fn fdxmp_cache_clear(cache_root: *const c_char) -> u64 {
    guard(FDXMP_U64_ERR, || {
        let root = match cstr_to_str(cache_root, "cache_root") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return FDXMP_U64_ERR;
            }
        };
        match CacheManager::new(root).clear_all() {
            Ok(freed) => freed,
            Err(err) => {
                set_last_error(err.to_string());
                FDXMP_U64_ERR
            }
        }
    })
}

/* ---------------- GPU 预览会话（Metal / DX12 / Vulkan；需启用 `gpu` feature） ----------------
 *
 * 移动端实时预览：源图只上传一次，参数变化时重渲染并回读 RGBA8。
 * 创建失败（无 GPU / 驱动不足 / 未启用 feature）返回 NULL，调用方回退 CPU 预览；
 * 「裁剪 + 蒙版」组合在 GPU 下坐标不一致，`render` 也返回 NULL
 * （错误信息见 `fdxmp_last_error_message`），同样由调用方回退 CPU。
 */

/// GPU 预览会话句柄（不透明；用 `fdxmp_gpu_preview_destroy` 释放）。
#[cfg(feature = "gpu")]
pub struct GpuPreviewHandle {
    session: fdxmp_core::gpu::PreviewSession,
}

/// 共享 GPU 设备：App 生命周期内只初始化一次，多次建会话不再重复初始化设备。
#[cfg(feature = "gpu")]
fn shared_gpu_context() -> std::result::Result<&'static fdxmp_core::gpu::GpuContext, String> {
    static SHARED: OnceLock<std::result::Result<&'static fdxmp_core::gpu::GpuContext, String>> =
        OnceLock::new();
    match SHARED.get_or_init(|| {
        fdxmp_core::gpu::GpuContext::new_blocking()
            .map(|context| &*Box::leak(Box::new(context)))
            .map_err(|err| err.to_string())
    }) {
        Ok(context) => Ok(context),
        Err(message) => Err(message.clone()),
    }
}

/// 未启用 `gpu` feature 时的占位句柄（所有调用失败返回 NULL）。
#[cfg(not(feature = "gpu"))]
pub struct GpuPreviewHandle {
    _private: (),
}

#[cfg(feature = "gpu")]
unsafe fn gpu_payload(width: u32, height: u32, rgba: &[u8], out_len: *mut usize) -> *mut u8 {
    let mut payload = Vec::with_capacity(8 + rgba.len());
    payload.extend_from_slice(&width.to_be_bytes());
    payload.extend_from_slice(&height.to_be_bytes());
    payload.extend_from_slice(rgba);
    let mut boxed = payload.into_boxed_slice();
    // SAFETY: 调用方保证 out_len 是非空可写指针。
    unsafe {
        *out_len = boxed.len();
    }
    let pointer = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    pointer
}

/// 用 RGBA8 源图创建 GPU 预览会话；失败返回 NULL（调用方回退 CPU）。
///
/// # Safety
///
/// `rgba` 必须指向至少 `width * height * 4` 字节的可读缓冲区。
#[cfg(feature = "gpu")]
#[no_mangle]
pub unsafe extern "C" fn fdxmp_gpu_preview_create(
    rgba: *const u8,
    width: u32,
    height: u32,
) -> *mut GpuPreviewHandle {
    guard(ptr::null_mut(), || {
        if rgba.is_null() || width == 0 || height == 0 {
            set_last_error("rgba / width / height 不合法");
            return ptr::null_mut();
        }
        let Some(expected) = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
        else {
            set_last_error("图像尺寸溢出");
            return ptr::null_mut();
        };
        // SAFETY: 调用方保证缓冲区至少 `width * height * 4` 字节。
        let buffer = std::slice::from_raw_parts(rgba, expected);
        let context = match shared_gpu_context() {
            Ok(context) => context,
            Err(message) => {
                set_last_error(format!("GPU 初始化失败：{message}"));
                return ptr::null_mut();
            }
        };
        match fdxmp_core::gpu::PreviewSession::new_with_context(context, buffer, width, height) {
            Ok(session) => Box::into_raw(Box::new(GpuPreviewHandle { session })),
            Err(err) => {
                set_last_error(err.to_string());
                ptr::null_mut()
            }
        }
    })
}

/// 未启用 `gpu` feature 的占位实现（返回 NULL）。
///
/// # Safety
///
/// 同正式实现（此版本不会读取任何指针）。
#[cfg(not(feature = "gpu"))]
#[no_mangle]
pub unsafe extern "C" fn fdxmp_gpu_preview_create(
    _rgba: *const u8,
    _width: u32,
    _height: u32,
) -> *mut GpuPreviewHandle {
    set_last_error("未启用 gpu feature，请使用 CPU 预览");
    ptr::null_mut()
}

/// 用参数 JSON 渲染一帧；返回格式同 `fdxmp_render_memory_rgba`
/// （8 字节大端宽高 + RGBA8，须用 `fdxmp_bytes_free` 释放）；失败返回 NULL。
///
/// # Safety
///
/// `handle` 必须是 `fdxmp_gpu_preview_create` 返回的指针；`params_json` 为合法 C 字符串；
/// `out_len` 为非空可写指针。
#[cfg(feature = "gpu")]
#[no_mangle]
pub unsafe extern "C" fn fdxmp_gpu_preview_render(
    handle: *mut GpuPreviewHandle,
    params_json: *const c_char,
    out_width: *mut u32,
    out_height: *mut u32,
    out_len: *mut usize,
) -> *mut u8 {
    guard(ptr::null_mut(), || {
        if handle.is_null() || out_width.is_null() || out_height.is_null() || out_len.is_null() {
            set_last_error("handle / out_width / out_height / out_len 不能为空指针");
            return ptr::null_mut();
        }
        let params_json = match cstr_to_str(params_json, "params_json") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return ptr::null_mut();
            }
        };
        let params: fdxmp_core::params::EditParams = match serde_json::from_str(params_json) {
            Ok(value) => value,
            Err(err) => {
                set_last_error(format!("参数 JSON 解析失败：{err}"));
                return ptr::null_mut();
            }
        };
        // SAFETY: 调用方保证 handle 由 create 返回且未被 destroy。
        let session = &mut (*handle).session;
        match session.render(&params) {
            Ok((width, height, rgba)) => {
                *out_width = width;
                *out_height = height;
                gpu_payload(width, height, &rgba, out_len)
            }
            Err(err) => {
                set_last_error(err.to_string());
                ptr::null_mut()
            }
        }
    })
}

/// 未启用 `gpu` feature 的占位实现（返回 NULL）。
///
/// # Safety
///
/// 同正式实现（此版本不会读取任何指针）。
#[cfg(not(feature = "gpu"))]
#[no_mangle]
pub unsafe extern "C" fn fdxmp_gpu_preview_render(
    _handle: *mut GpuPreviewHandle,
    _params_json: *const c_char,
    _out_width: *mut u32,
    _out_height: *mut u32,
    _out_len: *mut usize,
) -> *mut u8 {
    set_last_error("未启用 gpu feature，请使用 CPU 预览");
    ptr::null_mut()
}

/// 返回 GPU 后端名称（"metal" / "dx12" / "vulkan"…，须用 `fdxmp_string_free` 释放）；
/// 失败返回 NULL。
///
/// # Safety
///
/// `handle` 必须是 `fdxmp_gpu_preview_create` 返回的指针。
#[cfg(feature = "gpu")]
#[no_mangle]
pub unsafe extern "C" fn fdxmp_gpu_preview_backend(handle: *mut GpuPreviewHandle) -> *mut c_char {
    guard(ptr::null_mut(), || {
        if handle.is_null() {
            set_last_error("handle 不能为空指针");
            return ptr::null_mut();
        }
        // SAFETY: 调用方保证 handle 由 create 返回且未被 destroy。
        into_c_string((*handle).session.backend().to_string())
    })
}

/// 未启用 `gpu` feature 的占位实现（返回 NULL）。
///
/// # Safety
///
/// 同正式实现（此版本不会读取任何指针）。
#[cfg(not(feature = "gpu"))]
#[no_mangle]
pub unsafe extern "C" fn fdxmp_gpu_preview_backend(_handle: *mut GpuPreviewHandle) -> *mut c_char {
    set_last_error("未启用 gpu feature");
    ptr::null_mut()
}

/// 释放 GPU 预览会话。
///
/// # Safety
///
/// `handle` 必须是 `fdxmp_gpu_preview_create` 返回的指针，且只能释放一次。
#[cfg(feature = "gpu")]
#[no_mangle]
pub unsafe extern "C" fn fdxmp_gpu_preview_destroy(handle: *mut GpuPreviewHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: 调用方保证 handle 由 create 返回且未释放过。
    drop(Box::from_raw(handle));
}

/// 未启用 `gpu` feature 的占位实现（空操作）。
///
/// # Safety
///
/// 同正式实现（此版本不会读取任何指针）。
#[cfg(not(feature = "gpu"))]
#[no_mangle]
pub unsafe extern "C" fn fdxmp_gpu_preview_destroy(_handle: *mut GpuPreviewHandle) {}

/// 把 CAMetalLayer 挂到会话上（零拷贝上屏；iOS / macOS）。成功返回 0，失败返回 -1。
///
/// # Safety
///
/// `handle` 必须是 `fdxmp_gpu_preview_create` 返回的指针；`layer` 必须是存活的
/// `CAMetalLayer` 指针，且在会话使用期间一直有效。
#[cfg(all(feature = "gpu", target_vendor = "apple"))]
#[no_mangle]
pub unsafe extern "C" fn fdxmp_gpu_preview_attach_layer(
    handle: *mut GpuPreviewHandle,
    layer: *mut c_void,
) -> i32 {
    guard(FDXMP_ERR, || {
        if handle.is_null() || layer.is_null() {
            set_last_error("handle / layer 不能为空指针");
            return FDXMP_ERR;
        }
        // SAFETY: 调用方保证 handle 由 create 返回且未被 destroy。
        match (*handle).session.attach_layer(layer) {
            Ok(()) => FDXMP_OK,
            Err(err) => {
                set_last_error(err.to_string());
                FDXMP_ERR
            }
        }
    })
}

/// 当前平台 / 构建不支持零拷贝上屏（返回 -1）。
///
/// # Safety
///
/// 同正式实现（此版本不会读取任何指针）。
#[cfg(not(all(feature = "gpu", target_vendor = "apple")))]
#[no_mangle]
pub unsafe extern "C" fn fdxmp_gpu_preview_attach_layer(
    _handle: *mut GpuPreviewHandle,
    _layer: *mut c_void,
) -> i32 {
    set_last_error("当前平台 / 构建不支持零拷贝上屏");
    FDXMP_ERR
}

/// 渲染一帧并直接上屏（不读回像素；输出尺寸写入 out_width / out_height）。
/// 成功返回 0，失败返回 -1（调用方回退回读路径）。
///
/// # Safety
///
/// `handle` 必须是 `fdxmp_gpu_preview_create` 返回的指针；`params_json` 为合法 C 字符串；
/// `out_width`、`out_height` 为非空可写指针。
#[cfg(all(feature = "gpu", target_vendor = "apple"))]
#[no_mangle]
pub unsafe extern "C" fn fdxmp_gpu_preview_present(
    handle: *mut GpuPreviewHandle,
    params_json: *const c_char,
    out_width: *mut u32,
    out_height: *mut u32,
) -> i32 {
    guard(FDXMP_ERR, || {
        if handle.is_null() || out_width.is_null() || out_height.is_null() {
            set_last_error("handle / out_width / out_height 不能为空指针");
            return FDXMP_ERR;
        }
        let params_json = match cstr_to_str(params_json, "params_json") {
            Ok(value) => value,
            Err(message) => {
                set_last_error(message);
                return FDXMP_ERR;
            }
        };
        let params: fdxmp_core::params::EditParams = match serde_json::from_str(params_json) {
            Ok(value) => value,
            Err(err) => {
                set_last_error(format!("参数 JSON 解析失败：{err}"));
                return FDXMP_ERR;
            }
        };
        // SAFETY: 调用方保证 handle 由 create 返回且未被 destroy。
        match (*handle).session.present(&params) {
            Ok((width, height)) => {
                *out_width = width;
                *out_height = height;
                FDXMP_OK
            }
            Err(err) => {
                set_last_error(err.to_string());
                FDXMP_ERR
            }
        }
    })
}

/// 当前平台 / 构建不支持零拷贝上屏（返回 -1）。
///
/// # Safety
///
/// 同正式实现（此版本不会读取任何指针）。
#[cfg(not(all(feature = "gpu", target_vendor = "apple")))]
#[no_mangle]
pub unsafe extern "C" fn fdxmp_gpu_preview_present(
    _handle: *mut GpuPreviewHandle,
    _params_json: *const c_char,
    _out_width: *mut u32,
    _out_height: *mut u32,
) -> i32 {
    set_last_error("当前平台 / 构建不支持零拷贝上屏");
    FDXMP_ERR
}

/// GPU 直方图：返回 4×256 个小端 u32（R / G / B / 亮度，共 4096 字节），
/// 须用 `fdxmp_bytes_free` 释放；失败返回 NULL。
///
/// # Safety
///
/// `handle` 必须是 `fdxmp_gpu_preview_create` 返回的指针；`out_len` 为非空可写指针。
#[cfg(feature = "gpu")]
#[no_mangle]
pub unsafe extern "C" fn fdxmp_gpu_preview_histogram(
    handle: *mut GpuPreviewHandle,
    out_len: *mut usize,
) -> *mut u8 {
    guard(ptr::null_mut(), || {
        if handle.is_null() || out_len.is_null() {
            set_last_error("handle / out_len 不能为空指针");
            return ptr::null_mut();
        }
        // SAFETY: 调用方保证 handle 由 create 返回且未被 destroy。
        match (*handle).session.histogram() {
            Ok(bins) => {
                let mut payload = Vec::with_capacity(bins.len() * 4);
                for value in bins.iter() {
                    payload.extend_from_slice(&value.to_le_bytes());
                }
                let mut boxed = payload.into_boxed_slice();
                *out_len = boxed.len();
                let pointer = boxed.as_mut_ptr();
                std::mem::forget(boxed);
                pointer
            }
            Err(err) => {
                set_last_error(err.to_string());
                ptr::null_mut()
            }
        }
    })
}

/// 未启用 `gpu` feature 的占位实现（返回 NULL）。
///
/// # Safety
///
/// 同正式实现（此版本不会读取任何指针）。
#[cfg(not(feature = "gpu"))]
#[no_mangle]
pub unsafe extern "C" fn fdxmp_gpu_preview_histogram(
    _handle: *mut GpuPreviewHandle,
    _out_len: *mut usize,
) -> *mut u8 {
    set_last_error("未启用 gpu feature");
    ptr::null_mut()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn version_and_abi_are_stable() {
        assert_eq!(fdxmp_abi_version(), 1);
        let ptr = fdxmp_version();
        assert!(!ptr.is_null());
        // SAFETY: 返回的是库内静态字符串。
        let version = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert!(!version.is_empty());
    }

    #[test]
    fn smartmask_raster_submit_flows_into_overlay_and_render() {
        // 一张 8×8 的径向蒙版（不依赖模型），用提交的栅格走完整链路。
        let params = CString::new(
            r#"{"version":1,"masks":[{"id":"m1","name":"AI 主体","visible":true,"inverted":false,"feather":0,"opacity":100,"overlay":"red","kind":{"type":"ai_subject"}}]}"#,
        )
        .unwrap();
        let mask_id = CString::new("m1").unwrap();
        unsafe {
            // 1) 未提交前：状态 0，叠加层为全透明。
            assert_eq!(
                fdxmp_mask_raster_ready(params.as_ptr(), mask_id.as_ptr()),
                0
            );
            let mut out_w = 0u32;
            let mut out_h = 0u32;
            let mut out_len = 0usize;
            let empty = fdxmp_mask_overlay_rgba(
                params.as_ptr(),
                mask_id.as_ptr(),
                8,
                8,
                &mut out_w,
                &mut out_h,
                &mut out_len,
            );
            assert!(!empty.is_null());
            let transparent = (0..(out_w as usize) * (out_h as usize))
                .all(|index| *empty.add(8 + index * 4 + 3) == 0);
            assert!(transparent, "未提交栅格时叠加层应全透明");
            fdxmp_bytes_free(empty, out_len);

            // 2) 提交 4×4 基础栅格（8×8 请求会重采样）。
            let weights = [0.6f32; 16];
            assert_eq!(
                fdxmp_mask_raster_submit(
                    params.as_ptr(),
                    mask_id.as_ptr(),
                    4,
                    4,
                    weights.as_ptr(),
                    weights.len(),
                ),
                FDXMP_OK
            );
            assert_eq!(
                fdxmp_mask_raster_ready(params.as_ptr(), mask_id.as_ptr()),
                1
            );

            let overlay = fdxmp_mask_overlay_rgba(
                params.as_ptr(),
                mask_id.as_ptr(),
                8,
                8,
                &mut out_w,
                &mut out_h,
                &mut out_len,
            );
            assert!(!overlay.is_null());
            assert_eq!((out_w, out_h), (8, 8));
            assert_eq!(out_len, 8 + 8 * 8 * 4);
            // 权重 0.6 × 叠加层最大不透明度 0.45 → alpha ≈ 69。
            let alpha = *overlay.add(8 + 3);
            assert!((alpha as i32 - 69).abs() <= 2, "alpha={alpha}");
            fdxmp_bytes_free(overlay, out_len);

            // 3) 非法输入被拒绝。
            assert_eq!(
                fdxmp_mask_raster_submit(
                    params.as_ptr(),
                    mask_id.as_ptr(),
                    4,
                    4,
                    weights.as_ptr(),
                    15,
                ),
                FDXMP_ERR,
                "长度不匹配必须拒绝"
            );
            assert_eq!(
                fdxmp_mask_raster_submit(
                    params.as_ptr(),
                    mask_id.as_ptr(),
                    4097,
                    4,
                    weights.as_ptr(),
                    16,
                ),
                FDXMP_ERR,
                "超限尺寸必须拒绝"
            );
            assert_eq!(fdxmp_mask_raster_clear(), FDXMP_OK);
            assert_eq!(
                fdxmp_mask_raster_ready(params.as_ptr(), mask_id.as_ptr()),
                0
            );
        }
    }

    #[test]
    fn smartmask_polygons_submit_rasterizes_in_core() {
        let params = CString::new(
            r#"{"version":1,"masks":[{"id":"f1","name":"AI 面部","visible":true,"inverted":false,"feather":0,"opacity":100,"overlay":"red","kind":{"type":"ai_people","data":{"part":"face"}}}]}"#,
        )
        .unwrap();
        let mask_id = CString::new("f1").unwrap();
        let polygons = CString::new(
            r#"{"include":[[[0.1,0.1],[0.9,0.1],[0.9,0.9],[0.1,0.9]]],"exclude":[[[0.4,0.4],[0.6,0.4],[0.6,0.6],[0.4,0.6]]]}"#,
        )
        .unwrap();
        unsafe {
            assert_eq!(
                fdxmp_mask_polygons_submit(
                    params.as_ptr(),
                    mask_id.as_ptr(),
                    32,
                    32,
                    polygons.as_ptr(),
                    0.5,
                ),
                FDXMP_OK
            );
            assert_eq!(
                fdxmp_mask_raster_ready(params.as_ptr(), mask_id.as_ptr()),
                1
            );
            // 几何非法 / 空多边形必须拒绝。
            let empty = CString::new(r#"{"include":[],"exclude":[]}"#).unwrap();
            assert_eq!(
                fdxmp_mask_polygons_submit(
                    params.as_ptr(),
                    mask_id.as_ptr(),
                    32,
                    32,
                    empty.as_ptr(),
                    0.0,
                ),
                FDXMP_ERR
            );
            let bad_feather =
                CString::new(r#"{"include":[[[0.1,0.1],[0.9,0.1],[0.5,0.9]]]}"#).unwrap();
            assert_eq!(
                fdxmp_mask_polygons_submit(
                    params.as_ptr(),
                    mask_id.as_ptr(),
                    32,
                    32,
                    bad_feather.as_ptr(),
                    f32::NAN,
                ),
                FDXMP_ERR
            );
            fdxmp_mask_raster_clear();
        }
    }

    #[test]
    fn smartmask_models_json_lists_specs_and_hashes() {
        let pointer = fdxmp_smartmask_models_json();
        assert!(!pointer.is_null());
        // SAFETY: 返回的是本库分配的 C 字符串。
        let text = unsafe {
            let text = CStr::from_ptr(pointer).to_str().unwrap().to_string();
            fdxmp_string_free(pointer);
            text
        };
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let models = value["models"].as_array().unwrap();
        assert_eq!(models.len(), 8);
        let tasks: Vec<String> = models
            .iter()
            .map(|model| model["task"].as_str().unwrap().to_string())
            .collect();
        for task in [
            "face_detect",
            "person_coarse",
            "face_parse",
            "person_matting",
            "face_landmarks",
            "inpaint",
            "face_restore",
            "super_res",
        ] {
            assert!(tasks.contains(&task.to_string()), "缺少任务 {task}");
        }
        for model in models {
            assert!(model["hash_state"].is_string());
            assert!(!model["license"].as_str().unwrap().is_empty());
        }
    }

    #[test]
    fn retouch_ai_helpers_are_wired() {
        // 进度：空闲时也是合法（可释放）的字符串。
        let pointer = fdxmp_retouch_progress();
        assert!(!pointer.is_null());
        // SAFETY: 返回的是本库分配的 C 字符串。
        unsafe { fdxmp_string_free(pointer) };

        // 姿态 JSON 非法 → 错误路径（返回 -1，不崩溃）。
        let json = CString::new("{not json").unwrap();
        let rgba = [0u8; 8 * 8 * 4];
        let code = unsafe { fdxmp_retouch_submit_pose(rgba.as_ptr(), 8, 8, json.as_ptr()) };
        assert_eq!(code, FDXMP_ERR);

        // 超分：未随包模型（或未启用 ai）→ NULL。
        let mut width = 0u32;
        let mut height = 0u32;
        let mut len = 0usize;
        let out = unsafe {
            fdxmp_retouch_upscale(rgba.as_ptr(), 8, 8, 0, &mut width, &mut height, &mut len)
        };
        assert!(out.is_null());
    }

    #[test]
    fn engine_lifecycle_and_edit_roundtrip() {
        let engine = fdxmp_engine_new();
        assert!(!engine.is_null());
        // SAFETY: engine 来自上面的创建调用。
        unsafe {
            let json_ptr = fdxmp_engine_params_json(engine);
            assert!(!json_ptr.is_null());
            let json = CStr::from_ptr(json_ptr).to_str().unwrap().to_string();
            fdxmp_string_free(json_ptr);

            let mut params: serde_json::Value = serde_json::from_str(&json).unwrap();
            params["light"]["exposure"] = serde_json::json!(1.25);
            let label = CString::new("曝光").unwrap();
            let new_json = CString::new(params.to_string()).unwrap();
            assert_eq!(
                fdxmp_engine_apply_params_json(engine, label.as_ptr(), new_json.as_ptr()),
                FDXMP_OK
            );
            assert_eq!(fdxmp_engine_history_len(engine), 2);
            assert_eq!(fdxmp_engine_undo(engine), 1);
            assert_eq!(fdxmp_engine_redo(engine), 1);

            fdxmp_engine_free(engine);
        }
    }

    #[test]
    fn preset_bytes_roundtrip_through_ffi() {
        // SAFETY: 所有指针均指向本测试构造的合法 C 字符串。
        unsafe {
            let params = CString::new("{}").unwrap();
            let name = CString::new("ffi-preset").unwrap();
            let author = CString::new("tester").unwrap();
            let mut out_len = 0usize;
            let bytes = fdxmp_preset_bytes_from_params_json(
                params.as_ptr(),
                name.as_ptr(),
                author.as_ptr(),
                &mut out_len,
            );
            assert!(!bytes.is_null());
            assert!(out_len > 0);

            let json_ptr = fdxmp_preset_params_json_from_bytes(bytes, out_len);
            assert!(!json_ptr.is_null());
            let json = CStr::from_ptr(json_ptr).to_str().unwrap().to_string();
            assert!(json.contains("version"));
            fdxmp_string_free(json_ptr);

            // 新增接口：`.fdxmp` 元数据 + 参数 JSON。
            let meta_ptr = fdxmp_preset_json_from_bytes(bytes, out_len);
            assert!(!meta_ptr.is_null());
            let meta = CStr::from_ptr(meta_ptr).to_str().unwrap().to_string();
            assert!(meta.contains("ffi-preset"));
            assert!(meta.contains("params"));
            fdxmp_string_free(meta_ptr);
            fdxmp_bytes_free(bytes, out_len);
        }
    }

    #[test]
    fn preset_json_from_xmp_through_ffi() {
        // SAFETY: 所有指针均指向本测试构造的合法 C 字符串。
        unsafe {
            let xmp = CString::new(
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
      xmp:PresetName="FFI 导入"
      crs:Exposure2012="+0.75"
      crs:Saturation="10" />
</x:xmpmeta>"#,
            )
            .unwrap();
            let pointer = fdxmp_preset_json_from_xmp(xmp.as_ptr());
            assert!(!pointer.is_null());
            let json = CStr::from_ptr(pointer).to_str().unwrap().to_string();
            fdxmp_string_free(pointer);
            assert!(json.contains("FFI 导入"), "{json}");
            assert!(json.contains("exposure"), "{json}");

            // 非 XMP / 没有 crs 属性：报错并返回 NULL。
            let invalid = CString::new("<xmpmeta></xmpmeta>").unwrap();
            let pointer = fdxmp_preset_json_from_xmp(invalid.as_ptr());
            assert!(pointer.is_null());
        }
    }

    #[test]
    fn memory_rgba_render_through_ffi() {
        // SAFETY: 像素缓冲区、参数 C 字符串与输出指针均为本测试构造的合法内存。
        unsafe {
            let mut rgba = [64u8; 4 * 4 * 4];
            for index in (3..rgba.len()).step_by(4) {
                rgba[index] = 255;
            }
            let params = CString::new(r#"{"version":1,"light":{"exposure":1.0}}"#).unwrap();
            let mut out_width = 0u32;
            let mut out_height = 0u32;
            let mut out_len = 0usize;
            let pointer = fdxmp_render_memory_rgba(
                rgba.as_ptr(),
                4,
                4,
                params.as_ptr(),
                &mut out_width,
                &mut out_height,
                &mut out_len,
            );
            assert!(!pointer.is_null());
            assert_eq!((out_width, out_height), (4, 4));
            assert_eq!(out_len, 8 + 4 * 4 * 4);
            let payload = std::slice::from_raw_parts(pointer, out_len);
            assert!(
                (85..=95).contains(&payload[8]),
                "曝光 +1EV 后应约为 90：{}",
                payload[8]
            );
            fdxmp_bytes_free(pointer, out_len);
        }
    }

    #[test]
    fn mask_overlay_through_ffi() {
        // SAFETY: 参数 C 字符串与输出指针均为本测试构造的合法内存。
        unsafe {
            let params = CString::new(
                r#"{"version":1,"masks":[{"id":"m1","name":"画笔 1","visible":true,"inverted":false,"feather":0,"opacity":100,"overlay":"red","kind":{"type":"brush","data":{"size":40.0,"feather":50.0,"flow":100.0,"density":100.0,"auto_mask":false,"strokes":[{"points":[[0.5,0.5]],"erase":false}]}}}]}"#,
            )
            .unwrap();
            let mask_id = CString::new("m1").unwrap();
            let mut out_width = 0u32;
            let mut out_height = 0u32;
            let mut out_len = 0usize;
            let pointer = fdxmp_mask_overlay_rgba(
                params.as_ptr(),
                mask_id.as_ptr(),
                64,
                64,
                &mut out_width,
                &mut out_height,
                &mut out_len,
            );
            assert!(!pointer.is_null());
            assert_eq!((out_width, out_height), (64, 64));
            assert_eq!(out_len, 8 + 64 * 64 * 4);
            let payload = std::slice::from_raw_parts(pointer, out_len);
            let center = 8 + 4 * (32 * 64 + 32);
            assert!(payload[center + 3] > 100, "笔画中心应有叠加 alpha");
            assert!(payload[8 + 3] < 10, "角落应无叠加");
            fdxmp_bytes_free(pointer, out_len);

            // 未知 id：返回 NULL 并写入错误信息。
            let missing = CString::new("nope").unwrap();
            let pointer = fdxmp_mask_overlay_rgba(
                params.as_ptr(),
                missing.as_ptr(),
                64,
                64,
                &mut out_width,
                &mut out_height,
                &mut out_len,
            );
            assert!(pointer.is_null());
            let message = fdxmp_last_error_message();
            assert!(!message.is_null());
            fdxmp_string_free(message);
        }
    }

    #[test]
    fn null_arguments_report_errors() {
        // SAFETY: 故意传入 NULL，验证错误处理路径。
        unsafe {
            assert!(fdxmp_engine_params_json(ptr::null()).is_null());
            let message = fdxmp_last_error_message();
            assert!(!message.is_null());
            fdxmp_string_free(message);
            assert_eq!(fdxmp_cache_total_size(ptr::null()), FDXMP_U64_ERR);
        }
    }

    /// RAW 内存解码入口（与 iOS `NativeCore.decodeRaw` 调用同一个 FFI 函数）。
    ///
    /// 需要本机样张（`%USERPROFILE%\.cache\fdxmp-tools\samples\test-z8.NEF` 或
    /// `exiftool-DNG.dng`，可用 FDXMP_TEST_RAW / FDXMP_TEST_DNG 指定）；没有则跳过。
    #[test]
    fn decode_raw_rgba8_through_ffi() {
        let Some(path) = raw_sample_path() else {
            eprintln!("跳过 FFI RAW 解码测试：未找到样张（FDXMP_TEST_RAW / FDXMP_TEST_DNG）");
            return;
        };
        let bytes = std::fs::read(&path).unwrap();
        let mut out_width = 0u32;
        let mut out_height = 0u32;
        let mut out_len = 0usize;
        // SAFETY: bytes 缓冲区在整个调用期间存活；out_* 是本地可写变量。
        let pointer = unsafe {
            fdxmp_decode_raw_rgba8(
                bytes.as_ptr(),
                bytes.len(),
                512,
                0,
                &mut out_width,
                &mut out_height,
                &mut out_len,
            )
        };
        if cfg!(feature = "raw") {
            assert!(!pointer.is_null(), "启用 raw 后解码应成功");
            // SAFETY: 返回指针按约定是 8 字节大端宽高 + RGBA8 像素。
            let payload = unsafe { std::slice::from_raw_parts(pointer, out_len) };
            let width = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let height = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
            assert_eq!((width, height), (out_width, out_height));
            assert!(width.max(height) <= 512, "长边应受 max_edge 限制");
            assert_eq!(out_len, 8 + (width as usize) * (height as usize) * 4);
            unsafe { fdxmp_bytes_free(pointer, out_len) };
        } else {
            assert!(pointer.is_null());
            // SAFETY: 错误信息为库内字符串，读完即释放。
            let message = unsafe {
                let ptr = fdxmp_last_error_message();
                assert!(!ptr.is_null());
                let text = CStr::from_ptr(ptr).to_string_lossy().to_string();
                fdxmp_string_free(ptr);
                text
            };
            assert!(
                message.contains("raw"),
                "未启用 feature 时应给出提示：{message}"
            );
        }
    }

    /// 本机 RAW 样张路径（环境变量优先，其次默认缓存目录）。
    fn raw_sample_path() -> Option<std::path::PathBuf> {
        for key in ["FDXMP_TEST_RAW", "FDXMP_TEST_DNG"] {
            if let Some(path) = std::env::var_os(key) {
                let path = std::path::PathBuf::from(path);
                if path.is_file() {
                    return Some(path);
                }
            }
        }
        let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
        let samples = std::path::Path::new(&home)
            .join(".cache")
            .join("fdxmp-tools")
            .join("samples");
        for name in ["test-z8.NEF", "exiftool-DNG.dng"] {
            let candidate = samples.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }
}
