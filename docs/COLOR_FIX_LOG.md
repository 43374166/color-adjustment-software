# 色彩管线修复记录（2026-10-05）

依据 2026-10-05 的代码审查（核心算法清单 + 问题清单），按建议顺序分批完成：

1. 第一批：3 个 P0（对比度反相 / 蒙版叠加语义 / 模糊抽头间距）——见 1.1–1.3；
2. 第二批：C1 + C5 合并（细节参考域 + 线性 f16 工作空间）——见 1.4；
3. 第三批：C2 + C8 合并（色相 / 色调分区重写）——见 1.5。
4. 第四批：C6 + C5b + C7 合并（输入 / 导出色彩管理闭环 + 线性源上传）——见 1.6。
5. 第五批：C3 + C4 合并（白平衡色适应 + 颜色分级重做）——见 1.7。
6. 第六批：C9 + C10 合并（曲线单调样条 + 1:1 分块一致性）——见 1.8。
7. 第七批：C11 + C17 + C14（部分）（蒙版缓存 / 分块导出 / bind group 缓存）——见 1.9。
8. 第八批：C10c + C17b 合并（细节半径跨分辨率 + 导出 CPU 内存）——见 1.10。
9. 第九批：C12 + C13 + C15（几何面板写入目标 / CPU 参考对齐 / 导出格式细节）——见 1.11。
10. 第十批：C15b（WebP 部分）+ C6b + C6c（元数据 / 16bit 输入 ICC / RAW 色彩路径）——见 1.12。
11. 第十一批：C15b（HEIC 部分）+ C18（HEIC 元数据 + 镜头校正 f32）——见 1.13。
12. 第十二批：C14b（部分）+ C3b + C15b 调研（提交合并 + 开尔文显示 + TIFF 可行性）——见 1.14。
13. 第十三批：界面模块化 + 效果 / 颗粒 / 暗角面板（非算法修复，见 1.15）。
14. 第十四批：左栏图标列（图库 / 预设 / 历史 / 导出 / AI）+ AI 面板（非算法修复，见 1.15 末）。

其余问题**不动代码**，在本文件第 3 节登记建账。

## 0. 2026-10-25 极端值修复（调色优先）

目标：滑块到 ±100 时不能出现反相 / 负通道 / 硬裁剪；CPU 与 GPU 的最终显示结果必须一致。

算法参考：LightCraft 的显示段顺序（色调后调色、调色后色域出口）与 CPU/GPU 一致性验收；
darktable `colorbalancergb` 的"感知空间 + 色域压缩"思路（只借鉴公开文档与参数语义，未复制 GPL 代码）。

| 问题 | 修复 | 回归测试 |
| --- | --- | --- |
| 点颜色 `lum_shift=-100` 把近黑压成负 L，OkLab→RGB 出负通道 | `ColorOps` 在点颜色每步后夹 `L∈[0,1]`、`C≥0`、色相回绕 | `ops::point_color_extreme_lum_shift_keeps_black_black`、`color_extremes::point_color_endpoints...` |
| 颜色分级亮度 ±100 把黑推成负 L | `grade()` 末尾夹 `lab[0]`；GPU 同步 `clamp(lab.x)` | `ops::grading_extreme_lightness...`、`color_extremes::grading_luminance...` |
| HSL 明亮度 -100 把暗部 L 推成负数；+100 把纯黑抬灰 | 加入黑/白权重 `smooth(0,0.05,L)`，并夹 `L∈[0,1]`；GPU `smoothstep` 同步 | `ops::hsl_luminance_extremes_keep_black_and_white`、`color_extremes::hsl_luminance...` |
| 饱和度 +100 / 极值分级产生超出色域的线性 RGB，CPU 逐通道硬裁剪、GPU 直接编码 → 色相漂移 / 死色块 | 新增 `gamut::map_to_gamut_oklch`（固定 OkLCh L/h，只压缩 C）；CPU `ColorStage::map_display_gamut` 与 GPU `gamut_map_oklch` 同算法 | `core::color_extremes::saturation_plus100...`、`display_gamut_mapping...`、`gpu_curve_only_matches_cpu_preview`、`gpu_colour_stage_extremes_match_cpu_reference` |
| 色温 -100 + 色调 +100：3200K 的 Duv 0.032 贴近光谱边界，Bradford 蓝增益 169×，中灰 → 负 R/G + 34× B | 保留 `TINT_DUV=0.032`（不改变旧预设 tint 强度），把锥体增益夹到 `[1/4, 4]` | `white_balance::extreme_temperature_and_tint...`、`color_extremes::white_balance_endpoints...` |
| CPU 只在“有调色”时才做色域映射，曲线单独使用时与 GPU 不一致 | `adjust_pixel` 无论调色段是否激活都做显示色域映射 | `gpu_colour_stage_matches_cpu_reference`、`gpu_monochrome_mix...` |

验证（WSL Kali + Rust 1.99.1，llvmpipe Vulkan）：

- `cargo test -p fdxmp-color`：42 个单元 / 文档内测试 + 3 个极端 sweep = 45 通过。
- `cargo test -p fdxmp-core --lib -- --test-threads=1`：173 通过、1 ignored（串行；并行时 `masks::ai_masks_require_generated_raster` 为既有全局状态 flaky）。
- `cargo test -p fdxmp-core --features gpu --test gpu_pipeline`：32 通过（含新增极端 CPU/GPU 一致性、曲线单独使用一致性）。
- `cargo test -p fdxmp-core --test color_extremes`：12 通过（含曝光 / 效果 / 饱和度 +100 的色相漂移 <2°、明度漂移 <0.10 断言）。
- `cargo clippy -p fdxmp-color -p fdxmp-core --all-targets -- -D warnings`：通过。
- 真机照片 `DSC09784.jpg` 用 CPU 预览跑了 10 组极值并目检（诊断图不落盘；可用 `cargo run -p fdxmp-core --example color_sweep -- <输入> <输出目录>` 复现）。

Windows 原生验证（`x86_64-pc-windows-gnu` 交叉编译，RTX 3060 Ti / Vulkan）：

- Windows CPU 测试：`fdxmp-core --lib` 173、`fdxmp-color` 42 + 极端 sweep 3、`core::color_extremes` 12，全部通过。
- Windows GPU 测试：`gpu_pipeline` 32 通过（含新增 `gpu_colour_stage_extremes_match_cpu_reference` 与曲线单独使用一致性）。
- Windows 桌面：release `fdxmp-win-ui.exe` 启动并打开 `DSC09784.jpg`，状态栏显示首屏 314 ms、代理 2048×1365、Vulkan RTX 3060 Ti。
- Windows UI Automation：成功把「饱和度」设为 +100、「对比度」设为 -100；画面饱和度变化、黑白不反转，历史记录正常。鼠标拖拽仍受 Windows 前台/焦点限制，未做稳定的端到端拖拽脚本。

Android 模拟器验证（API 35 / x86_64 / KVM，WSL 内 headless emulator）：

- 同一份 Rust 核心，`cargo test --target x86_64-linux-android` 产物 push 到模拟器运行：
  - `fdxmp-color`：42 + 极端 sweep 3 全部通过；
  - `fdxmp-core --lib`：173 通过、1 ignored；
  - `color_extremes`：12 通过；
  - `fdxmp-ffi --lib`：12 通过（JNI/C ABI 同源核心）。
- NDK 27.2 + `x86_64-linux-android35-clang` 交叉编译；说明同一份 `crates/color` / `crates/core` 在 Android 运行时通过。复现步骤见 [`ANDROID_EMULATOR_TEST.md`](ANDROID_EMULATOR_TEST.md)。

HarmonyOS：`cargo check -p fdxmp-color --target aarch64-unknown-linux-ohos` 通过（纯色彩数学层）；完整 core / bridge / 模拟器仍需 DevEco/OHOS SDK。

复现命令（WSL/Linux 交叉编译到 Windows GNU）：

```bash
rustup target add x86_64-pc-windows-gnu
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
cargo build --release -p fdxmp-win-ui --target x86_64-pc-windows-gnu
cargo test -p fdxmp-core --features gpu --test gpu_pipeline \
  --target x86_64-pc-windows-gnu --no-run
```


尚未完成（环境阻塞）：
- HarmonyOS 模拟器：本机未装 DevEco Studio/OHOS SDK；纯色彩层已通过 OHOS target check，但没有 HAP/真机/模拟器验证。
- Lightroom 2026 对照：已定位 `D:\install\Adobe\Adobe Lightroom Classic 2026` 与 `D:\Pictures\Lightroom\Lightroom Catalog.lrcat`，本轮未做自动化导出对照，下一步用真实 RAW/JPEG 做 ΔE 基线。



## 1. 已修复

### 1.1 对比度 -100 反相（P0）

- 位置：`crates/core/src/shaders/color_pipeline.wgsl:166`
- 旧行为：`contrast = 1 + p(1)/100 × 1.5`
  - `-100` → 系数 **-0.5**（黑白颠倒）
  - `≈ -66.7` → 系数 0（整幅压成 0.18 中灰）
- 新行为：正半轴保持线性放大；负半轴用 `1 / (1 - 1.5a)`（`a` 夹取到 `-1..1`）

| 对比度 | -100 | -50 | 0 | +50 | +100 |
| --- | --- | --- | --- | --- | --- |
| 系数 | 0.40 | 0.57 | 1.00 | 1.75 | 2.50 |

  单调、连续（在 0 处两段导数相同）、任意取值都不反相。
- 回归测试：`gpu_negative_contrast_does_not_invert`
  （黑白分区图在对比度 -100 下必须保持黑 < 白，且向中灰压缩）

### 1.2 蒙版由"替换全局"改为"全局 + 局部增量"（P0）

- 位置：
  - `crates/core/src/params.rs:894` 新增 `EditParams::compose_local`
    （并抽出 `eval_curve` / `ToneCurve::is_identity` 供曲线串联复用）
  - `crates/core/src/gpu_pipeline.rs:789` 蒙版趟先合成再打包参数
- 旧行为：每个蒙版把 `mask.adjustments` 当作完整参数集直接对**原图**渲染，
  再按权重混入当前结果。新建蒙版默认全 0，因此笔迹范围内会**把全局调色全部抹掉**；
  局部 +1EV 得到的是"原图 +1EV"而不是"全局结果 +1EV"。
- 新行为：`composed = global.compose_local(mask.adjustments)`，再以 `mix(current, composed, weight)` 叠加，
  局部区域 = 完整全局管线 + 局部增量。

  合成规则：

  | 类别 | 规则 |
  | --- | --- |
  | 光照 / 颜色 / 效果 / HSL / 分级 / 降噪 / 锐化数量 | 全局 + 本地，按合法范围夹取 |
  | 曲线（RGB 与 3 通道） | 串联：`local(global(x))`；本地为恒等时保持全局 |
  | 锐化半径、参数曲线分界 | 本地非默认（> 0）时覆盖 |
  | 裁剪 / 变换 / 镜头 | 只取全局（蒙版不参与几何调整） |
  | 嵌套蒙版 | 忽略（蒙版不可嵌套） |

- 回归测试：
  - GPU：`gpu_empty_mask_keeps_global_edits`（全局 +1EV + 零增量蒙版 = 全局结果）
  - 单元：`compose_local_default_is_noop` / `compose_local_stacks_and_clamps` /
    `compose_local_keeps_geometry_and_drops_nested_masks` / `compose_local_concatenates_curves`
- 兼容性：`.fdxmp` 与参数 JSON 结构未变。已存工程里的蒙版 `adjustments`
  语义从"绝对值"变为"增量"——这正是修正后的 Lightroom 语义，
  旧工程中带蒙版的照片观感会变化（属预期修正）。

### 1.3 模糊抽头间距（P0）

- 位置：`crates/core/src/shaders/blur.wgsl:23`
- 旧行为：方向先乘半径、循环里再乘 `i`，抽头间距 = 半径：

  | 传入半径 | 旧采样偏移 | 后果 |
  | --- | --- | --- |
  | 0.5 | {0} | 模糊 = 原图，锐化完全失效 |
  | 2.0 | {0, ±2, ±4} | 跳过奇数像素，1px 细节出现 ±2px 幽灵边缘 |
  | 4.0（主模糊） | {0, ±4, …, ±16} | 摩尔纹 / 重影，污染纹理、清晰度、锐化、降噪 |

- 新行为：方向取单位向量（±1/0），`偏移 = 方向 × i`，间距恒为 1px；
  半径只决定抽头数量（1..6，最多 13 抽头）。
- 连带变化：清晰度 / 纹理 / 锐化 / 降噪的**强度观感会变化**（旧效果被伪影放大）；
  这是预期内的修正。
- 测试调整：`gpu_detail_subparameters_change_output` 中"颜色降噪平滑度"用例
  原先是靠梳状伪影才越过阈值（修复后 0.19 < 阈值 0.2），现改为使用
  **带色度噪声的输入图**（`test_image_with_color_noise`）——颜色降噪本就作用于色噪，
  干净合成图上该参数几乎无用武之地。

### 1.4 细节参考域 + 线性 f16 工作空间（C1 + C5，第二阶段）

- 位置：`crates/core/src/shaders/color_pipeline.wgsl`、`crates/core/src/gpu_pipeline.rs`
- 旧结构：单趟 `cs_main` 一次做完"调色 + 细节"，模糊参考取**未调色的原图**；
  中间结果与蒙版 ping-pong 都是 8bit sRGB（每趟 `clamp(0,1)` + 量化）。
- 新结构（每次渲染的完整趟序）：

  ```
  cs_geometry  src(sRGB8) → stage(f16 线性)
  cs_base      stage → base(f16 线性)          // 白平衡→曝光→色调→对比度→
                                               // 参数曲线→曲线 LUT→HSL→分级→饱和度
  模糊 ×2/×4   base → blur_big / blur_small    // 参考与细节链路同域
  cs_main      base + 模糊 → work(f16 线性)     // 纹理/清晰度/去雾/锐化/降噪/暗角/颗粒
  蒙版（逐个）  compose_local → cs_base → 模糊 → cs_main（按权重混入 work）
  cs_encode    work → display(rgba8unorm sRGB) // 整条管线唯一的量化 / 裁剪点
  cs_overlay   蒙版叠加显示（8bit 暂存 → 拷回 display）
  ```

- 关键修复：
  - `detail = c - blurred` 不再含直流分量：曝光 / 白平衡变化后，纹理、清晰度、
    锐化、亮度 / 颜色降噪都只作用于真实局部细节（回归测试
    `gpu_detail_reference_shares_exposure`：均匀灰面上 +1EV + 细节参数全开，
    亮度不得变化）；
  - 蒙版趟同样按"蒙版自己的 base → 模糊 → 细节"渲染，
    局部调整的细节效果与全局一致；
  - 全链路 f16 线性、不再逐趟 `clamp(0,1)`；曲线与 HSL 段对 >1 的高光余量做了
    保余量处理（曲线只作用于 0..1 定义域、溢出部分原样保留；HSL 按峰值归一后还原）；
  - 蒙版 ping-pong 与中间缓冲改为 f16，量化 / 条带只可能发生在最后一次编码。

同时**移除**了"无蒙版时提前 return"的旧捷径（它会让编码趟不执行）。

尚未覆盖：源数据仍是解码后的 8bit sRGB（`decode` / `upload` 侧未动），
因此 RAW 的高光余量仍在解码环节被截掉，登记为 C5b。

性能（GTX 1050 Ti，2048×1365，`gpu_preview_frame_time_smoke_test`）：

| 场景 | 修复前 | 现在 |
| --- | --- | --- |
| 基础（含 2 趟模糊） | 5.4 ms（185 fps） | 6.8–8.3 ms（121–147 fps） |
| +2 个蒙版 | 10.0 ms（100 fps） | 18–23 ms（44–55 fps） |
| 旋转 5° + 裁剪 80% | — | 4.7–5.7 ms（174–212 fps） |
| 细节全开 | — | 8.5–10.2 ms（99–117 fps） |

  蒙版变贵是因为每个蒙版要多跑"基础调色 + 模糊"以得到同域的参考；
  仍满足 ≥30fps 目标。可选优化见 C17。

### 1.5 色相 / 色调分区重写（C2 + C8，第三阶段）

- 位置：`crates/core/src/shaders/color_pipeline.wgsl`
- 新增两个分区函数：
  - `hsl_band_weights(hue)`：色相落在相邻两个色带中心之间时线性分配权重，
    **权重和恒为 1**，分区无空洞；
  - `zone_weight(value, center, radius)`：平滑钟形分区，用于四条色调曲线。
- HSL 8 色带（旧行为：固定 30° 衰减半径 vs 60° 间距 → 90°/150°/210° 权重和为 0，
  滑杆完全无响应）：

  | 色相 | 90°（黄绿） | 150° | 210° | 260° | 270° |
  | --- | --- | --- | --- | --- | --- |
  | 旧权重和 | 0.000 | 0.000 | 0.000 | 0.52 | 0.74 |
  | 新权重和 | 1.000 | 1.000 | 1.000 | 1.000 | 1.000 |

  同时把色相判定从线性光空间移到**显示参考（sRGB）空间**，与肉眼看到的色相一致。
- 高光 / 阴影 / 白色 / 黑色（旧行为：乘性增益、权重重叠——中灰处 shadow≈0.70、
  black≈0.66）：

  | 滑块 | 分区中心 / 半径（sRGB 亮度） | 作用方式 |
  | --- | --- | --- |
  | 高光 | 0.75 / 0.35 | 增益（保护性） |
  | 阴影 | 0.25 / 0.35 | 增益 |
  | 白色 | 1.00 / 0.30 | **端点偏移**（显示域 ±8% @ ±100） |
  | 黑色 | 0.00 / 0.30 | **端点偏移**（显示域 ±8% @ ±100） |

  端点偏移解决了"乘性增益抬不起纯黑（0×任何数=0）、压不下纯白"的问题；
  四条曲线在中灰处互不干扰。
- 参数曲线（高光 / 中间调 / 阴影）的分界同样改用 sRGB 亮度：
  三个区域由分界（默认 0.65 / 0.35）切成"高光区 + 中间调平台 + 阴影区"，
  不再用线性亮度上的三角权（旧实现在线性域里 0.35 的分界相当于 sRGB 63%，
  "阴影"滑杆实际压住了大半幅中灰）。
- 回归测试：`gpu_hsl_bands_cover_all_hues`（90° 处黄 / 绿滑杆必须有响应；
  相邻色带 -100/+100 应抵消）、`gpu_tone_zones_are_separated`
  （黑色 +100 抬起纯黑、白色 -100 压下纯白、中灰不被端点滑块污染、
  阴影不改变纯黑）。
- 行为变化：色调 / HSL / 参数曲线相关滑杆与内置预设的观感会变化（属预期修正）。

### 1.6 输入 / 导出色彩管理闭环 + 线性源上传（C6 + C5b + C7，第四阶段）

**输入侧（C6）** — `crates/core/src/decode.rs`、`crates/core/src/color.rs`

- 常见格式改走 `ImageReader → ImageDecoder`：
  - 读取 EXIF `Orientation` 并 `apply_orientation`：竖拍 / 翻转的 JPEG / TIFF / WebP
    不再显示错误方向；
  - 读取嵌入 ICC（JPEG APP2 / PNG iCCP / TIFF·WebP 由解码器提供），
    用 lcms2 以**相对比色 + 黑点补偿**转换到 sRGB 工作空间。
- 覆盖 8bit RGB/RGBA；16bit 输入的 ICC 与 RAW 相机 ICC/DCP 登记为 C6b / C6c。

**线性源上传（C5b）** — `crates/core/src/decode.rs`、`crates/core/src/gpu_pipeline.rs`

- 新增 `DecodedImage::pixels_linear()`（16bit → 线性 f32）、
  `display_proxy_linear()`（线性光 Lanczos3 重采样）、
  `display_proxy_pair()`（一次重采样同时给出线性代理与对应 sRGB 8bit 版本）；
- 新增 `ColorPipeline::upload_linear_f32()`：源纹理为 `rgba16float`（含 f32→f16 转换），
  着色器读取即线性，源数据不再经过 8bit 量化；
- 接线：画布上传、全分辨率导出、批处理导出、1:1 分块渲染都改走线性通路；
  有镜头配置文件时仍走 8bit CPU 校正通路（见 C18）。

**导出侧（C7）** — `crates/core/src/color.rs`、`crates/core/src/export.rs`

- Display P3 配置改用 sRGB **分段传递函数**（lcms 参数曲线类型 4：
  `2.4, 1/1.055, 0.055/1.055, 1/12.92, 0.04045`），替换原来的纯 gamma 2.2 近似；
- 色彩空间转换意图从 `Perceptual` 改为 `RelativeColorimetric + BLACKPOINT_COMPENSATION`
  （矩阵型配置文件下 Perceptual 会退化且不补偿黑点）；
- 导出缩放改为**线性光** Lanczos3（sRGB → linear → resize → sRGB），
  消除在 gamma 编码值上重采样带来的边缘 / 中间调偏差。

**新增回归测试**

| 测试 | 覆盖 |
| --- | --- |
| `display_p3_roundtrips_back_to_srgb` | P3 分段曲线往返 ≤2/255 |
| `input_icc_conversion_restores_srgb` | 非 sRGB ICC 输入转换 |
| `downscale_averages_in_linear_light` | 黑白交替 4→2 缩放应得 ≈188 灰（旧为 ≈128） |
| `common_format_applies_exif_orientation` | EXIF Orientation=6 旋正并交换宽高 |
| `common_format_applies_embedded_icc` | "Adobe RGB + ICC" JPEG 解码回 sRGB |
| `f32_to_f16_roundtrips_typical_values` | 半精度转换精度 |
| `gpu_linear_upload_matches_srgb_upload` | 线性 f32 上传与 8bit sRGB 上传渲染一致 |

行为变化：竖拍照片自动转正；带非 sRGB ICC 的输入颜色更准确；
代理图 / 导出缩放改为 gamma 正确重采样（观感略有变化，属修正）。

### 1.7 白平衡色适应 + 颜色分级重做（C3 + C4，第五阶段）

**白平衡（C3）** — 新模块 `crates/core/src/white_balance.rs`

- 滑杆 ↔ 开尔文：`0 = 5500K`，暖端每单位 90K、冷端每单位 23K；与 XMP 导入共用同一映射
  （`kelvin_to_relative` / `relative_to_kelvin` 互逆，有往返单元测试）。
  原实现两处映射各自为政（XMP 按开尔文、渲染按固定增益），导入预设的色温必然偏。
- 白点模型：Planckian locus（Kim et al. 2002 近似）+ 色调沿轨迹法向偏移
  （±100 → ±0.02 Duv；正色调表示"假定照明偏绿"，校正后画面偏洋红）。
- 校正模型：Bradford 色适应（假定照明白点 → 5500K 参考白点），
  CPU 侧按参数算成 3×3 矩阵，通过 uniform 88..96 传给着色器第 1 段应用。
  0/0 恒等；温度 +100 变暖（R↑ B↓）、-100 变冷；色调 +100 偏洋红。
- 旧实现是固定增益（R/B ±25%、G ∓6%）：既不是色适应，也没有开尔文含义。
- uniform 参数区 88 → 100 floats（WGSL `Params.v` 22 → 25 个 vec4，4496 字节）。

**颜色分级（C4）** — `crates/core/src/shaders/color_pipeline.wgsl`

- 旧实现三个问题：分区用线性亮度；grade 直接加法（大会溢色发白）；
  `balance` 只是强度倍率；而且**每区的"明亮度"滑杆被当成上色量**——
  内置预设"电影"的阴影蓝色调（luminance −5）实际被算成负量，方向相反。
- 新实现：
  - 分区在 sRGB 亮度域；`混合` 控制过渡宽度（soft = 0.05..0.30），
    `平衡` 平移两个分界（±0.25）；
  - 每区 = 色相（颜色方向）+ 饱和度（上色强度）+ 明亮度（本区增益）；
  - 上色为**亮度保持混合**：目标色按自身亮度归一、乘当前像素亮度，
    再按 `强度 × 0.5` 混合——不再加法溢色；
  - 三区展开成无动态索引的累加（`add_grade_zone`）：曾因局部数组动态索引
    触发驱动降级，+2 蒙版从 22.9 ms 恶化到 37.5 ms，展开后恢复。
- 回归：`gpu_white_balance_warms_and_cools`、
  `gpu_grading_preserves_luminance_and_zones`（阴影染蓝且亮度变化 ≤8/255、
  其他分区不被污染、分区明亮度只提亮本区）+ 4 个 `white_balance` 单元测试。

行为变化：色温 / 色调滑杆与颜色分级、内置预设"电影 / 复古"的观感会变化
（预设原本的色彩分级方向是错的，现在按面板数值生效）。
新增两条待办：C3b（界面仍显示相对值，未显示开尔文）、
C3c（未读取 RAW as-shot 白平衡，开尔文以 5500K 为参考）。

### 1.8 曲线单调样条 + 1:1 分块一致性（C9 + C10，第六阶段）

**曲线插值（C9）** — `params.rs`、`color_pipeline.wgsl`、`win_ui`

- 控制点插值从线性改为**单调三次 Hermite（Fritsch–Carlson 限幅）**：
  曲线严格过控制点、一阶连续（控制点处不再折线），限幅保证不过冲
  （色调曲线不会反相 / 出光晕）；
- GPU 曲线 LUT 查表从最近邻改为**线性插值**（旧实现 256 级台阶在渐变处可见）；
- 曲线编辑器的绘制改用同一 `eval_curve`（所见即所得）；
- 回归：`curve_interpolation_is_smooth_and_monotone`（过点 / 单调 / 不过冲 /
  非线性）、`curve_is_c1_at_control_points`（控制点处一阶连续）。

**1:1 分块一致性（C10）** — `gpu_pipeline.rs`、`blur.wgsl`、`win_ui`

- 源图边距 4px → `BLUR_RADIUS × BLUR_SCALE_MAX + 4` = **12px**：覆盖细节模糊的
  抽头范围（旧值会让分块边缘因 clamp 采样产生接缝）；
- 暗角 / 颗粒改用**整图坐标**：新增 uniform 槽位 86/87（分块原点）与 97/98
  （整图输出尺寸），分块渲染时与整图渲染逐像素对齐；
- 细节模糊半径按分辨率缩放：`渲染长边 / 2048`，**只放大不缩小**（下限 1.0，
  上限 2.0）；半径 > 6 时切换到新的宽抽头入口 `cs_blur_wide`（±12，25 抽头），
  代理预览仍走原 13 抽头路径（性能不变）；
- 分块一致性测试重写为与 App 相同的流程（按输出矩形反算源图裁剪 + 12px 边距 +
  传入分块原点 / 整图尺寸），并加入暗角 + 颗粒；与整图渲染的偏差仍 ≤6/255。

性能（GTX 1050 Ti，2048×1365）：基础 8.41 ms（119 fps）、+2 蒙版 23.06 ms（43 fps）、
旋转 5° + 裁剪 5.79 ms（173 fps）、细节全开 10.20 ms（98 fps）——与改造前持平。

新增待办 C10c：半径缩放上限 2× 意味着 45MP 全分辨率导出的清晰度 / 锐化
仍比代理预览弱约一倍，彻底一致需要 mip 金字塔或多尺度模糊。

### 1.9 蒙版缓存 / 分块导出 / bind group 缓存（C11 + C17 + C14 部分，第七阶段）

**蒙版缓存与羽化（C11）** — `masks.rs`

- `mask_key` 只序列化 `kind / inverted / feather / opacity` + 图像尺寸：
  拖动局部调色滑块**不再触发蒙版栅格重建**（旧实现把整个 `MaskParams` 打进哈希，
  每次拖动都要重算画笔距离场 + 羽化模糊）；AI 栅格缓存同样受益。
- `box_blur` 改前缀和（水平）+ 逐列滑动窗口（垂直）：O(w·h·r) → O(w·h)。
  2048×1365、羽化半径约 68px 时，单次羽化从约 4 亿次乘加降到约 280 万次。
- 回归：`mask_key_ignores_adjustments_but_tracks_raster_fields`、
  `raster_is_independent_of_adjustments`（栅格确实与局部参数无关，保证缓存键安全）、
  `box_blur_matches_naive_reference`（滑动窗口与朴素实现逐像素一致）。

**分块渲染与显存（C17）** — `geometry.rs`、`win_ui`、`gpu_pipeline.rs`

- 全分辨率导出改**分块渲染**（`EXPORT_TILE_EDGE = 2048`）：GPU 只保留一个分块的工作集，
  45MP 全图的 f16 工作集从约 **2.9GB 降到约 0.3GB**；
- 分块规划抽到 core：`geometry::plan_tiles()`（输出分块 + 逆矩阵反算源图矩形 +
  细节模糊边距），导出路径与 1:1 视图共用；测试覆盖覆盖性 / 边距 / 越界夹取；
- 蒙版坐标补上**裁剪原点**（槽位 100/101）：此前 1:1 视图与分块渲染按
  "裁剪图坐标 ÷ 缩放"取蒙版，整体偏移裁剪原点/缩放（45MP 代理下可达上百像素）；
  现在按 `(源像素 + origin) / scale` 换算，分块测试加入蒙版 + 参考图原点后逐像素对齐。

**渲染开销（C14，部分）** — `gpu_pipeline.rs`

- 蒙版趟 bind group 按（目标代数 / 蒙版序号 / 乒乓相位 / 栅格哈希）缓存，
  目标重建时清空：拖动滑块不再每帧重建 bind group；
- 未做：把 6+ 次 `queue.submit` 合并为单 encoder——各趟共用同一个 uniform buffer，
  合并需要改成动态偏移（否则后写的参数会覆盖先写的），登记为 C14b；
- 说明：实测 bind group / submit 开销被 GPU 时间淹没（8.20 vs 8.41 ms 属噪声），
  该项收益主要在 CPU 侧抖动。

### 1.10 细节半径跨分辨率 + 导出 CPU 内存（C10c + C17b，第八阶段）

**细节半径跨分辨率（C10c）** — `blur.wgsl`、`gpu_pipeline.rs`

- 宽抽头模糊窗口 ±12（25 抽头）→ **±24（49 抽头）**，半径缩放上限
  `BLUR_SCALE_MAX` 2× → **4×**：45MP（8192 长边）的清晰度 / 锐化半径
  从 8px 提到 16px，与代理预览的相对强度一致（不再弱一半）；
- 小半径（锐化 / 降噪）同样按 > 6 切换宽抽头入口，上限同步到 24；
- 代理预览仍走 13 抽头窄路径（性能不变），宽路径只出现在 1:1 与全分辨率导出；
- 回归：`detail_blur_scale_grows_with_output_and_caps`（代理 1×、小图不缩小、
  1:1 按整图尺寸 4×、超限封顶）、`gpu_detail_radius_scales_with_full_size`
  （半径 > 6 时走宽抽头，宽 / 窄结果确有差异）。

**导出 CPU 内存（C17b）** — `decode.rs`、`win_ui`

- 新增 `DecodedImage::linear_region()`：按 16bit 原始数据裁剪后再解码到线性，
  分块导出与 1:1 视图**都不再为整图保留 Rgba32F 副本**（45MP 约 720MB）；
- 画布代理同样改为"先在 16bit 域缩放、再解码到线性"，
  加载时不再为整图建 f32 副本（45MP 约 720MB / 数百毫秒 → 代理尺寸几十 MB）；
- 回归：`linear_region_matches_full_linear_crop`（区域与整图线性数据一致 + 越界夹取）、
  `display_proxy_linear_scales_without_full_copy`（尺寸与线性值一致）。
- 代价（登记为 C17c）：代理缩放回到 sRGB 编码域（16bit），与导出路径的
  线性域缩放相比，代理边缘 / 中间调有细微差异；真正兼顾需分块线性缩放。

### 1.11 几何面板写入目标 + CPU 参考对齐 + 导出细节（C12 + C13 + C15，第九阶段）

**几何面板写入目标（C12）** — `win_ui`

- 新增 `edit_geometry_live()`：裁剪 / 变换 / 镜头（含"重置裁剪与变换"）共 17 处调用
  固定写**全局参数**；此前在蒙版编辑状态下这些面板读全局、写蒙版死字段，
  表现为"拖动后回弹、无任何效果"（蒙版本来就不参与几何调整）。

**CPU 参考实现对齐（C13）** — `pipeline.rs`

- `apply_reference` 从"曝光 + 旧对比度（pivot 0.5、系数 1+c）"改为与着色器
  `pipeline_base` 前四步同公式：**Bradford 白平衡矩阵 → 曝光 → 对比度
  （正半轴 1+1.5a / 负半轴 1/(1−1.5a)，pivot 0.18）**，并注明覆盖范围
  （色调分区 / 曲线 / HSL / 分级不在内）；
- 回归：`negative_contrast_matches_shader_and_does_not_invert`、
  `white_balance_reference_warms_and_cools`、
  **`gpu_base_stage_matches_cpu_reference`**（GPU 与 CPU 参考在
  白平衡 + 曝光 + 对比度组合下逐通道 ≤2/255）。

**导出细节（C15）** — `export.rs`、`win_ui`

- WebP 现在写回 **ICCP 块**：重写 RIFF 容器为扩展格式（VP8X + ICCP + 原图像块，
  去掉旧的扩展头并回填长度），P3 / AdobeRGB / ProPhoto 导出不再丢配置
  （回归测试用独立解码器读回 ICC 并校验图像仍可解码）；
- 全分辨率导出改为**默认开启**（关闭时只导出 2048px 代理，历史默认值易误用）；
- WebP 质量参数不生效这件事在界面与文档中标明（`image` crate 的 WebP 编码器
  只有无损模式）；
- 仍待办：C15b（TIFF / HEIC / WebP 的 EXIF / XMP 写回）、
  C15c（有损 WebP 需引入额外编码器）。

### 1.12 元数据写回 + 16bit 输入 ICC + RAW 色彩路径（第十阶段）

**WebP 元数据写回（C15b 部分）** — `export.rs`

- WebP 导出改为**一次重写 RIFF 容器**：VP8X 扩展头（按需设置 ICC / EXIF / XMP 标志）
  + ICCP + EXIF + XMP 块 + 原图像块；元数据提取抽成 `extract_source_metadata()`
  供 WebP 与 JPEG/PNG 共用（XMP 从 APP1 段剥离为裸包）；
- 独立解码器验证：读回 ICC（`acsp` 签名）与 EXIF 方向（Orientation=6 → `Rotate90`），
  图像仍可正常解码；
- 仍待办：TIFF（IFD 标签写入）与 HEIC（libheif 容器元数据，库本身提供
  `add_exif_metadata` / `add_xmp_metadata`，需要改造 `export_heic` 签名）——C15b 收窄为这两项。

**16bit 输入 ICC（C6b）** — `color.rs`、`decode.rs`

- 新增 `convert_icc_to_srgb_rgba16()`：用 lcms 的 `RGBA_16` 像素格式整段转换，
  切片重解释走 `bytemuck`（零拷贝；该依赖本就在依赖树内，提为直接依赖）；
- `apply_input_icc` 覆盖 `Rgb16` / `Rgba16`：16bit TIFF / PNG / 高比特输入
  带非 sRGB 配置时不再被当成 sRGB；
- 回归：16bit 通路与 8bit 通路结果一致（±2/255）。

**RAW 色彩路径（C6c）** — `decode.rs`

- 显式调用 `set_use_camera_matrix(true)`（LibRaw 默认即 3，写明意图避免上游默认变化），
  并在 `decode_raw` 文档里写明当前路径：相机白平衡 + 相机内嵌色彩矩阵 → sRGB 16bit；
- **阻塞说明**：rsraw 0.1.1 未暴露 `output_color` / `user_qual` / 高光模式等参数，
  也没有 DCP 解析——"相机标准 / 中性"等相机匹配风格需要换 LibRaw 绑定或引入
  DCP 引擎，C6c 保留在台账。

### 1.13 HEIC 元数据 + 镜头校正 f32（第十一阶段）

**HEIC 元数据（C15b 部分）** — `export.rs`

- `export_heic` 增加 exif / xmp 参数：编码得到 `ImageHandle` 后调用 libheif 的
  `add_exif_metadata` / `add_xmp_metadata` 再写盘；元数据提取移到编码之前
  （HEIC 必须编码时进容器，其余格式编码后写回）；
- 回归：带 EXIF 的源 + `MetadataPolicy::Keep` 导出 HEIC 后仍能正常解码
  （`--features heif` 下实测通过）；
- 仍待办：TIFF（需要在 IFD 里写 ICC 34675 / EXIF 34665，`image` 的 TIFF 编码器
  不支持自定义标签，需直接改用 `tiff` crate 的 `write_tag` 或自写编码路径）。

**镜头校正 f32（C18）** — `lens_db.rs`、`win_ui`

- 新增 `LensCorrection::apply_f32()`：与 8bit 版同一畸变 / 色差 / 暗角模型
  （lensfun `apply_color_modification_f32`），在线性 f32 域重采样与增益；
- 全分辨率导出改为**每块线性域校正**（不再先把整图降成 8bit、也不再有"有配置文件
  就退回 8bit 通路"的分支）；1:1 视图此前直接显示**未校正**的全分辨率画面，
  现在同样应用配置文件校正；
- 回归：`f32_profile_correction_matches_8bit`（畸变 + TCA + 暗角，两通路 ≤4/255）；
- 仍待办（C18b）：校正仍是 CPU 计算，45MP 导出逐块做 f32 校正有明显耗时，
  彻底方案是把 lensfun 模型搬进 GPU 几何趟。

### 1.14 提交合并 + 开尔文显示 + TIFF 元数据调研（第十二阶段）

**提交合并（C14b 部分）** — `gpu_pipeline.rs`

- 合并原则：只有"同一 encoder 内各趟共用 buffer、且期间不改写"才合并：
  - 几何趟 + 基础调色（几何写 `geometry_params_buffer`、base 读 `params_buffer`，互不冲突）；
  - 全局细节 + 编码（同一份参数、不同管线）；
  - 蒙版基础调色 + 细节混合（该蒙版不需要细节模糊时；需要模糊时仍须先提交 base 供模糊读取）。
- 效果：无蒙版 5 → 3 次 `queue.submit`，只调曝光 / 颜色的蒙版 3 → 2 次；
  带模糊的蒙版维持原样。像素结果由 27 个 GPU 用例逐像素校验不变，帧时间不变
  （8.31 / 22.89 / 5.71 / 10.12 ms）。
- 剩余（C14b 保留）：把 blur 参数与每蒙版参数改成 uniform **动态偏移**后，
  可把整帧完全合并成 1 次提交——收益约 1–2%（实测提交开销被 GPU 时间淹没），
  需要改动所有 bind group 布局，属低优先级。

**开尔文显示（C3b）** — `win_ui`

- 色温滑杆标签显示对应开尔文（`white_balance::relative_to_kelvin`：0 = 5500K、
  1 单位 = 55K）；蒙版编辑时显示"全局 + 增量"的合成值。

**TIFF 元数据调研（C15b）**

- 结论：**需要自写编码路径**。`image` 的 TIFF 编码器不支持自定义标签；
  `tiff` crate 的 `ImageEncoder` 只暴露写数据 / `finish`，`write_tag` 在低层
  `DirectoryEncoder` 上（要自己维护 strip offsets / byte counts），ExifIFD 还需要
  `extra_directory()` 写出后再把指针写回图像 IFD。本轮不动代码，台账保留（做法已写明）。

### 1.15 界面模块化 + 效果 / 颗粒 / 暗角面板（第十三阶段，非算法）

- 右侧栏拆成 **编辑 / 蒙版 / 剪切** 三个模块，模块按钮（放大）固定在最右侧，
  左侧显示当前模块面板（2026-10-06 晚按用户要求改为：**三个按钮横排在右栏底部**，
  位于调色面板下方）；直方图与照片信息固定在栏顶，三个模块共用；
- 「编辑」模块的基本面板按 Lightroom 分组：白平衡 / 色调 / 偏好；
  **纹理 / 清晰度移入「效果」**（与去雾同组）；
- **颗粒**（数量 / **大小** / **粗糙度**）与**暗角**（数量 / **中点** / **羽化**）
  各自独立成面板；
- 新增参数与实现：
  - 参数模型 `EffectParams` 增加 `grain_size` / `grain_roughness` /
    `vignette_midpoint` / `vignette_feather`（手写 `Default`：中点 / 羽化 / 粗糙度默认 50，
    旧预设缺字段时按默认值加载，观感与改造前一致）；
  - 着色器：颗粒用**值噪声**（格子哈希 + 平滑插值，大小 0→1px、100→约 8px；
    粗糙度在平滑噪声与逐像素哈希之间插值）；暗角用 `smoothstep(start, start+width, r)`，
    中点控制作用范围、羽化控制过渡宽度（默认 50/50 与旧固定 0.55→1.0 近似）；
  - uniform 参数区 104 → 108 floats（27 个 vec4），新增槽位 104..107；
  - 蒙版局部增量合成与预设强度混合都覆盖了新字段；
- 回归：`gpu_grain_and_vignette_subparams_change_output`（四个子参数都要产生可测差异 +
  中点越小压暗范围越大）；分块一致性用例仍覆盖颗粒与暗角（新参数默认值下逐像素对齐）。

**左栏图标列 + AI 面板（2026-10-06 晚）**

- 左栏改为"竖排图标列 + 内容区"：**图库 / 预设 / 历史 / 导出 / AI** 五项，
  选中项高亮（图标用几何字形，不依赖图标字体）；
- 新增 **AI 面板**：模型 / 运行库状态、缺失模型下载、高质量主体模型下载、
  打开模型目录，以及主体 / 天空 / 背景 / 人物 / 物体的**快捷创建**（创建后自动切到
  「蒙版」模块）；
- 蒙版面板里原来的模型状态与下载段收敛成一行提示 + 「AI 模型 / 下载 →」跳转按钮
  （下载入口统一到 AI 页）；
- 右栏模块工具条保持在右栏底部（见上一条）。

## 2. 验证结果

全部在 `packaging/windows/dev-env.ps1`（GNU 工具链）环境下执行：

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo test -p fdxmp-core` | 93 通过（无 gpu 特性，gpu 相关单测被条件编译排除） |
| `cargo test -p fdxmp-core --features gpu` | 101 单元 + 5 解码 + 27 GPU + 4 预设，全通过 |
| `cargo test -p fdxmp-core --features gpu,heif` | 102 单元 + 6 解码 + 27 GPU + 4 预设，全通过（含 HEIC 元数据用例） |
| `cargo test --workspace` | 全通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 警告 |
| `cargo clippy -p fdxmp-core --all-targets --features gpu -- -D warnings` | 0 警告 |

GPU 用例包含以下新增回归：
`gpu_negative_contrast_does_not_invert`（对比度不反相）、
`gpu_empty_mask_keeps_global_edits`（零增量蒙版不抹全局）、
`gpu_detail_reference_shares_exposure`（细节参考与曝光同域）、
`gpu_hsl_bands_cover_all_hues`（色带无空洞、权重和为 1）、
`gpu_tone_zones_are_separated`（色调分区互不污染、端点可抬黑 / 压白）、
`gpu_linear_upload_matches_srgb_upload`（线性 f32 源上传一致性）、
`gpu_white_balance_warms_and_cools`（色适应方向）、
`gpu_grading_preserves_luminance_and_zones`（分级亮度保持 + 分区隔离）。

## 3. 未修复问题台账（未动代码）

优先级：P1 = 影响色彩正确性/必须尽快；P2 = 影响观感或性能；P3 = 工程/体验完善。

| # | 位置 | 问题 | 影响 | 建议 | 优先级 |
| --- | --- | --- | --- | --- | --- |
| C16 | `docs/AI_MASKS.md` | AI 蒙版仅 Windows/ONNX Runtime，移动端推理未接入 | 与"移动端对标 Lightroom"的规格有差距 | 各平台接 Core ML / LiteRT / XEngine Kit | P8（阶段 7） |
| C6c | `decode.rs::decode_raw` | RAW 无相机 ICC/DCP（现为相机白平衡 + 内嵌矩阵 → sRGB）；**rsraw 0.1.1 未暴露** `output_color` / `user_qual` / 高光模式，也无 DCP 解析 | 厂商色彩 / 肤色与 Lightroom 有差异，无法选"相机标准 / 中性" | 换用可配置的 LibRaw 绑定（暴露 `output_color`）或引入 DCP 引擎后再接相机配置文件 | P2（受依赖阻塞） |
| C3c | `decode.rs::decode_raw` | 未读取 RAW 的 as-shot 白平衡（rsraw 未暴露该字段），开尔文以固定 5500K 为参考 | 与 Lightroom 的绝对开尔文刻度有常量差；不同相机的基准白点不同 | 换用可读 as-shot WB 的 LibRaw 绑定，或在元数据里记录 shot WB | P3（受依赖阻塞） |
| C14b | `gpu_pipeline.rs` 渲染循环 | 已合并无冲突趟（见 1.14），但 blur / 每蒙版参数共用同一 uniform，仍无法整帧合并 | 剩余 3 次左右提交开销（实测被 GPU 时间淹没，属低优先级） | 给 blur 与蒙版参数分配 uniform 动态偏移，整帧合并为单 encoder | P3 |
| C10d | `blur.wgsl` / `gpu_pipeline.rs` | 半径缩放上限 4× / 49 抽头，覆盖到约 12288px 长边 | 更大的图（全景拼接 / 放大后裁剪）细节仍比代理弱 | mip 金字塔（降采样后小核模糊再上采样） | P3 |
| C17c | `decode.rs::display_proxy_linear` | 代理缩放为省内存回到 sRGB 编码域（16bit），仅导出路径是线性域缩放 | 代理与导出的边缘 / 中间调有细微差异 | 分块线性缩放（先分块转线性再缩放拼接） | P3 |
| C15b | `export.rs::preserve_metadata` | 元数据写回覆盖 JPEG / PNG / WebP / **HEIC（已完成）**，仅 TIFF 未做 | TIFF 导出丢拍摄信息与方向 | TIFF 需写 IFD 标签（ICC 34675 / EXIF 34665）：`image` 的 TIFF 编码器不支持自定义标签，改用 `tiff` crate 的 `write_tag` 或自写编码路径 | P2 |
| C15c | `export.rs` WebP 编码 | `image` crate 只有无损 WebP 编码器，质量滑杆不生效（界面已注明） | 无法按质量参数导出有损 WebP（体积偏大） | 引入 `webp` / `libwebp-sys` 等有损编码器 | P3 |
| C18b | `lens_db.rs::apply_f32` / `win_ui` | 镜头配置文件校正仍是 CPU 计算（f32 线性域，逐块做） | 45MP 全分辨率导出 / 1:1 切块时校正耗时明显 | 把 lensfun 畸变 / TCA / 暗角模型搬进 GPU 几何趟（着色器已有手动镜头参数通道） | P3 |

## 4. 建议的后续顺序

1. ✅ 已完成：C1 + C5（细节参考域 + 线性 f16 工作空间，见 1.4）。
2. ✅ 已完成：C2 + C8（色相 / 色调分区重写，见 1.5）。
3. ✅ 已完成：C6 + C5b + C7（输入 / 导出色彩管理闭环 + 线性源上传，见 1.6）。
4. ✅ 已完成：C3 + C4（白平衡色适应 + 颜色分级重做，见 1.7）。
5. ✅ 已完成：C9 + C10（曲线单调样条 + 1:1 分块一致性，见 1.8）。
6. ✅ 已完成：C11 + C17 + C14（部分，见 1.9）。
7. ✅ 已完成：C10c + C17b（细节半径跨分辨率 + 导出 CPU 内存，见 1.10）。
8. ✅ 已完成：C12 + C13 + C15（几何面板写入目标 + CPU 参考对齐 + 导出细节，见 1.11）。
9. ✅ 已完成：C15b（WebP 部分）+ C6b + C6c（见 1.12；C6c 受 rsraw 依赖阻塞）。
10. ✅ 已完成：C15b（HEIC 部分）+ C18（镜头校正 f32，见 1.13）。
11. ✅ 已完成：C14b（部分）+ C3b + C15b 调研（见 1.14）。
12. 剩余项优先级（全部为 P2/P3 或受依赖阻塞）：
    - 画质 / 内存微调：C17c（代理分块线性缩放）、C10d（>12288px 需要 mip）；
    - 性能：C18b（镜头校正 GPU 化）、C14b 完整版（动态偏移单提交）、C15c（有损 WebP）；
    - 受依赖阻塞：C15b（TIFF 需自写编码路径）、C6c / C3c（rsraw 未暴露 DCP / as-shot WB）、
      C16（移动端 AI 运行时）。
