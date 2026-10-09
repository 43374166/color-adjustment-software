package com.fdxmp.app.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
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
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
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

    Box(
        modifier = modifier
            .onSizeChanged { boxSize = it }
            .pointerInput(boxSize, imageAspect, mask.id) {
                detectDragGestures(
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
            },
    ) {
        Canvas(modifier = Modifier.fillMaxSize()) {
            drawMask(mask, imageAspect)
        }
    }
}

/** 蒙版列表行：选择 / 添加（线性、径向）/ 删除 / 完成。 */
@Composable
fun MaskChipRow(
    masks: List<MaskUi>,
    selectedIndex: Int,
    onSelect: (Int) -> Unit,
    onAdd: (MaskKindUi) -> Unit,
    onRemove: (Int) -> Unit,
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
            }
        }
        Spacer(Modifier.width(8.dp))
        OutlinedButton(onClick = onDone, shape = RoundedCornerShape(12.dp)) {
            Text("完成")
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
