# 自转模型（可复现）

## face_parsing_bisenet19_fp32.onnx（人脸解析 19 类）

- **用途**：智能蒙版的皮肤 / 五官 / 头发解析；AI 修图的皮肤作用域。
- **来源**：zllrunning/face-parsing.PyTorch（**MIT**）的 79999_iter.pth（权重）
  + 同名仓库的 `model.py` / `resnet.py`（代码，MIT）。
- **权重下载**（hf-mirror，脚本里已内置）：
  `https://hf-mirror.com/vivym/face-parsing-bisenet/resolve/main/79999_iter.pth`
- **类别顺序**：zllrunning 自有顺序（2/3 眉、4/5 眼、10 鼻、11 口腔、12/13 唇、17 头发……），
  已写进 `ModelSpec.postprocess.class_map`，并通过与线上既有解析模型的交叉 IoU 验证。
- **文件**：`models/face_parsing_bisenet19_fp32.onnx`（≈50.7 MB，固定 512×512，ImageNet 归一化）。
  SHA-256：`2453dcc1592590d1f9f7b064b3031c20f35bca74375445d23ab7f5ad3c38ea2f`
- **重新生成**：
  ```bash
  python -m pip install torch torchvision onnx onnxscript   # CPU 版即可
  python packaging/models/convert-bisenet19.py              # 输出到 models/ 并打印哈希
  ```
  转换后请更新 `crates/core/src/smartmask/spec.rs` 里的 `sha256`。

## 随包策略

- iOS：CI / 本地打包脚本把 `models/*.onnx` 复制进 `dist/ios/models/`，再随 App 打包
  （`Fdxmp.app/models/`）。
- Windows：把需要的文件复制到 exe 同目录 `models/`（或 `%LOCALAPPDATA%\fdxmp\models`）。
