package com.fdxmp.app.ui

import android.graphics.Bitmap
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Fill
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.unit.dp
import kotlin.math.pow
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** 直方图数据：R / G / B / 亮度，各 256 个桶。 */
class HistogramData(
    val red: IntArray,
    val green: IntArray,
    val blue: IntArray,
    val luma: IntArray,
)

/** 从 Bitmap 采样计算直方图（最多约 25 万采样点，避免大图卡顿）。 */
suspend fun computeHistogram(bitmap: Bitmap): HistogramData = withContext(Dispatchers.Default) {
    val red = IntArray(256)
    val green = IntArray(256)
    val blue = IntArray(256)
    val luma = IntArray(256)
    val width = bitmap.width
    val height = bitmap.height
    if (width <= 0 || height <= 0) return@withContext HistogramData(red, green, blue, luma)

    val pixels = IntArray(width * height)
    bitmap.getPixels(pixels, 0, width, 0, 0, width, height)
    val step = (pixels.size / 250_000).coerceAtLeast(1)
    var index = 0
    while (index < pixels.size) {
        val pixel = pixels[index]
        val r = (pixel shr 16) and 0xFF
        val g = (pixel shr 8) and 0xFF
        val b = pixel and 0xFF
        red[r] += 1
        green[g] += 1
        blue[b] += 1
        val l = (r * 2126 + g * 7152 + b * 722) / 10_000
        luma[l.coerceIn(0, 255)] += 1
        index += step
    }
    HistogramData(red, green, blue, luma)
}

/** Lightroom 风格的直方图叠加卡片。 */
@Composable
fun HistogramOverlay(
    data: HistogramData?,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier
            .clip(RoundedCornerShape(8.dp))
            .background(Color(0xCC141414)),
    ) {
        if (data != null) {
            Canvas(modifier = Modifier.fillMaxSize().padding(horizontal = 8.dp, vertical = 8.dp)) {
                drawChannel(data.luma, Color(0x30FFFFFF), fill = true)
                drawChannel(data.blue, Color(0xCC4488FF), fill = false)
                drawChannel(data.green, Color(0xCC44DD66), fill = false)
                drawChannel(data.red, Color(0xCCFF4444), fill = false)
            }
        }
    }
}

private fun DrawScope.drawChannel(
    bins: IntArray,
    color: Color,
    fill: Boolean,
) {
    if (size.width <= 0f || size.height <= 0f) return
    val peak = bins.max().coerceAtLeast(1)
    val path = Path()
    path.moveTo(0f, size.height)
    for (index in bins.indices) {
        val x = size.width * index / (bins.size - 1).toFloat()
        val normalized = (bins[index].toFloat() / peak).pow(0.5f)
        val y = size.height * (1f - normalized)
        path.lineTo(x, y)
    }
    path.lineTo(size.width, size.height)
    path.close()
    if (fill) {
        drawPath(path, color = color, style = Fill)
    } else {
        drawPath(
            path = path,
            color = color,
            style = Stroke(width = 2f),
            alpha = 0.9f,
        )
    }
    if (!fill) {
        drawCircle(
            color = color,
            radius = 3f,
            center = Offset(size.width, size.height),
        )
    }
}
