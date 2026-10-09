# fdxmp PROJECT_REPORT

> 更新（2026-10-06）：色彩管线经过多轮修复（P0 三连、线性 f16 工作空间、色相 / 色调
> 分区重写、输入 / 导出色彩管理闭环、白平衡色适应 + 颜色分级重做），
> 最新行为、性能与未修台账以 [`docs/COLOR_FIX_LOG.md`](docs/COLOR_FIX_LOG.md) 为准；
> 本报告 §3.3 的性能数字与部分渲染描述已过时。

生成时间：2026-10-03
仓库：`F:\FD-Room`（对应规格中的 `fdxmp/` 根目录）

## 1. 项目概况

跨平台专业图像处理软件：Rust 核心 + wgpu GPU 渲染，移动端完全对标 Lightroom
（三大 Tab + 底部弹出编辑面板），Windows 端采用 Lightroom Classic 式桌面布局。

| 端 | 技术 | 状态 |
| --- | --- | --- |
| 核心 | Rust 2021 + wgpu + rsraw/LibRaw + libheif | ✅ 可用 |
| Windows | egui / eframe（桌面布局 + GPU 实时预览） | ✅ 可用 |
| Android | Kotlin + Jetpack Compose + JNI | 🟡 骨架可编译：APK 已产出（三 ABI Rust 桥接，2026-10-03），业务功能待接入 |
| HarmonyOS | ArkTS + ohos-rs N-API | 🟡 骨架可打包：HAP 已产出（DevEco 26 / API 26，命令行 hvigorw，2026-10-03）；ohos-rs 桥接与真机签名待办 |
| iOS | SwiftUI + C ABI（占位，不打包） | 🟡 骨架（按规格不打包） |

## 2. 阶段交付与验证

| 阶段 | 内容 | 状态 | 报告 |
| --- | --- | --- | --- |
| 0 | 脚手架：workspace / 7 个 crate / FFI 方案 / 目录骨架 | ✅ | `docs/STAGE0_REPORT.md` |
| 1 | 解码与 GPU 显示：RAW（rsraw）/ HEIF（libheif）/ 常见格式，代理图 + 直方图 | ✅ | `docs/STAGE1_REPORT.md` |
| 2 | 色彩管线：15 项调色 + 曲线 + HSL + 颜色分级 + 细节（wgpu compute）与实时预览 | ✅ | `docs/STAGE2_REPORT.md` |
| 3 | 蒙版：画笔 / 线性 / 径向 / 颜色范围 / 亮度范围 + GPU 局部调整 + UI（AI 接口预留） | ✅ | `docs/STAGE3_REPORT.md` |
| 4 | 预设与导出：`.fdxmp`、Lightroom XMP 导入、强度混合、JPEG/PNG/TIFF/WebP/HEIC 导出、水印、批处理 | ✅ | `docs/STAGE4_REPORT.md` |
| 5 | 移动端 UI：Android / HarmonyOS / iOS 三大 Tab 工程骨架 | 🟡 Android 已能构建 APK（SDK/NDK/Gradle 就绪）；HarmonyOS 已能用命令行打出 HAP（DevEco 26 / API 26，unsigned）；iOS 按规格不打包 | `docs/STAGE5_REPORT.md` |
| 6 | Windows UI 与打包器 | 🟡 桌面 UI 已完成；打包器可用；ICC/EXIF/1:1 分块渲染已完成（2026-10-03），安装包未生成 | `docs/DESKTOP_UI.md` |
| 7 | AI 蒙版与性能优化 | 🟡 Windows 端 AI 蒙版已完成（ONNX Runtime + 开源模型）；移动端推理待各平台运行时；性能达标（185fps） | `docs/AI_MASKS.md` |

## 3. 关键能力清单

### 3.1 解码

- RAW：rsraw / LibRaw，内嵌预览首屏 **218ms**（45.7MP Nikon Z8），全尺寸 16bit 解调 4.9s。
- HEIF：libheif 1.23.5，解码 + 探测 + 编码（导出）。
- 常见格式：JPEG / PNG / TIFF / WebP / BMP / GIF。
- 显示代理 2048px + 元数据（厂商 / 机型 / ISO / 快门 / 光圈 / 焦距 / 拍摄时间）。

### 3.2 编辑

- 15 项调色、曲线（RGB + 3 通道 + 参数曲线，LUT 实现）、HSL 8 色带、
  颜色分级（3 区 + 混合 / 平衡）、细节（纹理 / 清晰度 / 锐化 / 亮度降噪 / 颜色降噪 / 去雾）、
  暗角、颗粒。
- 蒙版 5 类 + 每蒙版独立全套参数 + 画笔直接涂抹 + 叠加显示。
- 历史记录（拖动实时预览、松手记一步）、预设（内置 7 类 + 用户 + XMP 导入 + 强度）。

### 3.3 性能（release，GTX 1050 Ti / Vulkan）

| 场景 | 结果 |
| --- | --- |
| 45.7MP RAW 首屏（内嵌预览） | 218 ms（目标 < 2s） |
| 24MP JPEG 解码 | 110 ms |
| GPU 调色 2048×1365（含两趟模糊） | 5.4 ms/帧 ≈ 185 fps（目标 ≥30fps） |
| GPU + 2 个蒙版 | 10.0 ms/帧 ≈ 100 fps |

## 4. 测试与质量

| 检查 | 结果 |
| --- | --- |
| `cargo test -p fdxmp-core --features gpu,raw,heif` | ✅ 50 单元 + 5 解码集成 + 8 GPU 集成 + 4 预设集成 |
| `cargo test --workspace` | ✅ 全绿 |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 0 警告 |
| 全特性 clippy | ✅ 0 警告 |
| `cargo fmt --all -- --check` | ✅ 通过 |
| criterion 基准 | ✅ 参数序列化 / 预设打包 / 24MP 解码 / RAW 首屏与全尺寸 |

FFI：`fdxmp_ffi.dll` 导出 20 个 C ABI 符号（引擎生命周期、参数 JSON、撤销重做、
预设读写、探测、缓存），Android JNI 与 HarmonyOS N-API 对照表就绪。

## 5. 已知限制

1. ~~AI 蒙版（Windows）~~：已完成（U2NetP / SegFormer-ADE / face-parsing + ONNX Runtime，见 `docs/AI_MASKS.md`）；移动端推理待各平台运行时。
2. **移动端编译**：需要 Android SDK/NDK + cargo-ndk、DevEco + ohos 目标、macOS + Xcode。
3. ~~ICC / EXIF~~：已接入 lcms2（ICC 转换 + 写回）与 EXIF / XMP 写回（2026-10-03）；TIFF / WebP / HEIC 的元数据写回待补。
4. **1:1 像素级缩放**：当前显示代理 2048px，全分辨率分块渲染待做（阶段 6）。
5. **中文文字水印**：内置点阵字体仅 ASCII，中文需字体引擎。
6. **ohos-rs**：需可访问 GitHub git 的环境启用（`crates/ohos_bridge/README.md`）。

## 6. 运行方式

```powershell
cd F:\FD-Room
. .\packaging\windows\dev-env.ps1                 # 加载 GNU 工具链 + 依赖环境
cargo run -p fdxmp-win-ui --features gpu,raw,heif  # 桌面端全功能
cargo run -p fdxmp-packer-ui                       # 打包器
cargo test --workspace                             # 全量测试
```

双击运行：`cargo build -p fdxmp-win-ui --features gpu,raw,heif` 后执行
`packaging\windows\deploy-runtime.ps1 -Exe target\debug\fdxmp-win-ui.exe` 复制运行时 DLL，
之后可直接双击 exe。

## 7. 结论

规格中的阶段 0–4 全部完成并通过测试；阶段 5 交付三端工程骨架（受本机 SDK 限制未编译）；
阶段 6 的桌面 UI 与打包器已可用，ICC/EXIF/分块渲染已完成（安装包待生成）；阶段 7 的性能目标已达成，
AI 模型集成需要模型与运行时。核心（解码 / 调色 / 蒙版 / 预设 / 导出）已是可运行的完整闭环。

## 8. 功能缺口清单（2026-10-03 更新）

图例：✅ 已完成　🟡 部分完成　❌ 未实现　⛔ 受环境/依赖阻塞

### 8.1 编辑能力

| 功能（规格要求） | 状态 | 说明 |
| --- | --- | --- |
| 15 项调色 | ✅ | GPU compute，桌面端可交互 |
| 曲线（RGB + 3 通道 + 参数曲线） | ✅ | 可拖点编辑器；曲线为线性插值（可升级单调样条） |
| HSL 8 色带 | ✅ | 3 模式 × 8 色带 |
| 颜色分级（3 区 + 混合 / 平衡） | ✅ | |
| 细节：锐化 / 降噪 | ✅ | 锐化半径/细节/蒙版（梯度自适应）、降噪亮度细节/对比度、颜色细节/平滑度全部进着色器（2026-10-03） |
| 镜头校正（配置文件 / 色差 / 晕影 / 几何） | ✅ | 手动校正（畸变/色差/暗角/缩放进几何趟）+ **配置文件校正**（纯 Rust lensfun 移植，内置数据库，按相机/镜头/焦距/光圈/距离插值，2026-10-03） |
| 变换（透视 / 旋转 / 翻转） | ✅ | GPU 几何趟 `cs_geometry` + 变换面板实时预览（2026-10-03） |
| 裁剪 + 旋转（含比例 / 网格 / 自动构图） | ✅ | 独立裁剪模式：全幅视图 + 四周压暗 + 手柄 + 网格 + 比例 + 摆正 + 自动构图（2026-10-03） |
| 蒙版：画笔 / 线性 / 径向 / 颜色范围 / 亮度范围 | ✅ | 独立蒙版编辑视图（M / 点击蒙版进入）：画面叠加 + 底部工具条 + 画布涂抹 / 拖动渐变 + 反向 / 羽化 / 不透明度（2026-10-03） |
| 蒙版：深度范围 | 🟡 | 接口预留（需要深度图来源，移动端人像模式） |
| AI 蒙版（主体 / 天空 / 背景 / 人物 / 物体） | ✅ | Windows 端完成：ONNX Runtime（`ort` load-dynamic）+ U2NetP / SegFormer-ADE / face-parsing；物体=点按连通域；可选 BiRefNet-lite 高质量；见 `docs/AI_MASKS.md`（2026-10-03） |
| 蒙版复制 / 粘贴（跨图） | ✅ | 蒙版「拷贝」进剪贴板，任意图片「粘贴蒙版」（2026-10-03） |
| 历史：撤销 / 重做 / 跳转 | ✅ | 拖动实时预览、松手记一步 |
| 历史快照 | ✅ | 历史面板「创建快照」按钮（★ 标记，可跳转）（2026-10-03） |

### 8.2 预设与导出

| 功能 | 状态 | 说明 |
| --- | --- | --- |
| 内置预设 7 分类 | ✅ | `presets/` 已生成 7 个 `.fdxmp`，UI 可一键应用 |
| 预设缩略图预览 / 长按详情 | 🟡 | 桌面端内置预设缩略图网格已完成（2026-10-03）；长按详情属移动端交互 |
| 用户预设 / XMP 导入 / 强度 | ✅ | XMP 覆盖 25 个常用 `crs:` 参数 |
| 导出格式 5 种 | ✅ | JPEG / PNG / TIFF / WebP / HEIC（HEIC 需 `heif` feature） |
| 导出尺寸 / 质量 | ✅ | 原始 / 长边 / 短边 / 百分比 |
| 色彩空间（P3 / AdobeRGB / ProPhoto） | ✅ | lcms2（静态编译）真实转换 + JPEG APP2 / PNG iCCP 写回（2026-10-03） |
| 元数据保留 / 删除 | ✅ | EXIF / XMP 写回 JPEG（APP1）与 PNG（eXIf），RAW 取内嵌预览元数据；其余容器待补 |
| 水印（文字 / 图片） | ✅ | 图片水印完整；文字水印改用 TrueType 字体（ab_glyph），支持中文（2026-10-03） |
| 批处理 | ✅ | core `export_batch`；UI 胶片条标记 + 「批量导出标记照片」（2026-10-03） |
| 导出到相册 / 分享 | ❌ | 移动端功能 |

### 8.3 移动端（三大 Tab 与流程）

| 功能 | 状态 | 说明 |
| --- | --- | --- |
| 三大 Tab 骨架（三端） | ✅ | Android Compose / HarmonyOS ArkTS / iOS SwiftUI |
| 冷启动流程（启动画面 → 初始化核心 → GPU → 登录 → 缓存 → 主界面） | ❌ | 需在移动端接 FFI + 启动页 |
| Tab1 设备：相册权限、网格/列表、全屏预览、多选批量、搜索排序筛选 | 🟡 | Android / HarmonyOS 已实现并打包验证（2026-10-09）；iOS 代码就绪，待 macOS + Xcode 编译；RAW 预览待 Rust 核心接入 |
| Tab2 编辑：编辑记录列表、缩略图 + 编辑时间 + 预设名、排序、滑动删除、批量导出 | 🟡 | Android 已实现记录列表（SharedPreferences + JSON，点开续编 / 删除，2026-10-09）；排序与批量导出待补；鸿蒙 / iOS 待做 |
| Tab3 我的：登录、账号同步、默认导出、缓存管理、预设管理、主题、语言、关于、隐私、退出 | 🟡 | 缓存管理 core 已就绪，其余待做 |
| 编辑界面（顶部工具 / 中部预览 / 底部弹出面板 / 实时 GPU 预览） | 🟡 | Android v1：Lightroom 式版式 + 直方图 + 15 项基础滑块 + 内置预设 + 曲线编辑器 + 效果子 Tab + 裁切 + 蒙版（线性 / 径向）+ HSL（8 色带）+ 颜色分级 + 细节（锐化 / 降噪）+ 长按看原图，全部实时防抖渲染（2026-10-09）；画笔与范围蒙版 / AI 蒙版 / 镜头 / 配置文件与 GPU 实时预览待做 |
| 相机入口 / 分享 / 收藏 | ❌ | 移动端系统集成 |
| 账号体系（先预留接口） | ❌ | 未定义接口 |

### 8.4 桌面端与工程

| 功能 | 状态 | 说明 |
| --- | --- | --- |
| Windows 桌面 UI（布局 / 面板 / 快捷键 / 拖放 / 导入 / 直方图 / 历史） | ✅ | 见 `docs/DESKTOP_UI.md` |
| 1:1 像素级缩放 / 全分辨率分块渲染 | ✅ | 1:1 视图按需解码全分辨率 + 只渲染可见分块（2026-10-03）；缩略图 LOD 金字塔待后续 |
| 多窗口 / 第二显示器 / 右键菜单 / 布局持久化 | ✅ | 用户澄清「第二窗口」= 蒙版 / 裁剪独立编辑视图（已完成）；右键菜单（画布 + 胶片条）与布局持久化（`ui_state.json`）已完成（2026-10-03）；真正的多窗口不做 |
| 打包器（检测 / 卡片 / 日志 / iOS 禁用） | ✅ | 可直接运行 |
| NSIS 安装包 / 7z 分发包 | ✅ | `dist/fdxmp-setup.exe`（NSIS 向导：目录选择 / 快捷方式 / 卸载器 / 应用和功能项，从 Gitee 源码自建 makensis）、`dist/fdxmp-setup-sfx.exe`（兜底）、`dist/fdxmp-0.1.0-win64.7z`（17.7 MB）；安装与卸载已实测（2026-10-03） |
| Android / HarmonyOS 打包 | 🟡 | Android APK ✅（`dist/android/`，三 ABI）；HarmonyOS HAP ✅ 命令行打包已打通（`dist/harmony/entry-default-unsigned.hap`，DevEco 26 / API 26，2026-10-03）；签名（真机安装）与桥接 .so 待办 |
| 移动端低端设备降级 | ❌ | 阶段 7 内容 |
| 冷启动 < 2s（桌面） | 🟡 | 已测：热启 0.84–0.87 s（达标）；文件缓存冷启动首次 3.6 s（超标，待延迟初始化 / 装 SSD） |

### 8.5 需要用户决策 / 环境的阻塞项

1. **移动端编译环境**：Android 已就绪（SDK/NDK/cargo-ndk，`dist/android/`）；HarmonyOS 已就绪（DevEco Studio 26 + API 26 装在 E 盘，命令行 hvigorw 已验证打包，2026-10-03）；剩余：ohos-rs 桥接 .so 需能访问 GitHub git 的环境、真机签名需一次性「自动签名」或 AGC 证书。
2. ~~ICC / EXIF 写回~~：已按用户授权引入 lcms2（静态编译）与字体引擎 ab_glyph，无需再确认。
3. **AI 蒙版**：Windows 已接入 ONNX Runtime + 走 hf-mirror 的模型下载；Android / iOS / 鸿蒙端推理待各平台运行时。
4. ~~镜头校正数据库~~：已用纯 Rust lensfun 移植完成（2026-10-03）。

### 8.6 建议优先级（2026-10-03 更新）

执行计划见 `docs/AUTONOMOUS_PLAN.md`。

1. ✅ 已完成：裁剪 / 旋转 / 变换 + 细节从属参数进着色器（P1 / P2）。
2. 进行中：1:1 全分辨率分块渲染（P3）→ 批量导出 UI、历史快照、预设缩略图、蒙版跨图复制粘贴（P4）。
3. B 类依赖（用户已同意酌情引入）：lensfun 镜头校正 → lcms ICC → EXIF 写回 → 中文字体水印（P5）。
4. ✅ 工程：7-Zip 独立版与 NSIS（从 Gitee 源码自建）已就绪，安装包/7z/冷启动测量完成（P6）。
5. 移动端：Android / HarmonyOS 均已能打包（APK / unsigned HAP）→ 下一步做设备 Tab（相册 + 预览）→ 编辑 Tab → 编辑界面；AI 蒙版需模型与推理运行时（P7）。
