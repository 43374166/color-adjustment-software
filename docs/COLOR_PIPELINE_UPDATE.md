# 色彩管线更新（2026-10-25）

## 已完成

### 1. 高光滚降/去饱和

`ColorStage::map_display_gamut` 在 OkLCh 色域映射前先做高光去饱和：

- 只对 `L > 0.85` 且 `C > 0.08` 的饱和高光生效；
- `L` 在 0.85..1.0 间用 smoothstep，彩度最多降低 35%；
- 保持 `L/h` 不变，只降低 `C`；
- GPU `highlight_desaturate` 与 CPU 同算法。

### 2. OkLCh 软色域过渡

`map_to_gamut_oklch` 在硬边界前加 soft knee：

- 在最大彩度的 85% 开始压缩；
- 使用 `f(t)=t-0.15*t*(1-t)` 平滑过渡；
- 超出边界仍回落到最大彩度；
- CPU/GPU 同算法。

### 3. Lightroom JPEG 基线

18 张 `D:\PS\2023-9-10\` JPEG，Lightroom 默认导出 vs fdxmp：

| 指标 | 修改前 | 修改后 |
|---|---:|---:|
| 全图 mean ΔE | 0.65 | **0.653** |
| 中位 | 0.56 | 0.557 |
| P95 | 1.63 | 1.633 |
| 高光 | 0.77 | 0.773 |
| 肤色 | 0.91 | 0.912 |

结论：高光去饱和/软色域没有破坏默认 JPEG 基线；只有极端高饱和高光才变化。

## 未完成：linear Rec.2020 工作空间

**没有切换成功。** 当前 `WORKSPACE` 仍是线性 sRGB。

原因：全管线切 Rec.2020 不只是改常量，需要同时改：

- 解码 sRGB → Rec.2020；
- 白平衡/校准/色调/Oklab 的 Rec.2020 矩阵；
- 曲线域（显示参考空间）；
- GPU WGSL 的 Oklab/luminance 常量；
- 显示/导出 Rec.2020 → sRGB；
- 所有 golden/对比测试重跑。

这是一个独立的、跨 CPU/GPU 的管线改造，不能只改 `WORKSPACE` 常量，否则 CPU/GPU 和 Lightroom 基线会同时失真。

## 当前状态

- 代码已通过 workspace 测试、GPU 32、shader validation、clippy；
- 默认 JPEG 基线保持 0.65；
- 改动待 commit / push。
