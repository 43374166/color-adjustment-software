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
