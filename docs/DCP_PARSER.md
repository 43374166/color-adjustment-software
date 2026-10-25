# DCP 解析骨架

新增 `crates/core/src/dcp.rs`，从公开 DNG/DCP 规范实现的最小 TIFF/DCP 读取器，不复制 Adobe/darktable 代码。

## 支持字段

| 字段 | Tag | 类型 |
|---|---:|---|
| UniqueCameraModel | 50708 | ASCII |
| ProfileName | 50936 | ASCII |
| ColorMatrix1 / 2 | 50721 / 50722 | SRATIONAL[9] |
| ForwardMatrix1 / 2 | 50964 / 50965 | SRATIONAL[9] |
| CalibrationIlluminant1 / 2 | 50778 / 50779 | SHORT |
| ToneCurve | 50940 | SRATIONAL[2N] |

DCP 文件头是 `IIRC`（magic `0x4352`），不是普通 TIFF 的 42；解析器对两者都兼容。

## 验证

用本机 Adobe Camera Raw 的公开安装文件做了只读验证：

```bash
fdxmp-cli dcp-info \
  "C:\ProgramData\Adobe\CameraRaw\CameraProfiles\Adobe Standard\Sony ILCE-7M4 Adobe Standard.dcp"
```

输出：

```text
Adobe Standard | matrix1=true | forward1=true | tone=0
ColorMatrix1:
  [0.8784, -0.4791, 0.1177]
  [-0.3468, 1.0693, 0.3213]
  [0.0009, 0.0507, 0.7395]
ForwardMatrix1:
  [0.4743, 0.3796, 0.1104]
  [0.2023, 0.7673, 0.0304]
  [0.0553, 0.0008, 0.769]
ToneCurve points: 0
```

说明解析器能正确读取真实 Sony A7 IV DCP 的相机矩阵和 ForwardMatrix。

## 转成 CameraProfile

`DcpProfile::to_camera_profile()` 已实现：

- ForwardMatrix1 求逆（camera → XYZ D50）；
- Bradford D50 → D65；
- XYZ D65 → 线性 sRGB；
- 输出 `CameraProfile { matrix, bias=0, curve=identity }`。

`fdxmp-cli dcp-info` 会打印转换后的矩阵：

```text
CameraProfile matrix (camera -> linear sRGB):
  [9.774792, -6.941852, -1.766927]
  [-4.031116, 4.491216, 0.44466203]
  [0.08436191, -0.3420614, 1.8289611]
```

**重要**：这个矩阵只对**相机原生 RGB**有效。当前 LibRaw 输出已经是 sRGB，直接套用是错误的，所以没有接进默认渲染；它只是 DCP → CameraProfile 的管线验证。

## 尚未做

- **没有把 DCP 应用到渲染**：DCP 的 ColorMatrix 期望相机原生 RGB，而当前 LibRaw 输出已经是 sRGB；
- 需要先把 RAW 解码改为相机 RGB / 或在 LibRaw 输出上做近似映射，才能正确使用 DCP；
- HueSatMap / LookTable 还没解析；ToneCurve 在 Adobe Standard 里为空。

## 下一步

1. 实现 `DcpProfile::to_camera_profile()`：ForwardMatrix 求逆 + D50→D65 适应 + sRGB 矩阵；
2. 用相机型号自动选择 DCP；
3. 在 RAW 解码路径接入，保持 CPU/GPU 一致；
4. 用 Sony DCP 与相机 JPEG/Lightroom 参考对比。
