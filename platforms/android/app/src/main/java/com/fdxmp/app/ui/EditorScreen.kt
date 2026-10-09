package com.fdxmp.app.ui

import android.graphics.Bitmap
import android.net.Uri
import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Redo
import androidx.compose.material.icons.automirrored.filled.Undo
import androidx.compose.material.icons.filled.AutoAwesome
import androidx.compose.material.icons.filled.Brush
import androidx.compose.material.icons.filled.Crop
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.filled.Tune
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
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

private enum class EditCategory(val label: String, val groupKey: String) {
    Light("亮度", "light"),
    Color("颜色", "color"),
    Effects("效果", "effects"),
}

private enum class EditorTool(val label: String, val icon: ImageVector) {
    Presets("预设", Icons.Filled.AutoAwesome),
    Crop("裁切", Icons.Filled.Crop),
    Edit("编辑", Icons.Filled.Tune),
    Mask("蒙版", Icons.Filled.Brush),
}

private data class EditorSlider(
    val key: String,
    val label: String,
    val category: EditCategory,
    val range: ClosedFloatingPointRange<Float>,
)

private val EDITOR_SLIDERS = listOf(
    EditorSlider("exposure", "曝光度", EditCategory.Light, -5f..5f),
    EditorSlider("contrast", "对比度", EditCategory.Light, -100f..100f),
    EditorSlider("highlights", "高光", EditCategory.Light, -100f..100f),
    EditorSlider("shadows", "阴影", EditCategory.Light, -100f..100f),
    EditorSlider("whites", "白色", EditCategory.Light, -100f..100f),
    EditorSlider("blacks", "黑色", EditCategory.Light, -100f..100f),
    EditorSlider("temperature", "色温", EditCategory.Color, -100f..100f),
    EditorSlider("tint", "色调", EditCategory.Color, -100f..100f),
    EditorSlider("vibrance", "自然饱和度", EditCategory.Color, -100f..100f),
    EditorSlider("saturation", "饱和度", EditCategory.Color, -100f..100f),
    EditorSlider("texture", "纹理", EditCategory.Effects, -100f..100f),
    EditorSlider("clarity", "清晰度", EditCategory.Effects, -100f..100f),
    EditorSlider("dehaze", "去雾", EditCategory.Effects, -100f..100f),
    EditorSlider("vignette", "暗角", EditCategory.Effects, -100f..100f),
    EditorSlider("grain", "颗粒", EditCategory.Effects, -100f..100f),
)

private data class BuiltinPreset(
    val name: String,
    val description: String,
    val params: Map<String, Float>,
)

private val BUILTIN_PRESETS = listOf(
    BuiltinPreset("原片", "全部归零", emptyMap()),
    BuiltinPreset(
        "人像",
        "柔和肤色 · 提亮阴影",
        mapOf(
            "contrast" to 8f, "highlights" to -15f, "shadows" to 20f,
            "vibrance" to 15f, "temperature" to 6f, "clarity" to -5f,
        ),
    ),
    BuiltinPreset(
        "风景",
        "通透 · 强化色彩层次",
        mapOf(
            "contrast" to 15f, "highlights" to -25f, "shadows" to 15f,
            "vibrance" to 25f, "saturation" to 8f, "clarity" to 12f,
        ),
    ),
    BuiltinPreset(
        "黑白",
        "去色 · 高对比",
        mapOf("saturation" to -100f, "contrast" to 18f, "clarity" to 10f),
    ),
    BuiltinPreset(
        "电影",
        "青橙 · 颗粒质感",
        mapOf(
            "contrast" to 12f, "highlights" to -20f, "shadows" to -10f,
            "temperature" to -6f, "vibrance" to 10f, "grain" to 18f,
        ),
    ),
    BuiltinPreset(
        "HDR",
        "压高光 · 提阴影",
        mapOf(
            "highlights" to -40f, "shadows" to 35f, "whites" to 10f,
            "blacks" to -10f, "clarity" to 15f, "vibrance" to 12f,
        ),
    ),
)

/**
 * 手机 Lightroom 风格的编辑界面（Android）：
 * 顶部工具 + 直方图 + 大图预览 + 底部「预设 / 裁切 / 编辑 / 蒙版」。
 * 长按图片看原图；曲线、裁切、蒙版面板后续接入。
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
    var histogram by remember { mutableStateOf<HistogramData?>(null) }
    var isRendering by remember { mutableStateOf(false) }
    var pressingOriginal by remember { mutableStateOf(false) }
    var renderVersion by remember { mutableStateOf(0) }
    var failure by remember { mutableStateOf<String?>(null) }
    var tool by remember { mutableStateOf(EditorTool.Edit) }
    var category by remember { mutableStateOf(EditCategory.Light) }
    var presetName by remember(record.uri) { mutableStateOf(record.presetName) }
    var showInfo by remember { mutableStateOf(false) }
    var dragSnapshot by remember { mutableStateOf<List<Float>?>(null) }
    val undoStack = remember { mutableStateListOf<List<Float>>() }
    val redoStack = remember { mutableStateListOf<List<Float>>() }

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

    LaunchedEffect(previewBitmap) {
        val bitmap = previewBitmap
        histogram = if (bitmap != null) computeHistogram(bitmap) else null
    }

    fun pushHistory(before: List<Float>) {
        undoStack.add(before)
        if (undoStack.size > 40) undoStack.removeAt(0)
        redoStack.clear()
    }

    fun applyPreset(preset: BuiltinPreset) {
        pushHistory(values)
        values = List(EDITOR_SLIDERS.size) { index ->
            preset.params[EDITOR_SLIDERS[index].key] ?: 0f
        }
        presetName = preset.name
        renderVersion += 1
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .statusBarsPadding()
            .navigationBarsPadding(),
    ) {
        EditorTopBar(
            canUndo = undoStack.isNotEmpty(),
            canRedo = redoStack.isNotEmpty(),
            infoActive = showInfo,
            onBack = onClose,
            onInfo = { showInfo = !showInfo },
            onUndo = {
                if (undoStack.isNotEmpty()) {
                    redoStack.add(values)
                    values = undoStack.removeAt(undoStack.lastIndex)
                    renderVersion += 1
                }
            },
            onRedo = {
                if (redoStack.isNotEmpty()) {
                    undoStack.add(values)
                    values = redoStack.removeAt(redoStack.lastIndex)
                    renderVersion += 1
                }
            },
            onSave = {
                val updated = record.copy(
                    paramsJson = buildParamsJson(values),
                    presetName = presetName,
                    updatedAt = System.currentTimeMillis(),
                )
                EditStore.upsert(context, updated)
                Toast.makeText(context, "已保存到编辑记录", Toast.LENGTH_SHORT).show()
                onSaved(updated)
            },
        )

        Box(
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
                .background(Color.Black),
            contentAlignment = Alignment.Center,
        ) {
            val bitmap = if (pressingOriginal) originalBitmap else previewBitmap
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
                    color = Color(0xFFCCCCCC),
                    style = MaterialTheme.typography.bodySmall,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(24.dp),
                )
            } else {
                CircularProgressIndicator()
            }

            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .pointerInput(record.uri) {
                        detectTapGestures(
                            onPress = {
                                pressingOriginal = true
                                tryAwaitRelease()
                                pressingOriginal = false
                            },
                        )
                    },
            )

            HistogramOverlay(
                data = histogram,
                modifier = Modifier
                    .align(Alignment.TopCenter)
                    .fillMaxWidth(0.84f)
                    .height(80.dp)
                    .padding(top = 8.dp),
            )

            if (pressingOriginal) {
                Text(
                    "原图",
                    color = Color.White,
                    style = MaterialTheme.typography.labelMedium,
                    modifier = Modifier
                        .align(Alignment.TopEnd)
                        .padding(10.dp)
                        .background(Color(0x99000000), RoundedCornerShape(4.dp))
                        .padding(horizontal = 8.dp, vertical = 3.dp),
                )
            }

            if (showInfo) {
                Text(
                    buildString {
                        append(record.name)
                        append(" · ")
                        append(viewportInfo(values))
                    },
                    color = Color(0xFFDDDDDD),
                    style = MaterialTheme.typography.labelSmall,
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .fillMaxWidth()
                        .background(Color(0x99000000))
                        .padding(horizontal = 12.dp, vertical = 6.dp),
                )
            }

            if (isRendering) {
                LinearProgressIndicator(
                    modifier = Modifier.align(Alignment.TopCenter).fillMaxWidth(),
                )
            }
        }

        Surface(color = MaterialTheme.colorScheme.surface, modifier = Modifier.fillMaxWidth()) {
            Column {
                Box(modifier = Modifier.fillMaxWidth().height(278.dp)) {
                    when (tool) {
                        EditorTool.Edit -> Column(modifier = Modifier.fillMaxSize()) {
                            PanelHeader(
                                presetName = presetName,
                                onCurve = {
                                    Toast.makeText(
                                        context,
                                        "曲线面板是下一步：RGB / 参数曲线",
                                        Toast.LENGTH_SHORT,
                                    ).show()
                                },
                                onReset = {
                                    pushHistory(values)
                                    values = List(EDITOR_SLIDERS.size) { 0f }
                                    presetName = "原片"
                                    renderVersion += 1
                                },
                            )
                            SlidersPanel(
                                category = category,
                                values = values,
                                modifier = Modifier.weight(1f),
                                onValueChange = { index, newValue ->
                                    if (dragSnapshot == null) dragSnapshot = values
                                    values = values.toMutableList().also { it[index] = newValue }
                                },
                                onValueChangeFinished = {
                                    dragSnapshot?.let { before ->
                                        if (before != values) pushHistory(before)
                                    }
                                    dragSnapshot = null
                                    presetName = "自定义"
                                    renderVersion += 1
                                },
                            )
                            CategoryRow(
                                selected = category,
                                onSelect = { category = it },
                            )
                        }

                        EditorTool.Presets -> PresetPanel(
                            current = presetName,
                            onPick = ::applyPreset,
                        )

                        EditorTool.Crop -> ToolPlaceholder(
                            title = "裁切",
                            detail = "自由裁剪 / 固定比例 / 旋转 / 翻转，下一步接入",
                        )

                        EditorTool.Mask -> ToolPlaceholder(
                            title = "蒙版",
                            detail = "画笔 / 线性渐变 / 径向渐变 / 颜色范围 / AI，下一步接入",
                        )
                    }
                }

                EditorToolBar(
                    selected = tool,
                    onSelect = { tool = it },
                )
            }
        }
    }
}

/* ------------------------------------------------------------ 顶部栏 */

@Composable
private fun EditorTopBar(
    canUndo: Boolean,
    canRedo: Boolean,
    infoActive: Boolean,
    onBack: () -> Unit,
    onInfo: () -> Unit,
    onUndo: () -> Unit,
    onRedo: () -> Unit,
    onSave: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surface)
            .padding(horizontal = 4.dp, vertical = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconButton(onClick = onBack) {
            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "返回")
        }
        IconButton(onClick = onInfo) {
            Icon(
                Icons.Filled.Info,
                contentDescription = "信息",
                tint = if (infoActive) {
                    MaterialTheme.colorScheme.primary
                } else {
                    MaterialTheme.colorScheme.onSurface
                },
            )
        }
        Spacer(Modifier.weight(1f))
        IconButton(onClick = onUndo, enabled = canUndo) {
            Icon(Icons.AutoMirrored.Filled.Undo, contentDescription = "撤销")
        }
        IconButton(onClick = onRedo, enabled = canRedo) {
            Icon(Icons.AutoMirrored.Filled.Redo, contentDescription = "重做")
        }
        IconButton(onClick = onSave) {
            Icon(
                Icons.Filled.Save,
                contentDescription = "保存",
                tint = MaterialTheme.colorScheme.primary,
            )
        }
    }
}

/* ------------------------------------------------------------ 编辑面板 */

@Composable
private fun PanelHeader(
    presetName: String,
    onCurve: () -> Unit,
    onReset: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(start = 16.dp, end = 8.dp, top = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            presetName,
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier.weight(1f),
        )
        TextButton(onClick = onReset) { Text("重置") }
        OutlinedButton(
            onClick = onCurve,
            shape = RoundedCornerShape(10.dp),
        ) { Text("曲线") }
    }
}

@Composable
private fun SlidersPanel(
    category: EditCategory,
    values: List<Float>,
    modifier: Modifier = Modifier,
    onValueChange: (Int, Float) -> Unit,
    onValueChangeFinished: () -> Unit,
) {
    val sliders = remember(category) {
        EDITOR_SLIDERS.withIndex().filter { it.value.category == category }
    }
    LazyColumn(
        modifier = modifier.fillMaxWidth(),
        contentPadding = PaddingValues(horizontal = 16.dp),
    ) {
        items(items = sliders, key = { it.value.key }) { indexed ->
            val index = indexed.index
            val slider = indexed.value
            Column {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        slider.label,
                        style = MaterialTheme.typography.bodyMedium,
                        modifier = Modifier.weight(1f),
                    )
                    Text(
                        formatValue(values[index]),
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Slider(
                    value = values[index],
                    onValueChange = { onValueChange(index, it) },
                    onValueChangeFinished = onValueChangeFinished,
                    valueRange = slider.range,
                )
            }
        }
    }
}

@Composable
private fun CategoryRow(
    selected: EditCategory,
    onSelect: (EditCategory) -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 2.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        EditCategory.entries.forEach { category ->
            val active = category == selected
            Text(
                category.label,
                textAlign = TextAlign.Center,
                style = MaterialTheme.typography.labelMedium,
                color = if (active) Color.White else MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier
                    .weight(1f)
                    .clip(RoundedCornerShape(10.dp))
                    .background(
                        if (active) MaterialTheme.colorScheme.primary else Color(0xFF1E1E1E),
                    )
                    .clickable { onSelect(category) }
                    .padding(vertical = 9.dp),
            )
        }
    }
}

@Composable
private fun PresetPanel(
    current: String,
    onPick: (BuiltinPreset) -> Unit,
) {
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
    ) {
        items(items = BUILTIN_PRESETS, key = { it.name }) { preset ->
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(10.dp))
                    .background(
                        if (preset.name == current) {
                            MaterialTheme.colorScheme.primary.copy(alpha = 0.18f)
                        } else {
                            Color.Transparent
                        },
                    )
                    .clickable { onPick(preset) }
                    .padding(horizontal = 12.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(preset.name, style = MaterialTheme.typography.bodyMedium)
                    Text(
                        preset.description,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                if (preset.name == current) {
                    Text(
                        "当前",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.primary,
                    )
                }
            }
        }
    }
}

@Composable
private fun ToolPlaceholder(title: String, detail: String) {
    Column(
        modifier = Modifier.fillMaxSize().padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(title, style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(8.dp))
        Text(
            detail,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
    }
}

@Composable
private fun EditorToolBar(
    selected: EditorTool,
    onSelect: (EditorTool) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surfaceVariant)
            .padding(vertical = 6.dp),
        horizontalArrangement = Arrangement.SpaceEvenly,
    ) {
        EditorTool.entries.forEach { item ->
            val active = item == selected
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                modifier = Modifier
                    .weight(1f)
                    .clip(RoundedCornerShape(10.dp))
                    .clickable { onSelect(item) }
                    .padding(vertical = 4.dp),
            ) {
                Icon(
                    item.icon,
                    contentDescription = item.label,
                    tint = if (active) {
                        MaterialTheme.colorScheme.primary
                    } else {
                        MaterialTheme.colorScheme.onSurfaceVariant
                    },
                    modifier = Modifier.size(22.dp),
                )
                Spacer(Modifier.height(2.dp))
                Text(
                    item.label,
                    style = MaterialTheme.typography.labelSmall,
                    color = if (active) {
                        MaterialTheme.colorScheme.primary
                    } else {
                        MaterialTheme.colorScheme.onSurfaceVariant
                    },
                )
            }
        }
    }
}

/* ------------------------------------------------------------ 参数工具 */

private fun parseValues(paramsJson: String): List<Float> {
    val root = runCatching { JSONObject(paramsJson) }.getOrNull()
        ?: return List(EDITOR_SLIDERS.size) { 0f }
    return EDITOR_SLIDERS.map { slider ->
        root.optJSONObject(slider.category.groupKey)
            ?.optDouble(slider.key, 0.0)
            ?.toFloat()
            ?: 0f
    }
}

private fun buildParamsJson(values: List<Float>): String {
    fun value(key: String): Double =
        values[EDITOR_SLIDERS.indexOfFirst { it.key == key }].toDouble()

    val grouped = EditCategory.entries.associateWith { category ->
        EDITOR_SLIDERS.filter { it.category == category }.map { it.key }
    }
    return JSONObject().apply {
        put("version", 1)
        grouped.forEach { (category, keys) ->
            put(
                category.groupKey,
                JSONObject().apply {
                    keys.forEach { key -> put(key, value(key)) }
                },
            )
        }
    }.toString()
}

private fun viewportInfo(values: List<Float>): String {
    val changed = values.count { it != 0f }
    return if (changed == 0) "未调整" else "已调整 $changed 项"
}

private fun formatValue(value: Float): String =
    String.format(Locale.US, "%+.2f", value)
