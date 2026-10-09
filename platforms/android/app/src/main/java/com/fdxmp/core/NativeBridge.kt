package com.fdxmp.core

/**
 * Rust 核心桥接（对应 crates/android_bridge）。
 *
 * 产物：libfdxmp_android_bridge.so（cargo ndk 构建，见 packaging/android/build.ps1）。
 * 当前暴露：核心版本 / ABI 版本 / CPU 预览渲染；其余解码、导出接口后续展开
 * （统一走 fdxmp-ffi 的 C ABI 语义）。
 */
object NativeBridge {
    init {
        System.loadLibrary("fdxmp_android_bridge")
    }

    external fun nativeAbiVersion(): Int
    external fun nativeVersion(): String

    /**
     * 解码 [path] 并按 [paramsJson]（EditParams JSON）渲染到最长边 [maxEdge]。
     *
     * 返回：前 8 字节为大端 width / height，其后为行优先 RGBA8 像素；
     * 失败抛 [IllegalStateException]。
     */
    external fun nativeRenderPreview(path: String, paramsJson: String, maxEdge: Int): ByteArray?
}
