# 阶段 0 报告：脚手架

日期：2026-10-02
范围：Cargo workspace、全部 crate、四端 FFI 可行性、项目骨架、构建与文档

## 1. 已完成

### 1.1 工程骨架

- Cargo workspace（`F:\FD-Room`，对应规格中的 `fdxmp/` 根）+ 7 个成员：

| crate | 类型 | 作用 |
| --- | --- | --- |
| `fdxmp-core` | lib | 解码、色彩管线、蒙版、预设、历史、缓存、导出参数 |
| `fdxmp-ffi` | cdylib + staticlib + rlib | 四端共用 C ABI（`include/fdxmp.h`） |
| `fdxmp-cli` | bin | 命令行测试工具（version / probe / preset / cache） |
| `fdxmp-win-ui` | bin | Windows UI 骨架（egui / eframe，三大 Tab） |
| `fdxmp-android-bridge` | cdylib + rlib | JNI 桥接（`com.fdxmp.core.NativeBridge`） |
| `fdxmp-ohos-bridge` | cdylib + rlib | ohos-rs / N-API 桥接设计 |
| `fdxmp-packer-ui` | bin | 单文件 UI 打包器（`packer_ui/src/main.rs`） |

- 目录骨架全部就位：`platforms/{ios,android,harmony,windows}`、
  `packaging/{windows,android,harmony,ios}`、`dist/{windows,android,harmony,ios}`、
  `presets/`、`docs/`、`assets/`。
- `packaging/windows/build.ps1` + `fdxmp.nsi`、`packaging/android|harmony/build.ps1`、
  iOS 占位说明；`packaging/windows/dev-env.ps1` 提供本机开发环境。
- git 仓库已初始化（未提交，工作区就绪）。

### 1.2 核心模块（可编译、可测试）

- **参数模型** `params.rs`：规格规定的 15 项调色滑块（顺序 / 中英文标签 / 取值范围 /
  get-set 与夹取）、曲线（RGB + 红绿蓝 + 参数曲线）、HSL 8 通道、颜色分级、
  细节（锐化 + 降噪）、镜头校正、变换、裁剪（比例 / 网格 / 旋转 / 翻转）、
  蒙版（画笔 / 线性 / 径向 / 颜色范围 / 亮度范围 / 深度范围预留 / AI 主体 / 天空 /
  背景 / 人物部位 / 物体 + 叠加色 / 反向 / 羽化 / 独立全套参数）。
- **预设 `.fdxmp`** `preset.rs`：JSON + 可选内嵌 LUT 的 ZIP 包；必填字段
  `schema_version` / `name` / `author` / `created_at` / `params`；版本校验、
  磁盘读写、RFC 3339 时间戳（无第三方时间库）；XMP 导入入口已预留（阶段 4）。
- **历史** `history.rs`：撤销 / 重做 / 跳转 / 快照 / 分支截断 / 步数上限。
- **缓存** `cache.rs`：缩略图 / 预览 / AI 模型三类缓存的大小统计、单类与整体清除、
  人类可读大小（「我的」Tab 的核心功能已具备实现）。
- **解码** `decode.rs`：魔数嗅探 + 扩展名推断（含 RAW 扩展名表）、常见格式尺寸探测、
  统一解码入口；RAW（rsraw）与 HEIF（libheif-rs）以 feature 接入。
- **色彩管线** `pipeline.rs`：15 阶段顺序常量 + 曝光/对比度 CPU 参考实现
  （阶段 2 与 GPU 结果做 golden 比对）。
- **GPU** `gpu.rs`（feature `gpu`）：wgpu 30 实例 / 适配器 / 设备 / 队列 +
  后端名称，已通过编译验证。
- **引擎** `lib.rs`：`Engine`（参数 + 历史 + 缓存）、参数 JSON 读写、
  默认缓存目录（可被 `FDXMP_CACHE_DIR` 覆盖）。

### 1.3 FFI 层

- `fdxmp-ffi` 导出 20 个 C ABI 符号（`fdxmp_ffi.dll` 导出表已核对）：
  版本 / 错误、内存释放、引擎生命周期、参数 JSON、撤销重做、历史查询、
  `.fdxmp` 字节流读写、解码探测、缓存统计与清除。
- 错误模型：NULL / -1 / UINT64_MAX + thread-local 错误信息；
  Rust panic 在边界被 `catch_unwind` 捕获；空指针安全。
- 头文件 `crates/ffi/include/fdxmp.h`，Android JNI 与 HarmonyOS N-API
  的符号 / 签名对照表分别落在两个 bridge crate 与 `docs/FFI.md`。

### 1.4 文档

`docs/ARCHITECTURE.md`（含 mermaid 架构图与数据流）、`docs/FFI.md`（桥接方案与
内存 / 错误 / ABI 约定）、`docs/API.md`、`docs/BUILD.md`、
`docs/SPEC.md`（需求冻结副本）、本报告。

### 1.5 可运行产物（界面截图）

| 产物 | 运行方式 | 截图 |
| --- | --- | --- |
| Windows 主界面（桌面布局） | `cargo run -p fdxmp-win-ui` | ![Windows 主界面](screenshots/stage0-win-ui.png) |
| 单文件打包器 | `cargo run -p fdxmp-packer-ui` | ![打包器](screenshots/stage0-packer-ui.png) |

- Windows 主界面：菜单栏（文件 / 编辑 / 视图 / 帮助 + 导入导出撤销重做）、左栏
  （目录 / 预设 / 历史，历史项可点击跳转）、中间画布（滚轮缩放提示）、右栏
  （直方图占位 + 15 项调色滑块 + 曲线 / HSL / 分级 / 细节 / 镜头 / 变换 / 蒙版 /
  预设 / 导出折叠面板）、底部胶片条与状态栏（版本 · ABI / 历史 / 缓存）。
  设计说明见 `docs/DESKTOP_UI.md`。
- **修订（2026-10-02）**：Windows 端最初照搬了移动端三 Tab，不符合桌面端定位；
  现已改为 Lightroom Classic 式桌面布局。规格中的三 Tab 流程仅适用于
  Android / HarmonyOS / iOS（`docs/DESKTOP_UI.md` §0 说明了分工与依据）。
- 桌面专属交互已可用：原生文件对话框导入、拖放导入、`Ctrl+Z` / `Ctrl+Y`
  撤销重做、`←` `→` 切换胶片条、`Tab` 隐藏 / 恢复面板、滑块双击复位、
  面板宽度可拖动、`视图` 菜单控制面板显隐。
- 打包器：每个平台一张卡片（Windows / Android / HarmonyOS / iOS），
  检测结果按绿 ✓ / 红 ✗ / 黄 ⚠ 着色，按钮执行 `packaging/*/build.ps1` 并把输出
  实时打到日志区，iOS 按钮禁用并显示「暂不支持」，输出目录默认 `dist/`。
- 中文字体：egui 内置字体不含 CJK，阶段 0 已加入系统中文字体加载
  （自动查找微软雅黑 / 黑体 / 等线，可用 `FDXMP_UI_FONT` 覆盖）；
  这是界面实拍中确认过的修复项。
- 说明：阶段 0 界面仅为骨架，尚无图片显示、解码与调色渲染（阶段 1/2 接入）。

## 2. 测试结果

环境：Rust 1.99.0（`stable-x86_64-pc-windows-gnu`）+ MinGW-w64 GCC 16.2
（本机无 Visual Studio，见 `docs/BUILD.md` §0.1）。

| 检查项 | 命令 | 结果 |
| --- | --- | --- |
| 全量检查 | `cargo check --workspace` | ✅ 通过 |
| 全量构建 | `cargo build --workspace` | ✅ 产出 `fdxmp_ffi.dll` / `fdxmp_android_bridge.dll` / `fdxmp_ohos_bridge.dll` / `fdxmp-cli.exe` / `fdxmp-win-ui.exe` / `fdxmp-packer-ui.exe` |
| 单元 + 集成测试 | `cargo test --workspace` | ✅ 12 个测试套件、33 个测试全部通过 |
| 格式 | `cargo fmt --all -- --check` | ✅ 通过 |
| 静态检查 | `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 通过（0 警告） |
| 基准 | `cargo bench -p fdxmp-core --bench params_bench -- --quick` | ✅ 运行成功 |
| CLI 冒烟 | `fdxmp-cli version / preset-new / preset-info / cache-size / cache-clear` | ✅ 通过（生成 `presets/base-correction.fdxmp`，855 字节，可回读） |
| feature `gpu` | `cargo check -p fdxmp-core --features gpu` | ✅ 通过（wgpu 30 API 验证） |
| feature `raw` | `cargo check -p fdxmp-core --features raw` | ⚠ 依赖解析成功，构建受本机缺少完整 Clang / MinGW 头文件限制（见 §4） |
| feature `heif` | `cargo check -p fdxmp-core --features heif` | ⚠ 需要 pkg-config + libheif（见 §4） |
| Android 真机编译 | `cargo ndk ...` | ⏸ 本机未安装 cargo-ndk / NDK（`D:\dev\android-sdk` 为空） |
| HarmonyOS 编译 | `cargo build --target aarch64-unknown-linux-ohos` | ⏸ 本机无 DevEco / hvigor / ohos target；ohos-rs 需 GitHub git 访问 |

测试覆盖的核心行为：参数序列化与默认值回落、滑块顺序与夹取、蒙版参数嵌套、
历史撤销/重做/跳转/分支、缓存统计与清除、`.fdxmp` 往返与 LUT 保留、
schema 版本拒绝、损坏归档拒绝、时间戳格式、pipeline 恒等与 +1EV、
FFI 生命周期 / 空指针错误 / 预设往返、JNI 与 N-API 导出对照表。

## 3. 性能基线（criterion，quick 模式）

| 基准 | 结果 | 说明 |
| --- | --- | --- |
| `params_to_json` | ≈ 3.33 µs | 全参数树 JSON 序列化 |
| `preset_to_bytes` | ≈ 230 µs | `.fdxmp`（ZIP/deflate）打包 |
| `reference_pixel` | ≈ 890 ps | CPU 曝光/对比度参考实现单像素 |

冷启动 / 首屏 / 实时预览指标属于阶段 1 交付范围。

## 4. 遗留问题与阶段 1 输入

1. **本机工具链**：无 Visual Studio / Windows SDK。当前使用 Rust GNU 宿主工具链 +
   本地 MinGW-w64（gcc 16.2 / dlltool / as），环境加载脚本为
   `packaging/windows/dev-env.ps1`。若后续安装 MSVC Build Tools，可直接切换回
   `x86_64-pc-windows-msvc`。
2. **rsraw（RAW）**：`rsraw-sys` 用 bindgen 生成 LibRaw 绑定，需要 libclang；
   本机用 PyPI libclang 18 + CPATH 指向 MinGW 头文件后，仍因 clang 与
   MinGW-w64 头文件的兼容性在 `stdlib.h` 处失败。阶段 1 开始前建议安装完整
   LLVM/Clang（例如 `winget install LLVM.LLVM`）或用 MSYS2 的 clang 包。
   `rsraw-sys` 明确不支持 MSVC 编译器，因此必须保留 MinGW/GNU 侧工具链。
3. **libheif（HEIC）**：需要 pkg-config 与 libheif（≥ 1.23，`libheif-sys` 要求），
   Windows 建议 vcpkg 或 MSYS2 安装后设置 `PKG_CONFIG_PATH`。
4. **Android**：`ANDROID_SDK_ROOT` 指向的 `D:\dev\android-sdk` 目前为空目录，
   且未安装 `cargo-ndk` 与 Rust Android 目标；阶段 5 前补齐即可。
5. **HarmonyOS**：本机无 DevEco / hvigor 与 ohos 目标；`napi-ohos` 未发布到
   crates.io 且本机无法 git clone GitHub（crates.io 正常），故暂不写入依赖，
   启用步骤见 `crates/ohos_bridge/README.md`。
6. **iOS**：按规格占位，不打包；阶段 5 提供 SwiftUI 工程与 C ABI 接入。
7. **打包器**：UI 与检测 / 日志 / 打包按钮已就绪；Android / HarmonyOS 的
   平台工程在阶段 5 生成后才能真正产出安装包（当前点击会提示脚本/环境缺失）。

## 5. 结论

阶段 0 目标达成：workspace、全部 crate、FFI 语义与四端桥接方案均已落地并通过
宿主机构建、33 项测试、clippy（-D warnings）与格式检查；GPU feature 编译通过，
RAW / HEIF / 移动端交叉编译的剩余工作已明确为环境依赖，不影响架构。
可以进入阶段 1（核心解码与 GPU 显示）。
