package com.fdxmp.app.ui

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.automirrored.filled.Sort
import androidx.compose.material.icons.filled.AddPhotoAlternate
import androidx.compose.material.icons.filled.CameraAlt
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.GridView
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.PhotoLibrary
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.SelectAll
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.fdxmp.app.media.MediaImage
import com.fdxmp.app.media.MediaStoreRepository
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

private enum class GalleryFilter(val label: String) {
    All("全部"),
    Raw("RAW"),
    Standard("常见格式"),
}

private enum class GallerySort(val label: String) {
    Newest("最新优先"),
    Oldest("最早优先"),
    Name("文件名"),
    Size("文件大小"),
}

/**
 * 设备 Tab：相册权限 → 网格 / 列表 → 搜索 / 排序 / 筛选 → 长按多选 → 全屏预览。
 *
 * 规格见 docs/SPEC.md「Tab 1：设备」。RAW 文件会列出并标记，但预览需要
 * Rust 核心解码（阶段 6 接入），当前显示提示。
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun DeviceTab(
    modifier: Modifier = Modifier,
    onOpenEditor: (MediaImage) -> Unit,
    onOpenViewer: (List<MediaImage>, Int) -> Unit,
) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current

    var permissionGranted by remember { mutableStateOf(hasGalleryPermission(context)) }
    var permissionRequested by remember { mutableStateOf(false) }

    val galleryPermissions = remember {
        buildList {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                add(Manifest.permission.READ_MEDIA_IMAGES)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                    add(Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED)
                }
            } else {
                add(Manifest.permission.READ_EXTERNAL_STORAGE)
            }
        }.toTypedArray()
    }

    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) {
        permissionRequested = true
        permissionGranted = hasGalleryPermission(context)
    }

    // 从系统设置授权后回到前台时刷新状态。
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                permissionGranted = hasGalleryPermission(context)
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    var images by remember { mutableStateOf<List<MediaImage>>(emptyList()) }
    var isLoading by remember { mutableStateOf(false) }
    var refreshToken by remember { mutableStateOf(0) }

    LaunchedEffect(permissionGranted, refreshToken) {
        if (!permissionGranted) {
            images = emptyList()
            return@LaunchedEffect
        }
        isLoading = true
        images = runCatching { MediaStoreRepository.queryImages(context) }.getOrDefault(emptyList())
        isLoading = false
    }

    var query by remember { mutableStateOf("") }
    var filter by remember { mutableStateOf(GalleryFilter.All) }
    var sort by remember { mutableStateOf(GallerySort.Newest) }
    var gridMode by remember { mutableStateOf(true) }

    var selectionMode by remember { mutableStateOf(false) }
    var selectedIds by remember { mutableStateOf<Set<Long>>(emptySet()) }
    var pendingCaptureUri by remember { mutableStateOf<Uri?>(null) }

    val visibleImages = remember(images, query, filter, sort) {
        images.asSequence()
            .filter { query.isBlank() || it.name.contains(query, ignoreCase = true) }
            .filter {
                when (filter) {
                    GalleryFilter.All -> true
                    GalleryFilter.Raw -> it.isRaw
                    GalleryFilter.Standard -> !it.isRaw
                }
            }
            .sortedWith(
                when (sort) {
                    GallerySort.Newest -> compareByDescending { it.dateAddedSeconds }
                    GallerySort.Oldest -> compareBy { it.dateAddedSeconds }
                    GallerySort.Name -> compareBy { it.name.lowercase(Locale.US) }
                    GallerySort.Size -> compareByDescending { it.sizeBytes }
                },
            )
            .toList()
    }

    fun shareImages(targets: List<MediaImage>) {
        if (targets.isEmpty()) return
        val intent = Intent(Intent.ACTION_SEND_MULTIPLE).apply {
            type = "image/*"
            putParcelableArrayListExtra(Intent.EXTRA_STREAM, ArrayList(targets.map { it.uri }))
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        runCatching { context.startActivity(Intent.createChooser(intent, "分享照片")) }
            .onFailure { Toast.makeText(context, "没有可用的分享应用", Toast.LENGTH_SHORT).show() }
    }

    val takePictureLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.TakePicture(),
    ) { saved ->
        val uri = pendingCaptureUri
        pendingCaptureUri = null
        when {
            uri == null -> Unit
            saved -> {
                MediaStoreRepository.finalizeCameraImage(context, uri)
                refreshToken += 1
            }

            else -> MediaStoreRepository.discardCameraImage(context, uri)
        }
    }

    val importLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.PickMultipleVisualMedia(50),
    ) { uris ->
        if (uris.isNotEmpty()) {
            Toast.makeText(
                context,
                "已选择 ${uris.size} 张照片（导入到编辑队列将在编辑页接入）",
                Toast.LENGTH_SHORT,
            ).show()
        }
    }

    Column(modifier = modifier.fillMaxSize()) {
        if (!permissionGranted) {
            PermissionGuide(
                modifier = Modifier.weight(1f),
                requested = permissionRequested,
                onRequest = { permissionLauncher.launch(galleryPermissions) },
                onOpenSettings = { context.openAppSettings() },
            )
            return@Column
        }

        if (hasPartialGalleryAccess(context)) {
            LimitedAccessBanner(
                onManage = { permissionLauncher.launch(galleryPermissions) },
            )
        }

        GalleryToolbar(
            query = query,
            onQueryChange = { query = it },
            gridMode = gridMode,
            onToggleView = { gridMode = !gridMode },
            sort = sort,
            onSortChange = { sort = it },
            filter = filter,
            onFilterChange = { filter = it },
        )

        if (selectionMode) {
            SelectionBar(
                count = selectedIds.size,
                onSelectAll = { selectedIds = visibleImages.map { it.id }.toSet() },
                onClear = { selectedIds = emptySet() },
                onShare = { shareImages(visibleImages.filter { it.id in selectedIds }) },
                onDone = {
                    selectionMode = false
                    selectedIds = emptySet()
                },
            )
        }

        Box(modifier = Modifier.weight(1f).fillMaxWidth()) {
            when {
                isLoading -> CircularProgressIndicator(Modifier.align(Alignment.Center))

                visibleImages.isEmpty() -> EmptyGallery(
                    modifier = Modifier.align(Alignment.Center),
                    filtered = images.isNotEmpty(),
                )

                gridMode -> LazyVerticalGrid(
                    columns = GridCells.Fixed(3),
                    modifier = Modifier.fillMaxSize(),
                    contentPadding = PaddingValues(2.dp),
                    verticalArrangement = Arrangement.spacedBy(2.dp),
                    horizontalArrangement = Arrangement.spacedBy(2.dp),
                ) {
                    items(items = visibleImages, key = { it.id }) { image ->
                        PhotoGridCell(
                            image = image,
                            selected = image.id in selectedIds,
                            selectionMode = selectionMode,
                            onClick = {
                                if (selectionMode) {
                                    selectedIds = selectedIds.toggle(image.id)
                                } else {
                                    onOpenViewer(visibleImages, visibleImages.indexOf(image))
                                }
                            },
                            onLongClick = {
                                selectionMode = true
                                selectedIds = selectedIds + image.id
                            },
                        )
                    }
                }

                else -> LazyColumn(
                    modifier = Modifier.fillMaxSize(),
                    contentPadding = PaddingValues(vertical = 4.dp),
                ) {
                    items(items = visibleImages, key = { it.id }) { image ->
                        PhotoListRow(
                            image = image,
                            selected = image.id in selectedIds,
                            selectionMode = selectionMode,
                            onClick = {
                                if (selectionMode) {
                                    selectedIds = selectedIds.toggle(image.id)
                                } else {
                                    onOpenViewer(visibleImages, visibleImages.indexOf(image))
                                }
                            },
                            onLongClick = {
                                selectionMode = true
                                selectedIds = selectedIds + image.id
                            },
                        )
                    }
                }
            }
        }

        BottomActionBar(
            selectedCount = selectedIds.size,
            onImport = {
                importLauncher.launch(
                    PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly),
                )
            },
            onCamera = {
                val uri = MediaStoreRepository.createCameraImageUri(context)
                if (uri == null) {
                    Toast.makeText(context, "无法创建照片文件", Toast.LENGTH_SHORT).show()
                } else {
                    pendingCaptureUri = uri
                    runCatching { takePictureLauncher.launch(uri) }
                        .onFailure {
                            pendingCaptureUri = null
                            MediaStoreRepository.discardCameraImage(context, uri)
                            Toast.makeText(context, "没有可用的相机应用", Toast.LENGTH_SHORT).show()
                        }
                }
            },
            onShare = { shareImages(visibleImages.filter { it.id in selectedIds }) },
        )
    }

}

/* ---------------------------------------------------------------- 权限 */

private fun galleryPermission(): String =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        Manifest.permission.READ_MEDIA_IMAGES
    } else {
        Manifest.permission.READ_EXTERNAL_STORAGE
    }

private fun hasGalleryPermission(context: Context): Boolean {
    if (ContextCompat.checkSelfPermission(context, galleryPermission()) ==
        PackageManager.PERMISSION_GRANTED
    ) {
        return true
    }
    return hasPartialGalleryAccess(context)
}

private fun hasPartialGalleryAccess(context: Context): Boolean {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) return false
    return ContextCompat.checkSelfPermission(
        context,
        Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED,
    ) == PackageManager.PERMISSION_GRANTED
}

private fun Context.openAppSettings() {
    val intent = Intent(
        Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
        Uri.fromParts("package", packageName, null),
    )
    runCatching { startActivity(intent) }
}

@Composable
private fun PermissionGuide(
    modifier: Modifier = Modifier,
    requested: Boolean,
    onRequest: () -> Unit,
    onOpenSettings: () -> Unit,
) {
    Column(
        modifier = modifier.fillMaxWidth().padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Icon(
            Icons.Filled.PhotoLibrary,
            contentDescription = null,
            modifier = Modifier.size(56.dp),
            tint = MaterialTheme.colorScheme.primary,
        )
        Spacer(Modifier.height(16.dp))
        Text("访问你的照片", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(8.dp))
        Text(
            text = if (requested) {
                "需要相册权限才能浏览和编辑设备上的照片。如果系统不再弹出授权框，请到设置里手动开启。"
            } else {
                "fdxmp 需要读取相册，用于网格浏览、预览和编辑。照片只在本机处理。"
            },
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(20.dp))
        Button(onClick = onRequest) { Text("授予相册权限") }
        if (requested) {
            Spacer(Modifier.height(8.dp))
            OutlinedButton(onClick = onOpenSettings) { Text("打开系统设置") }
        }
    }
}

@Composable
private fun LimitedAccessBanner(onManage: () -> Unit) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceVariant,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                "当前只能访问部分照片",
                style = MaterialTheme.typography.labelMedium,
                modifier = Modifier.weight(1f),
            )
            TextButton(onClick = onManage) { Text("管理") }
        }
    }
}

/* ------------------------------------------------------------ 工具条 */

@Composable
private fun GalleryToolbar(
    query: String,
    onQueryChange: (String) -> Unit,
    gridMode: Boolean,
    onToggleView: () -> Unit,
    sort: GallerySort,
    onSortChange: (GallerySort) -> Unit,
    filter: GalleryFilter,
    onFilterChange: (GalleryFilter) -> Unit,
) {
    var sortMenuOpen by remember { mutableStateOf(false) }

    Column {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedTextField(
                value = query,
                onValueChange = onQueryChange,
                modifier = Modifier.weight(1f),
                singleLine = true,
                shape = RoundedCornerShape(12.dp),
                placeholder = { Text("搜索文件名", style = MaterialTheme.typography.bodySmall) },
                leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
                trailingIcon = {
                    if (query.isNotEmpty()) {
                        IconButton(onClick = { onQueryChange("") }) {
                            Icon(Icons.Filled.Close, contentDescription = "清空")
                        }
                    }
                },
            )
            IconButton(onClick = onToggleView) {
                Icon(
                    if (gridMode) Icons.AutoMirrored.Filled.List else Icons.Filled.GridView,
                    contentDescription = if (gridMode) "切换列表" else "切换网格",
                )
            }
            Box {
                IconButton(onClick = { sortMenuOpen = true }) {
                    Icon(Icons.AutoMirrored.Filled.Sort, contentDescription = "排序")
                }
                DropdownMenu(
                    expanded = sortMenuOpen,
                    onDismissRequest = { sortMenuOpen = false },
                ) {
                    GallerySort.entries.forEach { option ->
                        DropdownMenuItem(
                            text = {
                                Text(if (sort == option) "✓ ${option.label}" else option.label)
                            },
                            onClick = {
                                onSortChange(option)
                                sortMenuOpen = false
                            },
                        )
                    }
                }
            }
        }
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            GalleryFilter.entries.forEach { option ->
                FilterChip(
                    selected = filter == option,
                    onClick = { onFilterChange(option) },
                    label = { Text(option.label) },
                )
            }
        }
    }
}

@Composable
private fun SelectionBar(
    count: Int,
    onSelectAll: () -> Unit,
    onClear: () -> Unit,
    onShare: () -> Unit,
    onDone: () -> Unit,
) {
    Surface(color = MaterialTheme.colorScheme.surfaceVariant, modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                "已选 $count 张",
                style = MaterialTheme.typography.labelLarge,
                modifier = Modifier.padding(start = 8.dp).weight(1f),
            )
            TextButton(onClick = onSelectAll) {
                Icon(Icons.Filled.SelectAll, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(4.dp))
                Text("全选")
            }
            TextButton(onClick = onClear, enabled = count > 0) { Text("取消全选") }
            IconButton(onClick = onShare, enabled = count > 0) {
                Icon(Icons.Filled.Share, contentDescription = "分享")
            }
            TextButton(onClick = onDone) { Text("完成") }
        }
    }
}

@Composable
private fun BottomActionBar(
    selectedCount: Int,
    onImport: () -> Unit,
    onCamera: () -> Unit,
    onShare: () -> Unit,
) {
    Surface(color = MaterialTheme.colorScheme.surface, modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
            horizontalArrangement = Arrangement.SpaceEvenly,
        ) {
            BottomAction(
                icon = Icons.Filled.AddPhotoAlternate,
                label = "导入",
                onClick = onImport,
            )
            BottomAction(
                icon = Icons.Filled.CameraAlt,
                label = "相机",
                onClick = onCamera,
            )
            BottomAction(
                icon = Icons.Filled.Share,
                label = if (selectedCount > 0) "分享($selectedCount)" else "分享",
                enabled = selectedCount > 0,
                onClick = onShare,
            )
        }
    }
}

@Composable
private fun BottomAction(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    label: String,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    TextButton(onClick = onClick, enabled = enabled) {
        Icon(icon, contentDescription = label, modifier = Modifier.size(20.dp))
        Spacer(Modifier.width(6.dp))
        Text(label)
    }
}

/* ------------------------------------------------------------ 列表项 */

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun PhotoGridCell(
    image: MediaImage,
    selected: Boolean,
    selectionMode: Boolean,
    onClick: () -> Unit,
    onLongClick: () -> Unit,
) {
    val context = LocalContext.current
    var bitmap by remember(image.id) { mutableStateOf<android.graphics.Bitmap?>(null) }

    LaunchedEffect(image.id) {
        bitmap = MediaStoreRepository.loadThumbnail(context, image)
    }

    Box(
        modifier = Modifier
            .aspectRatio(1f)
            .clip(RoundedCornerShape(4.dp))
            .background(MaterialTheme.colorScheme.surfaceVariant)
            .combinedClickable(onClick = onClick, onLongClick = onLongClick),
    ) {
        val current = bitmap
        if (current != null) {
            Image(
                bitmap = current.asImageBitmap(),
                contentDescription = image.name,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize(),
            )
        } else {
            Icon(
                Icons.Filled.Image,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.align(Alignment.Center).size(28.dp),
            )
        }

        if (image.isRaw) {
            Text(
                "RAW",
                style = MaterialTheme.typography.labelSmall,
                color = Color.White,
                modifier = Modifier
                    .align(Alignment.BottomStart)
                    .padding(4.dp)
                    .background(Color(0xAA000000), RoundedCornerShape(4.dp))
                    .padding(horizontal = 4.dp, vertical = 1.dp),
            )
        }

        if (selectionMode) {
            Icon(
                Icons.Filled.CheckCircle,
                contentDescription = if (selected) "已选择" else "未选择",
                tint = if (selected) MaterialTheme.colorScheme.primary else Color(0x99FFFFFF),
                modifier = Modifier.align(Alignment.TopEnd).padding(4.dp).size(20.dp),
            )
        }
        if (selected) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(MaterialTheme.colorScheme.primary.copy(alpha = 0.25f)),
            )
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun PhotoListRow(
    image: MediaImage,
    selected: Boolean,
    selectionMode: Boolean,
    onClick: () -> Unit,
    onLongClick: () -> Unit,
) {
    val context = LocalContext.current
    var bitmap by remember(image.id) { mutableStateOf<android.graphics.Bitmap?>(null) }
    val dateText = remember(image.dateAddedSeconds) {
        SimpleDateFormat("yyyy-MM-dd HH:mm", Locale.getDefault())
            .format(Date(image.dateAddedSeconds * 1000L))
    }

    LaunchedEffect(image.id) {
        bitmap = MediaStoreRepository.loadThumbnail(context, image, 160)
    }

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(
                if (selected) {
                    MaterialTheme.colorScheme.primary.copy(alpha = 0.18f)
                } else {
                    Color.Transparent
                },
            )
            .combinedClickable(onClick = onClick, onLongClick = onLongClick)
            .padding(horizontal = 12.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(56.dp)
                .clip(RoundedCornerShape(6.dp))
                .background(MaterialTheme.colorScheme.surfaceVariant),
            contentAlignment = Alignment.Center,
        ) {
            val current = bitmap
            if (current != null) {
                Image(
                    bitmap = current.asImageBitmap(),
                    contentDescription = image.name,
                    contentScale = ContentScale.Crop,
                    modifier = Modifier.fillMaxSize(),
                )
            } else {
                Icon(Icons.Filled.Image, contentDescription = null)
            }
        }
        Spacer(Modifier.width(12.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                image.name,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(2.dp))
            Text(
                buildString {
                    append(dateText)
                    append(" · ")
                    append(image.pixelSize)
                    append(" · ")
                    append(image.displaySize)
                    if (image.isRaw) append(" · RAW")
                },
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (selectionMode) {
            Icon(
                Icons.Filled.CheckCircle,
                contentDescription = if (selected) "已选择" else "未选择",
                tint = if (selected) MaterialTheme.colorScheme.primary else Color(0x55FFFFFF),
            )
        }
    }
}

@Composable
private fun EmptyGallery(modifier: Modifier = Modifier, filtered: Boolean) {
    Column(
        modifier = modifier.padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(
            Icons.Filled.PhotoLibrary,
            contentDescription = null,
            modifier = Modifier.size(44.dp),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(12.dp))
        Text(
            if (filtered) "没有符合条件的照片" else "相册里还没有照片",
            style = MaterialTheme.typography.titleSmall,
        )
        Spacer(Modifier.height(6.dp))
        Text(
            if (filtered) "换个关键词或筛选条件试试" else "可以点下方相机拍一张，或把照片放进设备相册",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

private fun Set<Long>.toggle(id: Long): Set<Long> =
    if (id in this) this - id else this + id
