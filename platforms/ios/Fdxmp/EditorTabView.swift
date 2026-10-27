import Photos
import SwiftUI
import UIKit

/// 编辑 Tab：顶部工具 + 大图预览 + 直方图 + 底部工具栏。
///
/// 工具：预设 / 编辑（亮度 · 颜色 · HSL · 分级 · 效果 · 细节）/ 曲线 / 裁切 / 蒙版。
/// 长按图片看原图；滑块松手记一步历史；导出按当前参数渲染并保存到相册。
struct EditorTabView: View {
    let asset: PHAsset?
    /// 当前项目（nil = 设备单张模式：不显示同步、不做左右切换、不自动基础调色）。
    var project: Project?
    /// 相册式左右滑动的邻居预览（设备 / 项目队列里的前一张 / 后一张）。
    var prevAsset: PHAsset?
    var nextAsset: PHAsset?
    /// 当前编辑会话的完整照片队列（导出 / 同步弹框里选择照片用）。
    var queue: [PHAsset] = []
    /// 返回回调（退出编辑会话，回到来源页面）。
    var onClose: () -> Void = {}
    /// 未放大时左右滑动切图：上一张 / 下一张（队列与边界由父层提供）。
    var onSwipePrev: () -> Void = {}
    var onSwipeNext: () -> Void = {}
    var canSwipePrev = false
    var canSwipeNext = false

    @StateObject private var model = EditorModel()
    @State private var immersive = false
    @State private var zoom: CGFloat = 1
    @State private var baseZoom: CGFloat = 1
    @State private var pan: CGSize = .zero
    @State private var basePan: CGSize = .zero
    /// 照片触摸跟踪状态：轻点 / 长按自己判定（不再用「双击手势压制单击」，
    /// 避免单击要等 0.3s 才有响应）。
    @State private var photoTouchActive = false
    @State private var photoTouchMoved = false
    @State private var photoHoldActive = false
    @State private var photoHoldWork: DispatchWorkItem?
    /// 上一次轻点的时间（300ms 内两次 = 双击放大 / 还原）。
    @State private var lastPhotoTapAt = Date.distantPast
    /// 本次轻点前是否已沉浸（双击放大要把单击的沉浸切换还原）。
    @State private var immersiveBeforeTap = false
    /// 蒙版画布交互控制器（拖动 / 涂抹 / 双指手势）。
    @State private var maskController = MaskCanvasController()
    /// 分类参数面板是否收起（点同一个分类两次 = 收起 / 再展开）。
    @State private var categoryCollapsed = false
    /// 长按分类对比：按住期间 + 松手后一小段时间都吞掉这次点击，避免顺带展开 / 收起。
    @State private var categoryHoldActive = false
    @State private var categorySuppressTapUntil = Date.distantPast
    /// 长按撤销键后，短时间内忽略这次点击（避免顺带撤销一步）。
    @State private var suppressUndoTapUntil = Date.distantPast
    /// 长按照片前是否已处于沉浸模式（松手后按原样恢复）。
    @State private var immersiveBeforeHold = false
    /// 最近一次双指缩放的时间（防止捏合手势被误判成左右滑动切图）。
    @State private var lastPinchAt = Date.distantPast
    /// 历史列表弹层。
    @State private var showHistory = false
    /// 分屏对比分割线位置（nil = 关闭对比模式）。
    @State private var compareSplit: CGFloat?
    /// HSL 色带选择样式：dots（Lightroom 色圆点）/ strip（彩虹色带）。
    @AppStorage("fdxmp_hsl_style") private var hslStyle = "dots"
    /// 颜色均衡器当前页签：0 色相 / 1 饱和度 / 2 亮度。
    @State private var eqChannel = 0
    /// 是否展开均衡器选项（阈值 / 曲线宽度 / 半径 / 旋转）。
    @State private var eqShowOptions = false
    /// 预览区尺寸（用于按视口取放大细节帧分辨率）。
    @State private var previewAreaSize: CGSize = .zero
    /// 相册式左右滑动：内容偏移（手指跟随 / 回弹）。
    @State private var swipeOffset: CGFloat = 0
    @State private var swipeAnimating = false
    /// 相邻照片的低清预览（滑动手感像系统图库）。
    @State private var neighborImages: [String: UIImage] = [:]
    /// 底部弹框（••• 更多 / 导出 / 同步）。
    @State private var activeSheet: EditorSheetKind?
    @State private var sheetSelection: Set<String> = []
    @State private var sheetSyncScope: EditorModel.SyncScope = .all
    @State private var sheetEnhanceExport = false

    @Environment(\.displayScale) private var displayScale

    var body: some View {
        Group {
            if asset != nil {
                editor
            } else {
                emptyState
            }
        }
        .background(Color(white: 0.08))
        // 导出进度弹框：半透明遮罩 + 进度条 + 百分比。
        .overlay {
            if model.isExporting {
                ZStack {
                    Color.black.opacity(0.45).ignoresSafeArea()
                    VStack(spacing: 12) {
                        Text("正在导出…")
                            .font(.subheadline)
                            .foregroundStyle(.white)
                        ProgressView(value: model.exportProgress ?? 0, total: 1)
                            .progressViewStyle(.linear)
                            .tint(Color.accentColor)
                            .frame(width: 200)
                        Text("\(Int(((model.exportProgress ?? 0) * 100).rounded()))%")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                    }
                    .padding(24)
                    .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 16))
                }
                .transition(.opacity)
            }
        }
        .onPreferenceChange(SliderDragPreferenceKey.self) { info in
            model.setDraggingSlider(info)
        }
        .onAppear(perform: loadIfNeeded)
        .onAppear {
            // 预热触觉引擎，第一次操作就有反馈。
            Haptics.prepare()
        }
        .onAppear {
            model.configureProject(project)
        }
        .onChange(of: project?.id) { _ in
            model.configureProject(project)
        }
        .onChange(of: asset?.localIdentifier) { _ in
            resetZoom()
            immersive = false
            model.clearDetailPreview()
            loadIfNeeded()
            prefetchNeighbors()
            swipeOffset = 0
        }
        .onChange(of: prevAsset?.localIdentifier) { _ in prefetchNeighbors() }
        .onChange(of: nextAsset?.localIdentifier) { _ in prefetchNeighbors() }
        // 底部弹框：••• → 导出 / 同步（都带照片多选）→ 重置。
        .sheet(item: $activeSheet) { sheet in
            sheetContent(sheet)
                .presentationDetents(sheet == .more ? [.height(300)] : [.medium, .large])
                .presentationDragIndicator(.hidden)
                .preferredColorScheme(.dark)
        }
        .onChange(of: model.tool) { _ in
            resetZoom()
            model.clearDetailPreview()
            maskController.reset()
            categoryCollapsed = false
            model.endCategoryPreview()
            compareSplit = nil
            model.clearBeforeImage()
            model.setComparePreviewActive(false)
            if model.tool == .mask {
                Haptics.prepare()
            }
        }
        .onChange(of: zoom) { value in
            // 放大时按缩放级别拉取"够用"的高分辨率帧，缩回 1x 释放。
            updateDetailRequest()
        }
        .onChange(of: pan) { _ in
            // 平移后可见范围变了：区域帧要跟着换（内部去抖）。
            updateDetailRequest()
        }
        .overlay(alignment: .bottom) {
            if let message = model.message {
                Text(message)
                    .font(.footnote)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
                    .background(.ultraThinMaterial, in: Capsule())
                    .padding(.bottom, 18)
                    .task(id: message) {
                        try? await Task.sleep(nanoseconds: 2_600_000_000)
                        model.clearMessage()
                    }
            }
        }
        .sheet(isPresented: $showHistory) {
            historySheet
        }
        .sheet(isPresented: exportShareBinding) {
            if let url = model.exportShareURL {
                ShareSheet(items: [url])
            }
        }
    }

    /// 导出后分享面板的显示绑定。
    private var exportShareBinding: Binding<Bool> {
        Binding(
            get: { model.exportShareURL != nil },
            set: { if !$0 { model.exportShareURL = nil } }
        )
    }

    /// 历史记录弹层（长按撤销键打开）：可一次回退多步。
    private var historySheet: some View {
        VStack(spacing: 0) {
            HStack {
                Text("历史记录")
                    .font(.subheadline.weight(.semibold))
                Spacer()
                Button("关闭") {
                    showHistory = false
                }
                .font(.caption)
                .foregroundStyle(Color.accentColor)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            Divider()
            List {
                HStack {
                    Text("当前")
                        .font(.subheadline)
                    Spacer()
                    Text(model.presetName)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                ForEach(Array(model.historyNames.enumerated()), id: \.offset) { index, name in
                    Button {
                        model.restoreHistory(stepBack: index + 1, announce: true)
                        showHistory = false
                    } label: {
                        HStack {
                            Text("撤销「\(name)」")
                                .font(.subheadline)
                                .foregroundStyle(Color.primary)
                            Spacer()
                            Text("回退 \(index + 1) 步")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
            .listStyle(.plain)
        }
        .presentationDetents([.medium, .large])
    }

    private func loadIfNeeded() {
        guard let asset else { return }
        model.load(asset: asset)
    }

    /// 预取相邻照片的低清预览（相册式滑动时随手指滑入）。
    private func prefetchNeighbors() {
        prefetch(assets: [prevAsset, nextAsset].compactMap { $0 })
    }

    /// 预取指定照片的高清预览（2048；相邻张在拖动开始时也会补拉，保证切换丝滑）。
    private func prefetch(assets: [PHAsset]) {
        for neighbor in assets {
            let id = neighbor.localIdentifier
            if neighborImages[id] != nil { continue }
            let options = PHImageRequestOptions()
            // 高质量预览：fastFormat 会返回很糊的图，滑动时观感差（用户反馈）。
            options.deliveryMode = .highQualityFormat
            // 关键：.exact 会把照片压成正方形（比例被改，滑动时看着"被放大/变形"），
            // 用 .fast 保持原始比例、只按目标尺寸等比缩放。
            options.resizeMode = .fast
            options.isNetworkAccessAllowed = true
            let target = CGSize(width: 2048, height: 2048)
            PHImageManager.default().requestImage(
                for: neighbor,
                targetSize: target,
                contentMode: .aspectFit,
                options: options
            ) { image, _ in
                guard let image, image.size.width >= 320 else { return }
                DispatchQueue.main.async {
                    neighborImages[id] = image
                    if neighborImages.count > 8 {
                        // 只留最近用到的几张（避免缓存无限涨）。
                        let keep = Set(
                            [prevAsset, nextAsset, asset]
                                .compactMap { $0?.localIdentifier }
                        )
                        neighborImages = neighborImages.filter { keep.contains($0.key) }
                    }
                }
            }
        }
    }

    /// 底部弹框内容。
    @ViewBuilder
    private func sheetContent(_ sheet: EditorSheetKind) -> some View {
        switch sheet {
        case .more:
            EditorMoreSheet(
                canSync: queue.count > 1,
                onExport: {
                    activeSheet = nil
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
                        sheetSelection = asset.map { [$0.localIdentifier] }.map(Set.init) ?? []
                        activeSheet = .export
                    }
                },
                onSync: {
                    activeSheet = nil
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
                        let targets = queue
                            .map(\.localIdentifier)
                            .filter { $0 != asset?.localIdentifier }
                        sheetSelection = Set(targets)
                        sheetSyncScope = .all
                        activeSheet = .sync
                    }
                },
                onReset: {
                    activeSheet = nil
                    model.resetAll()
                }
            )
        case .export:
            PhotoSelectionSheet(
                title: "导出",
                confirmTitle: "导出所选照片",
                assets: queue.isEmpty ? [asset].compactMap { $0 } : queue,
                currentAssetId: asset?.localIdentifier,
                syncScope: nil,
                enhance: $sheetEnhanceExport,
                selection: $sheetSelection,
                onConfirm: { ids in
                    activeSheet = nil
                    let selected = queue.isEmpty
                        ? [asset].compactMap { $0 }
                        : queue.filter { ids.contains($0.localIdentifier) }
                    let enhance = sheetEnhanceExport
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
                        model.exportAssets(selected, enhance: enhance)
                    }
                }
            )
        case .sync:
            PhotoSelectionSheet(
                title: "同步到项目照片",
                confirmTitle: "同步到所选照片",
                assets: queue,
                currentAssetId: asset?.localIdentifier,
                syncScope: $sheetSyncScope,
                selection: $sheetSelection,
                onConfirm: { ids in
                    activeSheet = nil
                    model.syncEdits(scope: sheetSyncScope, to: ids)
                }
            )
        }
    }

    /// 按当前缩放 / 视口请求"够用"的高分辨率细节帧（缩到 1x 时释放）。
    private func updateDetailRequest() {
        guard zoom > 1.05, previewAreaSize.width > 10 else {
            model.clearDetailPreview()
            model.clearDetailRegion()
            return
        }
        let viewport = CGSize(
            width: previewAreaSize.width * displayScale,
            height: previewAreaSize.height * displayScale
        )
        if model.canRenderRegionDetail {
            // 区域 1:1：只渲染可见范围（核心按帧外坐标保证与整帧一致）。
            model.clearDetailPreview()
            model.scheduleDetailRegion(
                visible: currentVisibleRect(viewSize: previewAreaSize),
                viewport: viewport
            )
        } else {
            model.clearDetailRegion()
            model.requestDetailPreview(viewport: viewport, zoom: zoom)
        }
    }

    /// 当前可见区域（归一化，输出坐标系）。
    private func currentVisibleRect(viewSize: CGSize) -> CGRect {
        let fitted = fittedRect(imageSize: previewImageSize, in: viewSize)
        guard fitted.width > 1, fitted.height > 1 else {
            return CGRect(x: 0, y: 0, width: 1, height: 1)
        }
        let scale = max(zoom, 0.0001)
        let center = CGPoint(x: viewSize.width / 2, y: viewSize.height / 2)
        let originX = center.x + (0 - center.x - pan.width) / scale
        let originY = center.y + (0 - center.y - pan.height) / scale
        return CGRect(
            x: (originX - fitted.minX) / fitted.width,
            y: (originY - fitted.minY) / fitted.height,
            width: viewSize.width / scale / fitted.width,
            height: viewSize.height / scale / fitted.height
        )
    }

    /// 区域帧是否覆盖当前可见范围（不覆盖就不显示，避免贴错位置的接缝）。
    private func detailRegionCovers(_ rect: CGRect, viewSize: CGSize) -> Bool {
        let visible = currentVisibleRect(viewSize: viewSize)
        let tolerance: CGFloat = 0.012
        return visible.minX >= rect.minX - tolerance
            && visible.minY >= rect.minY - tolerance
            && visible.maxX <= rect.maxX + tolerance
            && visible.maxY <= rect.maxY + tolerance
    }

    // MARK: - 主布局

    private var editor: some View {
        GeometryReader { geo in
            // 竖屏调色：面板浮在整图上（图片不被挤小）；蒙版 / 裁切 / 横屏保持推挤布局。
            let overlayPanel = geo.size.height > geo.size.width
                && (model.tool == .presets || model.tool == .edit || model.tool == .retouch)
            Group {
                if overlayPanel {
                    ZStack(alignment: .top) {
                        VStack(spacing: 0) {
                            editorColumn
                        }
                        if !immersive {
                            VStack(spacing: 0) {
                                Spacer(minLength: 0)
                                panel
                            }
                        }
                    }
                } else {
                    VStack(spacing: 0) {
                        editorColumn
                        // 沉浸查看（长按看原图 / 轻点隐藏 UI）时面板只隐不撤：
                        // 预览区尺寸不变，图片和曲线编辑区都不跳位。
                        panel
                            .opacity(immersive ? 0 : 1)
                            .allowsHitTesting(!immersive)
                    }
                }
            }
            .frame(width: geo.size.width, height: geo.size.height)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        // 沉浸拖动滑块：画面顶部显示大号数值（Lightroom 做法）。
        .overlay(alignment: .top) {
            if let label = model.draggingSlider, let text = model.draggingSliderText {
                SliderValueHud(label: label, text: text)
                    .padding(.top, 16)
                    .allowsHitTesting(false)
                    .transition(.opacity)
            }
        }
        .animation(.easeInOut(duration: 0.24), value: panelHeight)
        .animation(.easeInOut(duration: 0.24), value: hidesChrome)
        .animation(.easeInOut(duration: 0.24), value: hidesToolBar)
        // AI 修图：全屏加载动画（识别五官 → 逐项处理）。
        .overlay {
            if let stage = activeOverlayStage {
                RetouchProgressOverlay(text: stage)
                    .transition(.opacity)
            }
        }
        .animation(.easeInOut(duration: 0.2), value: activeOverlayStage)
    }

    /// 全屏加载文案：修图阶段 / 工作台切换 / 调色下的 AI 识别（智能蒙版）。
    private var activeOverlayStage: String? {
        if let stage = model.retouchStage {
            return stage
        }
        if let stage = model.workbenchStage {
            return stage
        }
        if !model.smartMaskGenerating.isEmpty {
            return "AI 识别中（智能蒙版）…"
        }
        return nil
    }

    @ViewBuilder
    private var editorColumn: some View {
        if !hidesChrome {
            // 拖动滑块 / 沉浸查看时顶部栏只隐藏不位移：
            // 图片位置不跳动（保持从灵动岛下面开始），轻点 / 长按也不会让画面移位。
            topBar
                .opacity(model.draggingSlider == nil && !immersive ? 1 : 0)
                .allowsHitTesting(model.draggingSlider == nil && !immersive)
        }
        previewArea
    }

    /// HSL / 颜色均衡器专注模式：进子页后隐藏所有导航（含底部 tab），只留「完成」退出。
    private var hslFocused: Bool {
        model.tool == .edit && model.category == .color
            && (model.colorTab == .hsl || model.colorTab == .equalizer)
    }

    /// 曲线 / 蒙版 / HSL 下隐藏顶部栏与底部工具条，把空间让给编辑区。
    private var hidesChrome: Bool {
        model.tool == .mask
            || (model.tool == .edit && model.category == .curves)
            || hslFocused
    }

    /// 预设 / 修图下隐藏底部工具条：修图是独立工作台（子页签在面板里），
    /// 不再需要底部那排工具；预设列表占满下方空间。
    private var hidesToolBar: Bool {
        hidesChrome || model.tool == .presets || model.tool == .retouch
    }

    private var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "slider.horizontal.3")
                .font(.system(size: 44))
                .foregroundStyle(Color.accentColor)
            Text("还没有选择照片")
                .font(.headline)
            Text("去「设备」Tab 点开一张照片，再点右上角的「编辑」。")
                .font(.footnote)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
        .padding(32)
    }

    // MARK: - 顶栏

    private var topBar: some View {
        HStack(spacing: 14) {
            Button {
                onClose()
            } label: {
                Image(systemName: "chevron.backward")
            }
            VStack(alignment: .leading, spacing: 1) {
                Text(model.project?.name ?? model.assetName)
                    .font(.footnote)
                    .lineLimit(1)
                HStack(spacing: 6) {
                    Text(model.presetName)
                    if let millis = model.renderMillis {
                        Text(String(format: "CPU · %.0f ms", millis))
                    }
                }
                .font(.caption2)
                .foregroundStyle(.secondary)
            }
            Spacer()
            // 修图 / 调色一键切换（带工作台加载动画）。
            Button {
                model.switchWorkbench()
            } label: {
                HStack(spacing: 4) {
                    Image(systemName: model.tool == .retouch ? "wand.and.rays" : "slider.horizontal.3")
                        .font(.caption)
                    Text(model.tool == .retouch ? "修图" : "调色")
                        .font(.caption.weight(.semibold))
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(
                    Capsule().fill(Color.accentColor.opacity(0.22))
                )
                .foregroundStyle(Color.accentColor)
            }
            .accessibilityLabel("切换修图 / 调色")
            Button {
                guard Date() >= suppressUndoTapUntil else { return }
                model.undo()
            } label: {
                Image(systemName: "arrow.uturn.backward")
            }
            .disabled(!model.canUndo)
            // 长按撤销键：弹出历史列表，可一次回退多步。
            .simultaneousGesture(
                LongPressGesture(minimumDuration: 0.4, maximumDistance: 12)
                    .onEnded { _ in
                        guard model.canUndo else { return }
                        suppressUndoTapUntil = Date().addingTimeInterval(0.5)
                        Haptics.light()
                        showHistory = true
                    }
            )
            Button {
                Haptics.tick()
                model.redo()
            } label: {
                Image(systemName: "arrow.uturn.forward")
            }
            .disabled(!model.canRedo)
            Button {
                Haptics.light()
                if compareSplit == nil {
                    compareSplit = 0.5
                    model.prepareBeforeImage()
                } else {
                    compareSplit = nil
                    model.clearBeforeImage()
                }
                // 进入 / 退出对比都要刷新一帧（对比模式下零拷贝上屏暂停，需要回读画面）。
                model.setComparePreviewActive(compareSplit != nil)
                model.refreshPreview()
            } label: {
                Image(systemName: "rectangle.split.2x1")
                    .foregroundStyle(
                        compareSplit == nil ? Color.white.opacity(0.9) : Color.accentColor
                    )
            }
            // 导出进度（导出入口在「•••」菜单里）。
            if let progress = model.exportProgress {
                Text("\(Int(progress * 100))%")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(Color.accentColor)
            }
            // 最右边：••• 更多（从下往上弹框：导出 / 同步 / 重置）。
            Button {
                Haptics.light()
                sheetSelection = asset.map { [$0.localIdentifier] }.map(Set.init) ?? []
                activeSheet = .more
            } label: {
                Image(systemName: "ellipsis")
                    .font(.body.weight(.semibold))
                    .frame(width: 30, height: 26)
            }
            .accessibilityLabel("更多（导出 / 同步 / 重置）")
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .background(Color(white: 0.11))
    }

    // MARK: - 预览

    private var previewImageSize: CGSize {
        // 零拷贝上屏时 previewImage 可能是旧帧，布局几何以最近一帧输出尺寸为准。
        model.previewOutputSize
            ?? model.previewImage?.size
            ?? model.originalImage?.size
            ?? CGSize(width: 4, height: 3)
    }

    /// 当前显示的图像：原图对比 > 放大细节帧 > 普通预览。
    private var shownImage: UIImage? {
        if model.showOriginal {
            return model.originalImage ?? model.previewImage
        }
        if zoom > 1.01, let detail = model.detailImage {
            return detail
        }
        return model.previewImage
    }

    /// 新照片的预取预览（切换瞬间先顶上，渲染好再替换——切换不再"先糊后清"）。
    private var placeholderImage: UIImage? {
        guard model.previewImage == nil else { return nil }
        guard let asset else { return nil }
        return neighborImages[asset.localIdentifier]
    }

    /// 单张照片图层：按**这张图自己的比例**做 aspect-fit + 上移（横向滑动时
    /// 每张照片的姿态和它加载完成后完全一致，不会因为尺寸取错而错位/变形）。
    private func photoLayer(_ image: UIImage, in size: CGSize) -> some View {
        let fitted = fittedRect(imageSize: image.size, in: size)
        return Image(uiImage: image)
            .resizable()
            .scaledToFit()
            .frame(width: max(fitted.width, 1), height: max(fitted.height, 1))
            .position(x: fitted.midX, y: fitted.midY)
    }

    private var overlayEnabled: Bool {
        model.tool != .crop && model.tool != .mask
    }

    /// 横图 / 方图的上移量：顶距从"剩余空间 1/2（居中）"先后两次各上移 1/3，
    /// 收到 2/9（与 fittedRect 一致）。
    /// 竖图（撑满高度）为 0，不受影响。
    private func fittedLift(imageSize: CGSize, viewSize: CGSize) -> CGFloat {
        let fitted = fittedRect(imageSize: imageSize, in: viewSize)
        return max(0, (viewSize.height - fitted.height) * 5 / 18)
    }

    private var previewArea: some View {
        GeometryReader { geo in
            Group {
                if model.tool == .mask {
                    // 蒙版模式：触摸全部交给 MaskTouchLayer（单指操作蒙版，双指缩放 / 平移）。
                    previewBody(geo: geo)
                } else if model.tool == .edit && model.category == .curves {
                    // 曲线：图片上的曲线编辑区自己收手势（长按看原图由它转发）。
                    previewBody(geo: geo)
                } else {
                    previewBody(geo: geo)
                        .contentShape(Rectangle())
                        .gesture(previewGesture(viewWidth: geo.size.width))
                        .simultaneousGesture(photoTouchGesture(viewSize: geo.size))
                }
            }
            .frame(width: geo.size.width, height: geo.size.height)
            .onAppear {
                previewAreaSize = geo.size
            }
            .onChange(of: geo.size) { value in
                previewAreaSize = value
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        // 放大后的图片不能盖住上面的导航栏：裁在预览区内。
        .clipped()
    }

    @ViewBuilder
    private func previewBody(geo: GeometryProxy) -> some View {
        ZStack {
            Color.black
            // 相册式左右滑动：两侧邻居照片随手指滑入（低清预览，加载完成后替换）。
            if swipeOffset > 1,
               let prev = prevAsset,
               let image = neighborImages[prev.localIdentifier]
            {
                photoLayer(image, in: geo.size)
                    .offset(x: swipeOffset - geo.size.width)
                    .allowsHitTesting(false)
            }
            if swipeOffset < -1,
               let next = nextAsset,
               let image = neighborImages[next.localIdentifier]
            {
                photoLayer(image, in: geo.size)
                    .offset(x: swipeOffset + geo.size.width)
                    .allowsHitTesting(false)
            }
            if let split = compareSplit, let before = model.beforeImage, let edited = shownImage {
                // 对比模式：左「调色前」/ 右「调色后」，分割线可拖。
                Image(uiImage: before)
                    .resizable()
                    .scaledToFit()
                    .frame(width: geo.size.width, height: geo.size.height)
                    .offset(y: -fittedLift(imageSize: previewImageSize, viewSize: geo.size))
                Image(uiImage: edited)
                    .resizable()
                    .scaledToFit()
                    .frame(width: geo.size.width, height: geo.size.height)
                    .mask(CompareSplitShape(fraction: split))
                    .offset(y: -fittedLift(imageSize: previewImageSize, viewSize: geo.size))
                // 分割线 + 手柄。
                Rectangle()
                    .fill(Color.white.opacity(0.9))
                    .frame(width: 2)
                    .position(x: geo.size.width * split, y: geo.size.height / 2)
                Circle()
                    .fill(.ultraThinMaterial)
                    .overlay(Circle().stroke(Color.white.opacity(0.7)))
                    .frame(width: 30, height: 30)
                    .overlay(
                        Image(systemName: "arrow.left.and.right")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(.white)
                    )
                    .position(x: geo.size.width * split, y: geo.size.height / 2)
            } else if shownImage != nil || model.zeroCopyActive || placeholderImage != nil {
                // 预览画面：Metal 图层常驻（零拷贝上屏用，未激活时透明不挡 Image），
                // 回读画面盖在上面（零拷贝生效 / 看原图 / 对比模式下隐藏）。
                let fitted = fittedRect(imageSize: previewImageSize, in: geo.size)
                let metalActive = model.zeroCopyActive && !model.showOriginal && compareSplit == nil
                ZStack {
                    MetalPreviewView { pointer, token in
                        model.attachPreviewLayer(pointer, token: token)
                    }
                    .frame(width: fitted.width, height: fitted.height)
                    .position(x: fitted.midX, y: fitted.midY)
                    .opacity(metalActive ? 1 : 0)
                    if !metalActive, let image = shownImage ?? placeholderImage {
                        // 和 fittedRect / 叠加层几何一致；按图片自身比例定位（切换不错位）。
                        photoLayer(image, in: geo.size)
                            // 切换照片：新帧淡入（配合 ContentView 的 withAnimation）。
                            .id(asset?.localIdentifier)
                            .transition(.opacity)
                    }
                }
                .frame(width: geo.size.width, height: geo.size.height)
                .scaleEffect(zoom)
                // 手指跟随时整块内容平移（相册式滑动）。
                .offset(x: pan.width + (zoom <= 1.01 ? swipeOffset : 0), y: pan.height)
            } else if model.isLoading {
                // 首帧还没出来：占位保持布局，真正的加载提示由下面的居中弹框负责，
                // 避免同时出现「居中转圈」和「弹框」两套加载 UI。
                Color.clear
            }
            // 切图 / 首帧加载：居中弹框（半透明遮罩 + 圆角卡片），加载期间拦截所有点击，
            // 加载完自动消失，避免先看到原图再跳成调色结果。
            if model.isLoading {
                ZStack {
                    Color.black.opacity(0.35).ignoresSafeArea()
                    VStack(spacing: 12) {
                        ProgressView()
                            .tint(.white)
                        Text(model.isRestoringEdits ? "正在应用上次的调色…" : "正在加载照片…")
                            .font(.footnote)
                            .foregroundStyle(.white.opacity(0.9))
                    }
                    .padding(.horizontal, 28)
                    .padding(.vertical, 22)
                    .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 16))
                }
                .transition(.opacity)
                .zIndex(30)
            }
            // 放大后的 1:1 区域帧（核心按帧外坐标渲染），按归一化矩形对齐贴上。
            if model.tool != .mask,
               let region = model.detailRegion,
               zoom > 1.01,
               detailRegionCovers(region.rect, viewSize: geo.size)
            {
                DetailRegionLayer(
                    image: region.image,
                    rect: region.rect,
                    imageSize: previewImageSize,
                    viewSize: geo.size,
                    zoom: zoom,
                    pan: pan
                )
            }
            if model.tool == .crop {
                CropOverlayView(model: model, imageSize: previewImageSize)
            }
            if model.tool == .mask {
                MaskOverlayLayer(
                    model: model,
                    imageSize: previewImageSize,
                    viewSize: geo.size,
                    zoom: zoom,
                    pan: pan
                )
                MaskTouchLayer { event in
                    handleMaskTouch(event, viewSize: geo.size)
                }
                .frame(width: geo.size.width, height: geo.size.height)
            }
            // 修图手动工具（污点修复 / 液化）：点按 / 拖动直接作用在图上。
            if model.tool == .retouch, model.retouchMode != .beauty {
                RetouchTouchLayer(
                    model: model,
                    imageSize: previewImageSize,
                    viewSize: geo.size,
                    zoom: zoom,
                    pan: pan
                )
            }
            // 点颜色取色：点按画布取样（几何与蒙版 / 修图同源）。
            if model.tool == .edit,
               model.category == .color,
               model.colorTab == .pointColor,
               model.pointSampling
            {
                PointColorTouchLayer(
                    model: model,
                    imageSize: previewImageSize,
                    viewSize: geo.size,
                    zoom: zoom,
                    pan: pan
                )
            }
            // 曲线：正方形透明编辑区直接叠在图片上（无背景板）。
            if model.tool == .edit && model.category == .curves {
                CurveOverlay(model: model, viewSize: geo.size) { active in
                    if active {
                        beginHoldPreview()
                    } else {
                        endHoldPreview()
                    }
                }
            }
            // 拖动滑块时隐藏所有 UI（只留滑块 + 画面）。
            if !immersive && model.draggingSlider == nil {
                VStack {
                    HStack {
                        if model.isRendering {
                            ProgressView()
                                .controlSize(.small)
                                .tint(.white)
                        }
                        Spacer()
                        if model.tool != .crop && model.tool != .mask {
                            HistogramOverlay(data: model.histogram)
                                // 直方图加长：屏幕宽度的 2/3。
                                .frame(
                                    width: max(220, geo.size.width * 2 / 3),
                                    height: 92
                                )
                        }
                    }
                    Spacer()
                }
                .padding(10)
            }
        }
    }

    /// 蒙版触摸事件 → 蒙版控件 / 画布操作。
    private func handleMaskTouch(_ event: MaskTouchEvent, viewSize: CGSize) {
        let geometry = MaskCanvasGeometry(
            imageSize: previewImageSize,
            viewSize: viewSize,
            zoom: zoom,
            pan: pan
        )
        maskController.handle(
            event,
            geometry: geometry,
            model: model,
            zoom: $zoom,
            pan: $pan
        )
    }

    /// 预览手势：双指缩放 + （放大后）单指平移；对比模式下单指拖动分割线。
    private func previewGesture(viewWidth: CGFloat) -> some Gesture {
        let magnify = MagnificationGesture()
            .onChanged { value in
                guard overlayEnabled else { return }
                lastPinchAt = Date()
                // 放到"能看到毛孔"的倍率（1:1 之上继续放大，细节帧按区域渲染）。
                zoom = (baseZoom * value).clamped(to: 1...24)
            }
            .onEnded { _ in
                guard overlayEnabled else { return }
                baseZoom = zoom
                if zoom <= 1.01 {
                    resetZoom()
                }
            }
        let drag = DragGesture(minimumDistance: 12)
            .onChanged { value in
                if compareSplit != nil {
                    compareSplit = (value.location.x / max(viewWidth, 1)).clamped(to: 0.05...0.95)
                    return
                }
                guard overlayEnabled else { return }
                if zoom > 1 {
                    pan = CGSize(
                        width: basePan.width + value.translation.width,
                        height: basePan.height + value.translation.height
                    )
                    return
                }
                // 图库式左右滑动：内容跟着手指走，边缘带阻尼。
                var offset = value.translation.width
                if (offset > 0 && !canSwipePrev) || (offset < 0 && !canSwipeNext) {
                    offset *= 0.28
                }
                swipeOffset = offset
            }
            .onEnded { value in
                if zoom > 1 {
                    basePan = pan
                    return
                }
                guard overlayEnabled, compareSplit == nil else { return }
                endSwipe(value)
            }
        return SimultaneousGesture(magnify, drag)
    }

    /// 松手收尾：过半 / 快速甩动 → 切图（回弹动画），否则弹回原位。
    private func endSwipe(_ value: DragGesture.Value) {
        guard !swipeAnimating else { return }
        let translation = value.translation
        guard abs(translation.width) >= abs(translation.height) * 1.2 else {
            withAnimation(.spring(response: 0.3, dampingFraction: 0.86)) {
                swipeOffset = 0
            }
            return
        }
        guard Date().timeIntervalSince(lastPinchAt) > 0.35 else {
            withAnimation(.spring(response: 0.3, dampingFraction: 0.86)) {
                swipeOffset = 0
            }
            return
        }
        let width = max(previewAreaSize.width, 1)
        let velocity = value.predictedEndTranslation.width - translation.width
        let forward = translation.width < 0
        let commit = abs(translation.width) > width * 0.2 || abs(velocity) > 650
        if commit, forward ? canSwipeNext : canSwipePrev {
            let direction: CGFloat = forward ? -1 : 1
            swipeAnimating = true
            Haptics.light()
            withAnimation(.easeOut(duration: 0.2)) {
                swipeOffset = direction * width
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
                if forward {
                    onSwipeNext()
                } else {
                    onSwipePrev()
                }
                // 新照片从另一侧滑入。
                swipeOffset = -direction * width * 0.3
                withAnimation(.easeOut(duration: 0.18)) {
                    swipeOffset = 0
                }
                swipeAnimating = false
            }
        } else {
            withAnimation(.spring(response: 0.3, dampingFraction: 0.86)) {
                swipeOffset = 0
            }
        }
    }

    /// 未放大时的左右滑动切图：横向位移要足够大且明显占主导；
    /// 刚做过双指缩放的短时间内不判定（捏合松手瞬间的收拢不算滑动）。
    private func handleSwipeToSwitch(_ translation: CGSize) {
        let horizontal = translation.width
        guard abs(horizontal) >= 64, abs(horizontal) >= abs(translation.height) * 1.6 else { return }
        guard Date().timeIntervalSince(lastPinchAt) > 0.35 else { return }
        if horizontal > 0 {
            guard canSwipePrev else { return }
            Haptics.light()
            onSwipePrev()
        } else {
            guard canSwipeNext else { return }
            Haptics.light()
            onSwipeNext()
        }
    }

    /// 预览区触摸：轻点 / 双击 / 长按都在这里判定，单击不再等双击判定 → 点击没有延迟。
    ///
    /// - 轻点：切换沉浸（隐藏 / 显示 UI），立刻生效；
    /// - 双击（300ms 内两次轻点）：放大 / 还原，并把单击造成的沉浸切换还原；
    /// - 长按 0.5s：看原图（同时隐藏 UI），松手恢复；长按期间移动手指不会取消；
    /// - 直方图卡片里的触摸交给直方图自己（点一下切通道 / 长按切换剪裁警告）；
    /// - 拖动（缩放后平移 / 左右滑动切图）超过 12pt 就不再算轻点。
    private func photoTouchGesture(viewSize: CGSize) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { value in
                if !photoTouchActive {
                    photoTouchActive = true
                    photoTouchMoved = false
                    photoHoldActive = false
                    // 直方图卡片自己收手势。
                    guard !histogramFrame(in: viewSize).contains(value.startLocation) else {
                        return
                    }
                    let work = DispatchWorkItem {
                        guard photoTouchActive, !photoTouchMoved, !photoHoldActive else { return }
                        photoHoldActive = true
                        beginHoldPreview()
                    }
                    photoHoldWork = work
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.5, execute: work)
                    return
                }
                if !photoTouchMoved,
                   hypot(value.translation.width, value.translation.height) > 12 {
                    photoTouchMoved = true
                    photoHoldWork?.cancel()
                    photoHoldWork = nil
                }
            }
            .onEnded { value in
                photoHoldWork?.cancel()
                photoHoldWork = nil
                let wasActive = photoTouchActive
                let moved = photoTouchMoved
                let held = photoHoldActive
                photoTouchActive = false
                photoTouchMoved = false
                photoHoldActive = false
                guard wasActive else { return }
                if held {
                    endHoldPreview()
                    return
                }
                guard !moved, overlayEnabled else { return }
                guard !histogramFrame(in: viewSize).contains(value.startLocation) else { return }
                // 刚捏合完的瞬间不算轻点（捏合松手时两根手指的收拢）。
                guard Date().timeIntervalSince(lastPinchAt) > 0.25 else { return }
                let now = Date()
                if now.timeIntervalSince(lastPhotoTapAt) < 0.35 {
                    // 双击：放大 / 还原，并把第一击的沉浸切换还原。
                    lastPhotoTapAt = .distantPast
                    withAnimation(.easeInOut(duration: 0.2)) {
                        immersive = immersiveBeforeTap
                    }
                    toggleZoom()
                } else {
                    lastPhotoTapAt = now
                    immersiveBeforeTap = immersive
                    withAnimation(.easeInOut(duration: 0.2)) {
                        immersive.toggle()
                    }
                }
            }
    }

    /// 长按看原图：隐藏全部 UI 沉浸查看（松手按长按前的状态恢复）。
    /// 照片区和曲线编辑区共用；长按期间曲线 / 参数都不变化。
    private func beginHoldPreview() {
        guard overlayEnabled, !model.showOriginal else { return }
        Haptics.medium()
        immersiveBeforeHold = immersive
        model.showOriginal = true
        withAnimation(.easeInOut(duration: 0.18)) {
            immersive = true
        }
    }

    private func endHoldPreview() {
        guard model.showOriginal else { return }
        model.showOriginal = false
        withAnimation(.easeInOut(duration: 0.18)) {
            immersive = immersiveBeforeHold
        }
    }

    /// 直方图卡片在预览区里的位置（与 previewBody 的 HUD 布局一致，外加一点手指容差）。
    private func histogramFrame(in viewSize: CGSize) -> CGRect {
        CGRect(
            x: viewSize.width - 132 - 10,
            y: 10,
            width: 132,
            height: 76
        )
        .insetBy(dx: -6, dy: -6)
    }

    /// 双击：放大 / 还原。
    private func toggleZoom() {
        guard overlayEnabled else { return }
        withAnimation(.easeInOut(duration: 0.22)) {
            if zoom > 1.01 {
                zoom = 1
                baseZoom = 1
                pan = .zero
                basePan = .zero
            } else {
                zoom = 2.5
                baseZoom = 2.5
            }
        }
    }

    private func resetZoom() {
        zoom = 1
        baseZoom = 1
        pan = .zero
        basePan = .zero
    }

    // MARK: - 底部面板

    private var panelHeight: CGFloat {
        switch model.tool {
        // 预设树（文件夹 + 预设竖排）比原来的卡片行高一些，给列表多留窗口。
        // 预设树：底部 tabbar 已隐藏，列表高一些、每项也更高（缩略图 64×42）。
        case .presets: return 300
        case .edit:
            if categoryCollapsed {
                return 64
            }
            if model.category == .curves {
                // 曲线面板压到最下面：普通通道只需要"标题 + 提示 + 通道 + 完成"，
                // 给一块矮面板（少挡画面）；参数通道再高一些，给滑块留窗口。
                return model.curveChannel == .parametric ? 288 : 172
            }
            if model.category == .optics {
                // 光学只有两个开关：面板压矮，少挡画面。
                return 224
            }
            if model.category == .color, model.colorTab == .equalizer {
                // 颜色均衡器：曲线图占的空间越大，8 个节点越好点。
                // 想再高/再矮改这个数（也可以写成 UIScreen.main.bounds.height * 0.44）。
                return 380
            }
            return 288
        case .crop: return 210
        // 蒙版：还没建蒙版时只留新建行（矮条）；创建后调整页才从底部弹起。
        // 分类行压矮后，把高度让给上面的参数（滑块）区。
        case .mask: return model.selectedMask == nil ? 150 : 300
        // 修图工作台：矮一些，把画面留给手动修图（预设 / 修复 / 液化）。
        case .retouch: return 246
        }
    }

    private var panel: some View {
        VStack(spacing: 0) {
            Divider()
                .overlay(Color.white.opacity(0.08))
                .opacity(model.draggingSlider == nil ? 1 : 0)
            Group {
                switch model.tool {
                case .presets:
                    PresetPanelView(model: model)
                case .crop:
                    cropPanel
                case .edit:
                    editPanel
                case .mask:
                    MaskPanelView(model: model)
                case .retouch:
                    RetouchPanelView(model: model)
                }
            }
            .frame(height: panelHeight)
            // 主工具条固定在最底部（Lightroom / Android 布局）；曲线 / 蒙版 / 预设下隐藏。
            if !hidesToolBar {
                // 沉浸拖动：工具条只隐藏不位移（滑块保持原位）。
                toolBar
                    .opacity(model.draggingSlider == nil ? 1 : 0)
                    .allowsHitTesting(model.draggingSlider == nil)
            }
        }
        .frame(maxWidth: .infinity)
        // 真透明（不是毛玻璃）：后面露一点照片，方便对比调色效果；
        // 拖动滑块时连底色一起隐掉，只留滑块和画面（Lightroom 做法）。
        // 修图工作台再透一些（竖图铺满屏幕，面板浮在上面）。
        .background(
            Color(white: 0.07)
                .opacity(
                    model.draggingSlider != nil
                        ? 0
                        : (model.tool == .retouch ? 0.5 : 0.78)
                )
        )
    }

    private var toolBar: some View {
        HStack(spacing: 0) {
            // 修图不占底部工具位（由顶部的「修图 / 调色」切换进入独立工作台）。
            ForEach(EditorTool.allCases.filter { $0 != .retouch }) { tool in
                Button {
                    if model.tool != tool {
                        Haptics.tick()
                    }
                    model.selectTool(tool)
                } label: {
                    VStack(spacing: 3) {
                        Image(systemName: tool.icon)
                            .font(.footnote)
                        Text(tool.rawValue)
                            .font(.caption2)
                    }
                    .frame(maxWidth: .infinity)
                    .foregroundStyle(
                        model.tool == tool ? Color.accentColor : Color.white.opacity(0.7)
                    )
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.vertical, 7)
    }

    // MARK: - 编辑面板（亮度 / 颜色 / 效果 / 细节）

    /// 布局（自下而上）：工具条（最底）→ 分类导航 → 参数 → 子页标签（最上）。
    private var editPanel: some View {
        VStack(spacing: 0) {
            if hslFocused {
                // HSL 专注：只有色带选择 + 三个滑块 + 完成。
                editContent
                doneRow {
                    model.colorTab = .base
                }
                .opacity(model.draggingSlider == nil ? 1 : 0)
                .allowsHitTesting(model.draggingSlider == nil)
            } else if !categoryCollapsed {
                editSubTabs
                    .opacity(model.draggingSlider == nil ? 1 : 0)
                    .allowsHitTesting(model.draggingSlider == nil)
                editContent
                Divider()
                    .overlay(Color.white.opacity(0.08))
                    .opacity(model.draggingSlider == nil ? 1 : 0)
                if model.category == .curves {
                    // 曲线：分类导航整排隐形，只留一个「完成」退出。
                    doneRow {
                        model.exitCurves()
                    }
                    .opacity(model.draggingSlider == nil ? 1 : 0)
                    .allowsHitTesting(model.draggingSlider == nil)
                } else {
                    categoryRow
                        .opacity(model.draggingSlider == nil ? 1 : 0)
                        .allowsHitTesting(model.draggingSlider == nil)
                }
            } else {
                categoryRow
                    .opacity(model.draggingSlider == nil ? 1 : 0)
                    .allowsHitTesting(model.draggingSlider == nil)
            }
        }
    }

    /// 专注模式（曲线 / HSL）底部的退出按钮。
    private func doneRow(_ action: @escaping () -> Void) -> some View {
        HStack {
            Spacer()
            Button("完成") {
                Haptics.tick()
                withAnimation(.easeInOut(duration: 0.24)) {
                    action()
                }
            }
            .font(.subheadline)
            .foregroundStyle(Color.accentColor)
            .padding(.vertical, 16)
            Spacer()
        }
    }

    /// 最上面的子页标签（可切换）：颜色/HSL/分级、效果/晕影/颗粒、锐化/降噪。
    @ViewBuilder
    private var editSubTabs: some View {
        if model.category == .color {
            subTabRow(
                ColorTab.allCases.map(\.rawValue),
                selected: ColorTab.allCases.firstIndex(of: model.colorTab) ?? 0
            ) { index in
                model.colorTab = ColorTab.allCases[index]
            }
        } else if model.category == .effects {
            subTabRow(
                EffectTab.allCases.map(\.rawValue),
                selected: EffectTab.allCases.firstIndex(of: model.effectTab) ?? 0
            ) { index in
                model.effectTab = EffectTab.allCases[index]
            }
        } else if model.category == .detail {
            subTabRow(
                DetailTab.allCases.map(\.rawValue),
                selected: DetailTab.allCases.firstIndex(of: model.detailTab) ?? 0
            ) { index in
                model.detailTab = DetailTab.allCases[index]
            }
        }
    }

    /// 参数区（滑块 / HSL / 分级 / 曲线 / 光学）。
    @ViewBuilder
    private var editContent: some View {
        if model.category == .curves {
            CurvePanelView(model: model)
        } else if model.category == .color, model.colorTab == .hsl {
            hslPanel
        } else if model.category == .color, model.colorTab == .equalizer {
            equalizerPanel
        } else if model.category == .color, model.colorTab == .grading {
            gradingPanel
        } else if model.category == .color, model.colorTab == .pointColor {
            pointColorPanel
        } else if model.category == .color, model.colorTab == .calibration {
            calibrationPanel
        } else if model.category == .color, model.colorTab == .blackWhite {
            blackWhitePanel
        } else if model.category == .optics {
            opticsPanel
        } else {
            sliderList
        }
    }

    private var currentSliders: [SliderSpec] {
        EditorSpec.visibleSliders(
            category: model.category,
            effectTab: model.effectTab,
            detailTab: model.detailTab
        )
    }

    private var sliderList: some View {
        ScrollView {
            LazyVStack(spacing: 0) {
                ForEach(currentSliders) { spec in
                    sliderRow(spec)
                        .opacity(model.sliderVisible(spec.label) ? 1 : 0)
                        .allowsHitTesting(model.sliderVisible(spec.label))
                }
            }
            .padding(.vertical, 8)
        }
        // 拖动滑块中：禁止面板上下滚动（只调值）。
        .scrollDisabled(model.draggingSlider != nil)
    }

    private var categoryRow: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            // 分类项之间收紧（6 项含「光学」后间距太大，亮度 / 曲线之间尤其明显）。
            HStack(spacing: 4) {
                ForEach(EditCategory.allCases) { category in
                    categoryButton(category)
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
    }

    /// 分类按钮（自己判定触摸）：轻点展开 / 收起，长按只做"这一类参数复原"对比。
    private func categoryButton(_ category: EditCategory) -> some View {
        Button {
            // 长按对比松手时 Button 也会触发一次点击：这里吞掉，别顺带展开 / 收起。
            // （两种回调顺序都兜住；即使手势回调缺失，下一次点按也能自救。）
            if categoryHoldActive || Date() < categorySuppressTapUntil {
                categoryHoldActive = false
                return
            }
            toggleCategory(category)
        } label: {
            VStack(spacing: 5) {
                Image(systemName: category.icon)
                    .font(.system(size: 16))
                Text(category.rawValue)
                    .font(.caption2)
            }
            .frame(width: 54)
            .padding(.vertical, 7)
            .background(
                RoundedRectangle(cornerRadius: 10)
                    .fill(
                        model.category == category
                            ? Color.white.opacity(0.14)
                            : Color.clear
                    )
            )
            .foregroundStyle(
                model.category == category
                    ? Color.white
                    : Color.white.opacity(0.6)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .simultaneousGesture(categoryHoldGesture(category))
    }

    /// 点分类：从下往上展开参数；再点同一个分类收起。
    private func toggleCategory(_ category: EditCategory) {
        Haptics.light()
        withAnimation(.easeInOut(duration: 0.24)) {
            if model.category == category {
                categoryCollapsed.toggle()
            } else {
                model.setCategory(category)
                categoryCollapsed = false
            }
        }
    }

    /// 长按分类（0.5s）：临时把该分类参数归零，只看"复原后"的样子，松手恢复。
    ///
    /// 用系统长按识别 + 串接拖动（0.5.0 起一直用的方式，稳定拿到"按住 0.5s"和"松手"）；
    /// 长按期间禁止点击、松手后短时间内也吞掉那次点击——两种回调顺序都不会顺带展开 / 收起。
    private func categoryHoldGesture(_ category: EditCategory) -> some Gesture {
        LongPressGesture(minimumDuration: 0.5, maximumDistance: 40)
            .sequenced(before: DragGesture(minimumDistance: 0))
            .onChanged { value in
                if case .second(true, _) = value {
                    guard !categoryHoldActive else { return }
                    categoryHoldActive = true
                    Haptics.medium()
                    model.beginCategoryPreview(category)
                }
            }
            .onEnded { _ in
                // 轻点（长按没成立）时不做任何事，别把这次正常点按吞掉。
                guard categoryHoldActive else { return }
                categoryHoldActive = false
                categorySuppressTapUntil = Date().addingTimeInterval(0.4)
                model.endCategoryPreview()
            }
    }

    private func subTabRow(
        _ labels: [String],
        selected: Int,
        onSelect: @escaping (Int) -> Void
    ) -> some View {
        // 颜色分类现在有 6 个子页：标签多于 4 个时收紧间距，小屏也放得下。
        let spacing: CGFloat = labels.count > 4 ? 15 : 26
        return HStack(spacing: spacing) {
            Spacer(minLength: 0)
            ForEach(Array(labels.enumerated()), id: \.offset) { index, label in
                Button {
                    onSelect(index)
                } label: {
                    VStack(spacing: 3) {
                        Text(label)
                            .font(.subheadline)
                            .fontWeight(selected == index ? .semibold : .regular)
                            .foregroundStyle(
                                selected == index ? Color.white : Color.white.opacity(0.55)
                            )
                        Rectangle()
                            .fill(selected == index ? Color.accentColor : Color.clear)
                            .frame(width: 24, height: 2)
                    }
                }
                .buttonStyle(.plain)
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 16)
        .padding(.top, 10)
        .padding(.bottom, 2)
    }

    private func sliderRow(_ spec: SliderSpec) -> some View {
        ValueSliderRow(
            label: spec.label,
            value: model.value(spec.key),
            range: spec.range,
            neutral: spec.defaultValue,
            step: step(for: spec),
            decimals: spec.key == "exposure"
                ? 2
                : (spec.range.upperBound - spec.range.lowerBound <= 3 ? 1 : 0),
            onBegin: { model.beginEdit() },
            onChange: { model.updateValue(spec.key, $0) },
            onCommit: { model.commitEdit() },
            trackColors: trackColors(for: spec.key)
        )
    }

    /// 颜色类滑块的轨道配色（和 Lightroom 一样用颜色区分调节方向，粗细不变）。
    private func trackColors(for key: String) -> [Color]? {
        switch key {
        case "temperature":
            return [
                Color(red: 0.30, green: 0.45, blue: 1.0),
                Color(white: 0.85),
                Color(red: 1.0, green: 0.82, blue: 0.30),
            ]
        case "tint":
            return [
                Color(red: 0.30, green: 0.90, blue: 0.45),
                Color(white: 0.85),
                Color(red: 0.85, green: 0.30, blue: 0.95),
            ]
        case "vibrance":
            return [
                Color(white: 0.45),
                Color(red: 0.25, green: 0.80, blue: 0.55),
                Color(red: 0.55, green: 0.35, blue: 0.95),
            ]
        case "saturation":
            return [Color(white: 0.5), Color(red: 0.95, green: 0.30, blue: 0.30)]
        default:
            return nil
        }
    }

    /// HSL 三个滑块的轨道配色（按当前色带的颜色）。
    private func hslTrackColors(_ channel: String) -> [Color] {
        let kind = model.hslBand
        let tint = Color(red: kind.tint.r, green: kind.tint.g, blue: kind.tint.b)
        switch channel {
        case "hue":
            // 和 Lightroom 一样：左 = 上一个色相、中 = 本色带、右 = 下一个色相
            // （绿色带 → 左黄、中绿、右青）。
            return [neighborTint(kind, offset: -1), tint, neighborTint(kind, offset: 1)]
        case "saturation":
            return [Color(white: 0.45), tint]
        default:
            return [Color(white: 0.1), tint, Color(white: 1.0)]
        }
    }

    /// 环形相邻色带的颜色（用于色相轨道两端）。
    private func neighborTint(_ kind: HslBandKind, offset: Int) -> Color {
        let all = HslBandKind.allCases
        guard let index = all.firstIndex(of: kind) else { return Color.white }
        let count = all.count
        let neighbor = all[((index + offset) % count + count) % count]
        return Color(red: neighbor.tint.r, green: neighbor.tint.g, blue: neighbor.tint.b)
    }

    private func step(for spec: SliderSpec) -> Float {
        if spec.key == "exposure" {
            return 0.1
        }
        if spec.range.upperBound - spec.range.lowerBound <= 3 {
            return 0.05
        }
        return 5
    }

    // MARK: - HSL

    // MARK: - 颜色均衡器（对标 darktable color equalizer）

    private var equalizerPanel: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Picker("", selection: $eqChannel) {
                    Text("色相").tag(0)
                    Text("饱和度").tag(1)
                    Text("亮度").tag(2)
                }
                .pickerStyle(.segmented)
                // 选项按钮：平时只留曲线，点了才展开下面的选项滑块。
                Button {
                    eqShowOptions.toggle()
                    Haptics.tick()
                } label: {
                    Image(systemName: "slider.horizontal.3")
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(eqShowOptions ? Color.accentColor : Color.secondary)
                        .frame(width: 34, height: 30)
                        .background(Color.white.opacity(eqShowOptions ? 0.18 : 0.08))
                        .clipShape(RoundedRectangle(cornerRadius: 7))
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 14)
            .padding(.top, 8)
            .padding(.bottom, 6)
            .opacity(model.draggingSlider == nil ? 1 : 0)
            .allowsHitTesting(model.draggingSlider == nil)

            EqualizerGraph(
                channel: eqChannel,
                state: model.state.equalizer,
                onBegin: { model.beginEdit() },
                onChange: { updated in
                    model.updateEqualizer(updated, commit: false)
                },
                onCommit: { model.commitEdit() }
            )
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .layoutPriority(1)
            .padding(.horizontal, 10)
            .padding(.bottom, 8)

            if eqShowOptions {
                equalizerOptions
                    .frame(maxHeight: 210)
            }
        }
    }

    private var equalizerOptions: some View {
        let eq = model.state.equalizer
        return ScrollView {
            VStack(spacing: 0) {
                Text("拖动节点改当前通道；节点整体旋转在下面。")
                    .font(.system(size: 10))
                    .foregroundStyle(.secondary)
                    .padding(.top, 6)
                    .padding(.bottom, 2)
                ValueSliderRow(
                    label: "阈值",
                    value: eq.threshold,
                    range: 0...0.3,
                    neutral: 0.1,
                    step: 0.01,
                    decimals: 2,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        var updated = model.state.equalizer
                        updated.threshold = value
                        model.updateEqualizer(updated, commit: false)
                    },
                    onCommit: { model.commitEdit() }
                )
                .opacity(model.sliderVisible("阈值") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("阈值"))
                ValueSliderRow(
                    label: "曲线宽度",
                    value: eq.smoothingHue,
                    range: 0.05...2,
                    neutral: 1,
                    step: 0.05,
                    decimals: 2,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        var updated = model.state.equalizer
                        updated.smoothingHue = value
                        model.updateEqualizer(updated, commit: false)
                    },
                    onCommit: { model.commitEdit() }
                )
                .opacity(model.sliderVisible("曲线宽度") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("曲线宽度"))
                ValueSliderRow(
                    label: "对比",
                    value: eq.contrast,
                    range: -1...1,
                    step: 0.05,
                    decimals: 2,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        var updated = model.state.equalizer
                        updated.contrast = value
                        model.updateEqualizer(updated, commit: false)
                    },
                    onCommit: { model.commitEdit() }
                )
                .opacity(model.sliderVisible("对比") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("对比"))
                ValueSliderRow(
                    label: "色度半径",
                    value: eq.chromaSize,
                    range: 1...10,
                    neutral: 1.5,
                    step: 0.5,
                    decimals: 1,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        var updated = model.state.equalizer
                        updated.chromaSize = value
                        model.updateEqualizer(updated, commit: false)
                    },
                    onCommit: { model.commitEdit() }
                )
                .opacity(model.sliderVisible("色度半径") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("色度半径"))
                ValueSliderRow(
                    label: "修正半径",
                    value: eq.paramSize,
                    range: 1...128,
                    neutral: 1,
                    step: 1,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        var updated = model.state.equalizer
                        updated.paramSize = value
                        model.updateEqualizer(updated, commit: false)
                    },
                    onCommit: { model.commitEdit() }
                )
                .opacity(model.sliderVisible("修正半径") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("修正半径"))
                ValueSliderRow(
                    label: "节点旋转",
                    value: eq.hueShift,
                    range: -23...23,
                    step: 1,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        var updated = model.state.equalizer
                        updated.hueShift = value
                        model.updateEqualizer(updated, commit: false)
                    },
                    onCommit: { model.commitEdit() }
                )
                .opacity(model.sliderVisible("节点旋转") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("节点旋转"))
                Toggle(isOn: Binding(
                    get: { model.state.equalizer.useFilter },
                    set: { value in
                        var updated = model.state.equalizer
                        updated.useFilter = value
                        model.updateEqualizer(updated, commit: true)
                    }
                )) {
                    Text("导向滤波（更慢，但压彩噪）")
                        .font(.caption)
                }
                .padding(.horizontal, 18)
                .padding(.vertical, 6)
            }
        }
        .scrollDisabled(model.draggingSlider != nil)
    }

        private var hslPanel: some View {
        VStack(spacing: 0) {
            // 色带选择：默认 Lightroom 那种 8 个色圆点（可横滑），
            // 也可切回"彩虹色带"老式样式（选择记忆在本地）。
            HStack(spacing: 8) {
                if hslStyle == "strip" {
                    HslBandStrip(selected: model.hslBand) { kind in
                        model.hslBand = kind
                    }
                } else {
                    HslBandDots(selected: model.hslBand) { kind in
                        model.hslBand = kind
                    }
                }
                Text(model.hslBand.rawValue)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .frame(width: 24, alignment: .trailing)
                Button {
                    Haptics.tick()
                    hslStyle = hslStyle == "strip" ? "dots" : "strip"
                } label: {
                    Image(systemName: hslStyle == "strip" ? "circle.lefthalf.filled" : "circle.grid.3x3")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .opacity(model.draggingSlider == nil ? 1 : 0)
            .allowsHitTesting(model.draggingSlider == nil)
            hslSliders
            Spacer(minLength: 0)
        }
    }

    private var hslSliders: some View {
        let band = model.state.hsl[model.hslBand]
        return ScrollView {
            VStack(spacing: 0) {
                // 拖动某个滑块时其余只隐藏不位移（滑块保持原位）。
                ValueSliderRow(
                    label: "色相",
                    value: band.hue,
                    range: -100...100,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        var updated = model.state.hsl[model.hslBand]
                        updated.hue = value
                        model.updateHsl(model.hslBand, updated, commit: false)
                    },
                    onCommit: { model.commitEdit() },
                    trackColors: hslTrackColors("hue")
                )
                .opacity(model.sliderVisible("色相") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("色相"))
                ValueSliderRow(
                    label: "饱和度",
                    value: band.saturation,
                    range: -100...100,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        var updated = model.state.hsl[model.hslBand]
                        updated.saturation = value
                        model.updateHsl(model.hslBand, updated, commit: false)
                    },
                    onCommit: { model.commitEdit() },
                    trackColors: hslTrackColors("saturation")
                )
                .opacity(model.sliderVisible("饱和度") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("饱和度"))
                ValueSliderRow(
                    label: "明亮度",
                    value: band.luminance,
                    range: -100...100,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        var updated = model.state.hsl[model.hslBand]
                        updated.luminance = value
                        model.updateHsl(model.hslBand, updated, commit: false)
                    },
                    onCommit: { model.commitEdit() },
                    trackColors: hslTrackColors("luminance")
                )
                .opacity(model.sliderVisible("明亮度") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("明亮度"))
            }
        }
        // 拖动滑块中：禁止面板上下滚动（只调值）。
        .scrollDisabled(model.draggingSlider != nil)
    }

    // MARK: - 颜色分级

    private var gradingPanel: some View {
        ScrollView {
            VStack(spacing: 0) {
                // 拖动滑块时：色轮区只隐藏不位移（滑块保持原位）。
                Group {
                    subTabRow(
                        GradingZoneKind.allCases.map(\.rawValue),
                        selected: GradingZoneKind.allCases.firstIndex(of: model.gradingZone) ?? 0
                    ) { index in
                        model.gradingZone = GradingZoneKind.allCases[index]
                    }
                    // 色相 / 饱和度色轮：点或拖选色（亮度仍用下面的滑块）。
                    GradingWheel(
                        zone: model.state.grading[model.gradingZone],
                        onBegin: { model.beginEdit() },
                        onChange: { updated in
                            model.updateGrading(model.gradingZone, updated, commit: false)
                        },
                        onEnd: { model.commitEdit() }
                    )
                }
                .opacity(model.draggingSlider == nil ? 1 : 0)
                .allowsHitTesting(model.draggingSlider == nil)
                gradingZoneSliders
                ValueSliderRow(
                    label: "混合",
                    value: model.state.grading.blending,
                    range: 0...100,
                    neutral: 50,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        model.updateGradingMix(
                            blending: value,
                            balance: model.state.grading.balance,
                            commit: false
                        )
                    },
                    onCommit: { model.commitEdit() }
                )
                .opacity(model.sliderVisible("混合") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("混合"))
                ValueSliderRow(
                    label: "平衡",
                    value: model.state.grading.balance,
                    range: -100...100,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        model.updateGradingMix(
                            blending: model.state.grading.blending,
                            balance: value,
                            commit: false
                        )
                    },
                    onCommit: { model.commitEdit() }
                )
                .opacity(model.sliderVisible("平衡") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("平衡"))
            }
        }
        // 拖动滑块中：禁止面板上下滚动（只调值）。
        .scrollDisabled(model.draggingSlider != nil)
    }

    private var gradingZoneSliders: some View {
        let zone = model.state.grading[model.gradingZone]
        return VStack(spacing: 0) {
            // 拖动某个滑块时其余只隐藏不位移（滑块保持原位）。
            ValueSliderRow(
                label: "色相",
                value: zone.hue,
                range: 0...360,
                neutral: 0,
                step: 15,
                onBegin: { model.beginEdit() },
                onChange: { value in
                    var updated = model.state.grading[model.gradingZone]
                    updated.hue = value
                    model.updateGrading(model.gradingZone, updated, commit: false)
                },
                onCommit: { model.commitEdit() }
            )
            .opacity(model.sliderVisible("色相") ? 1 : 0)
            .allowsHitTesting(model.sliderVisible("色相"))
            ValueSliderRow(
                label: "饱和度",
                value: zone.saturation,
                range: 0...100,
                neutral: 0,
                onBegin: { model.beginEdit() },
                onChange: { value in
                    var updated = model.state.grading[model.gradingZone]
                    updated.saturation = value
                    model.updateGrading(model.gradingZone, updated, commit: false)
                },
                onCommit: { model.commitEdit() }
            )
            .opacity(model.sliderVisible("饱和度") ? 1 : 0)
            .allowsHitTesting(model.sliderVisible("饱和度"))
            ValueSliderRow(
                label: "明亮度",
                value: zone.luminance,
                range: -100...100,
                onBegin: { model.beginEdit() },
                onChange: { value in
                    var updated = model.state.grading[model.gradingZone]
                    updated.luminance = value
                    model.updateGrading(model.gradingZone, updated, commit: false)
                },
                onCommit: { model.commitEdit() }
            )
            .opacity(model.sliderVisible("明亮度") ? 1 : 0)
            .allowsHitTesting(model.sliderVisible("明亮度"))
        }
    }

    // MARK: - 点颜色（取色 + 范围调整，最多 4 个样本）

    /// 当前选中的样本下标（样本数变化后自动收敛到有效范围）。
    private var pointColorSelectedIndex: Int {
        min(model.pointColorIndex, max(model.state.pointColors.count - 1, 0))
    }

    private var pointColorPanel: some View {
        VStack(spacing: 0) {
            if model.state.blackWhite {
                Text("黑白模式下点颜色不参与合成：先关掉「黑白」，或改用黑白面板的色带混色。")
                    .font(.system(size: 10))
                    .foregroundStyle(Color.orange.opacity(0.9))
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 18)
                    .padding(.top, 8)
            }
            pointColorToolbar
            if model.state.pointColors.isEmpty {
                VStack(spacing: 6) {
                    Image(systemName: "eyedropper.halffull")
                        .font(.title3)
                        .foregroundStyle(.secondary)
                    Text(
                        model.pointSampling
                            ? "点一下照片上想要调整的颜色"
                            : "点「取色」后在照片上点一下要调整的颜色"
                    )
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 22)
                Spacer(minLength: 0)
            } else {
                pointColorSliders
            }
        }
    }

    private var pointColorToolbar: some View {
        VStack(spacing: 8) {
            HStack(spacing: 10) {
                Button {
                    Haptics.tick()
                    model.pointSampling.toggle()
                } label: {
                    Label(
                        model.pointSampling ? "取色中…" : "取色",
                        systemImage: "eyedropper"
                    )
                    .font(.caption)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 7)
                    .background(
                        Capsule().fill(
                            model.pointSampling
                                ? Color.accentColor.opacity(0.35)
                                : Color.white.opacity(0.08)
                        )
                    )
                }
                .buttonStyle(.plain)
                Button {
                    Haptics.tick()
                    model.addPointColorSample()
                    model.pointSampling = true
                } label: {
                    Label("新增", systemImage: "plus.circle")
                        .font(.caption)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 7)
                        .background(Capsule().fill(Color.white.opacity(0.08)))
                }
                .buttonStyle(.plain)
                .disabled(model.state.pointColors.count >= pointColorSampleLimit)
                .opacity(
                    model.state.pointColors.count >= pointColorSampleLimit ? 0.4 : 1
                )
                Spacer(minLength: 0)
                if !model.state.pointColors.isEmpty {
                    Button {
                        Haptics.light()
                        model.clearPointColors()
                    } label: {
                        Text("清空")
                            .font(.caption)
                            .foregroundStyle(Color.white.opacity(0.65))
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, 14)
            if !model.state.pointColors.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 10) {
                        ForEach(
                            Array(model.state.pointColors.enumerated()),
                            id: \.element.id
                        ) { index, sample in
                            Button {
                                Haptics.tick()
                                model.pointColorIndex = index
                                model.pointSampling = true
                            } label: {
                                Circle()
                                    .fill(
                                        Color(
                                            red: sample.swatch.r,
                                            green: sample.swatch.g,
                                            blue: sample.swatch.b
                                        )
                                    )
                                    .frame(width: 24, height: 24)
                                    .overlay(
                                        Circle().stroke(
                                            index == pointColorSelectedIndex
                                                ? Color.white
                                                : Color.white.opacity(0.25),
                                            lineWidth: index == pointColorSelectedIndex ? 2 : 1
                                        )
                                    )
                            }
                            .buttonStyle(.plain)
                            .contextMenu {
                                Button(role: .destructive) {
                                    model.removePointColor(index: index)
                                } label: {
                                    Label("删除样本", systemImage: "trash")
                                }
                            }
                        }
                    }
                    .padding(.horizontal, 14)
                }
            }
        }
        .padding(.top, 8)
        .padding(.bottom, 4)
    }

    private var pointColorSliders: some View {
        let index = pointColorSelectedIndex
        let sample = model.state.pointColors.indices.contains(index)
            ? model.state.pointColors[index]
            : PointColorSample()
        return ScrollView {
            VStack(spacing: 0) {
                pointColorSlider("色相偏移", sample.hueShift, -100...100) { $0.hueShift = $1 }
                pointColorSlider("饱和度偏移", sample.satShift, -100...100) { $0.satShift = $1 }
                pointColorSlider("明亮度偏移", sample.lumShift, -100...100) { $0.lumShift = $1 }
                pointColorSlider("变体", sample.variance, -100...100) { $0.variance = $1 }
                pointColorSlider("范围", sample.range, 0...100) { $0.range = $1 }
                pointColorSlider("色相范围", sample.hueRange, 0...100) { $0.hueRange = $1 }
                pointColorSlider("饱和度范围", sample.satRange, 0...100) { $0.satRange = $1 }
                pointColorSlider("明亮度范围", sample.lumRange, 0...100) { $0.lumRange = $1 }
            }
        }
        .scrollDisabled(model.draggingSlider != nil)
    }

    private func pointColorSlider(
        _ label: String,
        _ value: Float,
        _ range: ClosedRange<Float>,
        set: @escaping (inout PointColorSample, Float) -> Void
    ) -> some View {
        let index = pointColorSelectedIndex
        return ValueSliderRow(
            label: label,
            value: value,
            range: range,
            onBegin: { model.beginEdit() },
            onChange: { newValue in
                model.updatePointColor(index: index, commit: false) { sample in
                    set(&sample, newValue)
                }
            },
            onCommit: { model.commitEdit() }
        )
        .opacity(model.sliderVisible(label) ? 1 : 0)
        .allowsHitTesting(model.sliderVisible(label))
    }

    // MARK: - 相机校准

    private var calibrationPanel: some View {
        let calibration = model.state.calibration
        return ScrollView {
            VStack(spacing: 0) {
                calibrationSlider("红原色色相", calibration.redHue) { $0.redHue = $1 }
                calibrationSlider("红原色饱和度", calibration.redSat) { $0.redSat = $1 }
                calibrationSlider("绿原色色相", calibration.greenHue) { $0.greenHue = $1 }
                calibrationSlider("绿原色饱和度", calibration.greenSat) { $0.greenSat = $1 }
                calibrationSlider("蓝原色色相", calibration.blueHue) { $0.blueHue = $1 }
                calibrationSlider("蓝原色饱和度", calibration.blueSat) { $0.blueSat = $1 }
                ValueSliderRow(
                    label: "阴影色调",
                    value: calibration.shadowsTint,
                    range: -100...100,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        var updated = model.state.calibration
                        updated.shadowsTint = value
                        model.updateCalibration(updated, commit: false)
                    },
                    onCommit: { model.commitEdit() },
                    trackColors: [Color(red: 0.25, green: 0.75, blue: 0.35), Color(white: 0.5), Color(red: 0.85, green: 0.35, blue: 0.75)]
                )
                .opacity(model.sliderVisible("阴影色调") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("阴影色调"))
                Text("校准改变相机三原色的色相与饱和度（在场景线性空间生效），是「像相机 / 像 Lightroom」的调色起点；阴影色调做整体微调。")
                    .font(.system(size: 10))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 18)
                    .padding(.top, 6)
            }
        }
        .scrollDisabled(model.draggingSlider != nil)
    }

    private func calibrationSlider(
        _ label: String,
        _ value: Float,
        set: @escaping (inout CalibrationState, Float) -> Void
    ) -> some View {
        ValueSliderRow(
            label: label,
            value: value,
            range: -100...100,
            onBegin: { model.beginEdit() },
            onChange: { newValue in
                var updated = model.state.calibration
                set(&updated, newValue)
                model.updateCalibration(updated, commit: false)
            },
            onCommit: { model.commitEdit() }
        )
        .opacity(model.sliderVisible(label) ? 1 : 0)
        .allowsHitTesting(model.sliderVisible(label))
    }

    // MARK: - 黑白

    private var blackWhitePanel: some View {
        let tint = model.bwBand.tint
        return VStack(spacing: 0) {
            opticsToggle(
                title: "黑白",
                hint: "打开后按下面的色带权重转单色；HSL / 点颜色 / 饱和度不参与合成",
                isOn: Binding(
                    get: { model.state.blackWhite },
                    set: { enabled in
                        Haptics.tick()
                        model.applyEdit(enabled ? "黑白" : "彩色") { state in
                            state.blackWhite = enabled
                        }
                    }
                )
            )
            HStack(spacing: 8) {
                HslBandDots(selected: model.bwBand) { kind in
                    model.bwBand = kind
                }
                Text(model.bwBand.rawValue)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .frame(width: 24, alignment: .trailing)
            }
            .padding(.horizontal, 14)
            .padding(.bottom, 6)
            .opacity(model.draggingSlider == nil ? 1 : 0)
            .allowsHitTesting(model.draggingSlider == nil)
            ValueSliderRow(
                label: "明亮度",
                value: model.state.bwMix[model.bwBand],
                range: -100...100,
                onBegin: { model.beginEdit() },
                onChange: { value in
                    model.updateBwMix(model.bwBand, value, commit: false)
                },
                onCommit: { model.commitEdit() },
                trackColors: [
                    Color(white: 0.12),
                    Color(red: tint.r, green: tint.g, blue: tint.b),
                    Color(white: 1.0),
                ]
            )
            .disabled(!model.state.blackWhite)
            .opacity(
                model.sliderVisible("明亮度") ? (model.state.blackWhite ? 1 : 0.4) : 0
            )
            .allowsHitTesting(model.sliderVisible("明亮度"))
            Text("正值提亮该色相（例如蓝 +：天空更亮），负值压暗。黑白关闭时此面板不生效。")
                .font(.system(size: 10))
                .foregroundStyle(.secondary)
                .padding(.horizontal, 18)
                .padding(.top, 6)
            Spacer(minLength: 0)
        }
    }

    // MARK: - 光学（删除色差 / 启用镜头校正）

    private var opticsPanel: some View {
        ScrollView {
            VStack(spacing: 0) {
                opticsToggle(
                    title: "删除色差",
                    hint: "去掉高对比边缘的红 / 青彩边（横向色差，固定强度）",
                    isOn: Binding(
                        get: { model.state.removeChromaticAberration },
                        set: { newValue in
                            Haptics.tick()
                            model.applyEdit("删除色差") { state in
                                state.removeChromaticAberration = newValue
                            }
                        }
                    )
                )
                opticsToggle(
                    title: "启用镜头校正",
                    hint: "内置档案校正桶形畸变（广角镜头明显）",
                    isOn: Binding(
                        get: { model.state.profileCorrection },
                        set: { newValue in
                            Haptics.tick()
                            model.applyEdit("镜头校正") { state in
                                state.profileCorrection = newValue
                            }
                        }
                    )
                )
            }
            .padding(.vertical, 10)
        }
        .scrollDisabled(model.draggingSlider != nil)
    }

    private func opticsToggle(title: String, hint: String, isOn: Binding<Bool>) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Toggle(isOn: isOn) {
                Text(title)
                    .font(.footnote)
            }
            .tint(Color.accentColor)
            Text(hint)
                .font(.system(size: 10))
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 10)
    }

    // MARK: - 裁切

    private var cropPanel: some View {
        VStack(spacing: 0) {
            // 拖动「拉直」滑块时：上面两行只隐藏不位移（滑块保持原位）。
            Group {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(CropAspect.allCases) { aspect in
                            Button {
                                model.setCropAspect(aspect, contentSize: previewImageSize)
                            } label: {
                                Text(aspect.rawValue)
                                    .font(.caption2)
                                    .padding(.horizontal, 12)
                                    .padding(.vertical, 6)
                                    .background(
                                        Capsule().fill(
                                            model.state.crop.aspect == aspect
                                                ? Color.accentColor.opacity(0.35)
                                                : Color.white.opacity(0.08)
                                        )
                                    )
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                }
                // 构图网格样式：关 / 三分 / 网格 / 对角 / 黄金。
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(CropGridUi.allCases) { grid in
                            Button {
                                Haptics.tick()
                                model.applyEdit("构图网格") { state in
                                    state.crop.grid = grid
                                }
                            } label: {
                                Text(grid.rawValue)
                                    .font(.caption2)
                                    .padding(.horizontal, 10)
                                    .padding(.vertical, 4)
                                    .background(
                                        Capsule().fill(
                                            model.state.crop.grid == grid
                                                ? Color.accentColor.opacity(0.35)
                                                : Color.white.opacity(0.08)
                                        )
                                    )
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, 14)
                    .padding(.bottom, 6)
                }
            }
            .scrollDisabled(model.draggingSlider != nil)
            .opacity(model.draggingSlider == nil ? 1 : 0)
            .allowsHitTesting(model.draggingSlider == nil)
            ValueSliderRow(
                label: "拉直",
                value: model.state.crop.rotation,
                range: -45...45,
                neutral: 0,
                step: 0.5,
                decimals: 1,
                onBegin: { model.beginEdit() },
                onChange: { model.updateCropRotation($0, commit: false) },
                onCommit: { model.commitEdit() }
            )
            Group {
                HStack(spacing: 16) {
                    Button("水平翻转") {
                        model.toggleCropFlip(horizontal: true)
                    }
                    Button("垂直翻转") {
                        model.toggleCropFlip(horizontal: false)
                    }
                    Spacer()
                    Button("重置") {
                        model.resetCrop()
                    }
                    Button("完成") {
                        model.selectTool(.edit)
                    }
                    .foregroundStyle(Color.accentColor)
                }
                .font(.caption)
                .padding(.horizontal, 18)
                .padding(.vertical, 8)
            }
            .opacity(model.draggingSlider == nil ? 1 : 0)
            .allowsHitTesting(model.draggingSlider == nil)
            Spacer(minLength: 0)
        }
    }
}

#Preview {
    EditorTabView(asset: nil)
}

/// 拖动滑块时的顶部数值浮层（沉浸模式下显示）。
/// 点颜色取色层：点按画布取样（几何与蒙版 / 修图层一致：aspect-fit + 上移 + 缩放 / 平移）。
private struct PointColorTouchLayer: View {
    @ObservedObject var model: EditorModel
    let imageSize: CGSize
    let viewSize: CGSize
    let zoom: CGFloat
    let pan: CGSize

    var body: some View {
        GeometryReader { _ in
            ZStack {
                Color.clear.contentShape(Rectangle())
                Image(systemName: "eyedropper")
                    .font(.caption2)
                    .foregroundStyle(.white.opacity(0.65))
                    .padding(6)
                    .background(Circle().fill(Color.black.opacity(0.35)))
                    .position(x: viewSize.width - 26, y: 26)
                    .allowsHitTesting(false)
            }
        }
        .frame(width: viewSize.width, height: viewSize.height)
        .gesture(
            DragGesture(minimumDistance: 0)
                .onEnded { value in
                    let geometry = MaskCanvasGeometry(
                        imageSize: imageSize,
                        viewSize: viewSize,
                        zoom: zoom,
                        pan: pan
                    )
                    let normalized = geometry.normalized(value.location)
                    guard (0...1).contains(normalized.x), (0...1).contains(normalized.y) else {
                        return
                    }
                    model.samplePointColor(at: normalized)
                }
        )
    }
}

private struct SliderValueHud: View {
    let label: String
    let text: String

    var body: some View {
        VStack(spacing: 1) {
            Text(label)
                .font(.caption2)
                .foregroundStyle(.secondary)
            Text(text)
                .font(.title3.monospacedDigit())
                .fontWeight(.semibold)
                .foregroundStyle(.white)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 8)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 12))
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.white.opacity(0.12))
        )
    }
}

/// 分屏对比的右侧遮罩：只显示分割线右边的部分。
private struct CompareSplitShape: Shape {
    var fraction: CGFloat

    func path(in rect: CGRect) -> Path {
        let x = (rect.width * fraction).clamped(to: 0...rect.width)
        var path = Path()
        path.addRect(
            CGRect(x: x, y: 0, width: max(rect.width - x, 0), height: rect.height)
        )
        return path
    }
}

/// 系统分享面板（导出完成后的文件）。
private struct ShareSheet: UIViewControllerRepresentable {
    let items: [Any]

    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: items, applicationActivities: nil)
    }

    func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
}

/// 区域高清帧层：把 1:1 区域图按归一化矩形对齐贴到预览上（与预览同一套缩放 / 平移）。
private struct CurveOverlay: View {
    @ObservedObject var model: EditorModel
    let viewSize: CGSize
    /// 长按曲线编辑区看原图（true = 开始，false = 结束）；曲线本身不变化。
    let onPreviewHold: (Bool) -> Void

    var body: some View {
        let side = min(viewSize.width, viewSize.height) * 0.84
        CurveEditorCanvas(
            points: model.curvePoints(model.curveChannel),
            channel: model.curveChannel,
            parametric: model.state.curves.parametric,
            onChange: { points, commit in
                model.updateCurve(model.curveChannel, points, commit: commit)
            },
            onParametricChange: { values, commit in
                model.updateParametric(values, commit: commit)
            },
            transparent: true,
            onHoldPreview: onPreviewHold
        )
        .frame(width: side, height: side)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .allowsHitTesting(model.draggingSlider == nil)
        .opacity(model.draggingSlider == nil ? 1 : 0)
    }
}

/// 区域高清帧层：把 1:1 区域图按归一化矩形对齐贴到预览上（与预览同一套缩放 / 平移）。
private struct DetailRegionLayer: View {
    let image: UIImage
    /// 覆盖的归一化矩形（输出坐标系）。
    let rect: CGRect
    let imageSize: CGSize
    let viewSize: CGSize
    let zoom: CGFloat
    let pan: CGSize

    var body: some View {
        let fitted = fittedRect(imageSize: imageSize, in: viewSize)
        Image(uiImage: image)
            .resizable()
            .frame(width: fitted.width * rect.width, height: fitted.height * rect.height)
            .position(
                x: fitted.minX + (rect.minX + rect.width / 2) * fitted.width,
                y: fitted.minY + (rect.minY + rect.height / 2) * fitted.height
            )
            .frame(width: viewSize.width, height: viewSize.height)
            .allowsHitTesting(false)
            .scaleEffect(zoom)
            .offset(pan)
    }
}

/// HSL 色相条：彩虹渐变 + 当前色带指示，点 / 拖选择色带。
private struct HslBandDots: View {
    let selected: HslBandKind
    let onSelect: (HslBandKind) -> Void

    var body: some View {
        GeometryReader { geo in
            let count = CGFloat(HslBandKind.allCases.count)
            let width = max(geo.size.width, 1)
            HStack(spacing: 0) {
                ForEach(HslBandKind.allCases) { kind in
                    Circle()
                        .strokeBorder(tint(kind), lineWidth: 3)
                        .background(
                            Circle()
                                .fill(kind == selected ? tint(kind) : Color.clear)
                                .frame(width: 8, height: 8)
                        )
                        .frame(width: 22, height: 22)
                        .frame(maxWidth: .infinity)
                }
            }
            .frame(width: width, height: 24)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { value in
                        let index = Int((value.location.x / width) * count)
                        let clamped = min(max(index, 0), HslBandKind.allCases.count - 1)
                        let kind = HslBandKind.allCases[clamped]
                        if kind != selected {
                            onSelect(kind)
                            Haptics.tick()
                        }
                    }
            )
        }
        .frame(height: 24)
    }

    private func tint(_ kind: HslBandKind) -> Color {
        Color(red: kind.tint.r, green: kind.tint.g, blue: kind.tint.b)
    }
}

/// HSL 色相条：彩虹渐变 + 当前色带指示，点 / 拖选择色带（老式样式）。
private struct HslBandStrip: View {
    let selected: HslBandKind
    let onSelect: (HslBandKind) -> Void

    var body: some View {
        GeometryReader { geo in
            let width = max(geo.size.width, 1)
            let count = CGFloat(HslBandKind.allCases.count)
            ZStack(alignment: .leading) {
                Capsule()
                    .fill(
                        LinearGradient(
                            stops: HslBandKind.allCases.enumerated().map { index, kind in
                                Gradient.Stop(
                                    color: Color(
                                        red: kind.tint.r,
                                        green: kind.tint.g,
                                        blue: kind.tint.b
                                    ),
                                    location: (CGFloat(index) + 0.5) / count
                                )
                            },
                            startPoint: .leading,
                            endPoint: .trailing
                        )
                    )
                    .frame(height: 20)
                if let index = HslBandKind.allCases.firstIndex(of: selected) {
                    let x = width * (CGFloat(index) + 0.5) / count
                    Circle()
                        .fill(Color.black.opacity(0.35))
                        .overlay(Circle().strokeBorder(Color.white, lineWidth: 2.5))
                        .frame(width: 18, height: 18)
                        .position(x: x, y: 10)
                }
            }
            .frame(height: 20)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { value in
                        let index = Int((value.location.x / width) * count)
                        let clamped = min(max(index, 0), HslBandKind.allCases.count - 1)
                        let kind = HslBandKind.allCases[clamped]
                        if kind != selected {
                            onSelect(kind)
                            Haptics.tick()
                        }
                    }
            )
        }
        .frame(height: 20)
    }
}

/// 颜色分级色相 / 饱和度色轮（中心 = 0 饱和度，角度 = 色相）。
private struct GradingWheel: View {
    let zone: GradingZone
    let onBegin: () -> Void
    let onChange: (GradingZone) -> Void
    let onEnd: () -> Void

    @State private var dragging = false
    /// 拖动的是内圈（只调色相）还是外点（色相 + 饱和度）。
    @State private var innerMode = false
    @State private var lastTapTime = Date.distantPast

    var body: some View {
        GeometryReader { geo in
            let size = min(geo.size.width, geo.size.height)
            let radius = max(size / 2, 1)
            // 色轮坐标系与手势位置都在 ZStack 本地空间（中心 = size/2）。
            let center = CGPoint(x: size / 2, y: size / 2)
            let saturation = CGFloat(zone.saturation) / 100
            let angle = CGFloat(zone.hue) * .pi / 180
            let dot = CGPoint(
                x: center.x + cos(angle) * radius * saturation,
                y: center.y + sin(angle) * radius * saturation
            )
            // 内圈小圆：固定在 0.34R 上，只表示 / 调整色相。
            let innerRadius = radius * 0.34
            let innerDot = CGPoint(
                x: center.x + cos(angle) * innerRadius,
                y: center.y + sin(angle) * innerRadius
            )
            ZStack {
                Circle()
                    .fill(
                        AngularGradient(
                            gradient: Gradient(colors: [
                                Color(red: 1, green: 0, blue: 0),
                                Color(red: 1, green: 1, blue: 0),
                                Color(red: 0, green: 1, blue: 0),
                                Color(red: 0, green: 1, blue: 1),
                                Color(red: 0, green: 0, blue: 1),
                                Color(red: 1, green: 0, blue: 1),
                                Color(red: 1, green: 0, blue: 0),
                            ]),
                            center: .center
                        )
                    )
                Circle()
                    .fill(
                        RadialGradient(
                            gradient: Gradient(colors: [
                                Color(white: 0.1),
                                Color(white: 0.1).opacity(0),
                            ]),
                            center: .center,
                            startRadius: 0,
                            endRadius: radius
                        )
                    )
                Circle()
                    .strokeBorder(Color.white.opacity(0.15), lineWidth: 1)
                // 内圈小圆（只调色相）：外圈白色、内点浅蓝。
                Circle()
                    .fill(Color.accentColor.opacity(0.55))
                    .overlay(Circle().strokeBorder(Color.white, lineWidth: 2))
                    .frame(width: 16, height: 16)
                    .position(innerDot)
                // 外点（色相 + 饱和度）。
                Circle()
                    .fill(Color.white)
                    .overlay(Circle().stroke(Color.black.opacity(0.4), lineWidth: 1))
                    .frame(width: 14, height: 14)
                    .position(dot)
            }
            .frame(width: size, height: size)
            .contentShape(Circle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { value in
                        if !dragging {
                            dragging = true
                            // 起手靠近内圈 → 只调色相；否则色相 + 饱和度。
                            let distance = hypot(
                                value.startLocation.x - innerDot.x,
                                value.startLocation.y - innerDot.y
                            )
                            innerMode = distance < 30
                            onBegin()
                            Haptics.prepare()
                        }
                        let dx = value.location.x - center.x
                        let dy = value.location.y - center.y
                        var hue = atan2(dy, dx) * 180 / .pi
                        if hue < 0 {
                            hue += 360
                        }
                        var updated = zone
                        updated.hue = Float(hue.truncatingRemainder(dividingBy: 360))
                        if !innerMode {
                            let distance = min((dx * dx + dy * dy).squareRoot(), radius)
                            updated.saturation = Float(distance / radius * 100)
                        }
                        onChange(updated)
                    }
                    .onEnded { value in
                        guard dragging else { return }
                        dragging = false
                        // 双击色轮：色相 + 饱和度归零（亮度不动）。
                        let isTap = hypot(value.translation.width, value.translation.height) < 8
                        if isTap {
                            let now = Date()
                            if now.timeIntervalSince(lastTapTime) < 0.32 {
                                lastTapTime = .distantPast
                                Haptics.light()
                                var reset = zone
                                reset.hue = 0
                                reset.saturation = 0
                                onChange(reset)
                                onEnd()
                                return
                            }
                            lastTapTime = now
                        }
                        onEnd()
                    }
            )
        }
        .frame(height: 128)
        .padding(.horizontal, 16)
        .padding(.top, 4)
    }
}
