# 智能蒙版（v1.4.0）

日期：2026-10-12
范围：Rust 核心 `fdxmp-core::smartmask` + Windows（ONNX Runtime）+ iOS（Vision）
原则：**只做蒙版，不修图**；双阶段、模型可插拔、安全优先，为 AI 修图与自训练预留接口。

## 1. 能力总览

| 蒙版 | Windows（ONNX） | iOS（Vision，端侧） |
| --- | --- | --- |
| 主体 | U²-Net（`u2netp.onnx`）/ 显著目标 + 连通域 | 前景实例分割（iOS 17+）/ 人物分割回退 |
| 人像 | MODNet 512² + U²-Net 精细抠图 | 人物分割（`.accurate`） |
| 面部 | BiSeNet 19 类（皮肤 / 五官合并） | 人脸关键点椭圆 |
| 皮肤 | BiSeNet 类别 1/8/9/16/17 | 面部椭圆 − 眼 / 眉 / 唇 |
| 头发 | BiSeNet 类别 13 | —（Vision 无此信号，待 CoreML 适配器） |
| 眼睛 / 眉毛 / 鼻子 / 嘴唇 / 牙齿 / 耳朵 | BiSeNet 类别 4·5 / 6·7 / 2 / 11·12 / 10 / 8·9·15 | 人脸关键点（内唇 ≈ 口腔 / 牙齿） |
| 背景 | 主体反向 | 主体反向 |
| 天空 | SegFormer ADE20K（既有 AI 蒙版） | — |

所有蒙版都是 **f32 0..1 权重 + 与原图一致的坐标**，带元数据
（类型、置信度、来源模型、模型版本、生成时间），可羽化、可组合、可缓存。

## 2. 架构

```text
平台提供者 / 模型适配器（可插拔）
  ├─ Windows：smartmask::onnx（SCRFD / MODNet / BiSeNet / U²-Net，ModelSpec 驱动）
  ├─ iOS：Vision（SmartMaskEngine.swift）→ fdxmp_mask_*_submit
  └─ 测试 / 联调：smartmask::mock（确定性 Mock，零模型依赖）
        │  trait：Stage1FaceDetector / Stage1PersonSegmenter
        │        Stage2FaceParser / Stage2PersonMatting / MaskRefiner
        ▼
MaskPipeline（双阶段并行调度）
  ├─ 第一阶段并行：人脸检测 + 人像粗分割（只"看哪里"）
  ├─ 第二阶段并行：人脸对齐→五官解析 ∥ 人像精细抠图（只"看清楚"）
  └─ 人脸对齐（ArcFace 5 点模板）/ 仿射逆变换 / 边界 clamp / 缓存
        ▼
masks:: 共享栅格缓存（按蒙版类型缓存；按请求分辨率自动重采样）
        ▼
渲染 / 叠加 / 缩略图 / 导出（CPU 与 GPU 同源；羽化 / 不透明度 / 反向由核心统一后处理）
```

### 代码索引

| 文件 | 内容 |
| --- | --- |
| `crates/core/src/smartmask/spec.rs` | `ModelSpec` / `ModelRegistry`：模型清单、SHA-256、下载白名单、状态 |
| `crates/core/src/smartmask/mask.rs` | `SmartMask`（羽化 / 形态学 / 组合 / 缩放 / 仿射重采样）、`PolygonMask`、`AffineMap` |
| `crates/core/src/smartmask/traits.rs` | 双阶段 trait、预 / 后处理抽象、`RetouchEngine`、`MaskConsumer` |
| `crates/core/src/smartmask/pipeline.rs` | `MaskPipeline`：并行、缓存、对齐、投影、装配 |
| `crates/core/src/smartmask/mock.rs` | Mock 模型（测试 / UI 联调） |
| `crates/core/src/smartmask/retouch.rs` | `RetouchParams` / `NoopRetouchEngine` / `DataRecorder` / `JsonlRecorder` / 数据集导出 |
| `crates/core/src/smartmask/onnx.rs` | ONNX 适配器（`ai` feature） |
| `crates/core/src/masks.rs` | 共享栅格缓存（`insert_ai_raster` / `cached_ai_raster`） |
| `crates/ffi/src/lib.rs` | `fdxmp_mask_raster_submit` / `fdxmp_mask_polygons_submit` / `fdxmp_mask_raster_ready` / `fdxmp_mask_raster_clear` / `fdxmp_smartmask_models_json` |
| `platforms/ios/Fdxmp/SmartMaskEngine.swift` | Vision 端侧实现 |

## 3. 模型清单与规格（配置驱动，禁止硬编码）

换模型只改 `ModelRegistry`（或 `upsert` 一份 `ModelSpec`）；输入输出结构不同时新增
一个 trait 适配器，上层 `MaskPipeline` 零修改。

| 任务 | 模型 | 文件 | 输入 | 归一化 | 许可证 | 可训练 |
| --- | --- | --- | --- | --- | --- | --- |
| 第一阶段人脸检测 | SCRFD-2.5G（bnkps） | `face_detector_scrfd2p5g_bnkps_fp32.onnx` | 640×640 RGB NCHW | (x−127.5)/128 | Apache-2.0 | 是 |
| 第一阶段人像粗分割 | MODNet | `person_coarse_modnet_fp32.onnx` | 512×512 RGB NCHW | (x/255−0.5)/0.5 | Apache-2.0 | 是 |
| 第二阶段人脸解析 | BiSeNet（face-parsing 19 类） | `face_parsing_quantized.onnx` | 512×512 RGB NCHW | ImageNet mean/std | MIT | 是 |
| 第二阶段精细抠图 | U²-Net 轻量（u2netp） | `u2netp.onnx` | 320×320 RGB NCHW | ImageNet mean/std | Apache-2.0 | 是 |

说明：
- 每个模型固定输入尺寸；文件放模型目录（`FDXMP_MODEL_DIR` → exe 同目录 `models/` →
  `%LOCALAPPDATA%\fdxmp\models`），命名规范 `{task}_{model}_{precision}.onnx`；
- 加载前校验 SHA-256（`ModelRegistry::verified_path`），不匹配直接拒绝；
- 下载只允许白名单域名（hf-mirror / huggingface / github）且必须 HTTPS；
- 换模型步骤：放文件 → 改 `ModelSpec`（哈希 / 输入尺寸 / 归一化 / 阈值 / 输出名）→
  必要时新增适配器 → 跑 `smartmask_models` 集成测试与 `dump_model_io` 诊断。

## 4. 双阶段流程（Windows 实测）

`cargo test -p fdxmp-core --features ai --test smartmask_models -- --nocapture`：

- 全部四个模型加载 + SHA-256 校验通过；
- 真实人像（1024 长边）端到端：检出 1 张人脸（置信度 0.732）→ 人像粗分割覆盖率 0.578 →
  人脸对齐 256² → 五官解析（皮肤 0.431 / 头发 0.443 / 眼睛 0.005 / 眉毛 0.006 /
  鼻子 0.039 / 嘴唇 0.018 / 牙齿 0.015）→ 精细抠图覆盖率 0.457；
- 可视化导出见测试参数 `FDXMP_TEST_OUT`（对齐人脸 + 各部位蒙版 PNG）。

调度：有人脸 → 并行「五官解析 + 精细抠图」；无人脸 → 只走人像路径；
纯磨皮 / 肤色场景可配置跳过精细抠图（`PipelineConfig::skip_matting_without_face`）。

## 5. iOS（随包模型 + Vision 兜底）

### 5.1 主线：随包 ONNX 模型（与 Windows 同一套流水线）

- ONNX Runtime：`ai-static` feature = pyke 预编译静态库（`ort/download-binaries` +
  `ort/coreml`，ORT 1.28 + CoreML 后端），静态链接进 `libfdxmp_ffi.a`，
  App 侧链接 `-framework CoreML -framework Accelerate`（project.yml）；
- 模型（≈123 MB）随 IPA 打包进 `Fdxmp.app/models/`（CI 下载 + `shasum` 校验，
  哈希与 `ModelRegistry` 一致；核心从「可执行文件同级 models/」读取）；
- Swift 调 `fdxmp_smartmask_generate` → 核心 [`runtime`] 跑双阶段流水线
  （SCRFD 人脸检测 / MODNet 人像 / BiSeNet 五官 / U²-Net 抠图）→ 写共享栅格缓存；
  同一张图的多次生成命中流水线缓存（只推理一次算法路径）；
- 运行时同样做 SHA-256 校验；模型缺失 / 类型不支持时返回 `FDXMP_SMARTMASK_FALLBACK`。

### 5.2 兜底：Apple Vision（系统端侧模型，无需打包文件）

`fdxmp_smartmask_generate` 返回 2（模型不可用 / 无对应模型）或推理失败时自动回退：

- `VNDetectFaceLandmarksRequest`（五官关键点）+ 人物分割
  （`VNGeneratePersonSegmentationRequest`）+ 前景实例分割（iOS 17+）；
- 分割类（主体 / 人像）：f32 平面经 `fdxmp_mask_raster_submit` 提交核心缓存；
- 关键点类（面部 / 皮肤 / 眼睛 / 眉毛 / 鼻子 / 嘴唇 / 牙齿）：
  归一化多边形经 `fdxmp_mask_polygons_submit` → 核心扫描线光栅化 + 羽化 + 缓存；
- 渲染 / 叠加 / 导出与 Windows 共用同一套核心后处理与坐标；
- 失败可恢复：无人脸 / 无人像 → 面板提示 + 「重新生成」，不影响其它编辑；
- 切换图片时清空栅格缓存（`fdxmp_mask_raster_clear`），编辑记录恢复后自动补生成。

> 说明：Android / HarmonyOS 当前构建未启用 `ai-static`，`fdxmp_smartmask_generate`
> 直接返回 2（回退平台实现），接口已就位。

## 6. 安全

- 模型加载前校验 SHA-256；只允许白名单来源；拒绝超大文件（> 1 GiB）；
- FFI 提交栅格 / 多边形有尺寸与数值上限（≤ 4096 边、≤ 16M 像素、≤ 64 多边形 /
  4096 点、拒绝 NaN / 非有限坐标）；
- 全部推理本地执行，默认不联网、不上传任何图像 / 蒙版 / 关键点 / 特征；
- 日志与崩溃信息不含图像数据；
- 缓存可一键清理（`fdxmp_mask_raster_clear`）。

## 7. 为 AI 修图与自训练预留

- `RetouchEngine`：`apply(image, mask, params)`，修图模型独立替换，蒙版模块零修改；
  v1.4.0 提供 `NoopRetouchEngine` 占位与参数结构（磨皮 / 肤色统一 / 祛痘 / 换背景 /
  inpainting / relight 等后续接入）；
- `DataRecorder` / `JsonlRecorder`：**用户明确授权后**导出原图 + 蒙版 + 模型版本 + 参数；
  布局兼容 DUTS / CelebAMask-HQ / COCO（`export_dataset`）；默认绝不上传；
- `ModelSpec.trainable` / `training_repo` 标记可微调模型；自训练完成后替换 ONNX +
  更新 `ModelSpec`（递增 `version`）即可无缝接入（缓存键自动失效）。

## 8. 测试

| 层 | 命令 | 覆盖 |
| --- | --- | --- |
| 核心单测 | `cargo test -p fdxmp-core --lib` | 规格 / 哈希 / 白名单、蒙版运算、仿射 / 多边形、双阶段 Mock、缓存、数据导出 |
| 核心（含 AI） | `cargo test -p fdxmp-core --features ai --lib` | 同上 + ONNX 适配器（无模型时跳过） |
| 真实模型集成 | `cargo test -p fdxmp-core --features ai --test smartmask_models` | 四模型加载 / 哈希 / 合成图跑通；`FDXMP_TEST_PORTRAIT` 开启真实人像端到端 |
| 模型诊断 | `-- --ignored dump_model_io` / `probe_segmenter_stats` | 打印模型输入输出名 / 形状 / 输出统计（换模型用） |
| FFI | `cargo test -p fdxmp-ffi` | 栅格提交 / 多边形提交 / 状态 / 叠加层 / 渲染 / 模型清单 |
