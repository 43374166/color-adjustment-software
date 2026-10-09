//! # fdxmp-ohos-bridge
//!
//! HarmonyOS（ArkTS / ArkUI）→ Rust 核心的 N-API 桥接层，基于 `ohos-rs`。
//!
//! - 目标三元组：`aarch64-unknown-linux-ohos`
//! - 依赖：`napi-ohos`（git: ohos-rs/ohos-rs，crates.io 未发布；见 README.md 启用方式）
//! - 模块注册入口：`napi_register_module_v1`（阶段 5 用 `#[napi]` 宏展开）
//!
//! ArkTS 侧对应模块：`libfdxmp_ohos_bridge.so`，导出 `abiVersion` / `version`
//! 以及后续的解码、编辑、导出接口。

/// 模块名（对应 ArkTS `import fdxmp from 'libfdxmp_ohos_bridge.so'`）。
pub const MODULE_NAME: &str = "fdxmp_ohos_bridge";

/// ABI 版本。
pub fn abi_version() -> i32 {
    fdxmp_core::ABI_VERSION as i32
}

/// 核心版本字符串。
pub fn core_version() -> &'static str {
    fdxmp_core::version()
}

/// 导出符号对照表（阶段 5 生成 ArkTS 声明文件时使用）。
pub fn exports() -> [(&'static str, &'static str); 2] {
    [("abiVersion", "(): number"), ("version", "(): string")]
}

#[cfg(target_env = "ohos")]
mod ohos_napi {
    //! 阶段 5 在此用 ohos-rs 的宏注册导出函数：
    //!
    //! ```ignore
    //! #[napi_ohos::module_exports]
    //! fn init() {}
    //!
    //! #[napi_ohos::js_function]
    //! pub fn abi_version() -> i32 { fdxmp_core::ABI_VERSION as i32 }
    //! ```
    //!
    //! 启用步骤（阶段 5）：
    //! 1. 在 `Cargo.toml` 中加入 `napi-ohos`（git 依赖，见 crate README）；
    //! 2. 恢复 `use napi_ohos as _;`；
    //! 3. 用 ohos-rs 的宏导出 `abiVersion` / `version` 及后续接口。
    //!
    //! 当前保持最小编译单元：缺少 DevEco / hvigor 与 git 网络的环境下
    //! 仍可对 workspace 做宿主机构建验证。
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_match_design() {
        let table = exports();
        assert_eq!(table.len(), 2);
        assert_eq!(abi_version(), 1);
        assert!(!core_version().is_empty());
        assert_eq!(MODULE_NAME, "fdxmp_ohos_bridge");
    }
}
