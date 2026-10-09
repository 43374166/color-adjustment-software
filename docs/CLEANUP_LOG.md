# 进程清理记录（2026-10-03）

用户指令：清理电脑中运行着的没用的进程，防止卡顿。

## 1. 已关闭（安全性：可重启、无未保存数据）

| 进程 | 数量 | 说明 |
| --- | --- | --- |
| `msedge.exe` 后台实例 | 7 | 无可见窗口的 Edge 后台（`--no-startup-window`）+ 渲染/网络/GPU 子进程，约 450 MB |
| `wps.exe` / `wpscloudsvr.exe` / `promecefpluginhost.exe` | 6 | WPS 后台云同步与首页插件（无打开文档，约 63 MB） |
| `M365Copilot.exe` + `msedgewebview2.exe` | 7 | Office Hub 自启动（约 27 MB） |
| `ubuntu-pro-agent(-launcher).exe` | 2 | Ubuntu Pro 后台代理 |

合计 23 个进程，工作集约 540 MB，释放物理内存约 370 MB；3 秒后复查无自动重启。

## 2. 未关闭（原因）

| 进程 | 原因 |
| --- | --- |
| ChatGPT / codex.exe / cua_node | Codex 自身宿主，关闭会中断当前任务 |
| chrome.exe（24 进程） | 启动于 13:11:44，前台活动且有网络请求，判断为正在使用（可能是用户远程或本机操作），未动 |
| MySQL / MSPCManager / Defender / Windows Search | 系统或用户工具，可能被其他项目使用，保持运行 |
| Desktop-Monitoring-v0.1.2 | 用户自己的桌面小插件（CPU ~8%），保留；如不需要可退出 |

## 3. 自启动项提示（未改动，供用户决定）

启动项：OneDrive、微信、Quark 更新器、剪映托盘、QQ 音乐、桌面临时小插件、Edge 自动启动、
AweSun（向日葵，服务已停止）。

其中 Quark 更新器、QQ 音乐、剪映托盘、M365 Copilot 属于常驻非必要项，可在需要更快的开机速度时禁用。

## 4. 磁盘清理（2026-10-03，第二轮）

用户指令：清掉 C 盘旧的东西。原则：只删可再生的缓存/构建残留，不碰程序本体、用户文件与构建工具链。

结果：C 盘剩余空间 **11.91 GB → 44.02 GB（净回收约 32.1 GB）**。

| 目标 | 释放 | 说明 |
| --- | --- | --- |
| `%LOCALAPPDATA%\Temp` 旧残留 | ~18.3 GB | 2004 项删除（`fr-target`、`fr-exfat2`、`fr-check`、`gocache-build` 等文件找回 / Go 构建残留）；22 个被运行中程序占用的文件跳过；剩余 48.7 MB |
| `C:\Users\Lico\.gradle` | 3.73 GB | Gradle 已改用 `F:\Android\gradle-home`，旧缓存可删 |
| Yarn `Cache` | 1.94 GB | 可再生 |
| npm-cache | 1.90 GB | 可再生 |
| pnpm + pnpm-cache | ~2.42 GB | store 可再生（删除后首次安装重新下载） |
| go-build | 1.41 GB | Go 构建缓存，可再生 |
| ms-playwright | 0.98 GB | 浏览器二进制缓存，需要时重新安装 |
| NuGet 缓存 | 0.62 GB | 可再生（不含 `.nuget\packages` 全局包） |
| pip cache | 0.57 GB | 可再生 |
| Pub（Flutter/Dart） | 0.58 GB | 可再生 |
| Quark 缓存 | ~0.5 GB | Default\Cache、Code Cache、GPUCache、Service Worker、shader/metrics 等（保留书签/登录数据） |
| Edge / Chrome Cache | ~0.3 GB | 浏览器运行中，部分文件锁定；关闭浏览器后可再清一轮 |

未清理（用户数据或必需工具链，如需清理需用户确认）：

- `C:\Users\Lico\Desktop` 6.99 GB —— 用户文件，不动。
- `%LOCALAPPDATA%\JianyingPro` 7.07 GB —— 剪映程序本体 + 草稿，删除需重装/丢失草稿，先保留。
- `.cargo` / `.rustup`（构建必需）、WPS 程序本体、`%LOCALAPPDATA%\Programs`（已安装程序）。
- Windows 系统目录（Temp / SoftwareDistribution / CrashDumps）—— 当前用户非管理员，无法清理，回收站为空。

## 5. 进程清理（2026-10-03，第二轮）

用户指令：清理内存、清除无用进程。原则：只关后台/残留进程，不碰用户正在使用的东西。

结果：物理内存占用 **4.06 GB → 3.82 GB**（释放约 230 MB），可用内存 3.60 GB → 3.83 GB。

| 进程 | 数量 | 工作集 | 说明 |
| --- | --- | --- | --- |
| `msedge.exe` 后台实例 | 7 | 220.2 MB | 无任何可见窗口（纯后台空跑），可安全关闭 |
| `wps.exe`（`--host` 后台宿主） | 1 | 52.8 MB | WPS 云服务/推荐组件后台宿主，无打开文档 |
| `M365Copilot.exe` + `msedgewebview2.exe` | 1 + 6 | ~52 MB | Office Hub 自启动组件 |
| `java.exe`（hvigor-java-daemon） | 1 | 18.6 MB | 之前命令行打包留下的构建守护进程，下次构建自动重启 |
| `ubuntu-pro-agent.exe` | 1 | 0.9 MB | Ubuntu Pro 后台代理 |

复查 6 秒 + 3 秒后均无自动重启（M365Copilot 会随 Office 组件再次拉起，属自启动项）。

保留（未动，供参考）：

- `ChatGPT`（9 进程，~800 MB）：Codex 桌面宿主，关闭会中断当前任务。
- `AweSun`（向日葵，~47 MB）：如果你是从别的设备远程连这台电脑，关闭会直接断线，未动。
- `OneDrive`（21 MB）、`Desktop-Monitoring-v0.1.2`（21 MB，用户自己的桌面插件）、
  `MSPCManagerService` / Defender / Windows Search（系统组件）。
