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

