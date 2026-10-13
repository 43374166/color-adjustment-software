import SwiftUI

/// 修图手动工具画布：点按 / 拖动直接作用在图上（污点修复 / 液化）。
struct RetouchTouchLayer: View {
    @ObservedObject var model: EditorModel
    let imageSize: CGSize
    let viewSize: CGSize
    let zoom: CGFloat
    let pan: CGSize

    @State private var points: [CGPoint] = []
    @State private var cursor: CGPoint?

    var body: some View {
        GeometryReader { _ in
            ZStack {
                Color.clear.contentShape(Rectangle())
                if points.count > 1, model.retouchMode == .liquify {
                    Path { path in
                        path.addLines(points)
                    }
                    .stroke(Color.accentColor.opacity(0.85), lineWidth: 2)
                }
                if let cursor {
                    Circle()
                        .stroke(Color.white.opacity(0.9), lineWidth: 1.5)
                        .background(Circle().fill(Color.white.opacity(0.06)))
                        .frame(width: cursorRadius * 2, height: cursorRadius * 2)
                        .position(cursor)
                        .allowsHitTesting(false)
                }
            }
        }
        .frame(width: viewSize.width, height: viewSize.height)
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { value in
                    cursor = value.location
                    if model.retouchMode == .liquify {
                        points.append(value.location)
                    }
                }
                .onEnded { value in
                    cursor = nil
                    defer { points = [] }
                    let geometry = MaskCanvasGeometry(
                        imageSize: imageSize,
                        viewSize: viewSize,
                        zoom: zoom,
                        pan: pan
                    )
                    let longEdge = max(imageSize.width, imageSize.height)
                    let radiusPx = Float(model.brushRadius) / 100 * Float(longEdge)
                    switch model.retouchMode {
                    case .spot:
                        let normalized = geometry.normalized(value.location)
                        guard (0...1).contains(normalized.x), (0...1).contains(normalized.y) else {
                            return
                        }
                        model.applyHeal(
                            at: normalized,
                            radius: radiusPx,
                            strength: Float(model.brushStrength) / 100
                        )
                    case .liquify:
                        var path = points
                        path.append(value.location)
                        let normalized = path
                            .map { geometry.normalized($0) }
                            .map { CGPoint(x: min(max($0.x, 0), 1), y: min(max($0.y, 0), 1)) }
                        guard normalized.count >= 2 else { return }
                        model.applyLiquifyStroke(
                            points: normalized,
                            radius: Float(model.brushRadius) / 100,
                            strength: Float(model.brushStrength) / 100
                        )
                    case .beauty:
                        break
                    }
                }
        )
    }

    /// 笔刷光标半径（view 坐标：图像像素 × 显示比例 × 缩放）。
    private var cursorRadius: CGFloat {
        let fitted = fittedRect(imageSize: imageSize, in: viewSize)
        let scale = fitted.width / max(imageSize.width, 1) * zoom
        let radiusPx = CGFloat(model.brushRadius) / 100 * max(imageSize.width, imageSize.height)
        return max(6, radiusPx * scale)
    }
}

/// AI 修图全屏加载动画（识别五官 / 去瑕疵 / 磨皮 / 肤色 / 塑形）。
struct RetouchProgressOverlay: View {
    let text: String

    @State private var pulse = false

    var body: some View {
        ZStack {
            Color.black.opacity(0.45)
                .ignoresSafeArea()
            VStack(spacing: 14) {
                ZStack {
                    Circle()
                        .stroke(Color.accentColor.opacity(0.25), lineWidth: 4)
                        .frame(width: 56, height: 56)
                    Circle()
                        .trim(from: 0, to: 0.3)
                        .stroke(
                            Color.accentColor,
                            style: StrokeStyle(lineWidth: 4, lineCap: .round)
                        )
                        .frame(width: 56, height: 56)
                        .rotationEffect(.degrees(pulse ? 360 : 0))
                        .animation(
                            .linear(duration: 1.0).repeatForever(autoreverses: false),
                            value: pulse
                        )
                }
                .onAppear { pulse = true }
                Text("AI 修图")
                    .font(.subheadline.weight(.semibold))
                Text(text)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .contentTransition(.opacity)
            }
            .padding(26)
            .background(
                RoundedRectangle(cornerRadius: 18)
                    .fill(Color(white: 0.12).opacity(0.95))
            )
            .overlay(
                RoundedRectangle(cornerRadius: 18)
                    .stroke(Color.white.opacity(0.08))
            )
        }
        .allowsHitTesting(true)
    }
}

/// 修图面板（v1.5.0）：一键修图 + 分组滑杆（去瑕疵 / 磨皮 / 肤色 / 塑形）。
///
/// 修图只改像素（在核心跑），与「编辑」Tab 的调色互不影响，可随时切 Tab 交替使用。
struct RetouchPanelView: View {
    @ObservedObject var model: EditorModel

    var body: some View {
        VStack(spacing: 0) {
            modeRow
            Divider()
                .overlay(Color.white.opacity(0.08))
            switch model.retouchMode {
            case .beauty:
                beautyContent
            case .spot:
                brushContent(
                    hint: "在画面上点按或轻拖瑕疵处 → 自动用周围皮肤盖住（手动祛痘笔）",
                    undoTitle: "撤销上一个",
                    canUndo: !model.healSpots.isEmpty,
                    undo: { model.undoLastHeal() }
                )
            case .liquify:
                brushContent(
                    hint: "在画面上拖动 → 像素沿拖动方向推挤（液化）；半径 / 强度可调",
                    undoTitle: "撤销一笔",
                    canUndo: !model.liquifyStrokes.isEmpty,
                    undo: { model.undoLastLiquify() }
                )
            }
        }
    }

    // MARK: - 子模式

    private var modeRow: some View {
        Picker(
            "",
            selection: Binding(
                get: { model.retouchMode },
                set: { model.retouchMode = $0 }
            )
        ) {
            ForEach(EditorModel.RetouchModeUi.allCases) { mode in
                Text(mode.rawValue).tag(mode)
            }
        }
        .pickerStyle(.segmented)
        .padding(.horizontal, 14)
        .padding(.top, 8)
        .padding(.bottom, 4)
        .opacity(model.draggingSlider == nil ? 1 : 0)
        .allowsHitTesting(model.draggingSlider == nil)
    }

    // MARK: - 美颜（一键修图 + 预设 + 参数）

    private var beautyContent: some View {
        VStack(spacing: 0) {
            oneTapRow
            presetsBrowser
            sliders
        }
    }

    /// 预设浏览器：上部分类（热门 / 人像 / 胶片 / 质感），下面一排卡片
    /// （原图在前，后面是当前分类的预设；图片 + 名称）。
    private var presetsBrowser: some View {
        VStack(spacing: 6) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(RetouchPresetCategory.allCases) { category in
                        Button {
                            Haptics.tick()
                            model.retouchPresetCategory = category
                        } label: {
                            Text(category.rawValue)
                                .font(
                                    .caption.weight(
                                        model.retouchPresetCategory == category
                                            ? .semibold : .regular
                                    )
                                )
                                .padding(.horizontal, 10)
                                .padding(.vertical, 4)
                                .background(
                                    Capsule().fill(
                                        model.retouchPresetCategory == category
                                            ? Color.accentColor.opacity(0.24)
                                            : Color.white.opacity(0.08)
                                    )
                                )
                                .foregroundStyle(
                                    model.retouchPresetCategory == category
                                        ? Color.accentColor
                                        : Color.white.opacity(0.8)
                                )
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, 14)
            }
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 10) {
                    presetCard(RetouchPresetUi.original)
                    ForEach(RetouchPresetUi.presets(in: model.retouchPresetCategory)) { preset in
                        presetCard(preset)
                    }
                }
                .padding(.horizontal, 14)
                .padding(.bottom, 2)
            }
        }
        .onAppear {
            if model.retouchPresetThumbnails.isEmpty {
                model.refreshRetouchPresetThumbnails()
            }
        }
        .opacity(model.draggingSlider == nil ? 1 : 0)
        .allowsHitTesting(model.draggingSlider == nil)
    }

    /// 预设卡片：图片（核心实时渲染）+ 名称。
    private func presetCard(_ preset: RetouchPresetUi) -> some View {
        let selected = model.state.retouch == preset.retouch
            && (preset.id == RetouchPresetUi.original.id ? model.state.retouch.isIdentity : true)
        return Button {
            model.applyRetouchPreset(preset)
        } label: {
            VStack(spacing: 4) {
                ZStack {
                    RoundedRectangle(cornerRadius: 8)
                        .fill(Color.white.opacity(0.06))
                        .frame(width: 64, height: 64)
                    if let thumb = model.retouchPresetThumbnails[preset.id] {
                        Image(uiImage: thumb)
                            .resizable()
                            .scaledToFill()
                            .frame(width: 64, height: 64)
                            .clipShape(RoundedRectangle(cornerRadius: 8))
                    } else {
                        ProgressView()
                            .controlSize(.mini)
                    }
                }
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .stroke(
                            selected ? Color.accentColor : Color.white.opacity(0.08),
                            lineWidth: selected ? 2 : 1
                        )
                )
                Text(preset.name)
                    .font(.system(size: 10))
                    .foregroundStyle(selected ? Color.accentColor : Color.white.opacity(0.8))
            }
        }
        .buttonStyle(.plain)
    }

    // MARK: - 笔刷（污点 / 液化）

    private func brushContent(
        hint: String,
        undoTitle: String,
        canUndo: Bool,
        undo: @escaping () -> Void
    ) -> some View {
        VStack(spacing: 0) {
            Text(hint)
                .font(.caption2)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 16)
                .padding(.top, 6)
                .opacity(model.draggingSlider == nil ? 1 : 0)
            ValueSliderRow(
                label: model.retouchMode == .liquify ? "笔刷半径" : "笔刷大小",
                value: model.brushRadius,
                range: 1...20,
                neutral: 6,
                step: 1,
                decimals: 0,
                onBegin: {},
                onChange: { model.brushRadius = $0 },
                onCommit: {}
            )
            .opacity(model.sliderVisible("笔刷大小") && model.sliderVisible("笔刷半径") ? 1 : 0)
            .allowsHitTesting(model.sliderVisible("笔刷大小") && model.sliderVisible("笔刷半径"))
            ValueSliderRow(
                label: "强度",
                value: model.brushStrength,
                range: 5...100,
                neutral: 70,
                step: 5,
                decimals: 0,
                onBegin: {},
                onChange: { model.brushStrength = $0 },
                onCommit: {}
            )
            .opacity(model.sliderVisible("强度") ? 1 : 0)
            .allowsHitTesting(model.sliderVisible("强度"))
            HStack {
                Button(undoTitle, action: undo)
                    .font(.caption)
                    .foregroundStyle(canUndo ? Color.accentColor : Color.white.opacity(0.3))
                    .disabled(!canUndo)
                Spacer()
                Button("清空") {
                    Haptics.light()
                    model.clearManualTools()
                }
                .font(.caption)
                .foregroundStyle(
                    (model.healSpots.isEmpty && model.liquifyStrokes.isEmpty)
                        ? Color.white.opacity(0.3)
                        : Color.accentColor
                )
                .disabled(model.healSpots.isEmpty && model.liquifyStrokes.isEmpty)
                if let stage = model.manualWorkStage {
                    Text(stage)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 6)
            .opacity(model.draggingSlider == nil ? 1 : 0)
            .allowsHitTesting(model.draggingSlider == nil)
            Spacer(minLength: 0)
        }
    }

    // MARK: - 一键修图

    private var oneTapRow: some View {
        VStack(spacing: 6) {
            Button {
                model.oneTapRetouch()
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: model.retouchAppliedReady ? "sparkles" : "wand.and.rays")
                    Text(model.retouchAppliedReady ? "重新一键修图" : "一键修图")
                        .font(.subheadline.weight(.semibold))
                    if model.retouchAppliedReady {
                        Text("已修图")
                            .font(.caption2)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(
                                Capsule().fill(Color.green.opacity(0.25))
                            )
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 10)
                .background(
                    RoundedRectangle(cornerRadius: 12)
                        .fill(Color.accentColor.opacity(model.retouchStage == nil ? 0.22 : 0.1))
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 12)
                        .stroke(Color.accentColor.opacity(0.5))
                )
            }
            .buttonStyle(.plain)
            .disabled(model.retouchStage != nil)
            .opacity(model.retouchStage == nil ? 1 : 0.6)

            if let analysis = model.retouchAnalysis {
                HStack(spacing: 6) {
                    Image(systemName: analysis.hasFace ? "checkmark.circle" : "exclamationmark.circle")
                        .font(.caption2)
                        .foregroundStyle(analysis.hasFace ? Color.green : Color.orange)
                    Text(statusText(analysis))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                Text("自动识别五官 → 去瑕疵 / 磨皮 / 肤色 / 塑形；识别时会显示加载动画")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(.horizontal, 14)
        .padding(.top, 10)
        .padding(.bottom, 6)
        .opacity(model.draggingSlider == nil ? 1 : 0)
        .allowsHitTesting(model.draggingSlider == nil)
    }

    private func statusText(_ analysis: RetouchAnalysisUi) -> String {
        if !analysis.hasFace {
            return "未检测到人脸：磨皮 / 去瑕疵 / 肤色按全图处理"
        }
        if !analysis.hasLandmarks {
            return "已识别人脸（五官关键点不完整，已跳过塑形）"
        }
        return "已识别五官 478 点：塑形 / 磨皮 / 肤色全部可用"
    }

    // MARK: - 滑杆

    private var sliders: some View {
        ScrollView {
            LazyVStack(spacing: 0) {
                sectionTitle("皮肤")
                retouchSlider(
                    "去瑕疵",
                    value: model.state.retouch.blemish,
                    neutral: 0
                ) { $0.blemish = $1 }
                retouchSlider(
                    "磨皮 · 低频（光影）",
                    value: model.state.retouch.smoothLow,
                    neutral: 0
                ) { $0.smoothLow = $1 }
                retouchSlider(
                    "磨皮 · 中频（纹理）",
                    value: model.state.retouch.smoothMid,
                    neutral: 0
                ) { $0.smoothMid = $1 }
                retouchSlider(
                    "磨皮 · 高频（毛孔）",
                    value: model.state.retouch.smoothHigh,
                    neutral: 0
                ) { $0.smoothHigh = $1 }
                retouchSlider(
                    "统一肤色",
                    value: model.state.retouch.skinTone,
                    neutral: 0
                ) { $0.skinTone = $1 }

                sectionTitle("脸部塑形")
                retouchSlider("瘦脸", value: model.state.retouch.faceSlim, neutral: 0) {
                    $0.faceSlim = $1
                }
                retouchSlider("下巴", value: model.state.retouch.chin, neutral: 0) {
                    $0.chin = $1
                }
                retouchSlider("大眼", value: model.state.retouch.eyeEnlarge, neutral: 0) {
                    $0.eyeEnlarge = $1
                }
                retouchSlider("瘦鼻", value: model.state.retouch.noseSlim, neutral: 0) {
                    $0.noseSlim = $1
                }

                sectionTitle("身体塑形")
                retouchSlider("瘦身（腰腹）", value: model.state.retouch.bodySlim, neutral: 0) {
                    $0.bodySlim = $1
                }

                HStack {
                    Button("重置修图") {
                        Haptics.light()
                        model.resetRetouch()
                    }
                    .font(.caption)
                    .foregroundStyle(
                        model.state.retouch.isActive ? Color.accentColor : Color.white.opacity(0.3)
                    )
                    .disabled(!model.state.retouch.isActive)
                    Spacer()
                    Text("修图与调色分开：切到「编辑」Tab 继续调色")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
                .opacity(model.draggingSlider == nil ? 1 : 0)
                .allowsHitTesting(model.draggingSlider == nil)
            }
            .padding(.vertical, 4)
        }
        .scrollDisabled(model.draggingSlider != nil)
    }

    private func sectionTitle(_ text: String) -> some View {
        Text(text)
            .font(.caption2.weight(.semibold))
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .padding(.top, 8)
            .padding(.bottom, 2)
            .opacity(model.draggingSlider == nil ? 1 : 0)
    }

    private func retouchSlider(
        _ label: String,
        value: Float,
        neutral: Float,
        apply: @escaping (inout RetouchUi, Float) -> Void
    ) -> some View {
        ValueSliderRow(
            label: label,
            value: value,
            range: 0...100,
            neutral: neutral,
            step: 5,
            decimals: 0,
            onBegin: { model.beginEdit() },
            onChange: { newValue in
                model.updateRetouch { apply(&$0, newValue) }
            },
            onCommit: {
                model.commitEdit()
                model.recomputeRetouch()
            }
        )
        .opacity(model.sliderVisible(label) ? 1 : 0)
        .allowsHitTesting(model.sliderVisible(label))
    }
}
