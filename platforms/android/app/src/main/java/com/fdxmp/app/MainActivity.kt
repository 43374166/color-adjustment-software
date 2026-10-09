package com.fdxmp.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.PhotoLibrary
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import com.fdxmp.core.NativeBridge

/**
 * fdxmp Android 主界面（阶段 5 骨架）。
 *
 * 严格按规格实现 Lightroom 移动版式的三大 Tab：
 *   设备（默认）/ 编辑 / 我的。底部导航 + 顶部标题随 Tab 变化，无侧边栏。
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
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
    var tab by remember { mutableStateOf(Tab.Device) }
    val coreVersion = remember { runCatching { NativeBridge.nativeVersion() }.getOrDefault("core 未加载") }

    Scaffold(
        topBar = { TopAppBar(title = { Text("fdxmp · ${tab.label}") }) },
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
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            when (tab) {
                Tab.Device -> Text("设备：相册网格 / 列表、RAW 与常见格式、全屏预览（阶段 5 接入）")
                Tab.Edit -> Text("编辑：已编辑记录 + 缩略图 + 预设名（阶段 5 接入）")
                Tab.Me -> Text("我的：账号 / 缓存管理 / 预设管理 / 主题 / 关于（阶段 5 接入）")
            }
            Text("Rust core：$coreVersion", style = MaterialTheme.typography.labelSmall)
        }
    }
}

