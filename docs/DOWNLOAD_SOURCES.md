# 下载源与镜像可用性（2026-10-03 实测）

本机网络对部分境外站点不可达；以下为实测结论与替代源，供后续阶段（打包、移动端、AI）使用。

## 1. 可达（可换用）

| 用途 | 源 | 备注 |
| --- | --- | --- |
| NSIS 源码 | `https://gitee.com/mirrors/NSIS.git` | git clone 成功；本地构建流程见 `packaging/windows/build-nsis.ps1` |
| Python 便携版 | `https://mirrors.huaweicloud.com/python/<版本>/` | 3.11/3.12 各版本 embed zip 均可下载 |
| PyPI 包 | 阿里云 `https://mirrors.aliyun.com/pypi/simple/`、腾讯云、华为云 | 清华 PyPI 对脚本请求有反爬（403），浏览器可用 |
| Rust crates | crates.io（cargo 直连）| 正常；如需加速可用 rsproxy.cn |
| Android SDK / NDK | 腾讯云 `https://mirrors.cloud.tencent.com/AndroidSDK/`（tools zip 306 MB 实测 200；NDK r26d/r27c 200）；`dl.google.com` 直连也可达 | 可安装到 E:/F: |
| DevEco Studio | `https://developer.huawei.com/consumer/cn/deveco-studio/` | 页面 200；实际下载可能需华为账号登录 |
| AI 模型 | `https://hf-mirror.com/`、`https://www.modelscope.cn/` | HuggingFace 直连不可达 |
| ONNX Runtime / NuGet | `https://api.nuget.org/v3/index.json` 可达；Rust `ort` 预编译 CDN `https://cdn.pyke.io/` 可达 | Windows AI 推理运行时可行 |
| 7-Zip | `https://www.7-zip.org/`（7zr.exe / 旧版 extra 包） | 新版 extra 包在 GitHub，走不了 |

## 2. 不可达（需替代或绕行）

| 站点 | 现象 | 替代 |
| --- | --- | --- |
| `github.com` / `raw.githubusercontent.com` | 超时 | Gitee 镜像；`cdn.jsdelivr.net` 可读取 GitHub 仓库文件 |
| SourceForge（NSIS 等） | 403 | Gitee 源码自建（已验证） |
| `huggingface.co` | 超时 | hf-mirror.com / ModelScope |
| Adobe 帮助页 | 403 | 使用本地知识 / 其他资料 |
| `ghfast.top` / `ghproxy.net` / `github.moeyy.xyz` 等 GitHub 代理 | 超时 | 同上 |

## 3. 对本项目剩余工作的意义

1. **P6 打包**：NSIS 已由 Gitee 源码 + 镜像 Python/scons 自建完成（见 `PACKAGING_REPORT.md`）。
2. **P7 移动端**：Android SDK/NDK 可从腾讯云镜像或 Google 直连下载，可安装到 E:/F: 后继续设备 Tab → 编辑 Tab → 编辑界面；
   HarmonyOS 的 DevEco Studio 页面可达，但安装包可能需要用户在华为账号下下载。
3. **P7 AI 蒙版**：模型走 hf-mirror / ModelScope；Windows 端推理运行时走 NuGet（onnxruntime）或 `ort` 的 pyke CDN 预编译包。

> 2026-10-03 实测落地：Windows AI 蒙版已完成 —— onnxruntime.dll 来自 NuGet
> `Microsoft.ML.OnnxRuntime 1.28.0`（win-x64），模型来自 hf-mirror
> （U2NetP / SegFormer-B0-ADE / face-parsing，可选 BiRefNet-lite），
> 细节见 `docs/AI_MASKS.md`。
