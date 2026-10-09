package com.fdxmp.app.media

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import com.fdxmp.core.NativeBridge
import java.io.File

/**
 * 预览渲染：
 *   - [cacheSource] 把 MediaStore 的 content:// 复制成核心可读的本地文件（缓存复用）；
 *   - [render] 调 Rust 核心解码 + 调色，返回 Bitmap；
 *   - [decodeSource] 直接解一张降采样原图（用于"看原图"对比）。
 */
object PreviewRenderer {

    private val gpuLock = Any()

    private var gpuHandle: Long = 0L
    private var gpuPath: String? = null

    @Volatile
    private var gpuActive = false

    @Volatile
    private var gpuPreparing = false

    @Volatile
    private var gpuDisabled = false

    @Volatile
    private var lastError: String? = null

    /** 最近一次预览是否由 GPU 渲染。 */
    fun isGpuActive(): Boolean = gpuActive

    /** GPU 会话是否已就绪（可走 GPU 渲染）。 */
    fun isGpuReady(): Boolean = gpuHandle != 0L

    /** 是否已判定 GPU 不可用（初始化失败 / 超时）。 */
    fun isGpuDisabled(): Boolean = gpuDisabled

    /** 允许重新尝试初始化 GPU（用户手动切回 GPU 时调用）。 */
    fun resetGpuDisabled() {
        gpuDisabled = false
    }

    /** 标记 GPU 不可用并释放会话（例如初始化超时）。 */
    fun markGpuDisabled(reason: String) {
        synchronized(gpuLock) {
            gpuDisabled = true
            lastError = reason
            val handle = gpuHandle
            gpuHandle = 0L
            gpuPath = null
            if (handle != 0L) {
                runCatching { NativeBridge.nativeGpuSessionFree(handle) }
            }
        }
        gpuActive = false
    }

    /** 最近一次 GPU 失败原因。 */
    fun lastGpuError(): String? = lastError

    /**
     * 后台准备 GPU 会话（**阻塞式 JNI 调用**，必须在工作线程执行）。
     * 同一路径已就绪直接返回 true；失败 / 超时后置 `gpuDisabled`。
     */
    fun prepareGpu(path: String, maxEdge: Int = 1440): Boolean {
        synchronized(gpuLock) {
            if (gpuHandle != 0L && gpuPath == path) return true
            if (gpuPreparing || gpuDisabled) return gpuHandle != 0L
            gpuPreparing = true
        }
        val handle = runCatching {
            NativeBridge.nativeGpuSessionNew(path, maxEdge)
        }.getOrDefault(0L)
        synchronized(gpuLock) {
            gpuPreparing = false
            if (gpuDisabled) {
                if (handle != 0L) {
                    runCatching { NativeBridge.nativeGpuSessionFree(handle) }
                }
                return false
            }
            if (handle == 0L) {
                lastError = runCatching { NativeBridge.nativeGpuLastError() }.getOrNull()
                gpuDisabled = true
                return false
            }
            val previous = gpuHandle
            gpuHandle = handle
            gpuPath = path
            if (previous != 0L) {
                runCatching { NativeBridge.nativeGpuSessionFree(previous) }
            }
            return true
        }
    }

    /** 会话已就绪时用 GPU 渲染一帧；未就绪返回 null（调用方走 CPU）。 */
    fun renderGpuIfReady(path: String, paramsJson: String): Bitmap? {
        val handle = synchronized(gpuLock) {
            if (gpuHandle != 0L && gpuPath == path) gpuHandle else 0L
        }
        if (handle == 0L) return null
        val payload = runCatching {
            NativeBridge.nativeGpuSessionRender(handle, paramsJson)
        }.getOrNull()
        if (payload == null) {
            markGpuDisabled("渲染失败")
            return null
        }
        val bitmap = decodePayload(payload)
        gpuActive = bitmap != null
        return bitmap
    }

    /** GPU 后端名称（无会话返回 null）。 */
    fun gpuBackend(): String? {
        val handle = gpuHandle
        if (handle == 0L) return null
        return runCatching { NativeBridge.nativeGpuBackend(handle) }.getOrNull()
    }

    /** GPU 预览：同一路径复用会话，参数变化只重渲染。失败返回 null（调用方回退 CPU）。 */
    fun renderGpu(path: String, paramsJson: String, maxEdge: Int = 1440): Bitmap? {
        if (gpuHandle == 0L || gpuPath != path) {
            releaseGpu()
            gpuHandle = runCatching {
                NativeBridge.nativeGpuSessionNew(path, maxEdge)
            }.getOrDefault(0L)
            gpuPath = path
            if (gpuHandle == 0L) {
                gpuActive = false
                lastError = runCatching { NativeBridge.nativeGpuLastError() }.getOrNull()
                return null
            }
        }
        val payload = runCatching {
            NativeBridge.nativeGpuSessionRender(gpuHandle, paramsJson)
        }.getOrNull()
        if (payload == null) {
            releaseGpu()
            gpuActive = false
            return null
        }
        val bitmap = decodePayload(payload)
        gpuActive = bitmap != null
        return bitmap
    }

    /** 一次性 GPU 渲染（导出用，渲染完即释放会话）。 */
    fun renderGpuOnce(path: String, paramsJson: String, maxEdge: Int): Bitmap? {
        val handle = runCatching {
            NativeBridge.nativeGpuSessionNew(path, maxEdge)
        }.getOrDefault(0L)
        if (handle == 0L) return null
        return try {
            runCatching {
                NativeBridge.nativeGpuSessionRender(handle, paramsJson)
            }.getOrNull()?.let { decodePayload(it) }
        } finally {
            runCatching { NativeBridge.nativeGpuSessionFree(handle) }
        }
    }

    fun releaseGpu() {
        val handle = gpuHandle
        if (handle != 0L) {
            runCatching { NativeBridge.nativeGpuSessionFree(handle) }
        }
        gpuHandle = 0L
        gpuPath = null
        gpuActive = false
    }

    /**
     * 性能基准：各跑 [frames] 帧，返回（CPU 毫秒/帧, GPU 毫秒/帧）。
     *
     * CPU 路径包含解码；GPU 路径不含首次上传（会话常驻场景的真实开销）。
     * GPU 不可用时第二项为 -1。
     */
    fun benchmark(
        path: String,
        paramsJson: String,
        maxEdge: Int = 1440,
        frames: Int = 3,
    ): Pair<Double, Double> {
        var cpuNanos = 0L
        repeat(frames) {
            val start = System.nanoTime()
            render(path, paramsJson, maxEdge)
            cpuNanos += System.nanoTime() - start
        }
        val handle = synchronized(gpuLock) {
            if (gpuHandle != 0L && gpuPath == path) gpuHandle else 0L
        }
        if (handle == 0L) {
            return Pair(cpuNanos / 1e6 / frames, -1.0)
        }
        var gpuNanos = 0L
        // 预热一帧（首帧可能包含管线状态刷新）。
        runCatching { NativeBridge.nativeGpuSessionRender(handle, paramsJson) }
        repeat(frames) {
            val start = System.nanoTime()
            runCatching { NativeBridge.nativeGpuSessionRender(handle, paramsJson) }
            gpuNanos += System.nanoTime() - start
        }
        val cpuMs = cpuNanos / 1e6 / frames
        val gpuMs = if (gpuNanos > 0) gpuNanos / 1e6 / frames else -1.0
        return Pair(cpuMs, gpuMs)
    }

    fun cacheSource(context: Context, uri: Uri): File? {
        val dir = File(context.cacheDir, "edit_sources").apply { mkdirs() }
        val file = File(dir, "src_${uri.toString().hashCode().toUInt().toString(16)}.bin")
        if (file.exists() && file.length() > 0) return file
        return runCatching {
            context.contentResolver.openInputStream(uri)?.use { input ->
                file.outputStream().use { output -> input.copyTo(output) }
            }
            if (file.length() > 0) file else null
        }.getOrNull()
    }

    /** 调 Rust 核心渲染；失败返回 null（RAW 需要核心启用 raw feature）。 */
    fun render(path: String, paramsJson: String, maxEdge: Int = 1440): Bitmap? {
        val payload = runCatching {
            NativeBridge.nativeRenderPreview(path, paramsJson, maxEdge)
        }.getOrNull() ?: return null
        gpuActive = false
        return decodePayload(payload)
    }

    private fun decodePayload(payload: ByteArray): Bitmap? {
        if (payload.size < 8) return null

        val width = ((payload[0].toInt() and 0xFF) shl 24) or
            ((payload[1].toInt() and 0xFF) shl 16) or
            ((payload[2].toInt() and 0xFF) shl 8) or
            (payload[3].toInt() and 0xFF)
        val height = ((payload[4].toInt() and 0xFF) shl 24) or
            ((payload[5].toInt() and 0xFF) shl 16) or
            ((payload[6].toInt() and 0xFF) shl 8) or
            (payload[7].toInt() and 0xFF)
        if (width <= 0 || height <= 0) return null
        if (payload.size < 8 + width * height * 4) return null

        val pixels = IntArray(width * height)
        var offset = 8
        for (index in pixels.indices) {
            val r = payload[offset].toInt() and 0xFF
            val g = payload[offset + 1].toInt() and 0xFF
            val b = payload[offset + 2].toInt() and 0xFF
            pixels[index] = (0xFF shl 24) or (r shl 16) or (g shl 8) or b
            offset += 4
        }
        return runCatching {
            Bitmap.createBitmap(pixels, width, height, Bitmap.Config.ARGB_8888)
        }.getOrNull()
    }

    fun decodeSource(path: String, maxEdge: Int = 1440): Bitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(path, bounds)
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
        var sample = 1
        val longest = maxOf(bounds.outWidth, bounds.outHeight)
        while (longest / (sample * 2) >= maxEdge) sample *= 2
        val options = BitmapFactory.Options().apply {
            inSampleSize = sample
            inPreferredConfig = Bitmap.Config.ARGB_8888
        }
        return runCatching { BitmapFactory.decodeFile(path, options) }.getOrNull()
    }
}
