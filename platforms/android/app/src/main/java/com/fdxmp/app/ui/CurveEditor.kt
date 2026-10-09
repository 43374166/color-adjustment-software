package com.fdxmp.app.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Timeline
import androidx.compose.material3.Icon
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import java.util.Locale
import kotlin.math.abs
import kotlin.math.hypot

/** 曲线上的一个控制点（归一化坐标）。 */
data class CurvePointUi(val x: Float, val y: Float)

/** 曲线通道：RGB 主通道 / 单通道 / 参数曲线。 */
enum class CurveChannel(val label: String) {
    Rgb("白色"),
    Red("红"),
    Green("绿"),
    Blue("蓝"),
    Parametric("参数"),
}

/** 参数曲线（高光 / 中间调 / 阴影 + 两个分界）。 */
data class ParametricUi(
    val highlights: Float = 0f,
    val midtones: Float = 0f,
    val shadows: Float = 0f,
    val highlightsSplit: Float = 0.75f,
    val midtonesSplit: Float = 0.25f,
)

/** 默认曲线：两个端点，恒等映射。 */
fun identityCurve(): List<CurvePointUi> = listOf(
    CurvePointUi(0f, 0f),
    CurvePointUi(1f, 1f),
)

/** 四个通道的默认（恒等）曲线。 */
fun identityCurveMap(): Map<CurveChannel, List<CurvePointUi>> = mapOf(
    CurveChannel.Rgb to identityCurve(),
    CurveChannel.Red to identityCurve(),
    CurveChannel.Green to identityCurve(),
    CurveChannel.Blue to identityCurve(),
)

/**
 * 透明曲线覆盖层：直接叠在图片上，只画网格 + 曲线 + 控制点，不遮挡画面。
 *
 * 交互：点空白添加控制点、点已有控制点删除、拖动控制点调整（参数模式不可拖点）。
 */
@Composable
fun CurveSurface(
    points: Map<CurveChannel, List<CurvePointUi>>,
    parametric: ParametricUi,
    channel: CurveChannel,
    modifier: Modifier = Modifier,
    onPointsChange: (CurveChannel, List<CurvePointUi>) -> Unit,
) {
    var canvasSize by remember { mutableStateOf(IntSize.Zero) }
    var draggingIndex by remember { mutableStateOf(-1) }

    val activePoints = points[channel] ?: identityCurve()
    val isParametric = channel == CurveChannel.Parametric

    val pointsState = rememberUpdatedState(activePoints)
    val channelState = rememberUpdatedState(channel)
    val changeState = rememberUpdatedState(onPointsChange)

    Box(modifier = modifier) {
        Canvas(
            modifier = Modifier
                .fillMaxSize()
                .onSizeChanged { canvasSize = it }
                .pointerInput(Unit) {
                    detectTapGestures { position ->
                        val currentChannel = channelState.value
                        if (currentChannel == CurveChannel.Parametric) return@detectTapGestures
                        val current = pointsState.value
                        val nearest = nearestIndex(current, position, size, 56f)
                        if (nearest >= 0) {
                            if (nearest in 1 until current.size - 1) {
                                changeState.value(
                                    currentChannel,
                                    current.filterIndexed { index, _ -> index != nearest },
                                )
                            }
                        } else {
                            val x = (position.x / size.width).coerceIn(0.02f, 0.98f)
                            val y = (1f - position.y / size.height).coerceIn(0f, 1f)
                            changeState.value(
                                currentChannel,
                                (current + CurvePointUi(x, y)).sortedBy { it.x },
                            )
                        }
                    }
                }
                .pointerInput(Unit) {
                    detectDragGestures(
                        onDragStart = { position ->
                            val currentChannel = channelState.value
                            if (currentChannel == CurveChannel.Parametric) {
                                return@detectDragGestures
                            }
                            draggingIndex = nearestIndex(
                                pointsState.value,
                                position,
                                size,
                                72f,
                            )
                        },
                        onDrag = { change, _ ->
                            val currentChannel = channelState.value
                            if (currentChannel == CurveChannel.Parametric) {
                                return@detectDragGestures
                            }
                            val index = draggingIndex
                            val current = pointsState.value
                            if (index in current.indices) {
                                change.consume()
                                val rawX = (change.position.x / size.width).coerceIn(0f, 1f)
                                val rawY = (1f - change.position.y / size.height).coerceIn(0f, 1f)
                                val left = if (index > 0) current[index - 1].x + 0.015f else 0f
                                val right = if (index < current.size - 1) {
                                    current[index + 1].x - 0.015f
                                } else {
                                    1f
                                }
                                val x = when (index) {
                                    0 -> 0f
                                    current.size - 1 -> 1f
                                    else -> rawX.coerceIn(left, right)
                                }
                                changeState.value(
                                    currentChannel,
                                    current.toMutableList().also {
                                        it[index] = CurvePointUi(x, rawY)
                                    },
                                )
                            }
                        },
                        onDragEnd = { draggingIndex = -1 },
                        onDragCancel = { draggingIndex = -1 },
                    )
                },
        ) {
            val gridColor = Color(0x33FFFFFF)
            for (step in 1..3) {
                val x = size.width * step / 4f
                val y = size.height * step / 4f
                drawLine(gridColor, Offset(x, 0f), Offset(x, size.height), strokeWidth = 1f)
                drawLine(gridColor, Offset(0f, y), Offset(size.width, y), strokeWidth = 1f)
            }
            drawLine(Color(0x22FFFFFF), Offset(0f, 0f), Offset(size.width, size.height), 1f)

            val path = Path()
            val samples = 128
            for (index in 0..samples) {
                val x = index / samples.toFloat()
                val y = if (isParametric) {
                    evalParametric(x, parametric)
                } else {
                    evalCurve(activePoints, x)
                }
                val px = x * size.width
                val py = (1f - y) * size.height
                if (index == 0) path.moveTo(px, py) else path.lineTo(px, py)
            }
            drawPath(path, color = channelColor(channel), style = Stroke(width = 4f))

            if (!isParametric) {
                activePoints.forEach { point ->
                    val center = Offset(point.x * size.width, (1f - point.y) * size.height)
                    drawCircle(Color(0x99000000), radius = 13f, center = center)
                    drawCircle(Color.White, radius = 10f, center = center)
                    drawCircle(Color(0x66000000), radius = 10f, center = center, style = Stroke(2f))
                }
            }
        }

        if (!isParametric) {
            Text(
                "点空白加点 · 点控制点删除 · 拖动调整",
                style = MaterialTheme.typography.labelSmall,
                color = Color(0x99FFFFFF),
                textAlign = TextAlign.Center,
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .padding(bottom = 8.dp)
                    .background(Color(0x66000000), RoundedCornerShape(6.dp))
                    .padding(horizontal = 8.dp, vertical = 3.dp),
            )
        }
    }
}

/** 曲线控制面板：标题 + 完成 + 通道选择（参数模式带 5 个滑块）。 */
@Composable
fun CurveControls(
    channel: CurveChannel,
    onChannelChange: (CurveChannel) -> Unit,
    parametric: ParametricUi,
    onParametricChange: (ParametricUi) -> Unit,
    onDone: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier.fillMaxSize()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("曲线", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.weight(1f))
            OutlinedButton(onClick = onDone, shape = RoundedCornerShape(12.dp)) {
                Text("完成")
            }
        }

        if (channel == CurveChannel.Parametric) {
            LazyColumn(
                modifier = Modifier.fillMaxWidth().weight(1f),
                contentPadding = PaddingValues(horizontal = 16.dp),
            ) {
                items(
                    items = listOf(
                        Triple("高光", parametric.highlights) { value: Float ->
                            onParametricChange(parametric.copy(highlights = value))
                        },
                        Triple("中间调", parametric.midtones) { value: Float ->
                            onParametricChange(parametric.copy(midtones = value))
                        },
                        Triple("阴影", parametric.shadows) { value: Float ->
                            onParametricChange(parametric.copy(shadows = value))
                        },
                        Triple("高光分界", parametric.highlightsSplit) { value: Float ->
                            onParametricChange(parametric.copy(highlightsSplit = value))
                        },
                        Triple("中间调分界", parametric.midtonesSplit) { value: Float ->
                            onParametricChange(parametric.copy(midtonesSplit = value))
                        },
                    ),
                    key = { it.first },
                ) { (label, value, updater) ->
                    Column {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(
                                label,
                                style = MaterialTheme.typography.bodySmall,
                                modifier = Modifier.weight(1f),
                            )
                            Text(
                                if (label.contains("分界")) {
                                    String.format(Locale.US, "%.2f", value)
                                } else {
                                    String.format(Locale.US, "%+.0f", value)
                                },
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        val range = if (label == "高光分界") {
                            0.5f..1f
                        } else if (label == "中间调分界") {
                            0f..0.5f
                        } else {
                            -100f..100f
                        }
                        Slider(
                            value = value,
                            onValueChange = { updater(it) },
                            valueRange = range,
                        )
                    }
                }
            }
        } else {
            Spacer(Modifier.weight(1f))
        }

        Row(
            modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp),
            horizontalArrangement = Arrangement.SpaceEvenly,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            CurveChannel.entries.forEach { item ->
                ChannelButton(
                    channel = item,
                    selected = item == channel,
                    onClick = { onChannelChange(item) },
                )
            }
        }
    }
}

@Composable
private fun ChannelButton(
    channel: CurveChannel,
    selected: Boolean,
    onClick: () -> Unit,
) {
    val color = channelColor(channel)
    Box(
        modifier = Modifier
            .size(44.dp)
            .clip(CircleShape)
            .background(if (selected) Color(0x33FFFFFF) else Color.Transparent)
            .clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        if (channel == CurveChannel.Parametric) {
            Icon(
                Icons.Filled.Timeline,
                contentDescription = channel.label,
                tint = if (selected) Color.White else MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(24.dp),
            )
        } else {
            Box(
                modifier = Modifier
                    .size(26.dp)
                    .clip(CircleShape)
                    .background(if (channel == CurveChannel.Rgb) Color.White else Color.Transparent)
                    .border(3.dp, color, CircleShape),
            )
        }
    }
}

private fun channelColor(channel: CurveChannel): Color = when (channel) {
    CurveChannel.Rgb -> Color.White
    CurveChannel.Red -> Color(0xFFFF4D4D)
    CurveChannel.Green -> Color(0xFF43D96B)
    CurveChannel.Blue -> Color(0xFF4D8DFF)
    CurveChannel.Parametric -> Color.White
}

private fun nearestIndex(
    points: List<CurvePointUi>,
    position: Offset,
    size: IntSize,
    threshold: Float,
): Int {
    if (size.width <= 0 || size.height <= 0) return -1
    var best = -1
    var bestDistance = threshold
    points.forEachIndexed { index, point ->
        val px = point.x * size.width
        val py = (1f - point.y) * size.height
        val distance = hypot(px - position.x, py - position.y)
        if (distance < bestDistance) {
            bestDistance = distance
            best = index
        }
    }
    return best
}

/** 与 Rust `params::eval_curve` 相同的单调三次 Hermite 求值。 */
fun evalCurve(points: List<CurvePointUi>, x: Float): Float {
    if (points.isEmpty()) return x
    if (points.size == 1) return points[0].y
    if (x <= points.first().x) return points.first().y
    if (x >= points.last().x) return points.last().y
    var index = 0
    while (index + 2 < points.size && x > points[index + 1].x) index += 1
    val left = points[index]
    val right = points[index + 1]
    val span = (right.x - left.x).coerceAtLeast(1e-6f)
    val t = ((x - left.x) / span).coerceIn(0f, 1f)
    val m0 = curveTangent(points, index)
    val m1 = curveTangent(points, index + 1)
    val t2 = t * t
    val t3 = t2 * t
    val h00 = 2f * t3 - 3f * t2 + 1f
    val h10 = t3 - 2f * t2 + t
    val h01 = -2f * t3 + 3f * t2
    val h11 = t3 - t2
    return (h00 * left.y + h10 * span * m0 + h01 * right.y + h11 * span * m1).coerceIn(0f, 1f)
}

private fun curveTangent(points: List<CurvePointUi>, index: Int): Float {
    fun secant(i: Int): Float {
        val span = (points[i + 1].x - points[i].x).coerceAtLeast(1e-6f)
        return (points[i + 1].y - points[i].y) / span
    }
    if (index == 0) return secant(0)
    if (index + 1 >= points.size) return secant(points.size - 2)
    val previous = secant(index - 1)
    val next = secant(index)
    if (previous * next <= 0f) return 0f
    val slope = (previous + next) * 0.5f
    val limit = 3f * minOf(abs(previous), abs(next))
    return slope.coerceIn(-limit, limit)
}

/** 参数曲线求值（与 Rust `preview::build_parametric_lut` 同一公式）。 */
fun evalParametric(x: Float, parametric: ParametricUi): Float {
    val highlightsSplit = if (parametric.highlightsSplit <= 0f) {
        0.75f
    } else {
        parametric.highlightsSplit.coerceIn(0.5f, 1f)
    }
    val midtonesSplit = if (parametric.midtonesSplit <= 0f) {
        0.25f
    } else {
        parametric.midtonesSplit.coerceIn(0f, 0.5f)
    }
    val upper = maxOf(highlightsSplit, midtonesSplit + 0.1f)
    val weightHighlights = smoothstep(midtonesSplit, upper, x)
    val weightShadows = 1f - weightHighlights
    val weightMidtones = (1f - weightHighlights - weightShadows).coerceAtLeast(0f)
    val delta = (parametric.highlights / 100f) * weightHighlights * 0.35f +
        (parametric.midtones / 100f) * weightMidtones * 0.35f +
        (parametric.shadows / 100f) * weightShadows * 0.35f
    return (x + delta).coerceIn(0f, 1f)
}

private fun smoothstep(edge0: Float, edge1: Float, x: Float): Float {
    val t = ((x - edge0) / (edge1 - edge0)).coerceIn(0f, 1f)
    return t * t * (3f - 2f * t)
}
