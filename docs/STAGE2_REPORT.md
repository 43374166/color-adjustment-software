# 阶段 2 报告：色彩管线（GPU compute）

日期：2026-10-03
范围：15 项调色、曲线、HSL、颜色分级、细节的 GPU 实现与实时预览

## 1. 已完成

### 1.1 GPU 色彩管线（`crates/core/src/shaders/color_pipeline.wgsl` + `gpu_pipeline.rs`）

三趟 compute dispatch：

| 趟次 | 入口 | 作用 |
| --- | --- | --- |
| 1 | `cs_blur`（水平） | 可分离高斯模糊 → `blur_a` |
| 2 | `cs_blur`（垂直） | → `blur_b`（清晰度 / 纹理 / 锐化 / 降噪的局部参考） |
| 3 | `cs_main` | 完整调色管线 → 输出纹理（sRGB 编码的 `rgba8unorm`） |

颜色空间链路（与 egui 用户纹理约定一致）：

```text
源 rgba8unorm-srgb ──读取即线性──► 线性处理（f32）
   ├─ 白平衡 / 曝光 / 高光阴影 / 对比度 / 参数曲线
   ├─ 曲线 LUT（gamma 空间，主曲线 × RGB 通道曲线合成）
   ├─ HSL 8 色带（红橙黄绿青蓝紫洋红，±30° 平滑权重）
   ├─ 颜色分级（阴影 / 中间调 / 高光 + 混合 / 平衡）
   ├─ 自然饱和度 / 饱和度
   ├─ 细节：纹理 / 清晰度 / 锐化 / 去雾（基于模糊参考）
   ├─ 降噪：亮度 + 颜色（色度与亮度分离处理）
   └─ 暗角 / 颗粒（哈希噪声）
        ▼
   linear → sRGB 编码 → rgba8unorm（egui 直接显示）
```

- **参数打包**：80 个标量（15 滑块 + 锐化 4 + 降噪 6 + 分级 11 + 参数曲线 6 + HSL 24 + 分级 9 + 元信息）
  + 256×4 曲线 LUT，共 4416 字节 uniform。
- **曲线**：主曲线与 RGB 通道曲线在 CPU 侧合成为 256 项 LUT（线性插值、严格过控制点），
  GPU 侧 O(1) 查表。
- **HSL**：Lightroom 风格色带中心（0°/30°/60°/120°/180°/240°/280°/320°），
  色相距离取环形最短距离，±30° 权重平滑过渡。
- 关键修复（自测发现）：曲线插值最初用 smoothstep 导致"恒等曲线"变成 S 曲线且
  主/通道两次叠加；HSL 的 `smoothstep(edge0>edge1)` 属于 WGSL 未定义行为 —— 均已修正。

### 1.2 实时预览（Windows UI）

- 复用 eframe 的 wgpu 设备/队列，输出纹理通过 `egui_wgpu::Renderer::register_native_texture`
  直接交给 egui 绘制（零拷贝，不经过 CPU）。
- 滑块拖动 → 立即重跑管线（实测 1-3 ms）；**拖动过程不写历史，松手才提交一步**
  （`Engine::set_params_live` + `commit_history`，一次拖动 = 一条历史）。
- 直方图在参数停止变化 300 ms 后回读 GPU 输出刷新（避免每帧回读）。
- 状态栏显示 `GPU x.xx ms`；GPU 不可用时自动回退 CPU 纹理显示路径。

### 1.3 headless 渲染命令

```powershell
cargo run -p fdxmp-cli --features gpu -- render in.png out.png `
  --exposure 1.0 --contrast 25 --saturation 30 --temperature 20 --vignette 45 --clarity 30
```

用于回归对比与报告素材，后续导出管线复用同一实现。

## 2. 测试结果

环境：GTX 1050 Ti（Vulkan 后端），Rust 1.99 GNU 工具链。

| 检查项 | 命令 | 结果 |
| --- | --- | --- |
| 核心全特性测试 | `cargo test -p fdxmp-core --features gpu,raw,heif` | ✅ 35 单元 + 5 解码集成 + 7 GPU 集成 + 4 预设集成 |
| 工作区测试 | `cargo test --workspace` | ✅ 全绿（低并发运行，避免整机负载过高） |
| 静态检查 | `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 0 警告 |
| 全特性 clippy | `cargo clippy -p fdxmp-core -p fdxmp-cli -p fdxmp-win-ui --features gpu,raw,heif --all-targets -- -D warnings` | ✅ 0 警告 |
| 格式 | `cargo fmt --all -- --check` | ✅ 通过 |

GPU 集成测试覆盖（`crates/core/tests/gpu_pipeline.rs`）：

| 测试 | 断言 |
| --- | --- |
| `gpu_identity_roundtrip` | 恒等参数下最大通道偏差 ≤ 4/255（实测 0） |
| `gpu_debug_gray_mapping` | 0/16/…/255 十级灰阶一一对应（实测完全一致） |
| `gpu_exposure_doubles_linear_light` | 曝光 +1EV 线性倍率 ≈ 2（容差 0.25） |
| `gpu_saturation_minus_hundred_is_gray` | 饱和度 -100 后 RGB 通道差 ≤ 6 |
| `gpu_vignette_darkens_corners` | 四角比中心暗 20 以上 |
| `gpu_curve_and_hsl_change_output` | S 曲线提亮中点；HSL 蓝色饱和度 -80 降低蓝块彩色度 |
| `gpu_preview_frame_time_smoke_test` | 2048×1365 单帧 < 33 ms（30fps 目标） |

## 3. 性能

| 项目 | 结果 |
| --- | --- |
| 显示代理 2048×1365（含两趟模糊 + 主管线） | **5.4 ms/帧 ≈ 185 fps** |
| 1600×1067 单帧（CLI，含回读） | 1.7 ms |
| 参数打包 + 曲线 LUT（CPU，每次参数变化） | 微秒级（256×4 次插值） |

结论：实时预览目标（≥30fps）有 6 倍以上余量。

## 4. 界面

![阶段 2：GPU 实时调色（曝光 2.30 / 清晰度 40，GPU 1.67 ms）](screenshots/stage2-gpu-live-edit.png)

右栏 15 项滑块直接驱动 GPU 管线；状态栏可见帧耗时；直方图随编辑刷新。

## 5. 遗留问题（阶段 3/4 输入）

1. **细节面板的从属参数**（锐化半径/细节/蒙版、降噪的细节/对比度/平滑度）暂未参与计算，
   当前只使用主参数；阶段 3 与蒙版一起完善（蒙版需要类似的局部权重通道）。
2. **曲线为线性插值**：可升级为单调三次样条（Lightroom 手感），不影响现有数据结构。
3. **清晰度/纹理为固定半径高斯**：Lightroom 使用多尺度局部对比；阶段 7 性能优化时再升级。
4. **GPU 直方图**：当前仍是"GPU 渲染 + CPU 回读抽样"，阶段 7 改为 compute 统计。
5. **蒙版（阶段 3）**、预设（阶段 4）、导出（阶段 4）尚未接入管线。
6. Windows 桌面 app 需要显式 `--features gpu,raw,heif` 才能获得全部能力
   （workspace 默认构建不带 gpu：`cdylib` 桥接在 MinGW 下会因 wgpu 符号过多触发
   导出序号溢出，已通过把 GPU 设为 UI 可选特性规避）。

## 6. 结论

阶段 2 目标达成：15 项调色 + 曲线 + HSL + 颜色分级 + 细节全部在 GPU compute 中实现，
桌面端实时预览可用（185 fps 余量），数学正确性由 7 项 GPU 集成测试兜底，
历史记录与直方图交互打磨完成。可以进入阶段 3（蒙版系统：画笔 / 线性 / 径向 /
颜色范围 / 亮度范围 / AI 蒙版）。

