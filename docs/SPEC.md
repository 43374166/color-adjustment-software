# fdxmp 需求规格（冻结版）

> 来源：`调色软件方案.docx`（2026-10-02）。本文件为仓库内的规格副本，
> 后续实现与评审以本文件为准。技术选型、目录结构、功能清单均已确定，不做增删。

## 角色设定

资深跨平台图像处理架构师，精通 Rust、wgpu、RAW 解码、色彩科学、移动端原生开发。
任务：从零实现 fdxmp —— 一款完全对标 Adobe Lightroom 的专业图像处理软件。

## 不可更改的技术选型

- 核心语言：Rust（2021 edition）
- GPU 渲染：wgpu（后端 Metal / Vulkan / DX12）
- RAW 解码：rsraw（LibRaw 封装）
- 常见格式：image crate + libheif-rs
- 序列化：serde + serde_json + zip
- 并行：rayon
- 数组：ndarray
- 错误：thiserror
- 异步：tokio
- 基准：criterion
- Windows UI：egui / eframe
- 打包 UI：egui / eframe（单文件）
- iOS UI：SwiftUI
- Android UI：Jetpack Compose
- HarmonyOS UI：ArkTS
- 鸿蒙桥接：ohos-rs
- iOS AI：Core ML / Vision
- Android AI：LiteRT
- 鸿蒙 AI：XEngine Kit
- Windows AI：ONNX Runtime

## 核心功能

### 移动端三大 Tab

#### Tab 1：设备（Device）

- 冷启动后默认进入
- 加载手机本地图片（相册权限）
- 网格 / 列表视图切换
- 支持 RAW 和常见格式
- 点击进入全屏预览
- 长按多选，支持批量操作
- 顶部：搜索、排序、筛选
- 底部：导入、相机、分享

#### Tab 2：编辑（Edit）

- 展示所有已编辑过的图片
- 缩略图 + 编辑时间 + 预设名称
- 支持按时间 / 名称 / 预设排序
- 点击进入编辑界面
- 支持滑动删除、批量导出
- 空状态引导用户去设备 Tab 选图

#### Tab 3：我的（Me）

- 需要登录（账号体系先预留接口）
- 设置项：账号与同步、默认导出格式与质量、缓存管理（查看大小、清除缓存）、
  预设管理（导入 / 导出 / 删除）、主题（跟随系统 / 浅色 / 深色）、语言、关于、
  隐私政策、退出登录
- 清除缓存为核心功能之一，必须实现
- 缓存包括：缩略图缓存、预览缓存、AI 模型缓存

### 编辑界面核心功能（对标 Lightroom）

调色面板（自上而下）：

1. 曝光（Exposure）
2. 对比度（Contrast）
3. 高光（Highlights）
4. 阴影（Shadows）
5. 白色（Whites）
6. 黑色（Blacks）
7. 色温（Temperature）
8. 色调（Tint）
9. 自然饱和度（Vibrance）
10. 饱和度（Saturation）
11. 纹理（Texture）
12. 清晰度（Clarity）
13. 去雾（Dehaze）
14. 暗角（Vignette）
15. 颗粒（Grain）

曲线面板：RGB 通道曲线、红 / 绿 / 蓝独立通道、参数曲线（高光 / 中间调 / 阴影）。

HSL 面板：色相 / 饱和度 / 亮度，8 个颜色通道（红、橙、黄、绿、青、蓝、紫、洋红）。

颜色分级面板：阴影 / 中间调 / 高光，色相、饱和度、亮度，混合与平衡。

细节面板：锐化（数量、半径、细节、蒙版）；降噪（亮度、细节、对比度、颜色、
颜色细节、颜色平滑度）。

镜头校正：配置文件校正、色差、晕影、几何（自动 / 手动）。

变换：透视校正、旋转、翻转。

裁剪（含旋转）：自由裁剪；固定比例 1:1、4:3、3:2、16:9、原始；旋转（-45° 到 +45°）；
翻转（水平 / 垂直）；网格辅助线；裁剪后自动调整构图。

蒙版（核心功能，对标 Lightroom）：

- 画笔（Brush）：大小、羽化、流动、密度、自动蒙版
- 线性渐变（Linear Gradient）
- 径向渐变（Radial Gradient）
- 颜色范围（Color Range）：选择颜色、调整范围
- 亮度范围（Luminance Range）：选择亮度区间
- 深度范围（Depth Range）：预留
- AI 蒙版：主体（Subject）、天空（Sky）、背景（Background）、
  人物（People：面部、身体、皮肤、头发、衣服）、物体（Objects：点按选择）
- 蒙版叠加显示：红色 / 绿色 / 蓝色 / 白色 / 黑色
- 蒙版反向、蒙版羽化、蒙版复制 / 粘贴
- 每个蒙版独立调整所有调色参数

预设面板：

- 内置预设分类：基础、人像、风景、黑白、电影、复古、HDR
- 用户预设、预设缩略图预览、点击应用、长按查看详情
- 强度调节滑块（0-100%）
- 导入 Lightroom XMP 预设
- 导出为 .fdxmp 格式
- .fdxmp 格式：JSON + 可选内嵌 LUT 的 ZIP 包，带 schema_version

历史记录：所有编辑操作可撤销 / 重做；历史步骤列表，点击跳转；支持快照。

导出：

- 格式：JPEG、PNG、TIFF、HEIC、WebP
- 质量滑块
- 尺寸：原始 / 长边 / 短边 / 百分比
- 色彩空间：sRGB、Display P3、Adobe RGB、ProPhoto RGB
- 元数据：保留 / 删除
- 水印：文字 / 图片
- 批处理
- 导出到相册 / 文件 / 分享

## 移动端软件流程（仅 Android / HarmonyOS / iOS；Windows 桌面端见 docs/DESKTOP_UI.md）

### 冷启动流程

1. 显示启动画面（Logo + 加载动画）
2. 初始化 Rust 核心（FFI 调用）
3. 初始化 GPU 上下文
4. 检查登录状态
5. 加载缩略图缓存
6. 进入主界面，默认停在 Tab 1（设备）

### 主界面结构

- 底部导航栏三个 Tab：设备 / 编辑 / 我的
- 顶部标题栏随 Tab 变化
- 无侧边栏，无抽屉
- 完全对标 Lightroom 移动版的交互

### Tab 1：设备

1. 请求相册权限
2. 若无权限，显示引导页
3. 加载本地图片网格
4. 点击图片进入全屏预览
5. 全屏预览底部：分享、编辑、删除、收藏
6. 点击「编辑」进入编辑界面

### Tab 2：编辑

1. 从本地数据库读取已编辑记录
2. 显示缩略图 + 编辑时间 + 预设名
3. 点击进入编辑界面，恢复上次编辑状态
4. 长按多选，支持批量导出 / 删除

### Tab 3：我的

1. 未登录：显示登录 / 注册入口
2. 已登录：显示头像、昵称、同步状态
3. 设置列表（见核心功能）
4. 清除缓存：显示当前缓存大小，点击确认后清除，显示清除后大小

### 编辑界面流程

1. 顶部：返回、撤销、重做、对比、导出
2. 中部：图像预览区（占最大面积）
3. 底部：工具选择栏（裁剪、调色、蒙版、预设、细节、光学、几何、历史）
4. 点击工具，从底部弹出对应面板
5. 面板可上下拖动，支持横屏
6. 所有调整实时预览，GPU 渲染
7. 退出时自动保存编辑参数到 sidecar 文件

## 项目结构

```text
fdxmp/
├── Cargo.toml
├── README.md
├── LICENSE
├── crates/
│   ├── core/                  # 图像处理核心
│   ├── ffi/                   # C ABI 导出层
│   ├── cli/                   # 命令行测试工具
│   ├── win_ui/                # Windows UI
│   ├── android_bridge/        # Android FFI 桥接
│   └── ohos_bridge/           # HarmonyOS FFI 桥接
├── platforms/
│   ├── ios/                   # 占位，先写代码
│   ├── android/
│   ├── harmony/
│   └── windows/
├── packaging/                 # 打包脚本，最外层
│   ├── windows/
│   ├── android/
│   ├── harmony/
│   └── ios/                   # 占位
├── dist/                      # 打包输出，最外层
│   ├── windows/
│   ├── android/
│   ├── harmony/
│   └── ios/                   # 占位
├── presets/                   # .fdxmp 示例预设
├── docs/                      # 文档
├── assets/                    # 图标、测试图片
└── packer_ui/                 # 单文件 UI 打包器
```

## 打包器（单文件 UI）

位置：`packer_ui/src/main.rs`（单文件，不拆分）。

功能：

1. 启动时自动检测各平台打包工具
2. 每个平台一张卡片，显示检测结果（绿 ✓ / 红 ✗ / 黄 ⚠）
3. 点击按钮执行对应打包命令
4. 实时日志输出到界面底部
5. 输出统一到根目录 `dist/{platform}/`
6. iOS 按钮禁用，显示「暂不支持」
7. 界面美观：深色主题、卡片布局、圆角、图标、状态色
8. 依赖：eframe、egui、egui_extras、rfd、serde、serde_json、which

检测项：

- Windows：cargo、rustup、makensis、7z
- Android：cargo-ndk、Android SDK、NDK、Gradle、Java
- HarmonyOS：ohos-rs、DevEco、hvigor
- iOS：禁用

打包命令：

- Windows：`cargo build --release -p win_ui`
- Android：`cargo ndk` + `gradle assembleRelease`
- HarmonyOS：`ohos-rs build` + `hvigor assembleHap`
- iOS：禁用

## 预设格式 .fdxmp

- 扩展名：.fdxmp
- 本质：JSON + 可选内嵌 LUT 的 ZIP 包
- 必须字段：schema_version、name、author、created_at、params
- params 包含所有调色参数
- 支持导入 Lightroom XMP
- 四端共用，Rust core 负责序列化

## 开发阶段

### 阶段 0：脚手架

创建 Cargo workspace，配置所有 crate，确认四端编译与 FFI 可行。

### 阶段 1：核心解码与 GPU 显示

实现 RAW + 常见格式统一解码，wgpu 显示图片，验证首屏性能。

### 阶段 2：色彩管线

实现所有调色参数，曲线，HSL，颜色分级，细节。GPU compute shader 实现。

### 阶段 3：蒙版系统

实现画笔、线性、径向、颜色范围、亮度范围、AI 蒙版。

### 阶段 4：预设与导出

实现 .fdxmp 格式，导入 XMP，导出管线。

### 阶段 5：移动端 UI

实现三大 Tab，编辑界面，裁剪，旋转，历史记录。

### 阶段 6：Windows UI 与打包器

实现 Windows UI 和单文件打包器。

### 阶段 7：AI 蒙版与性能优化

集成 AI 模型，GPU 调优，冷启动优化，低端设备降级。

## 输出要求

1. 每阶段输出报告：已完成、测试结果、性能、遗留问题。
2. 提供架构图、API 文档、构建说明。
3. 每个核心模块有单元测试和集成测试。
4. criterion 性能基准。
5. 最终输出 PROJECT_REPORT。

## 注意事项

1. 所有技术选型已确定，不要做任何选择。
2. 完全对标 Lightroom，功能不缺失。
3. 移动端三大 Tab 流程严格执行。
4. 调色和蒙版是核心，必须做到极致。
5. .fdxmp 预设四端共用。
6. 最外层必须有 packaging/ 和 dist/。
7. iOS 先写代码，不打包。
8. 打包器单文件，UI 美观。
9. 性能目标：冷启动 < 2s，24MP RAW 首屏 < 2s，实时预览 ≥ 30fps。
10. 代码通过 cargo clippy 和 cargo fmt。
