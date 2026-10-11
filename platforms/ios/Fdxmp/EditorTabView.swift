import Photos
import SwiftUI
import UIKit

/// 编辑 Tab：顶部工具 + 大图预览 + 直方图 + 底部工具栏。
///
/// 工具：预设 / 编辑（亮度 · 颜色 · HSL · 分级 · 效果 · 细节）/ 曲线 / 裁切 / 蒙版。
/// 长按图片看原图；滑块松手记一步历史；导出按当前参数渲染并保存到相册。
struct EditorTabView: View {
    let asset: PHAsset?
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
    /// 分类导航触摸跟踪状态（轻点 = 展开 / 收起，长按 = 只看这类参数复原）。
    @State private var categoryTouchActive = false
    @State private var categoryTouchMoved = false
    @State private var categoryPreviewActive = false
    @State private var categoryHoldWork: DispatchWorkItem?
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
    /// 预览区尺寸（用于按视口取放大细节帧分辨率）。
    @State private var previewAreaSize: CGSize = .zero

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
        .onPreferenceChange(SliderDragPreferenceKey.self) { info in
            model.setDraggingSlider(info)
        }
        .onAppear(perform: loadIfNeeded)
        .onAppear {
            // 预热触觉引擎，第一次操作就有反馈。
            Haptics.prepare()
        }
        .onChange(of: asset?.localIdentifier) { _ in
            resetZoom()
            immersive = false
            model.clearDetailPreview()
            loadIfNeeded()
        }
        .onChange(of: model.tool) { _ in
            resetZoom()
            model.clearDetailPreview()
            maskController.reset()
            categoryCollapsed = false
            model.endCategoryPreview()
            compareSplit = nil
            model.clearBeforeImage()
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
                && (model.tool == .presets || model.tool == .edit)
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

    /// HSL 专注模式：进 HSL 后隐藏所有导航（含底部 tab），只留「完成」退出。
    private var hslFocused: Bool {
        model.tool == .edit && model.category == .color && model.colorTab == .hsl
    }

    /// 曲线 / 蒙版 / HSL 下隐藏顶部栏与底部工具条，把空间让给编辑区。
    private var hidesChrome: Bool {
        model.tool == .mask
            || (model.tool == .edit && model.category == .curves)
            || hslFocused
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
                Text(model.assetName)
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
            } label: {
                Image(systemName: "rectangle.split.2x1")
                    .foregroundStyle(
                        compareSplit == nil ? Color.white.opacity(0.9) : Color.accentColor
                    )
            }
            Button {
                model.resetAll()
            } label: {
                Text("重置")
                    .font(.caption)
            }
            Button {
                model.export()
            } label: {
                if let progress = model.exportProgress {
                    Text("\(Int(progress * 100))%")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(Color.accentColor)
                } else {
                    Image(systemName: "square.and.arrow.up")
                }
            }
            .disabled(model.isExporting || model.previewImage == nil)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .background(Color(white: 0.11))
    }

    // MARK: - 预览

    private var previewImageSize: CGSize {
        model.previewImage?.size ?? model.originalImage?.size ?? CGSize(width: 4, height: 3)
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

    private var overlayEnabled: Bool {
        model.tool != .crop && model.tool != .mask
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
            if let split = compareSplit, let before = model.beforeImage, let edited = shownImage {
                // 对比模式：左「调色前」/ 右「调色后」，分割线可拖。
                Image(uiImage: before)
                    .resizable()
                    .scaledToFit()
                    .frame(width: geo.size.width, height: geo.size.height)
                Image(uiImage: edited)
                    .resizable()
                    .scaledToFit()
                    .frame(width: geo.size.width, height: geo.size.height)
                    .mask(CompareSplitShape(fraction: split))
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
            } else if let image = shownImage {
                Image(uiImage: image)
                    .resizable()
                    .scaledToFit()
                    // 居中显示（和 fittedRect / 叠加层几何一致）：
                    // 竖图本来就被高度撑满，横图不会被顶到导航栏下面。
                    .frame(
                        width: geo.size.width,
                        height: geo.size.height
                    )
                    .scaleEffect(zoom)
                    .offset(pan)
            } else if model.isLoading {
                VStack(spacing: 10) {
                    ProgressView()
                        .tint(.white)
                    Text("正在加载照片")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
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
                                .frame(width: 132, height: 76)
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
                zoom = (baseZoom * value).clamped(to: 1...6)
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
                guard overlayEnabled, zoom > 1 else { return }
                pan = CGSize(
                    width: basePan.width + value.translation.width,
                    height: basePan.height + value.translation.height
                )
            }
            .onEnded { value in
                if zoom > 1 {
                    basePan = pan
                    return
                }
                // 未放大时：左右滑动切上一张 / 下一张照片。
                guard overlayEnabled, compareSplit == nil else { return }
                handleSwipeToSwitch(value.translation)
            }
        return SimultaneousGesture(magnify, drag)
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
        case .presets: return 250
        case .edit:
            if categoryCollapsed {
                return 64
            }
            if model.category == .curves {
                // 曲线面板压到最下面：普通通道只需要"标题 + 提示 + 通道 + 完成"，
                // 给一块矮面板（少挡画面）；参数通道再高一些，给滑块留窗口。
                return model.curveChannel == .parametric ? 288 : 172
            }
            return 288
        case .crop: return 210
        // 蒙版：还没建蒙版时只留新建行（矮条）；创建后调整页才从底部弹起。
        // 羽化 / 叠加色挪回长按设置后，面板只剩"调整 + 子 nav"，再压低一档。
        case .mask: return model.selectedMask == nil ? 150 : 280
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
                }
            }
            .frame(height: panelHeight)
            // 主工具条固定在最底部（Lightroom / Android 布局）；曲线 / 蒙版下隐藏。
            if !hidesChrome {
                // 沉浸拖动：工具条只隐藏不位移（滑块保持原位）。
                toolBar
                    .opacity(model.draggingSlider == nil ? 1 : 0)
                    .allowsHitTesting(model.draggingSlider == nil)
            }
        }
        .frame(maxWidth: .infinity)
        // 真透明（不是毛玻璃）：后面露一点照片，方便对比调色效果；
        // 拖动滑块时连底色一起隐掉，只留滑块和画面（Lightroom 做法）。
        .background(Color(white: 0.07).opacity(model.draggingSlider == nil ? 0.78 : 0))
    }

    private var toolBar: some View {
        HStack(spacing: 0) {
            ForEach(EditorTool.allCases) { tool in
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

    /// 参数区（滑块 / HSL / 分级 / 曲线）。
    @ViewBuilder
    private var editContent: some View {
        if model.category == .curves {
            CurvePanelView(model: model)
        } else if model.category == .color, model.colorTab == .hsl {
            hslPanel
        } else if model.category == .color, model.colorTab == .grading {
            gradingPanel
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
            HStack(spacing: 12) {
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
        VStack(spacing: 5) {
            Image(systemName: category.icon)
                .font(.system(size: 16))
            Text(category.rawValue)
                .font(.caption2)
        }
        .frame(width: 58)
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
        .simultaneousGesture(categoryTouchGesture(category))
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

    /// 分类导航的触摸判定（轻点 = 展开 / 收起；长按 0.5s = 只看这类参数复原）。
    ///
    /// 不再用「Button + 同时长按」：那样长按松手还会顺带触发一次点击（展开 / 收起）。
    /// 这里自己判定，长按松手只结束对比，不做别的；横向滑动交给外层 ScrollView。
    private func categoryTouchGesture(_ category: EditCategory) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { value in
                if !categoryTouchActive {
                    categoryTouchActive = true
                    categoryTouchMoved = false
                    categoryPreviewActive = false
                    let work = DispatchWorkItem {
                        if categoryTouchActive, !categoryTouchMoved, !categoryPreviewActive {
                            categoryPreviewActive = true
                            Haptics.medium()
                            model.beginCategoryPreview(category)
                        }
                    }
                    categoryHoldWork = work
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.5, execute: work)
                    return
                }
                if !categoryTouchMoved,
                   hypot(value.translation.width, value.translation.height) > 12 {
                    categoryTouchMoved = true
                    categoryHoldWork?.cancel()
                    categoryHoldWork = nil
                }
            }
            .onEnded { _ in
                categoryHoldWork?.cancel()
                categoryHoldWork = nil
                let wasActive = categoryTouchActive
                let moved = categoryTouchMoved
                let previewing = categoryPreviewActive
                categoryTouchActive = false
                categoryTouchMoved = false
                categoryPreviewActive = false
                guard wasActive else { return }
                if previewing {
                    model.endCategoryPreview()
                    return
                }
                guard !moved else { return }
                toggleCategory(category)
            }
    }

    private func subTabRow(
        _ labels: [String],
        selected: Int,
        onSelect: @escaping (Int) -> Void
    ) -> some View {
        HStack(spacing: 26) {
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
