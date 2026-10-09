package com.fdxmp.core

/**
 * Rust 核心桥接（对应 crates/android_bridge）。
 *
 * 产物：libfdxmp_android_bridge.so（cargo ndk 构建，见 packaging/android/build.ps1）。
 * 完整解码 / 编辑 / 导出接口在阶段 5 展开（统一走 fdxmp-ffi 的 C ABI 语义）。
 */
object NativeBridge {
    init {
        System.loadLibrary("fdxmp_android_bridge")
    }

    external fun nativeAbiVersion(): Int
    external fun nativeVersion(): String
}

