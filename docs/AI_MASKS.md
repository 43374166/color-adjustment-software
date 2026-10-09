# Windows AI 蒙版（阶段 7，2026-10-03）

## 1. 能力

| 蒙版 | 模型 | 说明 |
| --- | --- | --- |
| 主体 / 背景 / 物体（点按） | U2NetP（默认，Apache-2.0，4.4 MB） | 显著目标分割；背景为反向；物体 = 显著掩膜中点按位置所在连通域 |
| 主体（高质量，可选） | BiRefNet-lite（MIT，213 MB） | 更准；CPU 慢、内存占用高，需 `FDXMP_AI_SUBJECT=hq` 启用 |
| 天空 | SegFormer-B0（ADE20K，14.6 MB） | 取 sky 类别（ADE20K #2） |
| 人物：面部 / 身体 / 皮肤 / 头发 / 衣服 | face-parsing（BiSeNet，85 MB 量化） | 19 类人脸解析，按类别映射到部位 |

推理运行时：**ONNX Runtime**（`ort` crate，`load-dynamic` 运行时加载 `onnxruntime.dll`），
与规格中「Windows AI：ONNX Runtime」一致。

## 2. 资源位置与获取

| 资源 | 位置 | 获取方式 |
| --- | --- | --- |
| `onnxruntime.dll`（15.1 MB） | exe 同目录，或 `FDXMP_ORT_DLL` 指定 | NuGet `Microsoft.ML.OnnxRuntime 1.28.0` 的 `runtimes/win-x64/native/onnxruntime.dll`（已放入 `.tmp/tools/onnxruntime/`） |
| 模型（默认三件 ≈ 104 MB） | `<exe目录>/models` 或 `%LOCALAPPDATA%\fdxmp\models`（`FDXMP_MODEL_DIR` 可覆盖） | 应用内「下载缺失模型」（hf-mirror 镜像）或 `packaging/windows/download-models.ps1` |
| 高质量主体模型（可选 +213 MB） | 同上（文件名 `birefnet_lite.onnx`） | 应用内「获取高质量主体模型」按钮 |

网络受限说明见 `docs/DOWNLOAD_SOURCES.md`（HuggingFace 直连不可达，统一走 hf-mirror）。

## 3. 交互

- 蒙版面板新增 AI 按钮：主体 / 天空 / 背景 / 人物（面部·身体·皮肤·头发·衣服）/ 物体；点击即创建蒙版并后台推理（首次约 0.5–5 秒）。
- 面板顶部显示运行库与模型状态、下载按钮、模型目录入口。
- 物体蒙版：创建后在画面上点按目标即可重新推理（取点按处连通域）。
- 生成结果写入缓存（键 = 蒙版参数 + 图像尺寸），切换图片自动失效并重算。

## 4. 性能（GTX 1050 Ti 机器，CPU 推理，合成 256×192 输入）

| 模型 | debug | release |
| --- | --- | --- |
| 主体 / 背景 / 物体（U2NetP 320²） | 0.35–0.95 s | 0.26–0.46 s |
| 天空（SegFormer-B0 512²） | 1.0 s | 0.5 s |
| 人物部位（face-parsing 512²） | 6.0 s | 4.5 s |

BiRefNet-lite（1024²，可选）在 debug 下单张需数分钟、峰值内存约 3.9 GB —— 8 GB 机器不建议开启；
release 预计仍需 10–60 s 与 1–2 GB 内存，故默认关闭。

## 5. 许可

- U2NetP：Apache-2.0（BritishWerewolf/U-2-Netp）
- BiRefNet-lite：MIT（onnx-community/BiRefNet_lite-ONNX）
- SegFormer-B0 ADE：Apache-2.0（lquint 转换自 nvidia）
- face-parsing：MIT（Xenova/face-parsing，源自 jonathandinu/face-parsing）
- 早前试用的 RMBG-1.4 为**非商用**许可，已弃用并删除模型文件。

## 6. 测试

- 单元：栅格缩放、连通域、部位映射、logits→概率、缓存命中（`cargo test -p fdxmp-core --features ai --lib`）。
- 集成：`cargo test -p fdxmp-core --features ai --test ai_models`（真实模型推理，缺模型自动跳过）。
- 高质量模型：`-- --ignored` 显式运行。

