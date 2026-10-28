# iOS 平台工程

SwiftUI 三大 Tab（设备 / 编辑 / 我的）+ Rust 核心（C ABI 静态库）。

## 架构：系统解码 + Rust 调色

iOS 的照片多为 HEIC / ProRAW，为省掉 libheif / LibRaw 的交叉编译，
iOS 端采用「系统解码 + Rust 调色」：

1. PhotoKit 解码照片（HEIC / JPEG / RAW 都由系统负责）；
2. Swift 侧把图像转成行优先 RGBA8；
3. Rust 核心应用 EditParams（调色 / 细节 / 效果 / 镜头 / 几何 / 蒙版）：
   预览优先走 GPU（`fdxmp_gpu_preview_*`，wgpu → Metal）：把 CAMetalLayer 挂到会话上后
   **零拷贝上屏**（显示纹理直接 present，不回读像素），直方图也由 GPU 统计；
   创建失败或单帧不适合（如「裁剪 + 蒙版」组合）时自动回退 CPU
   （`fdxmp_render_memory_rgba`，回读的 UIImage 路径仍然保留）；
4. 回读路径（对比模式 / 看原图 / GPU 不可用）把渲染结果转成 UIImage 显示，直方图在 Swift 侧统计。

因此 iOS 构建只启用核心 `gpu` feature（不带 `raw` / `heif`）；
wgpu 走系统 Metal，链接 Metal / QuartzCore 等系统框架，云打包不需要任何第三方原生库。

## 已实现

| 模块 | 说明 |
| --- | --- |
| 设备 Tab | 相册权限、网格 / 列表、搜索 / 排序 / 筛选、多选、全屏预览、导入 / 相机 / 分享（`DeviceTabView.swift`） |
| 编辑 Tab | 亮度（6）/ 曲线 / 颜色（4）/ HSL（8 色带 × 3）/ 颜色分级（3 区域 × 3 + 混合 · 平衡）/ 效果（9，含晕影 · 颗粒子页）/ 细节（10，锐化 · 降噪子页）/ 光学（删除色差 · 启用镜头校正）；Lightroom 风格中性点滑块（拖动 / 点按 ±step / 双击复位，进度条白色）；长按看原图；撤销重做（40 步）；预设库（系统 6 组 + 自建文件夹 / 存为预设并勾选包含的调整 / 除系统预设外可删除 / 导入导出 `.fdxmp`、复制粘贴、导入 Lightroom `.xmp`；选中预设后在缩略图上直接调强度） |
| 曲线 | RGB + 红 / 绿 / 蓝四通道点曲线（点空白加点、点控制点删除、拖动调整）+ 参数曲线（高光 / 中间调 / 阴影 + 两个分界），求值公式与 Rust 渲染一致 |
| 裁切 / 变换 | 可拖动裁剪框（角点缩放 / 框内平移、三分线）+ 自由 / 1:1 / 4:3 / 3:2 / 16:9 比例 + 拉直（±45°）+ 水平 / 垂直翻转 |
| 蒙版 | 画笔（直接涂抹，笔刷大小 / 羽化 / 流动 / 密度）/ 线性渐变（拖动定方向）/ 径向渐变（拖动定范围 + 内圈 · 旋转）；每个蒙版独立一整套调色参数；调整页（创建蒙版后才弹起）有子 nav（反转 / 复制 / 重置 / 重命名 / 删除）；长按卡片进「蒙版设置」（显示叠加 / 叠加色 / 形状参数 / 蒙版羽化 / 不透明度 / 复制粘贴设置） |
| 导出 | 按当前参数渲染（8MP 像素预算，超限自动降采样）并保存到相册「fdxmp」；独立导出队列 + 分段蓝条 / 循环白光进度 + 步骤文字（不显示百分比） |
| 渲染 | GPU 预览（wgpu → Metal，源图上传一次、参数变化重渲染回读）+ CPU 自动回退；2048px 预览 / 导出按 8MP 预算 |
| 存储 | 按照片 `localIdentifier` 保存 EditParams JSON（UserDefaults） |

尚未接入：颜色范围 / 亮度范围蒙版、AI 蒙版（需要各平台推理运行时）、
镜头校正的透视 / 畸变手动量（当前是「删除色差 + 启用配置文件校正」两项）。

## 目录

```
platforms/ios/
├── project.yml              # XcodeGen 工程定义（生成 Fdxmp.xcodeproj）
└── Fdxmp/
    ├── FdxmpApp.swift       # App 入口
    ├── ContentView.swift    # 三大 Tab
    ├── DeviceTabView.swift  # 设备 Tab（相册）
    ├── PhotoIndex.swift     # 文件名 / RAW 索引 + 磁盘缓存 + 缩略图缓存
    ├── RawFrameCache.swift  # 已解码 RAW 帧缓存（退出编辑页再进不再重解）
    ├── EditorTabView.swift  # 编辑 Tab（调色界面）
    ├── EditorModel.swift    # 渲染状态机（防抖 / 历史 / 导出 / 工具切换）
    ├── EditParams.swift     # 滑块规格表 / 预设 / 存储
    ├── EditState.swift      # 完整参数模型（曲线 / HSL / 分级 / 裁切 / 蒙版）+ JSON
    ├── CurvePanelView.swift # 曲线编辑器
    ├── CropOverlayView.swift# 裁切叠加层
    ├── MaskEditorView.swift # 蒙版叠加层与面板
    ├── CenteredSlider.swift # Lightroom 风格滑块
    ├── HistogramOverlay.swift
    ├── ImageUtil.swift      # UIImage ⇄ RGBA8
    ├── NativeCore.swift     # C ABI 桥接
    └── Fdxmp-Bridging-Header.h
```

## 构建

云端（GitHub Actions）：推送后到 Actions 页面下载 `Fdxmp-unsigned-ipa`，
或手动触发 `iOS 未签名 IPA` 工作流。详见 [`../../packaging/ios/README.md`](../../packaging/ios/README.md)。

本机（macOS + Xcode）：

```bash
brew install xcodegen
./packaging/ios/build-ipa.sh
```

产物：`dist/ios/Fdxmp-unsigned.ipa`。
