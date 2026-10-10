import Photos
import SwiftUI

/// 编辑 Tab：顶部工具 + 大图预览 + 直方图 + 底部工具栏。
///
/// 工具：预设 / 编辑（亮度 · 颜色 · HSL · 分级 · 效果 · 细节）/ 曲线 / 裁切 / 蒙版。
/// 长按图片看原图；滑块松手记一步历史；导出按当前参数渲染并保存到相册。
struct EditorTabView: View {
    let asset: PHAsset?
    /// 返回回调（退出编辑会话，回到来源页面）。
    var onClose: () -> Void = {}

    @StateObject private var model = EditorModel()
    @State private var immersive = false
    @State private var zoom: CGFloat = 1
    @State private var baseZoom: CGFloat = 1
    @State private var pan: CGSize = .zero
    @State private var basePan: CGSize = .zero
    /// 长按看原图后，短时间内忽略单击（避免松手瞬间触发沉浸切换）。
    @State private var suppressTapUntil = Date.distantPast
    /// 蒙版画布交互控制器（拖动 / 涂抹 / 双指手势）。
    @State private var maskController = MaskCanvasController()

    var body: some View {
        Group {
            if asset != nil {
                editor
            } else {
                emptyState
            }
        }
        .background(Color(white: 0.08))
        .onAppear(perform: loadIfNeeded)
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
        }
        .onChange(of: zoom) { value in
            // 放大时拉取高分辨率帧（原像素细节），缩回 1x 释放。
            if value > 1.01 {
                if model.detailImage == nil {
                    model.requestDetailPreview()
                }
            } else {
                model.clearDetailPreview()
            }
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
    }

    private func loadIfNeeded() {
        guard let asset else { return }
        model.load(asset: asset)
    }

    // MARK: - 主布局

    private var editor: some View {
        VStack(spacing: 0) {
            if !immersive {
                topBar
            }
            previewArea
            if !immersive {
                panel
            }
        }
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
                model.undo()
            } label: {
                Image(systemName: "arrow.uturn.backward")
            }
            .disabled(!model.canUndo)
            Button {
                model.redo()
            } label: {
                Image(systemName: "arrow.uturn.forward")
            }
            .disabled(!model.canRedo)
            Button {
                model.resetAll()
            } label: {
                Text("重置")
                    .font(.caption)
            }
            Button {
                model.export()
            } label: {
                if model.isExporting {
                    ProgressView()
                        .controlSize(.small)
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
                } else {
                    previewBody(geo: geo)
                        .contentShape(Rectangle())
                        .gesture(previewGesture())
                        .onTapGesture(count: 2) {
                            toggleZoom()
                        }
                        .onTapGesture {
                            if Date() < suppressTapUntil { return }
                            guard overlayEnabled else { return }
                            withAnimation(.easeInOut(duration: 0.2)) {
                                immersive.toggle()
                            }
                        }
                        .simultaneousGesture(holdToPreviewGesture())
                }
            }
            .frame(width: geo.size.width, height: geo.size.height)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @ViewBuilder
    private func previewBody(geo: GeometryProxy) -> some View {
        ZStack {
            Color.black
            if let image = shownImage {
                Image(uiImage: image)
                    .resizable()
                    .scaledToFit()
                    .frame(width: geo.size.width, height: geo.size.height)
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
            if !immersive {
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

    /// 预览手势：双指缩放 + （放大后）单指平移。
    private func previewGesture() -> some Gesture {
        let magnify = MagnificationGesture()
            .onChanged { value in
                guard overlayEnabled else { return }
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
                guard overlayEnabled, zoom > 1 else { return }
                pan = CGSize(
                    width: basePan.width + value.translation.width,
                    height: basePan.height + value.translation.height
                )
            }
            .onEnded { _ in
                if zoom > 1 {
                    basePan = pan
                }
            }
        return SimultaneousGesture(magnify, drag)
    }

    /// 长按 1 秒看原图：长按识别后继续跟踪手指，松手恢复调色效果。
    ///
    /// 用"长按 → 串接拖动"的串行手势（而不是 onLongPressGesture 的 pressing 回调），
    /// 能稳定拿到"按下 1s"与"松手"两个时刻，不会被缩放/拖动手势抢占。
    private func holdToPreviewGesture() -> some Gesture {
        LongPressGesture(minimumDuration: 1.0, maximumDistance: 30)
            .sequenced(before: DragGesture(minimumDistance: 0))
            .onChanged { value in
                guard overlayEnabled else { return }
                if case .second(true, _) = value {
                    if !model.showOriginal {
                        model.showOriginal = true
                    }
                    suppressTapUntil = Date().addingTimeInterval(0.5)
                }
            }
            .onEnded { _ in
                model.showOriginal = false
            }
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
        case .presets: return 210
        case .edit: return model.category == .curves ? 360 : 300
        case .crop: return 210
        case .mask: return 380
        }
    }

    private var panel: some View {
        VStack(spacing: 0) {
            Divider()
                .overlay(Color.white.opacity(0.08))
            Group {
                switch model.tool {
                case .presets:
                    presetPanel
                case .crop:
                    cropPanel
                case .edit:
                    editPanel
                case .mask:
                    MaskPanelView(model: model)
                }
            }
            .frame(height: panelHeight)
            // 主工具条固定在最底部（Lightroom / Android 布局）。
            toolBar
        }
        .background(Color(white: 0.11))
    }

    private var toolBar: some View {
        HStack(spacing: 0) {
            ForEach(EditorTool.allCases) { tool in
                Button {
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

    // MARK: - 预设

    private var presetPanel: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 10) {
                ForEach(EditorSpec.presets) { preset in
                    Button {
                        model.applyPreset(preset)
                    } label: {
                        presetCard(preset)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)
        }
    }

    private func presetCard(_ preset: BuiltinPreset) -> some View {
        let selected = model.presetName == preset.name
        return VStack(alignment: .leading, spacing: 4) {
            Text(preset.name)
                .font(.subheadline.weight(.semibold))
            Text(preset.description)
                .font(.caption2)
                .foregroundStyle(.secondary)
                .lineLimit(2)
                .multilineTextAlignment(.leading)
        }
        .frame(width: 112, height: 74, alignment: .topLeading)
        .padding(10)
        .background(
            RoundedRectangle(cornerRadius: 10)
                .fill(selected ? Color.accentColor.opacity(0.25) : Color.white.opacity(0.06))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 10)
                .stroke(selected ? Color.accentColor : Color.white.opacity(0.1))
        )
    }

    // MARK: - 编辑面板（亮度 / 颜色 / 效果 / 细节）

    private var editPanel: some View {
        VStack(spacing: 0) {
            categoryRow
            if model.category == .curves {
                CurvePanelView(model: model)
            } else if model.category == .color {
                subTabRow(
                    ColorTab.allCases.map(\.rawValue),
                    selected: ColorTab.allCases.firstIndex(of: model.colorTab) ?? 0
                ) { index in
                    model.colorTab = ColorTab.allCases[index]
                }
                switch model.colorTab {
                case .base:
                    sliderList
                case .hsl:
                    hslPanel
                case .grading:
                    gradingPanel
                }
            } else {
                if model.category == .effects {
                    subTabRow(
                        EffectTab.allCases.map(\.rawValue),
                        selected: EffectTab.allCases.firstIndex(of: model.effectTab) ?? 0
                    ) { index in
                        model.effectTab = EffectTab.allCases[index]
                    }
                }
                if model.category == .detail {
                    subTabRow(
                        DetailTab.allCases.map(\.rawValue),
                        selected: DetailTab.allCases.firstIndex(of: model.detailTab) ?? 0
                    ) { index in
                        model.detailTab = DetailTab.allCases[index]
                    }
                }
                sliderList
            }
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
                }
            }
            .padding(.vertical, 8)
        }
    }

    private var categoryRow: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 12) {
                ForEach(EditCategory.allCases) { category in
                    Button {
                        model.category = category
                    } label: {
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
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.top, 8)
    }

    private func subTabRow(
        _ labels: [String],
        selected: Int,
        onSelect: @escaping (Int) -> Void
    ) -> some View {
        HStack(spacing: 18) {
            ForEach(Array(labels.enumerated()), id: \.offset) { index, label in
                Button {
                    onSelect(index)
                } label: {
                    VStack(spacing: 3) {
                        Text(label)
                            .font(.caption2)
                            .foregroundStyle(
                                selected == index ? Color.white : Color.white.opacity(0.5)
                            )
                        Rectangle()
                            .fill(selected == index ? Color.accentColor : Color.clear)
                            .frame(width: 20, height: 2)
                    }
                }
                .buttonStyle(.plain)
            }
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.top, 6)
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
            onCommit: { model.commitEdit() }
        )
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
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(HslBandKind.allCases) { kind in
                        Button {
                            model.hslBand = kind
                        } label: {
                            HStack(spacing: 5) {
                                Circle()
                                    .fill(
                                        Color(
                                            red: kind.tint.r,
                                            green: kind.tint.g,
                                            blue: kind.tint.b
                                        )
                                    )
                                    .frame(width: 10, height: 10)
                                Text(kind.rawValue)
                                    .font(.caption2)
                            }
                            .padding(.horizontal, 10)
                            .padding(.vertical, 5)
                            .background(
                                Capsule().fill(
                                    model.hslBand == kind
                                        ? Color.white.opacity(0.18)
                                        : Color.white.opacity(0.06)
                                )
                            )
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
            }
            hslSliders
            Spacer(minLength: 0)
        }
    }

    private var hslSliders: some View {
        let band = model.state.hsl[model.hslBand]
        return VStack(spacing: 0) {
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
                onCommit: { model.commitEdit() }
            )
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
                onCommit: { model.commitEdit() }
            )
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
                onCommit: { model.commitEdit() }
            )
        }
    }

    // MARK: - 颜色分级

    private var gradingPanel: some View {
        VStack(spacing: 0) {
            subTabRow(
                GradingZoneKind.allCases.map(\.rawValue),
                selected: GradingZoneKind.allCases.firstIndex(of: model.gradingZone) ?? 0
            ) { index in
                model.gradingZone = GradingZoneKind.allCases[index]
            }
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
            Spacer(minLength: 0)
        }
    }

    private var gradingZoneSliders: some View {
        let zone = model.state.grading[model.gradingZone]
        return VStack(spacing: 0) {
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
        }
    }

    // MARK: - 裁切

    private var cropPanel: some View {
        VStack(spacing: 0) {
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
            Spacer(minLength: 0)
        }
    }
}

#Preview {
    EditorTabView(asset: nil)
}
