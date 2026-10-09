fdxmp Windows 分发包
===================

本目录包含两种使用方式：

1) 安装（推荐）
   powershell -ExecutionPolicy Bypass -File install.ps1
   默认安装到 %LOCALAPPDATA%\Programs\fdxmp，并创建开始菜单 / 桌面快捷方式；
   卸载执行安装目录下的 uninstall.ps1，或：
   powershell -ExecutionPolicy Bypass -File install.ps1 -InstallDir "F:\fdxmp" -NoDesktopShortcut
   指定目录时也可以用同样的方式把程序装到非系统盘。

2) 绿色版
   直接双击 fdxmp-win-ui.exe（需保证同目录下所有 DLL 都在）。

系统要求：Windows 10/11 x64，支持 Vulkan / DX12 的显卡（GTX 1050 Ti 及以上实测可用）。

说明
- 当前分发包由 7-Zip 打包；NSIS 安装包脚本（packaging/windows/fdxmp.nsi）已就绪，
  待本机可获取 NSIS（makensis）后可直接生成 .exe 安装向导。
- 首次启动会初始化 GPU 与解码器，冷启动约 1-4 秒，之后热启动 < 1 秒。

