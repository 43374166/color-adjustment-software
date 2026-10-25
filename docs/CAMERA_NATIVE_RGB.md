# Camera-native RGB 实验（LibRaw C API）

目标：绕过 `rsraw` 封装的 sRGB 输出，直接让 LibRaw 输出相机原生 RGB，再应用 DCP。

## 做法

新增实验示例 `crates/core/examples/raw_camera_native.rs`：

- 直接调用 `rsraw-sys` / LibRaw C API；
- `params.output_color = 0`；
- `params.output_bps = 16`；
- `params.use_camera_wb = 1`；
- `params.use_camera_matrix = 0`；
- `no_auto_bright = 1`；
- 可选 `gamma = 1.0`。

```bash
cargo run -p fdxmp-core --example raw_camera_native --features raw -- \
  DSC09784.ARW raw-native.png
```

能正常输出 7028×4688、3 通道、16bit 图像。

## 结果

- 输出看起来仍接近“处理后的彩色图”，不是明显偏绿的相机原生 RGB；
- 与现有 sRGB 渲染平均 ΔE 约 **4.8**，不是原始相机 RGB 应有的差异；
- 把 Sony DCP 的 ForwardMatrix 转换矩阵套上去：
  - 默认 sRGB 基线约 **14.8**；
  - 应用 DCP 矩阵后 **24.3**，明显更差。

结论：`output_color=0 + use_camera_matrix=0` 仍没有拿到可正确套 DCP 的相机原生 RGB，或者 DCP 的矩阵方向/白平衡前置条件还不匹配。

## 下一步

1. 检查 LibRaw `rgb_cam` / `cam_xyz` / `cam_mul`，确认输出到底在哪一步被转换；
2. 尝试 **完全绕过 LibRaw 色彩管线**（raw mosaic + 自己的 demosaic），或修改 LibRaw 的 `output_color` 分支；
3. 只有在确认是相机 RGB 后，才继续 DCP 的 ForwardMatrix / ToneCurve / HueSatMap；
4. 不再基于 sRGB 输出硬套 DCP 矩阵。

## 进一步：直接读 `imgdata.image`

新增 `crates/core/examples/raw_camera_image.rs`：

- `libraw_unpack` 后调用 `libraw_raw2image`；
- 直接读 `imgdata.image`（4 通道 16bit）；
- 实测 `cam_mul = [2525,1024,1642,1024]`、`maximum=16383`、尺寸 7028×4688。

预览呈暗绿色，说明这次确实拿到了 **未做相机矩阵/白平衡的 RAW 域数据**。

**关键发现**：`imgdata.image` 仍是 **Bayer mosaic**（RGGB），不是已经插值好的 RGB。检查像素可见：

```text
[[R, 0, 0], [0, G, 0], ...]
[[0, 0, 0], [0, 0, B], ...]
```

所以之前直接把它当 RGB 套 DCP 的结果全部无效；必须先做 demosaic。

但直接应用 Sony DCP ForwardMatrix/ColorMatrix，ΔE 仍在 **18–40**，比现有 sRGB 基线的 **13.6** 更差。原因：

- 黑电平、白点、`cam_mul`、通道顺序还没有正确归一化；
- DCP 矩阵的前置色彩空间/方向仍未确认；
- `raw2image` 输出可能是 RAW 域，但还不是 DCP 期待的线性相机 RGB。

因此没有把 DCP 接入默认渲染。

## 尝试直接改 `rgb_cam`

在 `libraw_unpack` 后把 `color.rgb_cam` 设为单位矩阵，再 `dcraw_process`，希望 `convert_to_rgb_loop` 不再变换。实测：

- 进程结束后读回的 `rgb_cam` 仍是原始矩阵（`1.636, -0.448, ...`）；
- 输出与普通渲染接近；
- 用最小二乘拟合一个 3×3 矩阵，ΔE 只从 **15.07 → 14.62**，没有实质改善。

说明 LibRaw 在 `dcraw_process` 内部会重新使用/恢复相机矩阵，单改字段不可行。

## 新增 RGGB demosaic

[crates/core/src/demosaic.rs](crates/core/src/demosaic.rs) 增加 `demosaic_rggb`：

- 输入 `raw2image` 的 4 通道 mosaic；
- RGGB 模式，3×3 邻域双线性；
- 扣黑电平、按白点归一化；
- 3 个单元测试（常量色、黑白、空输入）。

已接进 `raw_camera_image` 示例；输出不再是 mosaic。但用 `pre_mul` + `cam_xyz` 试转 sRGB，ΔE 仍在 **22–40**，说明通道顺序、黑电平/白平衡或 `cam_xyz` 方向还没对，不能接 DCP。

## 下一步

1. 读 `color.black` / `color.cblack` / `color.maximum` / `pre_mul`，做正确的黑电平、白点和 WB 归一化；
2. 用 `cam_xyz` 而不是硬套 DCP，先验证 raw→XYZ；
3. 确认后再接 DCP ForwardMatrix/ToneCurve/HueSatMap。

当前没有把这条路径接进默认渲染；只是实验示例和诊断结论。
