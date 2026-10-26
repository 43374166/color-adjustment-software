# HSL 面板重做（JPEG 路径）

## 问题

旧 HSL 混色直接套用 LightCraft 的 OklCh 分区：

- 用 **OklCh 色相**选带，但 Lightroom 的 HSL 面板按**显示 HSL 色相**选带；
- 相邻两带硬分割，红带因为红→橙只有 30°、红←洋红有 45°，左右影响范围不对称；
- 色相/饱和度/亮度都在 OklCh 中改，和 Lightroom 的 H/S/L 语义不一致。

结果就是：滑块能“动”，但目标颜色不对、过渡不对称、和 Lightroom 差距明显。

## 修改

### 按像素 HSL 色相选带

`crates/color/src/bands.rs`：

- `band_weights_rgb(rgb)`：线性 RGB → sRGB 编码 → HSL 色相 → 选带；
- 带心仍是 Lightroom 的 `0/30/60/120/180/225/270/315°`；
- 8 个升余弦窗（半宽 = 相邻中心距离的 0.65 倍）叠加后归一化：
  - 带心仍是本带主导；
  - 带间连续重叠；
  - 红带在 0° 左右对称。

### HSL 原生调整

`ColorOps::apply_hsl_mixer`：

- 在 HSL 中直接改 H/S/L；
- 色相 ±100 约 ±30°；
- 饱和度 ±100 为 ×0 / ×2；
- 明亮度 ±100 约 ±0.22 HSL L；
- 之后仍用 OkLCh 做自然饱和度、点颜色、分级。

GPU `color_pipeline.wgsl` 同步实现：

- 新增 `rgb_to_hsl` / `hsl_to_rgb`；
- `pipeline_colour_ops` 先做 HSL 混色，再进入 OkLCh 点颜色/自然饱和度/分级；
- `pack_color_slots` 的 8 个带心改为 HSL 弧度。

## 验证

- `fdxmp-color` 50 个测试通过；
- 新增：
  - 红饱和度 -100 只去红、不动蓝；
  - 红色相 +100 落到 20–45°（橙方向）；
  - 明亮度 ±100 单调、不反相；
  - 中性灰不受 HSL 影响；
  - 8 个纯色命中各自 HSL 带；
  - 红带 0° 左右对称。
- GPU 32 个测试通过（含 HSL 分区/CPU 参考对齐）；
- Lightroom JPEG 18 张基线不变：mean ΔE **0.653**、中位 0.557、P95 1.633。

## 还没做

- 没有 Lightroom HSL 导出参考图，当前只能保证 HSL 语义/分区/端点正确，不能量化“和 Lightroom 每个滑块完全一致”；
- 色相行程/亮度行程是经验值，后续有 Lightroom HSL 参考图后可以逐滑块校准。
