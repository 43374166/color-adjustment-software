# 分析报告交付说明

本目录是《开源图像处理项目对比分析报告》的交付物，**未修改项目源代码**。

## 交付文件

| 文件 | 说明 |
| --- | --- |
| `fdxmp_开源图像处理项目对比分析报告.docx` | 可编辑的 Word 报告（31 页，含表格、雷达图、附录） |
| `fdxmp_开源图像处理项目对比分析报告.pdf` | 最终 PDF（由 DOCX 转换，31 页） |
| `assets/fig_radar.png` | 报告中的能力雷达图 |
| `assets/fig_scale.png` | 代码规模对比图（备用素材，未嵌入正文） |
| `assets/fig_timeline.png` | 三个项目的代际卡片图（备用素材，未嵌入正文） |
| `build_report.py` | 报告的 python-docx 生成脚本（可重新生成 DOCX） |
| `make_charts.py` | 图表生成脚本（纯 Pillow，无需 matplotlib） |

## 报告范围

- 本地项目：`D:\color-adjustment-software`（fdxmp v1.6.1，89 commits）
- LightCraft：`storytold/lightcraft`（v0.13.0，2026-10-23；数据截至 2026-10-25）
- darktable：`darktable-org/darktable`（稳定版 5.6.2；master 的 RELEASE_NOTES 已指向 5.8.0）

## 方法说明

- 对三个仓库做浅克隆/稀疏检出，统计文件、物理行、测试、模块数量；
- 阅读核心代码（fdxmp 的 `crates/core`、`crates/color`、FFI；LightCraft 的 `crates/pipeline`、`crates/gpu`、`crates/catalog`；darktable 的 `src/develop`、`src/iop`、`src/common/iop_order.c`）；
- 用 GitHub REST API 与仓库文档交叉验证版本、stars、issues、许可；
- **未重新编译 fdxmp**，报告中的性能数字凡来自项目文档均标注“[自述]”。

## 重新生成

```powershell
$py = "C:\Users\LicoLing\.dsh\dsh-runtimes\dsh-plugin-desktop-primary-runtime\dependencies\python\python.exe"
& $py .\reports\make_charts.py
& $py .\reports\build_report.py
```

PDF 转换使用 DSH 内置 LibreOffice Kit，命令见 `docs` 内的 office-export-pdf 说明。
