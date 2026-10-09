package com.fdxmp.app.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import java.util.Locale
import kotlin.math.abs

/** 归一化裁剪矩形。 */
data class CropRect(val x: Float, val y: Float, val w: Float, val h: Float)

/** 裁剪比例（ratio = 像素宽 / 像素高；null 表示不约束）。 */
enum class CropAspect(val label: String, val ratio: Float?, val serdeName: String) {
    Free("自由", null, "free"),
    Original("原始", null, "original"),
    Square("1:1", 1f, "square"),
    FourThree("4:3", 4f / 3f, "four_three"),
    ThreeTwo("3:2", 3f / 2f, "three_two"),
    SixteenNine("16:9", 16f / 9f, "sixteen_nine"),
}

/** 裁切状态。 */
data class CropUi(
    val rect: CropRect = CropRect(0f, 0f, 1f, 1f),
    val aspect: CropAspect = CropAspect.Free,
    val rotation: Float = 0f,
    val flipHorizontal: Boolean = false,
    val flipVertical: Boolean = false,
)

/** 解析当前生效的像素比例（Original 用图片自身比例）。 */
fun CropUi.resolvedAspect(imageAspect: Float): Float? = when (aspect) {
    CropAspect.Free -> null
    CropAspect.Original -> imageAspect
    else -> aspect.ratio
}

/**
 * 透明裁切覆盖层：暗角 + 裁剪框 + 三分线 + 8 个手柄，直接叠在图片上。
 * 拖动角 / 边调整大小，按住框内移动。
 */
@Composable
fun CropOverlay(
    crop: CropUi,
    imageAspect: Float,
    onCropChange: (CropUi) -> Unit,
    modifier: Modifier = Modifier,
) {
    var boxSize by remember { mutableStateOf(IntSize.Zero) }
    var dragMode by remember { mutableStateOf(CropDragMode.None) }

    val cropState = rememberUpdatedState(crop)
    val changeState = rememberUpdatedState(onCropChange)
    val aspectState = rememberUpdatedState(imageAspect)

    Box(
        modifier = modifier
            .onSizeChanged { boxSize = it }
            .pointerInput(boxSize, imageAspect) {
                detectDragGestures(
                    onDragStart = { position ->
                        dragMode = hitTest(
                            crop = cropState.value,
                            box = boxSize,
                            imageAspect = aspectState.value,
                            position = position,
                        )
                    },
                    onDrag = { change, delta ->
                        if (dragMode == CropDragMode.None) return@detectDragGestures
                        change.consume()
                        changeState.value(
                            applyCropDrag(
                                crop = cropState.value,
                                mode = dragMode,
                                delta = delta,
                                box = boxSize,
                                imageAspect = aspectState.value,
                            ),
                        )
                    },
                    onDragEnd = { dragMode = CropDragMode.None },
                    onDragCancel = { dragMode = CropDragMode.None },
                )
            },
    ) {
        Canvas(modifier = Modifier.fillMaxSize()) {
            drawCropOverlay(crop.rect, imageAspect)
        }
    }
}

/** 裁切控制面板：比例 / 校正（旋转）/ 翻转 / 重置 / 完成。 */
@Composable
fun CropControls(
    crop: CropUi,
    imageAspect: Float,
    onCropChange: (CropUi) -> Unit,
    onReset: () -> Unit,
    onDone: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("裁切", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.weight(1f))
            Text(
                "重置",
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier
                    .clip(RoundedCornerShape(8.dp))
                    .clickable(onClick = onReset)
                    .padding(horizontal = 10.dp, vertical = 6.dp),
            )
            Spacer(Modifier.width(8.dp))
            OutlinedButton(onClick = onDone, shape = RoundedCornerShape(12.dp)) {
                Text("完成")
            }
        }

        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 2.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            CropAspect.entries.forEach { option ->
                val active = option == crop.aspect
                Text(
                    option.label,
                    style = MaterialTheme.typography.labelMedium,
                    color = if (active) Color.White else MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier
                        .weight(1f)
                        .clip(RoundedCornerShape(9.dp))
                        .background(
                            if (active) MaterialTheme.colorScheme.primary else Color(0xFF1E1E1E),
                        )
                        .clickable {
                            onCropChange(
                                crop.copy(
                                    aspect = option,
                                    rect = fitCropToAspect(
                                        rect = crop.rect,
                                        aspect = if (option == CropAspect.Free) {
                                            null
                                        } else {
                                            option.ratio ?: imageAspect
                                        },
                                        imageAspect = imageAspect,
                                    ),
                                ),
                            )
                        }
                        .padding(vertical = 7.dp),
                    textAlign = TextAlign.Center,
                )
            }
        }

        Column(modifier = Modifier.padding(horizontal = 16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "校正",
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.weight(1f),
                )
                Text(
                    String.format(Locale.US, "%+.1f°", crop.rotation),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Slider(
                value = crop.rotation,
                onValueChange = { onCropChange(crop.copy(rotation = it)) },
                valueRange = -45f..45f,
            )
        }

        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 2.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            CropToggle(
                label = "水平翻转",
                active = crop.flipHorizontal,
                modifier = Modifier.weight(1f),
                onClick = {
                    onCropChange(crop.copy(flipHorizontal = !crop.flipHorizontal))
                },
            )
            CropToggle(
                label = "垂直翻转",
                active = crop.flipVertical,
                modifier = Modifier.weight(1f),
                onClick = {
                    onCropChange(crop.copy(flipVertical = !crop.flipVertical))
                },
            )
        }
    }
}

@Composable
private fun CropToggle(
    label: String,
    active: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Text(
        label,
        style = MaterialTheme.typography.labelMedium,
        color = if (active) Color.White else MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = modifier
            .clip(RoundedCornerShape(9.dp))
            .background(if (active) MaterialTheme.colorScheme.primary else Color(0xFF1E1E1E))
            .clickable(onClick = onClick)
            .padding(vertical = 8.dp),
        textAlign = TextAlign.Center,
    )
}

/* ------------------------------------------------------------ 几何工具 */

private enum class CropDragMode {
    None,
    Move,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

private data class FitRect(val left: Float, val top: Float, val width: Float, val height: Float)

private fun fitRect(box: IntSize, imageAspect: Float): FitRect {
    if (box.width <= 0 || box.height <= 0 || imageAspect <= 0f) {
        return FitRect(0f, 0f, 0f, 0f)
    }
    val boxAspect = box.width.toFloat() / box.height
    return if (imageAspect > boxAspect) {
        val width = box.width.toFloat()
        val height = width / imageAspect
        FitRect(0f, (box.height - height) / 2f, width, height)
    } else {
        val height = box.height.toFloat()
        val width = height * imageAspect
        FitRect((box.width - width) / 2f, 0f, width, height)
    }
}

private fun DrawScope.drawCropOverlay(rect: CropRect, imageAspect: Float) {
    val box = IntSize(size.width.toInt(), size.height.toInt())
    val fit = fitRect(box, imageAspect)
    if (fit.width <= 0f) return
    val left = fit.left + rect.x * fit.width
    val top = fit.top + rect.y * fit.height
    val right = left + rect.w * fit.width
    val bottom = top + rect.h * fit.height

    val scrim = Color(0xAA000000)
    drawRect(scrim, Offset(0f, 0f), Size(size.width, top.coerceAtLeast(0f)))
    drawRect(
        scrim,
        Offset(0f, bottom),
        Size(size.width, (size.height - bottom).coerceAtLeast(0f)),
    )
    drawRect(
        scrim,
        Offset(0f, top),
        Size(left.coerceAtLeast(0f), (bottom - top).coerceAtLeast(0f)),
    )
    drawRect(
        scrim,
        Offset(right, top),
        Size((size.width - right).coerceAtLeast(0f), (bottom - top).coerceAtLeast(0f)),
    )

    val gridColor = Color(0x66FFFFFF)
    for (step in 1..2) {
        val x = left + (right - left) * step / 3f
        val y = top + (bottom - top) * step / 3f
        drawLine(gridColor, Offset(x, top), Offset(x, bottom), strokeWidth = 1f)
        drawLine(gridColor, Offset(left, y), Offset(right, y), strokeWidth = 1f)
    }

    drawRect(
        color = Color.White,
        topLeft = Offset(left, top),
        size = Size(right - left, bottom - top),
        style = Stroke(width = 3f),
    )

    val handle = 34f
    val stroke = 6f
    drawLine(Color.White, Offset(left, top), Offset(left + handle, top), stroke)
    drawLine(Color.White, Offset(left, top), Offset(left, top + handle), stroke)
    drawLine(Color.White, Offset(right, top), Offset(right - handle, top), stroke)
    drawLine(Color.White, Offset(right, top), Offset(right, top + handle), stroke)
    drawLine(Color.White, Offset(left, bottom), Offset(left + handle, bottom), stroke)
    drawLine(Color.White, Offset(left, bottom), Offset(left, bottom - handle), stroke)
    drawLine(Color.White, Offset(right, bottom), Offset(right - handle, bottom), stroke)
    drawLine(Color.White, Offset(right, bottom), Offset(right, bottom - handle), stroke)
}

private fun hitTest(
    crop: CropUi,
    box: IntSize,
    imageAspect: Float,
    position: Offset,
): CropDragMode {
    val fit = fitRect(box, imageAspect)
    if (fit.width <= 0f || fit.height <= 0f) return CropDragMode.None
    val rect = crop.rect
    val left = fit.left + rect.x * fit.width
    val top = fit.top + rect.y * fit.height
    val right = left + rect.w * fit.width
    val bottom = top + rect.h * fit.height
    val corner = 64f
    val edge = 40f
    val aspectLocked = crop.resolvedAspect(imageAspect) != null

    val nearLeft = abs(position.x - left) < corner
    val nearRight = abs(position.x - right) < corner
    val nearTop = abs(position.y - top) < corner
    val nearBottom = abs(position.y - bottom) < corner
    return when {
        nearLeft && nearTop -> CropDragMode.TopLeft
        nearRight && nearTop -> CropDragMode.TopRight
        nearLeft && nearBottom -> CropDragMode.BottomLeft
        nearRight && nearBottom -> CropDragMode.BottomRight
        !aspectLocked && abs(position.x - left) < edge -> CropDragMode.Left
        !aspectLocked && abs(position.x - right) < edge -> CropDragMode.Right
        !aspectLocked && abs(position.y - top) < edge -> CropDragMode.Top
        !aspectLocked && abs(position.y - bottom) < edge -> CropDragMode.Bottom
        position.x in left..right && position.y in top..bottom -> CropDragMode.Move
        else -> CropDragMode.None
    }
}

private fun applyCropDrag(
    crop: CropUi,
    mode: CropDragMode,
    delta: Offset,
    box: IntSize,
    imageAspect: Float,
): CropUi {
    val fit = fitRect(box, imageAspect)
    if (fit.width <= 0f || fit.height <= 0f) return crop
    val dx = delta.x / fit.width
    val dy = delta.y / fit.height
    val minSize = 0.08f
    val aspect = crop.resolvedAspect(imageAspect)
    var rect = crop.rect

    when (mode) {
        CropDragMode.None -> return crop
        CropDragMode.Move -> {
            rect = rect.copy(
                x = (rect.x + dx).coerceIn(0f, 1f - rect.w),
                y = (rect.y + dy).coerceIn(0f, 1f - rect.h),
            )
        }

        CropDragMode.Left -> {
            val nx = (rect.x + dx).coerceIn(0f, rect.x + rect.w - minSize)
            rect = rect.copy(x = nx, w = rect.w + (rect.x - nx))
        }

        CropDragMode.Right -> {
            rect = rect.copy(w = (rect.w + dx).coerceIn(minSize, 1f - rect.x))
        }

        CropDragMode.Top -> {
            val ny = (rect.y + dy).coerceIn(0f, rect.y + rect.h - minSize)
            rect = rect.copy(y = ny, h = rect.h + (rect.y - ny))
        }

        CropDragMode.Bottom -> {
            rect = rect.copy(h = (rect.h + dy).coerceIn(minSize, 1f - rect.y))
        }

        else -> {
            val rightSide = mode == CropDragMode.TopRight || mode == CropDragMode.BottomRight
            val bottomSide = mode == CropDragMode.BottomLeft || mode == CropDragMode.BottomRight
            val anchorX = if (rightSide) rect.x else rect.x + rect.w
            val anchorY = if (bottomSide) rect.y else rect.y + rect.h
            var pointerX = if (rightSide) rect.x + rect.w + dx else rect.x + dx
            var pointerY = if (bottomSide) rect.y + rect.h + dy else rect.y + dy
            pointerX = pointerX.coerceIn(0f, 1f)
            pointerY = pointerY.coerceIn(0f, 1f)

            var w = abs(pointerX - anchorX).coerceAtLeast(minSize)
            var h = abs(pointerY - anchorY).coerceAtLeast(minSize)
            if (aspect != null) {
                val ratio = aspect / imageAspect
                val targetH = w / ratio
                if (targetH > h) {
                    h = targetH
                } else {
                    w = h * ratio
                }
            }
            val maxW = if (rightSide) 1f - anchorX else anchorX
            val maxH = if (bottomSide) 1f - anchorY else anchorY
            if (w > maxW) {
                w = maxW
                if (aspect != null) h = w / (aspect / imageAspect)
            }
            if (h > maxH) {
                h = maxH
                if (aspect != null) w = h * (aspect / imageAspect)
            }
            val x = if (rightSide) anchorX else anchorX - w
            val y = if (bottomSide) anchorY else anchorY - h
            rect = CropRect(
                x = x.coerceIn(0f, 1f - w),
                y = y.coerceIn(0f, 1f - h),
                w = w,
                h = h,
            )
        }
    }
    return crop.copy(rect = rect)
}

/** 按目标比例调整现有矩形（居中、缩放到画幅内）。 */
fun fitCropToAspect(rect: CropRect, aspect: Float?, imageAspect: Float): CropRect {
    if (aspect == null || imageAspect <= 0f) return rect
    val centerX = rect.x + rect.w / 2f
    val centerY = rect.y + rect.h / 2f
    var w = rect.w
    var h = w * imageAspect / aspect
    if (h > 1f) {
        h = 1f
        w = h * aspect / imageAspect
    }
    if (w > 1f) {
        w = 1f
        h = w * imageAspect / aspect
    }
    val x = (centerX - w / 2f).coerceIn(0f, 1f - w)
    val y = (centerY - h / 2f).coerceIn(0f, 1f - h)
    return CropRect(x, y, w, h)
}
