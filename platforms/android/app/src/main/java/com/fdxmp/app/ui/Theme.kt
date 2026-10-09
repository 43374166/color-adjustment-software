package com.fdxmp.app.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

/** 图像处理场景默认深色主题（与桌面端 / 移动端规格一致）。 */
private val FdxmpDarkColors = darkColorScheme(
    primary = Color(0xFF4A9EFF),
    onPrimary = Color.White,
    background = Color(0xFF0E0E0E),
    onBackground = Color(0xFFE8E8E8),
    surface = Color(0xFF141414),
    onSurface = Color(0xFFE8E8E8),
    surfaceVariant = Color(0xFF1E1E1E),
    onSurfaceVariant = Color(0xFFAAAAAA),
    outline = Color(0xFF3A3A3A),
)

@Composable
fun FdxmpTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = FdxmpDarkColors, content = content)
}
