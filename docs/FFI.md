# fdxmp FFI 桥接方案

## 1. 目标

一份 Rust 核心（`fdxmp-core`），四端可用：

| 端 | 桥接方式 | 产物 |
| --- | --- | --- |
| Windows | 直接依赖 Rust crate（egui / eframe） | `fdxmp-win-ui.exe` |
| Android | JNI（`crates/android_bridge`） | `libfdxmp_android_bridge.so` |
| HarmonyOS | ohos-rs / N-API（`crates/ohos_bridge`） | `libfdxmp_ohos_bridge.so` |
| iOS | C ABI staticlib（`crates/ffi`） | `libfdxmp_ffi.a` + `fdxmp.h` |

统一语义由 `crates/ffi` 定义：所有平台桥接层与 C ABI 保持一一对应，
避免四端行为漂移。

## 2. C ABI 约定

头文件：`crates/ffi/include/fdxmp.h`

### 2.1 内存所有权

| 返回类型 | 释放函数 | 说明 |
| --- | --- | --- |
| `char *` | `fdxmp_string_free` | UTF-8、NUL 结尾 |
| `uint8_t *` + `size_t` | `fdxmp_bytes_free(ptr, len)` | `.fdxmp` 字节流等二进制 |
| `const char *`（`fdxmp_version`） | 不释放 | 库内静态字符串 |
| `void *` engine | `fdxmp_engine_free` | 引擎句柄 |

### 2.2 错误模型

- 指针返回值：失败返回 `NULL`；
- 整数返回值：失败返回 `-1`；
- `uint64_t` 返回值：失败返回 `UINT64_MAX`；
- 失败原因通过 `fdxmp_last_error_message()` 获取（thread-local，同一线程读取）；
- Rust panic 在 FFI 边界被 `catch_unwind` 捕获，转换为上述错误返回；
- 空指针参数不会导致崩溃，统一写入错误信息。

### 2.3 ABI 版本

- `FDXMP_ABI_VERSION`（当前 `1`）；
- 破坏性变更（删除 / 改签名 / 改语义）必须递增；
- 新增函数不递增（向后兼容）。

## 3. 已导出接口（阶段 0）

| 函数 | 说明 |
| --- | --- |
| `fdxmp_abi_version` | ABI 版本 |
| `fdxmp_version` | 核心版本 |
| `fdxmp_last_error_message` | 最近错误 |
| `fdxmp_string_free` / `fdxmp_bytes_free` | 释放内存 |
| `fdxmp_engine_new` / `fdxmp_engine_new_with_cache_root` / `fdxmp_engine_free` | 引擎生命周期 |
| `fdxmp_engine_params_json` | 导出当前参数 JSON |
| `fdxmp_engine_apply_params_json` | 应用参数（写入历史） |
| `fdxmp_engine_undo` / `fdxmp_engine_redo` | 撤销 / 重做 |
| `fdxmp_engine_history_len` / `fdxmp_engine_history_index` | 历史状态 |
| `fdxmp_preset_params_json_from_bytes` | 解析 `.fdxmp` → 参数 JSON |
| `fdxmp_preset_bytes_from_params_json` | 参数 JSON + 名称 + 作者 → `.fdxmp` 字节流 |
| `fdxmp_probe_json` | 探测格式 / 尺寸 |
| `fdxmp_cache_total_size` / `fdxmp_cache_clear` | 缓存统计 / 清除 |

## 4. Android（JNI）

Kotlin 侧：

```kotlin
package com.fdxmp.core

object NativeBridge {
    init { System.loadLibrary("fdxmp_android_bridge") }
    external fun nativeAbiVersion(): Int
    external fun nativeVersion(): String
}
```

对应 Rust 导出（`crates/android_bridge/src/lib.rs`）：

| JNI 符号 | 签名 |
| --- | --- |
| `Java_com_fdxmp_core_NativeBridge_nativeAbiVersion` | `()I` |
| `Java_com_fdxmp_core_NativeBridge_nativeVersion` | `()Ljava/lang/String;` |

后续接口（阶段 5）按同一模式扩展：`nativeEngineNew`、`nativeApplyParams`、
`nativeExport` 等，参数使用 JSON 字符串 / 字节数组，避免复杂的 JNI 对象编组。

## 5. HarmonyOS（ohos-rs / N-API）

- crate：`napi-ohos`（git: `ohos-rs/ohos-rs`，仅在 `target_env = "ohos"` 下编译；
  阶段 0 环境无法完成 GitHub git clone，启用步骤见 `crates/ohos_bridge/README.md`）；
- 目标：`aarch64-unknown-linux-ohos`；
- 导出：`abiVersion(): number`、`version(): string`，阶段 5 扩展其余接口；
- ArkTS 侧 `import fdxmp from 'libfdxmp_ohos_bridge.so'`。

## 6. iOS（C ABI）

- `cargo build --target aarch64-apple-ios -p fdxmp-ffi` 产出 `staticlib`；
- 通过 bridging header / module map 引入 `fdxmp.h`；
- Swift 侧用 `@_silgen_name` 或 module 封装调用；
- 按规格：先写代码、不打包。

## 7. 编辑状态与 sidecar

- 每次退出编辑界面，把 `EditParams` 序列化为 sidecar（与图片同目录或应用沙盒数据库）；
- sidecar 与 `.fdxmp` 使用**同一** `EditParams` schema，保证互通；
- 预设文件（`.fdxmp`）额外携带 `schema_version` / `name` / `author` / `created_at`。
