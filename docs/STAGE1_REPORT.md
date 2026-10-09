# 阶段 1 报告：核心解码与 GPU 显示

日期：2026-10-02
范围：RAW + 常见格式统一解码、wgpu 显示、首屏性能验证

## 1. 已完成

### 1.1 统一解码管线（`crates/core/src/decode.rs`）

```rust
decode(path)                        // 完整质量（RAW 全尺寸 16bit 解调）
decode_preview(path)                // 首屏快速路径（RAW 内嵌预览 + 半尺寸）
decode_with(path, &DecodeOptions)   // 自定义策略
```

- **常见格式**：JPEG / PNG / TIFF / WebP / BMP / GIF（image crate）。
- **RAW**：rsraw（LibRaw 封装）
  - 内嵌 JPEG 预览提取（取最大的一张）→ 首屏路径；
  - 16bit 全尺寸 / 半尺寸解调，相机白平衡开关；
  - 元数据：厂商、机型、ISO、快门、光圈、焦距、拍摄时间（RFC 3339）；
  - `probe()` 通过 LibRaw 头解析直接给出 RAW 尺寸（不解调）。
- **HEIF**：libheif-rs 解码路径已实现（主图像 → 交错 RGB → `DynamicImage`），
  需要本地 libheif ≥ 1.23 才能编译（本机尚未安装，见 §5）。
- **显示代理**：`DecodedImage::display_proxy(max_edge)` 生成 RGBA8 代理图
  （45MP 原图直接上传要 ~180MB 显存，桌面先看 2048px 代理）。
- **直方图**：`crates/core/src/histogram.rs`，RGB + 亮度 256 bin，
  大图按步长抽样（上限 200 万像素）。

### 1.2 GPU 显示（Windows UI）

- eframe 0.36 默认 **wgpu 后端**（本机实测 `GeForce GTX 1050 Ti · Vulkan`），
  解码结果上传为 egui 纹理 → GPU 渲染到画布，状态栏显示后端信息。
- 画布交互：适应窗口 / 滚轮缩放（0.05×–8×）/ 拖拽平移。
- 解码在后台线程执行，UI 不阻塞（解码中显示 spinner，完成后自动切换）。
- 支持「用 fdxmp 打开」：`fdxmp-win-ui.exe <照片路径>` 直接载入。
- 右栏「直方图」接入真实数据，并显示尺寸 / 解码来源 / 耗时 / RAW 元数据。

### 1.3 工程与工具链

- rsraw 的 bindgen 构建打通：`LIBCLANG_PATH` + `CPATH`（MinGW 头文件）+
  `BINDGEN_EXTRA_CLANG_ARGS=--target=x86_64-w64-windows-gnu`，已写入
  `packaging/windows/dev-env.ps1`（阶段 0 遗留问题 2 解决）。
- 打包脚本 `packaging/windows/build.ps1` 改为 `--features raw` 构建，并复制
  MinGW 运行时 DLL（`libstdc++-6` / `libgcc_s_seh-1` / `libwinpthread-1`）。
- 新增 criterion 基准 `decode_bench`（24MP 常见格式 + RAW 首屏/全尺寸）。

## 2. 测试结果

样本：Nikon Z8 NEF（48.7MB，8280×5520，45.7MP，来自 rsraw 官方测试资产）。

| 检查项 | 命令 | 结果 |
| --- | --- | --- |
| 全量测试（默认 feature） | `cargo test --workspace` | ✅ 通过 |
| 核心 + RAW | `cargo test -p fdxmp-core --features raw` | ✅ 28 单元 + 4 集成 |
| RAW 集成测试 | `decodes_raw_preview_and_full` | ✅ 预览 8256×5504（内嵌预览）、全尺寸 8280×5520 16bit |
| 元数据 | 同上 | ✅ `Nikon Z 8 · ISO 250 · 1/100s · f/3.5 · 105mm` + 拍摄时间 |
| 静态检查 | `cargo clippy --workspace --all-targets -- -D warnings`（默认 + `raw`） | ✅ 0 警告 |
| 格式 | `cargo fmt --all -- --check` | ✅ 通过 |
| CLI 冒烟 | `fdxmp-cli probe / decode --preview / decode` | ✅ 见 §3 |

## 3. 性能（release，criterion quick 模式）

| 基准 | 结果 | 对应目标 |
| --- | --- | --- |
| 24MP JPEG 解码 | **110 ms** | — |
| 45.7MP RAW 首屏（内嵌 JPEG 预览） | **218 ms** | 24MP RAW 首屏 < 2s ✅（余量约 9×） |
| 45.7MP RAW 全尺寸 16bit 解调 | **4.92 s** | 首屏之后后台/渐进式处理 |

CLI 实测（release）：

```text
probe  test-z8.NEF            → raw, 8280×5520
decode test-z8.NEF --preview  → 内嵌预览 8256×5504（45.4MP），210 ms
decode test-z8.NEF            → RAW 全尺寸 8280×5520（45.7MP, Rgb16），5110 ms
```

## 4. 界面

![阶段 1：Windows 界面载入 45.7MP RAW](screenshots/stage1-win-ui-raw.png)

画布显示 Z8 RAW 的内嵌预览（显示代理 2048×1365），右栏直方图与元数据实时更新，
状态栏给出解码来源与耗时；渲染后端 `GeForce GTX 1050 Ti · Vulkan`。

## 5. 遗留问题（阶段 2 输入）

### 5.1 本轮新增解决（2026-10-03）

1. **HEIF 已接入并验证**：本机安装 libheif 1.23.5（MSYS2 包：libheif / libde265 /
   aom / dav1d / kvazaar / x265 / svt-av1 / rav1e / openh264 / openjpeg2 / openjph /
   libjpeg-turbo / libwebp(含 libsharpyuv) / libtiff / crypto++ / lerc / libdeflate /
   jbigkit / xz / zlib + pkgconf），`pkg-config --modversion libheif` = 1.23.5。
   用 `heif-enc` 生成真实 HEIC 样张，解码实测 1600×1067 / 56ms（debug），
   `probe()` 也能给出 HEIF 尺寸；集成测试 `decodes_heic_sample` 通过。
2. **全尺寸后台渐进渲染**：首屏（内嵌预览）先出图，随后后台线程跑 LibRaw 全尺寸
   解调，完成后自动替换画布并更新直方图 / 元数据；状态栏依次显示
   「首屏完成 …· 后台生成全尺寸…」→「全尺寸就绪 …（RAW 全尺寸，22320 ms）」。
3. **批量导入 + 真实缩略图 + 磁盘缓存**：导入 / 拖入多张时用 rayon 并行生成
   240px 缩略图（RAW 走内嵌预览），写入 `缓存目录/thumbnails/<hash>.png`
   （key = FNV-1a(路径+大小+修改时间)，源文件变化自动失效），胶片条显示真实缩略图。
4. **运行时 DLL 自动收集**：`packaging/windows/build.ps1` 现在按 exe 导入表
   递归收集 MinGW/C++ 运行时 DLL 到 `dist/`，不再手工维护清单。

### 5.2 仍留在后续阶段

1. **1:1 像素级缩放**：当前画布使用 2048px 代理图，放大超过 100% 会软；
   全分辨率分块渲染安排在阶段 6。
2. **直方图为 CPU 抽样**：阶段 2 改为 wgpu compute 统计。
3. **debug 构建带控制台窗口**：release 构建隐藏（阶段 6 打包时处理）。
4. **缩略图缓存目录**：使用默认 `%LOCALAPPDATA%\fdxmp\cache`，与「我的 → 清除缓存」
   共用（阶段 5 在移动端暴露入口）。

## 6. 结论

阶段 1 目标达成：RAW 与常见格式统一解码可用，45.7MP RAW 首屏 218ms
（远优于 24MP < 2s 目标），图像经 wgpu 后端显示，直方图与元数据联动，
测试与静态检查全绿。可以进入阶段 2（色彩管线：15 项调色 + 曲线 + HSL +
颜色分级 + 细节，GPU compute 实现）。
