# Windows 打包报告（2026-10-03）

## 1. 产物（dist/）

| 产物 | 大小 | 说明 |
| --- | --- | --- |
| `fdxmp-setup.exe` | 28.5 MB | **NSIS 安装向导**（用户级、免管理员）：目录选择页、可选任意盘（含 F:）、桌面/开始菜单快捷方式、卸载器、写入 HKCU 卸载项（显示在"应用和功能"） |
| `fdxmp-setup-sfx.exe` | 23.0 MB | IExpress 自解压兜底包（解压后运行 `install.ps1`，装到 `%LOCALAPPDATA%\Programs\fdxmp`） |
| `fdxmp-0.1.0-win64.7z` | 17.7 MB | 绿色版分发包（exe + 22 个 DLL + install.ps1 + README） |
| `windows\fdxmp-win-ui.exe` | 23.7 MB | release + LTO（features: gpu/raw/heif），可直接双击（含同目录 DLL） |

安装/卸载已实测：静默安装（`/S /D=<目录>`）装出 25 个文件 + 快捷方式 + 注册表项；
静默卸载后目录、快捷方式、注册表项全部清除。

## 2. 打包流程（`packaging/windows/build.ps1`）

1. `cargo build --release -p fdxmp-win-ui --features gpu,raw,heif`；
2. 复制 exe → `deploy-runtime.ps1` 递归收集 MinGW / libheif / 编解码器 DLL；
3. 复制 `install.ps1`（UTF-8 BOM）与 `README.txt`；
4. 安装包：优先 `.tmp/tools/nsis/makensis.exe`（本地从源码构建）→ PATH 上的 makensis → 都没有时用 IExpress 自解压包；
5. 7z 分发包：使用 `.tmp/tools/7zip` 的 7-Zip（无系统安装）。

## 3. NSIS 工具：从源码构建（网络受限下的换源方案）

官方 NSIS 下载（SourceForge / GitHub）不可达，改用镜像链路自建：

| 依赖 | 来源 | 结果 |
| --- | --- | --- |
| NSIS 源码 | `https://gitee.com/mirrors/NSIS.git` | ✓ git clone 成功 |
| 便携 Python 3.12 | 华为云镜像 `mirrors.huaweicloud.com/python/` | ✓ |
| scons 4.8.1 | 阿里云 PyPI 镜像 | ✓ |
| MinGW gcc/g++/windres | 本机已有（dev-env.ps1） | ✓ |

另需 3 处 GCC 16 兼容补丁（`offsetof` 常量表达式 ×2、`bool` 自增 ×1），
全部流程固化在 `packaging/windows/build-nsis.ps1`（幂等，可重复执行）。
构建产物：`.tmp/tools/nsis/`（`Bin\makensis.exe` + 2 个运行时 DLL、`Stubs\`、`Include\`；
makensis 以「exe 上级目录」作为 NSISDIR，故 exe 放在 `Bin\` 下），
验证：`makensis /VERSION` → `v03-Oct-2026.cvs`，无需设置环境变量即可直接使用。

## 4. 冷启动测量（release，GTX 1050 Ti / 8 GB / F 盘 HDD 分区）

| 场景 | 结果 |
| --- | --- |
| 窗口出现（首次，文件缓存冷） | 0.94 s |
| 窗口出现（热） | 0.08 s |
| 界面可用（窗口 + 工作集 > 100 MB，含 GPU 初始化） | 首次 3.61 s；热启 0.84 / 0.87 s |

热启满足规格 < 2 s；冷缓存首次偏慢，后续可做延迟初始化或建议安装到 SSD。

## 5. 已知限制

1. 安装包没有数字签名，Windows SmartScreen 可能提示"未知发布者"。
2. 移动端与 AI 相关下载见 `docs/DOWNLOAD_SOURCES.md`（GitHub / SourceForge / HuggingFace 直连不可达，但 Gitee / 腾讯云镜像 / hf-mirror / ModelScope / NuGet 可用）。
