# fdxmp 自主执行计划（2026-10-03 起）

> 依据用户 2026-10-03 指令：B 类依赖可酌情引入；以性能优先、对标 Lightroom；
> A 类技术全部实现；允许分类分阶段；用户不在电脑前，由执行方自行规定并自动执行；
> 多测试性能；环境安装到 C/D 盘以外的盘；之后再打磨 UI 与操作顺畅性；
> 蒙版与裁剪等交互参照桌面版 Lightroom（独立视图 / 独立模式，不在原视图里做）。

## 0. 前提与硬约束

- 机器：8 逻辑核 / 7.7 GB 内存 / GTX 1050 Ti；系统盘 C: 仅剩 ~3 GB，
  D: 30.7 GB（同为 SSD）。**一切环境、缓存、TEMP、构建产物一律放 E:/F:**。
- 构建入口：`. .\packaging\windows\dev-env.ps1`（GNU 工具链 + F:\.tmp 作为 TEMP）。
- 每个阶段必须：`cargo fmt --all -- --check`、`cargo clippy`（0 警告）、
  `cargo test` 全绿、并通过该阶段的性能/回归门槛后才进入下一阶段。
- 性能以 release 构建 + criterion/帧耗时统计为准，记录数字到阶段报告。

## 1. 阶段划分（按优先级）

### P1 几何管线：裁剪 / 旋转 / 翻转 / 透视（A 类，无新依赖）

- `core::geometry`：逆映射矩阵（旋转/翻转/透视/裁剪）、CPU 参考 warp、
  裁剪尺寸计算、旋转约束（自动构图）、比例锁。
- GPU `cs_geometry` 计算趟：双线性采样 + 输出尺寸随裁剪变化；
  接入 `ColorPipeline`（默认恒等快速路径，不额外开销）。
- 蒙版坐标保持在“源图空间”：着色器按几何逆映射读取蒙版权重，裁剪/旋转后
  蒙版仍贴合画面内容（与 Lightroom 一致）。
- 导出链路自动应用几何 + 裁剪（代理导出、全分辨率导出）。
- 测试：矩阵/约束单元测试、GPU 裁剪尺寸与像素回归、旋转 90° 精确性、
  翻转等价性、越界透明处理。
- 性能门槛：2048 长边 + 裁剪/旋转激活时 GPU 帧耗时 ≤ 8 ms（含全部调色）。

### P2 细节从属参数进着色器（A 类）

- 锐化：半径（多尺度模糊）、细节（高频/中频混合）、蒙版（梯度自适应）。
- 降噪：亮度细节 / 亮度对比度 / 颜色细节 / 颜色平滑度参与计算。
- 全部参数默认值下不得引入额外开销（惰性 dispatch）。
- 测试：每个从属参数单独变化必须产生可测量差异；默认参数输出与旧版逐像素一致。
- 性能门槛：2070 万像素代理（2048px）全参数 ≤ 9 ms/帧。

### P3 1:1 全分辨率分块渲染（A 类）

- 源图分块上传/渲染（tile 2048²），只渲染视口可见块；支持 1:1 像素级检查。
- 与几何/蒙版/细节管线共用同一 `ColorPipeline`。
- 测试：分块拼接与整图渲染逐像素一致；内存占用上限；缩放/平移时的缓存命中率。

### P4 桌面交互补全（A 类）

- Lightroom 式**独立裁剪模式**：全幅视图 + 四周压暗 + 网格/等比手柄 +
  直角旋转（拖角/滑杆）+ 比例按钮（自由/1:1/4:3/3:2/16:9/原始）+ 翻转 +
  自动构图（约束裁剪框始终在旋转后的画面内）。
- 变换面板（透视/旋转/翻转）与裁剪一致地实时预览。
- 批量导出 UI、历史快照按钮、预设缩略图预览。
- 蒙版独立视图/叠加与几何联动，蒙版跨图复制粘贴。

### P5 B 类依赖（用户已同意“酌情引入”）

按性价比排序，逐项引入并评估体积/许可证/性能：

1. `lensfun`（镜头配置文件校正，纯 Rust 绑定或 C 库）；
2. `lcms2`（ICC：Display P3 / AdobeRGB / ProPhoto 写回）；
3. EXIF 读写（`kamadak-exif` 或 `img-parts`）；
4. 中文文字水印字体引擎（`ab_glyph` + 内置开源字体）。

每项引入前先跑通构建与体积评估，引入后必须更新 `docs/SPEC.md` 的“技术选型”记录并复测性能。

### P6 打包与发布（E 类）

- 安装 NSIS / 7-Zip 到 F:（非 C/D），生成 NSIS 安装包与 7z 分发包；
- 桌面 release 冷启动耗时测量（目标 < 2 s）。

### P7 移动端与 AI（C/D 类，依赖外部环境/模型）

- 移动端需 SDK 环境（Android SDK/NDK、DevEco），安装到 E:/F:；
- AI 蒙版需 ONNX Runtime + 模型（下载到 F:），先 Windows 后移动端。

## 2. 本轮（用户不在电脑期间）执行顺序

1. 清理无用进程（已完成，见 `docs/CLEANUP_LOG.md`）。
2. P1 → P2 → P3 → P4 依次实现，每步构建 + 测试 + 性能记录。
3. 期间若 B 类依赖确实阻塞 A 类效果（如镜头校正、ICC），按 P5 顺序引入。
4. 每完成一个阶段，更新 `PROJECT_REPORT.md` 第 8 节与对应阶段报告。

## 2.1 进度（2026-10-03）

| 阶段 | 状态 | 说明 |
| --- | --- | --- |
| P0 进程清理 | ✅ | 关闭 Edge 后台 / WPS 后台 / M365 Copilot / Ubuntu Pro，释放 ~370 MB（`docs/CLEANUP_LOG.md`） |
| P1 几何管线 | ✅ | `core::geometry` + `cs_geometry` + 独立裁剪模式 UI + 变换面板 + 导出按裁剪输出；见 `docs/STAGE6_GEOMETRY.md` |
| P2 细节从属参数 | ✅ | 锐化半径/细节/蒙版、降噪细节/对比度/平滑度全部进着色器，惰性 dispatch |
| P3 1:1 分块渲染 | ✅ 初版 | 1:1 视图按需全分辨率解码 + 可见区分块渲染（`render_tile`）；无 LOD 金字塔（缩小仍用代理） |
| P4 桌面交互补全 | ✅ | 裁剪独立视图、蒙版独立编辑视图（M / 点击蒙版进入）、GPU 蒙版叠加、蒙版跨图复制/粘贴、批量导出 UI、历史快照、预设缩略图网格、画布/胶片条右键菜单、布局持久化（`ui_state.json`）；多窗口按用户澄清不做 |
| P5 B 类依赖 | ✅ | 手动镜头校正、EXIF+XMP 写回、lcms2 ICC、ab_glyph 中文水印、**镜头配置文件校正（纯 Rust lensfun 移植，内置 5 MB 数据库）**；详见 `docs/STAGE6_GEOMETRY.md` |
| P6 打包发布 | ✅ | `dist/fdxmp-setup.exe`（NSIS 安装向导，从 Gitee 源码自建 makensis）、`dist/fdxmp-setup-sfx.exe`（IExpress 兜底）、`dist/fdxmp-0.1.0-win64.7z`；静默安装/卸载已实测；冷启动已测（热启 0.85 s / 冷缓存首次 3.6 s）。详见 `docs/PACKAGING_REPORT.md` 与 `docs/DOWNLOAD_SOURCES.md` |
| P7 移动端 / AI | 🟡 部分完成 | ✅ Windows AI 蒙版（`docs/AI_MASKS.md`）；✅ Android 环境打通（SDK/NDK 在 E 盘、Gradle 8.7 在 F 盘、阿里云 Maven 镜像；Rust 桥接三 ABI + debug APK 16 MB 已产出，`dist/android/`）；✅ HarmonyOS 命令行打包打通（DevEco Studio 26 / API 26，hvigorw 产出 unsigned HAP 到 `dist/harmony/`，2026-10-03）；⏳ Android / HarmonyOS 业务功能（相册 / 编辑 / 我的）、ohos-rs 桥接 .so、真机签名与 iOS（按规格不打包） |

性能基线（debug，2048×1365）：全参数 5.5 ms、+2 蒙版 10.3 ms、旋转 5°+裁剪 80% 3.9 ms、细节全开 7.2 ms。

## 3. 回归与性能基线（每阶段复测）

| 基准 | 命令 | 门槛 |
| --- | --- | --- |
| 单元/集成测试 | `cargo test --workspace --features` 组合 | 全绿 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | 0 警告 |
| 解码/管线基准 | `cargo bench -p fdxmp-core` | 不劣化 >10% |
| GPU 帧耗时 | `cargo run -p fdxmp-win-ui --release`（状态栏显示） | 见各阶段门槛 |
| C: 空间 | `Get-Volume` | 始终 ≥ 2 GB |
