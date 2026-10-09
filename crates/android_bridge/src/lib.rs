//! # fdxmp-android-bridge
//!
//! Android（Kotlin / Jetpack Compose）→ Rust 核心的 JNI 桥接层。
//!
//! Kotlin 侧对应类：`com.fdxmp.core.NativeBridge`
//!
//! | Kotlin 方法 | JNI 符号 | 返回 |
//! | --- | --- | --- |
//! | `external fun nativeAbiVersion(): Int` | `Java_com_fdxmp_core_NativeBridge_nativeAbiVersion` | ABI 版本 |
//! | `external fun nativeVersion(): String` | `Java_com_fdxmp_core_NativeBridge_nativeVersion` | 核心版本 |
//!
//! 完整解码 / 编辑 / 导出接口在阶段 5 展开（统一走 `fdxmp-ffi` 的 C ABI 语义）。

/// Kotlin 侧桥接类全限定名。
pub const JNI_CLASS: &str = "com.fdxmp.core.NativeBridge";

/// ABI 版本
pub fn abi_version() -> jni::sys::jint {
    fdxmp_core::ABI_VERSION as jni::sys::jint
}

/// 核心版本字符串。
pub fn core_version() -> &'static str {
    fdxmp_core::version()
}

/// 导出符号与 JNI 签名对照表（供构建期校验脚本使用）。
pub fn signature_table() -> [(&'static str, &'static str); 2] {
    [
        ("Java_com_fdxmp_core_NativeBridge_nativeAbiVersion", "()I"),
        (
            "Java_com_fdxmp_core_NativeBridge_nativeVersion",
            "()Ljava/lang/String;",
        ),
    ]
}

#[cfg(target_os = "android")]
mod android_jni {
    //! 真机 / 模拟器上的 JNI 导出（仅在 Android 目标下编译）。

    use jni::objects::JClass;
    use jni::sys::{jint, jstring};
    use jni::EnvUnowned;

    #[no_mangle]
    pub extern "system" fn Java_com_fdxmp_core_NativeBridge_nativeAbiVersion<'caller>(
        _env: EnvUnowned<'caller>,
        _class: JClass<'caller>,
    ) -> jint {
        fdxmp_core::ABI_VERSION as jint
    }

    #[no_mangle]
    pub extern "system" fn Java_com_fdxmp_core_NativeBridge_nativeVersion<'caller>(
        mut env: EnvUnowned<'caller>,
        _class: JClass<'caller>,
    ) -> jstring {
        env.with_env(|env| -> jni::errors::Result<_> {
            Ok(env.new_string(fdxmp_core::version())?.into_raw())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_table_matches_exported_symbols() {
        let table = signature_table();
        assert_eq!(table.len(), 2);
        assert!(table[0].0.starts_with("Java_com_fdxmp_core_NativeBridge_"));
        assert_eq!(abi_version(), 1);
        assert!(!core_version().is_empty());
    }
}
