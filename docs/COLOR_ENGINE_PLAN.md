# 色彩引擎整合方案（LightCraft + darktable → fdxmp）

> 范围：调色（色彩算法、色调映射、调色参数与互操作）。蒙版、AI、解码不在本文范围内。
> 目标：算法只写一份（纯 Rust），四端（Windows / iOS / Android / HarmonyOS）共用。

## 0. 结论

**可以整合，但要区分两条完全不同的路：**

| 项目 | 许可 | 能做什么 |
| --- | --- | --- |
| LightCraft（storytold/lightcraft） | MIT OR Apache-2.0，且本身就是 Rust | **可以直接改编代码**（保留版权声明即可）。本仓库已按 MIT 条款吸收其调色管线结构 |
| darktable（darktable-org/darktable） | GPLv3 | **不能复制任何代码**。fdxmp 是 MIT，引入 GPL 代码会让整仓被迫 GPL。只能"看思路、按公开论文/标准重写数学" |

说明：`getartcraft.com/apps/lightcraft` 是 LightCraft 的产品页（不是第二个仓库）；
页面上另外两个仓库是 `craft-launcher`（安装器）与 `craft-libs`（UI 库），与调色无关。

## 1. 现状与差距（fdxmp）

| 环节 | 现状（`crates/core`） | 目标 | 里程碑 |
| --- | --- | --- | --- |
| 工作空间 | 线性 sRGB（rgba16float 中间纹理，`color_pipeline.wgsl`） | 保留（如需更宽色域再评估 Rec.2020） | — |
| 白平衡 | 自制相对刻度（5500K 基准）+ Bradford | 标准 Kelvin/Duv 模型；RAW 走相机空间重解矩阵 | M4 |
| 调色运算空间 | sRGB 编码域 HSV/HSL（`cs_base` 的 HSL 段） | OkLab / OkLCh（低饱和加权、肤色保护、色带分割） | M2 |
| 曲线 | gamma 空间 256 项 LUT（做法正确） | 复用；生成端换成单调样条（不出现色调反转） | M2 |
| 高光 / 阴影 | 全局 | 边缘感知的局部基底层（无光晕，接近 Lightroom 手感） | M3 |
| 色调映射 | 无胶片曲线（对比度在 sRGB 域） | `ToneMap::film`（对数域对比度 + 扩展 Reinhard 肩部 + 软膝趾部） | M3 |
| HSL / 颜色分级 | 参数齐全（8 段、三分区 + 混合/平衡） | 迁到 OkLCh；分级补全局轮与 fall-off | M2 |
| 缺失项 | 点颜色、相机校准、黑白混色、3D LUT、色域映射、DNG 相机 profile | 见 `crates/color` 与 M2–M4 | M2–M4 |
| `crs:` 互操作 | 导入 25 个字段（`preset.rs`） | 扩到曲线 / HSL / 分级 / 校准 / 颗粒细节 / 镜头 / 几何，并支持导出 | M5 |

## 2. 已交付：`crates/color`（本方案的第 1 步）

纯 Rust、零依赖、`forbid(unsafe_code)`，直接编进 `fdxmp-core`，四端与 WASM 共用：

| 模块 | 内容 |
| --- | --- |
| `math` | Mat3、CIE xy、sRGB / Display P3 / Rec.2020 / ProPhoto / Adobe RGB、Bradford 色适应 |
| `transfer` | sRGB、纯 gamma、PQ、HLG |
| `cct` | 黑体轨迹、色温 ↔ xy、色调（Duv，正=洋红）、mired、白平衡矩阵 |
| `perceptual` | OkLab / OkLCh（任意工作空间往返）、HSV / HSL |
| `spline` | Fritsch–Carlson 单调曲线 + LUT + LUT 合成 |
| `bands` | 8 色带中心色相（HSV→OkLCh 映射）与单位分解权重 |
| `ops` | 自然饱和度 / 饱和度 / 8 色带混色 / 点颜色 / 三级分级 / 相机校准 / 黑白 |
| `tone` | 场景参考胶片曲线、显示参考曲线（中性恒等） |
| `gamut` | 单调软裁剪 + 去饱和式色域映射 |

验证：`cargo test -p fdxmp-color` → 37 项通过；`cargo clippy -p fdxmp-color --all-targets -- -D warnings` 干净。

## 3. 从两个项目各取什么

### 3.1 LightCraft（可直接改编，MIT/Apache-2.0）

1. **调色算子**（`crates/pipeline/src/colorops.rs`）：自然饱和度（低饱和加权 + 肤色保护）、
   8 色带分割权重、点颜色（软范围 + variance）、三分级（blending / balance 掩码）、
   相机校准（基色在 OkLCh 旋转缩放后归一化回白点）、黑白混色 → 已进 `ops.rs`。
2. **色调映射**（`crates/pipeline/src/tone.rs`）：对数域对比度、可移动白点的扩展 Reinhard 肩部、软膝趾部 → 已进 `tone.rs`。
3. **色彩科学基座**（`crates/color`）：矩阵 / 色适应 / CCT / 传递函数 / 样条 → 已进 `math`、`cct`、`transfer`、`spline`。
4. **DNG 相机色彩**（`crates/raw/src/{color,profile}.rs`）：ColorMatrix1/2 按 CCT 插值、AnalogBalance /
   CameraCalibration / ForwardMatrix、AsShotNeutral → xy 迭代、相机空间白平衡重解、
   ProfileHueSatMap / ProfileLookTable / ProfileToneCurve（线性 ProPhoto 的 HSV 三线性查表）→ **M4 的主要素材**。
5. **`crs:` 字段映射表**（`docs/xmp-interop.md`）：~60 个 Lightroom 字段 → 控件 1:1 映射 → M5 扩表。
6. **工程方法**：阶段缓存键（拖动只重算受影响阶段）、`docs/pipeline.md` 式的阶段顺序文档、
   GPU/CPU 双实现"一致性到 1/255"的验收口径 → M2/M6 采用。

### 3.2 darktable（GPLv3：只取算法方向，代码必须自己重写）

| 想要的 | 为什么值得 | 重写依据（公开资料） | 落点 |
| --- | --- | --- | --- |
| darktable UCS 2022 感知空间 | 大幅推饱和度时色相 / 明度漂移比 OkLab 更小 | 作者公开的设计说明与论文 + 公开的色适应方法 | M2 备选（与 OkLab 并存，可切换） |
| `colorbalancergb` 的分级模型 | 四轮 + fall-off + 白点支点 + hue/chroma 分离 + 色域压缩，比 Lightroom 更完整 | Filmlight 公开的 Yrg 材料 + 该模块的公开文档（参数语义） | M2/M3 |
| sigmoid / AgX 显示变换 | 柔和的高光滚降、不偏色，是"相机直出感"的较好底子 | Sigmoid 的数学是广义 logistic（公开）；AgX 源自 Troy Sobotka 的公开设计 | M3（作为可选显示变换） |
| tone equalizer（曝光域局部色调） | 高光 / 阴影的"曝光感"调整，是 LR 手感的来源 | 公开的"曝光不变引导滤波"思路 | M3（与局部基底层合并） |
| 色域映射（感知空间软裁剪） | 场景线性管线的必需出口 | 公开的色域映射方法（向亮度收缩 / 软膝） | M3（`gamut.rs` 已有基础版） |
| 色卡生成相机 profile | 用户可自建"像相机 / 像 Adobe"的 profile | 公开的色彩目标与最小二乘拟合方法 | M4 |
| spektrafilm 胶片仿真 | 差异化卖点（光谱乳剂 / 相纸） | spektrafilm 是第三方项目，**许可需单独核实**，数据与代码不能默认可用 | 备选（先不做） |

同时明确**不要**做的：不引 darktable 的 `data/wb_presets.json`、`noiseprofiles.json`（同为 GPL 数据），
不做模块系统 / 直方图 UI / OpenCL 移植等与本项目无关的部分。

## 4. 里程碑

| 里程碑 | 内容 | 验收 | 估计 |
| --- | --- | --- | --- |
| **M1（已完成）** | `crates/color` 基础层 + 测试 + 许可文件 | `cargo test -p fdxmp-color` 全绿、clippy 干净 | — |
| **M2（已完成）** | core 接入：参数结构 → CPU 参考路径（导出 / iOS 预览）+ WGSL 内核（OkLCh 段、点颜色、校准、黑白）；曲线改用统一单调样条；`crs:` 扩展字段 | CPU/GPU 一致性 ≤1/255（`gpu_colour_stage_matches_cpu_reference`、`gpu_monochrome_mix_matches_cpu_reference`）；帧时间冒烟测试通过 | 已交付 |
| M3 | 色调：`film` 曲线进 `cs_base`、局部基底层（高光/阴影无光晕）、色域映射、可选显示变换 | 高光恢复无光晕用例；直方图不削顶 | 1.5–2 天 |
| M4 | 相机色彩：读 DNG 标签（矩阵 / AsShotNeutral / HueSatMap）→ 相机空间白平衡；Kelvin 滑杆切到标准模型（保留旧值迁移） | 同机 DNG 与 Lightroom 默认渲染的 ΔE 基线测量 | 2–4 天 |
| M5 | 互操作：`crs:` 读写扩展（曲线 / HSL / 分级 / 校准 / 颗粒 / 镜头）、`.cube` LUT 导入 | 用 Lightroom 预设往返导入导出 | 1–2 天 |
| M6 | 四端验收：wgpu 后端（DX12 / Vulkan / Metal）与 CPU 路径的对比测试；iOS Metal 预览补齐 | 四端各出一张同参数渲染 + 差异报告 | 1–2 天 |

### M2 交付记录（2026-10-16）

**新增参数**（`crates/core/src/params.rs`，全部带 serde 默认值，旧预设不受影响）

| 字段 | 说明 |
| --- | --- |
| `color.black_white` | 黑白模式开关：打开后按 `bw_mix` 的 8 色带权重转单色（原"饱和度 −100"的老预设仍按原样工作） |
| `calibration.*` | 相机校准：红 / 绿 / 蓝原色的色相 ±100、饱和度 ±100、阴影色调 ±100（绿 ↔ 洋红） |
| `bw_mix.*` | 8 色带黑白混色（红橙黄绿青蓝紫洋红，±100） |
| `point_colors[]` | 点颜色采样（最多 4 个进 GPU uniform：OkLab 明度 / 彩度 / 色相 + 四项调整 + 四项范围） |

**统一实现**（CPU 与 GPU 同一份数学）

- `crates/core/src/color_engine.rs`：`ColorStage`（相机校准 + 调色算子）与 `pack_color_slots`（GPU uniform 打包）。
- `color_pipeline.wgsl`：新增 OkLab / OkLCh 段（`pipeline_colour_ops`），替换原来的 HSV HSL + 显示域分级；
  uniform 由 27 个 `vec4` 扩到 49 个（`PARAM_FLOATS` 108 → 196），新增槽位：
  色带中心 108..115、黑白 116..124、校准 125..135、点颜色 136..176、分级向量 177..190、肤色 191、总开关 192。
- `preview.rs`（移动端 CPU 预览 / 导出路径）改为调用同一个 `ColorStage`，删除了旧的 `apply_hsl` / `apply_grading` /
  `rgb_to_hsv` / `hsv_to_rgb`——此前 CPU 与 GPU 的调色公式并不一致，现在两者由同一实现驱动。
- 曲线 LUT 改由 `fdxmp_color::MonotoneCurve` 生成（`gpu_pipeline::build_curve_lut`），控制点退化时回退到原实现。
- `preset.rs`（`crs:` 导入）新增：HSL 八色带（`HueAdjustment*` / `SaturationAdjustment*` / `LuminanceAdjustment*`）、
  颜色分级与旧分离色调（`ColorGrade*` / `SplitToning*`）、相机校准（`RedHue` / `RedSaturation` / … / `ShadowTint`）、
  黑白（`ConvertToGrayscale` / `GrayMixer*`）。

**语义变化（需要在 UI 文案与预设说明里体现）**

- HSL 的色带判定从"显示域 HSV + 固定 30° 半径"改为"OkLCh 色相 + 相邻带升余弦分割"：
  权重和恒为 1（无死区），但两带中点在 OkLCh 中不再等于 HSV 的几何中点。
- 明亮度 + 在 OkLCh 里保持彩度会显得更艳，因此正向提亮同时按 `1 − 1.2·ΔL` 轻微去饱和（贴合 Lightroom 的柔光观感）。
- 黑白模式成为一等状态（`color.black_white`），黑白的 8 色带混色只在该模式下生效。

**已知遗留（非本次改动引入）**

- `crates/core/tests/gpu_pipeline.rs` 中 `gpu_negative_contrast_does_not_invert` 与
  `gpu_tile_matches_full_render_region` 在本轮改动前即为失败状态（已用 `git stash` 在基线复核）。
- `preview.rs` 测试里有两处 `clippy::field_reassign_with_default` 警告，会让 `--all-targets -- -D warnings` 失败。
- 四端的 UI 面板（点颜色取色器、校准面板、黑白混色面板）尚未接入；参数、预设与 XMP 导入已就绪。

### M2.5 交付记录：iOS 调色面板（2026-10-16）

**新增面板**（`platforms/ios/Fdxmp/`，颜色分类下新增三个子页）

| 子页 | 内容 |
| --- | --- |
| 点颜色 | 「取色」进入取样模式（画布点按取样）、最多 4 个样本（色块可切换 / 长按删除）、样本调整：色相 / 饱和度 / 明亮度偏移、变体、范围、色相 / 饱和度 / 明度范围 |
| 校准 | 红 / 绿 / 蓝原色的色相与饱和度 ±100、阴影色调 ±100 |
| 黑白 | 黑白开关（写入 `color.black_white`）+ 8 色带明亮度混色 |

**数据通路**

- `EditState`：新增 `blackWhite` / `calibration` / `bwMix` / `pointColors`，`EditJSON.build` 按 Rust 字段名写出
  （`color.black_white`、根级 `calibration` / `bw_mix` / `point_colors`），`EditJSON.parse` 反向读取；旧编辑记录缺字段时回落到默认值。
- 用户预设（`PresetLibrary`）：相机校准 / 黑白 / 点颜色随「颜色」分组一起保存、应用与导出；预设强度按比例插值（黑白开关与点颜色样本按 50% 切换，与曲线同策略）。
- 取色：画布点按 → `MaskCanvasGeometry`（与蒙版 / 修图同一套几何）归一化 → 从最近一帧 CPU 预览像素取色 →
  新增的 C ABI **`fdxmp_color_oklch_from_srgb8`** 转成 OkLCh → 写入样本。矩阵与调色段同源（`color_engine::oklch_from_srgb8`），四端共用。
- 跨语言契约测试：`color_engine::tests::ios_json_payload_maps_into_the_stage` 用 iOS 写出的键名解析 `EditParams`，
  校验黑白 / 校准 / 黑白混色 / 点颜色四组字段都对得上。

**验收路径**：本机无 Swift 工具链，Swift 侧靠 GitHub Actions 的 `iOS 未签名 IPA` 工作流编译
（`platforms/ios/**` 或 `crates/**` 有推送即自动触发）；Rust 侧已通过 `cargo test -p fdxmp-core` 与 clippy。

**尚未接入**：Windows（egui）、Android（Compose）、HarmonyOS（ArkTS）的同类面板；
点颜色的「画布选色」在 GPU 零拷贝上屏路径下需要等一帧 CPU 预览（当前提示"预览尚未就绪"）。

### M3 交付记录：色调段（2026-10-16）

**改了什么**

1. **色调段进入场景线性**（`crates/core/src/tone.rs` 新增；GPU 在 `cs_main` 的 `pipeline_tone`）：
   - 对比度 / 白色 / 黑色 → `fdxmp_color::ToneMap::film`（对数域斜率、白点可移动的扩展 Reinhard 肩部、软膝趾部）；
   - 高光 / 阴影 → **乘性 EV 位移，权重取自基底层**（大模糊后的场景亮度，不是像素自身亮度）；
   - 色域映射（`gamut::map_to_gamut`：软裁剪 + 保色相去饱和）收在色域出口。
2. **中性 = 恒等**：三个曲线滑块为 0 时曲线逐点等于输入（8bit / JPEG 源原样通过）；
   白点的恒等锚点是场景白（`SCENE_WHITE_EV = log2(1/0.18)`），这是与 LightCraft 原常量（2.9 EV）的关键差别——
   否则只动"阴影"也会顺手压白。
3. 原来的显示空间对比度 S 曲线与分区色调（`pipeline::apply_reference` 的对比度段、`preview::apply_tone_zones`）**删除**；
   参数曲线 / 曲线 LUT / 调色算子整体后移到色调段之后（`pipeline_display_ops`）。
4. GPU uniform 增加电影曲线 LUT（4096 项 = 16 KB，槽位 `tone`），CPU / GPU 查同一张表；
   新增 `slot::TONE_ACTIVE`（193）整段跳过。

**语义变化（UI 文案与预设说明需要的）**

- 对比度：显示域 S 曲线 → 场景线性对数域斜率（±100 → 0.6× / 1.55×）。
- 白色：白点 EV 移动（+ 更亮直至裁切；- 收敛留余量）。
- 黑色：软膝趾部；`黑色 +` 会抬起黑点（纯黑 → 4.8%（≈48/255）@ +100，与 Lightroom 同向）。
- 高光 / 阴影：权重来自大模糊基底层，小面积高光被保留、大面积高光才被恢复（测试 `highlights_use_the_local_base_not_the_pixel`）。
- 色调段整体是乘性的 → 纯黑保持纯黑、通道比例（色相）不变。

**验收**

- CPU：`tone.rs` 6 个单元测试（恒等、灰锚定、对数域对比度、局部基底、乘性阴影、色域）；
- GPU：`gpu_base_stage_matches_cpu_reference` 扩展为"基底 + 色调段"的 CPU/GPU 一致性（≤2/255）；
  `gpu_negative_contrast_does_not_invert`（**修复了原基线失败**）、`gpu_tone_zones_are_separated` 按新语义更新；
  帧时间冒烟测试通过。

**遗留**

- 基底层半径当前复用大模糊参考（`4 px × 分辨率缩放` ≈ 长边的 0.2–0.8%），局部尺度偏小；
  更接近 LightCraft 的 2% 尺度需要新增一级模糊级联（GPU 两趟宽核 + 专用纹理）；
- CPU 基底层的核与 GPU 完全同核同分辨率（保证区域渲染逐像素一致），代价是**大图导出**时
  核成本随分辨率线性上升（6000 px 长边、半径 16 → 约 33 抽头 × 2 趟 × 3600 万像素）。
  候选优化：大图（> 2048）改为 1/4 分辨率基底 + 双线性上采样（牺牲大图区域渲染的逐像素一致，
  预览 ≤ 1440 不受影响），或直接把导出走 GPU 分块路径。
- `gpu_tile_matches_full_render_region` 仍是基线失败（分块与整图偏差 8 > 6），与本次改动无关；
  但**局部色调在分块渲染下也会在块的边缘失去基底层上下文**，导出大图前建议一并解决（给分块加余量）。

## 5. 约束与风险

1. **许可卫生**：禁止把 darktable 代码 / 数据搬进本仓库；重写时按公开论文和标准写，并在
   `NOTICE` / `crates/color/README.md` 记录来源。若某算法找不到公开依据，宁可不做。
2. **参数兼容**：现有 `.fdxmp` 预设、`crs:` 导入的数值语义不能变；新增字段一律带默认值，
   必要时给参数结构加版本号并写迁移。
3. **双实现一致性**：色彩算子在 CPU（`crates/color`）与 WGSL 各有一份，必须有黄金图对比测试，
   否则拖动预览与导出会分叉。
4. **性能**：新增算子尽量并入现有三趟（基础 / 模糊 / 主调色），参数走 uniform 数组；
   超出槽位再开一趟，不要每加一个滑杆就加一趟。
5. **工作空间**：暂保留线性 sRGB。OkLCh 在 sRGB 上往返有轻微色域压缩，
   若 M4 要处理 ProPhoto/Rec.2020 相机色彩，需评估切到 Rec.2020（`RgbSpace` 已支持，成本主要在着色器常量）。
