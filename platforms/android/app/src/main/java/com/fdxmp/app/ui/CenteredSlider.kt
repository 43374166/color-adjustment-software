package com.fdxmp.app.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
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

    var width by remember { mutableStateOf(1f) }
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
                detectTapGestures(
                    onTap = { position ->
                        val current = valueState.value
                        val thumbX = valueToX(current, size.width.toFloat())
                        val delta = if (position.x >= thumbX) step else -step
                        val next = (current + delta).coerceIn(start, end)
                        if (next != current) {
                            changeState.value(next)
                            finishState.value()
                        }
                    },
                    onDoubleTap = { position ->
                        val current = valueState.value
                        val thumbX = valueToX(current, size.width.toFloat())
                        if (abs(position.x - thumbX) <= 80f) {
                            changeState.value(neutralValue)
                            finishState.value()
                        }
                    },
                )
            }
            .pointerInput(valueRange) {
                detectDragGestures(
                    onDrag = { change, _ ->
                        change.consume()
                        val target = start +
                            (change.position.x / size.width.toFloat()).coerceIn(0f, 1f) * span
                        changeState.value(target.coerceIn(start, end))
                    },
                    onDragEnd = { finishState.value() },
                )
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
                topLeft = Offset(0f, centerY - 2.5f),
                size = Size(size.width, 5f),
                cornerRadius = CornerRadius(2.5f),
            )
            // 中性点刻度
            drawLine(
                color = Color(0x66FFFFFF),
                start = Offset(neutralX, centerY - 9f),
                end = Offset(neutralX, centerY + 9f),
                strokeWidth = 2f,
            )
            // 进度：仅当偏离中性值时显示，从中性点画到当前值
            if (abs(value - neutralValue) > 1e-4f && abs(thumbX - neutralX) > 0.5f) {
                drawRoundRect(
                    color = activeColor,
                    topLeft = Offset(minOf(neutralX, thumbX), centerY - 2.5f),
                    size = Size(abs(thumbX - neutralX), 5f),
                    cornerRadius = CornerRadius(2.5f),
                )
            }
            // 滑块圆点
            drawCircle(Color(0xFF101010), radius = 14f, center = Offset(thumbX, centerY))
            drawCircle(activeColor, radius = 10f, center = Offset(thumbX, centerY))
        }
    }
}

/** 按量程挑选点击步进：曝光 1，小量程 0.05，其余 5。 */
fun centeredSliderStep(range: ClosedFloatingPointRange<Float>, key: String? = null): Float = when {
    key == "exposure" -> 1f
    range.endInclusive - range.start <= 3f -> 0.05f
    else -> 5f
}
