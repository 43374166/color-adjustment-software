# HarmonyOS 平台工程（阶段 6：编辑界面开始接入）

- UI：ArkTS / ArkUI，三大 Tab（设备 / 编辑 / 我的）。
- 桥接：**已启用**。
  `libfdxmp_ohos_bridge.so`（N-API 薄封装，`entry/src/main/cpp/napi_init.cpp`）
  → `libfdxmp_ffi.so`（Rust 核心，C ABI，`crates/ffi`），与 Windows / iOS 同一份调色管线。
- 编辑页：`pages/EditorPage.ets`（顶部栏 + 预览 + 底部分类面板）。
  目前接通「颜色 → 均衡器」：512 点曲线由 Rust 烘焙（`NativeBridge.equalizerCurve`），
  8 个节点可拖、双击复位、两端各半；其余分类 / 子页先占位。
- 权限：`ohos.permission.READ_IMAGEVIDEO`。

## 构建

```powershell
. .\packaging\harmony\dev-env.ps1        # DevEco 路径 / hvigor / ohpm / NDK 联接
cd platforms\harmony
.\hvigorw.bat assembleHap --mode module -p product=default
```

一步到位（交叉编译 Rust + 打包 + 复制产物）：

```powershell
.\packaging\harmony\build.ps1 -WithRustBridge   # → dist/harmony/entry-default-unsigned.hap
```

`-WithRustBridge` 会为两个 ABI 交叉编译 `crates/ffi`：

| ABI | 用途 | Rust target |
| --- | --- | --- |
| `arm64-v8a` | 真机 | `aarch64-unknown-linux-ohos` |
| `x86_64` | 本地模拟器（Mate 90 Pro 是 x86_64） | `x86_64-unknown-linux-ohos` |

产物会同时复制到 `entry/src/main/cpp/libs/<abi>/`（CMake 链接用）和 `entry/libs/<abi>/`（打包用），
两处都在 `.gitignore` 里，不进仓库。

要点：

- 两侧 `.so` 缺一不可：`libfdxmp_ohos_bridge.so` 的 DT_NEEDED 指向 `libfdxmp_ffi.so`，
  运行时从 App 的 lib 目录解析。
- NDK 路径含空格（`DevEco Studio`），`dev-env.ps1` 会建一个无空格的目录联接
  `%SystemDrive%\ohos-ndk` 并导出 `FDXMP_OHOS_NDK`，否则 cc-rs 会把 `CC` 截断在空格处。
- `build-profile.json5` 的 `externalNativeOptions.abiFilters` 必须同时包含 `arm64-v8a` 和 `x86_64`，
  否则模拟器上加载不到桥接库。
- 无签名配置时产出 `entry-default-unsigned.hap`；装设备需要签名（DevEco 里做一次「自动签名」）。

## 下一步

1. 编辑页接渲染：解码 → `NativeBridge.render()` → PixelMap 预览（参数变化防抖渲染）。
2. 颜色分类其余子页：颜色 / HSL / 分级 / 点颜色 / 校准 / 黑白。
3. 直方图叠加（填充 + 波形，点击切换）。
4. 曲线、光学、细节、效果、几何。
5. 蒙版 / 修图 / 裁剪 / 预设 / 导出 / 我的页。
