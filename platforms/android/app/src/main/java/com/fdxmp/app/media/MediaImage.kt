package com.fdxmp.app.media

import android.net.Uri
import java.util.Locale

/**
 * 相册中的一张图片。
 *
 * 数据来自 MediaStore；RAW 文件同样会被列出，但 Android 平台解码器无法预览，
 * 全屏预览会提示需要 Rust 核心解码（阶段 6 接入）。
 */
data class MediaImage(
    val id: Long,
    val uri: Uri,
    val name: String,
    val mimeType: String,
    val sizeBytes: Long,
    val dateAddedSeconds: Long,
    val width: Int,
    val height: Int,
) {
    val isRaw: Boolean = isRawName(name) || isRawMime(mimeType)

    val displaySize: String
        get() = when {
            sizeBytes >= 1024L * 1024 * 1024 ->
                String.format(Locale.US, "%.1f GB", sizeBytes / 1024.0 / 1024 / 1024)

            sizeBytes >= 1024L * 1024 ->
                String.format(Locale.US, "%.1f MB", sizeBytes / 1024.0 / 1024)

            sizeBytes >= 1024L ->
                String.format(Locale.US, "%.0f KB", sizeBytes / 1024.0)

            else -> "$sizeBytes B"
        }

    val pixelSize: String
        get() = if (width > 0 && height > 0) "$width × $height" else "尺寸未知"

    companion object {
        private val RAW_EXTENSIONS = listOf(
            ".dng", ".cr2", ".cr3", ".crw", ".nef", ".nrw", ".arw", ".srf", ".sr2",
            ".raf", ".rw2", ".orf", ".pef", ".srw", ".raw", ".rwl", ".x3f", ".3fr",
            ".fff", ".iiq", ".mos", ".mrw", ".erf", ".kdc", ".dcr",
        )

        private fun isRawName(name: String): Boolean =
            RAW_EXTENSIONS.any { name.endsWith(it, ignoreCase = true) }

        private fun isRawMime(mime: String): Boolean {
            val value = mime.lowercase(Locale.US)
            return value.contains("raw") || value.contains("dng") || value.contains("x-adobe")
        }
    }
}
