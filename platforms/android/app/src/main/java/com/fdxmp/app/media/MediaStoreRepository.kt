package com.fdxmp.app.media

import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.util.LruCache
import android.util.Size
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * MediaStore 访问层：相册查询、缩略图 / 预览图解码、相机照片写入。
 *
 * 缩略图有一层进程内 LruCache；真正的磁盘缓存与 RAW 解码由 Rust 核心负责
 * （见 docs/SPEC.md 缓存管理），这里只做平台侧渲染。
 */
object MediaStoreRepository {

    /** 缩略图缓存，容量取可用堆内存的 1/8。 */
    private val thumbnailCache = object : LruCache<String, Bitmap>(cacheSizeKb()) {
        override fun sizeOf(key: String, value: Bitmap): Int = value.byteCount / 1024
    }

    private fun cacheSizeKb(): Int {
        val maxKb = (Runtime.getRuntime().maxMemory() / 1024L).toInt()
        return (maxKb / 8).coerceAtLeast(16 * 1024)
    }

    private val imageCollection: Uri
        get() = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)
        } else {
            MediaStore.Images.Media.EXTERNAL_CONTENT_URI
        }

    /** 查询设备上的全部图片，按加入时间倒序。 */
    suspend fun queryImages(context: Context): List<MediaImage> = withContext(Dispatchers.IO) {
        val projection = arrayOf(
            MediaStore.Images.Media._ID,
            MediaStore.Images.Media.DISPLAY_NAME,
            MediaStore.Images.Media.MIME_TYPE,
            MediaStore.Images.Media.SIZE,
            MediaStore.Images.Media.DATE_ADDED,
            MediaStore.Images.Media.WIDTH,
            MediaStore.Images.Media.HEIGHT,
        )
        val collection = imageCollection
        val result = ArrayList<MediaImage>()

        context.contentResolver.query(
            collection,
            projection,
            null,
            null,
            "${MediaStore.Images.Media.DATE_ADDED} DESC",
        )?.use { cursor ->
            val idColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media._ID)
            val nameColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.DISPLAY_NAME)
            val mimeColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.MIME_TYPE)
            val sizeColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.SIZE)
            val dateColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.DATE_ADDED)
            val widthColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.WIDTH)
            val heightColumn = cursor.getColumnIndexOrThrow(MediaStore.Images.Media.HEIGHT)

            while (cursor.moveToNext()) {
                val id = cursor.getLong(idColumn)
                result += MediaImage(
                    id = id,
                    uri = ContentUris.withAppendedId(collection, id),
                    name = cursor.getString(nameColumn) ?: "未命名_$id",
                    mimeType = cursor.getString(mimeColumn) ?: "image/*",
                    sizeBytes = cursor.getLong(sizeColumn),
                    dateAddedSeconds = cursor.getLong(dateColumn),
                    width = cursor.getInt(widthColumn),
                    height = cursor.getInt(heightColumn),
                )
            }
        }
        result
    }

    /** 网格缩略图（带内存缓存）。RAW 等平台解不了的格式返回 null。 */
    suspend fun loadThumbnail(
        context: Context,
        image: MediaImage,
        sizePx: Int = 320,
    ): Bitmap? = withContext(Dispatchers.IO) {
        val key = "${image.id}@$sizePx"
        thumbnailCache.get(key)?.let { return@withContext it }

        val bitmap = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            runCatching {
                context.contentResolver.loadThumbnail(image.uri, Size(sizePx, sizePx), null)
            }.getOrNull()
        } else {
            decodeSampled(context, image.uri, sizePx)
        } ?: return@withContext null

        thumbnailCache.put(key, bitmap)
        bitmap
    }

    /** 全屏预览用的大图，按最长边降采样，避免整张 50MP 图进内存。 */
    suspend fun loadPreview(
        context: Context,
        image: MediaImage,
        maxPx: Int = 2048,
    ): Bitmap? = withContext(Dispatchers.IO) {
        decodeSampled(context, image.uri, maxPx, highQuality = true)
    }

    private fun decodeSampled(
        context: Context,
        uri: Uri,
        maxPx: Int,
        highQuality: Boolean = false,
    ): Bitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        runCatching {
            context.contentResolver.openInputStream(uri)?.use {
                BitmapFactory.decodeStream(it, null, bounds)
            }
        }
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null

        var sample = 1
        val longest = maxOf(bounds.outWidth, bounds.outHeight)
        while (longest / (sample * 2) >= maxPx) sample *= 2

        val options = BitmapFactory.Options().apply {
            inSampleSize = sample
            inPreferredConfig = if (highQuality) Bitmap.Config.ARGB_8888 else Bitmap.Config.RGB_565
        }
        return runCatching {
            context.contentResolver.openInputStream(uri)?.use {
                BitmapFactory.decodeStream(it, null, options)
            }
        }.getOrNull()
    }

    /** 相机拍照前先占一个 MediaStore 记录，拿到可写的 output Uri。 */
    fun createCameraImageUri(context: Context): Uri? {
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
        return runCatching {
            context.contentResolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values)
        }.getOrNull()
    }

    /** 拍照成功：清除 pending，让照片出现在相册与媒体库里。 */
    fun finalizeCameraImage(context: Context, uri: Uri) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val values = ContentValues().apply {
                put(MediaStore.Images.Media.IS_PENDING, 0)
            }
            runCatching { context.contentResolver.update(uri, values, null, null) }
        }
    }

    /** 拍照取消：删掉占位记录。 */
    fun discardCameraImage(context: Context, uri: Uri) {
        runCatching { context.contentResolver.delete(uri, null, null) }
    }
}
