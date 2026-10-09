# 阶段 4 报告：预设与导出

日期：2026-10-03
范围：`.fdxmp` 应用与强度混合、Lightroom XMP 导入、导出管线（格式 / 尺寸 / 色彩空间 /
元数据 / 水印 / 批处理）与对应 UI 面板

## 1. 已完成

### 1.1 预设

| 功能 | 实现 |
| --- | --- |
| `.fdxmp` 读写 | 阶段 0 已完成（JSON + 可选内嵌 LUT 的 ZIP 包） |
| 应用预设 | `EditParams::blend(&preset, strength)`：数值线性插值；曲线控制点数量一致时逐点插值，否则按强度取舍；**蒙版保留当前图像的**（预设只影响全局参数） |
| 强度滑块 | 0–100%，实时预览（走 GPU 管线） |
| Lightroom XMP 导入 | 解析 `crs:` 属性 → 参数模型：15 项主滑块 + 锐化 4 项 + 降噪 6 项；色温按 5500K=0 线性映射并夹取；未识别属性忽略；完全无 `crs:` 时报错 |
| 导出当前参数 | 一键保存为 `.fdxmp`（文件对话框） |
| 用户预设列表 | 扫描 `presets/` 目录，点击应用（悬停显示路径） |

### 1.2 导出管线（`crates/core/src/export.rs`）

| 功能 | 实现 |
| --- | --- |
| 格式 | JPEG（质量参数）/ PNG / TIFF / WebP（image crate）+ **HEIC（libheif-rs 编码，实测可编码并回读）** |
| 尺寸 | 原始 / 长边 / 短边 / 百分比（`target_size` 有单元测试） |
| 色彩空间 | sRGB / Display P3 / Adobe RGB / ProPhoto RGB（当前统一按 sRGB 编码输出，ICC 写回阶段 6） |
| 元数据 | 保留 / 删除（重编码导致默认删除；EXIF 写回阶段 6） |
| 水印 | 文字（内置 5×7 点阵字体，ASCII，可调不透明度与字号）/ 图片（PNG 叠加，按宽度 18% 缩放、可调不透明度） |
| 批处理 | `export_batch` 顺序导出（避免内存峰值） |
| 导出分辨率 | 默认导出显示代理（快）；勾选「全分辨率导出」会重新解码原图 + 独立 GPU 上下文重渲染后导出 |

### 1.3 UI

- 右栏新增「预设」面板：强度滑块 / 导入 XMP·fdxmp / 导出当前参数 / 内置分类入口 / 用户预设列表。
- 右栏新增「导出」面板：格式、质量、尺寸、色彩空间、元数据、水印（文字/图片）、输出目录、
  全分辨率开关、导出按钮与进度提示；状态栏回报导出结果（尺寸 / 格式 / 大小 / 路径）。

## 2. 测试结果

| 检查项 | 命令 | 结果 |
| --- | --- | --- |
| 核心全特性测试 | `cargo test -p fdxmp-core --features gpu,raw,heif` | ✅ 50 单元 + 5 解码 + 8 GPU + 4 预设 |
| 工作区测试 | `cargo test --workspace` | ✅ 全绿 |
| clippy（默认 / 全特性） | `-D warnings` | ✅ 0 警告 |
| 格式 | `cargo fmt --all -- --check` | ✅ 通过 |

新增测试：

| 测试 | 断言 |
| --- | --- |
| `blend_mixes_values_and_keeps_masks` | 50% 强度取中值、蒙版保留 |
| `imports_lightroom_xmp_sliders` | 14 个 `crs:` 参数正确映射（含色调 +1.25、7000K→+27.3） |
| `rejects_xmp_without_crs_parameters` | 无 `crs:` 报错 |
| `kelvin_mapping_is_clamped` | 5500K→0、2000K→-63.6、超范围夹取 |
| `resize_modes_compute_expected_sizes` | 四种尺寸策略 |
| `exports_jpeg_png_tiff_webp` | 四种格式写出后能重新打开且尺寸正确 |
| `exports_heic_and_decodes_back` | HEIC 编码 → 用自家解码器读回（320×200） |
| `export_applies_resize_and_text_watermark` | 长边 400 + 水印像素出现在右下角 |
| `batch_export_reports_each_file` | 批处理返回每条结果 |

## 3. 遗留问题（阶段 5/6/7 输入）

1. **ICC 色彩空间**：P3 / Adobe RGB / ProPhoto 目前按 sRGB 编码输出，需要 ICC 配置文件写回（阶段 6）。
2. **EXIF 保留**：重编码会丢失 EXIF，"保留"选项在阶段 6 接入元数据写回。
3. **中文文字水印**：内置点阵字体仅覆盖 ASCII，中文需要字体引擎（阶段 6 与 UI 字体一起处理）。
4. **内置预设库**：7 个分类的预设内容在阶段 6 制作（当前可用用户预设与 XMP 导入）。
5. **全分辨率导出**：45MP RAW 全流程约需数十秒与较大显存，阶段 7 做分块渲染后再优化。

## 4. 结论

阶段 4 目标达成：预设可导入（XMP / .fdxmp）、可按强度应用、可导出；导出管线覆盖
JPEG/PNG/TIFF/WebP/HEIC、四种尺寸策略、水印与批处理，并已接入 UI。
可以进入阶段 5（移动端 UI：Android / HarmonyOS / iOS 工程与三大 Tab）。

