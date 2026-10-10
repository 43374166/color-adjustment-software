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
    /// 导出最长边。
    static let exportMaxEdge = 4096
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
    @Published var maskTab: MaskPanelTab = .shape

    private(set) var asset: PHAsset?

    private var baseBuffer: RgbaBuffer?
    private var dragSnapshot: EditState?
    private var undoStack: [EditState] = []
    private var redoStack: [EditState] = []
    private var renderBusy = false
    private var renderPending = false
    private let renderQueue = DispatchQueue(label: "fdxmp.ios.render", qos: .userInitiated)
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
        requestImage(asset: asset, maxEdge: EditorLimits.previewMaxEdge) { [weak self] image in
            guard let self else { return }
            guard let image else {
                self.isLoading = false
                self.message = "无法读取这张照片"
                return
            }
            // 像素转换（方向校正 + 缩放 + 色彩空间）放到渲染线程，避免卡主线程。
            self.renderQueue.async { [weak self] in
                let buffer = ImageUtil.rgbaBuffer(from: image, maxEdge: EditorLimits.previewMaxEdge)
                let original = buffer.flatMap { ImageUtil.image(from: $0) }
                Task { @MainActor in
                    guard let self else { return }
                    self.isLoading = false
                    guard let buffer else {
                        self.message = "无法读取这张照片的像素数据"
                        return
                    }
                    self.baseBuffer = buffer
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
    }

    func undo() {
        guard let previous = undoStack.popLast() else { return }
        redoStack.append(state)
        state = previous
        publishHistory()
        presetName = "自定义"
        persist()
        scheduleRender(immediate: true)
    }

    func redo() {
        guard let next = redoStack.popLast() else { return }
        undoStack.append(state)
        state = next
        publishHistory()
        presetName = "自定义"
        persist()
        scheduleRender(immediate: true)
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

    func addMask(_ kind: MaskKindUi) {
        let count = state.masks.filter { $0.kind == kind }.count + 1
        var mask = MaskUi(
            id: UUID().uuidString,
            name: "\(kind.rawValue) \(count)",
            kind: kind
        )
        mask.values = EditorSpec.defaultValues()
        applyEdit { state in
            state.masks.append(mask)
        }
        selectedMaskId = mask.id
    }

    func deleteSelectedMask() {
        guard let id = selectedMaskId else { return }
        applyEdit { state in
            state.masks.removeAll { $0.id == id }
        }
        selectedMaskId = state.masks.first?.id
    }

    func selectMask(_ id: String) {
        selectedMaskId = id
    }

    /// 修改选中蒙版（拖动中不触发渲染，只更新叠加显示）。
    func mutateSelectedMask(_ transform: (inout MaskUi) -> Void) {
        guard let id = selectedMaskId, let index = state.masks.firstIndex(where: { $0.id == id })
        else {
            return
        }
        transform(&state.masks[index])
    }

    /// 更新选中蒙版的调色参数（滑块）。
    func updateMaskValue(_ key: String, _ newValue: Float) {
        beginEdit()
        mutateSelectedMask { $0.values[key] = newValue }
        touchEdit()
    }

    // MARK: - 渲染

    /// 当前参数 JSON（裁切工具中使用未裁切预览）。
    private func currentJson(applyCropRect: Bool? = nil) -> String {
        EditJSON.build(
            state: state,
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
        renderQueue.async { [weak self] in
            let started = CFAbsoluteTimeGetCurrent()
            let result = NativeCore.shared.render(buffer: buffer, paramsJson: json)
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
        requestImage(asset: asset, maxEdge: EditorLimits.exportMaxEdge) { [weak self] image in
            guard let self else { return }
            guard let image else {
                self.isExporting = false
                self.message = "导出失败：无法读取原图"
                return
            }
            let json = self.currentJson(applyCropRect: true)
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
        completion: @escaping @MainActor (UIImage?) -> Void
    ) {
        let options = PHImageRequestOptions()
        options.deliveryMode = .highQualityFormat
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
            guard !degraded else { return }
            Task { @MainActor in
                completion(image)
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
            json: currentJson(applyCropRect: true)
        )
    }
}
