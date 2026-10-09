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
            fdxmp_bytes_free(bytes, out_len);
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
}
