package com.fdxmp.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
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
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import java.util.Locale

/* ------------------------------------------------------------ 数据 */

/** 颜色分类里的子页：基础颜色 / HSL / 颜色分级。 */
enum class ColorTab(val label: String) {
    Base("颜色"),
    Hsl("HSL"),
    Grading("分级"),
}

/** 细节子页：锐化 / 降噪。 */
enum class DetailTab(val label: String) {
    Sharpen("锐化"),
    Noise("降噪"),
}

/** HSL 八色带。 */
enum class HslBandKind(val label: String, val serde: String, val color: Color) {
    Red("红", "red", Color(0xFFFF4D4D)),
    Orange("橙", "orange", Color(0xFFFF9A3D)),
    Yellow("黄", "yellow", Color(0xFFFFE14D)),
    Green("绿", "green", Color(0xFF43D96B)),
    Aqua("青", "aqua", Color(0xFF3DD9D9)),
    Blue("蓝", "blue", Color(0xFF4D8DFF)),
    Purple("紫", "purple", Color(0xFF9B6BFF)),
    Magenta("洋红", "magenta", Color(0xFFFF5FD0)),
}

/** 单个色带的 HSL 值（-100..100）。 */
data class HslBandUi(
    val hue: Float = 0f,
    val saturation: Float = 0f,
    val luminance: Float = 0f,
)

/** HSL 面板状态。 */
data class HslUi(
    val bands: List<HslBandUi> = List(HslBandKind.entries.size) { HslBandUi() },
)

/** 颜色分级区域。 */
enum class GradingZoneKind(val label: String, val serde: String) {
    Shadows("阴影", "shadows"),
    Midtones("中间调", "midtones"),
    Highlights("高光", "highlights"),
}

/** 单个分级区域的值。 */
data class GradingZoneUi(
    val hue: Float = 0f,
    val saturation: Float = 0f,
    val luminance: Float = 0f,
)

/** 颜色分级状态。 */
data class GradingUi(
    val shadows: GradingZoneUi = GradingZoneUi(),
    val midtones: GradingZoneUi = GradingZoneUi(),
    val highlights: GradingZoneUi = GradingZoneUi(),
    val blending: Float = 50f,
    val balance: Float = 0f,
)

/** 锐化参数。 */
data class SharpenUi(
    val amount: Float = 0f,
    val radius: Float = 1f,
    val detail: Float = 25f,
    val masking: Float = 0f,
)

/** 降噪参数。 */
data class NoiseUi(
    val luminance: Float = 0f,
    val luminanceDetail: Float = 50f,
    val luminanceContrast: Float = 0f,
    val color: Float = 0f,
    val colorDetail: Float = 50f,
    val colorSmoothness: Float = 50f,
)

/** 细节面板状态。 */
data class DetailUi(
    val sharpen: SharpenUi = SharpenUi(),
    val noise: NoiseUi = NoiseUi(),
)

/** 镜头校正 / 配置文件。 */
data class LensUi(
    val profileCorrection: Boolean = false,
    val distortion: Float = 0f,
    val vertical: Float = 0f,
    val horizontal: Float = 0f,
    val rotate: Float = 0f,
    val scale: Float = 1f,
    val vignette: Float = 0f,
)

/* ------------------------------------------------------------ 通用控件 */

/** 子页切换（文本 + 下划线）。 */
@Composable
fun PanelSubTabs(
    labels: List<String>,
    selectedIndex: Int,
    onSelect: (Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 2.dp),
        horizontalArrangement = Arrangement.spacedBy(18.dp),
    ) {
        labels.forEachIndexed { index, label ->
            val active = index == selectedIndex
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                modifier = Modifier
                    .clickable { onSelect(index) }
                    .padding(vertical = 4.dp),
            ) {
                Text(
                    label,
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

/** 单行滑块：标签 + 数值 + 滑杆。 */
@Composable
fun PanelSliderRow(
    label: String,
    value: Float,
    range: ClosedFloatingPointRange<Float>,
    onChange: (Float) -> Unit,
    decimals: Int = 0,
    preview: String? = null,
) {
    Column {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                label,
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.weight(1f),
            )
            Text(
                preview ?: formatPanelValue(value, range, decimals),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Slider(
            value = value,
            onValueChange = onChange,
            valueRange = range,
        )
    }
}

private fun formatPanelValue(
    value: Float,
    range: ClosedFloatingPointRange<Float>,
    decimals: Int,
): String = when {
    decimals > 0 -> String.format(Locale.US, "%.${decimals}f", value)
    range.start < 0f -> String.format(Locale.US, "%+.0f", value)
    else -> String.format(Locale.US, "%.0f", value)
}

/* ------------------------------------------------------------ HSL 面板 */

@Composable
fun HslPanel(
    hsl: HslUi,
    selectedBand: HslBandKind,
    onSelectBand: (HslBandKind) -> Unit,
    onChange: (HslUi) -> Unit,
    modifier: Modifier = Modifier,
) {
    val bandIndex = selectedBand.ordinal
    val band = hsl.bands.getOrElse(bandIndex) { HslBandUi() }

    fun update(transform: (HslBandUi) -> HslBandUi) {
        val updated = hsl.bands.toMutableList()
        updated[bandIndex] = transform(updated.getOrElse(bandIndex) { HslBandUi() })
        onChange(hsl.copy(bands = updated))
    }

    Column(modifier = modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 2.dp),
            horizontalArrangement = Arrangement.SpaceEvenly,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            HslBandKind.entries.forEach { kind ->
                val active = kind == selectedBand
                Box(
                    modifier = Modifier
                        .size(if (active) 30.dp else 26.dp)
                        .clip(CircleShape)
                        .background(kind.color)
                        .border(
                            width = if (active) 2.dp else 0.dp,
                            color = if (active) Color.White else Color.Transparent,
                            shape = CircleShape,
                        )
                        .clickable { onSelectBand(kind) },
                )
            }
        }

        LazyColumn(
            modifier = Modifier.fillMaxWidth().weight(1f),
            contentPadding = PaddingValues(horizontal = 16.dp),
        ) {
            items(
                items = listOf(
                    Triple("色相", band.hue) { value: Float -> update { it.copy(hue = value) } },
                    Triple("饱和度", band.saturation) { value: Float ->
                        update { it.copy(saturation = value) }
                    },
                    Triple("明亮度", band.luminance) { value: Float ->
                        update { it.copy(luminance = value) }
                    },
                ),
                key = { it.first },
            ) { (label, value, updater) ->
                PanelSliderRow(
                    label = "${selectedBand.label} · $label",
                    value = value,
                    range = -100f..100f,
                    onChange = updater,
                )
            }
        }
    }
}

/* ------------------------------------------------------ 颜色分级面板 */

@Composable
fun GradingPanel(
    grading: GradingUi,
    selectedZone: GradingZoneKind,
    onSelectZone: (GradingZoneKind) -> Unit,
    onChange: (GradingUi) -> Unit,
    modifier: Modifier = Modifier,
) {
    val zone = when (selectedZone) {
        GradingZoneKind.Shadows -> grading.shadows
        GradingZoneKind.Midtones -> grading.midtones
        GradingZoneKind.Highlights -> grading.highlights
    }

    fun updateZone(transform: (GradingZoneUi) -> GradingZoneUi) {
        onChange(
            when (selectedZone) {
                GradingZoneKind.Shadows -> grading.copy(shadows = transform(grading.shadows))
                GradingZoneKind.Midtones -> grading.copy(midtones = transform(grading.midtones))
                GradingZoneKind.Highlights -> grading.copy(
                    highlights = transform(grading.highlights),
                )
            },
        )
    }

    Column(modifier = modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 2.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            GradingZoneKind.entries.forEach { kind ->
                val active = kind == selectedZone
                Text(
                    kind.label,
                    style = MaterialTheme.typography.labelMedium,
                    color = if (active) Color.White else MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                    modifier = Modifier
                        .weight(1f)
                        .clip(RoundedCornerShape(9.dp))
                        .background(
                            if (active) MaterialTheme.colorScheme.primary else Color(0xFF1E1E1E),
                        )
                        .clickable { onSelectZone(kind) }
                        .padding(vertical = 7.dp),
                )
            }
        }

        LazyColumn(
            modifier = Modifier.fillMaxWidth().weight(1f),
            contentPadding = PaddingValues(horizontal = 16.dp),
        ) {
            items(
                items = listOf(
                    Triple("色相", zone.hue) { value: Float ->
                        updateZone { it.copy(hue = value) }
                    },
                    Triple("饱和度", zone.saturation) { value: Float ->
                        updateZone { it.copy(saturation = value) }
                    },
                    Triple("明亮度", zone.luminance) { value: Float ->
                        updateZone { it.copy(luminance = value) }
                    },
                    Triple("混合", grading.blending) { value: Float ->
                        onChange(grading.copy(blending = value))
                    },
                    Triple("平衡", grading.balance) { value: Float ->
                        onChange(grading.copy(balance = value))
                    },
                ),
                key = { it.first },
            ) { (label, value, updater) ->
                val range = when (label) {
                    "色相" -> 0f..360f
                    "饱和度" -> 0f..100f
                    "混合" -> 0f..100f
                    else -> -100f..100f
                }
                PanelSliderRow(
                    label = label,
                    value = value,
                    range = range,
                    onChange = updater,
                )
            }
        }
    }
}

/* ------------------------------------------------------------ 细节面板 */

@Composable
fun DetailPanel(
    detail: DetailUi,
    selectedTab: DetailTab,
    onSelectTab: (DetailTab) -> Unit,
    onChange: (DetailUi) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier.fillMaxWidth()) {
        PanelSubTabs(
            labels = DetailTab.entries.map { it.label },
            selectedIndex = selectedTab.ordinal,
            onSelect = { onSelectTab(DetailTab.entries[it]) },
        )
        LazyColumn(
            modifier = Modifier.fillMaxWidth().weight(1f),
            contentPadding = PaddingValues(horizontal = 16.dp),
        ) {
            if (selectedTab == DetailTab.Sharpen) {
                val sharpen = detail.sharpen
                items(
                    items = listOf(
                        Triple("数量", sharpen.amount) { value: Float ->
                            onChange(detail.copy(sharpen = sharpen.copy(amount = value)))
                        },
                        Triple("半径", sharpen.radius) { value: Float ->
                            onChange(detail.copy(sharpen = sharpen.copy(radius = value)))
                        },
                        Triple("细节", sharpen.detail) { value: Float ->
                            onChange(detail.copy(sharpen = sharpen.copy(detail = value)))
                        },
                        Triple("蒙版", sharpen.masking) { value: Float ->
                            onChange(detail.copy(sharpen = sharpen.copy(masking = value)))
                        },
                    ),
                    key = { it.first },
                ) { (label, value, updater) ->
                    PanelSliderRow(
                        label = label,
                        value = value,
                        range = when (label) {
                            "数量" -> 0f..150f
                            "半径" -> 0.5f..3f
                            "细节" -> 0f..100f
                            else -> 0f..100f
                        },
                        decimals = if (label == "半径") 1 else 0,
                        onChange = updater,
                    )
                }
            } else {
                val noise = detail.noise
                items(
                    items = listOf(
                        Triple("亮度", noise.luminance) { value: Float ->
                            onChange(detail.copy(noise = noise.copy(luminance = value)))
                        },
                        Triple("亮度细节", noise.luminanceDetail) { value: Float ->
                            onChange(detail.copy(noise = noise.copy(luminanceDetail = value)))
                        },
                        Triple("亮度对比度", noise.luminanceContrast) { value: Float ->
                            onChange(detail.copy(noise = noise.copy(luminanceContrast = value)))
                        },
                        Triple("颜色", noise.color) { value: Float ->
                            onChange(detail.copy(noise = noise.copy(color = value)))
                        },
                        Triple("颜色细节", noise.colorDetail) { value: Float ->
                            onChange(detail.copy(noise = noise.copy(colorDetail = value)))
                        },
                        Triple("颜色平滑度", noise.colorSmoothness) { value: Float ->
                            onChange(detail.copy(noise = noise.copy(colorSmoothness = value)))
                        },
                    ),
                    key = { it.first },
                ) { (label, value, updater) ->
                    PanelSliderRow(
                        label = label,
                        value = value,
                        range = 0f..100f,
                        onChange = updater,
                    )
                }
            }
        }
    }
}

/* ------------------------------------------------------------ 镜头面板 */

@Composable
fun LensPanel(
    lens: LensUi,
    onChange: (LensUi) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 2.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                "配置文件校正（通用）",
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.weight(1f),
            )
            Switch(
                checked = lens.profileCorrection,
                onCheckedChange = { onChange(lens.copy(profileCorrection = it)) },
            )
        }
        LazyColumn(
            modifier = Modifier.fillMaxWidth().weight(1f),
            contentPadding = PaddingValues(horizontal = 16.dp),
        ) {
            items(
                items = listOf(
                    Triple("畸变", lens.distortion) { value: Float ->
                        onChange(lens.copy(distortion = value))
                    },
                    Triple("垂直", lens.vertical) { value: Float ->
                        onChange(lens.copy(vertical = value))
                    },
                    Triple("水平", lens.horizontal) { value: Float ->
                        onChange(lens.copy(horizontal = value))
                    },
                    Triple("旋转", lens.rotate) { value: Float ->
                        onChange(lens.copy(rotate = value))
                    },
                    Triple("缩放", lens.scale) { value: Float ->
                        onChange(lens.copy(scale = value))
                    },
                    Triple("镜头暗角", lens.vignette) { value: Float ->
                        onChange(lens.copy(vignette = value))
                    },
                ),
                key = { it.first },
            ) { (label, value, updater) ->
                val range = when (label) {
                    "旋转" -> -45f..45f
                    "缩放" -> 0.5f..2f
                    else -> -100f..100f
                }
                PanelSliderRow(
                    label = label,
                    value = value,
                    range = range,
                    decimals = if (label == "缩放") 2 else 0,
                    onChange = updater,
                )
            }
        }
    }
}
