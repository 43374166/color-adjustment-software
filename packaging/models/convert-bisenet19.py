# zllrunning/face-parsing (BiSeNet-19, MIT) → ONNX（固定 512×512，ImageNet 归一化）
import sys

import torch

sys.path.insert(0, r"F:\FD-Room\.tmp\bisenet")
from model import BiSeNet  # noqa: E402

WEIGHTS = r"F:\FD-Room\.tmp\bisenet\79999_iter.pth"
OUTPUT = r"F:\FD-Room\dist\windows\models\face_parsing_bisenet19_fp32.onnx"

net = BiSeNet(n_classes=19)
state = torch.load(WEIGHTS, map_location="cpu", weights_only=False)
if isinstance(state, dict) and "state_dict" in state:
    state = state["state_dict"]
missing, unexpected = net.load_state_dict(state, strict=False)
print("missing:", len(missing), "unexpected:", len(unexpected))
if missing:
    print("missing keys sample:", missing[:5])
net.eval()

dummy = torch.randn(1, 3, 512, 512)
torch.onnx.export(
    net,
    dummy,
    OUTPUT,
    input_names=["input"],
    output_names=["output", "output16", "output32"],
    opset_version=16,
    do_constant_folding=True,
)
print("exported ->", OUTPUT)

# 快速自检：同一张输入两次输出一致（eval 确定性）。
with torch.no_grad():
    a = net(torch.zeros(1, 3, 512, 512))[0]
    b = net(torch.zeros(1, 3, 512, 512))[0]
    print("determinism:", bool(torch.allclose(a, b)))
    print("output shape:", tuple(a.shape), "argmax classes:", int(a.argmax(1).max()) + 1)
