# 阶段 5 报告：移动端工程（骨架）

日期：2026-10-03
范围：Android / HarmonyOS / iOS 三大 Tab 工程骨架 + 桥接接入点

## 1. 已完成

### 1.1 Android（`platforms/android/`）

| 文件 | 作用 |
| --- | --- |
| `settings.gradle.kts` / `build.gradle.kts` / `app/build.gradle.kts` | Gradle 工程（AGP 8.5 / Kotlin 1.9 / Compose BOM 2024.06，minSdk 26） |
| `app/src/main/AndroidManifest.xml` | 相册权限（`READ_MEDIA_IMAGES` + 旧版兼容）、单 Activity |
| `MainActivity.kt` | **Jetpack Compose 三大 Tab**（设备默认 / 编辑 / 我的），底部 `NavigationBar`、顶部标题随 Tab 变化、无侧边栏；启动时通过 JNI 读取 Rust core 版本 |
| `core/NativeBridge.kt` | JNI 桥接（`System.loadLibrary("fdxmp_android_bridge")`，暴露 `nativeAbiVersion` / `nativeVersion`） |
| `README.md` | `cargo ndk` + Gradle 构建步骤与阶段 5 后续清单 |

### 1.2 HarmonyOS（`platforms/harmony/`）

| 文件 | 作用 |
| --- | --- |
| `entry/src/main/ets/pages/Index.ets` | **ArkTS 三大 Tab**：底部导航 + 顶部标题随 Tab 变化，`aboutToAppear` 读取 N-API 版本 |
| `entry/src/main/ets/core/NativeBridge.ets` | `libfdxmp_ohos_bridge.so` 封装（ohos-rs / N-API） |
| `EntryAbility.ets` / `module.json5` | UIAbility 与模块声明，申请 `ohos.permission.READ_IMAGEVIDEO` |
| `build-profile.json5` / `oh-package.json5` / `hvigorfile.ts` | hvigor 工程配置 |
| `README.md` | ohos-rs 启用步骤与阶段 5 后续清单 |

### 1.3 iOS（`platforms/ios/`，占位不打包）

| 文件 | 作用 |
| --- | --- |
| `Fdxmp/FdxmpApp.swift` + `ContentView.swift` | **SwiftUI 三大 Tab**（`TabView` + 系统图标，设备默认） |
| `Fdxmp/NativeCore.swift` | C ABI 接入点（`fdxmp_version` / `fdxmp_abi_version`，阶段 5 链接静态库后替换占位） |
| `Fdxmp/Info.plist` | 相册读写权限说明 |
| `README.md` | xcframework 合并与 Xcode 接入步骤 |

## 2. 验证情况

| 项目 | 状态 |
| --- | --- |
| Rust 桥接 crate 编译 + 测试 | ✅ `cargo test -p fdxmp-android-bridge` / `-p fdxmp-ohos-bridge` 通过（宿主机构建） |
| Android 工程编译 | ⏸ 本机无 Android SDK/NDK 与 `cargo-ndk`（`D:\dev\android-sdk` 为空目录） |
| HarmonyOS 工程编译 | ⏸ 本机无 DevEco / hvigor / ohos 目标 |
| iOS 工程编译 | ⏸ 本机为 Windows，无 Xcode |

三端骨架的代码结构、权限声明、桥接调用方式与规格一致；缺的只是对应 SDK 环境，
装上 SDK 后按各 README 的命令即可构建。

## 3. 遗留（阶段 5 后续 / 需要移动端环境）

1. 三端接入完整 C ABI（解码 / 编辑 / 导出 / 缓存）——桌面端已验证的同一套语义。
2. 设备 Tab：相册查询 + 网格 / 列表 + 全屏预览（MediaStore / photoAccessHelper / PHPhotoLibrary）。
3. 编辑 Tab：本地数据库记录已编辑文件与预设名。
4. 我的 Tab：缓存管理、预设导入导出、主题、账号接口预留。
5. 编辑界面：底部弹出工具面板、裁剪 / 旋转、实时 GPU 预览（wgpu Vulkan / Metal）。

## 4. 结论

阶段 5 在**可在 Windows 上完成的范围**内交付：三端工程骨架、三大 Tab 结构、
权限声明、桥接接入点与构建文档全部就绪；实际编译与真机运行需要对应 SDK / 工具链。

---

## 5. 后续更新：设备 Tab 实现（2026-10-09）

三端设备 Tab 从占位骨架升级为可用界面（编辑 / 我的两个 Tab 仍为占位）：

| 平台 | 实现 | 验证 |
| --- | --- | --- |
| Android | `MainActivity.kt` + `ui/DeviceTab.kt` / `ui/PhotoViewer.kt` / `media/MediaStoreRepository.kt`：运行时权限（含 Android 14 部分授权）、MediaStore 网格 / 列表、搜索 / 排序 / 筛选、长按多选、全屏预览（左右滑动 + 双击 / 双指缩放）、导入 / 相机 / 分享 | `gradle assembleDebug` 通过；API 35 x86_64 模拟器实测通过 |
| HarmonyOS | `pages/Index.ets` + `model/PhotoItem.ets`：`photoAccessHelper` 相册查询、运行时权限、网格 / 列表、搜索 / 排序 / 筛选、长按多选、全屏 Swiper 预览、系统相册导入 / 相机 | `hvigorw assembleHap` 通过（未签名 HAP） |
| iOS | `DeviceTabView.swift`：Photos 权限、`PHAsset` 网格 / 列表、搜索 / 排序 / 筛选、长按多选、全屏分页预览、PhotosPicker 导入、相机、系统分享 | 代码就绪，需 macOS + Xcode 编译验证 |

RAW 文件会在网格中标记，但预览需要 Rust 核心解码（阶段 6 接入），
当前显示提示文案。

### 5.1 编辑 Tab 与基础调色（Android，2026-10-09）

- Rust 核心新增 CPU 预览渲染 `preview::render_preview_rgba`（白平衡 / 曝光 / 对比度与
  WGSL 同公式，分区色调 / 饱和度 / 暗角 / 颗粒为 CPU 近似），C ABI 新增
  `fdxmp_render_preview_rgba`，Android JNI 新增 `nativeRenderPreview`；
- Android 编辑界面按手机 Lightroom 版式重做：顶部「返回 / 信息 / 撤销 / 重做 / 保存」
  + 直方图叠加 + 大图预览，底部只保留「预设 / 裁切 / 编辑 / 蒙版」，长按图片看原图；
  编辑页为亮度 / 颜色 / 效果 三个分类 + 内置预设；亮度后面是曲线图标，进入全屏曲线
  编辑器（网格 + 可拖拽控制点 + 白 / 红 / 绿 / 蓝 / 参数通道，与 Rust 端单调三次
  曲线 LUT 同公式）；效果分类内用「效果 / 晕影 / 颗粒」子 Tab 切换（暗角支持中点 /
  羽化，颗粒支持大小 / 粗糙度）；裁切、蒙版面板为下一步；
- 按需求移除分享入口（预览页与设备页批量分享均已去掉）；
- Android 编辑 Tab：编辑记录列表（缩略图 + 时间 + 预设名），点开继续编辑、删除记录；
- 验证：API 35 x86_64 模拟器实测通过（设备 → 编辑 → 曝光 +2.9 → 保存 →
  编辑 Tab 出现记录）。
