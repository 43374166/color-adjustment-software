package com.fdxmp.app.ui

import android.graphics.Bitmap
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
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
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import kotlin.math.abs
import kotlin.math.hypot
import java.util.Locale

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
 * 全屏曲线编辑器（手机 Lightroom 版式）：
 * 图片 + 网格 + 可拖拽控制点；底部「曲线 / 完成」与通道选择（白 / 红 / 绿 / 蓝 / 参数）。
 *
 * 交互：点空白添加控制点、点已有控制点删除、拖动控制点调整。
 */
@Composable
fun CurveEditorOverlay(
    image: Bitmap?,
    points: Map<CurveChannel, List<CurvePointUi>>,
    parametric: ParametricUi,
    onPointsChange: (CurveChannel, List<CurvePointUi>) -> Unit,
    onParametricChange: (ParametricUi) -> Unit,
    onDone: () -> Unit,
) {
    var channel by remember { mutableStateOf(CurveChannel.Rgb) }
    var canvasSize by remember { mutableStateOf(IntSize.Zero) }
    var draggingIndex by remember { mutableStateOf(-1) }

    val activePoints = points[channel] ?: identityCurve()
    val isParametric = channel == CurveChannel.Parametric

    val pointsState = rememberUpdatedState(activePoints)
    val channelState = rememberUpdatedState(channel)
    val parametricState = rememberUpdatedState(parametric)
    val changeState = rememberUpdatedState(onPointsChange)

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black)
            .statusBarsPadding()
            .navigationBarsPadding(),
    ) {
        Box(
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth(),
        ) {
            if (image != null) {
                Image(
                    bitmap = image.asImageBitmap(),
                    contentDescription = null,
                    contentScale = ContentScale.Fit,
                    modifier = Modifier.fillMaxSize().alpha(0.5f),
                )
            }
            Box(modifier = Modifier.fillMaxSize().background(Color(0x66000000)))

            Canvas(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(12.dp)
                    .onSizeChanged { canvasSize = it }
                    .pointerInput(Unit) {
                        detectTapGestures { position ->
                            val currentChannel = channelState.value
                            if (currentChannel == CurveChannel.Parametric) return@detectTapGestures
                            val current = pointsState.value
                            val threshold = 56f
                            val nearest = nearestIndex(current, position, size.toIntSizeSafe(), threshold)
                            if (nearest >= 0) {
                                if (nearest in 1 until current.size - 1) {
                                    changeState.value(currentChannel, current.filterIndexed { i, _ -> i != nearest })
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
                                if (currentChannel == CurveChannel.Parametric) return@detectDragGestures
                                draggingIndex = nearestIndex(
                                    pointsState.value,
                                    position,
                                    size.toIntSizeSafe(),
                                    72f,
                                )
                            },
                            onDrag = { change, _ ->
                                val currentChannel = channelState.value
                                if (currentChannel == CurveChannel.Parametric) return@detectDragGestures
                                val index = draggingIndex
                                val current = pointsState.value
                                if (index in current.indices) {
                                    change.consume()
                                    val rawX = (change.position.x / size.width).coerceIn(0f, 1f)
                                    val rawY = (1f - change.position.y / size.height).coerceIn(0f, 1f)
                                    val left = if (index > 0) current[index - 1].x + 0.015f else 0f
                                    val right = if (index < current.size - 1) current[index + 1].x - 0.015f else 1f
                                    val x = when (index) {
                                        0 -> 0f
                                        current.size - 1 -> 1f
                                        else -> rawX.coerceIn(left, right)
                                    }
                                    val updated = current.toMutableList().also {
                                        it[index] = CurvePointUi(x, rawY)
                                    }
                                    changeState.value(currentChannel, updated)
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
                drawLine(gridColor, Offset(0f, 0f), Offset(size.width, size.height), strokeWidth = 1f)

                val currentChannel = channel
                val path = Path()
                val samples = 128
                for (index in 0..samples) {
                    val x = index / samples.toFloat()
                    val y = if (currentChannel == CurveChannel.Parametric) {
                        evalParametric(x, parametric)
                    } else {
                        evalCurve(activePoints, x)
                    }
                    val px = x * size.width
                    val py = (1f - y) * size.height
                    if (index == 0) path.moveTo(px, py) else path.lineTo(px, py)
                }
                drawPath(path, color = channelColor(currentChannel), style = Stroke(width = 4f))

                if (!isParametric) {
                    activePoints.forEach { point ->
                        val center = Offset(point.x * size.width, (1f - point.y) * size.height)
                        drawCircle(Color(0xCC000000), radius = 13f, center = center)
                        drawCircle(Color.White, radius = 10f, center = center)
                        drawCircle(Color(0x66000000), radius = 10f, center = center, style = Stroke(2f))
                    }
                }
            }
        }

        if (isParametric) {
            ParametricSliders(
                parametric = parametric,
                onChange = onParametricChange,
            )
        }

        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(Color(0xFF141414))
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("曲线", style = MaterialTheme.typography.titleMedium, color = Color.White)
            Spacer(Modifier.weight(1f))
            OutlinedButton(onClick = onDone, shape = RoundedCornerShape(12.dp)) {
                Text("完成")
            }
        }

        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(Color(0xFF141414))
                .padding(vertical = 10.dp),
            horizontalArrangement = Arrangement.SpaceEvenly,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            CurveChannel.entries.forEach { item ->
                ChannelButton(
                    channel = item,
                    selected = item == channel,
                    onClick = { channel = item },
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
                tint = if (selected) Color.White else Color(0xFFAAAAAA),
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

@Composable
private fun ParametricSliders(
    parametric: ParametricUi,
    onChange: (ParametricUi) -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(Color(0xFF141414))
            .padding(horizontal = 16.dp, vertical = 4.dp),
    ) {
        ParametricSlider("高光", parametric.highlights, -100f..100f) {
            onChange(parametric.copy(highlights = it))
        }
        ParametricSlider("中间调", parametric.midtones, -100f..100f) {
            onChange(parametric.copy(midtones = it))
        }
        ParametricSlider("阴影", parametric.shadows, -100f..100f) {
            onChange(parametric.copy(shadows = it))
        }
        ParametricSlider("高光分界", parametric.highlightsSplit, 0.5f..1f) {
            onChange(parametric.copy(highlightsSplit = it))
        }
        ParametricSlider("中间调分界", parametric.midtonesSplit, 0f..0.5f) {
            onChange(parametric.copy(midtonesSplit = it))
        }
    }
}

@Composable
private fun ParametricSlider(
    label: String,
    value: Float,
    range: ClosedFloatingPointRange<Float>,
    onValueChange: (Float) -> Unit,
) {
    Column {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                label,
                style = MaterialTheme.typography.bodySmall,
                color = Color(0xFFDDDDDD),
                modifier = Modifier.weight(1f),
            )
            Text(
                if (range.endInclusive <= 1f) {
                    String.format(Locale.US, "%.2f", value)
                } else {
                    String.format(Locale.US, "%+.0f", value)
                },
                style = MaterialTheme.typography.labelSmall,
                color = Color(0xFFAAAAAA),
            )
        }
        Slider(
            value = value,
            onValueChange = onValueChange,
            valueRange = range,
        )
    }
}

private fun channelColor(channel: CurveChannel): Color = when (channel) {
    CurveChannel.Rgb -> Color.White
    CurveChannel.Red -> Color(0xFFFF4D4D)
    CurveChannel.Green -> Color(0xFF43D96B)
    CurveChannel.Blue -> Color(0xFF4D8DFF)
    CurveChannel.Parametric -> Color.White
}

private fun androidx.compose.ui.unit.IntSize.toIntSizeSafe(): IntSize = this

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
