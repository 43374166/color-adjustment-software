# Windows 平台工程

- 语言 / UI：Rust + egui / eframe（`crates/win_ui`）
- 发布形态：单文件 exe + NSIS 安装包（`packaging/windows/`）
- 打包命令：`cargo build --release -p fdxmp-win-ui`
- 输出目录：`dist/windows/`

阶段 0 仅包含 UI 骨架；阶段 6 实现完整桌面界面（设备 / 编辑 / 我的 + 打包器）。

