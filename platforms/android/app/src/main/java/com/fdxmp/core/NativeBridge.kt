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

    /** 创建常驻 GPU 预览会话（源图只上传一次），失败返回 0。 */
    external fun nativeGpuSessionNew(path: String, maxEdge: Int): Long

    /** 用 GPU 会话按参数渲染一帧；返回格式与 [nativeRenderPreview] 相同。 */
    external fun nativeGpuSessionRender(handle: Long, paramsJson: String): ByteArray?

    /** 释放 GPU 会话。 */
    external fun nativeGpuSessionFree(handle: Long)

    /** GPU 后端名称（vulkan / gles / …）。 */
    external fun nativeGpuBackend(handle: Long): String?

    /** 最近一次 GPU 会话创建失败的原因（成功时为 null）。 */
    external fun nativeGpuLastError(): String?
}
