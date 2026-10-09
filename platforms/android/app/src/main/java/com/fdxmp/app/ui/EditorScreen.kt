package com.fdxmp.app.ui

import android.graphics.Bitmap
import android.net.Uri
import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.fdxmp.app.media.EditRecord
import com.fdxmp.app.media.EditStore
import com.fdxmp.app.media.PreviewRenderer
import java.io.File
import java.util.Locale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

private data class EditorSlider(
    val key: String,
    val label: String,
    val group: String,
    val range: ClosedFloatingPointRange<Float>,
)

private val EDITOR_SLIDERS = listOf(
    EditorSlider("exposure", "曝光", "光照", -5f..5f),
    EditorSlider("contrast", "对比度", "光照", -100f..100f),
    EditorSlider("highlights", "高光", "光照", -100f..100f),
    EditorSlider("shadows", "阴影", "光照", -100f..100f),
    EditorSlider("whites", "白色", "光照", -100f..100f),
    EditorSlider("blacks", "黑色", "光照", -100f..100f),
    EditorSlider("temperature", "色温", "颜色", -100f..100f),
    EditorSlider("tint", "色调", "颜色", -100f..100f),
    EditorSlider("vibrance", "自然饱和度", "颜色", -100f..100f),
    EditorSlider("saturation", "饱和度", "颜色", -100f..100f),
    EditorSlider("texture", "纹理", "效果", -100f..100f),
    EditorSlider("clarity", "清晰度", "效果", -100f..100f),
    EditorSlider("dehaze", "去雾", "效果", -100f..100f),
    EditorSlider("vignette", "暗角", "效果", -100f..100f),
    EditorSlider("grain", "颗粒", "效果", -100f..100f),
)

/**
 * 基础调色编辑界面（Android v1）：
 * Rust 核心出预览，15 项基础滑块，实时/释放渲染，原图对比，保存到编辑记录。
 * 曲线 / HSL / 蒙版 / 裁剪等后续接入。
 */
@Composable
fun EditorScreen(
    record: EditRecord,
    onClose: () -> Unit,
    onSaved: (EditRecord) -> Unit,
) {
    val context = LocalContext.current
    BackHandler(onBack = onClose)

    var values by remember(record.uri) { mutableStateOf(parseValues(record.paramsJson)) }
    var sourceFile by remember(record.uri) { mutableStateOf<File?>(null) }
    var originalBitmap by remember(record.uri) { mutableStateOf<Bitmap?>(null) }
    var previewBitmap by remember(record.uri) { mutableStateOf<Bitmap?>(null) }
    var isRendering by remember { mutableStateOf(false) }
    var showOriginal by remember { mutableStateOf(false) }
    var renderVersion by remember { mutableStateOf(0) }
    var failure by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(record.uri) {
        val file = withContext(Dispatchers.IO) {
            PreviewRenderer.cacheSource(context, Uri.parse(record.uri))
        }
        sourceFile = file
        originalBitmap = withContext(Dispatchers.IO) {
            file?.let { PreviewRenderer.decodeSource(it.path) }
        }
    }

    LaunchedEffect(sourceFile, renderVersion) {
        val file = sourceFile ?: return@LaunchedEffect
        isRendering = true
        failure = null
        val json = buildParamsJson(values)
        val bitmap = withContext(Dispatchers.Default) {
            PreviewRenderer.render(file.path, json, 1440)
        }
        previewBitmap = bitmap
        if (bitmap == null) {
            failure = "无法渲染这张图片（RAW 需要核心启用 raw 解码）"
        }
        isRendering = false
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .statusBarsPadding()
            .navigationBarsPadding(),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surface)
                .padding(horizontal = 4.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onClose) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "返回")
            }
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    record.name,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    style = MaterialTheme.typography.bodyMedium,
                )
                Text(
                    "基础调色 · ${record.presetName}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            TextButton(onClick = {
                values = List(EDITOR_SLIDERS.size) { 0f }
                renderVersion += 1
            }) { Text("重置") }
            TextButton(onClick = {
                val updated = record.copy(
                    paramsJson = buildParamsJson(values),
                    presetName = if (values.all { it == 0f }) "原片" else "自定义",
                    updatedAt = System.currentTimeMillis(),
                )
                EditStore.upsert(context, updated)
                Toast.makeText(context, "已保存到编辑记录", Toast.LENGTH_SHORT).show()
                onSaved(updated)
            }) { Text("保存") }
        }

        Box(
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
                .background(androidx.compose.ui.graphics.Color.Black),
            contentAlignment = Alignment.Center,
        ) {
            val bitmap = if (showOriginal) originalBitmap else previewBitmap
            if (bitmap != null) {
                Image(
                    bitmap = bitmap.asImageBitmap(),
                    contentDescription = record.name,
                    contentScale = ContentScale.Fit,
                    modifier = Modifier.fillMaxSize(),
                )
            } else if (failure != null) {
                Text(
                    failure ?: "",
                    color = androidx.compose.ui.graphics.Color(0xFFCCCCCC),
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(24.dp),
                )
            } else {
                CircularProgressIndicator()
            }

            if (isRendering) {
                LinearProgressIndicator(
                    modifier = Modifier.align(Alignment.TopCenter).fillMaxWidth(),
                )
            }

            TextButton(
                onClick = { showOriginal = !showOriginal },
                modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 4.dp),
            ) {
                Text(if (showOriginal) "看效果" else "看原图")
            }
        }

        Surface(
            color = MaterialTheme.colorScheme.surface,
            modifier = Modifier.fillMaxWidth().height(300.dp),
        ) {
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                contentPadding = androidx.compose.foundation.layout.PaddingValues(
                    horizontal = 16.dp,
                    vertical = 8.dp,
                ),
            ) {
                var currentGroup = ""
                EDITOR_SLIDERS.forEachIndexed { index, slider ->
                    if (slider.group != currentGroup) {
                        currentGroup = slider.group
                        item(key = "group-${slider.group}") {
                            Text(
                                slider.group,
                                style = MaterialTheme.typography.labelMedium,
                                color = MaterialTheme.colorScheme.primary,
                                modifier = Modifier.padding(top = 8.dp, bottom = 2.dp),
                            )
                        }
                    }
                    item(key = slider.key) {
                        Column {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Text(
                                    slider.label,
                                    style = MaterialTheme.typography.bodySmall,
                                    modifier = Modifier.weight(1f),
                                )
                                Text(
                                    formatValue(values[index]),
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                            Slider(
                                value = values[index],
                                onValueChange = { newValue ->
                                    values = values.toMutableList().also { it[index] = newValue }
                                },
                                onValueChangeFinished = { renderVersion += 1 },
                                valueRange = slider.range,
                            )
                        }
                    }
                }
            }
        }
    }
}

private fun parseValues(paramsJson: String): List<Float> {
    val defaults = List(EDITOR_SLIDERS.size) { 0f }
    val root = runCatching { JSONObject(paramsJson) }.getOrNull() ?: return defaults
    val groups = mapOf(
        "light" to listOf("exposure", "contrast", "highlights", "shadows", "whites", "blacks"),
        "color" to listOf("temperature", "tint", "vibrance", "saturation"),
        "effects" to listOf("texture", "clarity", "dehaze", "vignette", "grain"),
    )
    return EDITOR_SLIDERS.map { slider ->
        val group = groups.entries.firstOrNull { slider.key in it.value }?.key
        val json = group?.let { root.optJSONObject(it) }
        json?.optDouble(slider.key, 0.0)?.toFloat() ?: 0f
    }
}

private fun buildParamsJson(values: List<Float>): String {
    fun value(key: String): Double =
        values[EDITOR_SLIDERS.indexOfFirst { it.key == key }].toDouble()

    return JSONObject().apply {
        put("version", 1)
        put(
            "light",
            JSONObject().apply {
                put("exposure", value("exposure"))
                put("contrast", value("contrast"))
                put("highlights", value("highlights"))
                put("shadows", value("shadows"))
                put("whites", value("whites"))
                put("blacks", value("blacks"))
            },
        )
        put(
            "color",
            JSONObject().apply {
                put("temperature", value("temperature"))
                put("tint", value("tint"))
                put("vibrance", value("vibrance"))
                put("saturation", value("saturation"))
            },
        )
        put(
            "effects",
            JSONObject().apply {
                put("texture", value("texture"))
                put("clarity", value("clarity"))
                put("dehaze", value("dehaze"))
                put("vignette", value("vignette"))
                put("grain", value("grain"))
            },
        )
    }.toString()
}

private fun formatValue(value: Float): String =
    String.format(Locale.US, "%+.2f", value)
