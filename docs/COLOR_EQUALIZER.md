# 颜色均衡器（color equalizer）实现说明

参照 darktable 的 color equalizer 行为，在 `crates/color` 内以纯 Rust 独立实现。
**不含 darktable 源码**：只按公开的 UCS 2022 定义、导向滤波论文与周期 RBF 插值的
公开文章重写公式；darktable 为 GPLv3，本仓库为 MIT，两者不可混用代码。

## HSL 面板（已并入同一引擎）

「HSL / 颜色」只剩 Lightroom 8 段，内部换成均衡器那套（dt UCS + 周期曲线），
「连续色域」模式已删除（要连续曲线请用颜色均衡器）。三点关键差异：

1. **节点位置**用 Lightroom 的 0/30/60/120/180/225/270/315，不是等距 45°。
2. 这些节点**疏密不均**，周期 RBF 会严重过冲（实测 6° 冲到 36.8°），所以 HSL 用
   **周期分段线性**插值（与 darktable 的 color zones 一致）；等距的均衡器仍用 RBF。
3. **选带坐标是 sRGB HSL 色相**（Lightroom 语义），修正量在 UCS 里施加；
   两种色相空间只有红色经 20° 偏移能对齐，用 UCS 选带会让蓝色带偏到青/紫上。

行程沿用旧面板：色相 ±30°×`HSL_HUE_SCALE`、饱和 +100≈×1.6 / −100→×0、亮度 ±12%。
GPU 与 CPU 逐像素一致（实测平均 0.014、最大 1 个 8bit LSB）。

已知差异：品红带（315°）落在 315→360 的收尾区间，响应比 Lightroom 的重叠窗弱；
亮度是 UCS 亮度增益，不像旧的 HSL 明度那样自动伴随去饱和。

## 模块

| 文件 | 内容 |
| --- | --- |
| `crates/color/src/ucs.rs` | darktable UCS 2022：`L*`、`U'V'`、JCH、HSB 双向变换；色域边界 LUT 与 `map_hsb` 软裁剪 |
| `crates/color/src/guided.rs` | 可分高斯模糊、Scharr 梯度、2 通道导向滤波、按行多线程调度 |
| `crates/color/src/equalizer.rs` | 参数、8 节点周期 RBF、三张 512 点色相 LUT、整图算子 |

## 数据流

```
线性 RGB ─┬─→ XYZ D65 → xy →(U'V', L*)→ JCH → HSB ─→ 查 LUT ─┐
          └─→ (max-min)/max → 高斯模糊 ─────────────┐        │
                                                    ↓        ↓
                                     logistic 低饱和权重 + 导向滤波（可选）
                                                             ↓
                      H += ΔH, S ×(1+2(g_s-1)), B ×(1+8·ΔB) → 色域软裁剪 → RGB
```

- 色相域是 **dt UCS 的极角**，不是 HSV/HSL；sRGB 红落在 20°，节点再加 `hue_shift`。
- 8 个节点（红橙黄绿青蓝紫品红）经周期 RBF 插值摊到整圈：核
  `k(l)=exp(-l²/s)`，特征 `φ(Δ)=exp(Σ k(l)cos(lΔ))`，解 `AᵀA λ = Aᵀy` 后展开 512 点 LUT。
- 低饱和像素用 logistic 权重 `1/(1+exp(factor·(s-threshold)/2))` 衰减修正，
  `threshold` 默认 0.1 → 实际拐点 6% 饱和度；`contrast` 控制软硬。
- 两个导向滤波都以 `U'V'` 为导向图：一个平滑输入色度，一个平滑修正量；
  后者对亮度通道用 `0.1σ` 求均值（参考实现的做法，保持亮度修正局部）。
- Scharr 梯度（阈值 0.02、平方、再模糊）作为亮度修正的边缘闸门，抑制光晕。
- 输出前 `Gamut::map_hsb` 按色域边界软裁剪饱和度，避免推出色域产生荧光色。

## 参数映射（与参考实现一致）

| 参数 | 作用 |
| --- | --- |
| `threshold` | `sat_shift = -0.015 + 0.3·(5t)²` |
| `contrast` | logistic 斜率 `factor = -60 - 40c` |
| `white_level` | `L_white = L*(2^white_level)`，默认 1.0 ≈ 0.988 |
| `chroma_size` | 色度导向滤波 `σ = 0.5·chroma_size · scale` |
| `param_size` | 修正量导向滤波 `σ = 0.5·param_size · scale` |
| `use_filter` | 关掉两个导向滤波（快约 40%，彩噪/光晕回来） |
| `saturation[i]` / `brightness[i]` | 增益（1.0 不变），视觉放大 2× / 8× |
| `hue[i]` | 色相偏移（度），加性 |
| `hue_shift` | 8 个节点整体旋转（度） |

## 用法

```rust
use fdxmp_color::{ColorEqualizer, EqualizerParams, SRGB};

let mut p = EqualizerParams::default();
p.saturation[0] = 1.3;   // 红更饱和
p.hue[3] = 8.0;          // 绿往青偏 8°
p.brightness[2] = 1.1;   // 黄提亮 10%

let eq = ColorEqualizer::new(p, &SRGB);       // 烘焙 LUT（微秒级）
let mut rgb = vec![0.2f32; w * h * 3];
eq.apply(&mut rgb, w, h, 3);                  // 3 或 4 通道都行
```

- 预览用小图：`apply_scaled(&mut rgb, w, h, 3, preview_w as f32 / full_w as f32)`，
  半径会按比例缩小。
- 参数默认时 `apply` 直接返回，不改动像素（严格恒等）。
- 只换参数：`eq.set_params(&p)`；换工作空间：`eq.set_space(&space)`。

## 性能（单机实测，1920×1080，release，默认参数）

| 模式 | 耗时 |
| --- | --- |
| `use_filter = true` | ≈ 0.42 s |
| `use_filter = false` | ≈ 0.25 s |

33 MP（4672×7008）实测 5.7 s，与像素数基本线性。
行级并行（`std::thread`，最多 16 线程；wasm 自动退化为单线程），
无第三方依赖，保持 `#![forbid(unsafe_code)]`。缓存按像素分配约 24 B/px，
4K 图约 200 MB；更大图建议分块调用（上下左右各留 16 + max_radius 像素重叠）。

## 与参考实现的可能差异

- 高斯用的是 3σ 截断的可分卷积，参考实现是 O(1) 递归高斯；σ 相同时视觉等价，
  逐像素不保证 bit 级一致。
- 双线性重采样用像素中心对齐，参考实现另有实现细节；只在 `σ > 2.5` 触发降采样时可见。
- 未实现 GUI 侧的曲线绘制、蒙版可视化与 OpenCL 内核。

## 接入 fdxmp（已完成 CPU + GPU 两条路径）

| 层 | 状态 | 位置 |
| --- | --- | --- |
| 参数序列化 | ✅ | `crates/core/src/params.rs` → `EqualizerParams`（`EditParams.equalizer`，`#[serde(default)]`，旧预设自动回落为关闭） |
| 引擎适配 | ✅ | `crates/core/src/color_engine.rs` → `EqualizerStage`（默认关闭时 `is_identity()`，零开销） |
| CPU 预览 | ✅ | `crates/core/src/preview.rs`，在色调映射之前对整帧线性基底跑一遍 |
| GPU（wgpu） | ✅ | `shaders/color_pipeline.wgsl` 的 `eq_apply()`，在 `cs_main` 里接在 `pipeline_base` 之后、`pipeline_tone` 之前；三张 LUT + 色域表由 `pack_equalizer()` 写进 uniform |
| 单元测试 | ✅ | 恒等、色相选带、JSON 往返、缺字段回落、GPU 管线编译、GPU vs CPU 逐像素 |
| UI 面板 | ✅ | `crates/win_ui/src/main.rs` → `equalizer_panel` / `eq_graph`：编辑侧栏「颜色均衡器」，dt 风格曲线图（三页签 + 彩色背景 + 8 个可拖动节点 + 底部色相条）+「选项」+ 重置 |

### GPU 版与 CPU 版的实测差异

在 24×24 色相扫描图上逐像素比对（8bit sRGB，`cargo test --features gpu -- --ignored`）：

| 模式 | 平均差 | 最大差 |
| --- | --- | --- |
| `use_filter = false`（默认） | **0.042** | **1** |
| `use_filter = true` | 2.96 | 18 |

- 关滤波时 GPU 与 CPU 是同一套逐像素公式，差异只来自 f16 存储与 8bit 量化。
- 开滤波时 GPU 用单遍 5×5 窗口近似（CPU 是分离高斯 + 两次导向滤波），
  在强色度边缘处最多差 ~2%——所以**默认 `use_filter = false`**，
  保证预览 / 导出 / CPU 参考三边一致；要彩噪抑制可在面板里打开。
- 后续补真正的多 pass 导向滤波时，把 `eq_apply` 里的 5×5 换成 4 个 pass 即可，
  参数与 LUT 通道不用动。

注意事项：

- 引擎用的工作空间是 `WORKSPACE = 线性 sRGB`；对拍时用的是 darktable 的 linear Rec2020。
  同一颜色在两者中 UCS 坐标相同，只有超出 sRGB 色域时色域边界表不同 → 结果会有细微差异。
- `enabled = false` 是默认值：升级后已有预设/照片不会因为多了这个字段而改变。
- uniform 里新增 2056 个 float（8 全局 + 4×512 表），参数缓冲从 21.3 KB 增到 29.5 KB，
  仍在 64 KB 的 `max_uniform_buffer_binding_size` 之内。

## 与 darktable 5.6.2 对拍（已完成）

`crates/color/examples/equalize_raw.rs` 是专门的对拍工具：吃裸 `f32` 线性 RGB，
按参数文件跑一遍，再把结果写回裸 `f32`，这样 Python 只负责图片解码/编码，
算法侧不引入任何图像依赖。

```text
cargo run --release -p fdxmp-color --example equalize_raw -- \
    src_lin.raw out_lin.raw 4672 7008 3 params.txt
```

`params.txt`：

```text
space=rec2020          # 与 darktable 默认工作空间一致
hue=0,0,-60,0,0,0,0,0  # 黄 -60°
```

### 结果（DSC09472，7008×4672，darktable 5.6.2 导出的 16-bit sRGB TIFF）

| 指标 | 值 |
| --- | --- |
| 平均绝对误差 | **0.15 / 255**（0.06%） |
| p99 | 0.96 / 255 |
| 最大 | 6.3 / 255（只在极少数强边缘像素） |
| ≤ 1 个 8-bit LSB | **99.14%** |
| ≤ 2 个 8-bit LSB | 99.92% |
| 分通道均值误差 | R 0.17 / G 0.06 / B 0.22 |

对拍流程（可复现）：

1. `darktable-cli <jpg> <xmp> out.tif --core --configdir <临时目录>
   --conf plugins/imageio/format/tiff/bpp=16`
   （`--configdir` 指向空目录，避免和正在运行的 GUI 抢 library 锁）；
2. `ffmpeg -i out.tif -f rawvideo -pix_fmt rgb48le out.raw`；
3. 用 Python 把 JPEG 解成线性 sRGB 的 `f32`，跑 `equalize_raw`，再编码回 sRGB 比较。

经验：**务必确认 width/height 顺序**（memmap 是行优先），写反会让空间滤波沿错误
方向工作，误差会从 0.15 级飙到 6 级——这是本次对拍唯一踩到的坑。

## 单元测试

```
cargo test -p fdxmp-color --lib                            # 74 项：UCS 往返、RBF、恒等、色相隔离、空间滤波
cargo test -p fdxmp-color --lib -- --ignored --nocapture   # 1080p 性能
```
