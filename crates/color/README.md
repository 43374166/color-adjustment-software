# fdxmp-color

纯 Rust 色彩科学核心：色彩空间 / 白平衡（色温·色调）/ 感知空间（OkLab·OkLCh）/
曲线（单调样条 + LUT）/ 8 色带混色 / 点颜色 / 颜色分级 / 相机校准 / 黑白 /
胶片式色调映射 / 色域映射。

- 无平台依赖、无第三方依赖、`#![forbid(unsafe_code)]`：桌面（Windows/egui）、
  iOS、Android、HarmonyOS 以及未来的 WASM 共用同一份数学。
- 与渲染无关：调用方（`fdxmp-core`）负责解码、GPU 上传、参数序列化；
  本 crate 只提供"给定线性 RGB 与参数，输出线性 RGB"的纯函数。

## 来源与许可

本 crate 是 MIT（随 fdxmp 仓库）。算法来源分三类：

1. **公开标准与论文**（可直接实现）：
   - IEC 61966-2-1（sRGB）、SMPTE ST 2084（PQ）、ITU-R BT.2100（HLG）；
   - CIE 色度学与 Bradford 色适应；
   - Kim et al. 2002 的普朗克轨迹三次近似（`cct.rs`）；
   - Björn Ottosson 的 OkLab / OkLCh（`perceptual.rs`）；
   - Fritsch–Carlson 单调三次插值（`spline.rs`）。

2. **LightCraft（storytold/lightcraft，MIT OR Apache-2.0）**：
   调色管线的结构与参数语义（自然饱和度的低饱和加权与肤色保护、8 色带的分割权重、
   点颜色的软范围与 variance、三分级的 blending/balance 掩码、相机校准的基色矩阵构造、
   场景参考色调映射的对数域对比度与扩展 Reinhard 肩部）改编自其
   `crates/color`、`crates/pipeline/src/{colorops,tone,finish}.rs`。
   按 MIT 条款吸收；本仓库保留其版权声明（见 `NOTICE`）。

3. **darktable（GPLv3）**：**没有复制任何代码**。仅作为算法方向的参考
   （其在"感知空间调色""显示变换""色域压缩"上的模块划分），
   重写计划与来源见 `docs/COLOR_ENGINE_PLAN.md`。

## 测试

```powershell
cargo test -p fdxmp-color
```
