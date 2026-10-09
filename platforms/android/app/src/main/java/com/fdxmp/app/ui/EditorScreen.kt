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
import androidx.compose.material.icons.filled.CenterFocusStrong
import androidx.compose.material.icons.filled.Details
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.LightMode
import androidx.compose.material.icons.filled.PhotoCamera
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.filled.Timeline
import androidx.compose.material.icons.filled.Tune
import androidx.compose.material.icons.filled.WaterDrop
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
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
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
import com.fdxmp.app.media.ExportHelper
import com.fdxmp.app.media.PreviewRenderer
import java.io.File
import java.util.Locale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

private enum class EditCategory(val label: String, val groupKey: String) {
    Light("亮度", "light"),
    Color("颜色", "color"),
    Effects("效果", "effects"),
    Detail("细节", "detail"),
    Optics("镜头", "lens"),
}

private enum class EffectTab(val label: String) {
    Effects("效果"),
    Vignette("晕影"),
    Grain("颗粒"),
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
    val defaultValue: Float = 0f,
    val effectTab: EffectTab? = null,
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
    EditorSlider("texture", "纹理", EditCategory.Effects, -100f..100f, effectTab = EffectTab.Effects),
    EditorSlider("clarity", "清晰度", EditCategory.Effects, -100f..100f, effectTab = EffectTab.Effects),
    EditorSlider("dehaze", "去雾", EditCategory.Effects, -100f..100f, effectTab = EffectTab.Effects),
    EditorSlider(
        "vignette", "暗角", EditCategory.Effects, -100f..100f,
        effectTab = EffectTab.Vignette,
    ),
    EditorSlider(
        "vignette_midpoint", "中点", EditCategory.Effects, 0f..100f,
        defaultValue = 50f, effectTab = EffectTab.Vignette,
    ),
    EditorSlider(
        "vignette_feather", "羽化", EditCategory.Effects, 0f..100f,
        defaultValue = 50f, effectTab = EffectTab.Vignette,
    ),
    EditorSlider(
        "grain", "颗粒", EditCategory.Effects, 0f..100f,
        effectTab = EffectTab.Grain,
    ),
    EditorSlider(
        "grain_size", "大小", EditCategory.Effects, 0f..100f,
        effectTab = EffectTab.Grain,
    ),
    EditorSlider(
        "grain_roughness", "粗糙度", EditCategory.Effects, 0f..100f,
        defaultValue = 50f, effectTab = EffectTab.Grain,
    ),
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
    val scope = rememberCoroutineScope()
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
    var effectTab by remember { mutableStateOf(EffectTab.Effects) }
    var presetName by remember(record.uri) { mutableStateOf(record.presetName) }
    var showInfo by remember { mutableStateOf(false) }
    var showCurve by remember { mutableStateOf(false) }
    var curveChannel by remember { mutableStateOf(CurveChannel.Rgb) }
    var curvePoints by remember(record.uri) { mutableStateOf(parseCurvePoints(record.paramsJson)) }
    var parametric by remember(record.uri) { mutableStateOf(parseParametric(record.paramsJson)) }
    var cropUi by remember(record.uri) { mutableStateOf(parseCrop(record.paramsJson)) }
    var masks by remember(record.uri) { mutableStateOf(parseMasks(record.paramsJson)) }
    var selectedMask by remember { mutableStateOf(0) }
    var maskCategory by remember { mutableStateOf(EditCategory.Light) }
    var maskEffectTab by remember { mutableStateOf(EffectTab.Effects) }
    var maskPanelTab by remember { mutableStateOf(0) }
    var colorTab by remember { mutableStateOf(ColorTab.Base) }
    var colorTabHslBand by remember { mutableStateOf(HslBandKind.Red) }
    var hslUi by remember(record.uri) { mutableStateOf(parseHsl(record.paramsJson)) }
    var gradingZone by remember { mutableStateOf(GradingZoneKind.Shadows) }
    var gradingUi by remember(record.uri) { mutableStateOf(parseGrading(record.paramsJson)) }
    var detailTab by remember { mutableStateOf(DetailTab.Sharpen) }
    var detailUi by remember(record.uri) { mutableStateOf(parseDetail(record.paramsJson)) }
    var lensUi by remember(record.uri) { mutableStateOf(parseLens(record.paramsJson)) }
    var exporting by remember { mutableStateOf(false) }
    var pendingRender by remember { mutableStateOf(0) }
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

    LaunchedEffect(sourceFile, renderVersion, tool) {
        val file = sourceFile ?: return@LaunchedEffect
        isRendering = true
        failure = null
        val json = buildParamsJson(
            values = values,
            curves = curvePoints,
            parametric = parametric,
            crop = cropUi,
            applyCropRect = tool != EditorTool.Crop,
            masks = masks,
            hsl = hslUi,
            grading = gradingUi,
            detail = detailUi,
            lens = lensUi,
        )
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

    // 实时渲染：滑块 / 曲线每次变化后 140ms 防抖，只保留最后一次。
    LaunchedEffect(pendingRender) {
        if (pendingRender == 0) return@LaunchedEffect
        delay(140)
        renderVersion += 1
    }

    fun pushHistory(before: List<Float>) {
        undoStack.add(before)
        if (undoStack.size > 40) undoStack.removeAt(0)
        redoStack.clear()
    }

    fun applyPreset(preset: BuiltinPreset) {
        pushHistory(values)
        values = List(EDITOR_SLIDERS.size) { index ->
            preset.params[EDITOR_SLIDERS[index].key] ?: EDITOR_SLIDERS[index].defaultValue
        }
        presetName = preset.name
        curvePoints = identityCurveMap()
        parametric = ParametricUi()
        renderVersion += 1
    }

    fun updateMask(index: Int, transform: (MaskUi) -> MaskUi) {
        if (index !in masks.indices) return
        masks = masks.toMutableList().also { it[index] = transform(it[index]) }
    }

    Box(modifier = Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        Column(
            modifier = Modifier
                .fillMaxSize()
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
                    paramsJson = buildParamsJson(
                        values = values,
                        curves = curvePoints,
                        parametric = parametric,
                        crop = cropUi,
                        applyCropRect = true,
                        masks = masks,
                        hsl = hslUi,
                        grading = gradingUi,
                        detail = detailUi,
                        lens = lensUi,
                    ),
                    presetName = presetName,
                    updatedAt = System.currentTimeMillis(),
                )
                EditStore.upsert(context, updated)
                Toast.makeText(context, "已保存到编辑记录", Toast.LENGTH_SHORT).show()
                onSaved(updated)
            },
            exporting = exporting,
            onExport = {
                val file = sourceFile
                if (file == null || exporting) {
                    Toast.makeText(context, "正在准备图片…", Toast.LENGTH_SHORT).show()
                } else {
                    scope.launch {
                        exporting = true
                        val json = buildParamsJson(
                            values = values,
                            curves = curvePoints,
                            parametric = parametric,
                            crop = cropUi,
                            applyCropRect = true,
                            masks = masks,
                            hsl = hslUi,
                            grading = gradingUi,
                            detail = detailUi,
                            lens = lensUi,
                        )
                        val bitmap = withContext(Dispatchers.Default) {
                            PreviewRenderer.render(file.path, json, 4096)
                        }
                        val ok = if (bitmap != null) {
                            withContext(Dispatchers.IO) {
                                ExportHelper.saveToGallery(context, bitmap)
                            }
                        } else {
                            false
                        }
                        exporting = false
                        Toast.makeText(
                            context,
                            if (ok) "已导出到相册 Pictures/fdxmp" else "导出失败",
                            Toast.LENGTH_LONG,
                        ).show()
                    }
                }
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

            when {
                showCurve -> CurveSurface(
                    points = curvePoints,
                    parametric = parametric,
                    channel = curveChannel,
                    modifier = Modifier.fillMaxSize(),
                    onPointsChange = { target, updated ->
                        curvePoints = curvePoints + (target to updated)
                        pendingRender += 1
                    },
                )

                tool == EditorTool.Crop -> CropOverlay(
                    crop = cropUi,
                    imageAspect = previewBitmap?.let {
                        it.width.toFloat() / it.height.coerceAtLeast(1)
                    } ?: 1f,
                    onCropChange = { updated -> cropUi = updated },
                    modifier = Modifier.fillMaxSize(),
                )

                tool == EditorTool.Mask -> MaskOverlay(
                    mask = masks.getOrNull(
                        selectedMask.coerceIn(0, (masks.size - 1).coerceAtLeast(0)),
                    ),
                    imageAspect = previewBitmap?.let {
                        it.width.toFloat() / it.height.coerceAtLeast(1)
                    } ?: 1f,
                    previewBitmap = previewBitmap,
                    onMaskChange = { updated ->
                        updateMask(selectedMask) { updated }
                        pendingRender += 1
                    },
                    modifier = Modifier.fillMaxSize(),
                )

                else -> Box(
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
            }

            if (tool != EditorTool.Crop) {
                HistogramOverlay(
                    data = histogram,
                    modifier = Modifier
                        .align(Alignment.TopCenter)
                        .fillMaxWidth(0.84f)
                        .height(80.dp)
                        .padding(top = 8.dp),
                )
            }

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

            if (exporting) {
                Text(
                    "导出中…",
                    color = Color.White,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier
                        .align(Alignment.Center)
                        .background(Color(0xBB000000), RoundedCornerShape(8.dp))
                        .padding(horizontal = 14.dp, vertical = 8.dp),
                )
            }
        }

        Surface(color = MaterialTheme.colorScheme.surface, modifier = Modifier.fillMaxWidth()) {
            Column {
                Box(modifier = Modifier.fillMaxWidth().height(278.dp)) {
                    when (tool) {
                        EditorTool.Edit -> if (showCurve) {
                            CurveControls(
                                channel = curveChannel,
                                onChannelChange = { curveChannel = it },
                                parametric = parametric,
                                onParametricChange = {
                                    parametric = it
                                    pendingRender += 1
                                },
                                onDone = {
                                    showCurve = false
                                    presetName = "自定义"
                                    renderVersion += 1
                                },
                            )
                        } else {
                            Column(modifier = Modifier.fillMaxSize()) {
                                PanelHeader(
                                    presetName = presetName,
                                    onReset = {
                                        pushHistory(values)
                                        values = EDITOR_SLIDERS.map { it.defaultValue }
                                        curvePoints = identityCurveMap()
                                        parametric = ParametricUi()
                                        hslUi = HslUi()
                                        gradingUi = GradingUi()
                                        detailUi = DetailUi()
                                        lensUi = LensUi()
                                        presetName = "原片"
                                        renderVersion += 1
                                    },
                                )
                                Box(modifier = Modifier.weight(1f).fillMaxWidth()) {
                                    when (category) {
                                        EditCategory.Detail -> DetailPanel(
                                            detail = detailUi,
                                            selectedTab = detailTab,
                                            onSelectTab = { detailTab = it },
                                            onChange = {
                                                detailUi = it
                                                presetName = "自定义"
                                                pendingRender += 1
                                            },
                                            modifier = Modifier.fillMaxSize(),
                                        )

                                        EditCategory.Optics -> LensPanel(
                                            lens = lensUi,
                                            onChange = {
                                                lensUi = it
                                                presetName = "自定义"
                                                pendingRender += 1
                                            },
                                            modifier = Modifier.fillMaxSize(),
                                        )

                                        EditCategory.Color -> Column(Modifier.fillMaxSize()) {
                                            PanelSubTabs(
                                                labels = ColorTab.entries.map { it.label },
                                                selectedIndex = colorTab.ordinal,
                                                onSelect = { colorTab = ColorTab.entries[it] },
                                            )
                                            when (colorTab) {
                                                ColorTab.Base -> SlidersPanel(
                                                    category = EditCategory.Color,
                                                    effectTab = effectTab,
                                                    values = values,
                                                    modifier = Modifier.weight(1f),
                                                    onEffectTabChange = { effectTab = it },
                                                    onValueChange = { index, newValue ->
                                                        if (dragSnapshot == null) {
                                                            dragSnapshot = values
                                                        }
                                                        values = values.toMutableList().also {
                                                            it[index] = newValue
                                                        }
                                                        pendingRender += 1
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

                                                ColorTab.Hsl -> HslPanel(
                                                    hsl = hslUi,
                                                    selectedBand = colorTabHslBand,
                                                    onSelectBand = { colorTabHslBand = it },
                                                    onChange = {
                                                        hslUi = it
                                                        presetName = "自定义"
                                                        pendingRender += 1
                                                    },
                                                    modifier = Modifier.weight(1f),
                                                )

                                                ColorTab.Grading -> GradingPanel(
                                                    grading = gradingUi,
                                                    selectedZone = gradingZone,
                                                    onSelectZone = { gradingZone = it },
                                                    onChange = {
                                                        gradingUi = it
                                                        presetName = "自定义"
                                                        pendingRender += 1
                                                    },
                                                    modifier = Modifier.weight(1f),
                                                )
                                            }
                                        }

                                        else -> SlidersPanel(
                                            category = category,
                                            effectTab = effectTab,
                                            values = values,
                                            modifier = Modifier.fillMaxSize(),
                                            onEffectTabChange = { effectTab = it },
                                            onValueChange = { index, newValue ->
                                                if (dragSnapshot == null) dragSnapshot = values
                                                values = values.toMutableList().also {
                                                    it[index] = newValue
                                                }
                                                pendingRender += 1
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
                                    }
                                }
                                CategoryRow(
                                    selected = category,
                                    onSelect = { category = it },
                                    curveActive = showCurve,
                                    onCurve = {
                                        showCurve = true
                                        presetName = "自定义"
                                    },
                                )
                            }
                        }

                        EditorTool.Presets -> PresetPanel(
                            current = presetName,
                            onPick = ::applyPreset,
                        )

                        EditorTool.Crop -> CropControls(
                            crop = cropUi,
                            imageAspect = previewBitmap?.let {
                                it.width.toFloat() / it.height.coerceAtLeast(1)
                            } ?: 1f,
                            onCropChange = { updated ->
                                val needsRender = updated.rotation != cropUi.rotation ||
                                    updated.flipHorizontal != cropUi.flipHorizontal ||
                                    updated.flipVertical != cropUi.flipVertical
                                cropUi = updated
                                if (needsRender) pendingRender += 1
                            },
                            onReset = {
                                cropUi = CropUi()
                                pendingRender += 1
                            },
                            onDone = {
                                tool = EditorTool.Edit
                                renderVersion += 1
                            },
                        )

                        EditorTool.Mask -> Column(modifier = Modifier.fillMaxSize()) {
                            MaskChipRow(
                                masks = masks,
                                selectedIndex = selectedMask.coerceIn(
                                    0,
                                    (masks.size - 1).coerceAtLeast(0),
                                ),
                                onSelect = { selectedMask = it },
                                onAdd = { kind ->
                                    masks = masks + defaultMask(
                                        kind = kind,
                                        index = masks.size,
                                        defaults = EDITOR_SLIDERS.map { it.defaultValue },
                                    )
                                    selectedMask = masks.lastIndex
                                    tool = EditorTool.Mask
                                },
                                onRemove = { index ->
                                    masks = masks.filterIndexed { i, _ -> i != index }
                                    selectedMask = selectedMask.coerceIn(
                                        0,
                                        (masks.size - 1).coerceAtLeast(0),
                                    )
                                    pendingRender += 1
                                },
                                onAddAi = { label ->
                                    Toast.makeText(
                                        context,
                                        "$label 需要 AI 模型，后续接入",
                                        Toast.LENGTH_SHORT,
                                    ).show()
                                },
                                onDone = {
                                    tool = EditorTool.Edit
                                    renderVersion += 1
                                },
                            )

                            val currentMask = masks.getOrNull(selectedMask)
                            if (currentMask == null) {
                                Box(
                                    modifier = Modifier.weight(1f).fillMaxWidth(),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    Text(
                                        "点右上「＋」添加线性 / 径向蒙版",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            } else {
                                PanelSubTabs(
                                    labels = listOf("蒙版", "调整"),
                                    selectedIndex = maskPanelTab,
                                    onSelect = { maskPanelTab = it },
                                )
                                if (maskPanelTab == 0) {
                                    MaskParamsPanel(
                                        mask = currentMask,
                                        onChange = { updated ->
                                            updateMask(selectedMask) { updated }
                                            pendingRender += 1
                                        },
                                        modifier = Modifier.weight(1f),
                                    )
                                } else {
                                    Row(
                                        modifier = Modifier
                                            .fillMaxWidth()
                                            .padding(horizontal = 8.dp, vertical = 2.dp),
                                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                                    ) {
                                        CategoryChip(
                                            icon = Icons.Filled.LightMode,
                                            label = "亮度",
                                            active = maskCategory == EditCategory.Light,
                                            onClick = { maskCategory = EditCategory.Light },
                                            modifier = Modifier.weight(1f),
                                        )
                                        CategoryChip(
                                            icon = Icons.Filled.WaterDrop,
                                            label = "颜色",
                                            active = maskCategory == EditCategory.Color,
                                            onClick = { maskCategory = EditCategory.Color },
                                            modifier = Modifier.weight(1f),
                                        )
                                        CategoryChip(
                                            icon = Icons.Filled.CenterFocusStrong,
                                            label = "效果",
                                            active = maskCategory == EditCategory.Effects,
                                            onClick = { maskCategory = EditCategory.Effects },
                                            modifier = Modifier.weight(1f),
                                        )
                                    }
                                    SlidersPanel(
                                        category = maskCategory,
                                        effectTab = maskEffectTab,
                                        values = currentMask.values,
                                        modifier = Modifier.weight(1f),
                                        onEffectTabChange = { maskEffectTab = it },
                                        onValueChange = { index, value ->
                                            updateMask(selectedMask) { mask ->
                                                mask.copy(
                                                    values = mask.values.toMutableList().also {
                                                        it[index] = value
                                                    },
                                                )
                                            }
                                            pendingRender += 1
                                        },
                                        onValueChangeFinished = { renderVersion += 1 },
                                    )
                                }
                            }
                        }
                    }
                }

                if (!showCurve && tool != EditorTool.Crop && tool != EditorTool.Mask) {
                    EditorToolBar(
                        selected = tool,
                        onSelect = { tool = it },
                    )
                }
            }
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
    exporting: Boolean,
    onBack: () -> Unit,
    onInfo: () -> Unit,
    onUndo: () -> Unit,
    onRedo: () -> Unit,
    onSave: () -> Unit,
    onExport: () -> Unit,
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
        IconButton(onClick = onExport, enabled = !exporting) {
            Icon(
                Icons.Filled.Download,
                contentDescription = "导出到相册",
                tint = MaterialTheme.colorScheme.onSurface,
            )
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
    }
}

@Composable
private fun SlidersPanel(
    category: EditCategory,
    effectTab: EffectTab,
    values: List<Float>,
    modifier: Modifier = Modifier,
    onEffectTabChange: (EffectTab) -> Unit,
    onValueChange: (Int, Float) -> Unit,
    onValueChangeFinished: () -> Unit,
) {
    val sliders = remember(category, effectTab) {
        EDITOR_SLIDERS.withIndex().filter { indexed ->
            indexed.value.category == category &&
                (category != EditCategory.Effects || indexed.value.effectTab == effectTab)
        }
    }
    Column(modifier = modifier.fillMaxWidth()) {
        if (category == EditCategory.Effects) {
            Row(
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 2.dp),
                horizontalArrangement = Arrangement.spacedBy(18.dp),
            ) {
                EffectTab.entries.forEach { tab ->
                    val active = tab == effectTab
                    Column(
                        horizontalAlignment = Alignment.CenterHorizontally,
                        modifier = Modifier
                            .clickable { onEffectTabChange(tab) }
                            .padding(vertical = 4.dp),
                    ) {
                        Text(
                            tab.label,
                            style = MaterialTheme.typography.labelLarge,
                            color = if (active) {
                                MaterialTheme.colorScheme.onSurface
                            } else {
                                MaterialTheme.colorScheme.onSurfaceVariant
                            },
                        )
                        Spacer(Modifier.height(3.dp))
                        Box(
                            modifier = Modifier
                                .width(28.dp)
                                .height(2.dp)
                                .background(
                                    if (active) {
                                        MaterialTheme.colorScheme.primary
                                    } else {
                                        Color.Transparent
                                    },
                                ),
                        )
                    }
                }
            }
        }
        LazyColumn(
            modifier = Modifier.fillMaxWidth().weight(1f),
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
                            formatSliderValue(slider, values[index]),
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
}

@Composable
private fun CategoryRow(
    selected: EditCategory,
    onSelect: (EditCategory) -> Unit,
    curveActive: Boolean,
    onCurve: () -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 2.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        CategoryChip(
            icon = Icons.Filled.LightMode,
            label = EditCategory.Light.label,
            active = selected == EditCategory.Light && !curveActive,
            onClick = { onSelect(EditCategory.Light) },
            modifier = Modifier.weight(1f),
        )
        CategoryChip(
            icon = Icons.Filled.Timeline,
            label = "曲线",
            active = curveActive,
            onClick = onCurve,
            modifier = Modifier.weight(1f),
        )
        CategoryChip(
            icon = Icons.Filled.WaterDrop,
            label = EditCategory.Color.label,
            active = selected == EditCategory.Color && !curveActive,
            onClick = { onSelect(EditCategory.Color) },
            modifier = Modifier.weight(1f),
        )
        CategoryChip(
            icon = Icons.Filled.CenterFocusStrong,
            label = EditCategory.Effects.label,
            active = selected == EditCategory.Effects && !curveActive,
            onClick = { onSelect(EditCategory.Effects) },
            modifier = Modifier.weight(1f),
        )
        CategoryChip(
            icon = Icons.Filled.Details,
            label = EditCategory.Detail.label,
            active = selected == EditCategory.Detail && !curveActive,
            onClick = { onSelect(EditCategory.Detail) },
            modifier = Modifier.weight(1f),
        )
        CategoryChip(
            icon = Icons.Filled.PhotoCamera,
            label = EditCategory.Optics.label,
            active = selected == EditCategory.Optics && !curveActive,
            onClick = { onSelect(EditCategory.Optics) },
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun CategoryChip(
    icon: ImageVector,
    label: String,
    active: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        horizontalAlignment = Alignment.CenterHorizontally,
        modifier = modifier
            .clip(RoundedCornerShape(10.dp))
            .background(
                if (active) MaterialTheme.colorScheme.primary else Color(0xFF1E1E1E),
            )
            .clickable(onClick = onClick)
            .padding(vertical = 6.dp),
    ) {
        Icon(
            icon,
            contentDescription = label,
            tint = if (active) Color.White else MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(18.dp),
        )
        Spacer(Modifier.height(2.dp))
        Text(
            label,
            textAlign = TextAlign.Center,
            style = MaterialTheme.typography.labelSmall,
            color = if (active) Color.White else MaterialTheme.colorScheme.onSurfaceVariant,
        )
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
    return EDITOR_SLIDERS.map { slider ->
        val value = root?.optJSONObject(slider.category.groupKey)
            ?.optDouble(slider.key, Double.NaN)
        if (value == null || value.isNaN()) slider.defaultValue else value.toFloat()
    }
}

private fun parseCurvePoints(paramsJson: String): Map<CurveChannel, List<CurvePointUi>> {
    val result = identityCurveMap().toMutableMap()
    val curves = runCatching {
        JSONObject(paramsJson).optJSONObject("curves")
    }.getOrNull() ?: return result
    val channels = mapOf(
        CurveChannel.Rgb to "rgb",
        CurveChannel.Red to "red",
        CurveChannel.Green to "green",
        CurveChannel.Blue to "blue",
    )
    channels.forEach { (channel, key) ->
        val pointsJson = curves.optJSONObject(key)?.optJSONArray("points") ?: return@forEach
        val points = buildList {
            for (index in 0 until pointsJson.length()) {
                val point = pointsJson.optJSONObject(index) ?: continue
                add(
                    CurvePointUi(
                        x = point.optDouble("x", 0.0).toFloat(),
                        y = point.optDouble("y", 0.0).toFloat(),
                    ),
                )
            }
        }
        if (points.size >= 2) {
            result[channel] = points.sortedBy { it.x }
        }
    }
    return result
}

private fun parseParametric(paramsJson: String): ParametricUi {
    val json = runCatching {
        JSONObject(paramsJson).optJSONObject("curves")?.optJSONObject("parametric")
    }.getOrNull() ?: return ParametricUi()
    return ParametricUi(
        highlights = json.optDouble("highlights", 0.0).toFloat(),
        midtones = json.optDouble("midtones", 0.0).toFloat(),
        shadows = json.optDouble("shadows", 0.0).toFloat(),
        highlightsSplit = json.optDouble("highlights_split", 0.75).toFloat(),
        midtonesSplit = json.optDouble("midtones_split", 0.25).toFloat(),
    )
}

private fun parseCrop(paramsJson: String): CropUi {
    val json = runCatching {
        JSONObject(paramsJson).optJSONObject("crop")
    }.getOrNull() ?: return CropUi()
    val aspect = CropAspect.entries.firstOrNull { it.serdeName == json.optString("aspect") }
        ?: CropAspect.Free
    val rectJson = json.optJSONArray("rect")
    val rect = if (rectJson != null && rectJson.length() >= 4) {
        CropRect(
            x = rectJson.optDouble(0, 0.0).toFloat().coerceIn(0f, 1f),
            y = rectJson.optDouble(1, 0.0).toFloat().coerceIn(0f, 1f),
            w = rectJson.optDouble(2, 1.0).toFloat().coerceIn(0.01f, 1f),
            h = rectJson.optDouble(3, 1.0).toFloat().coerceIn(0.01f, 1f),
        )
    } else {
        CropRect(0f, 0f, 1f, 1f)
    }
    return CropUi(
        rect = rect,
        aspect = aspect,
        rotation = json.optDouble("rotation_deg", 0.0).toFloat().coerceIn(-45f, 45f),
        flipHorizontal = json.optBoolean("flip_horizontal", false),
        flipVertical = json.optBoolean("flip_vertical", false),
    )
}

private fun parseMasks(paramsJson: String): List<MaskUi> {
    val array = runCatching {
        JSONObject(paramsJson).optJSONArray("masks")
    }.getOrNull() ?: return emptyList()
    val masks = mutableListOf<MaskUi>()
    for (index in 0 until array.length()) {
        val item = array.optJSONObject(index) ?: continue
        val kindJson = item.optJSONObject("kind") ?: continue
        val kind = MaskKindUi.entries.firstOrNull {
            it.serdeName == kindJson.optString("type")
        } ?: continue
        val data = kindJson.optJSONObject("data")
        val adjustments = item.optJSONObject("adjustments")
        val values = EDITOR_SLIDERS.map { slider ->
            adjustments
                ?.optJSONObject(slider.category.groupKey)
                ?.optDouble(slider.key, slider.defaultValue.toDouble())
                ?.toFloat()
                ?: slider.defaultValue
        }
        fun pair(name: String, fallbackX: Float, fallbackY: Float): Offset {
            val json = data?.optJSONArray(name) ?: return Offset(fallbackX, fallbackY)
            return Offset(
                json.optDouble(0, fallbackX.toDouble()).toFloat(),
                json.optDouble(1, fallbackY.toDouble()).toFloat(),
            )
        }
        masks += MaskUi(
            id = item.optString("id", "mask-$index"),
            name = item.optString("name", "蒙版 ${index + 1}"),
            kind = kind,
            start = pair("start", 0.5f, 0.32f),
            end = pair("end", 0.5f, 0.68f),
            center = pair("center", 0.5f, 0.5f),
            radii = pair("radii", 0.34f, 0.30f),
            rotation = data?.optDouble("rotation", 0.0)?.toFloat() ?: 0f,
            inner = data?.optDouble("inner", 0.75)?.toFloat() ?: 0.75f,
            values = values,
            opacity = item.optDouble("opacity", 100.0).toFloat(),
            feather = item.optDouble("feather", 0.0).toFloat(),
            inverted = item.optBoolean("inverted", false),
            brushSize = data?.optDouble("size", 30.0)?.toFloat() ?: 30f,
            brushFeather = data?.optDouble("feather", 50.0)?.toFloat() ?: 50f,
            brushFlow = data?.optDouble("flow", 100.0)?.toFloat() ?: 100f,
            brushDensity = data?.optDouble("density", 100.0)?.toFloat() ?: 100f,
            strokes = parseMaskStrokes(data),
            color = parseMaskColor(data),
            colorRange = data?.optDouble("range", 50.0)?.toFloat() ?: 50f,
            colorSmoothness = data?.optDouble("smoothness", 50.0)?.toFloat() ?: 50f,
            lumMin = data?.optDouble("min", 0.0)?.toFloat() ?: 0f,
            lumMax = data?.optDouble("max", 1.0)?.toFloat() ?: 1f,
            lumSmoothness = data?.optDouble("smoothness", 50.0)?.toFloat() ?: 50f,
        )
    }
    return masks
}

private fun parseMaskStrokes(data: JSONObject?): List<List<Offset>> {
    val array = data?.optJSONArray("strokes") ?: return emptyList()
    return buildList {
        for (index in 0 until array.length()) {
            val strokeJson = array.optJSONArray(index) ?: continue
            val stroke = buildList {
                for (pointIndex in 0 until strokeJson.length()) {
                    val point = strokeJson.optJSONArray(pointIndex) ?: continue
                    add(
                        Offset(
                            point.optDouble(0, 0.0).toFloat(),
                            point.optDouble(1, 0.0).toFloat(),
                        ),
                    )
                }
            }
            if (stroke.isNotEmpty()) add(stroke)
        }
    }
}

private fun parseMaskColor(data: JSONObject?): Triple<Float, Float, Float> {
    val array = data?.optJSONArray("color") ?: return Triple(0.5f, 0.5f, 0.5f)
    return Triple(
        array.optDouble(0, 0.5).toFloat(),
        array.optDouble(1, 0.5).toFloat(),
        array.optDouble(2, 0.5).toFloat(),
    )
}

private fun parseHsl(paramsJson: String): HslUi {
    val json = runCatching { JSONObject(paramsJson).optJSONObject("hsl") }.getOrNull()
        ?: return HslUi()
    return HslUi(
        bands = HslBandKind.entries.map { kind ->
            val band = json.optJSONObject(kind.serde) ?: return@map HslBandUi()
            HslBandUi(
                hue = band.optDouble("hue", 0.0).toFloat(),
                saturation = band.optDouble("saturation", 0.0).toFloat(),
                luminance = band.optDouble("luminance", 0.0).toFloat(),
            )
        },
    )
}

private fun parseGrading(paramsJson: String): GradingUi {
    val json = runCatching { JSONObject(paramsJson).optJSONObject("grading") }.getOrNull()
        ?: return GradingUi()
    fun zone(name: String): GradingZoneUi {
        val zoneJson = json.optJSONObject(name) ?: return GradingZoneUi()
        return GradingZoneUi(
            hue = zoneJson.optDouble("hue", 0.0).toFloat(),
            saturation = zoneJson.optDouble("saturation", 0.0).toFloat(),
            luminance = zoneJson.optDouble("luminance", 0.0).toFloat(),
        )
    }
    return GradingUi(
        shadows = zone("shadows"),
        midtones = zone("midtones"),
        highlights = zone("highlights"),
        blending = json.optDouble("blending", 50.0).toFloat(),
        balance = json.optDouble("balance", 0.0).toFloat(),
    )
}

private fun parseDetail(paramsJson: String): DetailUi {
    val json = runCatching { JSONObject(paramsJson).optJSONObject("detail") }.getOrNull()
        ?: return DetailUi()
    val sharpenJson = json.optJSONObject("sharpen")
    val noiseJson = json.optJSONObject("noise")
    val radius = sharpenJson?.optDouble("radius", 1.0)?.toFloat() ?: 1f
    return DetailUi(
        sharpen = SharpenUi(
            amount = sharpenJson?.optDouble("amount", 0.0)?.toFloat() ?: 0f,
            radius = if (radius < 0.3f) 1f else radius,
            detail = sharpenJson?.optDouble("detail", 25.0)?.toFloat() ?: 25f,
            masking = sharpenJson?.optDouble("masking", 0.0)?.toFloat() ?: 0f,
        ),
        noise = NoiseUi(
            luminance = noiseJson?.optDouble("luminance", 0.0)?.toFloat() ?: 0f,
            luminanceDetail = noiseJson?.optDouble("luminance_detail", 50.0)?.toFloat() ?: 50f,
            luminanceContrast = noiseJson?.optDouble("luminance_contrast", 0.0)?.toFloat() ?: 0f,
            color = noiseJson?.optDouble("color", 0.0)?.toFloat() ?: 0f,
            colorDetail = noiseJson?.optDouble("color_detail", 50.0)?.toFloat() ?: 50f,
            colorSmoothness = noiseJson?.optDouble("color_smoothness", 50.0)?.toFloat() ?: 50f,
        ),
    )
}

private fun parseLens(paramsJson: String): LensUi {
    val json = runCatching { JSONObject(paramsJson).optJSONObject("lens") }.getOrNull()
        ?: return LensUi()
    val geometry = json.optJSONObject("geometry")
    return LensUi(
        profileCorrection = json.optBoolean("profile_correction", false),
        distortion = geometry?.optDouble("distortion", 0.0)?.toFloat() ?: 0f,
        vertical = geometry?.optDouble("vertical", 0.0)?.toFloat() ?: 0f,
        horizontal = geometry?.optDouble("horizontal", 0.0)?.toFloat() ?: 0f,
        rotate = geometry?.optDouble("rotate", 0.0)?.toFloat() ?: 0f,
        scale = geometry?.optDouble("scale", 1.0)?.toFloat()?.let {
            if (it < 0.3f) 1f else it
        } ?: 1f,
        vignette = json.optDouble("vignette", 0.0).toFloat(),
    )
}

private fun buildParamsJson(
    values: List<Float>,
    curves: Map<CurveChannel, List<CurvePointUi>>,
    parametric: ParametricUi,
    crop: CropUi,
    applyCropRect: Boolean,
    masks: List<MaskUi>,
    hsl: HslUi,
    grading: GradingUi,
    detail: DetailUi,
    lens: LensUi,
): String {
    fun value(key: String): Double =
        values[EDITOR_SLIDERS.indexOfFirst { it.key == key }].toDouble()

    val grouped = EditCategory.entries
        .filter { it != EditCategory.Detail }
        .associateWith { category -> EDITOR_SLIDERS.filter { it.category == category } }
    return JSONObject().apply {
        put("version", 1)
        grouped.forEach { (category, sliders) ->
            put(
                category.groupKey,
                JSONObject().apply {
                    sliders.forEach { put(it.key, value(it.key)) }
                },
            )
        }
        put(
            "curves",
            JSONObject().apply {
                fun pointsJson(channel: CurveChannel): JSONArray = JSONArray().apply {
                    (curves[channel] ?: identityCurve()).forEach { point ->
                        put(
                            JSONObject().apply {
                                put("x", point.x.toDouble())
                                put("y", point.y.toDouble())
                            },
                        )
                    }
                }
                put("rgb", JSONObject().apply { put("points", pointsJson(CurveChannel.Rgb)) })
                put("red", JSONObject().apply { put("points", pointsJson(CurveChannel.Red)) })
                put("green", JSONObject().apply { put("points", pointsJson(CurveChannel.Green)) })
                put("blue", JSONObject().apply { put("points", pointsJson(CurveChannel.Blue)) })
                put(
                    "parametric",
                    JSONObject().apply {
                        put("highlights", parametric.highlights.toDouble())
                        put("midtones", parametric.midtones.toDouble())
                        put("shadows", parametric.shadows.toDouble())
                        put("highlights_split", parametric.highlightsSplit.toDouble())
                        put("midtones_split", parametric.midtonesSplit.toDouble())
                    },
                )
            },
        )
        put(
            "crop",
            JSONObject().apply {
                val rect = crop.rect
                put(
                    "rect",
                    JSONArray().apply {
                        put(if (applyCropRect) rect.x.toDouble() else 0.0)
                        put(if (applyCropRect) rect.y.toDouble() else 0.0)
                        put(if (applyCropRect) rect.w.toDouble() else 1.0)
                        put(if (applyCropRect) rect.h.toDouble() else 1.0)
                    },
                )
                put("aspect", crop.aspect.serdeName)
                put("rotation_deg", crop.rotation.toDouble())
                put("flip_horizontal", crop.flipHorizontal)
                put("flip_vertical", crop.flipVertical)
                put("grid", "thirds")
                put("auto_compose", false)
            },
        )
        put(
            "masks",
            JSONArray().apply {
                masks.forEach { mask ->
                    put(
                        JSONObject().apply {
                            put("id", mask.id)
                            put("name", mask.name)
                            put(
                                "kind",
                                JSONObject().apply {
                                    put("type", mask.kind.serdeName)
                                    put(
                                        "data",
                                        JSONObject().apply {
                                            when (mask.kind) {
                                                MaskKindUi.Linear -> {
                                                    put(
                                                        "start",
                                                        JSONArray().apply {
                                                            put(mask.start.x.toDouble())
                                                            put(mask.start.y.toDouble())
                                                        },
                                                    )
                                                    put(
                                                        "end",
                                                        JSONArray().apply {
                                                            put(mask.end.x.toDouble())
                                                            put(mask.end.y.toDouble())
                                                        },
                                                    )
                                                }

                                                MaskKindUi.Radial -> {
                                                    put(
                                                        "center",
                                                        JSONArray().apply {
                                                            put(mask.center.x.toDouble())
                                                            put(mask.center.y.toDouble())
                                                        },
                                                    )
                                                    put(
                                                        "radii",
                                                        JSONArray().apply {
                                                            put(mask.radii.x.toDouble())
                                                            put(mask.radii.y.toDouble())
                                                        },
                                                    )
                                                    put("rotation", mask.rotation.toDouble())
                                                    put("inner", mask.inner.toDouble())
                                                }

                                                MaskKindUi.Brush -> {
                                                    put("size", mask.brushSize.toDouble())
                                                    put("feather", mask.brushFeather.toDouble())
                                                    put("flow", mask.brushFlow.toDouble())
                                                    put("density", mask.brushDensity.toDouble())
                                                    put("auto_mask", false)
                                                    put(
                                                        "strokes",
                                                        JSONArray().apply {
                                                            mask.strokes.forEach { stroke ->
                                                                put(
                                                                    JSONArray().apply {
                                                                        stroke.forEach { point ->
                                                                            put(
                                                                                JSONArray().apply {
                                                                                    put(
                                                                                        point.x
                                                                                            .toDouble(),
                                                                                    )
                                                                                    put(
                                                                                        point.y
                                                                                            .toDouble(),
                                                                                    )
                                                                                },
                                                                            )
                                                                        }
                                                                    },
                                                                )
                                                            }
                                                        },
                                                    )
                                                }

                                                MaskKindUi.ColorRange -> {
                                                    put(
                                                        "color",
                                                        JSONArray().apply {
                                                            put(mask.color.first.toDouble())
                                                            put(mask.color.second.toDouble())
                                                            put(mask.color.third.toDouble())
                                                        },
                                                    )
                                                    put("range", mask.colorRange.toDouble())
                                                    put(
                                                        "smoothness",
                                                        mask.colorSmoothness.toDouble(),
                                                    )
                                                }

                                                MaskKindUi.LuminanceRange -> {
                                                    put("min", mask.lumMin.toDouble())
                                                    put("max", mask.lumMax.toDouble())
                                                    put(
                                                        "smoothness",
                                                        mask.lumSmoothness.toDouble(),
                                                    )
                                                }
                                            }
                                        },
                                    )
                                },
                            )
                            put("inverted", mask.inverted)
                            put("feather", mask.feather.toDouble())
                            put("opacity", mask.opacity.toDouble())
                            put("overlay", "red")
                            put(
                                "adjustments",
                                JSONObject().apply {
                                    grouped.forEach { (category, sliders) ->
                                        put(
                                            category.groupKey,
                                            JSONObject().apply {
                                                sliders.forEach { slider ->
                                                    put(
                                                        slider.key,
                                                        mask.values[
                                                            EDITOR_SLIDERS.indexOfFirst {
                                                                it.key == slider.key
                                                            }
                                                        ].toDouble(),
                                                    )
                                                }
                                            },
                                        )
                                    }
                                },
                            )
                        },
                    )
                }
            },
        )
        put(
            "hsl",
            JSONObject().apply {
                HslBandKind.entries.forEachIndexed { index, kind ->
                    val band = hsl.bands.getOrElse(index) { HslBandUi() }
                    put(
                        kind.serde,
                        JSONObject().apply {
                            put("hue", band.hue.toDouble())
                            put("saturation", band.saturation.toDouble())
                            put("luminance", band.luminance.toDouble())
                        },
                    )
                }
            },
        )
        put(
            "grading",
            JSONObject().apply {
                fun zoneJson(zone: GradingZoneUi): JSONObject = JSONObject().apply {
                    put("hue", zone.hue.toDouble())
                    put("saturation", zone.saturation.toDouble())
                    put("luminance", zone.luminance.toDouble())
                }
                put("shadows", zoneJson(grading.shadows))
                put("midtones", zoneJson(grading.midtones))
                put("highlights", zoneJson(grading.highlights))
                put("blending", grading.blending.toDouble())
                put("balance", grading.balance.toDouble())
            },
        )
        put(
            "detail",
            JSONObject().apply {
                put(
                    "sharpen",
                    JSONObject().apply {
                        put("amount", detail.sharpen.amount.toDouble())
                        put("radius", detail.sharpen.radius.toDouble())
                        put("detail", detail.sharpen.detail.toDouble())
                        put("masking", detail.sharpen.masking.toDouble())
                    },
                )
                put(
                    "noise",
                    JSONObject().apply {
                        put("luminance", detail.noise.luminance.toDouble())
                        put("luminance_detail", detail.noise.luminanceDetail.toDouble())
                        put("luminance_contrast", detail.noise.luminanceContrast.toDouble())
                        put("color", detail.noise.color.toDouble())
                        put("color_detail", detail.noise.colorDetail.toDouble())
                        put("color_smoothness", detail.noise.colorSmoothness.toDouble())
                    },
                )
            },
        )
        put(
            "lens",
            JSONObject().apply {
                put("profile_correction", lens.profileCorrection)
                put("chromatic_aberration", 0.0)
                put("vignette", lens.vignette.toDouble())
                put(
                    "geometry",
                    JSONObject().apply {
                        put("auto", false)
                        put("distortion", lens.distortion.toDouble())
                        put("vertical", lens.vertical.toDouble())
                        put("horizontal", lens.horizontal.toDouble())
                        put("rotate", lens.rotate.toDouble())
                        put("scale", lens.scale.toDouble())
                    },
                )
            },
        )
    }.toString()
}

private fun viewportInfo(values: List<Float>): String {
    val changed = values.withIndex().count { (index, value) ->
        value != EDITOR_SLIDERS[index].defaultValue
    }
    return if (changed == 0) "未调整" else "已调整 $changed 项"
}

private fun formatSliderValue(slider: EditorSlider, value: Float): String = when {
    slider.key == "exposure" -> String.format(Locale.US, "%+.2f", value)
    slider.range.start < 0f -> String.format(Locale.US, "%+.0f", value)
    else -> String.format(Locale.US, "%.0f", value)
}
