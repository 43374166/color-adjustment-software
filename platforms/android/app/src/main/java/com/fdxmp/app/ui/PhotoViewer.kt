package com.fdxmp.app.ui

import android.graphics.Bitmap
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
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
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Image
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.fdxmp.app.media.MediaImage
import com.fdxmp.app.media.MediaStoreRepository

/**
 * 全屏预览：左右滑动切换照片，双击 / 双指缩放，顶部显示信息，底部可进入编辑或分享。
 *
 * 作为 Activity 根布局上的覆盖层使用（不是独立 Dialog），这样系统栏 insets 才可靠。
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun PhotoViewer(
    images: List<MediaImage>,
    initialIndex: Int,
    onDismiss: () -> Unit,
    onEdit: (MediaImage) -> Unit,
) {
    if (images.isEmpty()) return

    val pagerState = rememberPagerState(
        initialPage = initialIndex.coerceIn(0, images.lastIndex),
    ) { images.size }
    var zoomed by remember { mutableStateOf(false) }

    BackHandler(onBack = onDismiss)
    LaunchedEffect(pagerState.currentPage) { zoomed = false }

    val current = images[pagerState.currentPage.coerceIn(0, images.lastIndex)]

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black),
    ) {
        HorizontalPager(
            state = pagerState,
            modifier = Modifier.fillMaxSize(),
            userScrollEnabled = !zoomed,
        ) { page ->
            ZoomablePhoto(
                image = images[page],
                onZoomChanged = { zoomed = it },
            )
        }

        Row(
            modifier = Modifier
                .align(Alignment.TopCenter)
                .fillMaxWidth()
                .statusBarsPadding()
                .background(Color(0x99000000))
                .padding(horizontal = 12.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                "${pagerState.currentPage + 1} / ${images.size}",
                color = Color.White,
                style = MaterialTheme.typography.labelMedium,
            )
            Spacer(Modifier.width(12.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    current.name,
                    color = Color.White,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    style = MaterialTheme.typography.bodyMedium,
                )
                Text(
                    buildString {
                        append(current.pixelSize)
                        append(" · ")
                        append(current.displaySize)
                        if (current.isRaw) append(" · RAW")
                    },
                    color = Color(0xFFB0B0B0),
                    style = MaterialTheme.typography.labelSmall,
                )
            }
            IconButton(onClick = onDismiss) {
                Icon(Icons.Filled.Close, contentDescription = "关闭", tint = Color.White)
            }
        }

        Row(
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .navigationBarsPadding()
                .background(Color(0x99000000))
                .padding(vertical = 6.dp),
            horizontalArrangement = androidx.compose.foundation.layout.Arrangement.SpaceEvenly,
        ) {
            BottomViewerAction(
                icon = Icons.Filled.Edit,
                label = "编辑",
                onClick = { onEdit(current) },
            )
        }
    }
}

@Composable
private fun BottomViewerAction(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    label: String,
    onClick: () -> Unit,
) {
    androidx.compose.material3.TextButton(onClick = onClick) {
        Icon(icon, contentDescription = label, tint = Color.White, modifier = Modifier.size(18.dp))
        Spacer(Modifier.width(6.dp))
        Text(label, color = Color.White)
    }
}

@Composable
private fun ZoomablePhoto(
    image: MediaImage,
    onZoomChanged: (Boolean) -> Unit,
) {
    val context = LocalContext.current
    var bitmap by remember(image.id) { mutableStateOf<Bitmap?>(null) }
    var failed by remember(image.id) { mutableStateOf(false) }
    var scale by remember(image.id) { mutableStateOf(1f) }
    var offset by remember(image.id) { mutableStateOf(Offset.Zero) }

    LaunchedEffect(image.id) {
        failed = false
        bitmap = MediaStoreRepository.loadPreview(context, image, 2048)
        failed = bitmap == null
    }

    LaunchedEffect(scale) { onZoomChanged(scale > 1.01f) }

    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        val current = bitmap
        when {
            current != null -> Image(
                bitmap = current.asImageBitmap(),
                contentDescription = image.name,
                contentScale = ContentScale.Fit,
                modifier = Modifier
                    .fillMaxSize()
                    .graphicsLayer(
                        scaleX = scale,
                        scaleY = scale,
                        translationX = offset.x,
                        translationY = offset.y,
                    )
                    .pointerInput(image.id) {
                        detectTransformGestures(panZoomLock = true) { _, pan, zoom, _ ->
                            scale = (scale * zoom).coerceIn(1f, 6f)
                            offset = if (scale > 1f) offset + pan else Offset.Zero
                        }
                    }
                    .pointerInput(image.id) {
                        detectTapGestures(
                            onDoubleTap = {
                                if (scale > 1f) {
                                    scale = 1f
                                    offset = Offset.Zero
                                } else {
                                    scale = 2.5f
                                }
                            },
                        )
                    },
            )

            failed -> Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                modifier = Modifier.padding(32.dp),
            ) {
                Icon(
                    Icons.Filled.Image,
                    contentDescription = null,
                    tint = Color(0xFF888888),
                    modifier = Modifier.size(48.dp),
                )
                Spacer(Modifier.height(12.dp))
                Text(
                    if (image.isRaw) {
                        "RAW 文件需要 Rust 核心解码后才能预览（阶段 6 接入）"
                    } else {
                        "无法加载这张图片"
                    },
                    color = Color(0xFFCCCCCC),
                    textAlign = TextAlign.Center,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }

            else -> CircularProgressIndicator(color = MaterialTheme.colorScheme.primary)
        }
    }
}
