package com.fdxmp.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.PhotoLibrary
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.fdxmp.app.media.EditRecord
import com.fdxmp.app.media.EditStore
import com.fdxmp.app.media.MediaImage
import com.fdxmp.app.ui.DeviceTab
import com.fdxmp.app.ui.EditTab
import com.fdxmp.app.ui.EditorScreen
import com.fdxmp.app.ui.FdxmpTheme
import com.fdxmp.app.ui.PhotoViewer
import com.fdxmp.core.NativeBridge

/**
 * fdxmp Android 主界面。
 *
 * 严格按规格实现 Lightroom 移动版式的三大 Tab：
 *   设备（默认）/ 编辑 / 我的。底部导航 + 顶部标题随 Tab 变化，无侧边栏。
 * 设备 Tab 已接入相册（网格 / 列表 / 预览 / 多选）；
 * 编辑 Tab + 基础调色编辑界面（Rust 核心出预览）已接入；我的待后续阶段。
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            FdxmpTheme {
                FdxmpApp()
            }
        }
    }
}

private enum class Tab(val label: String, val icon: ImageVector) {
    Device("设备", Icons.Filled.PhotoLibrary),
    Edit("编辑", Icons.Filled.Edit),
    Me("我的", Icons.Filled.Person),
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun FdxmpApp() {
    val context = LocalContext.current
    var tab by remember { mutableStateOf(Tab.Device) }
    var editorRecord by remember { mutableStateOf<EditRecord?>(null) }
    var editsVersion by remember { mutableStateOf(0) }
    var viewerImages by remember { mutableStateOf<List<MediaImage>>(emptyList()) }
    var viewerStart by remember { mutableStateOf(0) }
    var viewerOpen by remember { mutableStateOf(false) }
    val coreVersion = remember {
        runCatching { NativeBridge.nativeVersion() }.getOrDefault("core 未加载")
    }

    Box(modifier = Modifier.fillMaxSize()) {
        Scaffold(
            containerColor = MaterialTheme.colorScheme.background,
            topBar = {
                TopAppBar(
                    title = {
                        Column {
                            Text("fdxmp · ${tab.label}")
                            Text(
                                "Rust core $coreVersion",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    },
                    colors = TopAppBarDefaults.topAppBarColors(
                        containerColor = MaterialTheme.colorScheme.surface,
                    ),
                )
            },
            bottomBar = {
                NavigationBar {
                    Tab.entries.forEach { candidate ->
                        NavigationBarItem(
                            selected = tab == candidate,
                            onClick = { tab = candidate },
                            icon = { Icon(candidate.icon, contentDescription = candidate.label) },
                            label = { Text(candidate.label) },
                        )
                    }
                }
            },
        ) { padding ->
            when (tab) {
                Tab.Device -> DeviceTab(
                    modifier = Modifier.padding(padding),
                    onOpenEditor = { image ->
                        val uri = image.uri.toString()
                        editorRecord = EditStore.find(context, uri)
                            ?: EditRecord.fresh(uri, image.name)
                    },
                    onOpenViewer = { images, start ->
                        viewerImages = images
                        viewerStart = start
                        viewerOpen = true
                    },
                )

                Tab.Edit -> EditTab(
                    modifier = Modifier.padding(padding),
                    refreshToken = editsVersion,
                    onOpenEditor = { editorRecord = it },
                )

                Tab.Me -> PlaceholderTab(
                    modifier = Modifier.padding(padding),
                    title = "我的",
                    detail = "账号 / 缓存管理 / 预设管理 / 主题 / 关于（阶段 6 接入）",
                )
            }
        }

        if (viewerOpen && viewerImages.isNotEmpty()) {
            PhotoViewer(
                images = viewerImages,
                initialIndex = viewerStart,
                onDismiss = { viewerOpen = false },
                onEdit = { image ->
                    viewerOpen = false
                    val uri = image.uri.toString()
                    editorRecord = EditStore.find(context, uri)
                        ?: EditRecord.fresh(uri, image.name)
                },
            )
        }

        editorRecord?.let { record ->
            EditorScreen(
                record = record,
                onClose = { editorRecord = null },
                onSaved = {
                    editsVersion += 1
                    editorRecord = null
                },
            )
        }
    }
}

@Composable
private fun PlaceholderTab(
    modifier: Modifier,
    title: String,
    detail: String,
) {
    Column(
        modifier = modifier.fillMaxSize().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(title, style = MaterialTheme.typography.titleLarge)
        Spacer(Modifier.height(12.dp))
        Text(
            detail,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
    }
}
