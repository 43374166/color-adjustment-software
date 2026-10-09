package com.fdxmp.app.ui

import android.graphics.Bitmap
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import kotlin.math.abs
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.hypot
import kotlin.math.pow
import kotlin.math.sin

/** 蒙版类型（当前实现线性 / 径向，其余下一步）。 */
enum class MaskKindUi(val label: String, val serdeName: String) {
    Linear("线性渐变", "linear_gradient"),
    Radial("径向渐变", "radial_gradient"),
    Brush("画笔", "brush"),
    ColorRange("颜色范围", "color_range"),
    LuminanceRange("亮度范围", "luminance_range"),
}

/**
 * 蒙版状态：几何（线性 start/end；径向 center/radii/rotation/inner）+ 15 项局部调整值。
 * `values` 顺序与全局 EDITOR_SLIDERS 一致。
 */
data class MaskUi(
    val id: String,
    val name: String,
    val kind: MaskKindUi,
    val start: Offset,
    val end: Offset,
    val center: Offset,
    val radii: Offset,
    val rotation: Float,
    val inner: Float,
    val values: List<Float>,
    val opacity: Float = 100f,
    val feather: Float = 0f,
    val inverted: Boolean = false,
    val brushSize: Float = 30f,
    val brushFeather: Float = 50f,
    val brushFlow: Float = 100f,
    val brushDensity: Float = 100f,
    val strokes: List<List<Offset>> = emptyList(),
    val color: Triple<Float, Float, Float> = Triple(0.5f, 0.5f, 0.5f),
    val colorRange: Float = 50f,
    val colorSmoothness: Float = 50f,
    val lumMin: Float = 0f,
    val lumMax: Float = 1f,
    val lumSmoothness: Float = 50f,
)

/** 新建蒙版：线性落在画面中部，径向居中。 */
fun defaultMask(kind: MaskKindUi, index: Int, defaults: List<Float>): MaskUi = MaskUi(
    id = "mask-${System.currentTimeMillis()}-$index",
    name = "蒙版 ${index + 1}",
    kind = kind,
    start = Offset(0.5f, 0.32f),
    end = Offset(0.5f, 0.68f),
    center = Offset(0.5f, 0.5f),
    radii = Offset(0.34f, 0.30f),
    rotation = 0f,
    inner = 0.75f,
    values = defaults,
    opacity = 100f,
    feather = 0f,
    inverted = false,
    color = when (kind) {
        MaskKindUi.ColorRange -> Triple(0.5f, 0.5f, 0.5f)
        else -> Triple(0.5f, 0.5f, 0.5f)
    },
    lumMin = 0f,
    lumMax = 1f,
)

/**
 * 蒙版覆盖层：红色叠加显示 + 参考线 / 手柄。
 *
 * - 线性：三条线（100% / 50% / 0%），拖中线移动、拖两侧线改羽化、拖端点手柄旋转；
 * - 径向：外圈椭圆（四向手柄 + 旋转手柄）与内圈椭圆（四向手柄改内外过渡）。
 */
@Composable
fun MaskOverlay(
    mask: MaskUi?,
    imageAspect: Float,
    previewBitmap: Bitmap?,
    onMaskChange: (MaskUi) -> Unit,
    modifier: Modifier = Modifier,
) {
    if (mask == null) return
    var boxSize by remember { mutableStateOf(IntSize.Zero) }
    var drag by remember { mutableStateOf(MaskDrag.None) }
    var lastPointer by remember { mutableStateOf(Offset.Zero) }

    val maskState = rememberUpdatedState(mask)
    val changeState = rememberUpdatedState(onMaskChange)
    val aspectState = rememberUpdatedState(imageAspect)
    val bitmapState = rememberUpdatedState(previewBitmap)

    Box(
        modifier = modifier
            .onSizeChanged { boxSize = it }
            .pointerInput(boxSize, imageAspect, mask.id) {
                when (maskState.value.kind) {
                    MaskKindUi.Brush -> {
                        var strokeIndex = -1
                        detectDragGestures(
                            onDragStart = { position ->
                                val normalized = toNormalized(position, boxSize, aspectState.value)
                                val updated = maskState.value.copy(
                                    strokes = maskState.value.strokes + listOf(listOf(normalized)),
                                )
                                strokeIndex = updated.strokes.lastIndex
                                changeState.value(updated)
                            },
                            onDrag = { change, _ ->
                                change.consume()
                                val current = maskState.value
                                if (strokeIndex !in current.strokes.indices) {
                                    return@detectDragGestures
                                }
                                val normalized =
                                    toNormalized(change.position, boxSize, aspectState.value)
                                val strokes = current.strokes.toMutableList()
                                val stroke = strokes[strokeIndex].toMutableList()
                                val last = stroke.lastOrNull()
                                if (last == null ||
                                    (normalized - last).getDistance() > 0.01f
                                ) {
                                    stroke.add(normalized)
                                    strokes[strokeIndex] = stroke
                                    changeState.value(current.copy(strokes = strokes))
                                }
                            },
                            onDragEnd = { strokeIndex = -1 },
                            onDragCancel = { strokeIndex = -1 },
                        )
                    }

                    MaskKindUi.ColorRange -> detectTapGestures { position ->
                        val bitmap = bitmapState.value ?: return@detectTapGestures
                        val normalized = toNormalized(position, boxSize, aspectState.value)
                        val px = (normalized.x * bitmap.width).toInt()
                            .coerceIn(0, bitmap.width - 1)
                        val py = (normalized.y * bitmap.height).toInt()
                            .coerceIn(0, bitmap.height - 1)
                        val pixel = bitmap.getPixel(px, py)
                        changeState.value(
                            maskState.value.copy(
                                color = Triple(
                                    ((pixel shr 16) and 0xFF) / 255f,
                                    ((pixel shr 8) and 0xFF) / 255f,
                                    (pixel and 0xFF) / 255f,
                                ),
                            ),
                        )
                    }

                    MaskKindUi.LuminanceRange -> Unit

                    else -> detectDragGestures(
                        onDragStart = { position ->
                            drag = hitTestMask(
                                mask = maskState.value,
                                box = boxSize,
                                imageAspect = aspectState.value,
                                position = position,
                            )
                            lastPointer = position
                        },
                        onDrag = { change, _ ->
                            if (drag == MaskDrag.None) return@detectDragGestures
                            change.consume()
                            val updated = applyMaskDrag(
                                mask = maskState.value,
                                mode = drag,
                                box = boxSize,
                                imageAspect = aspectState.value,
                                position = change.position,
                                previous = lastPointer,
                            )
                            lastPointer = change.position
                            changeState.value(updated)
                        },
                        onDragEnd = { drag = MaskDrag.None },
                        onDragCancel = { drag = MaskDrag.None },
                    )
                }
            },
    ) {
        Canvas(modifier = Modifier.fillMaxSize()) {
            drawMask(mask, imageAspect)
        }
    }
}

private fun toNormalized(position: Offset, box: IntSize, imageAspect: Float): Offset {
    val fit = maskFitRect(box, imageAspect)
    if (fit.width <= 0f || fit.height <= 0f) return Offset(0.5f, 0.5f)
    return Offset(
        ((position.x - fit.left) / fit.width).coerceIn(0f, 1f),
        ((position.y - fit.top) / fit.height).coerceIn(0f, 1f),
    )
}

/** 蒙版列表行：选择 / 添加（线性、径向）/ 删除 / 完成。 */
@Composable
fun MaskChipRow(
    masks: List<MaskUi>,
    selectedIndex: Int,
    onSelect: (Int) -> Unit,
    onAdd: (MaskKindUi) -> Unit,
    onRemove: (Int) -> Unit,
    onAddAi: (String) -> Unit,
    onDone: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var addMenuOpen by remember { mutableStateOf(false) }
    Row(
        modifier = modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("蒙版", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.width(10.dp))
        LazyRow(
            modifier = Modifier.weight(1f),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            items(items = masks, key = { it.id }) { mask ->
                val index = masks.indexOf(mask)
                val active = index == selectedIndex
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    modifier = Modifier
                        .clip(RoundedCornerShape(9.dp))
                        .background(
                            if (active) {
                                MaterialTheme.colorScheme.primary
                            } else {
                                Color(0xFF1E1E1E)
                            },
                        )
                        .clickable { onSelect(index) }
                        .padding(horizontal = 10.dp, vertical = 7.dp),
                ) {
                    Text(
                        mask.name,
                        style = MaterialTheme.typography.labelMedium,
                        color = if (active) Color.White else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    if (active) {
                        Spacer(Modifier.width(6.dp))
                        Icon(
                            Icons.Filled.Close,
                            contentDescription = "删除蒙版",
                            tint = Color.White,
                            modifier = Modifier
                                .size(14.dp)
                                .clickable { onRemove(index) },
                        )
                    }
                }
            }
        }
        Box {
            Icon(
                Icons.Filled.Add,
                contentDescription = "添加蒙版",
                tint = MaterialTheme.colorScheme.primary,
                modifier = Modifier
                    .size(34.dp)
                    .clip(CircleShape)
                    .background(Color(0xFF1E1E1E))
                    .clickable { addMenuOpen = true }
                    .padding(6.dp),
            )
            DropdownMenu(expanded = addMenuOpen, onDismissRequest = { addMenuOpen = false }) {
                MaskKindUi.entries.forEach { kind ->
                    DropdownMenuItem(
                        text = { Text(kind.label) },
                        onClick = {
                            onAdd(kind)
                            addMenuOpen = false
                        },
                    )
                }
                listOf("AI 主体", "AI 天空", "AI 人物").forEach { label ->
                    DropdownMenuItem(
                        text = {
                            Text(
                                "$label（需模型 · 后续接入）",
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        },
                        onClick = {
                            onAddAi(label)
                            addMenuOpen = false
                        },
                    )
                }
            }
        }
        Spacer(Modifier.width(8.dp))
        OutlinedButton(onClick = onDone, shape = RoundedCornerShape(12.dp)) {
            Text("完成")
        }
    }
}

/** 蒙版自身参数（按类型）：反转 + 不透明度 / 羽化 / 画笔 / 颜色范围 / 亮度范围。 */
@Composable
fun MaskParamsPanel(
    mask: MaskUi,
    onChange: (MaskUi) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 2.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                "反转蒙版",
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.weight(1f),
            )
            Switch(
                checked = mask.inverted,
                onCheckedChange = { onChange(mask.copy(inverted = it)) },
            )
        }
        val rows: List<Triple<String, Float, (Float) -> Unit>> = when (mask.kind) {
            MaskKindUi.Brush -> listOf(
                Triple("大小", mask.brushSize) { onChange(mask.copy(brushSize = it)) },
                Triple("羽化", mask.brushFeather) { onChange(mask.copy(brushFeather = it)) },
                Triple("流动", mask.brushFlow) { onChange(mask.copy(brushFlow = it)) },
                Triple("密度", mask.brushDensity) { onChange(mask.copy(brushDensity = it)) },
                Triple("不透明度", mask.opacity) { onChange(mask.copy(opacity = it)) },
            )

            MaskKindUi.ColorRange -> listOf(
                Triple("范围", mask.colorRange) { onChange(mask.copy(colorRange = it)) },
                Triple("平滑度", mask.colorSmoothness) {
                    onChange(mask.copy(colorSmoothness = it))
                },
                Triple("不透明度", mask.opacity) { onChange(mask.copy(opacity = it)) },
            )

            MaskKindUi.LuminanceRange -> listOf(
                Triple("下界", mask.lumMin) { onChange(mask.copy(lumMin = it)) },
                Triple("上界", mask.lumMax) { onChange(mask.copy(lumMax = it)) },
                Triple("平滑度", mask.lumSmoothness) {
                    onChange(mask.copy(lumSmoothness = it))
                },
                Triple("不透明度", mask.opacity) { onChange(mask.copy(opacity = it)) },
            )

            else -> listOf(
                Triple("不透明度", mask.opacity) { onChange(mask.copy(opacity = it)) },
                Triple("羽化", mask.feather) { onChange(mask.copy(feather = it)) },
            )
        }
        LazyColumn(
            modifier = Modifier.fillMaxWidth().weight(1f),
            contentPadding = PaddingValues(horizontal = 16.dp),
        ) {
            item(key = "hint") {
                Text(
                    when (mask.kind) {
                        MaskKindUi.Brush -> "在画面上涂抹添加蒙版"
                        MaskKindUi.ColorRange -> "在画面上点按取色"
                        MaskKindUi.LuminanceRange -> "按亮度区间选择"
                        else -> "在画面上拖动调整参考线 / 椭圆"
                    },
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            items(items = rows, key = { it.first }) { (label, value, updater) ->
                val range = when (label) {
                    "下界", "上界" -> 0f..1f
                    "大小" -> 2f..100f
                    else -> 0f..100f
                }
                PanelSliderRow(
                    label = label,
                    value = value,
                    range = range,
                    decimals = if (label == "下界" || label == "上界") 2 else 0,
                    neutral = when (label) {
                        "大小" -> 30f
                        "羽化", "平滑度", "范围" -> 50f
                        "流动", "密度", "不透明度" -> 100f
                        "上界" -> 1f
                        else -> 0f
                    },
                    step = if (range.endInclusive - range.start <= 1.5f) 0.05f else 5f,
                    onChange = updater,
                )
            }
        }
    }
}

/* ------------------------------------------------------------ 绘制 */

private data class MaskFitRect(
    val left: Float,
    val top: Float,
    val width: Float,
    val height: Float,
)

private fun maskFitRect(box: IntSize, imageAspect: Float): MaskFitRect {
    if (box.width <= 0 || box.height <= 0 || imageAspect <= 0f) {
        return MaskFitRect(0f, 0f, 0f, 0f)
    }
    val boxAspect = box.width.toFloat() / box.height
    return if (imageAspect > boxAspect) {
        val width = box.width.toFloat()
        val height = width / imageAspect
        MaskFitRect(0f, (box.height - height) / 2f, width, height)
    } else {
        val height = box.height.toFloat()
        val width = height * imageAspect
        MaskFitRect((box.width - width) / 2f, 0f, width, height)
    }
}

private fun DrawScope.drawMask(mask: MaskUi, imageAspect: Float) {
    val box = IntSize(size.width.toInt(), size.height.toInt())
    val fit = maskFitRect(box, imageAspect)
    if (fit.width <= 0f) return
    when (mask.kind) {
        MaskKindUi.Linear -> drawLinearMask(mask, fit)
        MaskKindUi.Radial -> drawRadialMask(mask, fit)
        MaskKindUi.Brush -> drawBrushMask(mask, fit)
        MaskKindUi.ColorRange, MaskKindUi.LuminanceRange -> Unit
    }
}

private fun DrawScope.drawBrushMask(mask: MaskUi, fit: MaskFitRect) {
    if (mask.strokes.isEmpty()) return
    val longEdge = maxOf(fit.width, fit.height)
    val radius = (mask.brushSize / 100f).coerceIn(0.002f, 0.5f) * longEdge * 0.5f
    mask.strokes.forEach { stroke ->
        val points = stroke.map {
            Offset(fit.left + it.x * fit.width, fit.top + it.y * fit.height)
        }
        if (points.size == 1) {
            drawCircle(Color(0x55FF3B30), radius = radius, center = points[0])
            drawCircle(Color(0xCCFFFFFF), radius = radius, center = points[0], style = Stroke(2f))
        } else if (points.size > 1) {
            val path = Path()
            points.forEachIndexed { index, point ->
                if (index == 0) path.moveTo(point.x, point.y) else path.lineTo(point.x, point.y)
            }
            drawPath(
                path,
                Color(0x55FF3B30),
                style = Stroke(
                    width = radius * 2f,
                    cap = StrokeCap.Round,
                    join = StrokeJoin.Round,
                ),
            )
            drawPath(
                path,
                Color(0xCCFFFFFF),
                style = Stroke(width = 2f, cap = StrokeCap.Round, join = StrokeJoin.Round),
            )
        }
    }
}

private fun DrawScope.drawLinearMask(mask: MaskUi, fit: MaskFitRect) {
    val start = Offset(fit.left + mask.start.x * fit.width, fit.top + mask.start.y * fit.height)
    val end = Offset(fit.left + mask.end.x * fit.width, fit.top + mask.end.y * fit.height)
    val diff = end - start
    val length = diff.getDistance().coerceAtLeast(1f)
    val unit = diff / length
    val perp = Offset(-unit.y, unit.x)
    val extend = hypot(fit.width, fit.height) * 1.4f

    // 红色叠加：沿渐变方向从选中到未选中。
    drawRect(
        brush = Brush.linearGradient(
            colors = listOf(Color(0x55FF3B30), Color(0x00FF3B30)),
            start = start,
            end = end,
        ),
        topLeft = Offset(fit.left, fit.top),
        size = Size(fit.width, fit.height),
    )

    val lines = listOf(0f, 0.5f, 1f)
    lines.forEach { t ->
        val center = start + diff * t
        val outer = t != 0.5f
        drawLine(
            color = if (outer) Color(0xCCFFFFFF) else Color.White,
            start = center - perp * extend,
            end = center + perp * extend,
            strokeWidth = if (outer) 2.5f else 4f,
        )
    }

    // 旋转手柄（中线两端）。
    val middle = start + diff * 0.5f
    listOf(-1f, 1f).forEach { side ->
        val grip = middle + perp * (side * fit.width * 0.38f)
        drawCircle(Color(0xCC000000), radius = 16f, center = grip)
        drawCircle(Color.White, radius = 12f, center = grip)
        drawCircle(Color(0x66000000), radius = 12f, center = grip, style = Stroke(2f))
    }
}

private fun DrawScope.drawRadialMask(mask: MaskUi, fit: MaskFitRect) {
    val center = Offset(fit.left + mask.center.x * fit.width, fit.top + mask.center.y * fit.height)
    val rx = mask.radii.x * fit.width
    val ry = mask.radii.y * fit.height
    if (rx <= 1f || ry <= 1f) return
    val innerScale = mask.inner.coerceIn(0.02f, 0.98f)

    rotate(mask.rotation, pivot = center) {
        // 选中区域叠加
        drawOval(
            color = Color(0x33FF3B30),
            topLeft = Offset(center.x - rx, center.y - ry),
            size = Size(rx * 2f, ry * 2f),
        )
        // 外圈
        drawOval(
            color = Color.White,
            topLeft = Offset(center.x - rx, center.y - ry),
            size = Size(rx * 2f, ry * 2f),
            style = Stroke(width = 3f),
        )
        // 内圈
        drawOval(
            color = Color(0xCCFFFFFF),
            topLeft = Offset(center.x - rx * innerScale, center.y - ry * innerScale),
            size = Size(rx * innerScale * 2f, ry * innerScale * 2f),
            style = Stroke(width = 2.5f),
        )
    }

    // 外圈四向手柄
    listOf(0f, 90f, 180f, 270f).forEach { angle ->
        val point = ellipsePoint(center, rx, ry, mask.rotation, angle)
        drawCircle(Color(0xCC000000), radius = 15f, center = point)
        drawCircle(Color.White, radius = 11f, center = point)
    }
    // 内圈四向手柄
    listOf(0f, 90f, 180f, 270f).forEach { angle ->
        val point = ellipsePoint(center, rx * innerScale, ry * innerScale, mask.rotation, angle)
        drawCircle(Color(0xCC000000), radius = 13f, center = point)
        drawCircle(Color(0xFFFFD166), radius = 9f, center = point)
    }
    // 旋转手柄（外圈上方）
    val rotatePoint = ellipsePoint(center, rx, ry + 56f, mask.rotation, 270f)
    drawLine(Color(0xCCFFFFFF), center, rotatePoint, strokeWidth = 2f)
    drawCircle(Color(0xCC000000), radius = 16f, center = rotatePoint)
    drawCircle(Color(0xFF4A9EFF), radius = 12f, center = rotatePoint)
}

private fun ellipsePoint(
    center: Offset,
    rx: Float,
    ry: Float,
    rotationDegrees: Float,
    angleDegrees: Float,
): Offset {
    val radians = Math.toRadians(angleDegrees.toDouble())
    val localX = rx * cos(radians).toFloat()
    val localY = ry * sin(radians).toFloat()
    val rotation = Math.toRadians(rotationDegrees.toDouble())
    val cosR = cos(rotation).toFloat()
    val sinR = sin(rotation).toFloat()
    return Offset(
        center.x + localX * cosR - localY * sinR,
        center.y + localX * sinR + localY * cosR,
    )
}

/* ------------------------------------------------------------ 交互 */

private enum class MaskDrag {
    None,
    LinearMove,
    LinearStart,
    LinearEnd,
    LinearRotate,
    RadialMove,
    RadialRotate,
    RadialRadiusX,
    RadialRadiusY,
    RadialInnerX,
    RadialInnerY,
}

private fun hitTestMask(
    mask: MaskUi,
    box: IntSize,
    imageAspect: Float,
    position: Offset,
): MaskDrag {
    val fit = maskFitRect(box, imageAspect)
    if (fit.width <= 0f) return MaskDrag.None
    return when (mask.kind) {
        MaskKindUi.Linear -> hitTestLinear(mask, fit, position)
        MaskKindUi.Radial -> hitTestRadial(mask, fit, position)
        else -> MaskDrag.None
    }
}

private fun hitTestLinear(mask: MaskUi, fit: MaskFitRect, position: Offset): MaskDrag {
    val start = Offset(fit.left + mask.start.x * fit.width, fit.top + mask.start.y * fit.height)
    val end = Offset(fit.left + mask.end.x * fit.width, fit.top + mask.end.y * fit.height)
    val diff = end - start
    val length = diff.getDistance().coerceAtLeast(1f)
    val unit = diff / length
    val perp = Offset(-unit.y, unit.x)
    val middle = start + diff * 0.5f

    val gripDistance = fit.width * 0.38f
    val grips = listOf(
        middle - perp * gripDistance,
        middle + perp * gripDistance,
    )
    if (grips.any { hypot(it.x - position.x, it.y - position.y) < 44f }) {
        return MaskDrag.LinearRotate
    }
    if (hypot(start.x - position.x, start.y - position.y) < 48f) {
        return MaskDrag.LinearStart
    }
    if (hypot(end.x - position.x, end.y - position.y) < 48f) {
        return MaskDrag.LinearEnd
    }
    val offset = position - middle
    val perpendicularDistance = abs(offset.x * perp.x + offset.y * perp.y)
    if (perpendicularDistance < 40f) {
        return MaskDrag.LinearMove
    }
    return MaskDrag.None
}

private fun hitTestRadial(mask: MaskUi, fit: MaskFitRect, position: Offset): MaskDrag {
    val center = Offset(fit.left + mask.center.x * fit.width, fit.top + mask.center.y * fit.height)
    val rx = mask.radii.x * fit.width
    val ry = mask.radii.y * fit.height
    val innerScale = mask.inner.coerceIn(0.02f, 0.98f)

    val rotatePoint = ellipsePoint(center, rx, ry + 56f, mask.rotation, 270f)
    if (hypot(rotatePoint.x - position.x, rotatePoint.y - position.y) < 48f) {
        return MaskDrag.RadialRotate
    }
    listOf(0f, 90f, 180f, 270f).forEach { angle ->
        val point = ellipsePoint(center, rx, ry, mask.rotation, angle)
        if (hypot(point.x - position.x, point.y - position.y) < 48f) {
            return if (angle == 0f || angle == 180f) {
                MaskDrag.RadialRadiusX
            } else {
                MaskDrag.RadialRadiusY
            }
        }
    }
    listOf(0f, 90f, 180f, 270f).forEach { angle ->
        val point = ellipsePoint(center, rx * innerScale, ry * innerScale, mask.rotation, angle)
        if (hypot(point.x - position.x, point.y - position.y) < 48f) {
            return if (angle == 0f || angle == 180f) {
                MaskDrag.RadialInnerX
            } else {
                MaskDrag.RadialInnerY
            }
        }
    }
    // 内圈内部：整体移动
    val local = toLocal(position - center, mask.rotation)
    val innerRx = (rx * innerScale).coerceAtLeast(1f)
    val innerRy = (ry * innerScale).coerceAtLeast(1f)
    if ((local.x / innerRx).pow(2) + (local.y / innerRy).pow(2) <= 1f) {
        return MaskDrag.RadialMove
    }
    return MaskDrag.None
}

private fun applyMaskDrag(
    mask: MaskUi,
    mode: MaskDrag,
    box: IntSize,
    imageAspect: Float,
    position: Offset,
    previous: Offset,
): MaskUi {
    val fit = maskFitRect(box, imageAspect)
    if (fit.width <= 0f) return mask
    val nx = ((position.x - fit.left) / fit.width).coerceIn(0f, 1f)
    val ny = ((position.y - fit.top) / fit.height).coerceIn(0f, 1f)
    val deltaX = (position.x - previous.x) / fit.width
    val deltaY = (position.y - previous.y) / fit.height

    return when (mode) {
        MaskDrag.None -> mask
        MaskDrag.LinearMove -> mask.copy(
            start = Offset((mask.start.x + deltaX).coerceIn(0f, 1f), (mask.start.y + deltaY).coerceIn(0f, 1f)),
            end = Offset((mask.end.x + deltaX).coerceIn(0f, 1f), (mask.end.y + deltaY).coerceIn(0f, 1f)),
        )

        MaskDrag.LinearStart -> {
            val direction = (mask.end - mask.start).let {
                val length = it.getDistance().coerceAtLeast(1e-4f)
                it / length
            }
            val pointer = Offset(nx, ny)
            val projected = mask.end - direction * ((mask.end - pointer).x * direction.x + (mask.end - pointer).y * direction.y)
            if ((mask.end - projected).getDistance() < 0.03f) mask else mask.copy(start = projected)
        }

        MaskDrag.LinearEnd -> {
            val direction = (mask.end - mask.start).let {
                val length = it.getDistance().coerceAtLeast(1e-4f)
                it / length
            }
            val pointer = Offset(nx, ny)
            val projected = mask.start + direction * ((pointer - mask.start).x * direction.x + (pointer - mask.start).y * direction.y)
            if ((projected - mask.start).getDistance() < 0.03f) mask else mask.copy(end = projected)
        }

        MaskDrag.LinearRotate -> {
            val middle = Offset(
                (mask.start.x + mask.end.x) / 2f,
                (mask.start.y + mask.end.y) / 2f,
            )
            val previousAngle = angleDegrees(previous, fit, middle)
            val currentAngle = angleDegrees(position, fit, middle)
            val delta = normalizeAngle(currentAngle - previousAngle)
            mask.copy(
                start = rotatePoint(mask.start, middle, delta),
                end = rotatePoint(mask.end, middle, delta),
            )
        }

        MaskDrag.RadialMove -> mask.copy(
            center = Offset(
                (mask.center.x + deltaX).coerceIn(0f, 1f),
                (mask.center.y + deltaY).coerceIn(0f, 1f),
            ),
        )

        MaskDrag.RadialRotate -> {
            val previousAngle = angleDegrees(previous, fit, Offset(mask.center.x, mask.center.y))
            val currentAngle = angleDegrees(position, fit, Offset(mask.center.x, mask.center.y))
            mask.copy(rotation = normalizeAngle(mask.rotation + (currentAngle - previousAngle)))
        }

        MaskDrag.RadialRadiusX, MaskDrag.RadialRadiusY -> {
            val centerScreen = Offset(
                fit.left + mask.center.x * fit.width,
                fit.top + mask.center.y * fit.height,
            )
            val local = toLocal(position - centerScreen, mask.rotation)
            if (mode == MaskDrag.RadialRadiusX) {
                mask.copy(radii = Offset((abs(local.x) / fit.width).coerceIn(0.05f, 1.5f), mask.radii.y))
            } else {
                mask.copy(radii = Offset(mask.radii.x, (abs(local.y) / fit.height).coerceIn(0.05f, 1.5f)))
            }
        }

        MaskDrag.RadialInnerX, MaskDrag.RadialInnerY -> {
            val centerScreen = Offset(
                fit.left + mask.center.x * fit.width,
                fit.top + mask.center.y * fit.height,
            )
            val local = toLocal(position - centerScreen, mask.rotation)
            val rx = (mask.radii.x * fit.width).coerceAtLeast(1f)
            val ry = (mask.radii.y * fit.height).coerceAtLeast(1f)
            val scale = if (mode == MaskDrag.RadialInnerX) {
                abs(local.x) / rx
            } else {
                abs(local.y) / ry
            }
            mask.copy(inner = scale.coerceIn(0.05f, 0.98f))
        }
    }
}

private fun toLocal(offset: Offset, rotationDegrees: Float): Offset {
    val radians = Math.toRadians((-rotationDegrees).toDouble())
    val cosR = cos(radians).toFloat()
    val sinR = sin(radians).toFloat()
    return Offset(
        offset.x * cosR - offset.y * sinR,
        offset.x * sinR + offset.y * cosR,
    )
}

private fun angleDegrees(position: Offset, fit: MaskFitRect, centerNormalized: Offset): Float {
    val center = Offset(
        fit.left + centerNormalized.x * fit.width,
        fit.top + centerNormalized.y * fit.height,
    )
    return Math.toDegrees(atan2((position.y - center.y).toDouble(), (position.x - center.x).toDouble()))
        .toFloat()
}

private fun normalizeAngle(angle: Float): Float {
    var value = angle
    while (value > 180f) value -= 360f
    while (value < -180f) value += 360f
    return value
}

private fun rotatePoint(point: Offset, center: Offset, degrees: Float): Offset {
    val radians = Math.toRadians(degrees.toDouble())
    val cosR = cos(radians).toFloat()
    val sinR = sin(radians).toFloat()
    val dx = point.x - center.x
    val dy = point.y - center.y
    return Offset(
        center.x + dx * cosR - dy * sinR,
        center.y + dx * sinR + dy * cosR,
    )
}
