import Combine
import ImageIO
import Photos
import UIKit

/// 编辑工具（底部工具栏）。
enum EditorTool: String, CaseIterable, Identifiable {
    case presets = "预设"
    case crop = "裁切"
    case edit = "编辑"
    case mask = "蒙版"
    case retouch = "修图"

    var id: String { rawValue }

    var icon: String {
        switch self {
        case .presets: return "wand.and.stars"
        case .crop: return "crop"
        case .edit: return "slider.horizontal.3"
        case .mask: return "paintbrush.pointed"
        case .retouch: return "wand.and.rays"
        }
    }
}

/// 颜色分类的子页。
enum ColorTab: String, CaseIterable, Identifiable {
    case base = "颜色"
    case hsl = "HSL"
    case equalizer = "均衡器"
    case grading = "分级"
    case pointColor = "点颜色"
    case calibration = "校准"
    case blackWhite = "黑白"

    var id: String { rawValue }
}

/// 渲染尺寸上限（非 actor 隔离，可在后台渲染闭包中安全引用）。
enum EditorLimits {
    /// 预览最长边（越大越清晰、渲染越慢）。
    ///
    /// 2048 覆盖 3x 屏竖图铺满时的像素长边（约 1560–2048px），
    /// 1x 显示时不会再因为预览只有 1440 而被放大。
    static let previewMaxEdge = 2048
    /// 蒙版拖动 / 涂抹时的快速预览最长边（低分辨率缓冲，手感更跟手）。
    static let fastPreviewMaxEdge = 720
    /// 蒙版叠加层最长边（与预览一致：共用同一份蒙版栅格缓存，避免重复计算）。
    static let maskOverlayMaxEdge = 2048
    /// 蒙版列表缩略图最长边。
    static let maskThumbnailMaxEdge = 128
    /// 导出按**原图分辨率**渲染（不再限制最长边）。
    /// 导出像素预算：超过就按比例降采样。CPU 管线是 f32 多平面（约 40 字节/像素），
    /// N MP 的峰值约 N × 40 MB；8MP ≈ 320MB，是 iPhone 上比较安全的上限。
    static let exportPixelLimit = 8_000_000
    /// 导出格式 / 质量 / 保留元数据 / 导出后分享（与「我的」页 @AppStorage 共用）。
    static let exportFormatKey = "fdxmp_export_format"
    static let exportQualityKey = "fdxmp_export_quality"
    static let exportKeepMetadataKey = "fdxmp_export_keep_metadata"
    static let exportShareKey = "fdxmp_export_share"

    /// 当前导出格式。
    static var exportFormat: EditorModel.ExportFormat {
        let stored = UserDefaults.standard.string(forKey: exportFormatKey) ?? ""
        return EditorModel.ExportFormat(rawValue: stored) ?? .jpeg
    }

    /// JPEG 质量（0.5..1，其它格式忽略）。
    static var exportQuality: Double {
        let stored = UserDefaults.standard.double(forKey: exportQualityKey)
        return stored > 0 ? stored : 1.0
    }

    /// 是否保留原图 EXIF / GPS 等拍摄信息。
    static var exportKeepMetadata: Bool {
        UserDefaults.standard.object(forKey: exportKeepMetadataKey) as? Bool ?? true
    }

    /// 导出完成后是否弹出分享面板。
    static var exportShareAfterSave: Bool {
        UserDefaults.standard.object(forKey: exportShareKey) as? Bool ?? false
    }
    /// 放大查看（原像素细节）用的最长边。
    static let detailMaxEdge = 4096
    /// 全分辨率细节源的最长边上限（PhotoKit 原图 / RAW 解调后的像素预算）。
    static let detailSourceMaxEdge = 8192
    /// 全分辨率细节源的像素上限（RGBA8：32MP ≈ 128MB）。
    static let detailSourcePixelLimit = 32_000_000
    /// 单次区域渲染的像素上限（含外扩余量；核心 CPU 管线约 40 字节/像素）。
    static let detailRegionPixelLimit = 6_000_000
    /// RAW 全尺寸解调的像素上限（超过则用半尺寸，避免 48MP ProRAW 内存尖峰）。
    static let rawFullSizePixelLimit = 24_000_000
}

/// 预览一帧实际走的渲染路径（顶栏标签用，方便判断 GPU 有没有参与）。
///
/// - `gpu`：零拷贝上屏（`fdxmp_gpu_preview_present`），不读回像素，耗时最短；
/// - `gpuReadback`：GPU 渲染后读回像素（对比模式 / 未挂上图层等），耗时含回读 + 转 UIImage；
/// - `cpu`：Rust CPU 渲染 + 转 UIImage（GPU 会话创建失败或本帧不适合 GPU）。
enum RenderPath: String {
    case cpu = "CPU"
    case gpu = "GPU"
    case gpuReadback = "GPU→CPU"
}

/// 编辑界面状态机：照片加载 → 防抖渲染 → 直方图 / 撤销重做 / 导出。
///
/// 渲染走 Rust CPU 预览（`fdxmp_render_memory_rgba`）：
/// 滑块 / 曲线 / 蒙版变化 140ms 防抖，渲染期间只保留最后一次请求。
/// 一步历史：操作名 + 该操作前的完整状态（用于撤销提示与长按历史列表）。
struct HistoryEntry {
    let name: String
    let state: EditState
}

@MainActor
final class EditorModel: ObservableObject {
    // MARK: 渲染与照片

    @Published private(set) var previewImage: UIImage?
    /// 最近一帧预览的像素缓冲（取色器用；零拷贝 GPU 上屏时不更新）。
    @Published private(set) var previewPixels: RgbaBuffer?
    @Published private(set) var originalImage: UIImage?
    @Published var showOriginal = false
    @Published private(set) var histogram: HistogramData?
    /// 放大查看时的高分辨率帧（仅缩放 > 1 时显示）。
    @Published private(set) var detailImage: UIImage?
    @Published private(set) var isLoading = false
    /// 首帧加载期间是否在恢复历史调色（决定加载弹框的文案）。
    @Published private(set) var isRestoringEdits = false
    @Published private(set) var isRendering = false
    /// 加载框：从 `load` 开始显示，直到**第一帧带当前调色**的画面真正上屏才收起。
    ///
    /// 之前原图一就位就把加载框关了，而调色首帧要等渲染完才出来，
    /// 于是"点进去先看到没调色的图，过一会才跳成调色结果"。
    private var awaitingFirstFrame = false
    /// 这张图有没有"已保存的调色 / 自动基础调色"要等。
    ///
    /// 没有调色的图原图就是最终效果，解码完就能收加载框（不然每张图都要多等一次渲染，
    /// 相册式左右滑动会变慢）；有调色才必须等带调色的首帧。
    private var awaitingEditedFrame = false
    /// 加载框出现时刻（首帧太快时补足最短显示时间，避免闪一下）。
    private var loadingStartedAt: CFAbsoluteTime = 0
    /// 首帧看门狗（坏文件 / 渲染异常时不能让加载框永远盖住画面）。
    private var firstFrameWatchdog: DispatchWorkItem?
    /// 加载框最短显示时间（秒）。
    private static let loadingMinDuration: CFAbsoluteTime = 0.3
    /// 首帧看门狗超时（秒）。
    private static let loadingTimeout: TimeInterval = 30
    @Published private(set) var isExporting = false
    /// 导出进度（0..1；nil = 未在导出）。
    @Published private(set) var exportProgress: Double?
    /// 导出完成后的分享文件（开了"导出后分享"才有值）。
    @Published var exportShareURL: URL?
    @Published private(set) var canUndo = false
    @Published private(set) var canRedo = false
    @Published private(set) var renderMillis: Double?
    /// 上一帧走的渲染路径（顶栏标签：GPU / GPU→CPU / CPU）。
    @Published private(set) var renderPath: RenderPath = .cpu
    @Published private(set) var message: String?

    // MARK: 编辑状态

    /// 完整编辑参数（滑块 / 曲线 / HSL / 分级 / 裁切 / 蒙版）。
    @Published var state = EditState()
    @Published private(set) var tool: EditorTool = .edit
    @Published private(set) var presetName = "原片"
    /// 当前应用的预设引用（系统预设 `b:名称` / 用户预设 `u:id`；手动调整后清空）。
    @Published private(set) var presetRef: String?
    /// 预设强度 0..100（应用预设后可调；0 = 应用前的样子）。
    @Published private(set) var presetStrength: Float = 100
    /// 用户预设文件夹（父，按创建顺序）。
    @Published private(set) var presetFolders: [PresetFolder] = []
    /// 用户预设（子，按创建顺序）。
    @Published private(set) var userPresets: [UserPreset] = []

    // 面板导航
    @Published var category: EditCategory = .light
    @Published var colorTab: ColorTab = .base
    @Published var effectTab: EffectTab = .effects
    @Published var detailTab: DetailTab = .sharpen
    @Published var curveChannel: CurveChannel = .rgb
    @Published var gradingZone: GradingZoneKind = .highlights
    @Published var hslBand: HslBandKind = .red
    /// 黑白混色面板当前选中的色带。
    @Published var bwBand: HslBandKind = .red
    /// 点颜色：当前选中的样本下标。
    @Published var pointColorIndex: Int = 0
    /// 点颜色：取色模式（画布点按取样）。
    @Published var pointSampling = false
    @Published var selectedMaskId: String?
    /// 是否叠加显示当前蒙版（红色半透明覆盖）。
    @Published var maskOverlayVisible = true
    /// 画笔擦除模式（橡皮擦）。
    @Published var brushErase = false
    /// 当前蒙版叠加层（id → 红色半透明预乘 RGBA 图像）。
    @Published private(set) var maskOverlays: [String: UIImage] = [:]
    /// 蒙版列表缩略图（id → 图像）。
    @Published private(set) var maskThumbnails: [String: UIImage] = [:]
    /// 画笔光标（归一化坐标；手指在画布上时非空）。
    @Published var brushCursor: CGPoint?
    /// 长按分类导航时临时归零预览的分类（松手恢复；不写历史、不落盘）。
    @Published private(set) var previewCategory: EditCategory?
    /// 长按蒙版调整页分类：临时把该蒙版这一类的局部参数归零预览。
    @Published private(set) var previewMaskCategory: EditCategory?
    /// 正在横向拖动的滑块标签（Lightroom 式沉浸：拖动时只留这一个滑块）。
    @Published private(set) var draggingSlider: String?
    /// 拖动滑块的实时数值（显示为画面顶部的大号数值浮层）。
    @Published private(set) var draggingSliderText: String?
    /// 对比模式的「调色前」整图（保留当前裁切、全部调整归零，与预览同尺寸）。
    @Published private(set) var beforeImage: UIImage?
    /// 正在准备对比帧。
    @Published private(set) var isPreparingBefore = false
    /// 预设缩略图（键 = 预设引用 `b:` / `u:`，用当前照片的小图渲染）。
    @Published private(set) var presetThumbnails: [String: UIImage] = [:]

    private(set) var asset: PHAsset?

    private var baseBuffer: RgbaBuffer?
    /// 当前 RAW 照片的原始文件字节（LibRaw 解码源；细节帧 / 导出复用）。
    private(set) var rawData: Data?
    /// 当前 RAW 帧的缓存键（nil = 非 RAW / 无缓存）。
    private var rawCacheKey: String?
    /// RAW 细节缓冲（LibRaw 解一版，供整帧细节帧复用；nil = 未就绪）。
    private var rawDetailBuffer: RgbaBuffer?
    /// 全分辨率细节源：放大后的区域渲染 / 整帧细节帧都从这里取像素。
    ///
    /// 非 RAW 走 PhotoKit 原图，RAW 走 LibRaw；只在第一次需要时准备，
    /// 与低分辨率的 `baseBuffer` 分开——后者只负责 1x 预览和交互。
    private var detailSource: RgbaBuffer?
    private var detailSourceInFlight = false
    private var detailSourceFailed = false
    private var detailSourceToken = 0
    /// 细节源就绪前挂起的区域请求（源到位后补发，避免第一次放大落空）。
    private var pendingDetailRegion: (visible: CGRect, viewport: CGSize)?
    /// 放大状态：暂停零拷贝上屏，改走回读，保证 SwiftUI 侧拿到的是新帧。
    private var detailZoomActive = false
    /// GPU 预览会话（nil = GPU 不可用 / 未就绪，预览自动走 CPU）。
    private var gpuSession: GpuPreviewSession?
    /// GPU 会话代际（换图 / 重建后旧回调作废）。
    private var gpuSessionToken = 0
    /// 零拷贝上屏：预览图层指针（由 MetalPreviewView 提供；nil = 无图层）。
    private var previewLayerPointer: UnsafeMutableRawPointer?
    /// 当前图层的实例 token（用来忽略旧图层销毁回调）。
    private var previewLayerToken: UUID?
    /// 零拷贝上屏是否生效（视图据此显示 Metal 图层或 UIImage）。
    @Published private(set) var zeroCopyActive = false
    /// 最近一帧的输出尺寸（零拷贝时 previewImage 可能是旧帧，布局几何以它为准）。
    @Published private(set) var previewOutputSize: CGSize?
    /// 视图侧要求暂停零拷贝上屏（对比模式等；由视图在切换时设置）。
    private var comparePreviewActive = false
    /// 拖动蒙版控件 / 涂抹时的低分辨率缓冲（长边 720，预览更跟手）。
    private var fastBuffer: RgbaBuffer?
    /// 蒙版交互进行中（用快速缓冲渲染，结束时回落到全分辨率）。
    private(set) var maskInteracting = false
    private var loadToken = 0
    private var detailToken = 0
    private var beforeToken = 0
    /// 放大细节帧：上次请求的视口 / 缩放与所用分辨率（用于判断是否需要重取）。
    private var lastDetailViewport = CGSize(width: 1200, height: 900)
    private var lastDetailZoom: CGFloat = 2.5
    private var detailEdgeUsed = 0
    private var detailInFlight = false
    private var dragSnapshot: EditState?
    private var undoStack: [HistoryEntry] = []
    private var redoStack: [HistoryEntry] = []
    /// 当前编辑的操作名（记入历史时用；不设则记为"编辑"）。
    private var pendingLabel: String?
    private var renderBusy = false
    private var renderPending = false
    private let renderQueue = DispatchQueue(label: "fdxmp.ios.render", qos: .userInitiated)
    /// 细节源 / 区域帧独立队列：不阻塞预览渲染（放大时滑块仍然跟手）。
    private let detailQueue = DispatchQueue(label: "fdxmp.ios.detail", qos: .userInitiated)
    private let maskAssetQueue = DispatchQueue(label: "fdxmp.ios.mask-assets", qos: .userInitiated)
    private var maskAssetToken = 0
    private var maskAssetScheduled = false
    /// 进入曲线前的分类（曲线面板「完成」时恢复）。
    private var categoryBeforeCurves: EditCategory = .light
    /// 预设强度的混合快照（应用预设前 / 预设结果）。
    private var presetBase: EditState?
    private var presetTarget: EditState?
    /// 正在拖预设强度滑块（此时不要清掉混合上下文）。
    private var suppressPresetStrengthReset = false
    private static let imageManager = PHCachingImageManager()

    init() {
        let library = PresetLibraryCoding.load()
        presetFolders = library.folders
        userPresets = library.presets
    }

    // MARK: - 只读派生值

    var assetName: String {
        asset?.displayName ?? "照片"
    }

    /// 长按预览时展示原图。
    var displayImage: UIImage? {
        showOriginal ? (originalImage ?? previewImage) : previewImage
    }

    /// 当前选中的蒙版。
    var selectedMask: MaskUi? {
        guard let selectedMaskId else { return nil }
        return state.masks.first { $0.id == selectedMaskId }
    }

    // MARK: - 生命周期

    /// 载入照片（同一张照片重复调用会直接返回）。
    func load(asset: PHAsset) {
        if self.asset?.localIdentifier == asset.localIdentifier {
            return
        }
        self.asset = asset
        previewImage = nil
        previewPixels = nil
        originalImage = nil
        histogram = nil
        baseBuffer = nil
        rawData = nil
        rawDetailBuffer = nil
        detailSource = nil
        detailSourceInFlight = false
        detailSourceFailed = false
        detailSourceToken &+= 1
        pendingDetailRegion = nil
        detailZoomActive = false
        fastBuffer = nil
        clearGpuSession()
        previewOutputSize = nil
        maskOverlays = [:]
        maskThumbnails = [:]
        smartMaskGenerating.removeAll()
        smartMaskFailed.removeAll()
        resetRetouchState()
        // 智能蒙版栅格与图像内容绑定：切换图片时清空核心缓存（重新生成 / 重新提交）。
        NativeCore.shared.clearMaskRasters()
        maskInteracting = false
        brushCursor = nil
        brushErase = false
        maskOverlayVisible = true
        beforeImage = nil
        isPreparingBefore = false
        detailRegion = nil
        detailRegionKey = ""
        lastRegionVisible = .zero
        pendingRegion = nil
        detailToken &+= 1
        detailImage = nil
        detailInFlight = false
        detailEdgeUsed = 0
        undoStack.removeAll()
        redoStack.removeAll()
        publishHistory()
        renderMillis = nil
        renderPath = .cpu
        message = nil
        if let json = EditStore.json(assetId: asset.localIdentifier) {
            state = EditJSON.parse(json)
            presetName = "自定义"
            presetRef = nil
            isRestoringEdits = true
        } else {
            state = EditState()
            presetName = "原片"
            presetRef = EditorSpec.presets.first?.ref
            isRestoringEdits = false
        }
        selectedMaskId = state.masks.first?.id
        isLoading = true
        // 有已保存的调色才需要等"带调色的首帧"（自动基础调色会在 finishLoading 里补上）。
        awaitingEditedFrame = isRestoringEdits
        loadToken &+= 1
        let token = loadToken
        beginAwaitingFirstFrame(token: token)
        let maxEdge = EditorLimits.previewMaxEdge
        let isRaw = PhotoRaw.isRawAsset(asset)
        rawCacheKey = isRaw
            ? RawFrameCache.key(
                assetId: asset.localIdentifier,
                maxEdge: maxEdge,
                version: asset.modificationDate?.timeIntervalSince1970
            )
            : nil

        // 0) 命中上次解好的 RAW 帧：退出编辑页再进来不再重新下载 / 解调
        //    （原来这条链路要十几秒，命中后只剩一次首帧渲染）。
        if let key = rawCacheKey, let cached = RawFrameCache.shared.entry(for: key) {
            rawData = cached.data
            baseBuffer = cached.base
            rawDetailBuffer = cached.detail
            let buffer = cached.base
            renderQueue.async { [weak self] in
                let original = ImageUtil.image(from: buffer)
                Task { @MainActor in
                    guard let self, self.loadToken == token else { return }
                    self.finishLoading(buffer: buffer, original: original, token: token)
                }
            }
            return
        }

        // 1) 快速首图：RAW / 大图先出一张低质量图垫在加载框后面。
        //    注意：这里不能关加载框——调色首帧还没出来，关了就会先看到没调色的图。
        requestImage(asset: asset, maxEdge: maxEdge, mode: .fastFormat) { [weak self] image, _ in
            guard let self, self.loadToken == token, self.baseBuffer == nil else { return }
            guard let image else { return }
            self.previewImage = image
        }

        // 2) 正式图：RAW（ProRAW / DNG）走 LibRaw 解调，其余交给系统解码后转像素缓冲。
        if isRaw {
            loadRawImage(asset: asset, token: token, maxEdge: maxEdge)
        } else {
            loadSystemImage(asset: asset, token: token, maxEdge: maxEdge)
        }
    }

    /// RAW 正式图：取原始文件字节 → LibRaw 半尺寸解调 → RGBA8；失败回退系统解码。
    private func loadRawImage(asset: PHAsset, token: Int, maxEdge: Int) {
        PhotoRaw.requestData(for: asset) { [weak self] data in
            Task { @MainActor in
                guard let self, self.loadToken == token else { return }
                guard let data, !data.isEmpty else {
                    // 取不到原始数据（如 iCloud 拉取失败 / 资源缺失）：回退系统渲染。
                    self.loadSystemImage(asset: asset, token: token, maxEdge: maxEdge)
                    return
                }
                self.rawData = data
                self.renderQueue.async { [weak self] in
                    let buffer = NativeCore.shared.decodeRaw(
                        data: data,
                        maxEdge: maxEdge,
                        fullSize: false
                    )
                    let original = buffer.flatMap { ImageUtil.image(from: $0) }
                    Task { @MainActor in
                        guard let self, self.loadToken == token else { return }
                        guard buffer != nil else {
                            // LibRaw 解不了（罕见格式等）：回退系统渲染并释放原始字节。
                            self.rawData = nil
                            self.loadSystemImage(asset: asset, token: token, maxEdge: maxEdge)
                            return
                        }
                        self.finishLoading(buffer: buffer, original: original, token: token)
                        // 写进 RAW 帧缓存：退出编辑页再进来直接复用，不再重新取字节 / 解调。
                        // 细节缓冲不在这里解：那是一次额外的解调，白占几秒 CPU 和一二百 MB 内存，
                        // 等用户第一次放大（`requestDetailPreview`）时再解。
                        if let buffer, let key = self.rawCacheKey {
                            RawFrameCache.shared.store(
                                RawFrameEntry(data: data, base: buffer),
                                for: key
                            )
                        }
                    }
                }
            }
        }
    }

    /// 系统解码正式图（JPEG / HEIC / 非 RAW，也是 RAW 不可用时的回退）。
    private func loadSystemImage(asset: PHAsset, token: Int, maxEdge: Int) {
        requestImage(
            asset: asset,
            maxEdge: maxEdge,
            mode: .highQualityFormat
        ) { [weak self] image, degraded in
            guard let self, self.loadToken == token else { return }
            if degraded {
                return
            }
            guard let image else {
                self.message = "无法读取这张照片"
                self.finishFirstFrameFailed()
                return
            }
            // 像素转换（方向校正 + 缩放 + 色彩空间）放到渲染线程，避免卡主线程。
            self.renderQueue.async { [weak self] in
                let buffer = ImageUtil.rgbaBuffer(from: image, maxEdge: maxEdge)
                let original = buffer.flatMap { ImageUtil.image(from: $0) }
                Task { @MainActor in
                    guard let self, self.loadToken == token else { return }
                    self.finishLoading(buffer: buffer, original: original, token: token)
                }
            }
        }
    }

    /// 载入完成（系统解码与 LibRaw 共用）：缓冲 / 原图 / 首帧渲染 / GPU 会话。
    ///
    /// 这里**不直接关加载框**：`previewImage` 先垫一版原图；
    /// 有调色时等 `scheduleRender` 把带调色的首帧渲染出来（`finishLoadingOverlay`）再收起，
    /// 避免先看到没调色的图。
    private func finishLoading(buffer: RgbaBuffer?, original: UIImage?, token: Int) {
        guard loadToken == token else { return }
        guard let buffer else {
            message = "无法读取这张照片的像素数据"
            finishFirstFrameFailed()
            return
        }
        baseBuffer = buffer
        fastBuffer = ImageUtil.downscaled(
            buffer: buffer,
            maxEdge: EditorLimits.fastPreviewMaxEdge
        )
        originalImage = original
        // 先显示原图占位，首帧渲染完成后替换为调色结果。
        previewImage = original
        // 项目内新照片：自动基础调色（不写编辑记录，用户改动后才落盘）。
        applyAutoBaseToneIfNeeded()
        scheduleRender(immediate: true)
        // 首帧先走 CPU 保证立刻出图；GPU 会话在后台创建，就绪后接管预览渲染。
        if let source = renderSource {
            createGpuSession(for: source)
        }
        // 没有调色要等（纯原图）：解码完成就收加载框，不再多等一次渲染。
        if !awaitingEditedFrame {
            finishLoadingOverlay()
        }
        // 有保存过修图参数的照片：后台重算修图缓冲。
        prepareRetouchAfterLoad()
        // Vision 人像蒙版（MODNet 不随包）：后台算好提交给核心（身体塑形 / 人像路径用）。
        prepareVisionPersonMask()
        // 首帧之后预热原图细节源：第一次放大直接出 1:1 区域帧。
        preloadDetailSourceIfUseful()
    }

    // MARK: - 首帧加载框

    /// 开始等首帧：加载框亮起，直到**第一帧带当前调色**的画面真正上屏。
    private func beginAwaitingFirstFrame(token: Int) {
        awaitingFirstFrame = true
        loadingStartedAt = CFAbsoluteTimeGetCurrent()
        firstFrameWatchdog?.cancel()
        let item = DispatchWorkItem { [weak self] in
            guard let self, self.loadToken == token, self.awaitingFirstFrame else { return }
            // 弱网 / 坏文件 / 渲染异常：到点强制收起，不能让加载框永远盖住画面。
            self.awaitingFirstFrame = false
            self.clearLoadingIfNeeded()
        }
        firstFrameWatchdog = item
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.loadingTimeout, execute: item)
    }

    /// 收起加载框：带调色的首帧上屏时调用；首帧太快时补足最短显示时间，避免闪一下。
    private func finishLoadingOverlay() {
        guard awaitingFirstFrame else { return }
        awaitingFirstFrame = false
        firstFrameWatchdog?.cancel()
        firstFrameWatchdog = nil
        let token = loadToken
        let remaining = Self.loadingMinDuration - (CFAbsoluteTimeGetCurrent() - loadingStartedAt)
        guard remaining > 0.02 else {
            clearLoadingIfNeeded()
            return
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + remaining) { [weak self] in
            // 延迟期间又换了照片：让新的加载流程自己收尾。
            guard let self, self.loadToken == token, !self.awaitingFirstFrame else { return }
            self.clearLoadingIfNeeded()
        }
    }

    /// 首帧失败（取不到图 / 渲染失败）：收起加载框，露出原图，错误提示保留在 `message`。
    private func finishFirstFrameFailed() {
        awaitingFirstFrame = false
        firstFrameWatchdog?.cancel()
        firstFrameWatchdog = nil
        clearLoadingIfNeeded()
    }

    private func clearLoadingIfNeeded() {
        guard isLoading else { return }
        isLoading = false
        isRestoringEdits = false
    }

    /// 准备全分辨率细节源：放大后的区域渲染 / 整帧细节帧共用。
    ///
    /// - 非 RAW：PhotoKit 取原图（按 `detailSourcePixelLimit` 封顶），方向校正后转 RGBA8；
    /// - RAW：LibRaw 全尺寸解调（≤24MP；更大的仍走半尺寸，避免 48MP ProRAW 内存尖峰）。
    ///
    /// 只在第一次放大时准备，进编辑页不预解；非 RAW 会在首帧后低优先级预热，
    /// 这样第一次双指放大通常已经能直接出区域帧，而不是先看一版放大的预览。
    private func ensureDetailSource() {
        guard detailSource == nil, !detailSourceInFlight, !detailSourceFailed,
              let asset
        else {
            return
        }
        let pixelWidth = asset.pixelWidth
        let pixelHeight = asset.pixelHeight
        let longEdge = max(pixelWidth, pixelHeight)
        guard longEdge > 64 else { return }
        let pixels = Double(pixelWidth) * Double(pixelHeight)
        let budgetScale = min(
            1.0,
            (Double(EditorLimits.detailSourcePixelLimit) / max(pixels, 1)).squareRoot()
        )
        let maxEdge = min(
            longEdge,
            min(
                EditorLimits.detailSourceMaxEdge,
                max(64, Int((Double(longEdge) * budgetScale).rounded(.down)))
            )
        )
        detailSourceToken &+= 1
        let token = detailSourceToken
        detailSourceInFlight = true

        if let data = rawData {
            let fullSize = pixels > 0 && pixels <= Double(EditorLimits.rawFullSizePixelLimit)
            detailQueue.async { [weak self] in
                let buffer = NativeCore.shared.decodeRaw(
                    data: data,
                    maxEdge: maxEdge,
                    fullSize: fullSize
                )
                Task { @MainActor in
                    guard let self, self.detailSourceToken == token else { return }
                    guard let buffer else {
                        self.failDetailSource()
                        return
                    }
                    self.detailSourceInFlight = false
                    self.detailSource = buffer
                    self.rawDetailBuffer = buffer
                    self.didLoadDetailSource()
                }
            }
            return
        }

        requestFullImage(asset: asset, maxEdge: maxEdge) { [weak self] image in
            guard let self, self.detailSourceToken == token else { return }
            guard let image else {
                self.failDetailSource()
                return
            }
            self.detailQueue.async { [weak self] in
                let buffer = ImageUtil.rgbaBuffer(from: image, maxEdge: maxEdge)
                Task { @MainActor in
                    guard let self, self.detailSourceToken == token else { return }
                    guard let buffer else {
                        self.failDetailSource()
                        return
                    }
                    self.detailSourceInFlight = false
                    self.detailSource = buffer
                    self.didLoadDetailSource()
                }
            }
        }
    }

    /// 细节源失败：放出挂起的区域请求，退回整帧细节帧（不反复重试）。
    private func failDetailSource() {
        detailSourceInFlight = false
        detailSourceFailed = true
        if let pending = pendingDetailRegion {
            pendingDetailRegion = nil
            requestDetailPreview(viewport: pending.viewport, zoom: lastDetailZoom, force: true)
        } else if detailZoomActive, detailImage == nil, !detailInFlight {
            requestDetailPreview(viewport: lastDetailViewport, zoom: lastDetailZoom, force: true)
        }
    }

    /// 细节源就绪：补发挂起的区域请求；已放大时用新源重渲染整帧细节帧。
    private func didLoadDetailSource() {
        if let pending = pendingDetailRegion {
            pendingDetailRegion = nil
            performDetailRegionRequest(visible: pending.visible, viewport: pending.viewport)
        } else if detailRegion != nil, !lastRegionVisible.isEmpty {
            // 之前用低分辨率源渲染过区域帧：源升级后重渲染一次。
            performDetailRegionRequest(
                visible: lastRegionVisible,
                viewport: lastDetailViewport
            )
        }
        if detailImage != nil, !detailInFlight {
            requestDetailPreview(
                viewport: lastDetailViewport,
                zoom: lastDetailZoom,
                force: true
            )
        }
    }

    /// 首帧之后预热非 RAW 的原图细节源（≤24MP，避免 48MP 图在后台吃 200MB）。
    private func preloadDetailSourceIfUseful() {
        guard rawData == nil, let asset else { return }
        let pixels = Double(asset.pixelWidth) * Double(asset.pixelHeight)
        guard pixels > 0, pixels <= 24_000_000 else { return }
        let token = loadToken
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) { [weak self] in
            guard let self, self.loadToken == token, !self.detailZoomActive else { return }
            self.ensureDetailSource()
        }
    }

    /// 取原图尺寸的 UIImage（只回调一次非降级结果；失败回调 nil）。
    private func requestFullImage(
        asset: PHAsset,
        maxEdge: Int,
        completion: @escaping @MainActor (UIImage?) -> Void
    ) {
        let options = PHImageRequestOptions()
        options.deliveryMode = .highQualityFormat
        // `.fast` 保持原始比例；`.exact` 会按 targetSize 压成正方形导致比例失真。
        options.resizeMode = .fast
        options.isNetworkAccessAllowed = true
        options.version = .current
        let target = CGSize(width: maxEdge, height: maxEdge)
        Self.imageManager.requestImage(
            for: asset,
            targetSize: target,
            contentMode: .aspectFit,
            options: options
        ) { image, info in
            let degraded = (info?[PHImageResultIsDegradedKey] as? Bool) ?? false
            if degraded { return }
            Task { @MainActor in
                completion(image)
            }
        }
    }

    /// 丢弃当前 GPU 会话（换图 / 重置时调用；在用的会话由渲染闭包强引用，安全等到自然结束）。
    private func clearGpuSession() {
        gpuSessionToken &+= 1
        gpuSession = nil
        zeroCopyActive = false
    }

    /// 为预览缓冲在后台创建 GPU 会话；失败（模拟器 / 老设备 / 驱动不足）静默留在 CPU 路径。
    private func createGpuSession(for buffer: RgbaBuffer) {
        clearGpuSession()
        let token = gpuSessionToken
        renderQueue.async { [weak self] in
            let session = GpuPreviewSession(buffer: buffer)
            Task { @MainActor in
                guard let self, self.gpuSessionToken == token else { return }
                self.gpuSession = session
                if session != nil {
                    // 会话就绪：若预览图层在场就顺手挂上（零拷贝），然后重渲染一帧。
                    self.tryAttachPreviewLayer()
                }
            }
        }
    }

    /// MetalPreviewView 报告预览图层（pointer = 新图层，nil = 该实例销毁）。
    ///
    /// 注意：`nil` 分支来自 SwiftUI 的 `dismantleUIView`，此时正处于属性图失效阶段，
    /// 直接改 `@Published` 会触发 Swift 独占访问检查（`swift_beginAccess`）崩溃。
    /// 所以这里只改非 Published 的簿记字段，Published 的部分挪到下一个 runloop。
    func attachPreviewLayer(_ pointer: UnsafeMutableRawPointer?, token: UUID) {
        if let pointer {
            previewLayerToken = token
            previewLayerPointer = pointer
            tryAttachPreviewLayer()
        } else {
            guard previewLayerToken == token else { return }
            previewLayerToken = nil
            previewLayerPointer = nil
            DispatchQueue.main.async { [weak self] in
                guard let self, self.previewLayerPointer == nil else { return }
                if self.zeroCopyActive {
                    self.zeroCopyActive = false
                }
            }
        }
    }

    /// 把当前预览图层挂到 GPU 会话上（成功 = 打开零拷贝；失败 / 无图层 = 留在回读路径）。
    private func tryAttachPreviewLayer() {
        guard let session = gpuSession, let pointer = previewLayerPointer else { return }
        renderQueue.async { [weak self] in
            let attached = session.attachLayer(pointer)
            Task { @MainActor in
                guard let self, self.gpuSession === session else { return }
                self.zeroCopyActive = attached
                // 无论是否上屏，都重渲染一帧：零拷贝时出首帧，回读时刷新显示。
                self.scheduleRender(immediate: true)
            }
        }
    }

    /// 立即重渲染一帧预览（进入 / 退出对比模式等需要刷新回读画面时调用）。
    func refreshPreview() {
        scheduleRender(immediate: true)
    }

    /// 放大状态开关：放大后暂停零拷贝上屏，保证 `previewImage` 与画面同步刷新。
    func setDetailZoomActive(_ active: Bool) {
        guard detailZoomActive != active else { return }
        detailZoomActive = active
        // 避免在 SwiftUI 的 onChange 里同步改 `isRendering` 等 Published 状态。
        DispatchQueue.main.async { [weak self] in
            self?.refreshPreview()
        }
    }

    /// 对比模式开关（视图切换分屏对比时调用；暂停零拷贝上屏，改用回读画面）。
    func setComparePreviewActive(_ active: Bool) {
        comparePreviewActive = active
    }

    func clearMessage() {
        message = nil
    }

    // MARK: - 放大细节（原像素）

    /// 生成对比用的「调色前」帧：同裁切、无调整，尺寸与预览一致（左原图 / 右调色）。
    func prepareBeforeImage() {
        guard let buffer = baseBuffer else { return }
        beforeToken &+= 1
        let token = beforeToken
        isPreparingBefore = true
        var beforeState = EditState()
        beforeState.crop = state.crop
        let json = EditJSON.build(state: beforeState, applyCropRect: true)
        renderQueue.async { [weak self] in
            let result = NativeCore.shared.render(buffer: buffer, paramsJson: json)
            let image = result.flatMap {
                ImageUtil.image(
                    from: RgbaBuffer(pixels: $0.pixels, width: $0.width, height: $0.height)
                )
            }
            Task { @MainActor in
                guard let self, self.beforeToken == token else { return }
                self.isPreparingBefore = false
                self.beforeImage = image
            }
        }
    }

    // MARK: - 预设缩略图

    private var presetThumbToken = 0

    /// 用当前照片的小图渲染每个预设的效果（进入预设工具 / 预设库变化时刷新）。
    func refreshPresetThumbnails() {
        guard let buffer = renderSource else { return }
        presetThumbToken &+= 1
        let token = presetThumbToken
        let crop = state.crop
        let builtins = EditorSpec.presets
        let users = userPresets
        maskAssetQueue.async { [weak self] in
            let small = ImageUtil.downscaled(buffer: buffer, maxEdge: 200) ?? buffer
            var images: [String: UIImage] = [:]
            func render(_ ref: String, _ thumbState: EditState) {
                let json = EditJSON.build(state: thumbState, applyCropRect: true)
                guard
                    let result = NativeCore.shared.render(buffer: small, paramsJson: json),
                    let image = ImageUtil.image(
                        from: RgbaBuffer(
                            pixels: result.pixels,
                            width: result.width,
                            height: result.height
                        )
                    )
                else {
                    return
                }
                images[ref] = image
            }
            for preset in builtins {
                var thumbState = EditState()
                thumbState.crop = crop
                var values = EditorSpec.defaultValues()
                for (key, value) in preset.params {
                    values[key] = value
                }
                thumbState.values = values
                render(preset.ref, thumbState)
            }
            for preset in users {
                var thumbState = EditState()
                thumbState.crop = crop
                preset.apply(to: &thumbState)
                render(preset.ref, thumbState)
            }
            Task { @MainActor in
                guard let self, self.presetThumbToken == token else { return }
                self.presetThumbnails = images
            }
        }
    }

    /// 关闭对比模式：释放对比帧。
    func clearBeforeImage() {
        beforeToken &+= 1
        isPreparingBefore = false
        beforeImage = nil
    }

    /// 请求一张高分辨率帧：双指 / 双击放大时用，保证放大后看到的是原始像素而不是放大后的代理图。
    ///
    /// 分辨率按缩放级别动态取"刚好够用"（视口像素 × 缩放 × 1.15，1200..4096）：
    /// 不再一律 4096，内存峰值随缩放下降明显（4096² RGBA 约 64MB → 2.5x 时约 36MB）。
    func requestDetailPreview(
        viewport: CGSize = CGSize(width: 1200, height: 900),
        zoom: CGFloat = 2.5,
        force: Bool = false
    ) {
        lastDetailViewport = viewport
        lastDetailZoom = zoom
        guard let asset, !detailInFlight else { return }
        let needed = Int(max(viewport.width, viewport.height) * zoom * 1.15)
        let maxEdge = min(max(needed, 1200), EditorLimits.detailMaxEdge)
        // 已有一张分辨率相近的帧就不重复请求（避免每档缩放都重渲染）。
        if !force, detailImage != nil, abs(maxEdge - detailEdgeUsed) <= max(160, detailEdgeUsed / 8) {
            return
        }
        detailEdgeUsed = maxEdge
        detailInFlight = true
        detailToken &+= 1
        let token = detailToken
        let json = currentJson(applyCropRect: tool != .crop)
        let retouchParams = state.retouch

        // 手动污点 / 液化后的画面来自另一条像素链：细节帧也用它，避免和预览观感不一致。
        if !retouchParams.isActive, let edited = liquifyBuffer ?? healBuffer {
            finishDetailImage(source: edited, maxEdge: maxEdge, json: json, token: token)
            return
        }

        // 已经有全分辨率细节源：整帧细节帧也从这里渲染，不再向 PhotoKit 要另一版。
        if let source = detailSource {
            if !retouchParams.isActive {
                finishDetailImage(source: source, maxEdge: maxEdge, json: json, token: token)
                return
            }
            // RAW 的修图缓冲是预览分辨率：继续用修图结果，保证与预览观感一致。
            if rawData != nil {
                let prepared = retouchBuffer ?? rawDetailBuffer ?? source
                finishDetailImage(source: prepared, maxEdge: maxEdge, json: json, token: token)
                return
            }
        }

        // RAW：细节帧同源走 LibRaw 缓冲（避免混入系统渲染的观感差异）。
        if rawData != nil {
            if !retouchParams.isActive {
                ensureDetailSource()
            }
            guard let source = retouchBuffer ?? rawDetailBuffer ?? baseBuffer else {
                detailInFlight = false
                return
            }
            finishDetailImage(source: source, maxEdge: maxEdge, json: json, token: token)
            return
        }

        // 非 RAW：先让原图细节源在后台准备；当前这一帧仍按请求分辨率出图。
        if !retouchParams.isActive {
            ensureDetailSource()
        }
        requestImage(
            asset: asset,
            maxEdge: maxEdge,
            mode: .highQualityFormat
        ) { [weak self] image, degraded in
            guard let self, self.detailToken == token, !degraded else { return }
            guard let image else {
                self.detailInFlight = false
                return
            }
            self.detailQueue.async { [weak self] in
                let buffer = ImageUtil.rgbaBuffer(from: image, maxEdge: maxEdge)
                // 修图开启时：细节帧先在请求分辨率上算好，保证与预览一致。
                let prepared = buffer.flatMap { buffer -> RgbaBuffer? in
                    guard retouchParams.isActive else { return buffer }
                    return Self.applyRetouchForExport(
                        buffer: buffer,
                        params: retouchParams
                    ) ?? buffer
                }
                let result = prepared.flatMap {
                    NativeCore.shared.render(buffer: $0, paramsJson: json)
                }
                let output = result.flatMap {
                    ImageUtil.image(
                        from: RgbaBuffer(pixels: $0.pixels, width: $0.width, height: $0.height)
                    )
                }
                Task { @MainActor in
                    guard let self, self.detailToken == token else { return }
                    self.detailInFlight = false
                    self.detailImage = output
                }
            }
        }
    }

    /// 整帧细节帧：降采样到 `maxEdge` → CPU 渲染 → UIImage（区域渲染不可用时的兜底）。
    private func finishDetailImage(
        source: RgbaBuffer,
        maxEdge: Int,
        json: String,
        token: Int
    ) {
        detailQueue.async { [weak self] in
            var prepared = source
            if max(prepared.width, prepared.height) > maxEdge,
               let scaled = ImageUtil.downscaled(buffer: prepared, maxEdge: maxEdge) {
                prepared = scaled
            }
            let result = NativeCore.shared.render(buffer: prepared, paramsJson: json)
            let output = result.flatMap {
                ImageUtil.image(
                    from: RgbaBuffer(pixels: $0.pixels, width: $0.width, height: $0.height)
                )
            }
            Task { @MainActor in
                guard let self, self.detailToken == token else { return }
                self.detailInFlight = false
                self.detailImage = output
            }
        }
    }

    /// 缩回 1x 时释放高分辨率帧，省内存。
    func clearDetailPreview() {
        detailToken &+= 1
        detailInFlight = false
        detailImage = nil
        detailEdgeUsed = 0
        // 细节源没取到（iCloud 瞬时失败等）：缩回后再放大允许重试一次。
        if detailSource == nil {
            detailSourceFailed = false
        }
    }

    // MARK: - 放大细节（区域 1:1，核心"帧外坐标"渲染）

    /// 只覆盖 `rect`（归一化，输出坐标系）的高清帧。
    struct DetailRegion {
        let image: UIImage
        let rect: CGRect
    }

    /// 当前区域高清帧。
    @Published private(set) var detailRegion: DetailRegion?

    private var detailRegionToken = 0
    private var regionDebounceToken = 0
    private var pendingRegion: (visible: CGRect, viewport: CGSize)?
    private var lastRegionVisible: CGRect = .zero
    private var detailRegionKey = ""

    private static let unitCropRect = CGRect(x: 0, y: 0, width: 1, height: 1)

    /// 区域渲染使用的裁切矩形（裁切工具显示未裁切原图）。
    private var regionCropRect: CGRect {
        tool == .crop ? Self.unitCropRect : state.crop.rect
    }

    /// 是否可用区域渲染。
    ///
    /// 核心按「源像素 + 帧外坐标」渲染，裁切 / 翻转 / 90° 旋转由 Swift 侧做几何映射；
    /// 拉直、镜头校正、修图、内容相关蒙版仍回退整帧细节帧。
    var canRenderRegionDetail: Bool {
        // 修图 / 污点 / 液化后的画面来自另一条像素链：区域帧若从原图取会不一致。
        guard retouchBuffer == nil, liquifyBuffer == nil, healBuffer == nil else { return false }
        let crop = state.crop
        guard abs(crop.rotation) < 0.01 else { return false }
        // 光学（色差 / 配置文件校正）也算几何变换：核心区域管线要求单位镜头模型。
        guard !state.removeChromaticAberration, !state.profileCorrection else { return false }
        // 智能蒙版依赖参考图，核心的区域管线无法重建栅格。
        guard !state.masks.contains(where: { $0.visible && $0.kind.isSmart }) else { return false }
        let hasVisibleMasks = state.masks.contains { $0.visible }
        let nonIdentityGeometry = crop.flipHorizontal
            || crop.flipVertical
            || crop.quarterTurns % 4 != 0
            || regionCropRect != Self.unitCropRect
        if hasVisibleMasks, nonIdentityGeometry {
            // 蒙版在核心是「几何之后」按输出坐标合成的；非单位几何时区域帧在源坐标渲染，
            // 蒙版参数也要一起映射，当前先回退整帧（质量仍然来自全分辨率细节源）。
            return false
        }
        return true
    }

    /// 当前区域帧对应的几何 / 源尺寸（用于判断平移后能否直接复用）。
    private var regionGeometryKey: String {
        let crop = state.crop
        let rect = regionCropRect
        return "\(detailSource?.width ?? 0)x\(detailSource?.height ?? 0)"
            + "|\(rect.minX),\(rect.minY),\(rect.width),\(rect.height)"
            + "|\(crop.flipHorizontal)|\(crop.flipVertical)|\(crop.quarterTurns % 4)"
            + "|\(tool == .crop)"
    }

    /// 去抖地请求区域高清帧（缩放 / 平移时反复调用只保留最后一次）。
    func scheduleDetailRegion(visible: CGRect, viewport: CGSize, zoom: CGFloat) {
        lastDetailViewport = viewport
        lastDetailZoom = max(zoom, 1)
        pendingRegion = (visible, viewport)
        // 已有区域帧且仍覆盖可见范围（留 2% 余量）：小幅平移不重渲染。
        if let current = detailRegion, detailRegionKey == regionGeometryKey {
            let inner = current.rect.insetBy(
                dx: current.rect.width * 0.02,
                dy: current.rect.height * 0.02
            )
            if inner.contains(visible) {
                pendingRegion = nil
                return
            }
        }
        regionDebounceToken &+= 1
        let token = regionDebounceToken
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.16) { [weak self] in
            guard let self, self.regionDebounceToken == token,
                  let pending = self.pendingRegion
            else {
                return
            }
            self.pendingRegion = nil
            self.performDetailRegionRequest(visible: pending.visible, viewport: pending.viewport)
        }
    }

    /// 释放区域高清帧。
    func clearDetailRegion() {
        detailRegionToken &+= 1
        regionDebounceToken &+= 1
        pendingRegion = nil
        pendingDetailRegion = nil
        detailRegion = nil
        detailRegionKey = ""
        lastRegionVisible = .zero
    }

    private func performDetailRegionRequest(visible: CGRect, viewport: CGSize) {
        guard canRenderRegionDetail else { return }
        guard visible.width > 0.002, visible.height > 0.002 else { return }
        lastRegionVisible = visible
        lastDetailViewport = viewport

        guard let source = detailSource else {
            if detailSourceFailed {
                requestDetailPreview(viewport: viewport, zoom: lastDetailZoom, force: true)
            } else {
                pendingDetailRegion = (visible, viewport)
                ensureDetailSource()
            }
            return
        }
        guard source.width > 32, source.height > 32 else { return }

        let geometry = DetailRegionGeometry(
            crop: regionCropRect,
            flipHorizontal: state.crop.flipHorizontal,
            flipVertical: state.crop.flipVertical,
            quarterTurns: state.crop.quarterTurns
        )
        let sourceWidth = CGFloat(source.width)
        let sourceHeight = CGFloat(source.height)

        // 可渲染的整帧范围（裁切之后）：区域只在裁切内取样，避免把裁切外的像素
        // 贴到输出边缘；翻转 / 旋转由输出矩形映射处理。
        let allowed = geometry.sourceRect(fromOutput: Self.unitCropRect)
        let allowedX0 = max(0, min(source.width - 1, Int((allowed.minX * sourceWidth).rounded(.down))))
        let allowedY0 = max(0, min(source.height - 1, Int((allowed.minY * sourceHeight).rounded(.down))))
        let allowedX1 = max(allowedX0 + 1, min(source.width, Int((allowed.maxX * sourceWidth).rounded(.up))))
        let allowedY1 = max(allowedY0 + 1, min(source.height, Int((allowed.maxY * sourceHeight).rounded(.up))))

        let sourceVisible = geometry.sourceRect(fromOutput: visible)
        let vx0 = max(allowedX0, min(allowedX1 - 1, Int((sourceVisible.minX * sourceWidth).rounded(.down))))
        let vy0 = max(allowedY0, min(allowedY1 - 1, Int((sourceVisible.minY * sourceHeight).rounded(.down))))
        let vx1 = min(allowedX1, max(vx0 + 1, Int((sourceVisible.maxX * sourceWidth).rounded(.up))))
        let vy1 = min(allowedY1, max(vy0 + 1, Int((sourceVisible.maxY * sourceHeight).rounded(.up))))
        let visibleWidth = max(1, vx1 - vx0)
        let visibleHeight = max(1, vy1 - vy0)

        func expandedRegion(margin: Int) -> (x: Int, y: Int, width: Int, height: Int) {
            let x = max(allowedX0, vx0 - margin)
            let y = max(allowedY0, vy0 - margin)
            let right = min(allowedX1, vx1 + margin)
            let bottom = min(allowedY1, vy1 + margin)
            return (x, y, max(1, right - x), max(1, bottom - y))
        }

        // 区域向外扩一点余量：平移几像素时可直接复用，不必每帧都重新渲染。
        var margin = max(24, Int(Double(max(visibleWidth, visibleHeight)) * 0.18))
        var region = expandedRegion(margin: margin)
        let pixelLimit = EditorLimits.detailRegionPixelLimit
        while region.width * region.height > pixelLimit, margin > 0 {
            margin /= 2
            region = expandedRegion(margin: margin)
        }
        guard region.width * region.height <= pixelLimit, region.width > 8, region.height > 8
        else {
            // 可见范围本身就超过区域预算：退回整帧细节帧。
            detailRegion = nil
            detailRegionKey = ""
            requestDetailPreview(viewport: viewport, zoom: lastDetailZoom, force: true)
            return
        }

        // 输出坐标：源区域 → 翻转 / 旋转 → 裁切。
        let regionSource = CGRect(
            x: CGFloat(region.x) / sourceWidth,
            y: CGFloat(region.y) / sourceHeight,
            width: CGFloat(region.width) / sourceWidth,
            height: CGFloat(region.height) / sourceHeight
        )
        let outputRect = geometry.outputRect(fromSource: regionSource)

        // 区域管线只负责调色 / 效果 / 蒙版；几何已由上面的映射和像素变换完成，
        // 所以参数里的裁切必须置为单位几何（否则核心会拒绝或重复裁切）。
        var regionState = renderState
        regionState.crop = CropState()
        let json = EditJSON.build(state: regionState, applyCropRect: false)
        let flipHorizontal = state.crop.flipHorizontal
        let flipVertical = state.crop.flipVertical
        let quarterTurns = state.crop.quarterTurns

        detailRegionToken &+= 1
        let token = detailRegionToken
        let key = regionGeometryKey
        detailQueue.async { [weak self] in
            guard let self else { return }
            guard
                let regionBuffer = ImageUtil.cropped(
                    buffer: source,
                    x: region.x,
                    y: region.y,
                    width: region.width,
                    height: region.height
                ),
                let result = NativeCore.shared.renderRegion(
                    buffer: regionBuffer,
                    originX: region.x,
                    originY: region.y,
                    visibleX: region.x,
                    visibleY: region.y,
                    visibleWidth: region.width,
                    visibleHeight: region.height,
                    fullWidth: source.width,
                    fullHeight: source.height,
                    paramsJson: json
                ),
                let transformed = ImageUtil.transformed(
                    buffer: RgbaBuffer(
                        pixels: result.pixels,
                        width: result.width,
                        height: result.height
                    ),
                    flipHorizontal: flipHorizontal,
                    flipVertical: flipVertical,
                    quarterTurns: quarterTurns
                ),
                let rendered = ImageUtil.image(from: transformed)
            else {
                // 核心拒绝区域渲染（非单位蒙版等）：回退整帧细节帧。
                Task { @MainActor in
                    guard self.detailRegionToken == token else { return }
                    self.detailRegion = nil
                    self.detailRegionKey = ""
                    self.requestDetailPreview(
                        viewport: viewport,
                        zoom: self.lastDetailZoom,
                        force: true
                    )
                }
                return
            }
            Task { @MainActor in
                guard self.detailRegionToken == token else { return }
                self.detailRegion = DetailRegion(image: rendered, rect: outputRect)
                self.detailRegionKey = key
            }
        }
    }

    /// 编辑提交后刷新（放大状态下保持像素级清晰）。
    private func refreshDetailIfNeeded() {
        guard detailZoomActive || detailRegion != nil || detailImage != nil else { return }
        if detailRegion != nil
            || (detailZoomActive && canRenderRegionDetail && !lastRegionVisible.isEmpty)
        {
            if canRenderRegionDetail, !lastRegionVisible.isEmpty {
                performDetailRegionRequest(visible: lastRegionVisible, viewport: lastDetailViewport)
            } else {
                // 开了镜头校正 / 色差 / 修图：区域渲染不可用，回落到整帧细节帧。
                clearDetailRegion()
                requestDetailPreview(viewport: lastDetailViewport, zoom: lastDetailZoom, force: true)
            }
        } else if detailImage != nil, !detailInFlight {
            // 整帧模式：参数变了，用同一档分辨率强制重渲染。
            requestDetailPreview(viewport: lastDetailViewport, zoom: lastDetailZoom, force: true)
        }
    }

    /// 区域帧的几何映射（归一化坐标）：裁切 → 翻转 → 90° 旋转，与核心 `apply_geometry` 同序。
    private struct DetailRegionGeometry {
        let crop: CGRect
        let flipHorizontal: Bool
        let flipVertical: Bool
        let quarterTurns: Int

        private var turns: Int {
            ((quarterTurns % 4) + 4) % 4
        }

        /// 输出坐标（裁切后）→ 源坐标（原图）。
        func sourceRect(fromOutput rect: CGRect) -> CGRect {
            let safeCrop = CGRect(
                x: crop.minX,
                y: crop.minY,
                width: max(crop.width, 0.0001),
                height: max(crop.height, 0.0001)
            )
            var result = CGRect(
                x: safeCrop.minX + rect.minX * safeCrop.width,
                y: safeCrop.minY + rect.minY * safeCrop.height,
                width: rect.width * safeCrop.width,
                height: rect.height * safeCrop.height
            )
            // 反向 90° 旋转。
            switch turns {
            case 1:
                result = CGRect(
                    x: result.minY,
                    y: 1 - result.maxX,
                    width: result.height,
                    height: result.width
                )
            case 2:
                result = CGRect(
                    x: 1 - result.maxX,
                    y: 1 - result.maxY,
                    width: result.width,
                    height: result.height
                )
            case 3:
                result = CGRect(
                    x: 1 - result.maxY,
                    y: result.minX,
                    width: result.height,
                    height: result.width
                )
            default:
                break
            }
            // 反向翻转（翻转在旋转之前）。
            if flipHorizontal {
                result.origin.x = 1 - result.maxX
            }
            if flipVertical {
                result.origin.y = 1 - result.maxY
            }
            return result
        }

        /// 源坐标 → 输出坐标（裁切后）。
        func outputRect(fromSource rect: CGRect) -> CGRect {
            var result = rect
            // 正向翻转。
            if flipHorizontal {
                result.origin.x = 1 - result.maxX
            }
            if flipVertical {
                result.origin.y = 1 - result.maxY
            }
            // 正向 90° 旋转。
            switch turns {
            case 1:
                result = CGRect(
                    x: 1 - result.maxY,
                    y: result.minX,
                    width: result.height,
                    height: result.width
                )
            case 2:
                result = CGRect(
                    x: 1 - result.maxX,
                    y: 1 - result.maxY,
                    width: result.width,
                    height: result.height
                )
            case 3:
                result = CGRect(
                    x: result.minY,
                    y: 1 - result.maxX,
                    width: result.height,
                    height: result.width
                )
            default:
                break
            }
            let safeCrop = CGRect(
                x: crop.minX,
                y: crop.minY,
                width: max(crop.width, 0.0001),
                height: max(crop.height, 0.0001)
            )
            let x = (result.minX - safeCrop.minX) / safeCrop.width
            let y = (result.minY - safeCrop.minY) / safeCrop.height
            let width = result.width / safeCrop.width
            let height = result.height / safeCrop.height
            return CGRect(
                x: min(max(x, 0), 1),
                y: min(max(y, 0), 1),
                width: min(max(width, 0), 1 - min(max(x, 0), 1)),
                height: min(max(height, 0), 1 - min(max(y, 0), 1))
            )
        }
    }

    /// 切换工具（裁切时预览显示未裁切原图，其余工具显示裁切结果）。
    func selectTool(_ newTool: EditorTool) {
        guard tool != newTool else { return }
        let previousTool = tool
        tool = newTool
        draggingSlider = nil
        // 工具切换可能改变"裁切是否生效"的几何：区域帧必须按新映射重取。
        clearDetailRegion()
        if newTool == .crop || previousTool == .crop {
            clearDetailPreview()
        }
        if newTool == .mask {
            if selectedMaskId == nil {
                selectedMaskId = state.masks.first?.id
            }
            // 蒙版调整面板不包含细节分类。
            if category == .detail {
                category = .light
            }
            scheduleMaskAssetsRefresh(immediate: true)
        } else if newTool == .presets {
            // 进入预设工具：用当前照片刷一遍预设缩略图。
            refreshPresetThumbnails()
        } else {
            brushCursor = nil
        }
        scheduleRender(immediate: true)
    }

    // MARK: - 通用编辑（撤销 / 重做 / 提交）

    /// 开始一次编辑（滑块按下 / 拖动开始）：记录快照。
    func beginEdit() {
        if suppressPresetStrengthReset {
            suppressPresetStrengthReset = false
        } else {
            // 又做了别的编辑：预设强度上下文作废。
            presetBase = nil
            presetTarget = nil
        }
        if dragSnapshot == nil {
            dragSnapshot = state
        }
    }

    /// 编辑中：请求防抖渲染。
    func touchEdit() {
        scheduleRender()
    }

    /// 编辑结束：记录一步历史、持久化并立即渲染。
    func commitEdit() {
        if let before = dragSnapshot, before != state {
            pushUndo(before)
        }
        dragSnapshot = nil
        draggingSlider = nil
        draggingSliderText = nil
        presetName = "自定义"
        presetRef = nil
        persist()
        scheduleRender(immediate: true)
        refreshDetailIfNeeded()
    }

    /// 一次性参数修改（点按类操作）：自动记一步历史。
    func applyEdit(_ label: String = "编辑", _ mutate: (inout EditState) -> Void) {
        presetBase = nil
        presetTarget = nil
        let before = state
        mutate(&state)
        guard state != before else { return }
        pendingLabel = label
        pushUndo(before)
        presetName = "自定义"
        presetRef = nil
        persist()
        scheduleRender(immediate: true)
        refreshDetailIfNeeded()
    }

    func undo() {
        restoreHistory(stepBack: 1, announce: true)
    }

    func redo() {
        guard let entry = redoStack.popLast() else { return }
        undoStack.append(HistoryEntry(name: entry.name, state: state))
        state = entry.state
        presetBase = nil
        presetTarget = nil
        publishHistory()
        presetName = "自定义"
        presetRef = nil
        message = "已重做：\(entry.name)"
        persist()
        scheduleRender(immediate: true)
        refreshDetailIfNeeded()
    }

    /// 历史步骤名（由近到远，不含"当前"）。
    var historyNames: [String] {
        undoStack.reversed().map(\.name)
    }

    /// 回退 `steps` 步（用于长按撤销键的历史列表；0 = 不动）。
    func restoreHistory(stepBack steps: Int, announce: Bool = false) {
        guard steps > 0 else { return }
        var last: String?
        for _ in 0..<steps {
            guard let entry = undoStack.popLast() else { break }
            redoStack.append(HistoryEntry(name: entry.name, state: state))
            state = entry.state
            last = entry.name
        }
        guard let name = last else { return }
        presetBase = nil
        presetTarget = nil
        publishHistory()
        presetName = "自定义"
        presetRef = nil
        if announce {
            message = "已撤销：\(name)"
        }
        persist()
        scheduleRender(immediate: true)
        refreshDetailIfNeeded()
    }

    // MARK: - 基础滑块

    func value(_ key: String) -> Float {
        state.values[key] ?? 0
    }

    func updateValue(_ key: String, _ newValue: Float) {
        pendingLabel = EditorSpec.sliders.first { $0.key == key }?.label ?? "调色"
        state.values[key] = newValue
        touchEdit()
    }

    // MARK: - 预设

    /// 应用内置预设（与 Android 一致：重置滑块与曲线，保留 HSL / 分级 / 蒙版）。
    func applyPreset(_ preset: BuiltinPreset) {
        let base = state
        applyEdit(preset.name) { state in
            var values = EditorSpec.defaultValues()
            for (key, value) in preset.params {
                values[key] = value
            }
            state.values = values
            state.curves.reset()
        }
        presetName = preset.name
        presetRef = preset.ref
        // 记住应用前后：预设「强度」在两者之间插值。
        presetBase = base
        presetTarget = state
        presetStrength = 100
    }

    // MARK: 用户预设库（文件夹 + 预设）

    /// 新建预设文件夹。
    @discardableResult
    func createPresetFolder(name: String) -> PresetFolder? {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        let folder = PresetFolder(id: UUID().uuidString, name: trimmed, createdAt: Date())
        presetFolders.append(folder)
        persistPresetLibrary()
        return folder
    }

    /// 删除文件夹（连同里面的预设）。
    func deletePresetFolder(_ id: String) {
        let removed = userPresets.filter { $0.folderId == id }
        userPresets.removeAll { $0.folderId == id }
        presetFolders.removeAll { $0.id == id }
        if let ref = presetRef, removed.contains(where: { $0.ref == ref }) {
            presetRef = nil
            presetName = "自定义"
        }
        persistPresetLibrary()
        refreshPresetThumbnails()
    }

    /// 删除用户预设。
    func deleteUserPreset(_ id: String) {
        userPresets.removeAll { $0.id == id }
        if presetRef == "u:\(id)" {
            presetRef = nil
            presetName = "自定义"
        }
        persistPresetLibrary()
        refreshPresetThumbnails()
    }

    /// 把当前调色存为用户预设（只收集勾选的分组）。
    @discardableResult
    func saveUserPreset(
        name: String,
        folderId: String,
        sections: Set<PresetSection>
    ) -> UserPreset? {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty,
              presetFolders.contains(where: { $0.id == folderId }),
              !sections.isEmpty else { return nil }
        let preset = UserPreset.make(
            from: state,
            name: trimmed,
            folderId: folderId,
            sections: sections
        )
        userPresets.append(preset)
        persistPresetLibrary()
        message = "已保存预设「\(trimmed)」"
        refreshPresetThumbnails()
        return preset
    }

    /// 应用用户预设：只覆盖勾选的分组，其余保持原样；之后可调「强度」。
    func applyUserPreset(_ preset: UserPreset) {
        let base = state
        applyEdit(preset.name) { state in
            preset.apply(to: &state)
        }
        presetName = preset.name
        presetRef = preset.ref
        presetBase = base
        presetTarget = state
        presetStrength = 100
    }

    private func persistPresetLibrary() {
        PresetLibraryCoding.save(folders: presetFolders, presets: userPresets)
    }

    /// 有可调强度的预设上下文（应用过预设后为 true）。
    var canAdjustPresetStrength: Bool {
        presetBase != nil && presetTarget != nil
    }

    /// 开始拖预设强度滑块（此时不要清掉混合上下文）。
    func beginPresetStrengthEdit() {
        suppressPresetStrengthReset = true
        pendingLabel = "预设强度"
        beginEdit()
    }

    /// 拖动预设强度：在「应用前」与「预设结果」之间插值。
    /// 曲线按 50% 分界切换（与核心 `EditParams::blend` 的桌面行为一致）；
    /// 预设没包含的分组在前后两份状态里本来就相同，插值等于不变。
    func updatePresetStrength(_ value: Float) {
        guard let base = presetBase, let target = presetTarget else { return }
        presetStrength = value
        let t = value / 100
        var updated = state
        for spec in EditorSpec.sliders {
            let from = base.values[spec.key] ?? spec.defaultValue
            let to = target.values[spec.key] ?? spec.defaultValue
            updated.values[spec.key] = from + (to - from) * t
        }
        // HSL / 颜色分级同样插值（用户预设可能包含这两组）。
        var hsl = updated.hsl
        for kind in HslBandKind.allCases {
            let from = base.hsl[kind]
            let to = target.hsl[kind]
            hsl[kind] = HslBand(
                hue: from.hue + (to.hue - from.hue) * t,
                saturation: from.saturation + (to.saturation - from.saturation) * t,
                luminance: from.luminance + (to.luminance - from.luminance) * t
            )
        }
        updated.hsl = hsl
        var grading = updated.grading
        for kind in GradingZoneKind.allCases {
            let from = base.grading[kind]
            let to = target.grading[kind]
            grading[kind] = GradingZone(
                hue: from.hue + (to.hue - from.hue) * t,
                saturation: from.saturation + (to.saturation - from.saturation) * t,
                luminance: from.luminance + (to.luminance - from.luminance) * t
            )
        }
        grading.blending = base.grading.blending
            + (target.grading.blending - base.grading.blending) * t
        grading.balance = base.grading.balance
            + (target.grading.balance - base.grading.balance) * t
        updated.grading = grading
        // 相机校准按比例插值；黑白开关与点颜色样本按 50% 切换（与曲线同一策略）。
        var calibration = updated.calibration
        calibration.redHue = base.calibration.redHue
            + (target.calibration.redHue - base.calibration.redHue) * t
        calibration.redSat = base.calibration.redSat
            + (target.calibration.redSat - base.calibration.redSat) * t
        calibration.greenHue = base.calibration.greenHue
            + (target.calibration.greenHue - base.calibration.greenHue) * t
        calibration.greenSat = base.calibration.greenSat
            + (target.calibration.greenSat - base.calibration.greenSat) * t
        calibration.blueHue = base.calibration.blueHue
            + (target.calibration.blueHue - base.calibration.blueHue) * t
        calibration.blueSat = base.calibration.blueSat
            + (target.calibration.blueSat - base.calibration.blueSat) * t
        calibration.shadowsTint = base.calibration.shadowsTint
            + (target.calibration.shadowsTint - base.calibration.shadowsTint) * t
        updated.calibration = calibration
        var bwMix = updated.bwMix
        for kind in HslBandKind.allCases {
            let from = base.bwMix[kind]
            let to = target.bwMix[kind]
            bwMix[kind] = from + (to - from) * t
        }
        updated.bwMix = bwMix
        updated.blackWhite = t < 0.5 ? base.blackWhite : target.blackWhite
        updated.pointColors = t < 0.5 ? base.pointColors : target.pointColors
        updated.curves = t < 0.5 ? base.curves : target.curves
        state = updated
        touchEdit()
    }

    /// 结束预设强度拖动。
    func commitPresetStrengthEdit() {
        // 与普通编辑的 commit 不同：调完强度后预设仍是"当前预设"（缩略图上的
        // 强度控件要继续可用），因此这里不清 presetName / presetRef。
        if let before = dragSnapshot, before != state {
            pushUndo(before)
        }
        dragSnapshot = nil
        draggingSlider = nil
        draggingSliderText = nil
        persist()
        scheduleRender(immediate: true)
        refreshDetailIfNeeded()
    }

    // MARK: - 预设导入 / 导出 / 剪贴板

    /// 预设的完整参数快照（用户预设：勾选的分组应用到默认状态）。
    func presetSnapshot(_ preset: UserPreset) -> EditState {
        var state = EditState()
        preset.apply(to: &state)
        return state
    }

    /// 预设的完整参数快照（系统预设：参数覆盖默认值）。
    func presetSnapshot(_ preset: BuiltinPreset) -> EditState {
        var state = EditState()
        var values = EditorSpec.defaultValues()
        for (key, value) in preset.params {
            values[key] = value
        }
        state.values = values
        return state
    }

    /// 复制预设到系统剪贴板（带 `fdxmp_preset` 标记的 JSON 文本）。
    func copyPresetToPasteboard(name: String, state: EditState) {
        let paramsJson = EditJSON.build(state: state, applyCropRect: false)
        guard
            let params = Self.jsonObject(paramsJson),
            let payload = try? JSONSerialization.data(withJSONObject: [
                "fdxmp_preset": 1,
                "name": name,
                "params": params,
            ]),
            let text = String(data: payload, encoding: .utf8)
        else {
            message = "复制预设失败"
            return
        }
        UIPasteboard.general.string = text
        message = "已复制预设「\(name)」"
    }

    /// 从系统剪贴板粘贴预设（不存在 / 格式不对时给出提示）。
    @discardableResult
    func pastePresetFromPasteboard() -> UserPreset? {
        guard
            let text = UIPasteboard.general.string?
                .trimmingCharacters(in: .whitespacesAndNewlines),
            !text.isEmpty,
            let root = Self.jsonObject(text),
            let params = root["params"],
            let paramsData = try? JSONSerialization.data(withJSONObject: params),
            let paramsJson = String(data: paramsData, encoding: .utf8)
        else {
            message = "剪贴板里没有可粘贴的预设"
            return nil
        }
        let name = (root["name"] as? String) ?? "粘贴的预设"
        return importUserPreset(name: name, paramsJson: paramsJson)
    }

    /// 从 `.fdxmp` 字节流导入预设。
    @discardableResult
    func importPresetFile(_ data: Data) -> UserPreset? {
        guard
            let json = NativeCore.shared.presetJsonFromFdxmp(data),
            let imported = Self.importedPresetPayload(json)
        else {
            message = "导入失败：文件格式不支持"
            return nil
        }
        return importUserPreset(name: imported.name, paramsJson: imported.paramsJson)
    }

    /// 从 Lightroom `.xmp` 文本导入预设。
    @discardableResult
    func importPresetXmp(_ text: String) -> UserPreset? {
        guard
            let json = NativeCore.shared.presetJsonFromXmp(text),
            let imported = Self.importedPresetPayload(json)
        else {
            message = "导入失败：XMP 里没有可识别的参数"
            return nil
        }
        return importUserPreset(name: imported.name, paramsJson: imported.paramsJson)
    }

    /// 导出预设为 `.fdxmp` 临时文件（供系统分享面板使用）。
    func exportPresetFile(name: String, state: EditState) -> URL? {
        let paramsJson = EditJSON.build(state: state, applyCropRect: false)
        guard
            let bytes = NativeCore.shared.presetBytes(
                paramsJson: paramsJson,
                name: name,
                author: "fdxmp"
            )
        else {
            message = "导出预设失败：\(NativeCore.shared.lastError() ?? "未知错误")"
            return nil
        }
        let safeName = name
            .replacingOccurrences(of: "/", with: "-")
            .replacingOccurrences(of: ":", with: "-")
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("\(safeName.isEmpty ? "预设" : safeName).fdxmp")
        do {
            try Data(bytes).write(to: url, options: .atomic)
            return url
        } catch {
            message = "导出预设失败：\(error.localizedDescription)"
            return nil
        }
    }

    /// 由参数 JSON 建用户预设（导入 / 粘贴共用）：放进「导入」文件夹。
    @discardableResult
    func importUserPreset(name: String, paramsJson: String) -> UserPreset? {
        let parsed = EditJSON.parse(paramsJson)
        let folder = ensureImportFolder()
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let preset = UserPreset.make(
            from: parsed,
            name: trimmed.isEmpty ? "导入的预设" : trimmed,
            folderId: folder.id,
            sections: Set(PresetSection.allCases)
        )
        userPresets.append(preset)
        persistPresetLibrary()
        message = "已导入预设「\(preset.name)」"
        refreshPresetThumbnails()
        return preset
    }

    /// 显示一条提示（导入失败等场景由视图调用）。
    func showMessage(_ text: String) {
        message = text
    }

    /// 导入文件夹（不存在时创建，并把新预设放进这里）。
    private func ensureImportFolder() -> PresetFolder {
        if let existing = presetFolders.first(where: { $0.name == "导入" }) {
            return existing
        }
        let folder = PresetFolder(id: UUID().uuidString, name: "导入", createdAt: Date())
        presetFolders.append(folder)
        return folder
    }

    /// 解析导入 JSON（`{name, params}`）→ 名称 + 参数 JSON 文本。
    private static func importedPresetPayload(
        _ json: String
    ) -> (name: String, paramsJson: String)? {
        guard
            let root = jsonObject(json),
            let params = root["params"],
            let paramsData = try? JSONSerialization.data(withJSONObject: params),
            let paramsJson = String(data: paramsData, encoding: .utf8)
        else {
            return nil
        }
        let name = (root["name"] as? String) ?? ""
        return (name.isEmpty ? "导入的预设" : name, paramsJson)
    }

    /// JSON 文本 → 对象（解析失败返回 nil）。
    private static func jsonObject(_ json: String) -> [String: Any]? {
        guard
            let data = json.data(using: .utf8),
            let object = try? JSONSerialization.jsonObject(with: data)
        else {
            return nil
        }
        return object as? [String: Any]
    }

    /// 全部归零。
    func resetAll() {
        applyEdit("全部重置") { state in
            state = EditState()
        }
        selectedMaskId = nil
        brushErase = false
        brushCursor = nil
        maskInteracting = false
        presetName = "原片"
        presetRef = EditorSpec.presets.first?.ref
        // 全部重置 = 调色 + 修图（含手动污点 / 液化）一起回到原图。
        activeRetouchPreset = nil
        retouchStrength = 0
        healSpots.removeAll()
        liquifyStrokes.removeAll()
        healBuffer = nil
        liquifyBuffer = nil
        retouchApplied = nil
        retouchBuffer = nil
        retouchAppliedReady = false
        clearGpuSession()
        if let source = renderSource {
            createGpuSession(for: source)
        }
        scheduleRender(immediate: true)
        scheduleMaskAssetsRefresh(immediate: true)
    }

    /// 加载卡死时的自救入口：删掉落盘的编辑记录，按原片重新载入。
    ///
    /// 目前个别参数组合会让核心渲染卡住（后续单独修），这里先给用户一个逃生口：
    /// 清掉这张图的调色记录 → 重新解码 → 用默认参数重新渲染。
    func resetEditsAfterStuckLoad() {
        guard let current = asset else { return }
        EditStore.remove(assetId: current.localIdentifier)
        state = EditState()
        presetName = "原片"
        presetRef = EditorSpec.presets.first?.ref
        selectedMaskId = nil
        // load() 遇到"同一张照片"会直接返回：先清掉当前资产，让整条链路重来。
        self.asset = nil
        load(asset: current)
        message = "已重置全部编辑，正在按原片重新加载"
    }

    // MARK: - 曲线

    func curvePoints(_ channel: CurveChannel) -> [CurvePoint] {
        state.curves.points(channel)
    }

    /// 更新曲线控制点（拖动中 commit = false）。
    func updateCurve(_ channel: CurveChannel, _ points: [CurvePoint], commit: Bool) {
        beginEdit()
        pendingLabel = "曲线 · \(channel.rawValue)"
        state.curves.setPoints(channel, points)
        if commit {
            commitEdit()
        } else {
            touchEdit()
        }
    }

    /// 更新参数曲线（滑块）。
    func updateParametric(_ values: ParametricValues, commit: Bool) {
        beginEdit()
        pendingLabel = "曲线 · 参数"
        state.curves.parametric = values
        if commit {
            commitEdit()
        } else {
            touchEdit()
        }
    }

    func resetCurve(_ channel: CurveChannel) {
        applyEdit("重置曲线 · \(channel.rawValue)") { state in
            state.curves.setPoints(channel, CurveState.identity)
        }
    }

    // MARK: - HSL / 颜色分级

    func updateHsl(_ band: HslBandKind, _ value: HslBand, commit: Bool) {
        beginEdit()
        pendingLabel = "HSL · \(band.rawValue)"
        state.hsl[band] = value
        if commit {
            commitEdit()
        } else {
            touchEdit()
        }
    }

    /// 颜色均衡器（整组参数一起改，面板按字段调用）。
    func updateEqualizer(_ value: EqualizerState, commit: Bool) {
        beginEdit()
        pendingLabel = "颜色均衡器"
        state.equalizer = value
        if commit {
            commitEdit()
        } else {
            touchEdit()
        }
    }

    func updateGrading(_ zone: GradingZoneKind, _ value: GradingZone, commit: Bool) {        beginEdit()
        pendingLabel = "分级 · \(zone.rawValue)"
        state.grading[zone] = value
        if commit {
            commitEdit()
        } else {
            touchEdit()
        }
    }

    // MARK: - 相机校准 / 黑白混色 / 点颜色

    /// 相机校准（七项一起改，面板按字段调用）。
    func updateCalibration(_ value: CalibrationState, commit: Bool) {
        beginEdit()
        pendingLabel = "校准"
        state.calibration = value
        if commit {
            commitEdit()
        } else {
            touchEdit()
        }
    }

    /// 黑白混色（单色带）。
    func updateBwMix(_ kind: HslBandKind, _ value: Float, commit: Bool) {
        beginEdit()
        pendingLabel = "黑白混色 · \(kind.rawValue)"
        state.bwMix[kind] = value
        if commit {
            commitEdit()
        } else {
            touchEdit()
        }
    }

    /// 点颜色：新增一个待取样的样本并选中它。
    func addPointColorSample() {
        guard state.pointColors.count < pointColorSampleLimit else {
            message = "最多 \(pointColorSampleLimit) 个点颜色样本"
            return
        }
        beginEdit()
        pendingLabel = "新增点颜色"
        state.pointColors.append(PointColorSample())
        pointColorIndex = state.pointColors.count - 1
        commitEdit()
    }

    /// 点颜色：修改第 `index` 个样本。
    func updatePointColor(
        index: Int,
        commit: Bool,
        _ mutate: (inout PointColorSample) -> Void
    ) {
        guard state.pointColors.indices.contains(index) else { return }
        beginEdit()
        pendingLabel = "点颜色"
        var sample = state.pointColors[index]
        mutate(&sample)
        state.pointColors[index] = sample
        if commit {
            commitEdit()
        } else {
            touchEdit()
        }
    }

    /// 点颜色：删除一个样本。
    func removePointColor(index: Int) {
        guard state.pointColors.indices.contains(index) else { return }
        beginEdit()
        pendingLabel = "删除点颜色"
        state.pointColors.remove(at: index)
        pointColorIndex = min(pointColorIndex, max(state.pointColors.count - 1, 0))
        commitEdit()
    }

    /// 点颜色：清空全部样本。
    func clearPointColors() {
        guard !state.pointColors.isEmpty else { return }
        beginEdit()
        pendingLabel = "清空点颜色"
        state.pointColors.removeAll()
        pointColorIndex = 0
        commitEdit()
    }

    /// 取色：把画布归一化坐标处的像素转成 OkLCh 写进样本（无样本时新建，最多 4 个）。
    ///
    /// 采样源是最近一帧 CPU 预览（`previewPixels`）；GPU 零拷贝上屏时不读回像素，
    /// 此时提示用户等一帧即可。
    @discardableResult
    func samplePointColor(at normalized: CGPoint) -> Bool {
        guard let pixels = previewPixels, pixels.width > 0, pixels.height > 0 else {
            message = "预览尚未就绪，请稍后取色"
            return false
        }
        let x = min(max(Int((normalized.x * CGFloat(pixels.width)).rounded(.down)), 0), pixels.width - 1)
        let y = min(max(Int((normalized.y * CGFloat(pixels.height)).rounded(.down)), 0), pixels.height - 1)
        let offset = (y * pixels.width + x) * 4
        guard offset + 2 < pixels.pixels.count else { return false }
        let r = pixels.pixels[offset]
        let g = pixels.pixels[offset + 1]
        let b = pixels.pixels[offset + 2]
        guard let lch = NativeCore.shared.oklch(r: r, g: g, b: b) else {
            message = NativeCore.shared.lastError() ?? "取色失败"
            return false
        }

        beginEdit()
        pendingLabel = "点颜色取色"
        if state.pointColors.isEmpty {
            var sample = PointColorSample()
            sample.lum = lch.l
            sample.chroma = lch.c
            sample.hue = lch.h * 180 / .pi
            state.pointColors.append(sample)
            pointColorIndex = 0
        } else {
            let index = min(pointColorIndex, state.pointColors.count - 1)
            state.pointColors[index].lum = lch.l
            state.pointColors[index].chroma = lch.c
            state.pointColors[index].hue = lch.h * 180 / .pi
            pointColorIndex = index
        }
        commitEdit()
        Haptics.tick()
        return true
    }

    func updateGradingMix(blending: Float, balance: Float, commit: Bool) {
        beginEdit()
        pendingLabel = "分级 · 混合"
        state.grading.blending = blending
        state.grading.balance = balance
        if commit {
            commitEdit()
        } else {
            touchEdit()
        }
    }

    // MARK: - 裁切 / 变换

    func updateCropRect(_ rect: CGRect, commit: Bool) {
        beginEdit()
        pendingLabel = "裁切"
        if commit {
            state.crop.rect = clampedCropRect(rect)
            commitCropDrag()
        } else {
            // 拖动中允许少量阻尼越界（松手回弹到合法范围）。
            state.crop.rect = rect
        }
    }

    /// 裁切拖动结束：带阻尼越界的矩形缓动回合法范围（约 160ms），再记一步历史。
    func finishCropDrag() {
        let target = clampedCropRect(state.crop.rect)
        let start = state.crop.rect
        guard start != target else {
            commitCropDrag()
            return
        }
        let steps = 8
        for step in 1...steps {
            let t = Double(step) / Double(steps)
            let eased = CGFloat(1 - pow(1 - t, 3))
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.02 * Double(step)) { [weak self] in
                guard let self else { return }
                self.state.crop.rect = CGRect(
                    x: start.minX + (target.minX - start.minX) * eased,
                    y: start.minY + (target.minY - start.minY) * eased,
                    width: start.width + (target.width - start.width) * eased,
                    height: start.height + (target.height - start.height) * eased
                )
                if step == steps {
                    self.commitCropDrag()
                }
            }
        }
    }

    /// 裁切拖动提交：夹取 + 记历史 + 保存（裁切结果在切换工具 / 导出时统一渲染）。
    private func commitCropDrag() {
        state.crop.rect = clampedCropRect(state.crop.rect)
        if let before = dragSnapshot, before != state {
            pushUndo(before)
        }
        dragSnapshot = nil
        presetName = "自定义"
        presetRef = nil
        persist()
        scheduleRender(immediate: true)
        refreshDetailIfNeeded()
    }

    func updateCropRotation(_ value: Float, commit: Bool) {
        beginEdit()
        pendingLabel = "拉直"
        state.crop.rotation = value.clamped(to: -45...45)
        if commit {
            commitEdit()
        } else {
            touchEdit()
        }
    }

    /// 选择固定比例：按比例调整当前裁剪框（不缩放图像）。
    func setCropAspect(_ aspect: CropAspect, contentSize: CGSize) {
        applyEdit("裁切比例") { state in
            state.crop.aspect = aspect
            guard let ratio = aspect.ratio, contentSize.width > 0, contentSize.height > 0 else {
                return
            }
            // 以当前裁剪框中心为基准，按目标比例取最大内接矩形。
            let current = state.crop.rect
            let center = CGPoint(x: current.midX, y: current.midY)
            // 归一化坐标下，宽高比需要换算图像本身的像素高宽比。
            let contentAspect = contentSize.height / contentSize.width
            var width = min(max(current.width, 0.05), 1)
            var height = width * contentAspect / ratio
            if height > 1 {
                height = 1
                width = height * ratio / contentAspect
            }
            if width > 1 {
                width = 1
                height = width * contentAspect / ratio
            }
            let x = (center.x - width / 2).clamped(to: 0...max(1 - width, 0))
            let y = (center.y - height / 2).clamped(to: 0...max(1 - height, 0))
            state.crop.rect = clampedCropRect(
                CGRect(x: x, y: y, width: width, height: height)
            )
        }
    }

    func toggleCropFlip(horizontal: Bool) {
        applyEdit("翻转") { state in
            if horizontal {
                state.crop.flipHorizontal.toggle()
            } else {
                state.crop.flipVertical.toggle()
            }
        }
    }

    /// 顺时针旋转 90°（整圈旋转：输出宽高互换，裁剪框 / 蒙版跟着转到新构图）。
    func rotateCropQuarterTurn() {
        applyEdit("旋转 90°") { state in
            state.crop.quarterTurns = (state.crop.quarterTurns + 1) % 4
            // 裁剪框：旧帧 → 新帧（顺时针 90°：(x, y, w, h) → (1-y-h, x, h, w)）。
            let rect = state.crop.rect
            state.crop.rect = clampedCropRect(
                CGRect(
                    x: 1 - rect.maxY,
                    y: rect.minX,
                    width: rect.height,
                    height: rect.width
                )
            )
            state.masks = state.masks.map(Self.rotatedQuarterTurn)
        }
        // 90° 整圈旋转 GPU 预览对不上（宽高变了）：先丢掉会话走 CPU，转回 0 圈再重建。
        clearGpuSession()
        if state.crop.quarterTurns == 0, let source = renderSource {
            createGpuSession(for: source)
        }
        scheduleRender(immediate: true)
        refreshDetailIfNeeded()
    }

    /// 蒙版几何顺时针转 90°（归一化坐标）：线性端点 / 径向中心与轴向 / 画笔点。
    private static func rotatedQuarterTurn(_ mask: MaskUi) -> MaskUi {
        var copy = mask
        func turn(_ point: CGPoint) -> CGPoint {
            CGPoint(x: 1 - point.y, y: point.x)
        }
        copy.start = turn(mask.start)
        copy.end = turn(mask.end)
        copy.center = turn(mask.center)
        copy.radii = CGSize(width: mask.radii.height, height: mask.radii.width)
        copy.rotation = mask.rotation + 90
        copy.strokes = mask.strokes.map { stroke in
            var item = stroke
            item.points = stroke.points.map(turn)
            return item
        }
        return copy
    }

    func resetCrop() {
        applyEdit("重置裁切") { state in
            state.crop.reset()
        }
    }

    // MARK: - 蒙版

    /// 新建蒙版（线性 / 径向 / 画笔 / 智能），并选中它。
    func addMask(_ kind: MaskKindUi) {
        maskPlacementKind = nil
        let mask = makeMask(kind)
        applyEdit("新建\(kind.rawValue)") { state in
            state.masks.append(mask)
        }
        selectedMaskId = mask.id
        brushErase = false
        if kind.isSmart {
            generateSmartMask(mask.id)
        }
    }

    /// 按类型造一个带默认几何的蒙版（名字按同类序号）。
    private func makeMask(_ kind: MaskKindUi) -> MaskUi {
        let count = state.masks.filter { $0.kind == kind }.count + 1
        var mask = MaskUi(
            id: UUID().uuidString,
            name: "\(kind.rawValue) \(count)",
            kind: kind
        )
        switch kind {
        case .linear:
            // 落在画面中部：过渡带从 32% 到 62%（拖端点 / 调参数再细调）。
            mask.start = CGPoint(x: 0.5, y: 0.32)
            mask.end = CGPoint(x: 0.5, y: 0.62)
        case .radial:
            mask.center = CGPoint(x: 0.5, y: 0.5)
            mask.radii = CGSize(width: 0.42, height: 0.3)
            mask.rotation = 0
            mask.radialFeather = 30
        case .brush:
            mask.brushSize = 30
            mask.brushFeather = 50
            mask.brushFlow = 100
            mask.brushDensity = 100
        case .aiSubject, .aiPortrait, .aiFace, .aiSkin,
            .aiEyes, .aiEyebrows, .aiNose, .aiLips, .aiTeeth:
            // 智能蒙版：无几何参数；栅格由 Vision 生成后提交核心缓存。
            break
        }
        mask.values = EditorSpec.defaultValues()
        return mask
    }

    // MARK: - 拖动放置几何蒙版（线性 / 径向）

    /// 待放置的蒙版类型：点「线性渐变 / 径向渐变」后进入放置态，等用户在画面上拖出位置。
    @Published private(set) var maskPlacementKind: MaskKindUi?

    /// 点「线性渐变 / 径向渐变」：先不建蒙版，等手指在画面上按下再定位置 / 方向 / 大小。
    func requestMaskPlacement(_ kind: MaskKindUi) {
        guard kind == .linear || kind == .radial else { return }
        maskPlacementKind = kind
        Haptics.light()
        showMessage("在画面上拖动：\(kind == .linear ? "从起点拖出渐变方向" : "从中心拖出大小")")
    }

    /// 画布按下：真正创建蒙版（先记快照，撤销时整步消失），起手给一个很小的几何。
    func beginMaskPlacement(at point: CGPoint) {
        guard let kind = maskPlacementKind else { return }
        beginEdit()
        pendingLabel = "新建\(kind.rawValue)"
        var mask = makeMask(kind)
        if kind == .linear {
            mask.start = point
            mask.end = CGPoint(x: point.x, y: point.y + 0.08)
        } else {
            mask.center = point
            // 径向按下就先给一个极小的几何，之后完全由拖动距离决定大小。
            mask.radii = CGSize(width: 0.01, height: 0.01)
        }
        state.masks.append(mask)
        selectedMaskId = mask.id
        brushErase = false
        brushCursor = nil
        maskInteracting = true
        scheduleRender()
        scheduleMaskAssetsRefresh(immediate: true)
    }

    /// 松手：几何完全跟手（拖多大就是多大），整步提交。
    func finishMaskPlacement() {
        maskPlacementKind = nil
        finishMaskInteraction()
    }

    /// 取消本次放置刚创建的蒙版（只点了一下 / 第二指打断）：
    /// 从状态里移除、恢复之前的选中项，不落历史；放置态保留，方便再拖一次。
    func cancelMaskPlacementCreation(maskId: String, restoreSelection: String?) {
        if let index = state.masks.firstIndex(where: { $0.id == maskId }) {
            state.masks.remove(at: index)
        }
        selectedMaskId = restoreSelection
        dragSnapshot = nil
        pendingLabel = nil
        maskInteracting = false
        scheduleRender(immediate: true)
        scheduleMaskAssetsRefresh(immediate: true)
    }

    /// 取消放置（切工具 / 切蒙版时调用）。
    func cancelMaskPlacement() {
        maskPlacementKind = nil
    }

    // MARK: - 智能蒙版（Vision）

    /// 正在生成中的智能蒙版 id（UI 显示转圈）。
    @Published private(set) var smartMaskGenerating: Set<String> = []

    // MARK: - 项目（编辑 Tab 的「＋」创建）

    /// 当前项目（nil = 设备单张模式：不显示同步、不做左右切换、不自动基础调色）。
    @Published private(set) var project: Project?

    /// 绑定 / 解除项目上下文（进入项目编辑 / 回到单张编辑时调用）。
    func configureProject(_ project: Project?) {
        self.project = project
    }

    // MARK: - AI 修图（v1.5.0）

    /// 修图识别结果（一键修图时填充）。
    @Published private(set) var retouchAnalysis: RetouchAnalysisUi?
    /// 修图阶段提示（nil = 空闲；非 nil 时显示全屏加载动画）。
    @Published private(set) var retouchStage: String?
    /// 是否已有修图结果（UI 显示「已修图」）。
    @Published private(set) var retouchAppliedReady = false

    /// 修图后的基础缓冲（渲染与展示都用它；nil = 用原图）。
    private var retouchBuffer: RgbaBuffer?
    /// 已应用的修图参数（避免重复计算）。
    private var retouchApplied: RetouchUi?
    private var retouchWorkInFlight = false
    private var retouchPending = false
    private var retouchDebounce: DispatchWorkItem?
    private var retouchStageTimer: Timer?
    private var retouchStageIndex = 0
    private var retouchPresetToken = 0
    private let retouchQueue = DispatchQueue(label: "fdxmp.ios.retouch", qos: .userInitiated)

    /// 一键修图的阶段文案（核心 AI 进度不可用时的兜底轮播；加载动画用）。
    private let retouchStages = [
        "正在识别五官…",
        "AI 修复瑕疵…",
        "皮肤美化…",
        "肤色统一…",
        "五官精修…",
        "脸部 / 身体塑形…",
    ]

    // MARK: 手动工具（污点修复 / 液化）

    /// 手动祛痘笔的污点列表（归一化坐标 + 半径像素 + 强度）。
    @Published private(set) var healSpots: [HealSpot] = []
    /// 液化笔画列表（归一化坐标）。
    @Published private(set) var liquifyStrokes: [LiquifyStrokeUi] = []
    /// 污点修复 / 液化进行中（小加载提示）。
    @Published private(set) var manualWorkStage: String?

    /// 修图子模式：美颜（预设 + 参数）/ 污点修复 / 液化。
    enum RetouchModeUi: String, CaseIterable, Identifiable {
        case beauty = "预设"
        case spot = "修复"
        case liquify = "液化"

        var id: String { rawValue }
    }

    @Published var retouchMode: RetouchModeUi = .beauty
    /// 笔刷半径（长边百分比，1..20）。
    @Published var brushRadius: Float = 6
    /// 笔刷强度（1..100）。
    @Published var brushStrength: Float = 70
    /// 修图预设面板：当前分类。
    @Published var retouchPresetCategory: RetouchPresetCategory = .hot
    /// 当前选中的修图预设（nil = 未选 / 原图）。
    @Published private(set) var activeRetouchPreset: RetouchPresetUi?
    /// 预设强度 0..100（0 = 原图，100 = 预设原样）——就是"一键修图"的强度。
    @Published var retouchStrength: Float = 0
    /// 修图预设缩略图（预设 id → 小图；核心实时渲染）。
    @Published private(set) var retouchPresetThumbnails: [String: UIImage] = [:]
    /// 工作台切换动画文案（修图 ⇄ 调色，nil = 空闲）。
    @Published private(set) var workbenchStage: String?

    struct HealSpot: Equatable {
        var x: Float
        var y: Float
        var radius: Float
        var strength: Float
    }

    struct LiquifyStrokeUi: Equatable {
        var points: [CGPoint]
        var radius: Float
        var strength: Float

        var jsonObject: [String: Any] {
            [
                "points": points.map { [Double($0.x), Double($0.y)] },
                "radius": Double(radius),
                "strength": Double(strength),
            ]
        }
    }

    /// 修复 / 液化后的基础缓冲（链：baseBuffer → 污点 → 液化 → 修图参数）。
    private var healBuffer: RgbaBuffer?
    private var liquifyBuffer: RgbaBuffer?

    /// 修图链的输入缓冲（污点 → 液化 → 原图）。
    private var retouchInputBuffer: RgbaBuffer? { liquifyBuffer ?? healBuffer ?? baseBuffer }

    /// 手动污点修复（点的归一化坐标；radius 相对长边的像素）。
    func applyHeal(at point: CGPoint, radius: Float, strength: Float) {
        guard let base = retouchInputBuffer, manualWorkStage == nil else { return }
        let spot = HealSpot(
            x: Float(point.x),
            y: Float(point.y),
            radius: radius,
            strength: strength
        )
        healSpots.append(spot)
        manualWorkStage = "污点修复中…"
        Haptics.light()
        retouchQueue.async { [weak self] in
            guard let self else { return }
            let output = NativeCore.shared.healApply(
                buffer: base,
                x: spot.x,
                y: spot.y,
                radius: spot.radius,
                strength: spot.strength
            )
            Task { @MainActor in
                self.manualWorkStage = nil
                guard let output else {
                    self.showMessage("污点修复失败：\(NativeCore.shared.lastError() ?? "未知原因")")
                    self.healSpots.removeLast()
                    return
                }
                self.healBuffer = output
                self.recomputeRetouch(force: true)
            }
        }
    }

    /// 撤销上一个污点（从原图重建整条链）。
    func undoLastHeal() {
        guard !healSpots.isEmpty, manualWorkStage == nil else { return }
        healSpots.removeLast()
        rebuildManualChain()
    }

    /// 应用一笔液化（宽/高归一的笔画点）。
    func applyLiquifyStroke(points: [CGPoint], radius: Float, strength: Float) {
        guard let base = retouchInputBuffer, points.count >= 2, manualWorkStage == nil else {
            return
        }
        let stroke = LiquifyStrokeUi(points: points, radius: radius, strength: strength)
        liquifyStrokes.append(stroke)
        manualWorkStage = "液化处理中…"
        Haptics.light()
        retouchQueue.async { [weak self] in
            guard let self else { return }
            let json = Self.strokesJson([stroke])
            let output = NativeCore.shared.liquifyApply(buffer: base, strokesJson: json)
            Task { @MainActor in
                self.manualWorkStage = nil
                guard let output else {
                    self.showMessage("液化失败：\(NativeCore.shared.lastError() ?? "未知原因")")
                    self.liquifyStrokes.removeLast()
                    return
                }
                self.liquifyBuffer = output
                self.recomputeRetouch(force: true)
            }
        }
    }

    /// 撤销最近一笔液化。
    func undoLastLiquify() {
        guard !liquifyStrokes.isEmpty, manualWorkStage == nil else { return }
        liquifyStrokes.removeLast()
        rebuildManualChain()
    }

    /// 从原图重建「污点 → 液化」链（撤销 / 清空时用）。
    private func rebuildManualChain() {
        guard let base = baseBuffer else { return }
        let spots = healSpots
        let strokes = liquifyStrokes
        manualWorkStage = "重建修图链…"
        retouchQueue.async { [weak self] in
            guard let self else { return }
            var buffer = base
            for spot in spots {
                if let output = NativeCore.shared.healApply(
                    buffer: buffer,
                    x: spot.x,
                    y: spot.y,
                    radius: spot.radius,
                    strength: spot.strength
                ) {
                    buffer = output
                }
            }
            let healed = spots.isEmpty ? nil : buffer
            if !strokes.isEmpty,
               let output = NativeCore.shared.liquifyApply(
                   buffer: buffer,
                   strokesJson: Self.strokesJson(strokes)
               ) {
                buffer = output
            }
            let liquified = strokes.isEmpty ? nil : buffer
            Task { @MainActor in
                self.manualWorkStage = nil
                self.healBuffer = healed
                self.liquifyBuffer = liquified
                self.recomputeRetouch(force: true)
            }
        }
    }

    /// 清空手动工具（污点 / 液化）。
    func clearManualTools() {
        guard !healSpots.isEmpty || !liquifyStrokes.isEmpty else { return }
        healSpots.removeAll()
        liquifyStrokes.removeAll()
        rebuildManualChain()
    }

    private static func strokesJson(_ strokes: [LiquifyStrokeUi]) -> String {
        let array = strokes.map(\.jsonObject)
        guard
            let data = try? JSONSerialization.data(withJSONObject: array, options: [.sortedKeys]),
            let text = String(data: data, encoding: .utf8)
        else {
            return "[]"
        }
        return text
    }

    /// 准备 Vision 平台数据（MODNet 不随包）：
    /// 人像蒙版 α + 人脸 / 身体关键点（姿态），提交给核心供修图 / 塑形使用。
    private func prepareVisionPersonMask() {
        guard let buffer = baseBuffer else { return }
        guard let cgImage = SmartMaskEngine.makeCGImage(buffer: buffer) else { return }
        retouchQueue.async { [weak self] in
            guard let self else { return }
            if let plane = SmartMaskEngine.personMaskPlane(cgImage: cgImage, buffer: buffer) {
                _ = NativeCore.shared.retouchSubmitPersonMask(
                    buffer: buffer,
                    plane: plane.data,
                    planeWidth: plane.width,
                    planeHeight: plane.height
                )
            }
            if let pose = SmartMaskEngine.platformPoseJson(cgImage: cgImage) {
                _ = NativeCore.shared.retouchSubmitPose(buffer: buffer, poseJson: pose)
            }
            _ = self
        }
    }

    /// 渲染 / 显示用的当前源图（修图后 = 修图结果，否则原图）。
    private var renderSource: RgbaBuffer? {
        retouchBuffer ?? liquifyBuffer ?? healBuffer ?? baseBuffer
    }

    /// 切换照片时重置修图状态（核心按图像指纹自动失效识别结果）。
    private func resetRetouchState() {
        retouchStageTimer?.invalidate()
        retouchStageTimer = nil
        retouchStage = nil
        retouchBuffer = nil
        retouchApplied = nil
        retouchAppliedReady = false
        retouchAnalysis = nil
        retouchWorkInFlight = false
        retouchPending = false
        healSpots.removeAll()
        liquifyStrokes.removeAll()
        healBuffer = nil
        liquifyBuffer = nil
        manualWorkStage = nil
        retouchPresetThumbnails = [:]
    }

    /// 载入完成后：若该图有已保存的修图参数 → 后台重算；否则保持原图。
    private func prepareRetouchAfterLoad() {
        guard state.retouch.isActive else {
            retouchApplied = state.retouch
            retouchBuffer = nil
            retouchAppliedReady = false
            return
        }
        // 新图第一次应用修图要重新识别五官：给一段加载动画。
        startRetouchStages()
        recomputeRetouch(completion: { self.stopRetouchStages() })
    }

    /// 一键修图：识别五官（加载动画）→ 应用默认强度。
    func oneTapRetouch() {
        guard let buffer = baseBuffer, retouchStage == nil else { return }
        Haptics.light()
        startRetouchStages()
        let source = buffer
        retouchQueue.async { [weak self] in
            guard let self else { return }
            var analysis: RetouchAnalysisUi?
            if let json = NativeCore.shared.retouchAnalyze(buffer: source) {
                analysis = RetouchAnalysisUi.from(json: json)
            }
            Task { @MainActor in
                self.retouchAnalysis = analysis
                let params = RetouchUi.autoDefaults(
                    faces: analysis?.faces ?? 0,
                    landmarks: analysis?.landmarks ?? 0,
                    person: analysis?.person ?? true
                )
                self.applyEdit("一键修图") { state in
                    state.retouch = params
                }
                self.recomputeRetouch(completion: { self.stopRetouchStages() })
                if let analysis {
                    if analysis.faces == 0 {
                        self.showMessage("未检测到人脸：已跳过脸部修图（磨皮 / 肤色按全图处理）")
                    } else if !analysis.hasLandmarks {
                        self.showMessage("五官识别不完整：已跳过脸部塑形")
                    } else {
                        self.showMessage("一键修图完成：可切到「编辑」Tab 继续调色")
                    }
                }
            }
        }
    }

    /// 滑杆实时调整修图参数（拖动中防抖重算；松手由面板调用 [`recomputeRetouch`] 立即重算）。
    func updateRetouch(_ transform: (inout RetouchUi) -> Void) {
        var params = state.retouch
        transform(&params)
        state.retouch = params
        retouchDebounce?.cancel()
        let work = DispatchWorkItem { [weak self] in
            self?.recomputeRetouch()
        }
        retouchDebounce = work
        retouchQueue.asyncAfter(deadline: .now() + 0.35, execute: work)
    }

    /// 修图参数重置（回到未修图）。
    func resetRetouch() {
        guard state.retouch.isActive else { return }
        applyEdit("重置修图") { state in
            state.retouch = RetouchUi()
        }
        recomputeRetouch()
    }

    /// 应用修图预设（= 一键修图：修图参数 + 配套调色；"原图" = 清空）。
    func applyRetouchPreset(_ preset: RetouchPresetUi) {
        guard manualWorkStage == nil, workbenchStage == nil else { return }
        Haptics.light()
        if preset.id == RetouchPresetUi.original.id {
            activeRetouchPreset = nil
            retouchStrength = 0
            applyEdit("预设·原图") { state in
                state.retouch = RetouchUi()
                state.values = EditorSpec.defaultValues()
            }
            recomputeRetouch(force: true)
            return
        }
        activeRetouchPreset = preset
        retouchStrength = 100
        applyEdit("预设·\(preset.name)") { state in
            state.retouch = preset.retouch
            var values = EditorSpec.defaultValues()
            for (key, value) in preset.color {
                values[key] = value
            }
            state.values = values
        }
        // 首次要识别五官（加载动画），之后是纯参数重算。
        startRetouchStages()
        recomputeRetouch(completion: { self.stopRetouchStages() }, force: true)
    }

    /// 预设强度（拖动实时；0 = 原图，100 = 预设原样）——即"一键修图"的强度。
    func updateRetouchStrength(_ value: Float) {
        retouchStrength = value
        guard let preset = activeRetouchPreset else { return }
        let scale = value / 100
        var params = preset.retouch
        params.blemish *= scale
        params.smoothLow *= scale
        params.smoothMid *= scale
        params.smoothHigh *= scale
        params.skinTone *= scale
        params.restore *= scale
        params.faceSlim *= scale
        params.chin *= scale
        params.eyeEnlarge *= scale
        params.noseSlim *= scale
        params.bodySlim *= scale
        state.retouch = params
        var values = EditorSpec.defaultValues()
        for (key, presetValue) in preset.color {
            values[key] = presetValue * scale
        }
        state.values = values
        retouchDebounce?.cancel()
        let work = DispatchWorkItem { [weak self] in
            self?.recomputeRetouch(force: true)
        }
        retouchDebounce = work
        retouchQueue.asyncAfter(deadline: .now() + 0.35, execute: work)
    }

    /// 修图预设缩略图（后台核心渲染：小图先修图、再过该预设的调色）。
    func refreshRetouchPresetThumbnails() {
        guard let buffer = baseBuffer else { return }
        let small = ImageUtil.downscaled(buffer: buffer, maxEdge: 240) ?? buffer
        let presets = [RetouchPresetUi.original] + RetouchPresetUi.catalog
        retouchPresetToken &+= 1
        let token = retouchPresetToken
        maskAssetQueue.async { [weak self] in
            var thumbs: [String: UIImage] = [:]
            for preset in presets {
                let source: RgbaBuffer
                if preset.retouch.isActive {
                    // 缩略图只跑轻量经典修图（240px 下 AI 修复 / 增强不可见，且会显著拖慢渲染）。
                    var thumbParams = preset.retouch
                    thumbParams.restore = 0
                    thumbParams.blemish = 0
                    guard
                        let retouched = NativeCore.shared.retouchApply(
                            paramsJson: thumbParams.jsonString,
                            buffer: small
                        )
                    else {
                        continue
                    }
                    source = retouched
                } else {
                    source = small
                }
                var presetState = EditState()
                for (key, value) in preset.color {
                    presetState.values[key] = value
                }
                let json = EditJSON.build(state: presetState, applyCropRect: false)
                guard
                    let result = NativeCore.shared.render(buffer: source, paramsJson: json),
                    let image = ImageUtil.image(
                        from: RgbaBuffer(
                            pixels: result.pixels,
                            width: result.width,
                            height: result.height
                        )
                    )
                else {
                    continue
                }
                thumbs[preset.id] = image
            }
            Task { @MainActor in
                guard let self, self.retouchPresetToken == token else { return }
                guard !thumbs.isEmpty else { return }
                self.retouchPresetThumbnails = thumbs
            }
        }
    }

    /// 工作台切换（修图 ⇄ 调色）：带加载动画；进入修图时先把修图缓冲算好。
    func switchWorkbench() {
        guard workbenchStage == nil else { return }
        Haptics.tick()
        if tool == .retouch {
            // 返回调色工作台：同样先给一段加载动画（修图结果留在缓冲区，切回即用）。
            workbenchStage = "正在返回调色工作台…"
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { [weak self] in
                guard let self else { return }
                self.selectTool(.edit)
                self.workbenchStage = nil
            }
            return
        }
        workbenchStage = "正在进入修图工作台…"
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { [weak self] in
            guard let self else { return }
            self.selectTool(.retouch)
            let needsWork = self.state.retouch.isActive
                || self.healBuffer != nil
                || self.liquifyBuffer != nil
            if needsWork {
                self.recomputeRetouch(completion: {
                    self.workbenchStage = nil
                    self.refreshRetouchPresetThumbnails()
                }, force: true)
            } else {
                self.workbenchStage = nil
                self.refreshRetouchPresetThumbnails()
            }
        }
    }

    /// 导出 / 细节帧用：在给定分辨率上应用修图。
    ///
    /// 超过 4096 长边时先降采样再上采样（保内存；修图主要作用于皮肤，损失可接受）。
    static func applyRetouchForExport(buffer: RgbaBuffer, params: RetouchUi) -> RgbaBuffer? {
        guard params.isActive else { return buffer }
        let longEdge = max(buffer.width, buffer.height)
        let limit = 4096
        if longEdge <= limit {
            return NativeCore.shared.retouchApply(paramsJson: params.jsonString, buffer: buffer)
        }
        let scale = Double(limit) / Double(longEdge)
        let width = max(1, Int((Double(buffer.width) * scale).rounded()))
        let height = max(1, Int((Double(buffer.height) * scale).rounded()))
        guard
            let small = ImageUtil.scaled(buffer: buffer, toWidth: width, toHeight: height),
            let retouched = NativeCore.shared.retouchApply(
                paramsJson: params.jsonString,
                buffer: small
            )
        else {
            return nil
        }
        return ImageUtil.scaled(
            buffer: retouched,
            toWidth: buffer.width,
            toHeight: buffer.height
        )
    }

    /// 重算修图缓冲（后台；完成后重建 GPU 会话并刷新预览）。
    func recomputeRetouch(completion: (() -> Void)? = nil, force: Bool = false) {
        guard let buffer = retouchInputBuffer else {
            completion?()
            return
        }
        let params = state.retouch
        if params == retouchApplied, !force {
            completion?()
            return
        }
        if retouchWorkInFlight {
            retouchPending = true
            completion?()
            return
        }
        retouchWorkInFlight = true
        let json = params.jsonString
        retouchQueue.async { [weak self] in
            guard let self else { return }
            var output: RgbaBuffer?
            if !params.isIdentity {
                output = NativeCore.shared.retouchApply(paramsJson: json, buffer: buffer)
            }
            Task { @MainActor in
                self.retouchWorkInFlight = false
                if params.isIdentity {
                    self.retouchBuffer = nil
                    self.retouchAppliedReady = false
                } else if let output {
                    self.retouchBuffer = output
                    self.retouchAppliedReady = true
                } else if self.message == nil {
                    self.showMessage("修图失败：\(NativeCore.shared.lastError() ?? "未知原因")")
                }
                self.retouchApplied = params
                // GPU 会话按修图结果重建；CPU 路径下一帧自动走新缓冲。
                self.clearGpuSession()
                if let source = self.renderSource {
                    self.createGpuSession(for: source)
                }
                self.scheduleRender(immediate: true)
                self.scheduleMaskAssetsRefresh(immediate: true)
                self.refreshDetailIfNeeded()
                completion?()
                if self.retouchPending {
                    self.retouchPending = false
                    self.recomputeRetouch()
                }
            }
        }
    }

    /// 启动阶段动画（识别 → 去瑕疵 → 磨皮 → 肤色 → 塑形）。
    ///
    /// 优先显示核心实时进度（`fdxmp_retouch_progress`：AI 人脸增强 / AI 修复瑕疵 / …），
    /// 核心空闲时轮播兜底文案。
    private func startRetouchStages() {
        retouchStageIndex = 0
        retouchStage = retouchStages[0]
        retouchStageTimer?.invalidate()
        retouchStageTimer = Timer.scheduledTimer(withTimeInterval: 0.4, repeats: true) { [weak self] _ in
            guard let self else { return }
            let progress = NativeCore.shared.retouchProgress()
            if !progress.isEmpty {
                self.retouchStage = progress
                return
            }
            if self.retouchStageIndex + 1 < self.retouchStages.count {
                self.retouchStageIndex += 1
                self.retouchStage = self.retouchStages[self.retouchStageIndex]
            }
        }
    }

    private func stopRetouchStages() {
        retouchStageTimer?.invalidate()
        retouchStageTimer = nil
        retouchStage = nil
    }

    // MARK: - 项目同步 / 基础调色

    /// 同步范围（项目内「同步」菜单）。
    enum SyncScope: String, CaseIterable, Identifiable {
        case all = "调色 + 修图"
        case retouchOnly = "仅修图"
        case colorOnly = "仅调色"

        var id: String { rawValue }
    }

    /// 把当前编辑同步到项目内其余照片（返回同步张数）。
    ///
    /// 调色 + 修图一起同步；裁切与蒙版几何是每张照片自己的构图 / 笔迹，保持不动。
    @discardableResult
    func syncEdits(scope: SyncScope) -> Int {
        guard let project, let current = asset else { return 0 }
        let targets = project.assetIds.filter { $0 != current.localIdentifier }
        return syncEdits(scope: scope, to: targets)
    }

    /// 把当前编辑同步到指定照片（底部弹框里的多选同步）。
    @discardableResult
    func syncEdits(scope: SyncScope, to assetIds: [String]) -> Int {
        guard let current = asset else { return 0 }
        var synced = 0
        for assetId in assetIds where assetId != current.localIdentifier {
            var target: EditState
            if let json = EditStore.json(assetId: assetId) {
                target = EditJSON.parse(json)
            } else {
                target = EditState()
            }
            switch scope {
            case .all:
                target.values = state.values
                target.curves = state.curves
                target.hsl = state.hsl
                target.grading = state.grading
                target.removeChromaticAberration = state.removeChromaticAberration
                target.profileCorrection = state.profileCorrection
                target.retouch = state.retouch
            case .retouchOnly:
                target.retouch = state.retouch
            case .colorOnly:
                target.values = state.values
                target.curves = state.curves
                target.hsl = state.hsl
                target.grading = state.grading
                target.removeChromaticAberration = state.removeChromaticAberration
                target.profileCorrection = state.profileCorrection
            }
            EditStore.save(
                assetId: assetId,
                json: EditJSON.build(state: target, applyCropRect: true)
            )
            synced += 1
        }
        if synced > 0 {
            Haptics.medium()
            showMessage("已同步到项目内 \(synced) 张照片")
        }
        return synced
    }

    /// 项目内、且没有编辑记录的照片：自动做一版「基础调色」，避免原片 / RAW 发灰。
    private func applyAutoBaseToneIfNeeded() {
        guard project != nil, let buffer = baseBuffer, let current = asset else { return }
        guard EditStore.json(assetId: current.localIdentifier) == nil else { return }
        guard let tone = Self.autoBaseTone(buffer: buffer) else { return }
        var values = EditorSpec.defaultValues()
        values["exposure"] = tone.exposure
        values["temperature"] = tone.temperature
        values["tint"] = tone.tint
        values["contrast"] = tone.contrast
        values["shadows"] = tone.shadows
        values["blacks"] = tone.blacks
        values["vibrance"] = tone.vibrance
        state.values = values
        presetName = "基础调色"
        presetRef = nil
        // 自动基础调色也会改变画面：加载框要等这一版渲染出来再收。
        awaitingEditedFrame = true
        scheduleRender(immediate: true)
    }

    /// 从图像统计推一版温和的基础基调（灰度世界白平衡 + 中位亮度曝光）。
    private static func autoBaseTone(
        buffer: RgbaBuffer
    ) -> (
        exposure: Float,
        temperature: Float,
        tint: Float,
        contrast: Float,
        shadows: Float,
        blacks: Float,
        vibrance: Float
    )? {
        guard buffer.width > 8, buffer.height > 8 else { return nil }
        var sumR = 0.0
        var sumG = 0.0
        var sumB = 0.0
        var samples = 0.0
        var histogram = [Int](repeating: 0, count: 256)
        let step = 16
        var y = 0
        while y < buffer.height {
            var x = 0
            while x < buffer.width {
                let index = (y * buffer.width + x) * 4
                let r = Double(buffer.pixels[index]) / 255.0
                let g = Double(buffer.pixels[index + 1]) / 255.0
                let b = Double(buffer.pixels[index + 2]) / 255.0
                sumR += r
                sumG += g
                sumB += b
                let luma = Int((0.299 * r + 0.587 * g + 0.114 * b) * 255.0)
                histogram[min(max(luma, 0), 255)] += 1
                samples += 1
                x += step
            }
            y += step
        }
        guard samples > 100 else { return nil }
        let meanR = sumR / samples
        let meanG = sumG / samples
        let meanB = sumB / samples
        let mean = max((meanR + meanG + meanB) / 3.0, 0.02)
        let half = Int(samples / 2)
        var seen = 0
        var median = 0.5
        for (index, count) in histogram.enumerated() {
            seen += count
            if seen >= half {
                median = Double(index) / 255.0
                break
            }
        }
        let exposure = Float(min(max(log2(0.45 / max(median, 0.05)), -1.2), 1.2))
        let temperature = Float(min(max((meanB - meanR) / mean * 90.0, -30), 30))
        let tint = Float(min(max(((meanR + meanB) / 2.0 - meanG) / mean * 90.0, -25), 25))
        return (exposure, temperature, tint, 6, 8, -4, 6)
    }

    /// 生成失败的智能蒙版 id → 原因（避免自动重试死循环；可手动「重新生成」）。
    @Published private(set) var smartMaskFailed: [String: String] = [:]

    /// 补生成缺失的智能蒙版（加载图片 / 恢复编辑记录 / 撤销重做后调用）。
    func ensureSmartMasksReady() {
        guard baseBuffer != nil else { return }
        let paramsJson = currentJson(applyCropRect: true)
        for mask in state.masks where mask.kind.isSmart {
            guard !smartMaskGenerating.contains(mask.id) else { continue }
            guard smartMaskFailed[mask.id] == nil else { continue }
            if !NativeCore.shared.maskRasterReady(paramsJson: paramsJson, maskId: mask.id) {
                generateSmartMask(mask.id)
            }
        }
    }

    /// 手动重新生成（清除失败标记）。
    func retrySmartMask(_ id: String) {
        smartMaskFailed.removeValue(forKey: id)
        generateSmartMask(id)
    }

    /// 为指定的智能蒙版生成权重（后台线程：Vision 分析 → 提交核心缓存）。
    func generateSmartMask(_ id: String) {
        guard let mask = state.masks.first(where: { $0.id == id }), mask.kind.isSmart else {
            return
        }
        guard let buffer = baseBuffer else { return }
        guard !smartMaskGenerating.contains(id) else { return }
        let paramsJson = currentJson(applyCropRect: true)
        smartMaskGenerating.insert(id)
        showMessage("正在生成\(mask.kind.rawValue)…")
        let kind = mask.kind
        let maskId = mask.id
        maskAssetQueue.async { [weak self] in
            var failure: String?
            var ready = false
            // 1) 随包 ONNX 模型（SCRFD / MODNet / BiSeNet / U²-Net）：主体、人像与五官。
            let modelCode = NativeCore.shared.smartMaskGenerate(
                paramsJson: paramsJson,
                maskId: maskId,
                buffer: buffer
            )
            if modelCode == 0 {
                ready = true
            } else {
                let modelMessage = NativeCore.shared.lastError() ?? "模型不可用"
                // 2) 回退 Apple Vision（系统端侧模型；模型未随包 / 该类型无模型时）。
                do {
                    switch try SmartMaskEngine.generate(kind: kind, buffer: buffer) {
                    case let .plane(width, height, data):
                        ready = NativeCore.shared.submitMaskRaster(
                            paramsJson: paramsJson,
                            maskId: maskId,
                            width: width,
                            height: height,
                            weights: data
                        )
                    case let .polygons(include, exclude):
                        ready = NativeCore.shared.submitMaskPolygons(
                            paramsJson: paramsJson,
                            maskId: maskId,
                            width: buffer.width,
                            height: buffer.height,
                            polygonsJson: SmartMaskPolygons.json(include: include, exclude: exclude),
                            feather: 2.0
                        )
                    }
                    if !ready {
                        failure = "模型：\(modelMessage)；Vision：\(NativeCore.shared.lastError() ?? "提交失败")"
                    }
                } catch {
                    failure = "模型：\(modelMessage)；Vision：\(error.localizedDescription)"
                }
            }
            Task { @MainActor in
                guard let self else { return }
                self.smartMaskGenerating.remove(maskId)
                if let failure {
                    self.smartMaskFailed[maskId] = failure
                    self.showMessage("智能蒙版生成失败：\(failure)")
                } else {
                    self.smartMaskFailed.removeValue(forKey: maskId)
                    self.scheduleRender(immediate: true)
                    self.scheduleMaskAssetsRefresh(immediate: true)
                }
            }
        }
    }

    /// 复制选中蒙版（插在原蒙版之后）。
    func duplicateSelectedMask() {
        guard let id = selectedMaskId, let source = state.masks.first(where: { $0.id == id })
        else {
            return
        }
        var copy = source
        copy.id = UUID().uuidString
        copy.name = "\(source.name) 副本"
        applyEdit("复制蒙版") { state in
            if let index = state.masks.firstIndex(where: { $0.id == id }) {
                state.masks.insert(copy, at: index + 1)
            } else {
                state.masks.append(copy)
            }
        }
        selectedMaskId = copy.id
    }

    /// 删除选中蒙版。
    func deleteSelectedMask() {
        guard let id = selectedMaskId else { return }
        applyEdit("删除蒙版") { state in
            state.masks.removeAll { $0.id == id }
        }
        selectedMaskId = state.masks.first?.id
        brushErase = false
    }

    /// 重命名选中蒙版。
    func renameSelectedMask(_ name: String) {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let id = selectedMaskId else { return }
        applyEdit("重命名蒙版") { state in
            if let index = state.masks.firstIndex(where: { $0.id == id }) {
                state.masks[index].name = trimmed
            }
        }
    }

    /// 显示 / 隐藏某个蒙版（隐藏后不参与渲染与叠加）。
    func toggleMaskVisible(_ id: String) {
        applyEdit("切换蒙版显示") { state in
            if let index = state.masks.firstIndex(where: { $0.id == id }) {
                state.masks[index].visible.toggle()
            }
        }
    }

    /// 反转选中蒙版。
    func invertSelectedMask() {
        guard let id = selectedMaskId else { return }
        applyEdit("反转蒙版") { state in
            if let index = state.masks.firstIndex(where: { $0.id == id }) {
                state.masks[index].inverted.toggle()
            }
        }
    }

    /// 重置选中蒙版：清空笔迹、几何回默认、局部调整归零（保留名称与可见性）。
    func resetSelectedMask() {
        guard let id = selectedMaskId,
              let source = state.masks.first(where: { $0.id == id })
        else {
            return
        }
        var fresh = MaskUi(id: source.id, name: source.name, kind: source.kind)
        fresh.visible = source.visible
        switch source.kind {
        case .linear:
            fresh.start = CGPoint(x: 0.5, y: 0.32)
            fresh.end = CGPoint(x: 0.5, y: 0.62)
        case .radial:
            fresh.center = CGPoint(x: 0.5, y: 0.5)
            fresh.radii = CGSize(width: 0.42, height: 0.3)
        case .brush:
            fresh.brushSize = source.brushSize
            fresh.brushFeather = source.brushFeather
            fresh.brushFlow = source.brushFlow
            fresh.brushDensity = source.brushDensity
        case .aiSubject, .aiPortrait, .aiFace, .aiSkin,
            .aiEyes, .aiEyebrows, .aiNose, .aiLips, .aiTeeth:
            // 智能蒙版重置：栅格与形状无关，保留即可（不重新推理）。
            fresh.visible = source.visible
        }
        applyEdit("重置蒙版") { state in
            if let index = state.masks.firstIndex(where: { $0.id == id }) {
                state.masks[index] = fresh
            }
        }
    }

    /// 撤销画笔的最后一笔（不改动其它参数）。
    func undoLastStroke() {
        guard let id = selectedMaskId,
              let mask = state.masks.first(where: { $0.id == id }),
              !mask.strokes.isEmpty
        else {
            return
        }
        applyEdit("撤销一笔") { state in
            if let index = state.masks.firstIndex(where: { $0.id == id }) {
                state.masks[index].strokes.removeLast()
            }
        }
    }

    // MARK: - 蒙版设置复制 / 粘贴

    /// 蒙版设置剪贴板（跨蒙版 / 跨照片）。
    private var maskClipboard: MaskUi?

    /// 是否有可粘贴的蒙版设置。
    var canPasteMaskSettings: Bool {
        maskClipboard != nil && selectedMask != nil
    }

    /// 复制选中蒙版的设置（形状 + 局部调整）。
    func copySelectedMaskSettings() {
        guard let mask = selectedMask else { return }
        maskClipboard = mask
    }

    /// 粘贴设置到选中蒙版：类型一致时连形状一起套用，否则只套用局部调整。
    func pasteMaskSettings() {
        guard let clipboard = maskClipboard, let id = selectedMaskId else { return }
        let source = state.masks.first { $0.id == id }
        let sameKind = source?.kind == clipboard.kind
        applyEdit("粘贴蒙版设置") { state in
            guard let index = state.masks.firstIndex(where: { $0.id == id }) else { return }
            let name = state.masks[index].name
            let savedId = state.masks[index].id
            if sameKind {
                var pasted = clipboard
                pasted.id = savedId
                pasted.name = name
                state.masks[index] = pasted
            } else {
                state.masks[index].values = clipboard.values
                state.masks[index].opacity = clipboard.opacity
                state.masks[index].inverted = clipboard.inverted
            }
        }
    }

    func selectMask(_ id: String) {
        // 选别的蒙版 = 放弃待放置的新蒙版。
        maskPlacementKind = nil
        guard selectedMaskId != id else { return }
        selectedMaskId = id
        brushErase = false
        brushCursor = nil
        scheduleMaskAssetsRefresh(immediate: true)
    }

    /// 直接修改选中蒙版（面板拖动 / 画布拖动中调用：静默，不落历史）。
    func mutateSelectedMask(_ transform: (inout MaskUi) -> Void) {
        guard let id = selectedMaskId, let index = state.masks.firstIndex(where: { $0.id == id })
        else {
            return
        }
        transform(&state.masks[index])
    }

    /// 画布拖动中的实时更新：快速预览渲染 + 叠加层节流刷新。
    func updateSelectedMask(_ transform: (inout MaskUi) -> Void) {
        mutateSelectedMask(transform)
        touchMaskLiveEdit()
    }

    /// 更新选中蒙版的调色参数（滑块）。
    func updateMaskValue(_ key: String, _ newValue: Float) {
        beginEdit()
        pendingLabel = "蒙版 · \(EditorSpec.sliders.first { $0.key == key }?.label ?? "调整")"
        mutateSelectedMask { $0.values[key] = newValue }
        touchEdit()
    }

    // MARK: - 蒙版画布交互（手势层驱动）

    /// 开始拖动蒙版控件：记录快照，拖动期间用快速预览渲染。
    func beginMaskDrag(label: String = "调整蒙版") {
        guard selectedMask != nil else { return }
        pendingLabel = label
        maskInteracting = true
        beginEdit()
    }

    /// 结束拖动（整次拖动记一步历史）。
    func endMaskDrag() {
        finishMaskInteraction()
    }

    /// 落笔：生成一条笔画（参数取当前笔刷设置快照）。
    func beginBrushStroke(at point: CGPoint) {
        guard let id = selectedMaskId,
              let index = state.masks.firstIndex(where: { $0.id == id }),
              state.masks[index].kind == .brush
        else {
            return
        }
        beginEdit()
        pendingLabel = brushErase ? "擦除" : "画笔"
        maskInteracting = true
        let mask = state.masks[index]
        let start = Self.quantized(point)
        state.masks[index].strokes.append(
            BrushStrokeUi(
                points: [start],
                erase: brushErase,
                size: mask.brushSize,
                feather: mask.brushFeather,
                flow: mask.brushFlow,
                density: mask.brushDensity
            )
        )
        brushCursor = point
        touchMaskLiveEdit()
    }

    /// 涂抹中：追加点（距离阈值抽稀）。
    func extendBrushStroke(to point: CGPoint) {
        guard let id = selectedMaskId,
              let index = state.masks.firstIndex(where: { $0.id == id }),
              state.masks[index].kind == .brush,
              var stroke = state.masks[index].strokes.last
        else {
            return
        }
        let point = Self.quantized(point)
        if let last = stroke.points.last, hypot(last.x - point.x, last.y - point.y) < 0.003 {
            return
        }
        stroke.points.append(point)
        state.masks[index].strokes[state.masks[index].strokes.count - 1] = stroke
        brushCursor = point
        touchMaskLiveEdit()
    }

    /// 抬笔：整笔记一步历史。
    func endBrushStroke() {
        // 抬笔后保留笔刷光标位置（仍显示大小圆），方便接着画。
        finishMaskInteraction()
    }

    /// 笔画坐标量化到 3 位小数（缩小项目 JSON 体积）。
    private static func quantized(_ point: CGPoint) -> CGPoint {
        CGPoint(
            x: (point.x * 1000).rounded() / 1000,
            y: (point.y * 1000).rounded() / 1000
        )
    }

    /// 结束蒙版交互：提交历史 + 全分辨率渲染。
    private func finishMaskInteraction() {
        maskInteracting = false
        commitEdit()
    }

    /// 拖动中的实时更新（快速渲染 + 叠加层节流刷新）。
    private func touchMaskLiveEdit() {
        scheduleRender()
        scheduleMaskAssetsRefresh()
    }

    // MARK: - 蒙版叠加层 / 缩略图

    /// 请求刷新蒙版叠加层与缩略图（节流；`immediate` = 提交后立即）。
    private func scheduleMaskAssetsRefresh(immediate: Bool = false) {
        maskAssetToken &+= 1
        guard !maskAssetScheduled else { return }
        maskAssetScheduled = true
        let delay: Double = immediate ? 0 : 0.12
        maskAssetQueue.asyncAfter(deadline: .now() + delay) { [weak self] in
            Task { @MainActor in
                guard let self else { return }
                self.maskAssetScheduled = false
                self.generateMaskAssets()
            }
        }
    }

    /// 生成叠加层（选中蒙版，按预览尺寸）与缩略图（全部蒙版）—— 后台线程。
    private func generateMaskAssets() {
        let masks = state.masks
        guard !masks.isEmpty else {
            maskOverlays = [:]
            maskThumbnails = [:]
            return
        }
        // 智能蒙版：加载 / 撤销重做 / 恢复编辑记录后自动补齐缺失的栅格。
        ensureSmartMasksReady()
        let json = currentJson(applyCropRect: true)
        let size = previewImage?.size ?? originalImage?.size ?? CGSize(width: 4, height: 3)
        let overlaySize = Self.cappedSize(
            size,
            maxEdge: CGFloat(EditorLimits.maskOverlayMaxEdge)
        )
        let thumbSize = Self.cappedSize(
            size,
            maxEdge: CGFloat(EditorLimits.maskThumbnailMaxEdge)
        )
        let selected = selectedMaskId
        let wantOverlay = maskOverlayVisible
        let token = maskAssetToken
        maskAssetQueue.async { [weak self] in
            var overlays: [String: UIImage] = [:]
            var thumbnails: [String: UIImage] = [:]
            for mask in masks {
                // 叠加层收起时不用生成大图（只留缩略图）。
                let isSelected = mask.id == selected
                let wantBig = isSelected && wantOverlay
                let target = wantBig ? overlaySize : thumbSize
                guard
                    let result = NativeCore.shared.maskOverlay(
                        paramsJson: json,
                        maskId: mask.id,
                        width: Int(target.width),
                        height: Int(target.height)
                    ),
                    let image = ImageUtil.image(
                        from: RgbaBuffer(
                            pixels: result.pixels,
                            width: result.width,
                            height: result.height
                        )
                    )
                else {
                    continue
                }
                if wantBig {
                    overlays[mask.id] = image
                }
                thumbnails[mask.id] = image
            }
            Task { @MainActor in
                guard let self, self.maskAssetToken == token else { return }
                self.maskOverlays = overlays
                self.maskThumbnails = thumbnails
            }
        }
    }

    /// 等比缩到最长边（不放大）。
    private static func cappedSize(_ size: CGSize, maxEdge: CGFloat) -> CGSize {
        let longEdge = max(size.width, size.height)
        guard longEdge > maxEdge, longEdge > 0 else { return size }
        let scale = maxEdge / longEdge
        return CGSize(
            width: max(1, (size.width * scale).rounded()),
            height: max(1, (size.height * scale).rounded())
        )
    }

    // MARK: - 渲染

    /// 长按分类导航：临时把该分类参数归零（与「长按照片看原图」同款对比手感）。
    func beginCategoryPreview(_ category: EditCategory) {
        guard previewCategory != category else { return }
        previewCategory = category
        scheduleRender(immediate: true)
    }

    /// 更新沉浸拖动状态（由 `ValueSliderRow` 通过 PreferenceKey 上报）。
    func setDraggingSlider(_ info: SliderDragInfo?) {
        let label = info?.label
        let text = info?.text
        if draggingSlider != label {
            draggingSlider = label
        }
        if draggingSliderText != text {
            draggingSliderText = text
        }
    }

    /// 沉浸拖动中：除指定标签的滑块外都隐藏。
    func sliderVisible(_ label: String) -> Bool {
        draggingSlider == nil || draggingSlider == label
    }

    /// 切换分类导航（进入曲线前记住来源分类，供曲线「完成」返回）。
    func setCategory(_ category: EditCategory) {
        if category == .curves, self.category != .curves {
            categoryBeforeCurves = self.category
        }
        self.category = category
    }

    /// 曲线面板「完成」：回到进入曲线前的分类。
    func exitCurves() {
        if categoryBeforeCurves == .curves {
            categoryBeforeCurves = .light
        }
        category = categoryBeforeCurves
    }

    /// 松开分类导航：恢复预览。
    func endCategoryPreview() {
        guard previewCategory != nil else { return }
        previewCategory = nil
        scheduleRender(immediate: true)
    }

    /// 长按蒙版调整页分类：临时把该蒙版这一类的局部参数归零，松手恢复。
    func beginMaskCategoryPreview(_ category: EditCategory) {
        guard previewMaskCategory != category else { return }
        previewMaskCategory = category
        scheduleRender(immediate: true)
    }

    func endMaskCategoryPreview() {
        guard previewMaskCategory != nil else { return }
        previewMaskCategory = nil
        scheduleRender(immediate: true)
    }

    /// 渲染 / 预览用状态：长按分类时把该分类参数归零（取默认值 = 无效果）。
    private var renderState: EditState {
        guard previewCategory != nil || previewMaskCategory != nil else { return state }
        var copy = state
        if let category = previewCategory {
            switch category {
            case .light:
                for spec in EditorSpec.sliders where spec.category == .light {
                    copy.values[spec.key] = spec.defaultValue
                }
            case .color:
                for spec in EditorSpec.sliders where spec.category == .color {
                    copy.values[spec.key] = spec.defaultValue
                }
                // 颜色分类下还有 HSL / 颜色均衡器 / 颜色分级三个子页，一起归零。
                copy.hsl.reset()
                copy.equalizer.reset()
                copy.grading.reset()
            case .effects:
                for spec in EditorSpec.sliders where spec.category == .effects {
                    copy.values[spec.key] = spec.defaultValue
                }
            case .detail:
                for spec in EditorSpec.sliders where spec.category == .detail {
                    copy.values[spec.key] = spec.defaultValue
                }
            case .curves:
                copy.curves.reset()
            case .optics:
                copy.removeChromaticAberration = false
                copy.profileCorrection = false
            }
        }
        // 蒙版调整页的长按归零：只动当前蒙版的这一类局部参数。
        if let category = previewMaskCategory,
           let id = selectedMaskId,
           let index = copy.masks.firstIndex(where: { $0.id == id }) {
            for spec in EditorSpec.sliders where spec.category == category {
                copy.masks[index].values[spec.key] = spec.defaultValue
            }
        }
        return copy
    }

    /// 当前参数 JSON（裁切工具中使用未裁切预览）。
    ///
    /// `usePreview` = true 时长按分类的临时归零会生效（仅用于渲染预览；
    /// 保存 / 导出固定传 false，避免把预览状态写进去）。
    private func currentJson(applyCropRect: Bool? = nil, usePreview: Bool = true) -> String {
        EditJSON.build(
            state: usePreview ? renderState : state,
            applyCropRect: applyCropRect ?? (tool != .crop)
        )
    }

    private func scheduleRender(immediate: Bool = false) {
        guard renderSource != nil else {
            // 没有可渲染的源（载入失败）：别让加载框一直挂着。
            if awaitingFirstFrame { finishFirstFrameFailed() }
            return
        }
        // 空闲时立即渲染；正在渲染则记下待渲染，当前帧结束后自动接着渲染。
        // （此前的 140ms 防抖在持续拖动时会一直被重置，导致松手才出图。）
        if renderBusy {
            renderPending = true
        } else {
            startRender()
        }
    }

    private func startRender(fallback: Bool = false) {
        guard let buffer = renderSource else { return }
        if renderBusy {
            renderPending = true
            return
        }
        renderBusy = true
        isRendering = true
        // 这一帧属于哪次载入：换图后在途的旧帧不能覆盖新图，也不能算作新图的"首帧"。
        let generation = loadToken
        let json = currentJson()
        // 蒙版拖动 / 涂抹、以及沉浸拖动滑块时用低分辨率缓冲：
        // CPU 渲染量降约 4 倍，手感更跟手；松手提交后自动回到全分辨率预览。
        let interactive = maskInteracting || draggingSlider != nil || fallback
        let source = interactive ? (fastBuffer ?? buffer) : buffer
        let gpu = gpuSession
        // 零拷贝上屏：GPU 会话 + 图层就绪 + 非对比 / 非看原图 / 非放大查看时直接把画面推给 CAMetalLayer。
        let zeroCopy = gpu != nil && zeroCopyActive && !comparePreviewActive
            && !showOriginal && !detailZoomActive
        renderQueue.async { [weak self] in
            let started = CFAbsoluteTimeGetCurrent()
            if zeroCopy, let gpu, let size = gpu.present(paramsJson: json) {
                // 上屏路径：不读回像素，直方图也由 GPU 统计（4×256 桶）。
                let histogram = gpu.histogram()
                let millis = (CFAbsoluteTimeGetCurrent() - started) * 1000
                Task { @MainActor in
                    guard let self else { return }
                    self.renderBusy = false
                    self.isRendering = false
                    guard generation == self.loadToken else {
                        // 换图前就在跑的那一帧：别拿旧图覆盖新图，直接放行新任务。
                        if self.renderPending {
                            self.renderPending = false
                            self.startRender()
                        }
                        return
                    }
                    self.previewOutputSize = CGSize(width: size.width, height: size.height)
                    if let histogram {
                        self.histogram = histogram
                    }
                    self.renderMillis = millis
                    self.renderPath = .gpu
                    self.message = nil
                    // 渲染输出尺寸（裁切后长宽比）确定后再生成叠加层，保证对齐。
                    self.scheduleMaskAssetsRefresh()
                    self.finishLoadingOverlay()
                    if self.renderPending {
                        self.renderPending = false
                        self.startRender()
                    }
                }
                return
            }
            // GPU 优先（同一帧内失败 / 不适合时自动回退 CPU）。
            var result = gpu?.render(paramsJson: json)
            // 这一帧是不是 GPU 算的（读回路径；零拷贝上屏走上面的分支）。
            let usedGpu = result != nil
            if result == nil {
                result = NativeCore.shared.render(buffer: source, paramsJson: json)
            }
            let millis = (CFAbsoluteTimeGetCurrent() - started) * 1000
            let failure = result == nil ? NativeCore.shared.lastError() : nil
            let outputSize = result.map { CGSize(width: $0.width, height: $0.height) }
            let histogram = result.flatMap {
                HistogramData(pixels: $0.pixels, width: $0.width, height: $0.height)
            }
            let image = result.flatMap {
                ImageUtil.image(
                    from: RgbaBuffer(pixels: $0.pixels, width: $0.width, height: $0.height)
                )
            }
            let pixels = result.map {
                RgbaBuffer(pixels: $0.pixels, width: $0.width, height: $0.height)
            }
            Task { @MainActor in
                guard let self else { return }
                self.renderBusy = false
                self.isRendering = false
                if generation == self.loadToken {
                    if let image, let histogram {
                        self.previewImage = image
                        self.previewPixels = pixels
                        self.histogram = histogram
                        self.previewOutputSize = outputSize
                        self.renderMillis = millis
                        self.renderPath = usedGpu ? .gpuReadback : .cpu
                        self.message = nil
                        // 渲染输出尺寸（裁切后长宽比）确定后再生成叠加层，保证对齐。
                        self.scheduleMaskAssetsRefresh()
                        // 带当前调色的首帧已经上屏：这时才收起加载框。
                        self.finishLoadingOverlay()
                    } else if !fallback, self.fastBuffer != nil {
                        // 全分辨率渲染失败：降一档用快速缓冲重试一次，不再直接报错。
                        self.startRender(fallback: true)
                    } else {
                        self.message = failure ?? "渲染失败"
                        self.finishFirstFrameFailed()
                    }
                }
                if self.renderPending {
                    self.renderPending = false
                    self.startRender()
                }
            }
        }
    }

    // MARK: - 导出

    /// 导出格式。
    enum ExportFormat: String, CaseIterable, Identifiable {
        case jpeg = "JPEG"
        case png = "PNG"
        case heic = "HEIC"

        var id: String { rawValue }

        /// UTI（CGImageDestination 用）。
        var uti: String {
            switch self {
            case .jpeg: return "public.jpeg"
            case .png: return "public.png"
            case .heic: return "public.heic"
            }
        }

        var fileExtension: String {
            switch self {
            case .jpeg: return "jpg"
            case .png: return "png"
            case .heic: return "heic"
            }
        }
    }

    /// 按当前参数渲染高分辨率图片并保存到相册（fdxmp 相簿）。
    func export() {
        guard let asset, !isExporting else { return }
        isExporting = true
        exportProgress = 0.05
        exportShareURL = nil
        message = nil
        let format = EditorLimits.exportFormat
        let quality = EditorLimits.exportQuality
        let keepMetadata = EditorLimits.exportKeepMetadata
        // 原图分辨率导出（内存由分块渲染控制，不再按像素上限降采样）。
        let targetEdge = max(asset.pixelWidth, asset.pixelHeight)
        let crop = state.crop
        let identityGeometry = crop.rect == CGRect(x: 0, y: 0, width: 1, height: 1)
            && crop.rotation == 0
            && !crop.flipHorizontal
            && !crop.flipVertical
        let json = currentJson(applyCropRect: true, usePreview: false)
        // 修图参数（在主线程快照；导出时按导出分辨率重算）。
        let retouch = state.retouch
        // 读取原图 EXIF / GPS 等元数据（保留拍摄信息）。
        var metadata: [String: Any]?
        if keepMetadata {
            metadata = Self.loadExportMetadata(asset: asset)
        }

        // RAW：LibRaw 解调作为导出源（与预览同源）；像素量超限时自动用半尺寸解调。
        if let rawData {
            exportProgress = 0.25
            let pixels = Double(asset.pixelWidth) * Double(asset.pixelHeight)
            let fullSize = pixels > 0 && pixels <= Double(EditorLimits.rawFullSizePixelLimit)
            renderQueue.async { [weak self] in
                guard let self else { return }
                let buffer = NativeCore.shared.decodeRaw(
                    data: rawData,
                    maxEdge: targetEdge,
                    fullSize: fullSize
                )
                self.finishExport(
                    buffer: buffer,
                    paramsJson: json,
                    retouch: retouch,
                    identityGeometry: identityGeometry,
                    metadata: metadata,
                    format: format,
                    quality: quality
                )
            }
            return
        }

        requestImage(
            asset: asset,
            maxEdge: targetEdge,
            mode: .highQualityFormat
        ) { [weak self] image, _ in
            guard let self else { return }
            guard let image else {
                self.isExporting = false
                self.exportProgress = nil
                self.message = "导出失败：无法读取原图"
                return
            }
            self.exportProgress = 0.25
            self.renderQueue.async { [weak self] in
                guard let self else { return }
                let buffer = ImageUtil.rgbaBuffer(from: image, maxEdge: targetEdge)
                self.finishExport(
                    buffer: buffer,
                    paramsJson: json,
                    retouch: retouch,
                    identityGeometry: identityGeometry,
                    metadata: metadata,
                    format: format,
                    quality: quality
                )
            }
        }
    }

    /// 多选导出（底部弹框里选择照片）：当前张用当前状态，其余用各自保存的编辑记录。
    ///
    /// `enhance` = 高清增强（Real-ESRGAN ×2 超分，输出 ≤ 6144；失败自动退回原尺寸）。
    func exportAssets(_ assets: [PHAsset], enhance: Bool = false) {
        guard !assets.isEmpty, !isExporting else { return }
        isExporting = true
        exportProgress = 0.02
        exportShareURL = nil
        message = nil
        let format = EditorLimits.exportFormat
        let quality = EditorLimits.exportQuality
        let keepMetadata = EditorLimits.exportKeepMetadata
        let currentId = asset?.localIdentifier
        let currentJson = currentJson(applyCropRect: true, usePreview: false)
        let currentRetouch = state.retouch
        let total = assets.count
        renderQueue.async { [weak self] in
            guard let self else { return }
            var exported = 0
            for (index, item) in assets.enumerated() {
                defer {
                    let progress = 0.05 + 0.9 * Double(index + 1) / Double(total)
                    Task { @MainActor in self.exportProgress = progress }
                }
                let id = item.localIdentifier
                let isCurrent = id == currentId
                let saved = isCurrent ? nil : EditStore.json(assetId: id)
                let savedState = saved.map(EditJSON.parse) ?? EditState()
                let json = isCurrent
                    ? currentJson
                    : EditJSON.build(state: savedState, applyCropRect: false)
                let retouch = isCurrent ? currentRetouch : savedState.retouch
                let targetEdge = max(item.pixelWidth, item.pixelHeight)
                guard let image = Self.decodeAsset(item, maxEdge: targetEdge) else { continue }
                guard let buffer = ImageUtil.rgbaBuffer(from: image, maxEdge: targetEdge) else {
                    continue
                }
                var source = buffer
                if retouch.isActive {
                    source = Self.applyRetouchForExport(buffer: buffer, params: retouch) ?? buffer
                }
                guard let result = NativeCore.shared.render(buffer: source, paramsJson: json) else {
                    continue
                }
                var rendered = RgbaBuffer(
                    pixels: result.pixels,
                    width: result.width,
                    height: result.height
                )
                if enhance {
                    Task { @MainActor in self.exportProgress = max(self.exportProgress ?? 0, 0.4) }
                    if let enhanced = Self.enhancedExportBuffer(rendered) {
                        rendered = enhanced
                    }
                }
                guard let output = ImageUtil.image(from: rendered) else { continue }
                let metadata = keepMetadata ? Self.loadExportMetadata(asset: item) : nil
                guard
                    let url = Self.writeExportFile(
                        image: output,
                        format: format,
                        quality: quality,
                        metadata: metadata
                    )
                else {
                    continue
                }
                exported += 1
                Task { @MainActor in self.saveToLibrary(fileURL: url) }
            }
            Task { @MainActor in
                self.isExporting = false
                self.exportProgress = nil
                self.showMessage(
                    exported == total
                        ? "已导出 \(exported) 张到 fdxmp 相簿"
                        : "导出完成：\(exported)/\(total) 张（部分失败）"
                )
            }
        }
    }

    /// 导出增强（Real-ESRGAN ×2 超分；输出最长边 ≤ 6144）。
    ///
    /// 输入超限（> 4096）或模型不可用时返回 nil，调用方退回原缓冲。
    static func enhancedExportBuffer(_ buffer: RgbaBuffer) -> RgbaBuffer? {
        let maxEdge = max(buffer.width, buffer.height)
        guard maxEdge > 0, maxEdge <= 4096 else { return nil }
        let target = min(max(maxEdge * 2, 2048), 6144)
        return NativeCore.shared.retouchUpscale(buffer: buffer, targetMaxEdge: target)
    }

    /// 同步解码一张照片（后台线程用；系统解码路径）。
    private static func decodeAsset(_ item: PHAsset, maxEdge: Int) -> UIImage? {
        let options = PHImageRequestOptions()
        options.isSynchronous = true
        options.deliveryMode = .highQualityFormat
        options.resizeMode = .exact
        options.isNetworkAccessAllowed = true
        options.version = .current
        let target = CGSize(width: maxEdge, height: maxEdge)
        var output: UIImage?
        PHImageManager.default().requestImage(
            for: item,
            targetSize: target,
            contentMode: .aspectFit,
            options: options
        ) { image, _ in
            output = image
        }
        return output
    }

    /// 导出第二阶段（在渲染线程调用）：条带分块渲染 → 编码 → 存相册。
    private func finishExport(
        buffer: RgbaBuffer?,
        paramsJson json: String,
        retouch: RetouchUi,
        identityGeometry: Bool,
        metadata: [String: Any]?,
        format: ExportFormat,
        quality: Double
    ) {
        // 修图开启：先在解码缓冲上应用修图（按导出分辨率），再走调色渲染。
        var buffer = buffer
        if let source = buffer, retouch.isActive {
            buffer = Self.applyRetouchForExport(buffer: source, params: retouch) ?? source
        }
        // 大图优先按横向条带分块渲染（每块只跑区域管线，内存峰值明显低于整帧）。
        let renderedBuffer: RgbaBuffer?
        if let buffer,
           let result = NativeCore.shared.renderTiled(buffer: buffer, paramsJson: json) {
            renderedBuffer = RgbaBuffer(
                pixels: result.pixels,
                width: result.width,
                height: result.height
            )
        } else if let buffer,
                  let result = NativeCore.shared.render(buffer: buffer, paramsJson: json) {
            renderedBuffer = RgbaBuffer(
                pixels: result.pixels,
                width: result.width,
                height: result.height
            )
        } else {
            renderedBuffer = nil
        }
        let failure = renderedBuffer == nil ? NativeCore.shared.lastError() : nil
        let output = renderedBuffer.flatMap { ImageUtil.image(from: $0) }
        Task { @MainActor [weak self] in
            guard let self else { return }
            guard let output else {
                self.isExporting = false
                self.exportProgress = nil
                self.message = failure ?? (buffer == nil ? "导出失败：无法读取原图" : "导出失败")
                return
            }
            self.exportProgress = 0.75
            // 编码（JPEG/PNG/HEIC）放到工作线程，避免卡住界面。
            self.renderQueue.async { [weak self] in
                let fileURL = Self.writeExportFile(
                    image: output,
                    format: format,
                    quality: quality,
                    metadata: metadata
                )
                Task { @MainActor in
                    guard let self else { return }
                    guard let fileURL else {
                        self.isExporting = false
                        self.exportProgress = nil
                        self.message = "导出失败：无法写入文件"
                        return
                    }
                    self.exportProgress = 0.9
                    self.saveToLibrary(fileURL: fileURL)
                }
            }
        }
    }

    /// 读取原图的拍摄元数据（EXIF / TIFF / GPS / IPTC），并清掉会过期的尺寸字段。
    /// 大图导出：按横向条带分块调用区域渲染，拼成整张原图分辨率结果。
    ///
    /// 返回 nil 表示条件不满足（有裁切 / 旋转，或核心拒绝——例如带非单位几何的镜头参数），
    /// 调用方回退单次整帧渲染。
    private func renderTiledExport(
        buffer: RgbaBuffer,
        paramsJson: String,
        identityGeometry: Bool
    ) -> RgbaBuffer? {
        guard identityGeometry else { return nil }
        let width = buffer.width
        let height = buffer.height
        guard width > 32, height > 32 else { return nil }
        // 每条区域约 4MP（含余量），条带越矮内存越省、次数越多。
        let margin = 12
        let stripHeight = max(64, min(height, 4_000_000 / max(width, 1)))
        let stripCount = Int(ceil(Double(height) / Double(stripHeight)))
        var output = [UInt8](repeating: 0, count: width * height * 4)
        var y = 0
        var index = 0
        while y < height {
            let h = min(stripHeight, height - y)
            let regionY = max(0, y - margin)
            let regionBottom = min(height, y + h + margin)
            let regionHeight = regionBottom - regionY
            guard
                let region = ImageUtil.cropped(
                    buffer: buffer,
                    x: 0,
                    y: regionY,
                    width: width,
                    height: regionHeight
                ),
                let result = NativeCore.shared.renderRegion(
                    buffer: region,
                    originX: 0,
                    originY: regionY,
                    visibleX: 0,
                    visibleY: y,
                    visibleWidth: width,
                    visibleHeight: h,
                    fullWidth: width,
                    fullHeight: height,
                    paramsJson: paramsJson
                ),
                result.width == width,
                result.height == h
            else {
                return nil
            }
            let targetStart = y * width * 4
            let bytes = h * width * 4
            output.replaceSubrange(
                targetStart..<(targetStart + bytes),
                with: result.pixels[0..<bytes]
            )
            index += 1
            y += h
            let progress = 0.3 + 0.5 * Double(index) / Double(max(stripCount, 1))
            Task { @MainActor [weak self] in
                self?.exportProgress = min(progress, 0.8)
            }
        }
        return RgbaBuffer(pixels: output, width: width, height: height)
    }

    /// 读取原图的拍摄元数据（EXIF / TIFF / GPS / IPTC），并清掉会过期的尺寸字段。
    private static func loadExportMetadata(asset: PHAsset) -> [String: Any]? {
        let options = PHImageRequestOptions()
        // 只读本地元数据：避免 iCloud 资源在同步回调里阻塞。
        options.isNetworkAccessAllowed = false
        options.version = .current
        options.isSynchronous = true
        var data: Data?
        imageManager.requestImageDataAndOrientation(for: asset, options: options) { result, _, _, _ in
            data = result
        }
        guard
            let data,
            let source = CGImageSourceCreateWithData(data as CFData, nil),
            let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any]
        else {
            return nil
        }
        var keep: [String: Any] = [:]
        for key in [
            kCGImagePropertyExifDictionary,
            kCGImagePropertyTIFFDictionary,
            kCGImagePropertyGPSDictionary,
            kCGImagePropertyIPTCDictionary,
        ] {
            if let value = props[key as String] {
                keep[key as String] = value
            }
        }
        if var exif = keep[kCGImagePropertyExifDictionary as String] as? [String: Any] {
            exif.removeValue(forKey: kCGImagePropertyExifPixelXDimension as String)
            exif.removeValue(forKey: kCGImagePropertyExifPixelYDimension as String)
            keep[kCGImagePropertyExifDictionary as String] = exif
        }
        return keep.isEmpty ? nil : keep
    }

    /// 按所选格式 / 质量（含 EXIF 与颜色配置）写入临时文件；失败返回 nil。
    private static func writeExportFile(
        image: UIImage,
        format: ExportFormat,
        quality: Double,
        metadata: [String: Any]?
    ) -> URL? {
        guard let cgImage = image.cgImage else { return nil }
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("fdxmp-export-\(UUID().uuidString).\(format.fileExtension)")
        guard
            let destination = CGImageDestinationCreateWithURL(
                url as CFURL,
                format.uti as CFString,
                1,
                nil
            )
        else {
            return nil
        }
        var properties: [CFString: Any] = [:]
        if format == .jpeg {
            properties[kCGImageDestinationLossyCompressionQuality] =
                quality.clamped(to: 0.5...1)
        }
        if let metadata {
            for (key, value) in metadata {
                properties[key as CFString] = value
            }
        }
        CGImageDestinationAddImage(destination, cgImage, properties as CFDictionary)
        guard CGImageDestinationFinalize(destination) else { return nil }
        return url
    }

    private func saveToLibrary(fileURL: URL) {
        PHPhotoLibrary.shared().performChanges({
            guard
                let request = PHAssetChangeRequest.creationRequestForAssetFromImage(atFileURL: fileURL)
            else {
                return
            }
            guard let placeholder = request.placeholderForCreatedAsset else { return }
            if let album = Self.fdxmpAlbum() {
                PHAssetCollectionChangeRequest(for: album)?.addAssets([placeholder] as NSArray)
            } else {
                PHAssetCollectionChangeRequest
                    .creationRequestForAssetCollection(withTitle: "fdxmp")
                    .addAssets([placeholder] as NSArray)
            }
        }) { [weak self] success, error in
            Task { @MainActor in
                guard let self else { return }
                self.isExporting = false
                self.exportProgress = nil
                if success {
                    self.message = "已导出到相册「fdxmp」"
                    self.persist()
                    if EditorLimits.exportShareAfterSave {
                        self.exportShareURL = fileURL
                    }
                } else {
                    self.message = "保存失败：\(error?.localizedDescription ?? "未知错误")"
                }
            }
        }
    }

    private static func fdxmpAlbum() -> PHAssetCollection? {
        let options = PHFetchOptions()
        options.predicate = NSPredicate(format: "title = %@", "fdxmp")
        let result = PHAssetCollection.fetchAssetCollections(
            with: .album, subtype: .any, options: options
        )
        return result.firstObject
    }

    // MARK: - 辅助

    private func requestImage(
        asset: PHAsset,
        maxEdge: Int,
        mode: PHImageRequestOptionsDeliveryMode = .highQualityFormat,
        completion: @escaping @MainActor (UIImage?, Bool) -> Void
    ) {
        let options = PHImageRequestOptions()
        options.deliveryMode = mode
        options.resizeMode = .fast
        options.isNetworkAccessAllowed = true
        options.version = .current
        let target = CGSize(width: maxEdge, height: maxEdge)
        Self.imageManager.requestImage(
            for: asset,
            targetSize: target,
            contentMode: .aspectFit,
            options: options
        ) { image, info in
            guard let image else {
                // 正常情况会先回调一张降级图再回调正式图；只有真出错才传 nil。
                if info?[PHImageErrorKey] != nil {
                    Task { @MainActor in
                        completion(nil, false)
                    }
                }
                return
            }
            let degraded = (info?[PHImageResultIsDegradedKey] as? Bool) ?? false
            Task { @MainActor in
                completion(image, degraded)
            }
        }
    }

    private func pushUndo(_ snapshot: EditState) {
        pushUndo(snapshot, name: pendingLabel ?? "编辑")
        pendingLabel = nil
    }

    /// 记入一步历史（带操作名，供撤销提示与长按历史列表显示）。
    private func pushUndo(_ snapshot: EditState, name: String) {
        undoStack.append(HistoryEntry(name: name, state: snapshot))
        if undoStack.count > 40 {
            undoStack.removeFirst()
        }
        redoStack.removeAll()
        publishHistory()
    }

    private func publishHistory() {
        canUndo = !undoStack.isEmpty
        canRedo = !redoStack.isEmpty
    }

    private func persist() {
        guard let asset else { return }
        EditStore.save(
            assetId: asset.localIdentifier,
            json: currentJson(applyCropRect: true, usePreview: false)
        )
    }
}
