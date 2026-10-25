# Lightroom 2026 对照基线（阶段 0）

> 目标不是“感觉像”，而是每轮都能回答：哪类区域差多少、为什么差、修完有没有变好。

## 1. 样片与工具

样片位置：

- RAW：`D:\Pictures\2026\05-17\*.ARW`（63 张，Sony A7 系列）
- 已处理 JPEG：`D:\Pictures\2026\05-17\PS\*.jpg`（10 张）
- 更多 JPEG：`D:\PS\<日期>\`（已在 Lightroom catalog 中，3379 张）

工具：

- `fdxmp-cli render-cpu <in> <out.png> [--exposure ...]`：无 GPU 的 CPU 预览路径，2048 长边，便于跨机器复现。
- `fdxmp-cli preview <raw> <out.jpg>`：提取 RAW 内嵌 JPEG 预览（相机色彩基线）。
- `tools/compare_lightroom.py REF TEST --out report.json --heatmap heat.png`：CIEDE2000、全图/阴影/中间调/高光/肤色/高饱和分区统计。

示例：

```bash
fdxmp-cli preview DSC09784.ARW camera-preview.jpg
fdxmp-cli render-cpu DSC09784.ARW fdxmp-default.png
python tools/compare_lightroom.py camera-preview.jpg fdxmp-default.png \
  --out report.json --heatmap heat.png
```

## 2. 当前基线

### 2.1 18 张 JPEG：LR 默认导出 vs fdxmp CPU 默认

样片：`D:\PS\2023-9-10\` 的 18 张 JPEG；Lightroom 2026 导出（默认设置，JPEG 100，sRGB），fdxmp `render-cpu` 到 2048 长边，双方都缩到 2048 后比较。

| 指标 | 平均 ΔE2000 |
|---|---:|
| 全图 mean | **0.65** |
| 全图中位 | **0.56** |
| 全图 P95 | **1.63** |
| 阴影 | 0.82 |
| 中间调 | 0.66 |
| 高光 | 0.77 |
| 肤色 | 0.91 |
| 高饱和 | 0.73 |

结论：**JPEG 默认路径已经非常接近 Lightroom**（mean <1 ΔE，P95 <2）。这条链路说明输入/显示变换/默认曲线没有明显系统性偏色；真正的大差距在 **RAW 相机色彩**和**调色参数**，不是 JPEG 解码。

复现：

```bash
# 18 张源 JPEG 在 D:\PS\2023-9-10
# Lightroom 导出在 D:\PS\2026-03-22\LR（默认设置）
fdxmp-cli render-cpu <source.jpg> <stem>.png
python tools/batch_compare.py <lr-dir> <fdxmp-dir> <report-dir> --max-edge 2048
```

### 2.2 RAW：相机内嵌 JPEG 基线

| 区域 | 平均 ΔE2000 | 中位 | P95 |
|---|---:|---:|---:|
| 全图 | 13.61 | 9.65 | 40.36 |
| 阴影 | 8.34 | 4.37 | 29.93 |
| 中间调 | 15.42 | 11.64 | 40.89 |
| 高光 | 15.75 | 10.76 | 51.03 |
| 肤色 | 13.19 | 9.99 | 32.73 |
| 高饱和 | 16.82 | 12.73 | 42.82 |

解读：

- 当前默认 RAW 渲染与相机 JPEG 的差距已经很大，主要不是“滑块端点”，而是**相机色彩/色调曲线**。
- 中间调、高光、高饱和区最差；肤色也明显偏色。
- 这组数据只是“相机 JPEG 基线”，不是 Lightroom 基线；Lightroom 使用 Adobe Standard/Neutral，目标会不同，但错误聚集区通常一致。

### 2.3 调整参数基线（准备中）

用 catalog 副本 + SQLite 写入 develop 设置，可以批量生成“同参数 Lightroom 参考”：

```bash
python tools/lightroom_catalog_settings.py \
  ".tmp/lr-catalog/Lightroom Catalog.lrcat" \
  --folder "2023-9-10/" \
  --set Exposure2012=1 --set Contrast2012=50 \
  --set Highlights2012=-50 --set Shadows2012=50 \
  --set Whites2012=20 --set Blacks2012=-20 \
  --set Saturation=50 --set Vibrance=20
```

只在 catalog 副本上运行，绝不要对真实 catalog 执行。

**实测警告**：直接改 `Adobe_imageDevelopSettings.text` 并保持 `settingsID='default'` 时，Lightroom 2026 导出仍按默认设置渲染（导出的 18 张与默认基线逐像素相同，ΔE=0）。说明这条路径不能替代 Lightroom 自己的 develop 写入。下一步必须用 **Lightroom 预设 / UI 调参 / 正常插件 API** 写入，再导出。

## 3. Lightroom 参考导出

已跑通 **Lightroom 原生导出**（不依赖插件）：

- 在 Lightroom catalog 副本中选中 `D:\PS\2023-9-10\` 的 18 张 JPEG；
- `Ctrl+Shift+E` 打开导出对话框；
- JPEG 100、sRGB、默认分辨率；
- 导出到 `D:\PS\2026-03-22\LR\`，再复制到 `.tmp/lr-reference/` 做对照。

注意：

- 使用 catalog 副本，未修改真实 catalog；
- 这 18 张的 develop 设置全部是默认值（曝光/对比度/饱和度等为 0，As Shot），因此是真正的“Lightroom 默认渲染”基线；
- 下一次做参数对照时，需要在副本里先改 develop 设置，再导出到独立目录。

插件路线（`LrInitPlugin` / Plug-in Manager）仍保留，但当前不作为主路径；原生导出已经够用。

## 4. 第一轮迭代顺序

1. 用 10 张 RAW 建基线，按“肤色 / 高光 / 绿色 / 蓝色 / 高饱和”聚类误差；
2. 先修相机色彩（as-shot WB / 相机矩阵 / DCP），不要先调滑块；
3. 再修高光滚降与局部色调；
4. 每轮固定跑 `color_extremes`、`gpu_pipeline`，防止端点/CPU-GPU 回退。

## 5. 备注

- 相机 JPEG 不是 Lightroom；只作为“相机厂商色彩”的近似参考。
- 所有对比必须同尺寸、同裁切；Lightroom 导出时关闭锐化、降噪、镜头校正。
- 不复制 Adobe DCP / 预设，不复制 darktable GPL 代码。
