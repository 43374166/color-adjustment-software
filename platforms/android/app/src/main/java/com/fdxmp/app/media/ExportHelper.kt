package com.fdxmp.app.media

import android.content.ContentValues
import android.content.Context
import android.graphics.Bitmap
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** 导出到系统相册（Pictures/fdxmp）。 */
object ExportHelper {
    fun saveToGallery(context: Context, bitmap: Bitmap): Boolean {
        val name = "FDXMP_" +
            SimpleDateFormat("yyyyMMdd_HHmmss", Locale.US).format(Date()) +
            ".jpg"
        val values = ContentValues().apply {
            put(MediaStore.Images.Media.DISPLAY_NAME, name)
            put(MediaStore.Images.Media.MIME_TYPE, "image/jpeg")
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                put(
                    MediaStore.Images.Media.RELATIVE_PATH,
                    Environment.DIRECTORY_PICTURES + "/fdxmp",
                )
                put(MediaStore.Images.Media.IS_PENDING, 1)
            }
        }
        val resolver = context.contentResolver
        val uri = runCatching {
            resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values)
        }.getOrNull() ?: return false
        return runCatching {
            resolver.openOutputStream(uri)?.use { output ->
                bitmap.compress(Bitmap.CompressFormat.JPEG, 95, output)
            } ?: return false
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                val done = ContentValues().apply {
                    put(MediaStore.Images.Media.IS_PENDING, 0)
                }
                resolver.update(uri, done, null, null)
            }
            true
        }.getOrElse {
            runCatching { resolver.delete(uri, null, null) }
            false
        }
    }
}
