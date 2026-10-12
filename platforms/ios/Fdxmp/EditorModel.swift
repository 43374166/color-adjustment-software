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
    case grading = "分级"

    var id: String { rawValue }
}

/// 渲染尺寸上限（非 actor 隔离，可在后台渲染闭包中安全引用）。
enum EditorLimits {
    /// 预览最长边（越大越清晰、渲染越慢）。
    static let previewMaxEdge = 1440
    /// 蒙版拖动 / 涂抹时的快速预览最长边（低分辨率缓冲，手感更跟手）。
    static let fastPreviewMaxEdge = 720
    /// 蒙版叠加层最长边（与预览一致：共用同一份蒙版栅格缓存，避免重复计算）。
    static let maskOverlayMaxEdge = 1440
    /// 蒙版列表缩略图最长边。
    static let maskThumbnailMaxEdge = 128
    /// 导出按**原图分辨率**渲染（不再限制最长边）。
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
        return stored > 0 ? stored : 0.92
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
    /// RAW 细节缓冲最长边（LibRaw 解一版备用，供放大 1:1 / 区域细节复用）。
    static let rawDetailMaxEdge = 2560
    /// RAW 全尺寸解调的像素上限（超过则用半尺寸，避免 48MP ProRAW 内存尖峰）。
    static let rawFullSizePixelLimit = 24_000_000
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
    @Published private(set) var originalImage: UIImage?
    @Published var showOriginal = false
    @Published private(set) var histogram: HistogramData?
    /// 放大查看时的高分辨率帧（仅缩放 > 1 时显示）。
    @Published private(set) var detailImage: UIImage?
    @Published private(set) var isLoading = false
    @Published private(set) var isRendering = false
    @Published private(set) var isExporting = false
    /// 导出进度（0..1；nil = 未在导出）。
    @Published private(set) var exportProgress: Double?
    /// 导出完成后的分享文件（开了"导出后分享"才有值）。
    @Published var exportShareURL: URL?
    @Published private(set) var canUndo = false
    @Published private(set) var canRedo = false
    @Published private(set) var renderMillis: Double?
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
    /// RAW 细节缓冲（LibRaw 解一版 ≤2560，供放大 1:1 / 区域细节复用；nil = 未就绪）。
    private var rawDetailBuffer: RgbaBuffer?
    private var rawDetailInFlight = false
    private var rawDetailToken = 0
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
        originalImage = nil
        histogram = nil
        baseBuffer = nil
        rawData = nil
        rawDetailBuffer = nil
        rawDetailInFlight = false
        rawDetailToken &+= 1
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
        pendingRegion = nil
        undoStack.removeAll()
        redoStack.removeAll()
        publishHistory()
        renderMillis = nil
        message = nil
        if let json = EditStore.json(assetId: asset.localIdentifier) {
            state = EditJSON.parse(json)
            presetName = "自定义"
            presetRef = nil
        } else {
            state = EditState()
            presetName = "原片"
            presetRef = EditorSpec.presets.first?.ref
        }
        selectedMaskId = state.masks.first?.id
        isLoading = true
        loadToken &+= 1
        let token = loadToken
        let maxEdge = EditorLimits.previewMaxEdge

        // 1) 快速首图：RAW / 大图先出一张低质量图，避免几秒空屏。
        requestImage(asset: asset, maxEdge: maxEdge, mode: .fastFormat) { [weak self] image, _ in
            guard let self, self.loadToken == token, self.baseBuffer == nil else { return }
            guard let image else { return }
            self.previewImage = image
            self.isLoading = false
        }

        // 2) 正式图：RAW（ProRAW / DNG）走 LibRaw 解调，其余交给系统解码后转像素缓冲。
        if PhotoRaw.isRawAsset(asset) {
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
                        // 后台再解一版细节缓冲（放大 1:1 / 区域细节复用）。
                        self.prepareRawDetailBuffer(asset: asset)
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
                self.isLoading = false
                self.message = "无法读取这张照片"
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
    private func finishLoading(buffer: RgbaBuffer?, original: UIImage?, token: Int) {
        guard loadToken == token else { return }
        isLoading = false
        guard let buffer else {
            message = "无法读取这张照片的像素数据"
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
        // 有保存过修图参数的照片：后台重算修图缓冲。
        prepareRetouchAfterLoad()
    }

    /// RAW：后台解一版更精细的缓冲（≤2560）供放大 1:1 / 区域细节复用。
    private func prepareRawDetailBuffer(asset: PHAsset) {
        guard let data = rawData, rawDetailBuffer == nil, !rawDetailInFlight else { return }
        rawDetailToken &+= 1
        let token = rawDetailToken
        rawDetailInFlight = true
        let pixels = Double(asset.pixelWidth) * Double(asset.pixelHeight)
        let fullSize = pixels > 0 && pixels <= Double(EditorLimits.rawFullSizePixelLimit)
        let maxEdge = EditorLimits.rawDetailMaxEdge
        renderQueue.async { [weak self] in
            let buffer = NativeCore.shared.decodeRaw(data: data, maxEdge: maxEdge, fullSize: fullSize)
            Task { @MainActor in
                guard let self, self.rawDetailToken == token else { return }
                self.rawDetailInFlight = false
                self.rawDetailBuffer = buffer
                // 用户已放大（有细节帧）：用更高精度缓冲重渲染一帧。
                if buffer != nil, self.detailImage != nil {
                    self.requestDetailPreview(
                        viewport: self.lastDetailViewport,
                        zoom: self.lastDetailZoom,
                        force: true
                    )
                }
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
    func attachPreviewLayer(_ pointer: UnsafeMutableRawPointer?, token: UUID) {
        if let pointer {
            previewLayerToken = token
            previewLayerPointer = pointer
            tryAttachPreviewLayer()
        } else {
            guard previewLayerToken == token else { return }
            previewLayerToken = nil
            previewLayerPointer = nil
            zeroCopyActive = false
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
        guard let asset, !detailInFlight else { return }
        let needed = Int(max(viewport.width, viewport.height) * zoom * 1.15)
        let maxEdge = min(max(needed, 1200), EditorLimits.detailMaxEdge)
        // 已有一张分辨率相近的帧就不重复请求（避免每档缩放都重渲染）。
        lastDetailViewport = viewport
        lastDetailZoom = zoom
        if !force, detailImage != nil, abs(maxEdge - detailEdgeUsed) <= max(160, detailEdgeUsed / 8) {
            return
        }
        detailEdgeUsed = maxEdge
        detailInFlight = true
        detailToken &+= 1
        let token = detailToken
        let json = currentJson(applyCropRect: true)

        // RAW：细节帧同源走 LibRaw 缓冲（避免混入系统渲染的观感差异）。
        if rawData != nil {
            let source = retouchBuffer ?? rawDetailBuffer ?? baseBuffer
            renderQueue.async { [weak self] in
                let result = source.flatMap {
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
            return
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
            self.renderQueue.async { [weak self] in
                let buffer = ImageUtil.rgbaBuffer(from: image, maxEdge: maxEdge)
                // 修图开启时：细节帧先在修图缓冲上算好再上采样（保证与预览一致）。
                let prepared = buffer.flatMap { buffer -> RgbaBuffer? in
                    guard let self, self.state.retouch.isActive else { return buffer }
                    return Self.applyRetouchForExport(
                        buffer: buffer,
                        params: self.state.retouch
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

    /// 缩回 1x 时释放高分辨率帧，省内存。
    func clearDetailPreview() {
        detailToken &+= 1
        detailInFlight = false
        detailImage = nil
        detailEdgeUsed = 0
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

    /// 是否可用区域渲染（核心要求单位几何；蒙版类型也要能重建）。
    var canRenderRegionDetail: Bool {
        // RAW：区域帧若走系统渲染会与 LibRaw 基础帧观感不一致，先用整帧细节帧。
        guard rawData == nil else { return false }
        // 修图后：区域帧来自未修图的全分辨率源，会与修图预览不一致，先用整帧细节帧。
        guard retouchBuffer == nil else { return false }
        let crop = state.crop
        let identity = crop.rect == CGRect(x: 0, y: 0, width: 1, height: 1)
            && crop.rotation == 0
            && !crop.flipHorizontal
            && !crop.flipVertical
        // 光学（色差 / 配置文件校正）也算几何变换：区域渲染要求单位镜头模型。
        let lensIdentity = !state.removeChromaticAberration && !state.profileCorrection
        guard identity, lensIdentity, let asset else { return false }
        // 只对中等像素量走 1:1（全分辨率解码有内存尖峰，超大图仍用整帧细节帧）。
        let pixels = Double(asset.pixelWidth) * Double(asset.pixelHeight)
        return pixels > 0 && pixels <= 24_000_000
    }

    /// 去抖地请求区域高清帧（缩放 / 平移时反复调用只保留最后一次）。
    func scheduleDetailRegion(visible: CGRect, viewport: CGSize) {
        pendingRegion = (visible, viewport)
        regionDebounceToken &+= 1
        let token = regionDebounceToken
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.18) { [weak self] in
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
        detailRegion = nil
        lastRegionVisible = .zero
    }

    private func performDetailRegionRequest(visible: CGRect, viewport: CGSize) {
        guard canRenderRegionDetail, let asset else { return }
        let pixelWidth = CGFloat(asset.pixelWidth)
        let pixelHeight = CGFloat(asset.pixelHeight)
        guard pixelWidth > 32, pixelHeight > 32,
              visible.width > 0.01, visible.height > 0.01
        else {
            return
        }
        // 源分辨率：让可见区域像素 ≥ 视口像素（上限 = 原图长边）。
        let needed = max(viewport.width / visible.width, viewport.height / visible.height)
        let originalLong = max(pixelWidth, pixelHeight)
        let target = Int(min(originalLong, max(needed, 1400)))
        // 可见矩形 → 源像素矩形（单位几何：输出 = 源），向外扩余量给细节模糊。
        let margin: CGFloat = 12
        let vx = (visible.minX * pixelWidth).rounded()
        let vy = (visible.minY * pixelHeight).rounded()
        let vw = (visible.width * pixelWidth).rounded()
        let vh = (visible.height * pixelHeight).rounded()
        let rx = max(vx - margin, 0)
        let ry = max(vy - margin, 0)
        let rw = min(vx + vw + margin, pixelWidth) - rx
        let rh = min(vy + vh + margin, pixelHeight) - ry
        guard rw > 8, rh > 8 else { return }
        lastRegionVisible = visible
        detailRegionToken &+= 1
        let token = detailRegionToken
        let json = currentJson(applyCropRect: true)
        requestImage(asset: asset, maxEdge: target, mode: .highQualityFormat) { [weak self] image, degraded in
            guard let self, self.detailRegionToken == token, !degraded, let image else { return }
            self.renderQueue.async { [weak self] in
                guard let self else { return }
                // 方向校正后按比例把源像素矩形换算成请求图的像素矩形。
                guard let full = ImageUtil.rgbaBuffer(from: image, maxEdge: target) else { return }
                let scaleX = CGFloat(full.width) / pixelWidth
                let scaleY = CGFloat(full.height) / pixelHeight
                let cropX = Int((rx * scaleX).rounded())
                let cropY = Int((ry * scaleY).rounded())
                let cropW = max(8, Int((rw * scaleX).rounded()))
                let cropH = max(8, Int((rh * scaleY).rounded()))
                guard
                    let region = ImageUtil.cropped(
                        buffer: full,
                        x: cropX,
                        y: cropY,
                        width: cropW,
                        height: cropH
                    ),
                    let result = NativeCore.shared.renderRegion(
                        buffer: region,
                        originX: cropX,
                        originY: cropY,
                        visibleX: Int((vx * scaleX).rounded()),
                        visibleY: Int((vy * scaleY).rounded()),
                        visibleWidth: max(8, Int((vw * scaleX).rounded())),
                        visibleHeight: max(8, Int((vh * scaleY).rounded())),
                        fullWidth: full.width,
                        fullHeight: full.height,
                        paramsJson: json
                    ),
                    let rendered = ImageUtil.image(
                        from: RgbaBuffer(
                            pixels: result.pixels,
                            width: result.width,
                            height: result.height
                        )
                    )
                else {
                    // 核心拒绝区域渲染（例如带非单位几何的镜头参数）：回退整帧细节帧。
                    Task { @MainActor in
                        guard self.detailRegionToken == token else { return }
                        self.requestDetailPreview(viewport: viewport, zoom: self.lastDetailZoom)
                    }
                    return
                }
                Task { @MainActor in
                    guard self.detailRegionToken == token else { return }
                    self.detailRegion = DetailRegion(image: rendered, rect: visible)
                }
            }
        }
    }

    /// 编辑提交后刷新（放大状态下保持像素级清晰）。
    private func refreshDetailIfNeeded() {
        if detailRegion != nil {
            if canRenderRegionDetail {
                // 区域模式：按上次的可见矩形重渲染一块。
                performDetailRegionRequest(visible: lastRegionVisible, viewport: lastDetailViewport)
            } else {
                // 开了镜头校正 / 色差：区域渲染不可用，回落到整帧细节帧。
                clearDetailRegion()
                requestDetailPreview(viewport: lastDetailViewport, zoom: lastDetailZoom, force: true)
            }
        } else if detailImage != nil, !detailInFlight {
            // 整帧模式：参数变了，用同一档分辨率强制重渲染。
            requestDetailPreview(viewport: lastDetailViewport, zoom: lastDetailZoom, force: true)
        }
    }

    /// 切换工具（裁切时预览显示未裁切原图，其余工具显示裁切结果）。
    func selectTool(_ newTool: EditorTool) {
        guard tool != newTool else { return }
        tool = newTool
        draggingSlider = nil
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

    func updateGrading(_ zone: GradingZoneKind, _ value: GradingZone, commit: Bool) {
        beginEdit()
        pendingLabel = "分级 · \(zone.rawValue)"
        state.grading[zone] = value
        if commit {
            commitEdit()
        } else {
            touchEdit()
        }
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

    func resetCrop() {
        applyEdit("重置裁切") { state in
            state.crop.reset()
        }
    }

    // MARK: - 蒙版

    /// 新建蒙版（线性 / 径向 / 画笔 / 智能），并选中它。
    func addMask(_ kind: MaskKindUi) {
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
        applyEdit("新建\(kind.rawValue)") { state in
            state.masks.append(mask)
        }
        selectedMaskId = mask.id
        brushErase = false
        if kind.isSmart {
            generateSmartMask(mask.id)
        }
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
    private let retouchQueue = DispatchQueue(label: "fdxmp.ios.retouch", qos: .userInitiated)

    /// 一键修图的阶段文案（识别 → 逐项处理，加载动画用）。
    private let retouchStages = [
        "正在识别五官…",
        "去瑕疵…",
        "磨皮（低 / 中 / 高频）…",
        "统一肤色…",
        "脸部塑形…",
        "身体塑形…",
    ]

    /// 渲染 / 显示用的当前源图（修图后 = 修图结果，否则原图）。
    private var renderSource: RgbaBuffer? { retouchBuffer ?? baseBuffer }

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
                guard let self else { return }
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
    func recomputeRetouch(completion: (() -> Void)? = nil) {
        guard let buffer = baseBuffer else {
            completion?()
            return
        }
        let params = state.retouch
        if params == retouchApplied {
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
                guard let self else { return }
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
                completion?()
                if self.retouchPending {
                    self.retouchPending = false
                    self.recomputeRetouch()
                }
            }
        }
    }

    /// 启动阶段动画（识别 → 去瑕疵 → 磨皮 → 肤色 → 塑形）。
    private func startRetouchStages() {
        retouchStageIndex = 0
        retouchStage = retouchStages[0]
        retouchStageTimer?.invalidate()
        retouchStageTimer = Timer.scheduledTimer(withTimeInterval: 0.65, repeats: true) { [weak self] _ in
            guard let self else { return }
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
    enum SyncScope {
        case all
        case retouchOnly
        case colorOnly
    }

    /// 把当前编辑同步到项目内其余照片（返回同步张数）。
    ///
    /// 调色 + 修图一起同步；裁切与蒙版几何是每张照片自己的构图 / 笔迹，保持不动。
    @discardableResult
    func syncEdits(scope: SyncScope) -> Int {
        guard let project, let current = asset else { return 0 }
        var synced = 0
        for assetId in project.assetIds where assetId != current.localIdentifier {
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
                // 颜色分类下还有 HSL / 颜色分级两个子页，一起归零。
                copy.hsl.reset()
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
        guard renderSource != nil else { return }
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
        let json = currentJson()
        // 蒙版拖动 / 涂抹、以及沉浸拖动滑块时用低分辨率缓冲：
        // CPU 渲染量降约 4 倍，手感更跟手；松手提交后自动回到全分辨率预览。
        let interactive = maskInteracting || draggingSlider != nil || fallback
        let source = interactive ? (fastBuffer ?? buffer) : buffer
        let gpu = gpuSession
        // 零拷贝上屏：GPU 会话 + 图层就绪 + 非对比 / 非看原图时直接把画面推给 CAMetalLayer。
        let zeroCopy = gpu != nil && zeroCopyActive && !comparePreviewActive && !showOriginal
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
                    self.previewOutputSize = CGSize(width: size.width, height: size.height)
                    if let histogram {
                        self.histogram = histogram
                    }
                    self.renderMillis = millis
                    self.message = nil
                    // 渲染输出尺寸（裁切后长宽比）确定后再生成叠加层，保证对齐。
                    self.scheduleMaskAssetsRefresh()
                    if self.renderPending {
                        self.renderPending = false
                        self.startRender()
                    }
                }
                return
            }
            // GPU 优先（同一帧内失败 / 不适合时自动回退 CPU）。
            var result = gpu?.render(paramsJson: json)
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
            Task { @MainActor in
                guard let self else { return }
                self.renderBusy = false
                self.isRendering = false
                if let image, let histogram {
                    self.previewImage = image
                    self.histogram = histogram
                    self.previewOutputSize = outputSize
                    self.renderMillis = millis
                    self.message = nil
                    // 渲染输出尺寸（裁切后长宽比）确定后再生成叠加层，保证对齐。
                    self.scheduleMaskAssetsRefresh()
                } else if !fallback, self.fastBuffer != nil {
                    // 全分辨率渲染失败：降一档用快速缓冲重试一次，不再直接报错。
                    self.startRender(fallback: true)
                } else {
                    self.message = failure ?? "渲染失败"
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
        // 原图分辨率（不再限制最长边）。
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
        if let buffer, let tiled = renderTiledExport(
            buffer: buffer,
            paramsJson: json,
            identityGeometry: identityGeometry
        ) {
            renderedBuffer = tiled
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
