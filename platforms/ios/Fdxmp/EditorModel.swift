import Combine
import Photos
import UIKit

/// 编辑工具（底部工具栏）。
enum EditorTool: String, CaseIterable, Identifiable {
    case presets = "预设"
    case crop = "裁切"
    case edit = "编辑"
    case mask = "蒙版"

    var id: String { rawValue }

    var icon: String {
        switch self {
        case .presets: return "wand.and.stars"
        case .crop: return "crop"
        case .edit: return "slider.horizontal.3"
        case .mask: return "paintbrush.pointed"
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

/// 蒙版面板的子页。
enum MaskPanelTab: String, CaseIterable, Identifiable {
    case shape = "蒙版"
    case adjust = "调整"

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
    /// 导出最长边。
    /// 导出最长边（可在「我的」页切换 2048 / 4096）。
    static var exportMaxEdge: Int {
        let stored = UserDefaults.standard.integer(forKey: exportMaxEdgeKey)
        return stored > 0 ? stored : 4096
    }
    /// 导出尺寸设置键（与「我的」页的 @AppStorage 共用）。
    static let exportMaxEdgeKey = "fdxmp_export_max_edge"
    /// 放大查看（原像素细节）用的最长边。
    static let detailMaxEdge = 4096
}

/// 编辑界面状态机：照片加载 → 防抖渲染 → 直方图 / 撤销重做 / 导出。
///
/// 渲染走 Rust CPU 预览（`fdxmp_render_memory_rgba`）：
/// 滑块 / 曲线 / 蒙版变化 140ms 防抖，渲染期间只保留最后一次请求。
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
    @Published private(set) var canUndo = false
    @Published private(set) var canRedo = false
    @Published private(set) var renderMillis: Double?
    @Published private(set) var message: String?

    // MARK: 编辑状态

    /// 完整编辑参数（滑块 / 曲线 / HSL / 分级 / 裁切 / 蒙版）。
    @Published var state = EditState()
    @Published private(set) var tool: EditorTool = .edit
    @Published private(set) var presetName = "原片"

    // 面板导航
    @Published var category: EditCategory = .light
    @Published var colorTab: ColorTab = .base
    @Published var effectTab: EffectTab = .effects
    @Published var detailTab: DetailTab = .sharpen
    @Published var curveChannel: CurveChannel = .rgb
    @Published var gradingZone: GradingZoneKind = .highlights
    @Published var hslBand: HslBandKind = .red
    @Published var selectedMaskId: String?
    @Published private(set) var maskTab: MaskPanelTab = .shape
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

    private(set) var asset: PHAsset?

    private var baseBuffer: RgbaBuffer?
    /// 拖动蒙版控件 / 涂抹时的低分辨率缓冲（长边 720，预览更跟手）。
    private var fastBuffer: RgbaBuffer?
    /// 蒙版交互进行中（用快速缓冲渲染，结束时回落到全分辨率）。
    private(set) var maskInteracting = false
    private var loadToken = 0
    private var detailToken = 0
    private var detailInFlight = false
    private var dragSnapshot: EditState?
    private var undoStack: [EditState] = []
    private var redoStack: [EditState] = []
    private var renderBusy = false
    private var renderPending = false
    private let renderQueue = DispatchQueue(label: "fdxmp.ios.render", qos: .userInitiated)
    private let maskAssetQueue = DispatchQueue(label: "fdxmp.ios.mask-assets", qos: .userInitiated)
    private var maskAssetToken = 0
    private var maskAssetScheduled = false
    /// 进入「调整」子页前的叠加显示状态（用于回来时恢复）。
    private var maskOverlayRestore = true
    private static let imageManager = PHCachingImageManager()

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
        fastBuffer = nil
        maskOverlays = [:]
        maskThumbnails = [:]
        maskInteracting = false
        brushCursor = nil
        brushErase = false
        maskOverlayVisible = true
        undoStack.removeAll()
        redoStack.removeAll()
        publishHistory()
        renderMillis = nil
        message = nil
        if let json = EditStore.json(assetId: asset.localIdentifier) {
            state = EditJSON.parse(json)
            presetName = "自定义"
        } else {
            state = EditState()
            presetName = "原片"
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

        // 2) 正式图：转成像素缓冲后走 Rust 渲染。
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
                    self.isLoading = false
                    guard let buffer else {
                        self.message = "无法读取这张照片的像素数据"
                        return
                    }
                    self.baseBuffer = buffer
                    self.fastBuffer = ImageUtil.downscaled(
                        buffer: buffer,
                        maxEdge: EditorLimits.fastPreviewMaxEdge
                    )
                    self.originalImage = original
                    // 先显示原图占位，首帧渲染完成后替换为调色结果。
                    self.previewImage = original
                    self.scheduleRender(immediate: true)
                }
            }
        }
    }

    func clearMessage() {
        message = nil
    }

    // MARK: - 放大细节（原像素）

    /// 请求一张高分辨率帧：双指 / 双击放大时用，保证放大后看到的是原始像素而不是放大后的代理图。
    func requestDetailPreview() {
        guard let asset, !detailInFlight else { return }
        detailInFlight = true
        detailToken &+= 1
        let token = detailToken
        let json = currentJson(applyCropRect: true)
        requestImage(
            asset: asset,
            maxEdge: EditorLimits.detailMaxEdge,
            mode: .highQualityFormat
        ) { [weak self] image, degraded in
            guard let self, self.detailToken == token, !degraded else { return }
            guard let image else {
                self.detailInFlight = false
                return
            }
            self.renderQueue.async { [weak self] in
                let buffer = ImageUtil.rgbaBuffer(from: image, maxEdge: EditorLimits.detailMaxEdge)
                let result = buffer.flatMap {
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
    }

    /// 编辑提交后刷新（放大状态下保持像素级清晰）。
    private func refreshDetailIfNeeded() {
        if detailImage != nil, !detailInFlight {
            requestDetailPreview()
        }
    }

    /// 切换工具（裁切时预览显示未裁切原图，其余工具显示裁切结果）。
    func selectTool(_ newTool: EditorTool) {
        guard tool != newTool else { return }
        tool = newTool
        if newTool == .mask {
            if selectedMaskId == nil {
                selectedMaskId = state.masks.first?.id
            }
            // 蒙版调整面板不包含细节分类。
            if category == .detail {
                category = .light
            }
            scheduleMaskAssetsRefresh(immediate: true)
        } else {
            brushCursor = nil
        }
        scheduleRender(immediate: true)
    }

    // MARK: - 通用编辑（撤销 / 重做 / 提交）

    /// 开始一次编辑（滑块按下 / 拖动开始）：记录快照。
    func beginEdit() {
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
        presetName = "自定义"
        persist()
        scheduleRender(immediate: true)
        refreshDetailIfNeeded()
    }

    /// 一次性参数修改（点按类操作）：自动记一步历史。
    func applyEdit(_ mutate: (inout EditState) -> Void) {
        let before = state
        mutate(&state)
        guard state != before else { return }
        pushUndo(before)
        presetName = "自定义"
        persist()
        scheduleRender(immediate: true)
        refreshDetailIfNeeded()
    }

    func undo() {
        guard let previous = undoStack.popLast() else { return }
        redoStack.append(state)
        state = previous
        publishHistory()
        presetName = "自定义"
        persist()
        scheduleRender(immediate: true)
        refreshDetailIfNeeded()
    }

    func redo() {
        guard let next = redoStack.popLast() else { return }
        undoStack.append(state)
        state = next
        publishHistory()
        presetName = "自定义"
        persist()
        scheduleRender(immediate: true)
        refreshDetailIfNeeded()
    }

    // MARK: - 基础滑块

    func value(_ key: String) -> Float {
        state.values[key] ?? 0
    }

    func updateValue(_ key: String, _ newValue: Float) {
        state.values[key] = newValue
        touchEdit()
    }

    // MARK: - 预设

    /// 应用内置预设（与 Android 一致：重置滑块与曲线，保留 HSL / 分级 / 蒙版）。
    func applyPreset(_ preset: BuiltinPreset) {
        applyEdit { state in
            var values = EditorSpec.defaultValues()
            for (key, value) in preset.params {
                values[key] = value
            }
            state.values = values
            state.curves.reset()
        }
        presetName = preset.name
    }

    /// 全部归零。
    func resetAll() {
        applyEdit { state in
            state = EditState()
        }
        selectedMaskId = nil
        brushErase = false
        brushCursor = nil
        maskInteracting = false
        presetName = "原片"
    }

    // MARK: - 曲线

    func curvePoints(_ channel: CurveChannel) -> [CurvePoint] {
        state.curves.points(channel)
    }

    /// 更新曲线控制点（拖动中 commit = false）。
    func updateCurve(_ channel: CurveChannel, _ points: [CurvePoint], commit: Bool) {
        beginEdit()
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
        state.curves.parametric = values
        if commit {
            commitEdit()
        } else {
            touchEdit()
        }
    }

    func resetCurve(_ channel: CurveChannel) {
        applyEdit { state in
            state.curves.setPoints(channel, CurveState.identity)
        }
    }

    // MARK: - HSL / 颜色分级

    func updateHsl(_ band: HslBandKind, _ value: HslBand, commit: Bool) {
        beginEdit()
        state.hsl[band] = value
        if commit {
            commitEdit()
        } else {
            touchEdit()
        }
    }

    func updateGrading(_ zone: GradingZoneKind, _ value: GradingZone, commit: Bool) {
        beginEdit()
        state.grading[zone] = value
        if commit {
            commitEdit()
        } else {
            touchEdit()
        }
    }

    func updateGradingMix(blending: Float, balance: Float, commit: Bool) {
        beginEdit()
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
        state.crop.rect = clampedCropRect(rect)
        if commit {
            // 裁剪框只影响叠加层；裁切结果在切换工具 / 导出时统一渲染。
            if let before = dragSnapshot, before != state {
                pushUndo(before)
            }
            dragSnapshot = nil
            presetName = "自定义"
            persist()
        }
    }

    func updateCropRotation(_ value: Float, commit: Bool) {
        beginEdit()
        state.crop.rotation = value.clamped(to: -45...45)
        if commit {
            commitEdit()
        } else {
            touchEdit()
        }
    }

    /// 选择固定比例：按比例调整当前裁剪框（不缩放图像）。
    func setCropAspect(_ aspect: CropAspect, contentSize: CGSize) {
        applyEdit { state in
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
        applyEdit { state in
            if horizontal {
                state.crop.flipHorizontal.toggle()
            } else {
                state.crop.flipVertical.toggle()
            }
        }
    }

    func resetCrop() {
        applyEdit { state in
            state.crop.reset()
        }
    }

    // MARK: - 蒙版

    /// 新建蒙版（线性 / 径向 / 画笔），并选中它。
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
        }
        mask.values = EditorSpec.defaultValues()
        applyEdit { state in
            state.masks.append(mask)
        }
        selectedMaskId = mask.id
        setMaskTab(.shape)
        brushErase = false
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
        applyEdit { state in
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
        applyEdit { state in
            state.masks.removeAll { $0.id == id }
        }
        selectedMaskId = state.masks.first?.id
        brushErase = false
    }

    /// 重命名选中蒙版。
    func renameSelectedMask(_ name: String) {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let id = selectedMaskId else { return }
        applyEdit { state in
            if let index = state.masks.firstIndex(where: { $0.id == id }) {
                state.masks[index].name = trimmed
            }
        }
    }

    /// 显示 / 隐藏某个蒙版（隐藏后不参与渲染与叠加）。
    func toggleMaskVisible(_ id: String) {
        applyEdit { state in
            if let index = state.masks.firstIndex(where: { $0.id == id }) {
                state.masks[index].visible.toggle()
            }
        }
    }

    /// 反转选中蒙版。
    func invertSelectedMask() {
        guard let id = selectedMaskId else { return }
        applyEdit { state in
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
        }
        applyEdit { state in
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
        applyEdit { state in
            if let index = state.masks.firstIndex(where: { $0.id == id }) {
                state.masks[index].strokes.removeLast()
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

    /// 切换蒙版面板子页。
    ///
    /// 进入「调整」时自动收起红色叠加（调色时只看效果，不被蒙版挡住），
    /// 回到「蒙版」（形状）子页时恢复原来的叠加显示状态。
    func setMaskTab(_ tab: MaskPanelTab) {
        guard maskTab != tab else { return }
        if tab == .adjust {
            maskOverlayRestore = maskOverlayVisible
            maskOverlayVisible = false
        } else if maskTab == .adjust {
            maskOverlayVisible = maskOverlayRestore
            if maskOverlayVisible {
                // 收起期间可能错过了大图叠加的生成，这里补一次。
                scheduleMaskAssetsRefresh(immediate: true)
            }
        }
        maskTab = tab
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
        mutateSelectedMask { $0.values[key] = newValue }
        touchEdit()
    }

    // MARK: - 蒙版画布交互（手势层驱动）

    /// 开始拖动蒙版控件：记录快照，拖动期间用快速预览渲染。
    func beginMaskDrag() {
        guard selectedMask != nil else { return }
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
        maskInteracting = true
        let mask = state.masks[index]
        state.masks[index].strokes.append(
            BrushStrokeUi(
                points: [point],
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
        brushCursor = nil
        finishMaskInteraction()
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

    /// 松开分类导航：恢复预览。
    func endCategoryPreview() {
        guard previewCategory != nil else { return }
        previewCategory = nil
        scheduleRender(immediate: true)
    }

    /// 渲染 / 预览用状态：长按分类时把该分类参数归零（取默认值 = 无效果）。
    private var renderState: EditState {
        guard let category = previewCategory else { return state }
        var copy = state
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
        guard baseBuffer != nil else { return }
        // 空闲时立即渲染；正在渲染则记下待渲染，当前帧结束后自动接着渲染。
        // （此前的 140ms 防抖在持续拖动时会一直被重置，导致松手才出图。）
        if renderBusy {
            renderPending = true
        } else {
            startRender()
        }
    }

    private func startRender() {
        guard let buffer = baseBuffer else { return }
        if renderBusy {
            renderPending = true
            return
        }
        renderBusy = true
        isRendering = true
        let json = currentJson()
        // 蒙版拖动 / 涂抹中用低分辨率缓冲：CPU 渲染量降 4 倍，画布更跟手；
        // 松手提交后自动回到全分辨率预览。
        let source = maskInteracting ? (fastBuffer ?? buffer) : buffer
        renderQueue.async { [weak self] in
            let started = CFAbsoluteTimeGetCurrent()
            let result = NativeCore.shared.render(buffer: source, paramsJson: json)
            let millis = (CFAbsoluteTimeGetCurrent() - started) * 1000
            let failure = result == nil ? NativeCore.shared.lastError() : nil
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
                    self.renderMillis = millis
                    self.message = nil
                    // 渲染输出尺寸（裁切后长宽比）确定后再生成叠加层，保证对齐。
                    self.scheduleMaskAssetsRefresh()
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

    /// 按当前参数渲染高分辨率图片并保存到相册（fdxmp 相簿）。
    func export() {
        guard let asset, !isExporting else { return }
        isExporting = true
        message = nil
        requestImage(
            asset: asset,
            maxEdge: EditorLimits.exportMaxEdge,
            mode: .highQualityFormat
        ) { [weak self] image, _ in
            guard let self else { return }
            guard let image else {
                self.isExporting = false
                self.message = "导出失败：无法读取原图"
                return
            }
            let json = self.currentJson(applyCropRect: true, usePreview: false)
            self.renderQueue.async { [weak self] in
                let buffer = ImageUtil.rgbaBuffer(from: image, maxEdge: EditorLimits.exportMaxEdge)
                let result = buffer.flatMap {
                    NativeCore.shared.render(buffer: $0, paramsJson: json)
                }
                let failure = result == nil ? NativeCore.shared.lastError() : nil
                let output = result.flatMap {
                    ImageUtil.image(
                        from: RgbaBuffer(
                            pixels: $0.pixels, width: $0.width, height: $0.height
                        )
                    )
                }
                Task { @MainActor in
                    guard let self else { return }
                    guard let output else {
                        self.isExporting = false
                        self.message = failure ?? "导出失败"
                        return
                    }
                    self.saveToLibrary(output)
                }
            }
        }
    }

    private func saveToLibrary(_ image: UIImage) {
        PHPhotoLibrary.shared().performChanges({
            let request = PHAssetChangeRequest.creationRequestForAsset(from: image)
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
                if success {
                    self.message = "已导出到相册「fdxmp」"
                    self.persist()
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
        undoStack.append(snapshot)
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
