# iOS 平台工程（阶段 5 骨架）

- UI：SwiftUI，三大 Tab（设备 / 编辑 / 我的），`TabView` + 系统图标。
- 桥接：C ABI 静态库（`crates/ffi`，`fdxmp.h`）。
- 权限：`NSPhotoLibraryUsageDescription` / `NSPhotoLibraryAddUsageDescription`。

## 构建（占位，不打包）

按规格：**iOS 先写代码，不打包**。后续接入步骤：

1. `cargo build --release --target aarch64-apple-ios -p fdxmp-ffi`（macOS + Xcode 环境）
2. 合并 device / simulator 静态库为 `xcframework`
3. Xcode 工程链接 xcframework，`NativeCore.swift` 替换占位实现为真实 C ABI 调用
4. 阶段 5 后续：相册（PHPhotoLibrary）、编辑界面（底部弹出工具面板）、Core ML / Vision AI 蒙版

> 本机为 Windows，无 Xcode / iOS SDK，该工程未编译验证。

