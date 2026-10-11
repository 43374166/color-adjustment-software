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
        match fdxmp_core::gpu::PreviewSession::new(buffer, width, height) {
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
}
