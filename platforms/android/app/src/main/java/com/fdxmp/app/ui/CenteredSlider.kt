package com.fdxmp.app.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import kotlin.math.abs

/**
 * 中性点锚定的滑块（Lightroom 风格）：
 *
 * - 进度条从中性值（默认 0）画到当前值；等于中性值时**不显示**蓝色进度；
 * - 点击圆点右侧一次 +[step]，左侧一次 -[step]（曝光 step = 1，其余默认 5）；
 * - 拖动滑块连续调整；
 * - 双击圆点回到中性值。
 */
@Composable
fun CenteredSlider(
    value: Float,
    onValueChange: (Float) -> Unit,
    valueRange: ClosedFloatingPointRange<Float>,
    modifier: Modifier = Modifier,
    neutral: Float = 0f,
    step: Float = 5f,
    onValueChangeFinished: () -> Unit = {},
) {
    val start = valueRange.start
    val end = valueRange.endInclusive
    val span = (end - start).coerceAtLeast(1e-6f)
    val neutralValue = neutral.coerceIn(start, end)
    val activeColor = MaterialTheme.colorScheme.primary
    val density = LocalDensity.current
    val thumbRadius = with(density) { 10.dp.toPx() }
    val thumbRingRadius = with(density) { 13.dp.toPx() }
    val trackHeight = with(density) { 5.dp.toPx() }
    val trackHalf = trackHeight / 2f
    val tickHalf = with(density) { 9.dp.toPx() }
    val tickWidth = with(density) { 2.dp.toPx() }

    var width by remember { mutableStateOf(1f) }
    var lastTapTime by remember { mutableStateOf(0L) }
    var lastTapX by remember { mutableStateOf(-1f) }
    val valueState = rememberUpdatedState(value)
    val changeState = rememberUpdatedState(onValueChange)
    val finishState = rememberUpdatedState(onValueChangeFinished)

    fun valueToX(target: Float, total: Float): Float =
        ((target - start) / span).coerceIn(0f, 1f) * total

    Box(
        modifier = modifier
            .fillMaxWidth()
            .height(42.dp)
            .onSizeChanged { width = it.width.toFloat().coerceAtLeast(1f) }
            .pointerInput(valueRange, neutralValue, step) {
                // 单手势循环：>24px 视为拖动（连续），否则视为点击（±step / 双击归零）。
                awaitEachGesture {
                    val down = awaitFirstDown()
                    val startX = down.position.x
                    var moved = false
                    var lastX = startX
                    while (true) {
                        val event = awaitPointerEvent()
                        val change = event.changes.firstOrNull { it.id == down.id } ?: break
                        if (!change.pressed) break
                        if (abs(change.position.x - startX) > 24f) {
                            moved = true
                            change.consume()
                            lastX = change.position.x
                            val target = start +
                                (lastX / size.width.toFloat()).coerceIn(0f, 1f) * span
                            changeState.value(target.coerceIn(start, end))
                        }
                    }
                    if (moved) {
                        finishState.value()
                    } else {
                        val current = valueState.value
                        val thumbX = valueToX(current, size.width.toFloat())
                        val now = System.currentTimeMillis()
                        // 双击判定按两次点击的位置：圆点在两次点击之间会移动。
                        val doubleTap = now - lastTapTime < 320 &&
                            lastTapX >= 0f &&
                            abs(startX - lastTapX) <= 60f
                        lastTapTime = now
                        lastTapX = startX
                        if (doubleTap) {
                            changeState.value(neutralValue)
                        } else {
                            val delta = if (startX >= thumbX) step else -step
                            val next = (current + delta).coerceIn(start, end)
                            if (next != current) changeState.value(next)
                        }
                        finishState.value()
                    }
                }
            },
    ) {
        Canvas(modifier = Modifier.fillMaxWidth().height(42.dp)) {
            val centerY = size.height / 2f
            val trackColor = Color(0xFF3C3C3C)
            val thumbX = valueToX(value, size.width)
            val neutralX = valueToX(neutralValue, size.width)

            // 轨道
            drawRoundRect(
                color = trackColor,
                topLeft = Offset(0f, centerY - trackHalf),
                size = Size(size.width, trackHeight),
                cornerRadius = CornerRadius(trackHalf),
            )
            // 中性点刻度
            drawLine(
                color = Color(0x66FFFFFF),
                start = Offset(neutralX, centerY - tickHalf),
                end = Offset(neutralX, centerY + tickHalf),
                strokeWidth = tickWidth,
            )
            // 进度：仅当偏离中性值时显示，从中性点画到当前值
            if (abs(value - neutralValue) > 1e-4f && abs(thumbX - neutralX) > 0.5f) {
                drawRoundRect(
                    color = activeColor,
                    topLeft = Offset(minOf(neutralX, thumbX), centerY - trackHalf),
                    size = Size(abs(thumbX - neutralX), trackHeight),
                    cornerRadius = CornerRadius(trackHalf),
                )
            }
            // 滑块圆点
            drawCircle(Color(0xFF101010), radius = thumbRingRadius, center = Offset(thumbX, centerY))
            drawCircle(activeColor, radius = thumbRadius, center = Offset(thumbX, centerY))
        }
    }
}

/** 按量程挑选点击步进：曝光 1，小量程 0.05，其余 5。 */
fun centeredSliderStep(range: ClosedFloatingPointRange<Float>, key: String? = null): Float = when {
    key == "exposure" -> 1f
    range.endInclusive - range.start <= 3f -> 0.05f
    else -> 5f
}
