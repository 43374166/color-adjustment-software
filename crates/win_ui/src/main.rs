//! fdxmp Windows UI —— 桌面布局（Lightroom Classic 式），不是移动端三 Tab 的放大版。
//!
//! 布局：
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │ 菜单栏：文件 / 编辑 / 视图 / 帮助 + 常用操作                  │
//! ├───────────┬──────────────────────────┬───────────────────────┤
//! │ 左栏       │  画布（wgpu 实时预览）    │ 右栏：直方图 +        │
//! │ 图标列：   │  滚轮缩放 / 拖放导入       │ 当前模块面板          │
//! │ 图库/预设/ │                          │ ─────────────         │
//! │ 历史/导出/ │                          │ 编辑 / 蒙版 / 裁切    │
//! │ AI        │                          │                       │
//! ├───────────┴──────────────────────────┴───────────────────────┤
//! │ 胶片条（缩略图 + 左右方向键切换）                             │
//! ├──────────────────────────────────────────────────────────────┤
//! │ 状态栏：提示 / 缓存 / 历史 / 缩放                             │
//! └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! 右侧模块（三个按钮横排在右栏**底部**，位于调色面板下方）：
//! - **编辑**：基本（按 Lightroom 分组：白平衡 / 色调 / 偏好）+ 效果 + 曲线 / HSL / 分级 / 细节；
//! - **蒙版**：蒙版列表与新建工具（画笔 / 渐变 / 颜色范围 / AI）及选中蒙版参数；
//! - **裁切**：裁剪 · 旋转 / 变换（透视 · 翻转）/ 镜头校正。
//!
//! 直方图与照片信息固定在右侧栏顶部，三个模块共用。
//!
//! 桌面优势的落点：面板可拖动改变宽度、可整块隐藏、快捷键（Ctrl+Z / Ctrl+Y /
//! Tab / 方向键）、滚轮缩放、原生文件对话框、拖放导入、历史面板跳转。
//! 多窗口 / 双显示器、右键上下文菜单在阶段 6 展开。

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use eframe::egui;
use fdxmp_core::cache::CacheManager;
use fdxmp_core::decode::DecodedImage;
use fdxmp_core::histogram::Histogram;
use fdxmp_core::params::{ColorBand, CurvePoint, EditParams, SliderId};
use fdxmp_core::Engine;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1024.0, 640.0])
            .with_title("fdxmp"),
        ..Default::default()
    };
    eframe::run_native(
        "fdxmp",
        options,
        Box::new(|cc| {
            install_cjk_fonts(&cc.egui_ctx);
            let mut app = FdxmpApp {
                gpu_label: gpu_label(cc),
                ..FdxmpApp::default()
            };
            #[cfg(feature = "gpu")]
            {
                app.gpu = cc.wgpu_render_state.as_ref().map(GpuPreview::new);
            }
            // 支持"用 fdxmp 打开"：命令行传入的图片路径直接载入。
            for arg in std::env::args().skip(1) {
                let path = PathBuf::from(&arg);
                if path.is_file() {
                    app.add_photo(path);
                }
            }
            if app.selected.is_some() {
                let indices: Vec<usize> = (0..app.photos.len()).collect();
                app.start_thumbnails(&cc.egui_ctx, indices);
                app.load_selected(&cc.egui_ctx);
            }
            Ok(Box::new(app))
        }),
    )
}

/// 当前渲染后端信息（eframe 默认 wgpu：Windows 上为 DX12）。
fn gpu_label(cc: &eframe::CreationContext<'_>) -> String {
    match &cc.wgpu_render_state {
        Some(state) => {
            let info = state.adapter.get_info();
            format!("{} · {:?}", info.name, info.backend)
        }
        None => "未启用 wgpu".to_string(),
    }
}

/// 加载系统中文字体（egui 内置字体不含 CJK，缺失时中文显示为方框）。
///
/// 可用 `FDXMP_UI_FONT` 指定字体文件覆盖默认查找顺序。
fn install_cjk_fonts(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\Deng.ttf",
        "/System/Library/Fonts/PingFang.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    ];

    let path = std::env::var_os("FDXMP_UI_FONT")
        .map(PathBuf::from)
        .into_iter()
        .chain(CANDIDATES.iter().map(PathBuf::from))
        .find(|candidate| candidate.is_file());

    let Some(path) = path else {
        eprintln!("警告：未找到中文字体，界面中文可能显示为方框（可用 FDXMP_UI_FONT 指定）");
        return;
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "fdxmp-cjk".to_owned(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
    if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        list.insert(0, "fdxmp-cjk".to_owned());
    }
    if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        list.push("fdxmp-cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeftTab {
    /// 图库（照片列表 / 导入）。
    Catalog,
    /// 预设。
    Presets,
    /// 历史。
    History,
    /// 导出。
    Export,
    /// AI 模型与 AI 蒙版快捷入口。
    Ai,
}

/// 右侧模块：编辑（调色）/ 蒙版 / 剪切（裁剪 · 旋转 · 透视 · 镜头）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RightModule {
    Edit,
    Mask,
    Crop,
}

/// 编辑目标：全局参数或某个蒙版的独立参数（阶段 3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditTarget {
    Global,
    Mask(usize),
}

/// 裁剪模式交互手柄（对标 Lightroom 独立裁剪视图）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CropHandle {
    /// 框体内部：平移。
    Move,
    /// 左边。
    Left,
    /// 右边。
    Right,
    /// 上边。
    Top,
    /// 下边。
    Bottom,
    /// 左上角。
    TopLeft,
    /// 右上角。
    TopRight,
    /// 左下角。
    BottomLeft,
    /// 右下角。
    BottomRight,
    /// 画面外侧：旋转 / 摆正。
    Rotate,
}

/// 裁剪拖拽状态：记录起始值，用绝对量计算避免累积误差。
#[derive(Debug, Clone, Copy)]
struct CropDrag {
    handle: CropHandle,
    start_rect: [f32; 4],
    start_pointer: egui::Pos2,
    start_rotation: f32,
}

/// 蒙版编辑视图中的画布拖拽（线性 / 径向渐变）。
#[derive(Debug, Clone, Copy)]
struct MaskDrag {
    kind: MaskDragKind,
    start_uv: [f32; 2],
}

/// 蒙版拖拽类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaskDragKind {
    /// 线性渐变：拖动起点。
    LinearStart,
    /// 线性渐变：拖动终点。
    LinearEnd,
    /// 线性渐变：整段重设。
    LinearNew,
    /// 径向渐变：移动中心。
    RadialCenter,
    /// 径向渐变：调整半径。
    RadialRadius,
}

/// 可持久化的界面布局状态（写入缓存目录 `ui_state.json`）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct UiState {
    /// 左侧面板可见。
    show_left: bool,
    /// 右侧面板可见。
    show_right: bool,
    /// 胶片条可见。
    show_filmstrip: bool,
    /// 左侧标签页（catalog / presets / history）。
    left_tab: String,
    /// 适应窗口。
    fit_to_window: bool,
    /// 缩放倍率（非适应窗口时生效）。
    zoom: f32,
    /// 蒙版叠加显示。
    show_overlay: bool,
    /// 预设强度。
    preset_strength: f32,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            show_left: true,
            show_right: true,
            show_filmstrip: true,
            left_tab: "catalog".to_string(),
            fit_to_window: true,
            zoom: 1.0,
            show_overlay: true,
            preset_strength: 100.0,
        }
    }
}

impl UiState {
    /// 配置文件路径（缓存目录的上一级）。
    fn path() -> PathBuf {
        let mut dir = fdxmp_core::default_cache_root();
        if dir.file_name().map(|name| name == "cache").unwrap_or(false) {
            dir.pop();
        }
        dir.join("ui_state.json")
    }

    /// 读取（不存在或损坏时返回默认值）。
    fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// 写入（失败静默：布局持久化不影响主流程）。
    fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}

/// 左侧标签页的字符串标识（持久化用）。
fn left_tab_id(tab: LeftTab) -> &'static str {
    match tab {
        LeftTab::Catalog => "catalog",
        LeftTab::Presets => "presets",
        LeftTab::History => "history",
        LeftTab::Export => "export",
        LeftTab::Ai => "ai",
    }
}

/// 从字符串恢复左侧标签页。
fn left_tab_from_id(id: &str) -> LeftTab {
    match id {
        "presets" => LeftTab::Presets,
        "history" => LeftTab::History,
        "export" => LeftTab::Export,
        "ai" => LeftTab::Ai,
        _ => LeftTab::Catalog,
    }
}

/// 是否为 AI 蒙版类型（需要模型推理）。
#[cfg(feature = "ai")]
fn is_ai_mask(kind: &fdxmp_core::params::MaskKind) -> bool {
    use fdxmp_core::params::MaskKind;
    matches!(
        kind,
        MaskKind::AiSubject
            | MaskKind::AiSky
            | MaskKind::AiBackground
            | MaskKind::AiPeople { .. }
            | MaskKind::AiObject { .. }
    )
}

/// 画布右键菜单动作。
#[derive(Debug, Clone, Copy, PartialEq)]
enum CanvasAction {
    /// 适应窗口。
    Fit,
    /// 切换 1:1 细节视图。
    Detail,
    /// 切换裁剪模式。
    Crop,
    /// 进入蒙版编辑视图。
    Mask,
    /// 设置蒙版叠加显示。
    Overlay(bool),
    /// 重置裁剪与变换。
    ResetGeometry,
}

/// 蒙版工具（新建蒙版时使用，与 Lightroom 的蒙版工具条对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaskTool {
    /// 画笔。
    Brush,
    /// 线性渐变。
    Linear,
    /// 径向渐变。
    Radial,
    /// 颜色范围。
    ColorRange,
    /// 亮度范围。
    LuminanceRange,
}

/// 蒙版工具的默认参数与名称。
fn mask_tool_default(tool: MaskTool) -> (fdxmp_core::params::MaskKind, &'static str) {
    use fdxmp_core::params::{BrushSettings, MaskKind};
    match tool {
        MaskTool::Brush => (
            MaskKind::Brush(BrushSettings {
                size: 12.0,
                feather: 50.0,
                flow: 100.0,
                density: 100.0,
                auto_mask: false,
                strokes: Vec::new(),
            }),
            "画笔",
        ),
        MaskTool::Linear => (
            MaskKind::LinearGradient {
                start: [0.2, 0.5],
                end: [0.8, 0.5],
            },
            "线性渐变",
        ),
        MaskTool::Radial => (
            MaskKind::RadialGradient {
                center: [0.5, 0.5],
                radii: [0.3, 0.3],
                rotation: 0.0,
            },
            "径向渐变",
        ),
        MaskTool::ColorRange => (
            MaskKind::ColorRange {
                color: [0.3, 0.5, 0.8],
                range: 20.0,
                smoothness: 30.0,
            },
            "颜色范围",
        ),
        MaskTool::LuminanceRange => (
            MaskKind::LuminanceRange {
                min: 0.3,
                max: 0.7,
                smoothness: 30.0,
            },
            "亮度范围",
        ),
    }
}

/// 蒙版类型是否对应某个工具。
fn mask_tool_matches(tool: MaskTool, kind: &fdxmp_core::params::MaskKind) -> bool {
    use fdxmp_core::params::MaskKind;
    matches!(
        (tool, kind),
        (MaskTool::Brush, MaskKind::Brush(_))
            | (MaskTool::Linear, MaskKind::LinearGradient { .. })
            | (MaskTool::Radial, MaskKind::RadialGradient { .. })
            | (MaskTool::ColorRange, MaskKind::ColorRange { .. })
            | (MaskTool::LuminanceRange, MaskKind::LuminanceRange { .. })
    )
}

/// 用户在阶段 1 之前看到的"照片"只是文件名占位，用于验证桌面交互。
struct PhotoSlot {
    name: String,
    path: Option<PathBuf>,
    thumbnail: Option<egui::TextureHandle>,
    thumbnail_failed: bool,
}

impl PhotoSlot {
    /// 悬停提示：优先显示完整路径（阶段 1 还要用它做真实解码）。
    fn tooltip(&self) -> String {
        match &self.path {
            Some(path) => path.display().to_string(),
            None => self.name.clone(),
        }
    }
}

/// 已载入画布的图像（代理纹理 + 统计信息）。
struct LoadedImage {
    texture: egui::TextureHandle,
    proxy_pixels: image::RgbaImage,
    original: [u32; 2],
    proxy: [u32; 2],
    source: String,
    elapsed_ms: u128,
    metadata: Option<fdxmp_core::decode::RawMetadata>,
    histogram: Histogram,
    path: String,
}

/// GPU 实时预览（阶段 2）：复用 eframe 的 wgpu 设备，输出纹理直接交给 egui 绘制。
#[cfg(feature = "gpu")]
struct GpuPreview {
    state: eframe::egui_wgpu::RenderState,
    pipeline: fdxmp_core::gpu_pipeline::ColorPipeline,
    image: Option<fdxmp_core::gpu_pipeline::PipelineImage>,
    texture_id: Option<egui::TextureId>,
    texture_generation: u64,
    frame_ms: f32,
    mode: fdxmp_core::geometry::GeometryMode,
}

#[cfg(feature = "gpu")]
impl GpuPreview {
    fn new(state: &eframe::egui_wgpu::RenderState) -> Self {
        Self {
            state: state.clone(),
            pipeline: fdxmp_core::gpu_pipeline::ColorPipeline::new(&state.device),
            image: None,
            texture_id: None,
            texture_generation: 0,
            frame_ms: 0.0,
            mode: fdxmp_core::geometry::GeometryMode::Crop,
        }
    }

    fn upload(&mut self, rgba: &image::RgbaImage) -> Result<(), String> {
        let image = self
            .pipeline
            .upload(
                &self.state.device,
                &self.state.queue,
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
            )
            .map_err(|err| err.to_string())?;

        self.image = Some(image);
        self.texture_generation = 0;
        Ok(())
    }

    /// 线性 f32 源上传（16bit / RAW 通路，纹理 `rgba16float`）。
    fn upload_linear(&mut self, rgba: &[f32], width: u32, height: u32) -> Result<(), String> {
        let image = self
            .pipeline
            .upload_linear_f32(&self.state.device, &self.state.queue, rgba, width, height)
            .map_err(|err| err.to_string())?;
        self.image = Some(image);
        self.texture_generation = 0;
        Ok(())
    }

    fn render(
        &mut self,
        params: &fdxmp_core::params::EditParams,
        mode: fdxmp_core::geometry::GeometryMode,
    ) {
        let Some(image) = &self.image else {
            return;
        };
        let started = std::time::Instant::now();
        self.pipeline
            .render_with(&self.state.device, &self.state.queue, image, params, mode);
        let _ = self.state.device.poll(eframe::wgpu::PollType::Poll);
        self.frame_ms = started.elapsed().as_secs_f32() * 1000.0;
        self.mode = mode;
        self.refresh_texture();
    }

    /// 渲染目标尺寸 / 源图变化时重新注册 egui 纹理（裁剪尺寸会变）。
    fn refresh_texture(&mut self) {
        let generation = self.pipeline.target_generation();
        if self.texture_id.is_none() || generation != self.texture_generation {
            if let Some(old) = self.texture_id.take() {
                self.state.renderer.write().free_texture(&old);
            }
            if let Some(view) = self.pipeline.output_view() {
                let texture_id = self.state.renderer.write().register_native_texture(
                    &self.state.device,
                    view,
                    eframe::wgpu::FilterMode::Linear,
                );
                self.texture_id = Some(texture_id);
                self.texture_generation = generation;
            }
        }
    }

    /// 1:1 分块渲染（自定义“输出像素 → 源像素”矩阵与输出尺寸）。
    fn render_tile(
        &mut self,
        params: &fdxmp_core::params::EditParams,
        matrix: &fdxmp_core::geometry::Mat3,
        width: u32,
        height: u32,
        tile_origin: [f32; 2],
        full_size: [f32; 2],
    ) {
        let Some(image) = &self.image else {
            return;
        };
        let started = std::time::Instant::now();
        self.pipeline.render_tile(
            &self.state.device,
            &self.state.queue,
            image,
            params,
            &fdxmp_core::gpu_pipeline::RenderPlan {
                matrix: *matrix,
                width,
                height,
                geometry_active: true,
                tile_origin,
                full_size,
            },
        );
        let _ = self.state.device.poll(eframe::wgpu::PollType::Poll);
        self.frame_ms = started.elapsed().as_secs_f32() * 1000.0;
        self.refresh_texture();
    }

    /// 设置蒙版参考图（分块渲染时蒙版仍按代理图生成，坐标按比例换算）。
    fn set_mask_reference(
        &mut self,
        pixels: std::sync::Arc<image::RgbaImage>,
        scale: [f32; 2],
        origin: [f32; 2],
    ) {
        if let Some(image) = self.image.as_mut() {
            image.set_mask_reference(pixels, scale, origin);
        }
    }

    /// 当前渲染目标尺寸（裁剪模式下就是输出尺寸）。
    fn display_size(&self) -> Option<(u32, u32)> {
        self.pipeline.target_size()
    }

    /// 在 GPU 上叠加选中蒙版（与裁剪 / 旋转变换一致）。
    fn render_overlay(
        &mut self,
        params: &fdxmp_core::params::EditParams,
        mask_index: usize,
        style: fdxmp_core::gpu_pipeline::OverlayStyle,
    ) {
        let Some(image) = &self.image else {
            return;
        };
        self.pipeline.render_overlay(
            &self.state.device,
            &self.state.queue,
            image,
            params,
            mask_index,
            style,
        );
        let _ = self.state.device.poll(eframe::wgpu::PollType::Poll);
    }

    fn readback(&self) -> Option<image::RgbaImage> {
        let image = self.image.as_ref()?;
        let (width, height) = self.pipeline.target_size()?;
        let pixels = self
            .pipeline
            .readback(&self.state.device, &self.state.queue, image)
            .ok()?;
        image::RgbaImage::from_raw(width, height, pixels)
    }
}

struct FdxmpApp {
    engine: Engine,
    left_tab: LeftTab,
    /// 右侧模块（编辑 / 蒙版 / 剪切），模块按钮在最右侧。
    right_module: RightModule,
    show_left: bool,
    show_right: bool,
    show_filmstrip: bool,
    fit_to_window: bool,
    zoom: f32,
    selected: Option<usize>,
    photos: Vec<PhotoSlot>,
    status: String,
    loaded: Option<LoadedImage>,
    decode_rx: Option<Receiver<Result<DecodedImage, String>>>,
    decoding: Option<String>,
    pan: egui::Vec2,
    gpu_label: String,
    thumb_rx: Option<Receiver<(usize, Result<image::RgbaImage, String>)>>,
    full_rx: Option<Receiver<(usize, Result<DecodedImage, String>)>>,
    full_for: Option<usize>,
    #[cfg(feature = "gpu")]
    gpu: Option<GpuPreview>,
    params_dirty: bool,
    #[cfg(feature = "gpu")]
    histogram_dirty: bool,
    #[cfg(feature = "gpu")]
    last_change: std::time::Instant,
    pending_history: Option<String>,
    edit_target: EditTarget,
    curve_channel: usize,
    hsl_mode: usize,
    brush_painting: bool,
    show_overlay: bool,
    overlay_texture: Option<(u64, egui::TextureHandle)>,
    /// 是否处于独立裁剪模式（对标 Lightroom：全幅视图 + 裁剪框 + 四周压暗）。
    crop_mode: bool,
    /// 正在进行的裁剪拖拽。
    crop_drag: Option<CropDrag>,
    /// 是否处于独立蒙版编辑视图（对标 Lightroom：画面叠加 + 底部蒙版工具条）。
    mask_mode: bool,
    /// 进入蒙版视图前的叠加显示状态（退出时恢复）。
    mask_restore_overlay: bool,
    /// 正在进行的蒙版拖拽（线性 / 径向）。
    mask_drag: Option<MaskDrag>,
    /// 蒙版剪贴板（跨图片复制 / 粘贴）。
    mask_clipboard: Option<fdxmp_core::params::MaskParams>,
    /// 批量导出标记（照片下标）。
    marked: std::collections::HashSet<usize>,
    /// 1:1 细节视图：全分辨率像素（按需解码，退出即释放）。
    detail_mode: bool,
    detail_full: Option<std::sync::Arc<DecodedImage>>,
    detail_rx: Option<Receiver<Result<DecodedImage, String>>>,
    detail_for: Option<usize>,
    /// 1:1 分块渲染使用的独立 GPU 预览（与代理预览互不干扰）。
    #[cfg(feature = "gpu")]
    detail_tile: Option<GpuPreview>,
    /// 当前分块覆盖的源图矩形（x, y, w, h），用于避免重复提取。
    detail_tile_rect: Option<(i64, i64, u32, u32)>,
    /// 预设缩略图缓存（键包含照片、预设与强度档位）。
    #[cfg(feature = "gpu")]
    preset_thumbs: std::collections::HashMap<String, egui::TextureHandle>,
    /// 预设缩略图基准小图（当前照片缩放一次后复用）。
    #[cfg(feature = "gpu")]
    preset_base: Option<(String, image::RgbaImage)>,
    /// 预设缩略图专用 GPU 预览（独立管线，避免干扰主预览）。
    #[cfg(feature = "gpu")]
    thumb_gpu: Option<GpuPreview>,
    /// 镜头配置文件数据库与选择状态。
    lens_db: Option<fdxmp_core::lens_db::LensDatabase>,
    lens_db_rx: Option<Receiver<Result<fdxmp_core::lens_db::LensDatabase, String>>>,
    lens_camera_query: String,
    lens_lens_query: String,
    lens_cameras: Vec<fdxmp_core::lens_db::Camera>,
    lens_lenses: Vec<fdxmp_core::lens_db::Lens>,
    lens_selected_camera: Option<usize>,
    lens_selected_lens: Option<usize>,
    lens_focal: f32,
    lens_aperture: f32,
    lens_distance: f32,
    lens_enable_distortion: bool,
    lens_enable_tca: bool,
    lens_enable_vignetting: bool,
    /// 已应用的配置文件校正（预览与导出共用）。
    lens_profile: Option<fdxmp_core::lens_db::LensCorrection>,
    /// 配置文件校正计算（工作线程）。
    lens_apply_rx: Option<Receiver<Result<image::RgbaImage, String>>>,
    /// 未经配置文件校正的原始代理（切换 / 清除校正时使用）。
    pristine_proxy: Option<image::RgbaImage>,
    /// 正在重新计算配置文件校正。
    lens_applying: bool,
    /// AI 蒙版生成（工作线程）。
    #[cfg(feature = "ai")]
    ai_rx: Option<Receiver<Result<String, String>>>,
    ai_running: Option<String>,
    /// AI 模型下载（工作线程）。
    #[cfg(feature = "ai")]
    ai_download_rx: Option<Receiver<Result<String, String>>>,
    #[cfg(feature = "ai")]
    ai_download_status: String,
    preset_strength: f32,
    export_settings: fdxmp_core::export::ExportSettings,
    export_full_resolution: bool,
    export_rx: Option<Receiver<Result<String, String>>>,
    /// 布局持久化：上次快照、脏标记与写盘时间。
    ui_state_applied: UiState,
    ui_state_dirty: bool,
    ui_state_last_save: std::time::Instant,
}

impl Default for FdxmpApp {
    fn default() -> Self {
        // 恢复上次的界面布局（面板可见性 / 标签页 / 缩放 / 叠加 / 预设强度）。
        let ui_state = UiState::load();
        Self {
            engine: Engine::new(),
            left_tab: left_tab_from_id(&ui_state.left_tab),
            right_module: RightModule::Edit,
            show_left: ui_state.show_left,
            show_right: ui_state.show_right,
            show_filmstrip: ui_state.show_filmstrip,
            fit_to_window: ui_state.fit_to_window,
            zoom: ui_state.zoom.clamp(0.05, 8.0),
            selected: None,
            photos: Vec::new(),
            status: "阶段 1：导入或拖入照片即可解码显示（RAW 走内嵌预览快速首屏）".to_string(),
            loaded: None,
            decode_rx: None,
            decoding: None,
            pan: egui::Vec2::ZERO,
            gpu_label: "wgpu".to_string(),
            thumb_rx: None,
            full_rx: None,
            full_for: None,
            #[cfg(feature = "gpu")]
            gpu: None,
            params_dirty: false,
            #[cfg(feature = "gpu")]
            histogram_dirty: false,
            #[cfg(feature = "gpu")]
            last_change: std::time::Instant::now(),
            pending_history: None,
            edit_target: EditTarget::Global,
            curve_channel: 0,
            hsl_mode: 0,
            brush_painting: false,
            show_overlay: ui_state.show_overlay,
            overlay_texture: None,
            crop_mode: false,
            crop_drag: None,
            mask_mode: false,
            mask_restore_overlay: true,
            mask_drag: None,
            mask_clipboard: None,
            marked: std::collections::HashSet::new(),
            detail_mode: false,
            detail_full: None,
            detail_rx: None,
            detail_for: None,
            #[cfg(feature = "gpu")]
            detail_tile: None,
            detail_tile_rect: None,
            #[cfg(feature = "gpu")]
            preset_thumbs: std::collections::HashMap::new(),
            #[cfg(feature = "gpu")]
            preset_base: None,
            #[cfg(feature = "gpu")]
            thumb_gpu: None,
            lens_db: None,
            lens_db_rx: None,
            lens_camera_query: String::new(),
            lens_lens_query: String::new(),
            lens_cameras: Vec::new(),
            lens_lenses: Vec::new(),
            lens_selected_camera: None,
            lens_selected_lens: None,
            lens_focal: 35.0,
            lens_aperture: 5.6,
            lens_distance: 5.0,
            lens_enable_distortion: true,
            lens_enable_tca: true,
            lens_enable_vignetting: true,
            lens_profile: None,
            lens_apply_rx: None,
            pristine_proxy: None,
            lens_applying: false,
            #[cfg(feature = "ai")]
            ai_rx: None,
            ai_running: None,
            #[cfg(feature = "ai")]
            ai_download_rx: None,
            #[cfg(feature = "ai")]
            ai_download_status: String::new(),
            preset_strength: ui_state.preset_strength.clamp(0.0, 100.0),
            export_settings: fdxmp_core::export::ExportSettings::default(),
            // 默认全分辨率导出：关闭时只导出 2048px 代理（历史默认值，易误用）。
            export_full_resolution: true,
            export_rx: None,
            ui_state_applied: ui_state,
            // 首次运行也写一份，便于用户查看/修改布局文件。
            ui_state_dirty: true,
            ui_state_last_save: std::time::Instant::now(),
        }
    }
}

impl eframe::App for FdxmpApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_shortcuts(&ctx);
        self.handle_dropped_files(&ctx);
        self.poll_decode(&ctx);
        self.poll_thumbnails(&ctx);
        self.poll_full_decode(&ctx);
        self.poll_detail_decode(&ctx);
        self.poll_lens_tasks(&ctx);
        self.poll_ai_tasks(&ctx);
        self.maybe_request_ai_mask();
        #[cfg(feature = "gpu")]
        self.update_gpu(&ctx);
        self.update_overlay(&ctx);
        self.poll_export(&ctx);
        self.persist_ui_state();

        egui::Panel::top("menu_bar").show(ui, |ui| self.menu_bar(ui));

        if self.show_filmstrip {
            egui::Panel::bottom("filmstrip")
                .resizable(true)
                .default_size(126.0)
                .show(ui, |ui| self.filmstrip(ui));
        }
        egui::Panel::bottom("status_bar").show(ui, |ui| self.status_bar(ui));

        if self.show_left {
            egui::Panel::left("left_rail")
                .resizable(true)
                .default_size(286.0)
                .show(ui, |ui| self.left_rail(ui));
        }
        if self.show_right {
            egui::Panel::right("right_rail")
                .resizable(true)
                .default_size(330.0)
                .show(ui, |ui| self.right_rail(ui));
        }

        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));

        // 交互结束（松手 / 键盘调整完成）后提交一步历史。
        if self.pending_history.is_some() && !ctx.egui_is_using_pointer() {
            if let Some(label) = self.pending_history.take() {
                self.engine.commit_history(label);
            }
        }
    }
}

impl FdxmpApp {
    /// 当前界面布局快照（用于变化检测与写盘）。
    fn ui_state_snapshot(&self) -> UiState {
        UiState {
            show_left: self.show_left,
            show_right: self.show_right,
            show_filmstrip: self.show_filmstrip,
            left_tab: left_tab_id(self.left_tab).to_string(),
            fit_to_window: self.fit_to_window,
            zoom: self.zoom,
            show_overlay: self.show_overlay,
            preset_strength: self.preset_strength,
        }
    }

    /// 布局变化时延迟写盘（800ms 去抖，避免缩放拖动频繁写文件）。
    fn persist_ui_state(&mut self) {
        let current = self.ui_state_snapshot();
        if current != self.ui_state_applied {
            self.ui_state_applied = current;
            self.ui_state_dirty = true;
        }
        if self.ui_state_dirty
            && self.ui_state_last_save.elapsed() >= std::time::Duration::from_millis(800)
        {
            self.ui_state_dirty = false;
            self.ui_state_last_save = std::time::Instant::now();
            self.ui_state_applied.save();
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.menu_button("文件", |ui| {
                if ui.button("导入图片…").clicked() {
                    ui.close();
                    self.import_dialog(ui.ctx());
                }
                if ui.button("导出…").clicked() {
                    ui.close();
                    // 导出面板在左栏「导出」标签页
                    self.show_left = true;
                    self.left_tab = LeftTab::Export;
                }
                ui.separator();
                if ui.button("退出").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("编辑", |ui| {
                if ui.button("撤销   Ctrl+Z").clicked() {
                    ui.close();
                    self.undo();
                }
                if ui.button("重做   Ctrl+Y").clicked() {
                    ui.close();
                    self.redo();
                }
                ui.separator();
                if ui.button("重置调色").clicked() {
                    ui.close();
                    let mut params = self.engine.params().clone();
                    for id in SliderId::ALL {
                        params.set_slider(id, 0.0);
                    }
                    self.engine.apply("重置调色", params);
                    self.params_dirty = true;
                }
            });
            ui.menu_button("视图", |ui| {
                ui.checkbox(&mut self.show_left, "左侧面板");
                ui.checkbox(&mut self.show_right, "右侧面板");
                ui.checkbox(&mut self.show_filmstrip, "胶片条");
                ui.separator();
                if ui.button("适应窗口").clicked() {
                    self.fit_to_window = true;
                    self.zoom = 1.0;
                    ui.close();
                }
                if ui.button("100%").clicked() {
                    self.fit_to_window = false;
                    self.zoom = 1.0;
                    ui.close();
                }
            });
            ui.menu_button("帮助", |ui| {
                ui.label(format!(
                    "fdxmp {} · ABI {}",
                    fdxmp_core::version(),
                    fdxmp_core::ABI_VERSION
                ));
                ui.label("阶段 0：桌面骨架（阶段 1 接入解码与预览）");
                ui.separator();
                ui.label("快捷键：Ctrl+Z 撤销 / Ctrl+Y 重做 / Tab 隐藏面板 / ←→ 切换照片");
            });

            ui.separator();
            if ui.button("导入").clicked() {
                self.import_dialog(ui.ctx());
            }
            if ui.button("导出").clicked() {
                self.status = "导出设置（阶段 4 实现）".to_string();
            }
            ui.separator();

            let can_undo = self.engine.history().index() > 0;
            if ui
                .add_enabled(can_undo, egui::Button::new("撤销"))
                .clicked()
            {
                self.undo();
            }
            let can_redo = self.engine.history().index() + 1 < self.engine.history().len();
            if ui
                .add_enabled(can_redo, egui::Button::new("重做"))
                .clicked()
            {
                self.redo();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "画布 {}",
                        if self.fit_to_window {
                            "适应窗口".to_string()
                        } else {
                            format!("{:.0}%", self.zoom * 100.0)
                        }
                    ))
                    .weak(),
                );
            });
        });
    }

    fn left_rail(&mut self, ui: &mut egui::Ui) {
        // 左栏左侧的竖排图标列（参考图：图库 / 预设 / 历史 / 导出 / AI）。
        egui::Panel::left("left_tools")
            .resizable(false)
            .default_size(76.0)
            .show(ui, |ui| self.left_tool_column(ui));
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match self.left_tab {
                LeftTab::Catalog => self.catalog_panel(ui),
                LeftTab::Presets => self.preset_panel(ui),
                LeftTab::History => self.history_panel(ui),
                LeftTab::Export => self.export_panel(ui),
                LeftTab::Ai => self.ai_panel(ui),
            });
    }

    /// 左栏图标列：图库 / 预设 / 历史 / 导出 / AI（竖排，选中高亮）。
    fn left_tool_column(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        for (tab, icon, label) in [
            (LeftTab::Catalog, "▣", "图库"),
            (LeftTab::Presets, "◈", "预设"),
            (LeftTab::History, "↻", "历史"),
            (LeftTab::Export, "↥", "导出"),
            (LeftTab::Ai, "◉", "AI"),
        ] {
            let selected = self.left_tab == tab;
            let text = egui::RichText::new(format!("{icon}\n{label}")).size(15.0);
            let response = ui.add_sized(
                [ui.available_width(), 54.0],
                egui::Button::selectable(selected, text),
            );
            if response.clicked() {
                self.left_tab = tab;
            }
            ui.add_space(2.0);
        }
    }

    fn catalog_panel(&mut self, ui: &mut egui::Ui) {
        if ui.button("导入照片 / 文件夹…").clicked() {
            self.import_dialog(ui.ctx());
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new("当前目录").strong());
        if self.photos.is_empty() {
            ui.weak("（空）把照片拖进窗口即可导入");
        } else {
            for (index, photo) in self.photos.iter().enumerate() {
                let selected = self.selected == Some(index);
                let short = short_name(&photo.name);
                if ui
                    .selectable_label(selected, short.clone())
                    .on_hover_text(photo.tooltip())
                    .clicked()
                {
                    self.selected = Some(index);
                }
            }
        }
        ui.add_space(8.0);
        ui.separator();
        ui.label(egui::RichText::new("收藏夹 / 相册").strong());
        ui.weak("阶段 1：扫描本地目录并建立缩略图缓存");
        ui.weak("阶段 5：移动端同步相册");
    }

    fn history_panel(&mut self, ui: &mut egui::Ui) {
        let entries: Vec<(usize, String, bool)> = self
            .engine
            .history()
            .entries()
            .iter()
            .enumerate()
            .map(|(index, entry)| (index, entry.label.clone(), entry.snapshot))
            .collect();
        let current = self.engine.history().index();

        if ui.button("创建快照").clicked() {
            let count = self
                .engine
                .history()
                .entries()
                .iter()
                .filter(|entry| entry.snapshot)
                .count();
            self.engine.snapshot(format!("快照 {}", count + 1));
            self.status = "已创建历史快照".to_string();
        }
        ui.separator();

        let mut jump_to = None;
        for (index, label, snapshot) in entries {
            let text = if snapshot {
                format!("★ {label}")
            } else {
                label
            };
            if ui.selectable_label(index == current, text).clicked() {
                jump_to = Some(index);
            }
        }
        if let Some(index) = jump_to {
            self.engine.jump_to(index);
        }
    }

    fn right_rail(&mut self, ui: &mut egui::Ui) {
        // 模块工具条固定在右栏最下方（调色面板下面，参考 Lightroom 移动端的底部工具条）。
        egui::Panel::bottom("module_toolbar").show(ui, |ui| {
            ui.add_space(2.0);
            self.module_toolbar(ui);
            ui.add_space(4.0);
        });

        // 直方图与照片信息固定在顶部（三个模块共用，对标 Lightroom 右侧栏顶部）。
        egui::CollapsingHeader::new("直方图")
            .default_open(true)
            .show(ui, |ui| match &self.loaded {
                Some(loaded) => {
                    draw_histogram(ui, &loaded.histogram);
                    ui.add_space(4.0);
                    ui.label(format!(
                        "{}×{} · {} · {} ms",
                        loaded.original[0], loaded.original[1], loaded.source, loaded.elapsed_ms
                    ));
                    ui.weak(format!(
                        "显示代理 {}×{} · {}",
                        loaded.proxy[0],
                        loaded.proxy[1],
                        short_name(&loaded.path)
                    ));
                    if let Some(metadata) = &loaded.metadata {
                        ui.add_space(4.0);
                        ui.weak(metadata.summary());
                        if let Some(captured) = &metadata.captured_at {
                            ui.weak(format!("拍摄：{captured}"));
                        }
                    }
                }
                None => {
                    ui.weak("导入照片后显示实时直方图");
                }
            });
        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // 蒙版编辑状态提示（在蒙版模块下最常用，其他模块也给出口）。
                let editing_mask = match self.edit_target {
                    EditTarget::Mask(index) => self
                        .engine
                        .params()
                        .masks
                        .get(index)
                        .map(|mask| mask.name.clone()),
                    EditTarget::Global => None,
                };
                if let Some(name) = editing_mask {
                    let mut back_to_global = false;
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(0xFF, 0xC8, 0x66),
                            format!("正在编辑蒙版：{name}"),
                        );
                        if ui.small_button("返回全局").clicked() {
                            back_to_global = true;
                        }
                    });
                    if back_to_global {
                        self.exit_mask_mode();
                        self.edit_target = EditTarget::Global;
                        self.brush_painting = false;
                    }
                    ui.separator();
                }

                match self.right_module {
                    RightModule::Edit => self.edit_module(ui),
                    RightModule::Mask => self.mask_module(ui),
                    RightModule::Crop => self.crop_module(ui),
                }
            });
    }

    /// AI 面板：模型状态 / 下载 / 快捷创建 AI 蒙版。
    fn ai_panel(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("AI 蒙版").strong());
        ui.weak("主体 / 天空 / 背景 / 人物 / 物体（本地 ONNX 推理）");
        ui.add_space(6.0);

        #[cfg(feature = "ai")]
        {
            let status = fdxmp_core::ai::status();
            ui.weak(format!(
                "运行库：{}",
                if status.runtime {
                    "就绪"
                } else {
                    "缺少 onnxruntime.dll"
                }
            ));
            ui.weak(format!(
                "模型：主体 {} · 天空 {} · 人物 {}{}",
                if status.subject { "✓" } else { "✗" },
                if status.sky { "✓" } else { "✗" },
                if status.face { "✓" } else { "✗" },
                if status.subject_quality {
                    " · 高质量主体 ✓"
                } else {
                    ""
                }
            ));
            if !status.models_ready()
                && self.ai_download_rx.is_none()
                && ui.button("下载缺失模型（约 313 MB）").clicked()
            {
                self.start_ai_download();
            }
            if self.ai_download_rx.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak(if self.ai_download_status.is_empty() {
                        "正在下载 AI 模型…".to_string()
                    } else {
                        self.ai_download_status.clone()
                    });
                });
            }
            ui.horizontal_wrapped(|ui| {
                if ui.small_button("打开模型目录").clicked() {
                    open_in_explorer(&status.model_dir);
                }
                if !status.subject_quality
                    && self.ai_download_rx.is_none()
                    && ui
                        .small_button("高质量主体模型（213 MB）")
                        .on_hover_text("BiRefNet-lite：更准，但 CPU 推理慢、内存占用高")
                        .clicked()
                {
                    self.start_ai_download_quality();
                }
            });
            ui.separator();
        }

        #[cfg(not(feature = "ai"))]
        {
            ui.weak("当前构建未启用 ai feature：AI 蒙版需要 ONNX Runtime（Windows 端）");
            ui.separator();
        }

        ui.label(egui::RichText::new("快捷创建").strong());
        ui.horizontal_wrapped(|ui| {
            if ui.button("主体").clicked() {
                self.right_module = RightModule::Mask;
                self.create_ai_mask(fdxmp_core::params::MaskKind::AiSubject, "AI 主体");
            }
            if ui.button("天空").clicked() {
                self.right_module = RightModule::Mask;
                self.create_ai_mask(fdxmp_core::params::MaskKind::AiSky, "AI 天空");
            }
            if ui.button("背景").clicked() {
                self.right_module = RightModule::Mask;
                self.create_ai_mask(fdxmp_core::params::MaskKind::AiBackground, "AI 背景");
            }
            if ui.button("人物").clicked() {
                self.right_module = RightModule::Mask;
                self.create_ai_mask(
                    fdxmp_core::params::MaskKind::AiPeople {
                        part: fdxmp_core::params::PeoplePart::Body,
                    },
                    "AI 人物-身体",
                );
            }
            if ui.button("物体").clicked() {
                self.right_module = RightModule::Mask;
                self.create_ai_mask(
                    fdxmp_core::params::MaskKind::AiObject { point: [0.5, 0.5] },
                    "AI 物体",
                );
            }
        });
        ui.add_space(4.0);
        ui.weak("创建后自动切到「蒙版」模块；模型缺失时先在上面下载。");
    }

    /// 模块工具条：编辑 / 蒙版 / 裁切，横排在右栏最下方（调色面板下面）。
    fn module_toolbar(&mut self, ui: &mut egui::Ui) {
        let spacing = 6.0;
        let button_width = ((ui.available_width() - spacing * 2.0).max(60.0)) / 3.0;
        let mut clicked_module = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = spacing;
            for (module, title, hint) in [
                (RightModule::Edit, "编辑", "调色 · 曲线 · HSL · 分级 · 细节"),
                (
                    RightModule::Mask,
                    "蒙版",
                    "画笔 · 渐变 · 颜色 / 亮度范围 · AI",
                ),
                (RightModule::Crop, "裁切", "裁剪 · 旋转 · 透视 · 镜头校正"),
            ] {
                let selected = self.right_module == module;
                let text = egui::RichText::new(title).size(16.0);
                let response = ui
                    .add_sized(
                        [button_width, 40.0],
                        egui::Button::selectable(selected, text),
                    )
                    .on_hover_text(hint);
                if response.clicked() {
                    clicked_module = Some(module);
                }
            }
        });
        if let Some(module) = clicked_module {
            self.right_module = module;
            // 切到蒙版模块只切换面板；进入画布蒙版编辑仍由蒙版面板的按钮触发，
            // 离开蒙版模块时退出画布蒙版视图（避免叠加层残留）。
            if module == RightModule::Mask {
                if self.engine.params().masks.is_empty() {
                    self.status = "蒙版模块：先在上方面板新建一个蒙版".to_string();
                }
            } else if self.mask_mode {
                self.exit_mask_mode();
            }
        }
    }

    /// 编辑模块：基本（Lightroom 分组）+ 效果 + 曲线 / HSL / 分级 / 细节。
    fn edit_module(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("基本")
            .default_open(true)
            .show(ui, |ui| self.basic_panel(ui));
        egui::CollapsingHeader::new("效果（纹理 / 清晰度 / 去雾）")
            .default_open(false)
            .show(ui, |ui| self.effects_panel(ui));
        egui::CollapsingHeader::new("颗粒")
            .default_open(false)
            .show(ui, |ui| self.grain_panel(ui));
        egui::CollapsingHeader::new("暗角")
            .default_open(false)
            .show(ui, |ui| self.vignette_panel(ui));
        egui::CollapsingHeader::new("曲线")
            .default_open(false)
            .show(ui, |ui| self.curve_panel(ui));
        egui::CollapsingHeader::new("HSL / 颜色")
            .default_open(false)
            .show(ui, |ui| self.hsl_panel(ui));
        egui::CollapsingHeader::new("颜色分级")
            .default_open(false)
            .show(ui, |ui| self.grading_panel(ui));
        egui::CollapsingHeader::new("细节（锐化 / 降噪）")
            .default_open(false)
            .show(ui, |ui| self.detail_panel(ui));
    }

    /// 蒙版模块：蒙版列表 + 新建工具 + 选中蒙版的参数。
    fn mask_module(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("蒙版").strong());
        ui.weak("画笔 / 线性 / 径向 / 颜色范围 / 亮度范围 / AI");
        ui.add_space(4.0);
        self.mask_panel(ui);
    }

    /// 剪切模块：裁剪 · 旋转 / 变换（透视 · 翻转）/ 镜头校正。
    fn crop_module(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("裁剪 / 旋转")
            .default_open(true)
            .show(ui, |ui| self.crop_panel(ui));
        egui::CollapsingHeader::new("变换（透视 / 翻转）")
            .default_open(true)
            .show(ui, |ui| self.transform_panel(ui));
        egui::CollapsingHeader::new("镜头校正")
            .default_open(false)
            .show(ui, |ui| self.lens_panel(ui));
    }

    /// 镜头校正面板：畸变 / 色差 / 暗角 / 缩放（手动；配置文件数据库后续引入）。
    fn lens_panel(&mut self, ui: &mut egui::Ui) {
        let lens = self.engine.params().lens.clone();
        let mut distortion = lens.geometry.distortion;
        let mut chromatic = lens.chromatic_aberration;
        let mut vignette = lens.vignette;
        let mut scale = lens.geometry.scale;
        let mut changed = false;
        changed |= ui
            .add(egui::Slider::new(&mut distortion, -100.0..=100.0).text("畸变校正"))
            .changed();
        changed |= ui
            .add(egui::Slider::new(&mut chromatic, -100.0..=100.0).text("色差校正"))
            .changed();
        changed |= ui
            .add(egui::Slider::new(&mut vignette, -100.0..=100.0).text("镜头暗角"))
            .changed();
        changed |= ui
            .add(
                egui::Slider::new(&mut scale, 0.0..=200.0)
                    .text("缩放")
                    .suffix("%"),
            )
            .changed();
        if changed {
            self.edit_geometry_live("镜头校正", |params| {
                params.lens.geometry.distortion = distortion;
                params.lens.chromatic_aberration = chromatic;
                params.lens.vignette = vignette;
                params.lens.geometry.scale = scale;
            });
            self.pending_history = Some("镜头校正".to_string());
        }
        ui.horizontal(|ui| {
            if ui.small_button("重置").clicked() {
                self.edit_geometry_live("镜头校正重置", |params| {
                    params.lens.geometry.distortion = 0.0;
                    params.lens.chromatic_aberration = 0.0;
                    params.lens.vignette = 0.0;
                    params.lens.geometry.scale = 0.0;
                });
                self.pending_history = Some("镜头校正重置".to_string());
            }
            let mut profile = lens.profile_correction;
            ui.add_enabled(
                false,
                egui::Checkbox::new(&mut profile, "配置文件校正（需镜头数据库）"),
            )
            .on_disabled_hover_text("计划 P5 引入 lensfun 数据库后启用");
        });
        ui.weak("畸变 / 色差 / 暗角在 GPU 几何趟实时生效（预览与导出一致）");

        ui.separator();
        ui.label(egui::RichText::new("配置文件校正（LensFun 数据库）").strong());
        self.lens_profile_panel(ui);
    }

    /// 镜头配置文件小节：加载数据库、搜索相机 / 镜头、应用与清除。
    fn lens_profile_panel(&mut self, ui: &mut egui::Ui) {
        if self.lens_db.is_some() {
            let (camera_count, lens_count) = self
                .lens_db
                .as_ref()
                .map(|db| (db.camera_count(), db.lens_count()))
                .unwrap_or((0, 0));
            ui.weak(format!(
                "数据库：{camera_count} 台相机 / {lens_count} 支镜头"
            ));

            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.lens_camera_query)
                        .hint_text("相机关键词，如 EOS R5")
                        .desired_width(150.0),
                );
                if ui.small_button("搜索相机").clicked() {
                    if let Some(db) = self.lens_db.as_ref() {
                        self.lens_cameras = db.find_cameras(None, &self.lens_camera_query);
                        self.lens_cameras.truncate(50);
                    }
                    self.lens_selected_camera = None;
                    self.lens_lenses.clear();
                    self.lens_selected_lens = None;
                }
            });
            if !self.lens_cameras.is_empty() {
                egui::ScrollArea::vertical()
                    .id_salt("lens-camera-list")
                    .max_height(110.0)
                    .show(ui, |ui| {
                        for index in 0..self.lens_cameras.len() {
                            let selected = self.lens_selected_camera == Some(index);
                            let label = {
                                let camera = &self.lens_cameras[index];
                                format!("{} {}", camera.maker, camera.model)
                            };
                            if ui.selectable_label(selected, label).clicked() {
                                self.lens_selected_camera = Some(index);
                                self.lens_lenses.clear();
                                self.lens_selected_lens = None;
                            }
                        }
                    });
            }

            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.lens_lens_query)
                        .hint_text("镜头关键词，如 24-70")
                        .desired_width(150.0),
                );
                if ui.small_button("搜索镜头").clicked() {
                    let camera = self
                        .lens_selected_camera
                        .and_then(|index| self.lens_cameras.get(index))
                        .cloned();
                    if let Some(db) = self.lens_db.as_ref() {
                        self.lens_lenses = db.find_lenses(camera.as_ref(), &self.lens_lens_query);
                        self.lens_lenses.truncate(50);
                    }
                    self.lens_selected_lens = None;
                }
            });
            if !self.lens_lenses.is_empty() {
                egui::ScrollArea::vertical()
                    .id_salt("lens-lens-list")
                    .max_height(110.0)
                    .show(ui, |ui| {
                        for index in 0..self.lens_lenses.len() {
                            let selected = self.lens_selected_lens == Some(index);
                            let label = {
                                let lens = &self.lens_lenses[index];
                                format!("{} {}", lens.maker, lens.model)
                            };
                            if ui.selectable_label(selected, label).clicked() {
                                self.lens_selected_lens = Some(index);
                            }
                        }
                    });
            }

            ui.horizontal(|ui| {
                ui.add(
                    egui::Slider::new(&mut self.lens_focal, 8.0..=600.0)
                        .text("焦距")
                        .suffix("mm"),
                );
                ui.add(
                    egui::Slider::new(&mut self.lens_aperture, 1.0..=32.0)
                        .text("光圈")
                        .prefix("f/"),
                );
            });
            ui.add(
                egui::Slider::new(&mut self.lens_distance, 0.2..=30.0)
                    .text("距离")
                    .suffix("m"),
            );
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.lens_enable_distortion, "畸变");
                ui.checkbox(&mut self.lens_enable_tca, "色差");
                ui.checkbox(&mut self.lens_enable_vignetting, "暗角");
            });

            let camera = self
                .lens_selected_camera
                .and_then(|index| self.lens_cameras.get(index))
                .cloned();
            let lens = self
                .lens_selected_lens
                .and_then(|index| self.lens_lenses.get(index))
                .cloned();
            let can_apply = camera.is_some() && lens.is_some() && !self.lens_applying;
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(can_apply, egui::Button::new("应用配置文件校正"))
                    .clicked()
                {
                    if let (Some(camera), Some(lens)) = (camera.clone(), lens.clone()) {
                        let profile = fdxmp_core::lens_db::LensCorrection {
                            camera,
                            lens,
                            focal: self.lens_focal,
                            aperture: self.lens_aperture,
                            distance: self.lens_distance,
                            distortion: self.lens_enable_distortion,
                            tca: self.lens_enable_tca,
                            vignetting: self.lens_enable_vignetting,
                        };
                        self.lens_profile = Some(profile.clone());
                        self.start_profile_apply(profile);
                    }
                }
                if ui
                    .add_enabled(
                        self.lens_profile.is_some() && !self.lens_applying,
                        egui::Button::new("清除"),
                    )
                    .clicked()
                {
                    self.clear_profile_correction();
                }
                if self.lens_applying {
                    ui.spinner();
                    ui.weak("正在应用…");
                }
            });
            if let Some(profile) = &self.lens_profile {
                ui.weak(format!(
                    "当前配置：{} {} + {}（{:.0}mm f/{:.1}）",
                    profile.camera.maker,
                    profile.camera.model,
                    profile.lens.model,
                    profile.focal,
                    profile.aperture
                ));
            }
        } else if self.lens_db_rx.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("正在加载镜头数据库（内置约 5 MB）…");
            });
        } else if ui.button("加载镜头数据库（内置，首次约 1 秒）").clicked() {
            let (sender, receiver) = channel();
            self.lens_db_rx = Some(receiver);
            std::thread::spawn(move || {
                let result =
                    fdxmp_core::lens_db::LensDatabase::load().map_err(|err| err.to_string());
                let _ = sender.send(result);
            });
        }
    }

    /// 启动配置文件校正：对原始代理做 CPU 校正，完成后替换显示源并重传 GPU。
    fn start_profile_apply(&mut self, profile: fdxmp_core::lens_db::LensCorrection) {
        let Some(source) = self.pristine_proxy.clone().or_else(|| {
            self.loaded
                .as_ref()
                .map(|loaded| loaded.proxy_pixels.clone())
        }) else {
            self.status = "请先打开图片".to_string();
            return;
        };
        let (sender, receiver) = channel();
        self.lens_apply_rx = Some(receiver);
        self.lens_applying = true;
        self.status = "正在应用镜头配置文件校正…".to_string();
        std::thread::spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| profile.apply(&source)))
                    .map_err(|_| "镜头校正线程异常".to_string());
            let _ = sender.send(result);
        });
    }

    /// 清除配置文件校正，恢复原始代理。
    fn clear_profile_correction(&mut self) {
        self.lens_profile = None;
        if let Some(pristine) = self.pristine_proxy.clone() {
            if let Some(loaded) = self.loaded.as_mut() {
                loaded.proxy_pixels = pristine;
            }
            self.reupload_proxy();
            self.status = "已清除镜头配置文件校正".to_string();
        }
    }

    /// 把当前代理重新上传 GPU（配置文件校正 / 清除后调用）。
    #[cfg(feature = "gpu")]
    fn reupload_proxy(&mut self) {
        let uploaded = if let (Some(gpu), Some(loaded)) = (self.gpu.as_mut(), self.loaded.as_ref())
        {
            let pixels = loaded.proxy_pixels.clone();
            gpu.upload(&pixels).is_ok()
        } else {
            false
        };
        if uploaded {
            self.params_dirty = true;
        }
    }

    /// 无 GPU 构建：代理即显示源，无需重传。
    #[cfg(not(feature = "gpu"))]
    fn reupload_proxy(&mut self) {}

    /// 轮询镜头数据库加载与配置文件校正结果。
    fn poll_lens_tasks(&mut self, ctx: &egui::Context) {
        if self.lens_db_rx.is_some() {
            let received = self.lens_db_rx.as_ref().map(|receiver| receiver.try_recv());
            match received {
                Some(Ok(Ok(db))) => {
                    self.lens_db_rx = None;
                    self.lens_db = Some(db);
                    self.status = "镜头数据库已加载".to_string();
                }
                Some(Ok(Err(message))) => {
                    self.lens_db_rx = None;
                    self.status = format!("镜头数据库加载失败：{message}");
                }
                Some(Err(TryRecvError::Empty)) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Some(Err(TryRecvError::Disconnected)) | None => {
                    self.lens_db_rx = None;
                }
            }
        }
        if self.lens_apply_rx.is_some() {
            let received = self
                .lens_apply_rx
                .as_ref()
                .map(|receiver| receiver.try_recv());
            match received {
                Some(Ok(Ok(corrected))) => {
                    self.lens_apply_rx = None;
                    self.lens_applying = false;
                    if let Some(loaded) = self.loaded.as_mut() {
                        loaded.proxy_pixels = corrected;
                    }
                    self.reupload_proxy();
                    self.status = "镜头配置文件校正已应用".to_string();
                    ctx.request_repaint();
                }
                Some(Ok(Err(message))) => {
                    self.lens_apply_rx = None;
                    self.lens_applying = false;
                    self.status = format!("镜头校正失败：{message}");
                }
                Some(Err(TryRecvError::Empty)) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(150));
                }
                Some(Err(TryRecvError::Disconnected)) | None => {
                    self.lens_apply_rx = None;
                    self.lens_applying = false;
                }
            }
        }
    }

    /// 当前编辑目标下的参数（全局或某个蒙版）。
    fn read_params(&self) -> &EditParams {
        match self.edit_target {
            EditTarget::Global => self.engine.params(),
            EditTarget::Mask(index) => self
                .engine
                .params()
                .masks
                .get(index)
                .map(|mask| mask.adjustments.as_ref())
                .unwrap_or_else(|| self.engine.params()),
        }
    }

    /// 实时修改当前编辑目标并标记待提交历史。
    fn edit_params_live(&mut self, history: &str, apply: impl FnOnce(&mut EditParams)) {
        let mut params = self.engine.params().clone();
        match self.edit_target {
            EditTarget::Global => apply(&mut params),
            EditTarget::Mask(index) => {
                if let Some(mask) = params.masks.get_mut(index) {
                    apply(mask.adjustments.as_mut());
                }
            }
        }
        self.engine.set_params_live(params);
        self.params_dirty = true;
        self.pending_history = Some(history.to_string());
    }

    /// 几何 / 镜头类面板：始终作用于全局参数。
    ///
    /// 蒙版不参与几何调整（几何趟只读全局参数），若走 [`Self::edit_params_live`]，
    /// 在蒙版编辑状态下会写进蒙版的死字段——滑块读数来自全局、写入无效，
    /// 表现为"拖动后回弹"。
    fn edit_geometry_live(&mut self, history: &str, apply: impl FnOnce(&mut EditParams)) {
        let mut params = self.engine.params().clone();
        apply(&mut params);
        self.engine.set_params_live(params);
        self.params_dirty = true;
        self.pending_history = Some(history.to_string());
    }

    /// 通用参数滑块：拖动实时预览，松手统一提交一步历史。
    fn param_slider(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        value: f32,
        range: std::ops::RangeInclusive<f32>,
        history: &str,
        mut apply: impl FnMut(&mut EditParams, f32),
    ) {
        let mut current = value;
        let response = ui.add(
            egui::Slider::new(&mut current, range)
                .text(label)
                .fixed_decimals(2),
        );
        if response.changed() {
            self.edit_params_live(history, |params| apply(params, current));
        }
        if response.double_clicked() {
            self.edit_params_live(&format!("重置{history}"), |params| apply(params, 0.0));
        }
    }

    /// 单条调色 / 效果滑杆（基本与效果面板共用）。
    fn slider_row(&mut self, ui: &mut egui::Ui, id: SliderId) {
        let value = self.read_params().slider(id);
        // 色温滑杆附带开尔文读数（0 = 5500K，1 单位 = 55K；蒙版显示合成后的值）
        let label = if id == SliderId::Temperature {
            let effective = match self.edit_target {
                EditTarget::Mask(_) => self.engine.params().color.temperature + value,
                EditTarget::Global => value,
            };
            format!(
                "{} {:.0}K",
                id.label_zh(),
                fdxmp_core::white_balance::relative_to_kelvin(effective)
            )
        } else {
            format!("{} {}", id.label_zh(), id.label_en())
        };
        let history = id.label_zh();
        self.param_slider(ui, &label, value, id.range(), history, move |params, v| {
            params.set_slider(id, v);
        });
    }

    /// 「基本」面板：按 Lightroom 排版分组——白平衡 / 色调 / 偏好。
    fn basic_panel(&mut self, ui: &mut egui::Ui) {
        let groups: [(&str, &[SliderId]); 3] = [
            ("白平衡", &[SliderId::Temperature, SliderId::Tint]),
            (
                "色调",
                &[
                    SliderId::Exposure,
                    SliderId::Contrast,
                    SliderId::Highlights,
                    SliderId::Shadows,
                    SliderId::Whites,
                    SliderId::Blacks,
                ],
            ),
            ("偏好", &[SliderId::Vibrance, SliderId::Saturation]),
        ];
        for (title, sliders) in groups {
            ui.label(egui::RichText::new(title).strong());
            ui.add_space(2.0);
            for &id in sliders {
                self.slider_row(ui, id);
            }
            ui.add_space(6.0);
        }
        if ui.small_button("重置调色").clicked() {
            self.edit_params_live("重置调色", |params| {
                for id in SliderId::ALL {
                    params.set_slider(id, 0.0);
                }
                // 效果子参数回到默认（中点 / 羽化 / 粗糙度 50）
                let defaults = fdxmp_core::params::EffectParams::default();
                params.effects.vignette_midpoint = defaults.vignette_midpoint;
                params.effects.vignette_feather = defaults.vignette_feather;
                params.effects.grain_size = defaults.grain_size;
                params.effects.grain_roughness = defaults.grain_roughness;
            });
        }
    }

    /// 「效果」面板：去雾 / 暗角 / 颗粒（对标 Lightroom 的效果面板）。
    fn effects_panel(&mut self, ui: &mut egui::Ui) {
        // 纹理 / 清晰度放在效果里（与 Lightroom 的「偏好」分开），
        // 暗角与颗粒各自独立成面板（见下）。
        for id in [SliderId::Texture, SliderId::Clarity, SliderId::Dehaze] {
            self.slider_row(ui, id);
        }
    }

    /// 「颗粒」面板：数量 / 大小 / 粗糙度。
    fn grain_panel(&mut self, ui: &mut egui::Ui) {
        let effects = self.read_params().effects.clone();
        self.param_slider(
            ui,
            "数量",
            effects.grain,
            0.0..=100.0,
            "颗粒数量",
            |params, value| {
                params.effects.grain = value;
            },
        );
        self.param_slider(
            ui,
            "大小",
            effects.grain_size,
            0.0..=100.0,
            "颗粒大小",
            |params, value| params.effects.grain_size = value,
        );
        self.param_slider(
            ui,
            "粗糙度",
            effects.grain_roughness,
            0.0..=100.0,
            "颗粒粗糙度",
            |params, value| params.effects.grain_roughness = value,
        );
    }

    /// 「暗角」面板：数量 / 中点 / 羽化（正数量压暗四角，负数量提亮）。
    fn vignette_panel(&mut self, ui: &mut egui::Ui) {
        let effects = self.read_params().effects.clone();
        self.param_slider(
            ui,
            "数量",
            effects.vignette,
            -100.0..=100.0,
            "暗角数量",
            |params, value| params.effects.vignette = value,
        );
        self.param_slider(
            ui,
            "中点",
            effects.vignette_midpoint,
            0.0..=100.0,
            "暗角中点",
            |params, value| params.effects.vignette_midpoint = value,
        );
        self.param_slider(
            ui,
            "羽化",
            effects.vignette_feather,
            0.0..=100.0,
            "暗角羽化",
            |params, value| params.effects.vignette_feather = value,
        );
    }

    /// 预设面板：应用 / 强度 / 导入 XMP / 导出当前参数。
    fn preset_panel(&mut self, ui: &mut egui::Ui) {
        use fdxmp_core::export::ExportSettings;
        use fdxmp_core::preset::PresetFile;

        ui.horizontal(|ui| {
            ui.label("强度");
            ui.add(
                egui::Slider::new(&mut self.preset_strength, 0.0..=100.0)
                    .suffix("%")
                    .fixed_decimals(0),
            );
        });

        ui.horizontal_wrapped(|ui| {
            if ui.button("导入 XMP / .fdxmp…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("预设", &["fdxmp", "xmp"])
                    .pick_file()
                {
                    let loaded = if path.extension().and_then(|ext| ext.to_str()) == Some("xmp") {
                        std::fs::read_to_string(&path)
                            .map_err(|err| err.to_string())
                            .and_then(|text| {
                                PresetFile::from_lightroom_xmp(&text).map_err(|err| err.to_string())
                            })
                    } else {
                        PresetFile::load(&path).map_err(|err| err.to_string())
                    };
                    match loaded {
                        Ok(preset) => {
                            self.apply_preset(&preset);
                            self.status = format!("已导入并应用预设：{}", preset.name);
                        }
                        Err(message) => {
                            self.status = format!("预设导入失败：{message}");
                        }
                    }
                }
            }
            if ui.button("导出当前参数…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("fdxmp", &["fdxmp"])
                    .set_file_name("preset.fdxmp")
                    .save_file()
                {
                    let params = self.engine.params().clone();
                    let preset = PresetFile::new("自定义预设", "fdxmp", params);
                    self.status = match preset.save(&path) {
                        Ok(()) => format!("已导出预设：{}", path.display()),
                        Err(err) => format!("预设导出失败：{err}"),
                    };
                }
            }
        });
        let _ = ExportSettings::default; // 保持依赖显式（导出面板使用同一类型）

        ui.separator();
        ui.label(egui::RichText::new("内置分类").strong());
        let builtins = fdxmp_core::preset::builtin_presets();
        let mut apply_builtin: Option<fdxmp_core::preset::PresetFile> = None;
        egui::Grid::new("preset-thumb-grid")
            .num_columns(2)
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                let mut column = 0;
                for category in ["基础", "人像", "风景", "黑白", "电影", "复古", "HDR"]
                {
                    let Some(preset) = builtins
                        .iter()
                        .find(|preset| preset.name.starts_with(category))
                        .cloned()
                    else {
                        continue;
                    };
                    let thumbnail = self.preset_thumbnail(ui.ctx(), &preset);
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(126.0, 88.0), egui::Sense::click());
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(
                        rect,
                        egui::CornerRadius::from(3u8),
                        egui::Color32::from_gray(0x24),
                    );
                    match thumbnail {
                        Some(texture_id) => {
                            let size = egui::vec2(rect.width() - 8.0, rect.height() - 24.0);
                            let image_rect =
                                egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 4.0), size);
                            painter.image(
                                texture_id,
                                image_rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                        }
                        None => {
                            painter.text(
                                rect.center() - egui::vec2(0.0, 8.0),
                                egui::Align2::CENTER_CENTER,
                                "无预览",
                                egui::FontId::proportional(11.0),
                                egui::Color32::from_gray(0x88),
                            );
                        }
                    }
                    painter.text(
                        rect.left_bottom() + egui::vec2(6.0, -6.0),
                        egui::Align2::LEFT_BOTTOM,
                        category,
                        egui::FontId::proportional(12.0),
                        egui::Color32::from_gray(0xE0),
                    );
                    if response
                        .on_hover_text(format!("应用内置预设：{}", preset.name))
                        .clicked()
                    {
                        apply_builtin = Some(preset);
                    }
                    column += 1;
                    if column % 2 == 0 {
                        ui.end_row();
                    }
                }
            });
        if let Some(preset) = apply_builtin {
            self.apply_preset(&preset);
            self.status = format!(
                "已应用内置预设：{}（强度 {:.0}%）",
                preset.name, self.preset_strength
            );
        }

        ui.add_space(4.0);
        ui.label(egui::RichText::new("用户预设（presets/）").strong());
        let preset_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|crates| crates.parent())
            .map(|root| root.join("presets"));
        let mut applied: Option<PresetFile> = None;
        if let Some(dir) = preset_dir {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                let mut found = false;
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|ext| ext.to_str()) != Some("fdxmp") {
                        continue;
                    }
                    found = true;
                    let name = path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("preset")
                        .to_string();
                    if ui
                        .selectable_label(false, name)
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        match PresetFile::load(&path) {
                            Ok(preset) => applied = Some(preset),
                            Err(err) => self.status = format!("预设读取失败：{err}"),
                        }
                    }
                }
                if !found {
                    ui.weak("presets/ 目录下暂无 .fdxmp 文件");
                }
            } else {
                ui.weak("未找到 presets/ 目录");
            }
        }
        if let Some(preset) = applied {
            self.apply_preset(&preset);
            self.status = format!(
                "已应用预设：{}（强度 {:.0}%）",
                preset.name, self.preset_strength
            );
        }
    }

    /// 预设缩略图：把当前照片缩小后用该预设渲染一次（缓存，含强度档位）。
    #[cfg(feature = "gpu")]
    fn preset_thumbnail(
        &mut self,
        ctx: &egui::Context,
        preset: &fdxmp_core::preset::PresetFile,
    ) -> Option<egui::TextureId> {
        let loaded = self.loaded.as_ref()?;
        let strength = (self.preset_strength / 100.0).clamp(0.0, 1.0);
        let key = format!(
            "{}|{}|{:.0}",
            loaded.path,
            preset.name,
            (strength * 20.0).round()
        );
        if let Some(texture) = self.preset_thumbs.get(&key) {
            return Some(texture.id());
        }
        if self.preset_thumbs.len() > 96 {
            self.preset_thumbs.clear();
        }

        // 基准小图：同一张照片只缩放一次。
        let base_key = format!("{}|thumb-base", loaded.path);
        let needs_base = self
            .preset_base
            .as_ref()
            .map(|(cached, _)| cached != &base_key)
            .unwrap_or(true);
        if needs_base {
            let (proxy_w, proxy_h) = (loaded.proxy[0].max(1), loaded.proxy[1].max(1));
            let width = 128u32;
            let height = ((proxy_h as f32 / proxy_w as f32) * width as f32)
                .round()
                .max(1.0) as u32;
            let base = image::imageops::resize(
                &loaded.proxy_pixels,
                width,
                height,
                image::imageops::FilterType::Triangle,
            );
            self.preset_base = Some((base_key, base));
        }
        let base = self.preset_base.as_ref().map(|(_, image)| image.clone())?;

        if self.thumb_gpu.is_none() {
            self.thumb_gpu = Some(GpuPreview::new(&self.gpu.as_ref()?.state));
        }
        let gpu = self.thumb_gpu.as_mut()?;
        let mut params = self.engine.params().blend(&preset.params, strength);
        params.masks.clear();
        gpu.upload(&base).ok()?;
        gpu.render(&params, fdxmp_core::geometry::GeometryMode::Full);
        let rgba = gpu.readback()?;
        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            rgba.as_raw(),
        );
        let texture = ctx.load_texture(key.clone(), color_image, egui::TextureOptions::LINEAR);
        let id = texture.id();
        self.preset_thumbs.insert(key, texture);
        Some(id)
    }

    /// 无 GPU 构建：不渲染缩略图。
    #[cfg(not(feature = "gpu"))]
    fn preset_thumbnail(
        &mut self,
        _ctx: &egui::Context,
        _preset: &fdxmp_core::preset::PresetFile,
    ) -> Option<egui::TextureId> {
        None
    }

    /// 按当前强度把预设混入全局参数（保留蒙版）。
    fn apply_preset(&mut self, preset: &fdxmp_core::preset::PresetFile) {
        let strength = (self.preset_strength / 100.0).clamp(0.0, 1.0);
        let blended = self.engine.params().blend(&preset.params, strength);
        self.engine.set_params_live(blended);
        self.engine
            .commit_history(format!("应用预设 {}", preset.name));
        self.params_dirty = true;
    }

    /// 导出面板。
    fn export_panel(&mut self, ui: &mut egui::Ui) {
        use fdxmp_core::export::{ColorSpace, ExportFormat, MetadataPolicy, ResizeSpec, Watermark};

        ui.horizontal(|ui| {
            ui.label("格式");
            egui::ComboBox::from_id_salt("export-format")
                .selected_text(self.export_settings.format.label_zh())
                .show_ui(ui, |ui| {
                    for format in [
                        ExportFormat::Jpeg,
                        ExportFormat::Png,
                        ExportFormat::Tiff,
                        ExportFormat::Heic,
                        ExportFormat::WebP,
                    ] {
                        ui.selectable_value(
                            &mut self.export_settings.format,
                            format,
                            format.label_zh(),
                        );
                    }
                });
        });
        if let Some(value) =
            value_slider(ui, "质量", self.export_settings.quality as f32, 1.0..=100.0)
        {
            self.export_settings.quality = value.clamp(1.0, 100.0) as u8;
        }
        if self.export_settings.format == fdxmp_core::export::ExportFormat::WebP {
            ui.weak("WebP 目前使用无损编码，质量参数不生效");
        }

        ui.horizontal(|ui| {
            ui.label("尺寸");
            egui::ComboBox::from_id_salt("export-resize")
                .selected_text(match self.export_settings.resize {
                    ResizeSpec::Original => "原始".to_string(),
                    ResizeSpec::LongEdge(edge) => format!("长边 {edge}"),
                    ResizeSpec::ShortEdge(edge) => format!("短边 {edge}"),
                    ResizeSpec::Percent(percent) => format!("{percent:.0}%"),
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            matches!(self.export_settings.resize, ResizeSpec::Original),
                            "原始",
                        )
                        .clicked()
                    {
                        self.export_settings.resize = ResizeSpec::Original;
                    }
                    for (label, spec) in [
                        ("长边 4096", ResizeSpec::LongEdge(4096)),
                        ("长边 2048", ResizeSpec::LongEdge(2048)),
                        ("短边 1080", ResizeSpec::ShortEdge(1080)),
                        ("50%", ResizeSpec::Percent(50.0)),
                        ("25%", ResizeSpec::Percent(25.0)),
                    ] {
                        if ui
                            .selectable_label(self.export_settings.resize == spec, label)
                            .clicked()
                        {
                            self.export_settings.resize = spec;
                        }
                    }
                });
        });

        ui.horizontal(|ui| {
            ui.label("色彩空间");
            egui::ComboBox::from_id_salt("export-colorspace")
                .selected_text(self.export_settings.color_space.label_zh())
                .show_ui(ui, |ui| {
                    for space in [
                        ColorSpace::Srgb,
                        ColorSpace::DisplayP3,
                        ColorSpace::AdobeRgb,
                        ColorSpace::ProPhotoRgb,
                    ] {
                        ui.selectable_value(
                            &mut self.export_settings.color_space,
                            space,
                            space.label_zh(),
                        );
                    }
                });
        });
        let mut keep_metadata = self.export_settings.metadata == MetadataPolicy::Keep;
        if ui
            .checkbox(
                &mut keep_metadata,
                "保留元数据（EXIF / XMP 写回 JPEG、PNG；RAW 取内嵌预览元数据）",
            )
            .changed()
        {
            self.export_settings.metadata = if keep_metadata {
                MetadataPolicy::Keep
            } else {
                MetadataPolicy::Strip
            };
        }

        ui.horizontal(|ui| {
            ui.label("水印");
            egui::ComboBox::from_id_salt("export-watermark")
                .selected_text(match self.export_settings.watermark {
                    Watermark::None => "无".to_string(),
                    Watermark::Text { .. } => "文字".to_string(),
                    Watermark::Image { .. } => "图片".to_string(),
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            matches!(self.export_settings.watermark, Watermark::None),
                            "无",
                        )
                        .clicked()
                    {
                        self.export_settings.watermark = Watermark::None;
                    }
                    if ui
                        .selectable_label(
                            matches!(self.export_settings.watermark, Watermark::Text { .. }),
                            "文字",
                        )
                        .clicked()
                    {
                        self.export_settings.watermark = Watermark::Text {
                            text: "fdxmp".to_string(),
                            opacity: 70.0,
                            size: 28,
                        };
                    }
                    if ui
                        .selectable_label(
                            matches!(self.export_settings.watermark, Watermark::Image { .. }),
                            "图片",
                        )
                        .clicked()
                    {
                        self.export_settings.watermark = Watermark::Image {
                            path: String::new(),
                            opacity: 70.0,
                        };
                    }
                });
        });
        match self.export_settings.watermark.clone() {
            Watermark::None => {}
            Watermark::Text {
                text,
                opacity,
                size,
            } => {
                let mut new_text = text.clone();
                if ui.text_edit_singleline(&mut new_text).changed() {
                    self.export_settings.watermark = Watermark::Text {
                        text: new_text,
                        opacity,
                        size,
                    };
                }
                let mut new_opacity = opacity;
                if ui
                    .add(egui::Slider::new(&mut new_opacity, 0.0..=100.0).text("水印不透明度"))
                    .changed()
                {
                    self.export_settings.watermark = Watermark::Text {
                        text: text.clone(),
                        opacity: new_opacity,
                        size,
                    };
                }
                let mut new_size = size;
                if ui
                    .add(egui::Slider::new(&mut new_size, 8..=96).text("字号"))
                    .changed()
                {
                    self.export_settings.watermark = Watermark::Text {
                        text: text.clone(),
                        opacity,
                        size: new_size,
                    };
                }
            }
            Watermark::Image { path, opacity } => {
                if ui.button("选择水印图片…").clicked() {
                    if let Some(picked) = rfd::FileDialog::new()
                        .add_filter("PNG", &["png"])
                        .pick_file()
                    {
                        self.export_settings.watermark = Watermark::Image {
                            path: picked.display().to_string(),
                            opacity,
                        };
                    }
                }
                ui.weak(if path.is_empty() {
                    "未选择水印图片".to_string()
                } else {
                    path.clone()
                });
            }
        }

        ui.horizontal(|ui| {
            if ui.button("选择输出目录…").clicked() {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    self.export_settings.output_dir = dir.display().to_string();
                }
            }
            ui.weak(if self.export_settings.output_dir.is_empty() {
                "默认当前目录".to_string()
            } else {
                self.export_settings.output_dir.clone()
            });
        });
        ui.checkbox(
            &mut self.export_full_resolution,
            "全分辨率导出（重新解码 + GPU 重渲染，较慢）",
        );

        let busy = self.export_rx.is_some();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!busy, egui::Button::new("导出当前图片"))
                .clicked()
            {
                self.start_export(ui.ctx());
            }
            let marked_count = self.marked.len();
            if ui
                .add_enabled(
                    !busy && marked_count > 0,
                    egui::Button::new(format!("批量导出标记照片（{marked_count}）")),
                )
                .clicked()
            {
                self.start_batch_export(ui.ctx());
            }
            if busy {
                ui.spinner();
                ui.weak("导出中…");
            }
        });
        ui.weak("提示：胶片条缩略图左上角勾选可标记照片，批量导出按当前参数逐张渲染。");
    }

    /// 批量导出标记照片（工作线程；共享一个 GPU 上下文逐张渲染）。
    fn start_batch_export(&mut self, ctx: &egui::Context) {
        let items: Vec<(String, PathBuf)> = self
            .marked
            .iter()
            .filter_map(|&index| {
                self.photos
                    .get(index)
                    .and_then(|photo| photo.path.clone().map(|path| (photo.name.clone(), path)))
            })
            .collect();
        if items.is_empty() {
            self.status = "请先在胶片条标记要导出的照片".to_string();
            return;
        }
        let settings = self.export_settings.clone();
        #[cfg(feature = "gpu")]
        let params = self.engine.params().clone();
        let lens_profile = self.lens_profile.clone();
        let full_resolution = self.export_full_resolution;
        let count = items.len();
        let (sender, receiver) = channel();
        self.export_rx = Some(receiver);
        self.status = format!("批量导出 {count} 张…");
        ctx.request_repaint();

        std::thread::spawn(move || {
            let result = (|| -> Result<String, String> {
                #[cfg(feature = "gpu")]
                let gpu = fdxmp_core::gpu::GpuContext::new_blocking()
                    .map_err(|err| format!("GPU 初始化失败：{err}"))?;
                let mut exported = 0usize;
                let mut total_bytes = 0u64;
                for (name, path) in &items {
                    let decoded = if full_resolution {
                        fdxmp_core::decode::decode(path)
                    } else {
                        fdxmp_core::decode::decode_preview(path)
                    }
                    .map_err(|err| format!("{name}: {err}"))?;
                    #[cfg(feature = "gpu")]
                    let rendered = {
                        use fdxmp_core::gpu_pipeline::ColorPipeline;
                        let mut pipeline = ColorPipeline::new(&gpu.device);
                        // 有镜头配置文件时走 8bit CPU 校正通路，否则直接上传线性 f32
                        // （16bit / RAW 少一次 8bit 量化）。
                        let image = match &lens_profile {
                            Some(profile) => {
                                let source = if full_resolution {
                                    decoded.pixels.to_rgba8()
                                } else {
                                    decoded.display_proxy(2048)
                                };
                                let source = profile.apply(&source);
                                pipeline.upload(
                                    &gpu.device,
                                    &gpu.queue,
                                    source.as_raw(),
                                    source.width(),
                                    source.height(),
                                )
                            }
                            None => {
                                let linear = if full_resolution {
                                    decoded.pixels_linear()
                                } else {
                                    decoded.display_proxy_linear(2048)
                                };
                                let (width, height) = linear.dimensions();
                                pipeline.upload_linear_f32(
                                    &gpu.device,
                                    &gpu.queue,
                                    linear.as_raw(),
                                    width,
                                    height,
                                )
                            }
                        }
                        .map_err(|err| err.to_string())?;
                        pipeline.render_with(
                            &gpu.device,
                            &gpu.queue,
                            &image,
                            &params,
                            fdxmp_core::geometry::GeometryMode::Crop,
                        );
                        let (out_width, out_height) = pipeline
                            .target_size()
                            .ok_or_else(|| "渲染目标缺失".to_string())?;
                        let pixels = pipeline
                            .readback(&gpu.device, &gpu.queue, &image)
                            .map_err(|err| err.to_string())?;
                        image::RgbaImage::from_raw(out_width, out_height, pixels)
                            .ok_or_else(|| "渲染结果尺寸不匹配".to_string())?
                    };
                    #[cfg(not(feature = "gpu"))]
                    let rendered = {
                        let source = if full_resolution {
                            decoded.pixels.to_rgba8()
                        } else {
                            decoded.display_proxy(2048)
                        };
                        match &lens_profile {
                            Some(profile) => profile.apply(&source),
                            None => source,
                        }
                    };
                    let stem = path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("export")
                        .to_string();
                    let outcome = fdxmp_core::export::export_rgba_with_source(
                        &rendered,
                        &stem,
                        &settings,
                        Some(path.as_path()),
                    )
                    .map_err(|err| err.to_string())?;
                    exported += 1;
                    total_bytes += outcome.bytes as u64;
                }
                Ok(format!(
                    "批量导出完成：{exported} 张，共 {} KB",
                    total_bytes / 1024
                ))
            })();
            let _ = sender.send(result);
        });
    }

    /// 启动导出（工作线程；全分辨率时重新解码 + 独立 GPU 上下文渲染）。
    fn start_export(&mut self, ctx: &egui::Context) {
        let Some(loaded) = self.loaded.as_ref() else {
            self.status = "请先打开一张图片".to_string();
            return;
        };
        let source_path = self
            .selected
            .and_then(|index| self.photos.get(index))
            .and_then(|photo| photo.path.clone());
        let stem = source_path
            .as_ref()
            .and_then(|path| path.file_stem())
            .and_then(|stem| stem.to_str())
            .unwrap_or("export")
            .to_string();
        let settings = self.export_settings.clone();
        let params = self.engine.params().clone();
        let lens_profile = self.lens_profile.clone();

        // 代理尺寸的渲染结果（GPU 回读优先，其次 CPU 纹理对应的像素）
        #[cfg(feature = "gpu")]
        let preview_pixels = {
            // 确保回读的是裁剪后的结果（裁剪模式下画布在渲染全幅视图）。
            if let Some(gpu) = self.gpu.as_mut() {
                let params = self.engine.params().clone();
                gpu.render(&params, fdxmp_core::geometry::GeometryMode::Crop);
            }
            self.gpu.as_ref().and_then(|gpu| gpu.readback())
        };
        let fallback = loaded.proxy_pixels.clone();
        let full_resolution = self.export_full_resolution;

        let (sender, receiver) = channel();
        self.export_rx = Some(receiver);
        self.status = "正在导出…".to_string();
        ctx.request_repaint();

        std::thread::spawn(move || {
            let result = (|| -> Result<String, String> {
                let rendered = if full_resolution {
                    let path = source_path
                        .as_ref()
                        .ok_or_else(|| "缺少源文件路径".to_string())?;
                    render_full_resolution(path, &params, lens_profile.as_ref())?
                } else {
                    #[cfg(feature = "gpu")]
                    let rendered = preview_pixels.unwrap_or(fallback);
                    #[cfg(not(feature = "gpu"))]
                    let rendered = fallback;
                    rendered
                };
                let outcome = fdxmp_core::export::export_rgba_with_source(
                    &rendered,
                    &stem,
                    &settings,
                    source_path.as_deref(),
                )
                .map_err(|err| err.to_string())?;
                Ok(format!(
                    "已导出 {}×{}（{}，{} KB）→ {}",
                    outcome.width,
                    outcome.height,
                    settings.format.label_zh(),
                    outcome.bytes / 1024,
                    outcome.path.display()
                ))
            })();
            let _ = sender.send(result);
        });
    }

    fn poll_export(&mut self, ctx: &egui::Context) {
        let Some(receiver) = &self.export_rx else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(message)) => {
                self.export_rx = None;
                self.status = message;
            }
            Ok(Err(message)) => {
                self.export_rx = None;
                self.status = format!("导出失败：{message}");
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            Err(TryRecvError::Disconnected) => {
                self.export_rx = None;
            }
        }
    }

    /// 当前选中的蒙版。
    fn selected_mask(&self) -> Option<&fdxmp_core::params::MaskParams> {
        match self.edit_target {
            EditTarget::Mask(index) => self.engine.params().masks.get(index),
            EditTarget::Global => None,
        }
    }

    /// 实时修改选中蒙版的自身参数（类型 / 羽化 / 不透明度等）。
    fn edit_mask_live(
        &mut self,
        history: &str,
        apply: impl FnOnce(&mut fdxmp_core::params::MaskParams),
    ) {
        if let EditTarget::Mask(index) = self.edit_target {
            let mut params = self.engine.params().clone();
            if let Some(mask) = params.masks.get_mut(index) {
                apply(mask);
            }
            self.engine.set_params_live(params);
            self.params_dirty = true;
            self.pending_history = Some(history.to_string());
        }
    }

    fn create_mask(&mut self, kind: fdxmp_core::params::MaskKind, name: &str) {
        let mut params = self.engine.params().clone();
        let index = params.masks.len();
        let is_brush = matches!(kind, fdxmp_core::params::MaskKind::Brush(_));
        params.masks.push(fdxmp_core::params::MaskParams {
            id: format!("mask-{}", index + 1),
            name: name.to_string(),
            kind,
            ..fdxmp_core::params::MaskParams::default()
        });
        self.engine.set_params_live(params);
        self.engine.commit_history(format!("新建{name}蒙版"));
        self.edit_target = EditTarget::Mask(index);
        // 新建蒙版即进入独立蒙版编辑视图（对标 Lightroom）。
        self.brush_painting = is_brush;
        self.mask_restore_overlay = self.show_overlay;
        self.show_overlay = true;
        self.mask_mode = true;
        self.mask_drag = None;
        self.crop_mode = false;
        self.params_dirty = true;
        self.status = format!("已新建{name}蒙版：在画面上编辑，M / 回车完成");
    }

    /// 进入独立蒙版编辑视图（画面叠加 + 底部蒙版工具条）。
    fn enter_mask_mode(&mut self, index: Option<usize>) {
        let masks_len = self.engine.params().masks.len();
        let index = match index {
            Some(index) if index < masks_len => Some(index),
            _ => match self.edit_target {
                EditTarget::Mask(current) if current < masks_len => Some(current),
                _ => masks_len.checked_sub(1),
            },
        };
        let Some(index) = index else {
            self.status = "请先创建一个蒙版".to_string();
            return;
        };
        self.edit_target = EditTarget::Mask(index);
        self.mask_mode = true;
        self.mask_drag = None;
        self.crop_mode = false;
        if self.detail_mode {
            self.detail_mode = false;
            self.detail_full = None;
            self.detail_rx = None;
            self.detail_tile_rect = None;
        }
        self.mask_restore_overlay = self.show_overlay;
        self.show_overlay = true;
        self.brush_painting = matches!(
            self.engine.params().masks.get(index).map(|mask| &mask.kind),
            Some(fdxmp_core::params::MaskKind::Brush(_))
        );
        self.params_dirty = true;
        self.status = "蒙版编辑视图：涂抹 / 拖动编辑蒙版，O 切换叠加，M / 回车完成".to_string();
    }

    /// 退出蒙版编辑视图（保留蒙版与调整，恢复进入前的叠加设置）。
    fn exit_mask_mode(&mut self) {
        if !self.mask_mode {
            return;
        }
        self.mask_mode = false;
        self.mask_drag = None;
        self.brush_painting = false;
        self.show_overlay = self.mask_restore_overlay;
        if self.pending_history.is_none() {
            self.pending_history = Some("蒙版编辑".to_string());
        }
        self.params_dirty = true;
        self.status = "已完成蒙版编辑（返回原编辑视图）".to_string();
    }

    /// 复制蒙版（工具条与蒙版面板共用）。
    fn duplicate_mask(&mut self, index: usize) {
        let mut params = self.engine.params().clone();
        if let Some(mask) = params.masks.get(index).cloned() {
            let mut copy = mask;
            copy.id = format!("mask-{}-copy-{}", index + 1, params.masks.len() + 1);
            copy.name = format!("{} 副本", copy.name);
            params.masks.push(copy);
            let new_index = params.masks.len() - 1;
            self.engine.set_params_live(params);
            self.engine.commit_history("复制蒙版");
            self.edit_target = EditTarget::Mask(new_index);
            self.brush_painting = false;
            self.params_dirty = true;
        }
    }

    /// 创建 AI 蒙版并触发生成。
    fn create_ai_mask(&mut self, kind: fdxmp_core::params::MaskKind, name: &str) {
        self.create_mask(kind, name);
        self.request_ai_mask();
    }

    /// 请求为当前选中的 AI 蒙版生成权重（工作线程）。
    fn request_ai_mask(&mut self) {
        #[cfg(feature = "ai")]
        {
            if self.ai_rx.is_some() {
                return;
            }
            let Some(loaded) = self.loaded.as_ref() else {
                return;
            };
            let EditTarget::Mask(index) = self.edit_target else {
                return;
            };
            let Some(mask) = self.engine.params().masks.get(index).cloned() else {
                return;
            };
            if !is_ai_mask(&mask.kind) {
                return;
            }
            let (width, height) = (loaded.proxy[0], loaded.proxy[1]);
            if fdxmp_core::ai::cached_raster(&mask, width, height).is_some() {
                self.params_dirty = true;
                return;
            }
            let status = fdxmp_core::ai::status();
            if !status.runtime || !status.models_ready() {
                self.status =
                    "AI 运行库或模型缺失：请在左栏「AI」面板下载模型 / 检查 onnxruntime.dll"
                        .to_string();
                return;
            }
            let pixels = loaded.proxy_pixels.clone();
            let label = mask.name.clone();
            let (sender, receiver) = channel();
            self.ai_rx = Some(receiver);
            self.ai_running = Some(label.clone());
            self.status = format!("正在生成 {label}（AI 推理，首次约 2–5 秒）…");
            std::thread::spawn(move || {
                let result = fdxmp_core::ai::generate_into_cache(&mask, &pixels)
                    .map(|_| label)
                    .map_err(|err| err.to_string());
                let _ = sender.send(result);
            });
        }
    }

    /// 选中的 AI 蒙版没有缓存时自动生成（每帧安全检查，带防抖由 ai_rx 保证）。
    fn maybe_request_ai_mask(&mut self) {
        #[cfg(feature = "ai")]
        {
            if self.ai_rx.is_some() {
                return;
            }
            let Some(loaded) = self.loaded.as_ref() else {
                return;
            };
            let EditTarget::Mask(index) = self.edit_target else {
                return;
            };
            let Some(mask) = self.engine.params().masks.get(index).cloned() else {
                return;
            };
            if !is_ai_mask(&mask.kind) {
                return;
            }
            if fdxmp_core::ai::cached_raster(&mask, loaded.proxy[0], loaded.proxy[1]).is_some() {
                return;
            }
            self.request_ai_mask();
        }
    }

    /// 下载缺失的 AI 模型（hf-mirror）。
    #[cfg(feature = "ai")]
    fn start_ai_download(&mut self) {
        #[cfg(feature = "ai")]
        {
            let (sender, receiver) = channel();
            self.ai_download_rx = Some(receiver);
            self.ai_download_status = "准备下载…".to_string();
            std::thread::spawn(move || {
                let result = fdxmp_core::ai::download_models(|file, received, total| {
                    let message = if total > 0 {
                        format!(
                            "{file} {:.0}%（{:.1}/{:.1} MB）",
                            received as f64 / total as f64 * 100.0,
                            received as f64 / 1_048_576.0,
                            total as f64 / 1_048_576.0
                        )
                    } else {
                        format!("{file} 下载中…")
                    };
                    let _ = sender.send(Ok(message));
                })
                .map(|files| format!("模型下载完成（{} 个文件）", files.len()))
                .map_err(|err| err.to_string());
                let _ = sender.send(result);
            });
        }
    }

    /// 下载高质量主体模型（BiRefNet-lite，约 213 MB）。
    #[cfg(feature = "ai")]
    fn start_ai_download_quality(&mut self) {
        let (sender, receiver) = channel();
        self.ai_download_rx = Some(receiver);
        self.ai_download_status = "准备下载高质量主体模型…".to_string();
        std::thread::spawn(move || {
            let result = fdxmp_core::ai::download_quality_subject(|file, received, total| {
                let message = if total > 0 {
                    format!(
                        "{file} {:.0}%（{:.1}/{:.1} MB）",
                        received as f64 / total as f64 * 100.0,
                        received as f64 / 1_048_576.0,
                        total as f64 / 1_048_576.0
                    )
                } else {
                    format!("{file} 下载中…")
                };
                let _ = sender.send(Ok(message));
            })
            .map(|_| "高质量主体模型下载完成（设 FDXMP_AI_SUBJECT=hq 后启用）".to_string())
            .map_err(|err| err.to_string());
            let _ = sender.send(result);
        });
    }

    /// 轮询 AI 生成 / 下载结果。
    #[cfg_attr(not(feature = "ai"), allow(unused_variables))]
    fn poll_ai_tasks(&mut self, ctx: &egui::Context) {
        #[cfg(feature = "ai")]
        {
            if self.ai_rx.is_some() {
                let received = self.ai_rx.as_ref().map(|receiver| receiver.try_recv());
                match received {
                    Some(Ok(Ok(label))) => {
                        self.ai_rx = None;
                        self.ai_running = None;
                        self.params_dirty = true;
                        self.status = format!("{label} 已生成");
                        ctx.request_repaint();
                    }
                    Some(Ok(Err(message))) => {
                        self.ai_rx = None;
                        self.ai_running = None;
                        self.status = format!("AI 蒙版生成失败：{message}");
                    }
                    Some(Err(TryRecvError::Empty)) => {
                        ctx.request_repaint_after(std::time::Duration::from_millis(200));
                    }
                    Some(Err(TryRecvError::Disconnected)) | None => {
                        self.ai_rx = None;
                        self.ai_running = None;
                    }
                }
            }
            if self.ai_download_rx.is_some() {
                let mut finished = false;
                let mut last_message = None;
                if let Some(receiver) = self.ai_download_rx.as_ref() {
                    loop {
                        match receiver.try_recv() {
                            Ok(Ok(message)) => last_message = Some(message),
                            Ok(Err(message)) => {
                                self.status = format!("AI 模型下载失败：{message}");
                                finished = true;
                                break;
                            }
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => {
                                finished = true;
                                break;
                            }
                        }
                    }
                }
                if let Some(message) = last_message {
                    self.ai_download_status = message.clone();
                    if message.contains("完成") {
                        self.status = message;
                        ctx.request_repaint();
                    }
                }
                if finished {
                    self.ai_download_rx = None;
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_millis(300));
                }
            }
        }
    }

    /// 删除蒙版；删空后自动退出蒙版视图。
    fn delete_mask(&mut self, index: usize) {
        let mut params = self.engine.params().clone();
        if index < params.masks.len() {
            params.masks.remove(index);
            self.engine.set_params_live(params);
            self.engine.commit_history("删除蒙版");
            self.params_dirty = true;
        }
        self.edit_target = EditTarget::Global;
        self.brush_painting = false;
        self.mask_drag = None;
        if self.engine.params().masks.is_empty() {
            self.mask_mode = false;
            self.show_overlay = self.mask_restore_overlay;
        }
    }

    /// 蒙版工具条上的工具：同类型继续编辑，否则新建。
    fn apply_mask_tool(&mut self, tool: MaskTool) {
        if let EditTarget::Mask(index) = self.edit_target {
            if let Some(mask) = self.engine.params().masks.get(index) {
                if mask_tool_matches(tool, &mask.kind) {
                    self.mask_mode = true;
                    self.brush_painting = tool == MaskTool::Brush;
                    self.params_dirty = true;
                    return;
                }
            }
        }
        let (kind, name) = mask_tool_default(tool);
        self.create_mask(kind, name);
    }

    /// 蒙版编辑视图底部工具条（新建工具 / 叠加 / 颜色 / 复制 / 删除 / 完成）。
    fn mask_toolbar(&mut self, ui: &mut egui::Ui, canvas_rect: egui::Rect) {
        use fdxmp_core::params::{MaskKind, OverlayColor};
        let mut tool: Option<MaskTool> = None;
        let mut overlay_toggle: Option<bool> = None;
        let mut overlay_color: Option<OverlayColor> = None;
        let mut duplicate = false;
        let mut delete = false;
        let mut finish = false;
        let mut brush_change: Option<(usize, f32)> = None;

        let anchor = egui::pos2(canvas_rect.center().x - 330.0, canvas_rect.bottom() - 108.0);
        let current = match self.edit_target {
            EditTarget::Mask(index) => self.engine.params().masks.get(index).cloned(),
            EditTarget::Global => None,
        };
        egui::Area::new(egui::Id::new("fdxmp-mask-toolbar"))
            .fixed_pos(anchor)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(660.0);
                    ui.horizontal(|ui| {
                        ui.label("新建");
                        for (label, value) in [
                            ("画笔", MaskTool::Brush),
                            ("线性", MaskTool::Linear),
                            ("径向", MaskTool::Radial),
                            ("颜色范围", MaskTool::ColorRange),
                            ("亮度范围", MaskTool::LuminanceRange),
                        ] {
                            if ui.small_button(label).clicked() {
                                tool = Some(value);
                            }
                        }
                        ui.separator();
                        let mut overlay = self.show_overlay;
                        if ui.checkbox(&mut overlay, "显示叠加 (O)").changed() {
                            overlay_toggle = Some(overlay);
                        }
                        ui.separator();
                        ui.label("叠加色");
                        for color in [
                            OverlayColor::Red,
                            OverlayColor::Green,
                            OverlayColor::Blue,
                            OverlayColor::White,
                            OverlayColor::Black,
                        ] {
                            let rgba = fdxmp_core::masks::overlay_rgba(color);
                            let (rect, response) = ui
                                .allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                            let selected = current
                                .as_ref()
                                .map(|mask| mask.overlay == color)
                                .unwrap_or(false);
                            ui.painter().rect_filled(
                                rect,
                                egui::CornerRadius::from(2u8),
                                egui::Color32::from_rgb(
                                    (rgba[0] * 255.0) as u8,
                                    (rgba[1] * 255.0) as u8,
                                    (rgba[2] * 255.0) as u8,
                                ),
                            );
                            if selected {
                                ui.painter().rect_stroke(
                                    rect,
                                    egui::CornerRadius::from(2u8),
                                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            if response.clicked() {
                                overlay_color = Some(color);
                            }
                        }
                        ui.separator();
                        if ui.small_button("复制").clicked() {
                            duplicate = true;
                        }
                        if ui.small_button("删除").clicked() {
                            delete = true;
                        }
                        if ui.button("完成").clicked() {
                            finish = true;
                        }
                    });
                    ui.separator();
                    match current.as_ref().map(|mask| mask.kind.clone()) {
                        Some(MaskKind::Brush(brush)) => {
                            let mut size = brush.size;
                            let mut feather = brush.feather;
                            let mut flow = brush.flow;
                            let mut density = brush.density;
                            ui.horizontal(|ui| {
                                if ui
                                    .add(
                                        egui::Slider::new(&mut size, 2.0..=50.0)
                                            .text("大小")
                                            .fixed_decimals(0),
                                    )
                                    .changed()
                                {
                                    brush_change = Some((0, size));
                                }
                                if ui
                                    .add(
                                        egui::Slider::new(&mut feather, 0.0..=100.0)
                                            .text("羽化")
                                            .fixed_decimals(0),
                                    )
                                    .changed()
                                {
                                    brush_change = Some((1, feather));
                                }
                                if ui
                                    .add(
                                        egui::Slider::new(&mut flow, 0.0..=100.0)
                                            .text("流动")
                                            .fixed_decimals(0),
                                    )
                                    .changed()
                                {
                                    brush_change = Some((2, flow));
                                }
                                if ui
                                    .add(
                                        egui::Slider::new(&mut density, 0.0..=100.0)
                                            .text("密度")
                                            .fixed_decimals(0),
                                    )
                                    .changed()
                                {
                                    brush_change = Some((3, density));
                                }
                            });
                            ui.weak("按住左键在画面上涂抹；[ ] 调整笔刷大小");
                        }
                        Some(MaskKind::LinearGradient { .. }) => {
                            ui.weak("在画面上拖动设置渐变方向；拖动两端圆点可单独调整");
                        }
                        Some(MaskKind::RadialGradient { .. }) => {
                            ui.weak("在画面上拖动设置中心与半径");
                        }
                        Some(_) => {
                            ui.weak("颜色 / 亮度范围在右侧面板调整参数");
                        }
                        None => {
                            ui.weak("选择上方工具新建蒙版");
                        }
                    }
                });
            });

        if let Some(tool) = tool {
            self.apply_mask_tool(tool);
        }
        if let Some(value) = overlay_toggle {
            self.show_overlay = value;
        }
        if let Some(color) = overlay_color {
            self.edit_mask_live("叠加颜色", move |mask| mask.overlay = color);
        }
        if let Some((slot, value)) = brush_change {
            let apply = move |mask: &mut fdxmp_core::params::MaskParams| {
                if let MaskKind::Brush(brush) = &mut mask.kind {
                    match slot {
                        0 => brush.size = value,
                        1 => brush.feather = value,
                        2 => brush.flow = value,
                        _ => brush.density = value,
                    }
                }
            };
            self.edit_mask_live("画笔参数", apply);
        }
        if duplicate {
            if let EditTarget::Mask(index) = self.edit_target {
                self.duplicate_mask(index);
            }
        }
        if delete {
            if let EditTarget::Mask(index) = self.edit_target {
                self.delete_mask(index);
            }
        }
        if finish {
            self.exit_mask_mode();
        }
    }

    /// 蒙版视图画布交互：线性 / 径向渐变拖动（画笔在 canvas 常规逻辑中处理）。
    fn mask_canvas_interaction(
        &mut self,
        _ui: &egui::Ui,
        response: &egui::Response,
        target_rect: egui::Rect,
    ) {
        let EditTarget::Mask(index) = self.edit_target else {
            return;
        };
        let Some(kind) = self
            .engine
            .params()
            .masks
            .get(index)
            .map(|mask| mask.kind.clone())
        else {
            return;
        };

        match kind {
            fdxmp_core::params::MaskKind::LinearGradient { start, end } => {
                if response.drag_started() {
                    if let Some(pointer) = response.interact_pointer_pos() {
                        if let Some(uv) = self.pointer_to_source_uv(pointer, target_rect) {
                            let near = |pin: [f32; 2]| {
                                ((pin[0] - uv[0]).powi(2) + (pin[1] - uv[1]).powi(2)).sqrt() < 0.06
                            };
                            let drag_kind = if near(start) {
                                MaskDragKind::LinearStart
                            } else if near(end) {
                                MaskDragKind::LinearEnd
                            } else {
                                MaskDragKind::LinearNew
                            };
                            if drag_kind == MaskDragKind::LinearNew {
                                let mut params = self.engine.params().clone();
                                if let Some(mask) = params.masks.get_mut(index) {
                                    if let fdxmp_core::params::MaskKind::LinearGradient {
                                        start,
                                        end,
                                    } = &mut mask.kind
                                    {
                                        *start = uv;
                                        *end = uv;
                                    }
                                }
                                self.engine.set_params_live(params);
                                self.params_dirty = true;
                            }
                            self.mask_drag = Some(MaskDrag {
                                kind: drag_kind,
                                start_uv: uv,
                            });
                        }
                    }
                }
                if let Some(drag) = self.mask_drag {
                    if response.dragged() {
                        if let Some(pointer) = response.interact_pointer_pos() {
                            if let Some(uv) = self.pointer_to_source_uv(pointer, target_rect) {
                                let mut params = self.engine.params().clone();
                                if let Some(mask) = params.masks.get_mut(index) {
                                    if let fdxmp_core::params::MaskKind::LinearGradient {
                                        start,
                                        end,
                                    } = &mut mask.kind
                                    {
                                        match drag.kind {
                                            MaskDragKind::LinearStart => *start = uv,
                                            MaskDragKind::LinearEnd => *end = uv,
                                            MaskDragKind::LinearNew => {
                                                *start = drag.start_uv;
                                                *end = uv;
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                self.engine.set_params_live(params);
                                self.params_dirty = true;
                                self.pending_history = Some("线性渐变".to_string());
                            }
                        }
                    }
                    if response.drag_stopped() {
                        self.mask_drag = None;
                    }
                }
            }
            fdxmp_core::params::MaskKind::RadialGradient { center, .. } => {
                if response.drag_started() {
                    if let Some(pointer) = response.interact_pointer_pos() {
                        if let Some(uv) = self.pointer_to_source_uv(pointer, target_rect) {
                            let distance =
                                ((center[0] - uv[0]).powi(2) + (center[1] - uv[1]).powi(2)).sqrt();
                            self.mask_drag = Some(MaskDrag {
                                kind: if distance < 0.06 {
                                    MaskDragKind::RadialCenter
                                } else {
                                    MaskDragKind::RadialRadius
                                },
                                start_uv: uv,
                            });
                        }
                    }
                }
                if let Some(drag) = self.mask_drag {
                    if response.dragged() {
                        if let Some(pointer) = response.interact_pointer_pos() {
                            if let Some(uv) = self.pointer_to_source_uv(pointer, target_rect) {
                                let mut params = self.engine.params().clone();
                                if let Some(mask) = params.masks.get_mut(index) {
                                    if let fdxmp_core::params::MaskKind::RadialGradient {
                                        center,
                                        radii,
                                        ..
                                    } = &mut mask.kind
                                    {
                                        if drag.kind == MaskDragKind::RadialCenter {
                                            *center = uv;
                                        } else {
                                            radii[0] = (uv[0] - center[0]).abs().max(0.02);
                                            radii[1] = (uv[1] - center[1]).abs().max(0.02);
                                        }
                                    }
                                }
                                self.engine.set_params_live(params);
                                self.params_dirty = true;
                                self.pending_history = Some("径向渐变".to_string());
                            }
                        }
                    }
                    if response.drag_stopped() {
                        self.mask_drag = None;
                    }
                }
            }
            fdxmp_core::params::MaskKind::AiObject { .. } => {
                // AI 物体：在画面上点按选择目标，重新推理取该点所在连通域。
                if !response.clicked() {
                    return;
                }
                let Some(pointer) = response.interact_pointer_pos() else {
                    return;
                };
                let Some(uv) = self.pointer_to_source_uv(pointer, target_rect) else {
                    return;
                };
                let mut params = self.engine.params().clone();
                if let Some(mask) = params.masks.get_mut(index) {
                    if let fdxmp_core::params::MaskKind::AiObject { point } = &mut mask.kind {
                        *point = uv;
                    }
                }
                self.engine.set_params_live(params);
                self.params_dirty = true;
                self.pending_history = Some("AI 物体".to_string());
                self.request_ai_mask();
            }
            _ => {}
        }
    }

    /// 画笔光标：在画面上显示笔刷大小圆圈。
    fn paint_brush_cursor(&self, painter: &egui::Painter, ui: &egui::Ui, target_rect: egui::Rect) {
        if !self.brush_painting {
            return;
        }
        let EditTarget::Mask(index) = self.edit_target else {
            return;
        };
        let Some(mask) = self.engine.params().masks.get(index) else {
            return;
        };
        let fdxmp_core::params::MaskKind::Brush(brush) = &mask.kind else {
            return;
        };
        let Some(pointer) = ui.input(|input| input.pointer.hover_pos()) else {
            return;
        };
        if !target_rect.contains(pointer) {
            return;
        }
        let long_edge = target_rect.width().max(target_rect.height());
        let radius = (brush.size / 100.0 * long_edge).clamp(4.0, 420.0);
        painter.circle_stroke(
            pointer,
            radius + 1.0,
            egui::Stroke::new(2.5, egui::Color32::from_black_alpha(140)),
        );
        painter.circle_stroke(
            pointer,
            radius,
            egui::Stroke::new(1.5, egui::Color32::WHITE),
        );
    }

    /// 蒙版面板：创建 / 列表 / 选中蒙版的参数。
    fn mask_panel(&mut self, ui: &mut egui::Ui) {
        use fdxmp_core::params::{BrushSettings, MaskKind};

        ui.horizontal_wrapped(|ui| {
            if ui.button("画笔").clicked() {
                self.create_mask(
                    MaskKind::Brush(BrushSettings {
                        size: 12.0,
                        feather: 50.0,
                        flow: 100.0,
                        density: 100.0,
                        auto_mask: false,
                        strokes: Vec::new(),
                    }),
                    "画笔",
                );
            }
            if ui.button("线性").clicked() {
                self.create_mask(
                    MaskKind::LinearGradient {
                        start: [0.2, 0.5],
                        end: [0.8, 0.5],
                    },
                    "线性渐变",
                );
            }
            if ui.button("径向").clicked() {
                self.create_mask(
                    MaskKind::RadialGradient {
                        center: [0.5, 0.5],
                        radii: [0.3, 0.3],
                        rotation: 0.0,
                    },
                    "径向渐变",
                );
            }
            if ui.button("颜色范围").clicked() {
                self.create_mask(
                    MaskKind::ColorRange {
                        color: [0.3, 0.5, 0.8],
                        range: 20.0,
                        smoothness: 30.0,
                    },
                    "颜色范围",
                );
            }
            if ui.button("亮度范围").clicked() {
                self.create_mask(
                    MaskKind::LuminanceRange {
                        min: 0.3,
                        max: 0.7,
                        smoothness: 30.0,
                    },
                    "亮度范围",
                );
            }
            if self.mask_clipboard.is_some() && ui.button("粘贴蒙版").clicked() {
                let mut params = self.engine.params().clone();
                if let Some(mut mask) = self.mask_clipboard.clone() {
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|elapsed| elapsed.as_nanos())
                        .unwrap_or(0);
                    mask.id = format!("mask-{stamp:x}");
                    mask.name = format!("{} 粘贴", mask.name);
                    params.masks.push(mask);
                    let index = params.masks.len() - 1;
                    self.engine.set_params_live(params);
                    self.engine.commit_history("粘贴蒙版");
                    self.edit_target = EditTarget::Mask(index);
                    self.params_dirty = true;
                    self.status = "已粘贴蒙版（跨图片）".to_string();
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.weak("AI 蒙版：");
            if ui.button("主体").clicked() {
                self.create_ai_mask(MaskKind::AiSubject, "AI 主体");
            }
            if ui.button("天空").clicked() {
                self.create_ai_mask(MaskKind::AiSky, "AI 天空");
            }
            if ui.button("背景").clicked() {
                self.create_ai_mask(MaskKind::AiBackground, "AI 背景");
            }
            ui.menu_button("人物 ▾", |ui| {
                for (label, part) in [
                    ("面部", fdxmp_core::params::PeoplePart::Face),
                    ("身体", fdxmp_core::params::PeoplePart::Body),
                    ("皮肤", fdxmp_core::params::PeoplePart::Skin),
                    ("头发", fdxmp_core::params::PeoplePart::Hair),
                    ("衣服", fdxmp_core::params::PeoplePart::Clothes),
                ] {
                    if ui.button(label).clicked() {
                        self.create_ai_mask(
                            MaskKind::AiPeople { part },
                            &format!("AI 人物-{label}"),
                        );
                        ui.close();
                    }
                }
            });
            if ui
                .button("物体（点按）")
                .on_hover_text("生成后在画面上点击目标物体")
                .clicked()
            {
                self.create_ai_mask(MaskKind::AiObject { point: [0.5, 0.5] }, "AI 物体");
            }
        });
        // AI 模型状态与下载已移到左栏「AI」面板
        #[cfg(feature = "ai")]
        {
            let status = fdxmp_core::ai::status();
            ui.horizontal_wrapped(|ui| {
                ui.weak(if status.models_ready() {
                    "AI 模型已就绪"
                } else {
                    "AI 模型缺失"
                });
                if ui.small_button("AI 模型 / 下载 →").clicked() {
                    self.show_left = true;
                    self.left_tab = LeftTab::Ai;
                }
            });
        }
        if let Some(label) = &self.ai_running {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(format!("正在生成 {label}…"));
            });
        }

        ui.separator();
        let masks: Vec<(usize, String)> = self
            .engine
            .params()
            .masks
            .iter()
            .enumerate()
            .map(|(index, mask)| (index, mask.name.clone()))
            .collect();
        if masks.is_empty() {
            ui.weak("尚未创建蒙版：上方按钮新建，选中后在画布上调整。");
            return;
        }

        let mut pending_select = None;
        let mut pending_delete = None;
        let mut pending_duplicate = None;
        let mut pending_copy = None;
        for (index, name) in masks {
            let selected =
                matches!(self.edit_target, EditTarget::Mask(current) if current == index);
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(selected, format!("{}. {name}", index + 1))
                    .clicked()
                {
                    pending_select = Some(index);
                }
                if ui.small_button("复制").clicked() {
                    pending_duplicate = Some(index);
                }
                if ui.small_button("拷贝").clicked() {
                    pending_copy = Some(index);
                }
                if ui.small_button("删除").clicked() {
                    pending_delete = Some(index);
                }
            });
        }

        if let Some(index) = pending_copy {
            if let Some(mask) = self.engine.params().masks.get(index).cloned() {
                self.status = format!("已拷贝蒙版「{}」，可在其他图片粘贴", mask.name);
                self.mask_clipboard = Some(mask);
            }
        }
        if let Some(index) = pending_select {
            // 点击蒙版行进入独立蒙版编辑视图（对标 Lightroom）。
            self.enter_mask_mode(Some(index));
        }
        if let Some(index) = pending_duplicate {
            self.duplicate_mask(index);
        }
        if let Some(index) = pending_delete {
            self.delete_mask(index);
        }

        let Some(mask) = self.selected_mask().cloned() else {
            ui.weak("选择上面列表中的蒙版来调整参数。");
            return;
        };

        ui.separator();
        ui.horizontal(|ui| {
            let mut inverted = mask.inverted;
            if ui.checkbox(&mut inverted, "反向").changed() {
                self.edit_mask_live("蒙版反向", move |target| target.inverted = inverted);
            }
            let mut overlay = self.show_overlay;
            if ui.checkbox(&mut overlay, "叠加显示").changed() {
                self.show_overlay = overlay;
            }
        });
        if let Some(value) = value_slider(ui, "不透明度", mask.opacity, 0.0..=100.0) {
            let apply = move |target: &mut fdxmp_core::params::MaskParams| target.opacity = value;
            self.edit_mask_live("蒙版不透明度", apply);
        }
        if let Some(value) = value_slider(ui, "羽化", mask.feather, 0.0..=100.0) {
            let apply = move |target: &mut fdxmp_core::params::MaskParams| target.feather = value;
            self.edit_mask_live("蒙版羽化", apply);
        }

        match mask.kind.clone() {
            MaskKind::Brush(settings) => {
                for (label, value, range, setter) in [
                    ("画笔大小", settings.size, 2.0..=50.0, 0usize),
                    ("画笔羽化", settings.feather, 0.0..=100.0, 1),
                    ("流动", settings.flow, 0.0..=100.0, 2),
                    ("密度", settings.density, 0.0..=100.0, 3),
                ] {
                    if let Some(new_value) = value_slider(ui, label, value, range) {
                        let apply = move |target: &mut fdxmp_core::params::MaskParams| {
                            if let MaskKind::Brush(brush) = &mut target.kind {
                                match setter {
                                    0 => brush.size = new_value,
                                    1 => brush.feather = new_value,
                                    2 => brush.flow = new_value,
                                    _ => brush.density = new_value,
                                }
                            }
                        };
                        self.edit_mask_live(label, apply);
                    }
                }
                ui.horizontal(|ui| {
                    let label = if self.brush_painting {
                        "停止绘制"
                    } else {
                        "在画布上绘制"
                    };
                    if ui.button(label).clicked() {
                        self.brush_painting = !self.brush_painting;
                    }
                    if ui.small_button("清除笔画").clicked() {
                        let apply = |target: &mut fdxmp_core::params::MaskParams| {
                            if let MaskKind::Brush(brush) = &mut target.kind {
                                brush.strokes.clear();
                            }
                        };
                        self.edit_mask_live("清除笔画", apply);
                    }
                    ui.weak("按住左键拖动画出蒙版");
                });
            }
            MaskKind::LinearGradient { start, end } => {
                for (label, value, range, slot) in [
                    ("起点 X", start[0], 0.0..=1.0, 0usize),
                    ("起点 Y", start[1], 0.0..=1.0, 1),
                    ("终点 X", end[0], 0.0..=1.0, 2),
                    ("终点 Y", end[1], 0.0..=1.0, 3),
                ] {
                    if let Some(new_value) = value_slider(ui, label, value, range) {
                        let apply = move |target: &mut fdxmp_core::params::MaskParams| {
                            if let MaskKind::LinearGradient { start, end } = &mut target.kind {
                                match slot {
                                    0 => start[0] = new_value,
                                    1 => start[1] = new_value,
                                    2 => end[0] = new_value,
                                    _ => end[1] = new_value,
                                }
                            }
                        };
                        self.edit_mask_live(label, apply);
                    }
                }
            }
            MaskKind::RadialGradient {
                center,
                radii,
                rotation,
            } => {
                for (label, value, range, slot) in [
                    ("中心 X", center[0], 0.0..=1.0, 0usize),
                    ("中心 Y", center[1], 0.0..=1.0, 1),
                    ("半径 X", radii[0], 0.02..=1.0, 2),
                    ("半径 Y", radii[1], 0.02..=1.0, 3),
                    ("旋转", rotation, -180.0..=180.0, 4),
                ] {
                    if let Some(new_value) = value_slider(ui, label, value, range) {
                        let apply = move |target: &mut fdxmp_core::params::MaskParams| {
                            if let MaskKind::RadialGradient {
                                center,
                                radii,
                                rotation,
                            } = &mut target.kind
                            {
                                match slot {
                                    0 => center[0] = new_value,
                                    1 => center[1] = new_value,
                                    2 => radii[0] = new_value,
                                    3 => radii[1] = new_value,
                                    _ => *rotation = new_value,
                                }
                            }
                        };
                        self.edit_mask_live(label, apply);
                    }
                }
            }
            MaskKind::ColorRange {
                color,
                range,
                smoothness,
            } => {
                for (label, value, slot) in [
                    ("目标红", color[0], 0usize),
                    ("目标绿", color[1], 1),
                    ("目标蓝", color[2], 2),
                ] {
                    if let Some(new_value) = value_slider(ui, label, value, 0.0..=1.0) {
                        let apply = move |target: &mut fdxmp_core::params::MaskParams| {
                            if let MaskKind::ColorRange { color, .. } = &mut target.kind {
                                color[slot] = new_value;
                            }
                        };
                        self.edit_mask_live(label, apply);
                    }
                }
                if let Some(new_value) = value_slider(ui, "范围", range, 0.0..=100.0) {
                    let apply = move |target: &mut fdxmp_core::params::MaskParams| {
                        if let MaskKind::ColorRange { range, .. } = &mut target.kind {
                            *range = new_value;
                        }
                    };
                    self.edit_mask_live("颜色范围", apply);
                }
                if let Some(new_value) = value_slider(ui, "平滑度", smoothness, 0.0..=100.0) {
                    let apply = move |target: &mut fdxmp_core::params::MaskParams| {
                        if let MaskKind::ColorRange { smoothness, .. } = &mut target.kind {
                            *smoothness = new_value;
                        }
                    };
                    self.edit_mask_live("颜色平滑度", apply);
                }
            }
            MaskKind::LuminanceRange {
                min,
                max,
                smoothness,
            } => {
                if let Some(new_value) = value_slider(ui, "亮度下限", min, 0.0..=1.0) {
                    let apply = move |target: &mut fdxmp_core::params::MaskParams| {
                        if let MaskKind::LuminanceRange { min, .. } = &mut target.kind {
                            *min = new_value;
                        }
                    };
                    self.edit_mask_live("亮度下限", apply);
                }
                if let Some(new_value) = value_slider(ui, "亮度上限", max, 0.0..=1.0) {
                    let apply = move |target: &mut fdxmp_core::params::MaskParams| {
                        if let MaskKind::LuminanceRange { max, .. } = &mut target.kind {
                            *max = new_value;
                        }
                    };
                    self.edit_mask_live("亮度上限", apply);
                }
                if let Some(new_value) = value_slider(ui, "平滑度", smoothness, 0.0..=100.0) {
                    let apply = move |target: &mut fdxmp_core::params::MaskParams| {
                        if let MaskKind::LuminanceRange { smoothness, .. } = &mut target.kind {
                            *smoothness = new_value;
                        }
                    };
                    self.edit_mask_live("亮度平滑度", apply);
                }
            }
            other => {
                ui.weak(format!("{other:?}（阶段 7 支持）"));
            }
        }

        ui.separator();
        ui.weak("下面的调色 / 曲线 / HSL / 分级 / 细节面板此时编辑的是该蒙版。");
    }

    /// 曲线面板：RGB / 红 / 绿 / 蓝 四条曲线 + 参数曲线。
    fn curve_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (index, label) in ["RGB", "红", "绿", "蓝"].iter().enumerate() {
                if ui
                    .selectable_label(self.curve_channel == index, *label)
                    .clicked()
                {
                    self.curve_channel = index;
                }
            }
        });

        let channel = self.curve_channel;
        let mut points = match channel {
            1 => self.read_params().curves.red.points.clone(),
            2 => self.read_params().curves.green.points.clone(),
            3 => self.read_params().curves.blue.points.clone(),
            _ => self.read_params().curves.rgb.points.clone(),
        };
        if curve_editor(ui, &mut points) {
            let updated = points.clone();
            self.edit_params_live("曲线", move |params| match channel {
                1 => params.curves.red.points = updated.clone(),
                2 => params.curves.green.points = updated.clone(),
                3 => params.curves.blue.points = updated.clone(),
                _ => params.curves.rgb.points = updated.clone(),
            });
        }
        if ui.small_button("重置当前曲线").clicked() {
            self.edit_params_live("重置曲线", move |params| {
                let default = fdxmp_core::params::ToneCurve::default();
                match channel {
                    1 => params.curves.red = default,
                    2 => params.curves.green = default,
                    3 => params.curves.blue = default,
                    _ => params.curves.rgb = default,
                }
            });
        }

        ui.add_space(6.0);
        ui.label(egui::RichText::new("参数曲线").strong());
        let parametric = self.read_params().curves.parametric.clone();
        self.param_slider(
            ui,
            "  高光",
            parametric.highlights,
            -100.0..=100.0,
            "参数曲线",
            |params, value| params.curves.parametric.highlights = value,
        );
        self.param_slider(
            ui,
            "  中间调",
            parametric.midtones,
            -100.0..=100.0,
            "参数曲线",
            |params, value| params.curves.parametric.midtones = value,
        );
        self.param_slider(
            ui,
            "  阴影",
            parametric.shadows,
            -100.0..=100.0,
            "参数曲线",
            |params, value| params.curves.parametric.shadows = value,
        );
        self.param_slider(
            ui,
            "  高光分界",
            parametric.highlights_split,
            0.0..=1.0,
            "参数曲线",
            |params, value| params.curves.parametric.highlights_split = value,
        );
        self.param_slider(
            ui,
            "  中间调分界",
            parametric.midtones_split,
            0.0..=1.0,
            "参数曲线",
            |params, value| params.curves.parametric.midtones_split = value,
        );
    }

    /// HSL 面板：8 个色带 × 色相 / 饱和度 / 明亮度。
    fn hsl_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (index, label) in ["色相", "饱和度", "明亮度"].iter().enumerate() {
                if ui
                    .selectable_label(self.hsl_mode == index, *label)
                    .clicked()
                {
                    self.hsl_mode = index;
                }
            }
        });
        let mode = self.hsl_mode;
        for band in ColorBand::ALL {
            let hsl = self.read_params().hsl.band(band);
            let value = match mode {
                0 => hsl.hue,
                1 => hsl.saturation,
                _ => hsl.luminance,
            };
            let label = format!("  {}", band.label_zh());
            self.param_slider(
                ui,
                &label,
                value,
                -100.0..=100.0,
                "HSL",
                move |params, v| {
                    let hsl = params.hsl.band_mut(band);
                    match mode {
                        0 => hsl.hue = v,
                        1 => hsl.saturation = v,
                        _ => hsl.luminance = v,
                    }
                },
            );
        }
        if ui.small_button("重置 HSL").clicked() {
            self.edit_params_live("重置 HSL", |params| {
                params.hsl = fdxmp_core::params::HslParams::default();
            });
        }
    }

    /// 颜色分级面板：阴影 / 中间调 / 高光 + 混合 / 平衡。
    fn grading_panel(&mut self, ui: &mut egui::Ui) {
        for (label, index) in [("阴影", 0usize), ("中间调", 1), ("高光", 2)] {
            let zone = match index {
                0 => self.read_params().grading.shadows.clone(),
                1 => self.read_params().grading.midtones.clone(),
                _ => self.read_params().grading.highlights.clone(),
            };
            ui.label(egui::RichText::new(label).strong());
            let history = format!("分级{label}");
            self.param_slider(
                ui,
                "  色相",
                zone.hue,
                0.0..=360.0,
                &history,
                move |params, value| grading_zone_mut(params, index).hue = value,
            );
            self.param_slider(
                ui,
                "  饱和度",
                zone.saturation,
                0.0..=100.0,
                &history,
                move |params, value| grading_zone_mut(params, index).saturation = value,
            );
            self.param_slider(
                ui,
                "  明亮度",
                zone.luminance,
                -100.0..=100.0,
                &history,
                move |params, value| grading_zone_mut(params, index).luminance = value,
            );
        }
        let grading = self.read_params().grading.clone();
        self.param_slider(
            ui,
            "混合",
            grading.blending,
            0.0..=100.0,
            "分级混合",
            |params, value| params.grading.blending = value,
        );
        self.param_slider(
            ui,
            "平衡",
            grading.balance,
            -100.0..=100.0,
            "分级平衡",
            |params, value| params.grading.balance = value,
        );
    }

    /// 细节面板：锐化 + 降噪。
    fn detail_panel(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("锐化").strong());
        let sharpen = self.read_params().detail.sharpen.clone();
        self.param_slider(
            ui,
            "  数量",
            sharpen.amount,
            0.0..=150.0,
            "锐化数量",
            |params, value| params.detail.sharpen.amount = value,
        );
        self.param_slider(
            ui,
            "  半径",
            sharpen.radius,
            0.5..=3.0,
            "锐化半径",
            |params, value| params.detail.sharpen.radius = value,
        );
        self.param_slider(
            ui,
            "  细节",
            sharpen.detail,
            0.0..=100.0,
            "锐化细节",
            |params, value| params.detail.sharpen.detail = value,
        );
        self.param_slider(
            ui,
            "  蒙版",
            sharpen.masking,
            0.0..=100.0,
            "锐化蒙版",
            |params, value| params.detail.sharpen.masking = value,
        );
        ui.add_space(6.0);
        ui.label(egui::RichText::new("降噪").strong());
        let noise = self.read_params().detail.noise.clone();
        self.param_slider(
            ui,
            "  亮度",
            noise.luminance,
            0.0..=100.0,
            "降噪亮度",
            |params, value| params.detail.noise.luminance = value,
        );
        self.param_slider(
            ui,
            "  亮度细节",
            noise.luminance_detail,
            0.0..=100.0,
            "降噪亮度细节",
            |params, value| params.detail.noise.luminance_detail = value,
        );
        self.param_slider(
            ui,
            "  亮度对比度",
            noise.luminance_contrast,
            0.0..=100.0,
            "降噪亮度对比度",
            |params, value| params.detail.noise.luminance_contrast = value,
        );
        self.param_slider(
            ui,
            "  颜色",
            noise.color,
            0.0..=100.0,
            "降噪颜色",
            |params, value| params.detail.noise.color = value,
        );
        self.param_slider(
            ui,
            "  颜色细节",
            noise.color_detail,
            0.0..=100.0,
            "降噪颜色细节",
            |params, value| params.detail.noise.color_detail = value,
        );
        self.param_slider(
            ui,
            "  颜色平滑度",
            noise.color_smoothness,
            0.0..=100.0,
            "降噪颜色平滑度",
            |params, value| params.detail.noise.color_smoothness = value,
        );
    }

    /// 裁剪面板：进入独立裁剪模式 + 摆正 / 翻转 / 网格 / 自动构图。
    fn crop_panel(&mut self, ui: &mut egui::Ui) {
        use fdxmp_core::params::{AspectRatio, CropGrid};
        let crop = self.engine.params().crop.clone();
        ui.horizontal(|ui| {
            let label = if self.crop_mode {
                "退出裁剪 (R)"
            } else {
                "进入裁剪模式 (R)"
            };
            if ui.selectable_label(self.crop_mode, label).clicked() {
                self.toggle_crop_mode();
            }
            if ui.small_button("重置").clicked() {
                self.edit_geometry_live("裁剪重置", |params| {
                    params.crop.rect = [0.0, 0.0, 1.0, 1.0];
                    params.crop.rotation_deg = 0.0;
                    params.crop.flip_horizontal = false;
                    params.crop.flip_vertical = false;
                    params.crop.aspect = AspectRatio::Free;
                });
                self.pending_history = Some("裁剪重置".to_string());
            }
        });

        let mut angle = crop.rotation_deg;
        let response = ui.add(
            egui::Slider::new(&mut angle, -45.0..=45.0)
                .text("摆正")
                .suffix("°"),
        );
        if response.changed() {
            let mut params = self.engine.params().clone();
            params.crop.rotation_deg = angle;
            if params.crop.auto_compose {
                if let Some(loaded) = self.loaded.as_ref() {
                    let geometry = fdxmp_core::geometry::Geometry::from_params(&params);
                    params.crop.rect = fdxmp_core::geometry::constrain_rect(
                        &geometry,
                        params.crop.rect,
                        loaded.proxy[0],
                        loaded.proxy[1],
                    );
                }
            }
            self.engine.set_params_live(params);
            self.params_dirty = true;
            self.pending_history = Some("摆正".to_string());
        }

        ui.horizontal(|ui| {
            if ui
                .selectable_label(crop.flip_horizontal, "水平翻转")
                .clicked()
            {
                self.edit_geometry_live("裁剪翻转", |params| {
                    params.crop.flip_horizontal = !params.crop.flip_horizontal;
                });
                self.pending_history = Some("裁剪翻转".to_string());
            }
            if ui
                .selectable_label(crop.flip_vertical, "垂直翻转")
                .clicked()
            {
                self.edit_geometry_live("裁剪翻转", |params| {
                    params.crop.flip_vertical = !params.crop.flip_vertical;
                });
                self.pending_history = Some("裁剪翻转".to_string());
            }
        });

        ui.horizontal_wrapped(|ui| {
            ui.label("辅助线");
            for (label, grid) in [
                ("三分", CropGrid::Thirds),
                ("网格", CropGrid::Grid),
                ("对角", CropGrid::Diagonal),
                ("三角", CropGrid::Triangle),
                ("黄金", CropGrid::GoldenRatio),
                ("无", CropGrid::None),
            ] {
                if ui.selectable_label(crop.grid == grid, label).clicked() {
                    self.edit_geometry_live("裁剪网格", |params| params.crop.grid = grid);
                    self.pending_history = Some("裁剪网格".to_string());
                }
            }
        });

        let mut auto = crop.auto_compose;
        if ui
            .checkbox(&mut auto, "自动构图（约束裁剪框在旋转后的画面内）")
            .changed()
        {
            self.edit_geometry_live("自动构图", |params| params.crop.auto_compose = auto);
            if auto {
                self.apply_aspect_choice();
            }
            self.pending_history = Some("自动构图".to_string());
        }

        ui.weak(format!(
            "裁剪框 {:.0}% × {:.0}% · 摆正 {:.1}° · 比例 {:?}",
            crop.rect[2] * 100.0,
            crop.rect[3] * 100.0,
            crop.rotation_deg,
            crop.aspect
        ));
    }

    /// 变换面板：透视 / 旋转 / 翻转（GPU 几何趟实时预览）。
    fn transform_panel(&mut self, ui: &mut egui::Ui) {
        let params = self.engine.params().clone();
        let mut perspective_v = params.transform.perspective_vertical;
        let mut perspective_h = params.transform.perspective_horizontal;
        let mut rotate = params.transform.rotate;
        let mut changed = false;
        changed |= ui
            .add(egui::Slider::new(&mut perspective_v, -100.0..=100.0).text("垂直透视"))
            .changed();
        changed |= ui
            .add(egui::Slider::new(&mut perspective_h, -100.0..=100.0).text("水平透视"))
            .changed();
        changed |= ui
            .add(
                egui::Slider::new(&mut rotate, -45.0..=45.0)
                    .text("旋转")
                    .suffix("°"),
            )
            .changed();
        if changed {
            self.edit_geometry_live("变换", |params| {
                params.transform.perspective_vertical = perspective_v;
                params.transform.perspective_horizontal = perspective_h;
                params.transform.rotate = rotate;
            });
            self.pending_history = Some("变换".to_string());
        }
        ui.horizontal(|ui| {
            if ui
                .selectable_label(params.transform.flip_horizontal, "水平翻转")
                .clicked()
            {
                self.edit_geometry_live("变换", |params| {
                    params.transform.flip_horizontal = !params.transform.flip_horizontal;
                });
                self.pending_history = Some("变换".to_string());
            }
            if ui
                .selectable_label(params.transform.flip_vertical, "垂直翻转")
                .clicked()
            {
                self.edit_geometry_live("变换", |params| {
                    params.transform.flip_vertical = !params.transform.flip_vertical;
                });
                self.pending_history = Some("变换".to_string());
            }
            if ui.small_button("重置变换").clicked() {
                self.edit_geometry_live("变换重置", |params| {
                    params.transform = fdxmp_core::params::TransformParams::default();
                });
                self.pending_history = Some("变换重置".to_string());
            }
        });
        ui.weak("透视 / 旋转 / 翻转在 GPU 几何趟实时生效（含裁剪后的输出）");
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(
            rect,
            egui::CornerRadius::ZERO,
            egui::Color32::from_rgb(0x1b, 0x1b, 0x1b),
        );

        // 右键菜单：视图 / 裁剪 / 蒙版 / 重置几何。
        let mut canvas_action: Option<CanvasAction> = None;
        response.context_menu(|ui| {
            if ui.button("适应窗口").clicked() {
                canvas_action = Some(CanvasAction::Fit);
                ui.close();
            }
            if ui
                .selectable_label(self.detail_mode, "1:1 细节视图")
                .clicked()
            {
                canvas_action = Some(CanvasAction::Detail);
                ui.close();
            }
            ui.separator();
            if ui
                .selectable_label(self.crop_mode, "裁剪模式 (R)")
                .clicked()
            {
                canvas_action = Some(CanvasAction::Crop);
                ui.close();
            }
            let has_mask = !self.engine.params().masks.is_empty();
            if ui
                .add_enabled(has_mask, egui::Button::new("编辑蒙版 (M)"))
                .clicked()
            {
                canvas_action = Some(CanvasAction::Mask);
                ui.close();
            }
            if has_mask && !self.crop_mode {
                let mut overlay = self.show_overlay;
                if ui.checkbox(&mut overlay, "显示蒙版叠加").changed() {
                    canvas_action = Some(CanvasAction::Overlay(overlay));
                }
            }
            ui.separator();
            if ui.button("重置裁剪与变换").clicked() {
                canvas_action = Some(CanvasAction::ResetGeometry);
                ui.close();
            }
        });
        let ctx = ui.ctx().clone();
        self.apply_canvas_action(canvas_action, &ctx);

        // 1:1 细节视图：只渲染可见分块（全分辨率）。
        if self.detail_mode {
            #[cfg(feature = "gpu")]
            self.detail_canvas(ui, rect);
            #[cfg(not(feature = "gpu"))]
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "1:1 视图需要启用 GPU（gpu feature）",
                egui::FontId::proportional(14.0),
                egui::Color32::from_gray(0xb0),
            );
            if response.dragged() {
                self.pan += response.drag_delta();
            }
            self.zoom_toolbar(ui, rect);
            return;
        }

        let mut image_rect: Option<egui::Rect> = None;
        // 裁剪模式显示全幅（未裁剪）视图；正常模式显示裁剪后的输出尺寸。
        let display_dims: Option<(u32, u32)> = if self.crop_mode {
            self.loaded
                .as_ref()
                .map(|loaded| (loaded.proxy[0], loaded.proxy[1]))
        } else {
            #[cfg(feature = "gpu")]
            {
                self.gpu
                    .as_ref()
                    .and_then(|gpu| gpu.display_size())
                    .or_else(|| {
                        self.loaded
                            .as_ref()
                            .map(|loaded| (loaded.proxy[0], loaded.proxy[1]))
                    })
            }
            #[cfg(not(feature = "gpu"))]
            {
                self.loaded
                    .as_ref()
                    .map(|loaded| (loaded.proxy[0], loaded.proxy[1]))
            }
        };

        if let (Some(loaded), Some((display_w, display_h))) = (self.loaded.as_ref(), display_dims) {
            let fit =
                (rect.width() / display_w as f32).min(rect.height() / display_h as f32) * 0.98;
            let scale = if self.fit_to_window || self.crop_mode {
                fit
            } else {
                fit * self.zoom
            };
            let size = egui::vec2(display_w as f32 * scale, display_h as f32 * scale);
            let center = if self.crop_mode {
                rect.center()
            } else {
                rect.center() + self.pan
            };
            let drawn_rect = egui::Rect::from_center_size(center, size);
            #[cfg(feature = "gpu")]
            let texture_id = self
                .gpu
                .as_ref()
                .and_then(|gpu| gpu.texture_id)
                .unwrap_or_else(|| loaded.texture.id());
            #[cfg(not(feature = "gpu"))]
            let texture_id = loaded.texture.id();
            painter.image(
                texture_id,
                drawn_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            image_rect = Some(drawn_rect);
            // GPU 可用时叠加层已由 GPU 合成（与几何一致），否则用 CPU 生成的叠加纹理。
            #[cfg(feature = "gpu")]
            let gpu_overlay = self.gpu.is_some();
            #[cfg(not(feature = "gpu"))]
            let gpu_overlay = false;
            if self.show_overlay && !self.crop_mode && !gpu_overlay {
                if let Some((_, overlay)) = &self.overlay_texture {
                    painter.image(
                        overlay.id(),
                        drawn_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
            }
        } else {
            let hint = if self.decoding.is_some() {
                "正在解码…".to_string()
            } else {
                match self.selected.and_then(|index| self.photos.get(index)) {
                    Some(photo) => format!("{}（等待解码）", photo.name),
                    None => "拖入照片，或「文件 → 导入图片…」".to_string(),
                }
            };
            painter.text(
                rect.center() - egui::vec2(0.0, 12.0),
                egui::Align2::CENTER_CENTER,
                hint,
                egui::FontId::proportional(16.0),
                egui::Color32::from_gray(0xb0),
            );
            painter.text(
                rect.center() + egui::vec2(0.0, 16.0),
                egui::Align2::CENTER_CENTER,
                "支持 JPEG / PNG / TIFF / WebP / HEIC / 相机 RAW".to_string(),
                egui::FontId::proportional(12.0),
                egui::Color32::from_gray(0x78),
            );
        }

        if self.decoding.is_some() {
            let spinner_rect = egui::Rect::from_center_size(
                rect.center() + egui::vec2(0.0, 46.0),
                egui::vec2(18.0, 18.0),
            );
            ui.put(spinner_rect, egui::Spinner::new());
        }

        if self.crop_mode {
            if let Some(target_rect) = image_rect {
                self.crop_toolbar(ui, rect);
                self.crop_interaction(ui, &response, target_rect);
                self.paint_crop_overlay(&painter, target_rect);
            }
            return;
        }

        if response.dragged() && !self.mask_mode {
            self.pan += response.drag_delta();
        }

        // 画笔蒙版绘制：画布坐标 → 裁剪后坐标 → 源图归一化坐标
        if self.brush_painting {
            if let (EditTarget::Mask(mask_index), Some(target_rect)) =
                (self.edit_target, image_rect)
            {
                if response.drag_started() {
                    let mut params = self.engine.params().clone();
                    if let Some(mask) = params.masks.get_mut(mask_index) {
                        if let fdxmp_core::params::MaskKind::Brush(brush) = &mut mask.kind {
                            brush.strokes.push(Vec::new());
                        }
                    }
                    self.engine.set_params_live(params);
                    self.params_dirty = true;
                }
                if response.dragged() {
                    if let Some(pointer) = response.interact_pointer_pos() {
                        if let Some(uv) = self.pointer_to_source_uv(pointer, target_rect) {
                            let mut params = self.engine.params().clone();
                            if let Some(mask) = params.masks.get_mut(mask_index) {
                                if let fdxmp_core::params::MaskKind::Brush(brush) = &mut mask.kind {
                                    match brush.strokes.last_mut() {
                                        Some(points) => points.push(uv),
                                        None => brush.strokes.push(vec![uv]),
                                    }
                                }
                            }
                            self.engine.set_params_live(params);
                            self.params_dirty = true;
                            self.pending_history = Some("画笔".to_string());
                        }
                    }
                }
            }
        }

        if response.hovered() {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll.abs() > 0.0 {
                self.fit_to_window = false;
                self.zoom = (self.zoom * (1.0 + scroll * 0.002)).clamp(0.05, 8.0);
            }
        }

        // 蒙版编辑视图：画布拖拽编辑蒙版 + 底部工具条 + 画笔光标。
        if self.mask_mode {
            if let Some(target_rect) = image_rect {
                self.mask_canvas_interaction(ui, &response, target_rect);
                self.paint_brush_cursor(&painter, ui, target_rect);
                self.mask_toolbar(ui, rect);
            }
        } else {
            self.zoom_toolbar(ui, rect);
        }
    }

    /// 画布坐标（裁剪后视图）→ 源图归一化坐标（蒙版坐标空间）。
    fn pointer_to_source_uv(
        &self,
        pointer: egui::Pos2,
        target_rect: egui::Rect,
    ) -> Option<[f32; 2]> {
        let loaded = self.loaded.as_ref()?;
        let (source_w, source_h) = (loaded.proxy[0], loaded.proxy[1]);
        #[cfg(feature = "gpu")]
        let display_dims = if self.crop_mode {
            (source_w, source_h)
        } else {
            self.gpu
                .as_ref()
                .and_then(|gpu| gpu.display_size())
                .unwrap_or((source_w, source_h))
        };
        #[cfg(not(feature = "gpu"))]
        let display_dims = (source_w, source_h);
        let (display_w, display_h) = display_dims;
        let params = self.engine.params();
        let geometry = fdxmp_core::geometry::Geometry::from_params(params);
        let matrix = fdxmp_core::geometry::output_to_source_matrix(
            &geometry,
            params.crop.rect,
            source_w,
            source_h,
            display_w,
            display_h,
        );
        let ox = ((pointer.x - target_rect.left()) / target_rect.width().max(1.0)).clamp(0.0, 1.0)
            * display_w as f32;
        let oy = ((pointer.y - target_rect.top()) / target_rect.height().max(1.0)).clamp(0.0, 1.0)
            * display_h as f32;
        let mapped = fdxmp_core::geometry::apply(&matrix, [ox, oy]);
        Some([
            (mapped[0] / source_w as f32).clamp(0.0, 1.0),
            (mapped[1] / source_h as f32).clamp(0.0, 1.0),
        ])
    }

    /// 进入 / 退出裁剪模式（对标 Lightroom 的独立裁剪视图）。
    fn toggle_crop_mode(&mut self) {
        self.crop_mode = !self.crop_mode;
        self.crop_drag = None;
        if self.crop_mode {
            // 裁剪与蒙版各自是独立视图，进入裁剪时退出蒙版编辑。
            self.mask_mode = false;
            self.mask_drag = None;
            self.brush_painting = false;
        }
        self.params_dirty = true;
        self.fit_to_window = true;
        self.zoom = 1.0;
        self.pan = egui::Vec2::ZERO;
        if self.crop_mode {
            self.brush_painting = false;
            self.show_overlay = false;
            self.status =
                "裁剪模式：拖动手柄改变构图，框外拖动摆正，R / 回车完成，Esc 取消".to_string();
        } else {
            // 退出时把矩形约束到合法范围并记录一步历史。
            let mut params = self.engine.params().clone();
            params.crop.rect = fdxmp_core::geometry::clamp_center(params.crop.rect);
            self.engine.set_params_live(params);
            self.pending_history = Some("裁剪".to_string());
            self.status = "已应用裁剪（导出会按裁剪后的画面输出）".to_string();
        }
    }

    /// 裁剪模式的顶部工具条（比例 / 翻转 / 网格 / 自动构图 / 重置 / 完成）。
    fn crop_toolbar(&mut self, ui: &mut egui::Ui, canvas_rect: egui::Rect) {
        use fdxmp_core::params::{AspectRatio, CropGrid};
        let mut chosen_aspect: Option<AspectRatio> = None;
        let mut toggle_flip_h = false;
        let mut toggle_flip_v = false;
        let mut chosen_grid: Option<CropGrid> = None;
        let mut toggle_auto = false;
        let mut reset = false;
        let mut finish = false;

        egui::Area::new(egui::Id::new("fdxmp-crop-toolbar"))
            .fixed_pos(canvas_rect.left_top() + egui::vec2(12.0, 10.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let crop = self.engine.params().crop.clone();
                        for (label, aspect) in [
                            ("自由", AspectRatio::Free),
                            ("1:1", AspectRatio::Square),
                            ("4:3", AspectRatio::FourThree),
                            ("3:2", AspectRatio::ThreeTwo),
                            ("16:9", AspectRatio::SixteenNine),
                            ("原始", AspectRatio::Original),
                        ] {
                            if ui.selectable_label(crop.aspect == aspect, label).clicked() {
                                chosen_aspect = Some(aspect);
                            }
                        }
                        ui.separator();
                        if ui
                            .selectable_label(crop.flip_horizontal, "水平翻转")
                            .clicked()
                        {
                            toggle_flip_h = true;
                        }
                        if ui
                            .selectable_label(crop.flip_vertical, "垂直翻转")
                            .clicked()
                        {
                            toggle_flip_v = true;
                        }
                        ui.separator();
                        for (label, grid) in [
                            ("三分", CropGrid::Thirds),
                            ("网格", CropGrid::Grid),
                            ("对角", CropGrid::Diagonal),
                            ("三角", CropGrid::Triangle),
                            ("黄金", CropGrid::GoldenRatio),
                            ("无", CropGrid::None),
                        ] {
                            if ui.selectable_label(crop.grid == grid, label).clicked() {
                                chosen_grid = Some(grid);
                            }
                        }
                        ui.separator();
                        if ui.selectable_label(crop.auto_compose, "自动构图").clicked() {
                            toggle_auto = true;
                        }
                        if ui.button("重置").clicked() {
                            reset = true;
                        }
                        if ui.button("完成").clicked() {
                            finish = true;
                        }
                    });
                });
            });

        if let Some(aspect) = chosen_aspect {
            self.edit_geometry_live("裁剪比例", |params| params.crop.aspect = aspect);
            self.apply_aspect_choice();
            self.pending_history = Some("裁剪比例".to_string());
        }
        if toggle_flip_h || toggle_flip_v {
            self.edit_geometry_live("裁剪翻转", |params| {
                if toggle_flip_h {
                    params.crop.flip_horizontal = !params.crop.flip_horizontal;
                }
                if toggle_flip_v {
                    params.crop.flip_vertical = !params.crop.flip_vertical;
                }
            });
            self.pending_history = Some("裁剪翻转".to_string());
        }
        if let Some(grid) = chosen_grid {
            self.edit_geometry_live("裁剪网格", |params| params.crop.grid = grid);
            self.pending_history = Some("裁剪网格".to_string());
        }
        if toggle_auto {
            self.edit_geometry_live("自动构图", |params| {
                params.crop.auto_compose = !params.crop.auto_compose;
            });
            if self.engine.params().crop.auto_compose {
                self.apply_aspect_choice();
            }
            self.pending_history = Some("自动构图".to_string());
        }
        if reset {
            self.edit_geometry_live("裁剪重置", |params| {
                params.crop.rect = [0.0, 0.0, 1.0, 1.0];
                params.crop.rotation_deg = 0.0;
                params.crop.flip_horizontal = false;
                params.crop.flip_vertical = false;
                params.crop.aspect = AspectRatio::Free;
            });
            self.pending_history = Some("裁剪重置".to_string());
        }
        if finish {
            self.toggle_crop_mode();
        }
    }

    /// 应用当前比例选择：调整矩形并保证在旋转后的画面内。
    fn apply_aspect_choice(&mut self) {
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };
        let (source_w, source_h) = (loaded.proxy[0], loaded.proxy[1]);
        let mut params = self.engine.params().clone();
        let aspect = fdxmp_core::geometry::aspect_value(params.crop.aspect, source_w, source_h);
        let image_aspect = source_w as f32 / source_h.max(1) as f32;
        let mut rect =
            fdxmp_core::geometry::rect_with_aspect(params.crop.rect, aspect, image_aspect);
        rect = fdxmp_core::geometry::clamp_center(rect);
        if params.crop.auto_compose {
            let geometry = fdxmp_core::geometry::Geometry::from_params(&params);
            rect = fdxmp_core::geometry::constrain_rect(&geometry, rect, source_w, source_h);
        }
        params.crop.rect = rect;
        self.engine.set_params_live(params);
        self.params_dirty = true;
    }

    /// 裁剪交互：手柄拖拽改框，框外拖拽旋转（对标 Lightroom）。
    fn crop_interaction(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        target_rect: egui::Rect,
    ) {
        let crop = self.engine.params().crop.clone();
        let frame = crop_screen_rect(&crop, target_rect);
        let hit = |position: egui::Pos2| -> Option<CropHandle> {
            const HIT: f32 = 26.0;
            let near_left = (position.x - frame.left()).abs() <= HIT;
            let near_right = (position.x - frame.right()).abs() <= HIT;
            let near_top = (position.y - frame.top()).abs() <= HIT;
            let near_bottom = (position.y - frame.bottom()).abs() <= HIT;
            let inside_x = position.x >= frame.left() - HIT && position.x <= frame.right() + HIT;
            let inside_y = position.y >= frame.top() - HIT && position.y <= frame.bottom() + HIT;
            if near_left && near_top {
                Some(CropHandle::TopLeft)
            } else if near_right && near_top {
                Some(CropHandle::TopRight)
            } else if near_left && near_bottom {
                Some(CropHandle::BottomLeft)
            } else if near_right && near_bottom {
                Some(CropHandle::BottomRight)
            } else if near_left && inside_y {
                Some(CropHandle::Left)
            } else if near_right && inside_y {
                Some(CropHandle::Right)
            } else if near_top && inside_x {
                Some(CropHandle::Top)
            } else if near_bottom && inside_x {
                Some(CropHandle::Bottom)
            } else if frame.contains(position) {
                Some(CropHandle::Move)
            } else {
                Some(CropHandle::Rotate)
            }
        };

        if response.drag_started() {
            if let Some(position) = response.interact_pointer_pos() {
                self.crop_drag = hit(position).map(|handle| CropDrag {
                    handle,
                    start_rect: crop.rect,
                    start_pointer: position,
                    start_rotation: crop.rotation_deg,
                });
            }
        }
        if let Some(drag) = self.crop_drag {
            if response.dragged() {
                if let Some(position) = response.interact_pointer_pos() {
                    let delta = position - drag.start_pointer;
                    self.apply_crop_drag(drag, delta, target_rect);
                }
            }
            if response.drag_stopped() {
                self.crop_drag = None;
                self.pending_history = Some("裁剪".to_string());
            }
        }

        if let Some(position) = ui.input(|input| input.pointer.hover_pos()) {
            if target_rect.contains(position) {
                let cursor = match hit(position) {
                    Some(CropHandle::Rotate) => egui::CursorIcon::Grab,
                    Some(CropHandle::Move) => egui::CursorIcon::Move,
                    Some(CropHandle::Left | CropHandle::Right) => {
                        egui::CursorIcon::ResizeHorizontal
                    }
                    Some(CropHandle::Top | CropHandle::Bottom) => egui::CursorIcon::ResizeVertical,
                    Some(_) => egui::CursorIcon::ResizeNwSe,
                    None => egui::CursorIcon::Default,
                };
                ui.ctx().set_cursor_icon(cursor);
            }
        }
    }

    /// 应用一次裁剪拖拽（基于起始值的绝对计算）。
    fn apply_crop_drag(&mut self, drag: CropDrag, delta: egui::Vec2, target_rect: egui::Rect) {
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };
        let (source_w, source_h) = (loaded.proxy[0], loaded.proxy[1]);
        let mut params = self.engine.params().clone();
        let dx = delta.x / target_rect.width().max(1.0);
        let dy = delta.y / target_rect.height().max(1.0);
        const MIN_SIZE: f32 = 0.06;

        if drag.handle == CropHandle::Rotate {
            let center = target_rect.center();
            let angle_of = |position: egui::Pos2| -> f32 {
                (position.y - center.y)
                    .atan2(position.x - center.x)
                    .to_degrees()
            };
            let start_angle = angle_of(drag.start_pointer);
            let current_angle = angle_of(drag.start_pointer + delta);
            let mut angle = drag.start_rotation + (current_angle - start_angle);
            while angle > 45.0 {
                angle -= 90.0;
            }
            while angle < -45.0 {
                angle += 90.0;
            }
            params.crop.rotation_deg = angle.clamp(-45.0, 45.0);
        } else {
            let mut rect = drag.start_rect;
            match drag.handle {
                CropHandle::Move => {
                    rect[0] += dx;
                    rect[1] += dy;
                }
                CropHandle::Left => {
                    rect[0] += dx;
                    rect[2] -= dx;
                }
                CropHandle::Right => rect[2] += dx,
                CropHandle::Top => {
                    rect[1] += dy;
                    rect[3] -= dy;
                }
                CropHandle::Bottom => rect[3] += dy,
                CropHandle::TopLeft => {
                    rect[0] += dx;
                    rect[1] += dy;
                    rect[2] -= dx;
                    rect[3] -= dy;
                }
                CropHandle::TopRight => {
                    rect[1] += dy;
                    rect[2] += dx;
                    rect[3] -= dy;
                }
                CropHandle::BottomLeft => {
                    rect[0] += dx;
                    rect[2] -= dx;
                    rect[3] += dy;
                }
                CropHandle::BottomRight => {
                    rect[2] += dx;
                    rect[3] += dy;
                }
                CropHandle::Rotate => {}
            }
            // 归一化负尺寸（拖过头时保持边界语义）
            if rect[2] < 0.0 {
                rect[0] += rect[2];
                rect[2] = -rect[2];
            }
            if rect[3] < 0.0 {
                rect[1] += rect[3];
                rect[3] = -rect[3];
            }
            rect[2] = rect[2].max(MIN_SIZE);
            rect[3] = rect[3].max(MIN_SIZE);
            let image_aspect = source_w as f32 / source_h.max(1) as f32;
            let aspect = fdxmp_core::geometry::aspect_value(params.crop.aspect, source_w, source_h);
            rect = fdxmp_core::geometry::rect_with_aspect(rect, aspect, image_aspect);
            params.crop.rect = rect;
        }

        if params.crop.auto_compose {
            let geometry = fdxmp_core::geometry::Geometry::from_params(&params);
            params.crop.rect = fdxmp_core::geometry::constrain_rect(
                &geometry,
                params.crop.rect,
                source_w,
                source_h,
            );
        }
        params.crop.rect = fdxmp_core::geometry::clamp_center(params.crop.rect);
        self.engine.set_params_live(params);
        self.params_dirty = true;
    }

    /// 绘制裁剪叠加层：框外压暗、网格、边框与手柄。
    fn paint_crop_overlay(&self, painter: &egui::Painter, target_rect: egui::Rect) {
        use fdxmp_core::params::CropGrid;
        let params = self.engine.params();
        let frame = crop_screen_rect(&params.crop, target_rect);
        let dim = egui::Color32::from_black_alpha(150);

        if frame.top() > target_rect.top() {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    target_rect.min,
                    egui::pos2(target_rect.right(), frame.top()),
                ),
                0.0,
                dim,
            );
        }
        if frame.bottom() < target_rect.bottom() {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(target_rect.left(), frame.bottom()),
                    target_rect.max,
                ),
                0.0,
                dim,
            );
        }
        if frame.left() > target_rect.left() {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(target_rect.left(), frame.top()),
                    egui::pos2(frame.left(), frame.bottom()),
                ),
                0.0,
                dim,
            );
        }
        if frame.right() < target_rect.right() {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(frame.right(), frame.top()),
                    egui::pos2(target_rect.right(), frame.bottom()),
                ),
                0.0,
                dim,
            );
        }

        let grid_stroke = egui::Stroke::new(1.0, egui::Color32::from_white_alpha(110));
        let line = |a: egui::Pos2, b: egui::Pos2| painter.line_segment([a, b], grid_stroke);
        let fx = |t: f32| frame.left() + frame.width() * t;
        let fy = |t: f32| frame.top() + frame.height() * t;
        match params.crop.grid {
            CropGrid::None => {}
            CropGrid::Thirds => {
                for t in [1.0 / 3.0, 2.0 / 3.0] {
                    line(
                        egui::pos2(fx(t), frame.top()),
                        egui::pos2(fx(t), frame.bottom()),
                    );
                    line(
                        egui::pos2(frame.left(), fy(t)),
                        egui::pos2(frame.right(), fy(t)),
                    );
                }
            }
            CropGrid::Grid => {
                for i in 1..6 {
                    let t = i as f32 / 6.0;
                    line(
                        egui::pos2(fx(t), frame.top()),
                        egui::pos2(fx(t), frame.bottom()),
                    );
                }
                for i in 1..4 {
                    let t = i as f32 / 4.0;
                    line(
                        egui::pos2(frame.left(), fy(t)),
                        egui::pos2(frame.right(), fy(t)),
                    );
                }
            }
            CropGrid::GoldenRatio => {
                for t in [0.382, 0.618] {
                    line(
                        egui::pos2(fx(t), frame.top()),
                        egui::pos2(fx(t), frame.bottom()),
                    );
                    line(
                        egui::pos2(frame.left(), fy(t)),
                        egui::pos2(frame.right(), fy(t)),
                    );
                }
            }
            CropGrid::Diagonal => {
                line(frame.left_top(), frame.right_bottom());
                line(frame.right_top(), frame.left_bottom());
            }
            CropGrid::Triangle => {
                line(frame.left_top(), frame.right_bottom());
                line(frame.left_top(), frame.right_top());
                line(frame.right_top(), frame.left_bottom());
                line(
                    egui::pos2(fx(0.5), frame.top()),
                    egui::pos2(frame.right(), fy(0.5)),
                );
                line(
                    egui::pos2(frame.left(), fy(0.5)),
                    egui::pos2(fx(0.5), frame.bottom()),
                );
            }
        }

        if let Some(drag) = self.crop_drag {
            if drag.handle == CropHandle::Rotate {
                let stroke = egui::Stroke::new(1.0, egui::Color32::from_white_alpha(80));
                for i in 1..6 {
                    let t = target_rect.width() * i as f32 / 6.0;
                    painter.line_segment(
                        [
                            egui::pos2(target_rect.left() + t, target_rect.top()),
                            egui::pos2(target_rect.left() + t, target_rect.bottom()),
                        ],
                        stroke,
                    );
                }
                for i in 1..4 {
                    let t = target_rect.height() * i as f32 / 4.0;
                    painter.line_segment(
                        [
                            egui::pos2(target_rect.left(), target_rect.top() + t),
                            egui::pos2(target_rect.right(), target_rect.top() + t),
                        ],
                        stroke,
                    );
                }
            }
        }

        let shadow = egui::Stroke::new(3.0, egui::Color32::from_black_alpha(120));
        let border = egui::Stroke::new(1.5, egui::Color32::WHITE);
        painter.rect_stroke(frame, 0.0, shadow, egui::StrokeKind::Middle);
        painter.rect_stroke(frame, 0.0, border, egui::StrokeKind::Middle);

        let arm = 22.0f32;
        let thick = egui::Stroke::new(4.0, egui::Color32::WHITE);
        let corner_strokes = [
            (egui::pos2(frame.left(), frame.top()), egui::vec2(arm, 0.0)),
            (egui::pos2(frame.left(), frame.top()), egui::vec2(0.0, arm)),
            (
                egui::pos2(frame.right() - arm, frame.top()),
                egui::vec2(arm, 0.0),
            ),
            (egui::pos2(frame.right(), frame.top()), egui::vec2(0.0, arm)),
            (
                egui::pos2(frame.left(), frame.bottom()),
                egui::vec2(arm, 0.0),
            ),
            (
                egui::pos2(frame.left(), frame.bottom() - arm),
                egui::vec2(0.0, arm),
            ),
            (
                egui::pos2(frame.right() - arm, frame.bottom()),
                egui::vec2(arm, 0.0),
            ),
            (
                egui::pos2(frame.right(), frame.bottom() - arm),
                egui::vec2(0.0, arm),
            ),
        ];
        for (start, offset) in corner_strokes {
            painter.line_segment([start, start + offset], thick);
        }
        let edge = 16.0f32;
        painter.line_segment(
            [
                egui::pos2(frame.center().x - edge * 0.5, frame.top()),
                egui::pos2(frame.center().x + edge * 0.5, frame.top()),
            ],
            thick,
        );
        painter.line_segment(
            [
                egui::pos2(frame.center().x - edge * 0.5, frame.bottom()),
                egui::pos2(frame.center().x + edge * 0.5, frame.bottom()),
            ],
            thick,
        );
        painter.line_segment(
            [
                egui::pos2(frame.left(), frame.center().y - edge * 0.5),
                egui::pos2(frame.left(), frame.center().y + edge * 0.5),
            ],
            thick,
        );
        painter.line_segment(
            [
                egui::pos2(frame.right(), frame.center().y - edge * 0.5),
                egui::pos2(frame.right(), frame.center().y + edge * 0.5),
            ],
            thick,
        );
    }

    /// 应用画布右键菜单动作。
    fn apply_canvas_action(&mut self, action: Option<CanvasAction>, ctx: &egui::Context) {
        match action {
            Some(CanvasAction::Fit) => {
                self.fit_to_window = true;
                self.zoom = 1.0;
                self.pan = egui::Vec2::ZERO;
                if self.detail_mode {
                    self.exit_detail_mode();
                }
            }
            Some(CanvasAction::Detail) => {
                if self.detail_mode {
                    self.exit_detail_mode();
                } else {
                    self.enter_detail_mode(ctx);
                }
            }
            Some(CanvasAction::Crop) => self.toggle_crop_mode(),
            Some(CanvasAction::Mask) => self.enter_mask_mode(None),
            Some(CanvasAction::Overlay(value)) => self.show_overlay = value,
            Some(CanvasAction::ResetGeometry) => {
                self.edit_geometry_live("重置裁剪与变换", |params| {
                    params.crop.rect = [0.0, 0.0, 1.0, 1.0];
                    params.crop.rotation_deg = 0.0;
                    params.crop.flip_horizontal = false;
                    params.crop.flip_vertical = false;
                    params.crop.aspect = fdxmp_core::params::AspectRatio::Free;
                    params.transform = fdxmp_core::params::TransformParams::default();
                });
                self.pending_history = Some("重置裁剪与变换".to_string());
                self.params_dirty = true;
            }
            None => {}
        }
    }

    /// 从胶片条移除一张照片（同步调整选中与批量导出标记）。
    fn remove_photo(&mut self, index: usize) {
        if index >= self.photos.len() {
            return;
        }
        self.photos.remove(index);
        let mut remapped = std::collections::HashSet::new();
        for &old in &self.marked {
            if old < index {
                remapped.insert(old);
            } else if old > index {
                remapped.insert(old - 1);
            }
        }
        self.marked = remapped;
        match self.selected {
            Some(current) if current == index => {
                self.selected = None;
                self.loaded = None;
            }
            Some(current) if current > index => {
                self.selected = Some(current - 1);
            }
            _ => {}
        }
        self.status = "已从胶片条移除（不会删除磁盘文件）".to_string();
    }

    /// 进入 1:1（100%）细节视图：按需解码全分辨率，只渲染可见分块。
    fn enter_detail_mode(&mut self, ctx: &egui::Context) {
        if self.loaded.is_none() {
            self.status = "请先打开一张图片".to_string();
            return;
        }
        self.crop_mode = false;
        self.mask_mode = false;
        self.mask_drag = None;
        self.brush_painting = false;
        self.detail_mode = true;
        self.pan = egui::Vec2::ZERO;
        #[cfg(feature = "gpu")]
        {
            if self.detail_tile.is_none() {
                if let Some(gpu) = self.gpu.as_ref() {
                    self.detail_tile = Some(GpuPreview::new(&gpu.state));
                }
            }
        }
        if self.detail_full.is_some() || self.detail_rx.is_some() {
            return;
        }
        let Some(path) = self
            .selected
            .and_then(|index| self.photos.get(index))
            .and_then(|photo| photo.path.clone())
        else {
            self.status = "当前图片没有源文件路径，无法解码全分辨率".to_string();
            return;
        };
        let (sender, receiver) = channel();
        self.detail_rx = Some(receiver);
        self.detail_for = self.selected;
        self.status = "正在解码全分辨率（1:1 视图）…".to_string();
        ctx.request_repaint();
        std::thread::spawn(move || {
            let result = fdxmp_core::decode::decode(&path).map_err(|err| err.to_string());
            let _ = sender.send(result);
        });
    }

    /// 退出 1:1 视图并释放全分辨率内存。
    fn exit_detail_mode(&mut self) {
        self.detail_mode = false;
        self.detail_full = None;
        self.detail_rx = None;
        self.detail_for = None;
        self.detail_tile_rect = None;
        self.status = "已退出 1:1 视图（全分辨率内存已释放）".to_string();
    }

    /// 轮询全分辨率解码结果。
    fn poll_detail_decode(&mut self, ctx: &egui::Context) {
        let Some(receiver) = &self.detail_rx else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(image)) => {
                self.detail_rx = None;
                self.detail_full = Some(std::sync::Arc::new(image));
                self.detail_tile_rect = None;
                self.status = format!(
                    "1:1 视图已就绪（{}×{}）",
                    self.detail_full
                        .as_ref()
                        .map(|image| image.width)
                        .unwrap_or(0),
                    self.detail_full
                        .as_ref()
                        .map(|image| image.height)
                        .unwrap_or(0)
                );
                ctx.request_repaint();
            }
            Ok(Err(message)) => {
                self.detail_rx = None;
                self.detail_mode = false;
                self.status = format!("全分辨率解码失败：{message}");
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            Err(TryRecvError::Disconnected) => {
                self.detail_rx = None;
            }
        }
    }

    /// 1:1 视图渲染与绘制：只处理可见分块（全分辨率像素 → 分块纹理）。
    #[cfg(feature = "gpu")]
    fn detail_canvas(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let painter = ui.painter_at(rect);
        let Some(full) = self.detail_full.clone() else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "正在解码全分辨率…",
                egui::FontId::proportional(15.0),
                egui::Color32::from_gray(0xb0),
            );
            return;
        };
        let (Some(loaded), Some(tile)) = (self.loaded.as_ref(), self.detail_tile.as_mut()) else {
            return;
        };
        let params = self.engine.params().clone();
        let (full_w, full_h) = (full.width, full.height);
        let geometry = fdxmp_core::geometry::Geometry::from_params(&params);
        let (out_w, out_h) = fdxmp_core::geometry::output_size(
            &params,
            full_w,
            full_h,
            fdxmp_core::geometry::GeometryMode::Crop,
        );
        let full_matrix = fdxmp_core::geometry::output_to_source_matrix(
            &geometry,
            params.crop.rect,
            full_w,
            full_h,
            out_w,
            out_h,
        );

        let ppp = ui.ctx().pixels_per_point().max(0.1);
        let view_px = egui::vec2(rect.width() * ppp, rect.height() * ppp);
        let mut center = egui::vec2(out_w as f32 * 0.5, out_h as f32 * 0.5) - self.pan * ppp;
        center.x = if view_px.x >= out_w as f32 {
            out_w as f32 * 0.5
        } else {
            center
                .x
                .clamp(view_px.x * 0.5, out_w as f32 - view_px.x * 0.5)
        };
        center.y = if view_px.y >= out_h as f32 {
            out_h as f32 * 0.5
        } else {
            center
                .y
                .clamp(view_px.y * 0.5, out_h as f32 - view_px.y * 0.5)
        };

        let tx0 = (center.x - view_px.x * 0.5).floor().max(0.0);
        let ty0 = (center.y - view_px.y * 0.5).floor().max(0.0);
        let tx1 = (center.x + view_px.x * 0.5)
            .ceil()
            .min(out_w as f32)
            .max(tx0 + 1.0);
        let ty1 = (center.y + view_px.y * 0.5)
            .ceil()
            .min(out_h as f32)
            .max(ty0 + 1.0);
        let tile_out_w = (tx1 - tx0) as u32;
        let tile_out_h = (ty1 - ty0) as u32;

        // 需要的源图矩形（四角逆映射 + 边距），按 64px 网格量化以减少重复提取。
        let corners = [[tx0, ty0], [tx1, ty0], [tx0, ty1], [tx1, ty1]];
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for corner in corners {
            let mapped = fdxmp_core::geometry::apply(&full_matrix, corner);
            min_x = min_x.min(mapped[0]);
            min_y = min_y.min(mapped[1]);
            max_x = max_x.max(mapped[0]);
            max_y = max_y.max(mapped[1]);
        }
        let quant = 64.0f32;
        // 源图边距必须覆盖细节模糊的抽头范围（半径 × 缩放上限）+ 几何双线性采样，
        // 否则分块边缘会因 clamp 采样而与整图渲染出现接缝。
        let margin =
            fdxmp_core::gpu_pipeline::BLUR_RADIUS * fdxmp_core::gpu_pipeline::BLUR_SCALE_MAX + 4.0;
        let sx0 = (((min_x - margin).max(0.0)) / quant).floor() * quant;
        let sy0 = (((min_y - margin).max(0.0)) / quant).floor() * quant;
        let sx1 = (((max_x + margin).min(full_w as f32)) / quant).ceil() * quant;
        let sy1 = (((max_y + margin).min(full_h as f32)) / quant).ceil() * quant;
        let source_rect = (
            sx0 as i64,
            sy0 as i64,
            ((sx1 - sx0).max(1.0)) as u32,
            ((sy1 - sy0).max(1.0)) as u32,
        );

        if self.detail_tile_rect != Some(source_rect) {
            // 按块从解码数据取线性像素（不为整图保留 Rgba32F 副本）
            let sub = full.linear_region(
                source_rect.0 as u32,
                source_rect.1 as u32,
                source_rect.2,
                source_rect.3,
            );
            // 镜头配置文件校正在 1:1 视图同样生效（此前 1:1 直接显示未校正画面）
            let sub = match &self.lens_profile {
                Some(profile) => profile.apply_f32(&sub),
                None => sub,
            };
            if tile
                .upload_linear(sub.as_raw(), sub.width(), sub.height())
                .is_ok()
            {
                let scale = [
                    full_w as f32 / loaded.proxy[0].max(1) as f32,
                    full_h as f32 / loaded.proxy[1].max(1) as f32,
                ];
                tile.set_mask_reference(
                    std::sync::Arc::new(loaded.proxy_pixels.clone()),
                    scale,
                    [source_rect.0 as f32, source_rect.1 as f32],
                );
                self.detail_tile_rect = Some(source_rect);
            }
        }

        let tile_matrix = fdxmp_core::geometry::tile_matrix(
            &full_matrix,
            [tx0, ty0],
            [source_rect.0 as f32, source_rect.1 as f32],
        );
        tile.render_tile(
            &params,
            &tile_matrix,
            tile_out_w,
            tile_out_h,
            [tx0, ty0],
            [out_w as f32, out_h as f32],
        );
        if let Some(texture_id) = tile.texture_id {
            let origin = rect.center() + egui::vec2((tx0 - center.x) / ppp, (ty0 - center.y) / ppp);
            let size = egui::vec2(tile_out_w as f32 / ppp, tile_out_h as f32 / ppp);
            painter.image(
                texture_id,
                egui::Rect::from_min_size(origin, size),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }

    /// 缩放工具条（普通视图：适应 / 1:1；1:1 视图：适应 + 提示）。
    fn zoom_toolbar(&mut self, ui: &mut egui::Ui, canvas_rect: egui::Rect) {
        let mut exit_detail = false;
        let mut enter_detail = false;
        let mut fit = false;
        egui::Area::new(egui::Id::new("fdxmp-zoom-toolbar"))
            .fixed_pos(canvas_rect.right_top() + egui::vec2(-190.0, 10.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(self.fit_to_window && !self.detail_mode, "适应")
                            .clicked()
                        {
                            fit = true;
                        }
                        if ui.selectable_label(self.detail_mode, "1:1").clicked() {
                            if self.detail_mode {
                                exit_detail = true;
                            } else {
                                enter_detail = true;
                            }
                        }
                        if self.detail_mode {
                            ui.weak("100%");
                        } else {
                            ui.weak(format!("{:.0}%", (self.zoom * 100.0).max(1.0)));
                        }
                    });
                });
            });
        if fit {
            self.fit_to_window = true;
            self.zoom = 1.0;
            self.pan = egui::Vec2::ZERO;
            if self.detail_mode {
                exit_detail = true;
            }
        }
        if exit_detail {
            self.exit_detail_mode();
        }
        if enter_detail {
            let ctx = ui.ctx().clone();
            self.enter_detail_mode(&ctx);
        }
    }

    fn filmstrip(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("胶片条 · {} 张", self.photos.len())).strong());
            ui.separator();
            if ui.small_button("◀").clicked() {
                self.select_offset(-1, ui.ctx());
            }
            if ui.small_button("▶").clicked() {
                self.select_offset(1, ui.ctx());
            }
            ui.separator();
            ui.weak("← → 切换 · 选中后自动解码");
        });

        let mut clicked = None;
        let mut mark_toggle: Option<(usize, bool)> = None;
        let mut remove_index: Option<usize> = None;
        egui::ScrollArea::horizontal()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if self.photos.is_empty() {
                    ui.weak("尚未导入照片");
                    return;
                }
                ui.horizontal(|ui| {
                    for (index, photo) in self.photos.iter().enumerate() {
                        let selected = self.selected == Some(index);
                        let marked = self.marked.contains(&index);
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(112.0, 74.0), egui::Sense::click());
                        let painter = ui.painter_at(rect);
                        painter.rect_filled(
                            rect,
                            egui::CornerRadius::from(2u8),
                            egui::Color32::from_gray(0x28),
                        );
                        match &photo.thumbnail {
                            Some(texture) => {
                                let size = texture.size_vec2();
                                let scale = (rect.width() / size.x).min(rect.height() / size.y);
                                let image_rect =
                                    egui::Rect::from_center_size(rect.center(), size * scale);
                                painter.image(
                                    texture.id(),
                                    image_rect,
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    egui::Color32::WHITE,
                                );
                            }
                            None => {
                                let text = if photo.thumbnail_failed {
                                    "缩略图失败"
                                } else {
                                    "生成中…"
                                };
                                painter.text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    text,
                                    egui::FontId::proportional(11.0),
                                    egui::Color32::from_gray(0x88),
                                );
                            }
                        }
                        if selected {
                            painter.rect_stroke(
                                rect,
                                egui::CornerRadius::from(2u8),
                                egui::Stroke::new(2.0, egui::Color32::from_rgb(0x4A, 0x9E, 0xFF)),
                                egui::StrokeKind::Inside,
                            );
                        }
                        // 批量导出标记（左上角勾选框）
                        let mark_rect = egui::Rect::from_min_size(
                            rect.min + egui::vec2(4.0, 4.0),
                            egui::vec2(16.0, 16.0),
                        );
                        let mut check = marked;
                        if ui
                            .put(mark_rect, egui::Checkbox::without_text(&mut check))
                            .on_hover_text("标记用于批量导出")
                            .changed()
                        {
                            mark_toggle = Some((index, check));
                        }
                        let tile_response = response.on_hover_text(photo.tooltip());
                        if tile_response.clicked() {
                            clicked = Some(index);
                        }
                        // 右键菜单：载入 / 标记 / 移除 / 在资源管理器中显示。
                        tile_response.context_menu(|ui| {
                            if ui.button("载入此图").clicked() {
                                clicked = Some(index);
                                ui.close();
                            }
                            let label = if marked {
                                "取消批量导出标记"
                            } else {
                                "标记（批量导出）"
                            };
                            if ui.button(label).clicked() {
                                mark_toggle = Some((index, !marked));
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("从列表移除").clicked() {
                                remove_index = Some(index);
                                ui.close();
                            }
                            if photo.path.is_some() && ui.button("在资源管理器中显示").clicked()
                            {
                                if let Some(path) = &photo.path {
                                    open_in_explorer(path);
                                }
                                ui.close();
                            }
                        });
                    }
                });
            });
        if let Some(index) = remove_index {
            self.remove_photo(index);
        }
        if let Some((index, marked)) = mark_toggle {
            if marked {
                self.marked.insert(index);
            } else {
                self.marked.remove(&index);
            }
        }
        if let Some(index) = clicked {
            self.selected = Some(index);
            let ctx = ui.ctx().clone();
            self.load_selected(&ctx);
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(&self.status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let cache = self
                    .engine
                    .cache()
                    .total_size()
                    .map(CacheManager::human_size)
                    .unwrap_or_else(|_| "-".to_string());
                ui.label(format!("缓存 {cache}"));
                ui.separator();
                ui.label(format!(
                    "历史 {}/{}",
                    self.engine.history().index() + 1,
                    self.engine.history().len()
                ));
                ui.separator();
                ui.label(format!(
                    "{} · ABI {}",
                    fdxmp_core::version(),
                    fdxmp_core::ABI_VERSION
                ));
                ui.separator();
                #[cfg(feature = "gpu")]
                {
                    if let Some(gpu) = &self.gpu {
                        ui.label(format!("GPU {:.2} ms", gpu.frame_ms));
                        ui.separator();
                    }
                }
                ui.label(&self.gpu_label);
            });
        });
    }

    fn import_dialog(&mut self, ctx: &egui::Context) {
        let picked = rfd::FileDialog::new()
            .add_filter(
                "图片",
                &[
                    "jpg", "jpeg", "png", "tif", "tiff", "webp", "bmp", "gif", "heic", "heif",
                    "cr2", "cr3", "nef", "nrw", "arw", "dng", "raf", "orf", "rw2", "pef", "srw",
                ],
            )
            .pick_files();
        match picked {
            Some(paths) if !paths.is_empty() => {
                let start = self.photos.len();
                for path in paths {
                    self.add_photo(path);
                }
                let count = self.photos.len() - start;
                let indices: Vec<usize> = (start..self.photos.len()).collect();
                self.status = format!("已加入 {count} 张，正在生成缩略图并解码最后一张…");
                self.start_thumbnails(ctx, indices);
                self.load_selected(ctx);
            }
            _ => {
                self.status = "已取消导入".to_string();
            }
        }
    }

    fn add_photo(&mut self, path: PathBuf) {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("未命名")
            .to_string();
        self.photos.push(PhotoSlot {
            name,
            path: Some(path),
            thumbnail: None,
            thumbnail_failed: false,
        });
        self.selected = Some(self.photos.len() - 1);
    }

    fn select_offset(&mut self, offset: isize, ctx: &egui::Context) {
        if self.photos.is_empty() {
            return;
        }
        let current = self.selected.unwrap_or(0) as isize;
        let next = (current + offset).clamp(0, self.photos.len() as isize - 1);
        self.selected = Some(next as usize);
        self.load_selected(ctx);
    }

    /// 解码当前选中的照片（首屏路径：RAW 内嵌预览 + 代理图）。
    fn load_selected(&mut self, ctx: &egui::Context) {
        let Some(photo) = self.selected.and_then(|index| self.photos.get(index)) else {
            return;
        };
        let Some(path) = photo.path.clone() else {
            return;
        };
        let name = photo.name.clone();
        // 切换图片时退出 1:1 视图（释放上一张的全分辨率内存）。
        if self.detail_mode {
            self.detail_mode = false;
            self.detail_full = None;
            self.detail_rx = None;
            self.detail_for = None;
            self.detail_tile_rect = None;
        }
        let (sender, receiver) = channel();
        self.decode_rx = Some(receiver);
        self.decoding = Some(name.clone());
        self.status = format!("正在解码 {name}（快速首屏路径）…");
        ctx.request_repaint();

        std::thread::spawn(move || {
            let result = fdxmp_core::decode::decode_preview(&path).map_err(|err| err.to_string());
            let _ = sender.send(result);
        });
    }

    fn poll_decode(&mut self, ctx: &egui::Context) {
        let Some(receiver) = &self.decode_rx else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(decoded)) => {
                self.decode_rx = None;
                self.decoding = None;
                self.apply_decoded(ctx, decoded, false);
            }
            Ok(Err(message)) => {
                self.decode_rx = None;
                self.decoding = None;
                self.status = format!("解码失败：{message}");
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(80));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.decode_rx = None;
                self.decoding = None;
            }
        }
    }

    fn apply_decoded(&mut self, ctx: &egui::Context, decoded: DecodedImage, is_full: bool) {
        let source = decoded.source;
        let elapsed_ms = decoded.elapsed_ms;
        // 线性代理（GPU 上传，gamma 正确、保留 16bit 精度）+ 对应 8bit 版本
        // （蒙版 / 直方图 / CPU 回退），一次重采样得到两者。
        let (proxy_linear, proxy) = decoded.display_proxy_pair(2048);
        #[cfg(not(feature = "gpu"))]
        let _ = &proxy_linear;
        let (proxy_w, proxy_h) = (proxy.width(), proxy.height());
        let histogram = Histogram::from_rgba8(proxy.as_raw(), proxy_w as usize, proxy_h as usize);
        let texture = texture_from(ctx, &proxy, "fdxmp-canvas");
        let overlay_cache = proxy.clone();

        self.status = format!(
            "{}{}×{}（{}，{} ms，代理 {}×{}）",
            if is_full {
                "全尺寸就绪 "
            } else {
                "首屏完成 "
            },
            decoded.width,
            decoded.height,
            source.label_zh(),
            elapsed_ms,
            proxy_w,
            proxy_h
        );
        let metadata = decoded.metadata.clone();
        // 新图片：AI 蒙版缓存失效。
        #[cfg(feature = "ai")]
        fdxmp_core::ai::clear_cache();
        self.pristine_proxy = Some(proxy.clone());
        self.loaded = Some(LoadedImage {
            texture,
            proxy_pixels: overlay_cache,
            original: [decoded.width, decoded.height],
            proxy: [proxy_w, proxy_h],
            source: source.label_zh().to_string(),
            elapsed_ms,
            metadata,
            histogram,
            path: self
                .selected
                .and_then(|index| self.photos.get(index))
                .map(|photo| photo.name.clone())
                .unwrap_or_default(),
        });
        self.pan = egui::Vec2::ZERO;
        self.fit_to_window = true;
        self.zoom = 1.0;

        #[cfg(feature = "gpu")]
        {
            if let Some(gpu) = self.gpu.as_mut() {
                let result = gpu
                    .upload_linear(&proxy_linear, proxy_w, proxy_h)
                    .or_else(|_| gpu.upload(&proxy));
                match result {
                    Ok(()) => {
                        self.params_dirty = true;
                    }
                    Err(message) => {
                        self.status = format!("GPU 上传失败，回退到 CPU 显示：{message}");
                    }
                }
            }
        }
        // 新图片沿用已选的镜头配置文件（自动重新校正）。
        if let Some(profile) = self.lens_profile.clone() {
            self.start_profile_apply(profile);
        }

        // 首屏用的是内嵌预览 / 半尺寸时，后台继续渲染全尺寸并替换画布。
        let needs_full_quality = !is_full
            && matches!(
                source,
                fdxmp_core::decode::DecodeSource::EmbeddedPreview
                    | fdxmp_core::decode::DecodeSource::HalfRaw
            );
        if needs_full_quality {
            if let Some(index) = self.selected {
                if let Some(path) = self.photos.get(index).and_then(|photo| photo.path.clone()) {
                    self.status.push_str(" · 后台生成全尺寸…");
                    self.start_full_decode(ctx, index, path);
                }
            }
        }
    }

    /// 后台全尺寸解码（首屏之后再出精细结果）。
    fn start_full_decode(&mut self, ctx: &egui::Context, index: usize, path: PathBuf) {
        let (sender, receiver) = channel();
        self.full_rx = Some(receiver);
        self.full_for = Some(index);
        ctx.request_repaint();
        std::thread::spawn(move || {
            let result = fdxmp_core::decode::decode(&path).map_err(|err| err.to_string());
            let _ = sender.send((index, result));
        });
    }

    fn poll_full_decode(&mut self, ctx: &egui::Context) {
        let Some(receiver) = &self.full_rx else {
            return;
        };
        match receiver.try_recv() {
            Ok((index, Ok(decoded))) => {
                self.full_rx = None;
                self.full_for = None;
                if self.selected == Some(index) {
                    self.apply_decoded(ctx, decoded, true);
                }
            }
            Ok((index, Err(message))) => {
                self.full_rx = None;
                self.full_for = None;
                if self.selected == Some(index) {
                    self.status = format!("全尺寸解码失败：{message}");
                }
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
            }
            Err(TryRecvError::Disconnected) => {
                self.full_rx = None;
                self.full_for = None;
            }
        }
    }

    /// 批量生成缩略图（rayon 并行 + 磁盘缓存）。
    fn start_thumbnails(&mut self, ctx: &egui::Context, indices: Vec<usize>) {
        let jobs: Vec<(usize, PathBuf)> = indices
            .into_iter()
            .filter_map(|index| {
                self.photos
                    .get(index)
                    .and_then(|photo| photo.path.clone())
                    .map(|path| (index, path))
            })
            .collect();
        if jobs.is_empty() {
            return;
        }
        let cache_root = self.engine.cache().root().to_path_buf();
        let (sender, receiver) = channel();
        self.thumb_rx = Some(receiver);
        ctx.request_repaint();
        std::thread::spawn(move || {
            use rayon::prelude::*;
            jobs.into_par_iter()
                .for_each_with(sender, |sender, (index, path)| {
                    let result = load_or_make_thumbnail(&path, &cache_root, 240);
                    let _ = sender.send((index, result));
                });
        });
    }

    fn poll_thumbnails(&mut self, ctx: &egui::Context) {
        let Some(receiver) = &self.thumb_rx else {
            return;
        };
        let mut disconnected = false;
        loop {
            match receiver.try_recv() {
                Ok((index, Ok(thumbnail))) => {
                    if let Some(photo) = self.photos.get_mut(index) {
                        photo.thumbnail = Some(texture_from(ctx, &thumbnail, "fdxmp-thumb"));
                        photo.thumbnail_failed = false;
                    }
                }
                Ok((index, Err(_))) => {
                    if let Some(photo) = self.photos.get_mut(index) {
                        photo.thumbnail_failed = true;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if disconnected {
            self.thumb_rx = None;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    fn undo(&mut self) {
        self.status = if self.engine.undo() {
            "已撤销".to_string()
        } else {
            "没有可撤销的步骤".to_string()
        };
        self.params_dirty = true;
    }

    fn redo(&mut self) {
        self.status = if self.engine.redo() {
            "已重做".to_string()
        } else {
            "没有可重做的步骤".to_string()
        };
        self.params_dirty = true;
    }

    /// GPU 实时预览：参数变化时重跑管线，静置后再刷新直方图。
    #[cfg(feature = "gpu")]
    fn update_gpu(&mut self, ctx: &egui::Context) {
        if self.params_dirty {
            self.params_dirty = false;
            if let Some(gpu) = self.gpu.as_mut() {
                let params = self.engine.params().clone();
                let mode = if self.crop_mode {
                    fdxmp_core::geometry::GeometryMode::Full
                } else {
                    fdxmp_core::geometry::GeometryMode::Crop
                };
                // 1:1 视图下代理预览暂停（画布改渲染全分辨率分块）。
                let detail_ready = self.detail_mode && self.detail_full.is_some();
                if !detail_ready {
                    gpu.render(&params, mode);
                }
                // 蒙版叠加直接在 GPU 上合成（裁剪 / 旋转后仍与画面内容对齐）。
                if !self.crop_mode && self.show_overlay && !detail_ready {
                    if let EditTarget::Mask(index) = self.edit_target {
                        if let Some(mask) = params.masks.get(index) {
                            let rgba = fdxmp_core::masks::overlay_rgba(mask.overlay);
                            gpu.render_overlay(
                                &params,
                                index,
                                fdxmp_core::gpu_pipeline::OverlayStyle {
                                    color: [rgba[0], rgba[1], rgba[2]],
                                    alpha: rgba[3],
                                },
                            );
                        }
                    }
                }
            }
            self.last_change = std::time::Instant::now();
            self.histogram_dirty = true;
            ctx.request_repaint();
        }

        if self.histogram_dirty {
            if self.last_change.elapsed() >= std::time::Duration::from_millis(300) {
                self.histogram_dirty = false;
                if let Some(gpu) = self.gpu.as_ref() {
                    if let Some(rgba) = gpu.readback() {
                        let histogram = Histogram::from_rgba8(
                            rgba.as_raw(),
                            rgba.width() as usize,
                            rgba.height() as usize,
                        );
                        if let Some(loaded) = self.loaded.as_mut() {
                            loaded.histogram = histogram;
                        }
                    }
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(120));
            }
        }
    }

    /// 蒙版叠加层：按选中蒙版生成半透明彩色覆盖（CPU 生成，内容哈希缓存）。
    fn update_overlay(&mut self, ctx: &egui::Context) {
        // GPU 路径：叠加层由 `render_overlay` 在渲染管线内合成（几何对齐）。
        #[cfg(feature = "gpu")]
        if self.gpu.is_some() {
            self.overlay_texture = None;
            return;
        }
        if !self.show_overlay {
            self.overlay_texture = None;
            return;
        }

        let prepared = {
            let Some(loaded) = self.loaded.as_ref() else {
                self.overlay_texture = None;
                return;
            };
            let Some(mask) = self.selected_mask().cloned() else {
                self.overlay_texture = None;
                return;
            };
            let (width, height) = (loaded.proxy_pixels.width(), loaded.proxy_pixels.height());
            let key = fdxmp_core::masks::mask_key(&mask, width, height);
            if let Some((cached, _)) = &self.overlay_texture {
                if *cached == key {
                    return;
                }
            }
            match fdxmp_core::masks::generate_mask(&mask, &loaded.proxy_pixels, width, height) {
                Ok(raster) => Some((key, raster, fdxmp_core::masks::overlay_rgba(mask.overlay))),
                Err(_) => None,
            }
        };

        let Some((key, raster, color)) = prepared else {
            self.overlay_texture = None;
            return;
        };
        let (width, height) = (raster.width as usize, raster.height as usize);
        let mut bytes = Vec::with_capacity(raster.weights.len() * 4);
        for weight in &raster.weights {
            let alpha = (weight * color[3]).clamp(0.0, 1.0);
            bytes.push((color[0] * 255.0) as u8);
            bytes.push((color[1] * 255.0) as u8);
            bytes.push((color[2] * 255.0) as u8);
            bytes.push((alpha * 255.0) as u8);
        }
        let color_image = egui::ColorImage::from_rgba_unmultiplied([width, height], &bytes);
        let texture = ctx.load_texture(
            "fdxmp-mask-overlay",
            color_image,
            egui::TextureOptions::LINEAR,
        );
        self.overlay_texture = Some((key, texture));
        ctx.request_repaint();
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let (
            undo,
            redo,
            previous,
            next,
            toggle_panels,
            toggle_crop,
            finish_crop,
            toggle_mask,
            toggle_overlay,
            brush_smaller,
            brush_bigger,
        ) = ctx.input(|input| {
            let command = input.modifiers.command;
            (
                command && !input.modifiers.shift && input.key_pressed(egui::Key::Z),
                (command && input.modifiers.shift && input.key_pressed(egui::Key::Z))
                    || (command && input.key_pressed(egui::Key::Y)),
                input.key_pressed(egui::Key::ArrowLeft),
                input.key_pressed(egui::Key::ArrowRight),
                input.key_pressed(egui::Key::Tab),
                !command && input.key_pressed(egui::Key::R),
                input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Escape),
                !command && input.key_pressed(egui::Key::M),
                !command && input.key_pressed(egui::Key::O),
                input.key_pressed(egui::Key::OpenBracket),
                input.key_pressed(egui::Key::CloseBracket),
            )
        });

        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if previous {
            self.select_offset(-1, ctx);
        }
        if next {
            self.select_offset(1, ctx);
        }
        if toggle_panels {
            let visible = self.show_left || self.show_right;
            self.show_left = !visible;
            self.show_right = !visible;
            self.show_filmstrip = !visible;
        }
        if toggle_crop && !ctx.egui_wants_keyboard_input() {
            self.toggle_crop_mode();
        }
        if finish_crop && self.crop_mode && !ctx.egui_wants_keyboard_input() {
            self.toggle_crop_mode();
        }
        if finish_crop && self.detail_mode && !ctx.egui_wants_keyboard_input() {
            self.exit_detail_mode();
        }
        if toggle_mask && !ctx.egui_wants_keyboard_input() {
            if self.mask_mode {
                self.exit_mask_mode();
            } else {
                self.enter_mask_mode(None);
            }
        }
        if finish_crop && self.mask_mode && !ctx.egui_wants_keyboard_input() {
            self.exit_mask_mode();
        }
        if toggle_overlay && self.mask_mode && !ctx.egui_wants_keyboard_input() {
            self.show_overlay = !self.show_overlay;
        }
        if (brush_smaller || brush_bigger) && self.mask_mode {
            let delta = if brush_bigger { 2.0 } else { -2.0 };
            let apply = move |mask: &mut fdxmp_core::params::MaskParams| {
                if let fdxmp_core::params::MaskKind::Brush(brush) = &mut mask.kind {
                    brush.size = (brush.size + delta).clamp(2.0, 50.0);
                }
            };
            self.edit_mask_live("画笔大小", apply);
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        let count = dropped.len();
        let start = self.photos.len();
        for path in dropped {
            self.add_photo(path);
        }
        let indices: Vec<usize> = (start..self.photos.len()).collect();
        self.status = format!("拖入 {count} 张，正在生成缩略图并解码最后一张…");
        self.start_thumbnails(ctx, indices);
        self.load_selected(ctx);
    }
}

/// 绘制 RGB 直方图（叠加显示）。
fn draw_histogram(ui: &mut egui::Ui, histogram: &Histogram) {
    let height = 108.0;
    let width = ui.available_width().max(120.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        rect,
        egui::CornerRadius::ZERO,
        egui::Color32::from_rgb(0x14, 0x14, 0x14),
    );

    let peak = histogram.peak().max(1) as f32;
    let step = rect.width() / 256.0;
    let channels = [
        (
            &histogram.red,
            egui::Color32::from_rgba_unmultiplied(0xF0, 0x60, 0x60, 0xB0),
        ),
        (
            &histogram.green,
            egui::Color32::from_rgba_unmultiplied(0x60, 0xD0, 0x70, 0xB0),
        ),
        (
            &histogram.blue,
            egui::Color32::from_rgba_unmultiplied(0x60, 0x90, 0xF0, 0xB0),
        ),
    ];
    for (bins, color) in channels {
        for (index, value) in bins.iter().enumerate() {
            let normalized = *value as f32 / peak;
            let bar_height = normalized * (rect.height() - 4.0);
            if bar_height <= 0.5 {
                continue;
            }
            let x = rect.left() + index as f32 * step + step * 0.5;
            painter.line_segment(
                [
                    egui::pos2(x, rect.bottom() - 2.0),
                    egui::pos2(x, rect.bottom() - 2.0 - bar_height),
                ],
                egui::Stroke::new(step.max(1.0), color),
            );
        }
    }
}

fn short_name(name: &str) -> String {
    let stem = name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name);
    let mut text: String = stem.chars().take(12).collect();
    if stem.chars().count() > 12 {
        text.push('…');
    }
    text
}

/// 颜色分级区域的可变访问。
/// 全分辨率导出：重新解码 + 独立 GPU 上下文渲染 + 回读。
#[cfg(feature = "gpu")]
fn render_full_resolution(
    path: &std::path::Path,
    params: &EditParams,
    profile: Option<&fdxmp_core::lens_db::LensCorrection>,
) -> Result<image::RgbaImage, String> {
    use fdxmp_core::geometry::{self, GeometryMode};
    use fdxmp_core::gpu::GpuContext;
    use fdxmp_core::gpu_pipeline::{ColorPipeline, RenderPlan, BLUR_RADIUS, BLUR_SCALE_MAX};

    let decoded = fdxmp_core::decode::decode(path).map_err(|err| err.to_string())?;
    let (source_w, source_h) = (decoded.width, decoded.height);
    let geometry = geometry::Geometry::from_params(params);
    let (out_w, out_h) = geometry::output_size(params, source_w, source_h, GeometryMode::Crop);
    let full_matrix = geometry::output_to_source_matrix(
        &geometry,
        params.crop.rect,
        source_w,
        source_h,
        out_w,
        out_h,
    );
    let context = GpuContext::new_blocking().map_err(|err| err.to_string())?;
    let mut pipeline = ColorPipeline::new(&context.device);
    // 蒙版按代理分辨率生成（分块渲染时用 origin 把裁剪坐标加回整图）。
    let proxy = decoded.display_proxy(2048);
    let mask_scale = [
        source_w as f32 / proxy.width().max(1) as f32,
        source_h as f32 / proxy.height().max(1) as f32,
    ];
    let proxy = std::sync::Arc::new(proxy);

    // 分块渲染：GPU 纹理只保留一个分块（45MP 全图的 f16 工作集 ≈ 2.9GB → 约 0.3GB）。
    let margin = BLUR_RADIUS * BLUR_SCALE_MAX + 4.0;
    let mut output = image::RgbaImage::new(out_w.max(1), out_h.max(1));
    let tiles = geometry::plan_tiles(
        &full_matrix,
        out_w.max(1),
        out_h.max(1),
        source_w,
        source_h,
        fdxmp_core::gpu_pipeline::EXPORT_TILE_EDGE,
        margin,
    );
    for plan in tiles {
        let (x, y, tile_w, tile_h) = (plan.x, plan.y, plan.width, plan.height);
        let [sx0, sy0, sub_w, sub_h] = plan.source;
        // 该分块的线性 f32 源（按需生成，避免整图 720MB 副本）；
        // 有镜头配置文件时在 f32 线性域做校正（与预览同一模型，少一次 8bit 往返）。
        let sub = decoded.linear_region(sx0, sy0, sub_w, sub_h);
        let sub = match profile {
            Some(profile) => profile.apply_f32(&sub),
            None => sub,
        };
        let mut image = pipeline
            .upload_linear_f32(
                &context.device,
                &context.queue,
                sub.as_raw(),
                sub.width(),
                sub.height(),
            )
            .map_err(|err| err.to_string())?;
        image.set_mask_reference(proxy.clone(), mask_scale, [sx0 as f32, sy0 as f32]);

        let matrix =
            geometry::tile_matrix(&full_matrix, [x as f32, y as f32], [sx0 as f32, sy0 as f32]);
        pipeline.render_tile(
            &context.device,
            &context.queue,
            &image,
            params,
            &RenderPlan {
                matrix,
                width: tile_w,
                height: tile_h,
                geometry_active: true,
                tile_origin: [x as f32, y as f32],
                full_size: [out_w as f32, out_h as f32],
            },
        );
        let pixels = pipeline
            .readback(&context.device, &context.queue, &image)
            .map_err(|err| err.to_string())?;
        let tile = image::RgbaImage::from_raw(tile_w, tile_h, pixels)
            .ok_or_else(|| "渲染结果尺寸不匹配".to_string())?;
        image::imageops::replace(&mut output, &tile, x as i64, y as i64);
    }
    Ok(output)
}

#[cfg(not(feature = "gpu"))]
fn render_full_resolution(
    _path: &std::path::Path,
    _params: &EditParams,
    _profile: Option<&fdxmp_core::lens_db::LensCorrection>,
) -> Result<image::RgbaImage, String> {
    Err("全分辨率导出需要启用 gpu feature".to_string())
}

fn grading_zone_mut(params: &mut EditParams, index: usize) -> &mut fdxmp_core::params::GradingZone {
    match index {
        0 => &mut params.grading.shadows,
        1 => &mut params.grading.midtones,
        _ => &mut params.grading.highlights,
    }
}

/// 裁剪矩形（归一化）→ 屏幕矩形（裁剪模式叠加层与手柄交互用）。
fn crop_screen_rect(crop: &fdxmp_core::params::CropParams, target: egui::Rect) -> egui::Rect {
    let rect = crop.rect;
    egui::Rect::from_min_max(
        target.min + egui::vec2(rect[0] * target.width(), rect[1] * target.height()),
        target.min
            + egui::vec2(
                (rect[0] + rect[2]) * target.width(),
                (rect[1] + rect[3]) * target.height(),
            ),
    )
}

/// 在资源管理器中定位文件（Windows）。
#[cfg(target_os = "windows")]
fn open_in_explorer(path: &std::path::Path) {
    let _ = std::process::Command::new("explorer")
        .arg("/select,")
        .arg(path)
        .spawn();
}

/// 其他平台暂不支持「在文件管理器中显示」。
#[cfg(not(target_os = "windows"))]
fn open_in_explorer(path: &std::path::Path) {
    let _ = path;
}

/// 曲线插值（与 core 的 LUT 构建一致：线性、严格过控制点）。
fn value_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
) -> Option<f32> {
    let mut current = value;
    let response = ui.add(
        egui::Slider::new(&mut current, range)
            .text(label)
            .fixed_decimals(2),
    );
    response.changed().then_some(current)
}

fn eval_curve_points(points: &[CurvePoint], x: f32) -> f32 {
    // 与渲染一致：单调三次 Hermite（控制点处不再折线）
    fdxmp_core::params::eval_curve(points, x)
}

fn nearest_curve_point(
    points: &[CurvePoint],
    pointer: egui::Pos2,
    rect: egui::Rect,
    max_distance: f32,
) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (index, point) in points.iter().enumerate() {
        let pos = egui::pos2(
            rect.left() + point.x * rect.width(),
            rect.bottom() - point.y * rect.height(),
        );
        let distance = pos.distance(pointer);
        if distance <= max_distance && best.map(|(_, d)| distance < d).unwrap_or(true) {
            best = Some((index, distance));
        }
    }
    best.map(|(index, _)| index)
}

/// 可交互曲线编辑器：拖动控制点，双击空白加点，右键删点（端点 x 固定）。
fn curve_editor(ui: &mut egui::Ui, points: &mut Vec<CurvePoint>) -> bool {
    let width = ui.available_width().clamp(160.0, 300.0);
    let height = width * 0.68;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        rect,
        egui::CornerRadius::from(3u8),
        egui::Color32::from_gray(0x12),
    );

    for step in 1..4 {
        let t = step as f32 / 4.0;
        let stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(0x2e));
        painter.line_segment(
            [
                egui::pos2(rect.left() + t * rect.width(), rect.top()),
                egui::pos2(rect.left() + t * rect.width(), rect.bottom()),
            ],
            stroke,
        );
        painter.line_segment(
            [
                egui::pos2(rect.left(), rect.top() + t * rect.height()),
                egui::pos2(rect.right(), rect.top() + t * rect.height()),
            ],
            stroke,
        );
    }
    painter.line_segment(
        [
            egui::pos2(rect.left(), rect.bottom()),
            egui::pos2(rect.right(), rect.top()),
        ],
        egui::Stroke::new(1.0, egui::Color32::from_gray(0x26)),
    );

    let to_screen = |point: &CurvePoint| {
        egui::pos2(
            rect.left() + point.x * rect.width(),
            rect.bottom() - point.y * rect.height(),
        )
    };
    let to_param = |pos: egui::Pos2| CurvePoint {
        x: ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
        y: ((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0),
    };

    let curve: Vec<egui::Pos2> = (0..=64)
        .map(|index| {
            let x = index as f32 / 64.0;
            let y = eval_curve_points(points, x);
            egui::pos2(
                rect.left() + x * rect.width(),
                rect.bottom() - y * rect.height(),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        curve,
        egui::Stroke::new(1.6, egui::Color32::from_rgb(0x8A, 0xC8, 0xFF)),
    ));

    let mut changed = false;
    let drag_id = response.id.with("curve-drag");
    let mut dragging: Option<usize> = ui.data(|data| data.get_temp(drag_id));

    if response.drag_started() {
        if let Some(pointer) = response.interact_pointer_pos() {
            dragging = nearest_curve_point(points, pointer, rect, 16.0);
            ui.data_mut(|data| data.insert_temp(drag_id, dragging));
        }
    }
    if response.dragged() {
        if let (Some(index), Some(pointer)) = (dragging, response.interact_pointer_pos()) {
            if index < points.len() {
                let mut point = to_param(pointer);
                let lower = if index == 0 {
                    0.0
                } else {
                    points[index - 1].x + 0.005
                };
                let upper = if index + 1 == points.len() {
                    1.0
                } else {
                    points[index + 1].x - 0.005
                };
                point.x = if lower <= upper {
                    point.x.clamp(lower, upper)
                } else {
                    points[index].x
                };
                points[index] = point;
                changed = true;
            }
        }
    }
    if response.drag_stopped() {
        ui.data_mut(|data| data.remove::<Option<usize>>(drag_id));
    }
    if response.double_clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            points.push(to_param(pointer));
            points.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
            changed = true;
        }
    }
    if response.secondary_clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            if let Some(index) = nearest_curve_point(points, pointer, rect, 16.0) {
                if index != 0 && index + 1 != points.len() {
                    points.remove(index);
                    changed = true;
                }
            }
        }
    }

    for point in points.iter() {
        painter.circle_filled(
            to_screen(point),
            4.0,
            egui::Color32::from_rgb(0xF0, 0xF0, 0xF0),
        );
    }
    ui.weak("拖动控制点 · 双击加点 · 右键删点");
    changed
}

/// RGBA8 → egui 纹理（egui 由 wgpu 后端上传到 GPU）。
fn texture_from(ctx: &egui::Context, rgba: &image::RgbaImage, name: &str) -> egui::TextureHandle {
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    );
    ctx.load_texture(name.to_string(), color_image, egui::TextureOptions::LINEAR)
}

/// 缩略图：优先读磁盘缓存，未命中则解码生成并写回缓存。
fn load_or_make_thumbnail(
    path: &Path,
    cache_root: &Path,
    max_edge: u32,
) -> Result<image::RgbaImage, String> {
    let cache = CacheManager::new(cache_root);
    let cache_path = cache.thumbnail_path(path);
    if let Ok(bytes) = std::fs::read(&cache_path) {
        if let Ok(image) = image::load_from_memory(&bytes) {
            return Ok(image.to_rgba8());
        }
    }

    let decoded = fdxmp_core::decode::decode_preview(path).map_err(|err| err.to_string())?;
    let thumbnail = decoded.display_proxy(max_edge);
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = thumbnail.save(&cache_path);
    Ok(thumbnail)
}
