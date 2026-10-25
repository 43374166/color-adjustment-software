# 相机 Profile 骨架与代理实验

目标：在用户无法手工导出 Lightroom 参考时，先用 Sony RAW 内嵌 JPEG 做代理，验证相机矩阵/白平衡/profile 的代码路径。

## 1. 已实现的代码骨架

- `crates/color/src/camera.rs`
  - `CameraProfile { matrix, bias, curve }`
  - 线性 RGB 3×3 矩阵 + 偏置 + 每通道三次曲线
  - 非有限输入回退为黑，输出夹到 `[0,1]`
  - 身份 profile / 矩阵 / 曲线单元测试
- `fdxmp-cli render-cpu ... --camera-profile profile.json`
  - 在 RAW/JPEG 解码出线性代理后、进入色调/调色前应用 profile
  - 没有传 profile 时行为完全不变

JSON 格式：

```json
{
  "matrix": [[1,0,0],[0,1,0],[0,0,1]],
  "bias": [0,0,0],
  "curve": [[0,1,0,0],[0,1,0,0],[0,1,0,0]]
}
```

`curve` 系数顺序为 `c0 + c1*x + c2*x^2 + c3*x^3`。

代理拟合结果保存在 `profiles/sony-a7-camera-jpeg-proxy.json`，仅用于验证/实验，不作为默认 profile。

## 2. 代理拟合实验

工具：[`tools/fit_camera_profile.py`](../tools/fit_camera_profile.py)

- 10 组 `D:\Pictures\2026\05-17\*.ARW`
- 目标：RAW 内嵌 JPEG
- 输入：fdxmp CPU 默认 RAW 渲染
- 对齐：512 长边 + 相位相关平移 + 8 px 裁边；**相机内嵌 JPEG 需先做 90° 方向校正**（嵌入预览丢失 EXIF 方向）
- 模型：无偏置 3×3 矩阵（另测过 gain / affine / 三次曲线；带偏置和曲线会过拟合，反而变差）
- 8 张训练、2 张测试

结果（DSC09784，512 训练/测试统计）：

| 模型 | 训练前 | 训练后 | 测试前 | 测试后 |
|---|---:|---:|---:|---:|
| 无偏置 3×3 矩阵 | 12.88 | 12.51 | 12.54 | **11.99** |

2048 长边全量 10 张（矩阵在 8 张上训练）：

| 数据 | 默认平均 ΔE | 加 profile 平均 ΔE |
|---|---:|---:|
| 10 张 DSC097xx/099xx | **12.81** | **12.40** |

逐张看，9/10 有改善，1 张基本不变；改善约 3%。仍然不足以作为默认，但证明矩阵/profile 管线有效。

2048 长边单图（DSC09784）：

| 渲染 | 平均 ΔE2000 |
|---|---:|
| fdxmp RAW 默认 | **13.61** |
| 加矩阵 profile | **13.34** |

改善存在，但幅度很小（约 2%）；相机 JPEG 的局部对比度/锐化/场景色调不是全局矩阵能表达的。

原因：

- 相机内嵌 JPEG 有锐化、降噪、局部对比度和场景相关色调；
- 平移对齐仍不完美，镜头/裁切差异仍在；
- 全局 3×3 矩阵无法表达相机 JPEG 的场景相关色调；
- 单张训练容易过拟合，跨场景泛化差。

结论：**不把拟合 profile 设为默认**，不进入用户管线；它只作为 CameraProfile / DCP 管线的验证 profile。

## 3. 结论

- **不把拟合 profile 设为默认**，不进入用户管线；
- 代码骨架保留，后续可以接：
  - 真正的 DCP（相机矩阵 + HueSatMap + ToneCurve）；
  - Lightroom/Adobe Standard 参考；
  - 每个相机型号单独校准的 profile。
- 代理实验说明：**相机 JPEG 只能做粗调参考，不能替代 Lightroom/DCP 真值**。

## 4. 下一步

1. 有 Lightroom 参考后，按同样 JSON 格式生成 profile；
2. DCP 解析器把 `ColorMatrix` / `ForwardMatrix` / `ToneCurve` 映射到 `CameraProfile`；
3. 先用 identity profile 保证管线不退化，再接真实数据。
