import Foundation
import SwiftUI

/// 曲线工具面板：曲线画布 + 通道选择 + 参数曲线滑块。
struct CurvePanelView: View {
    @ObservedObject var model: EditorModel

    var body: some View {
        VStack(spacing: 0) {
            // 拖动参数滑块时：标题 / 提示只隐藏不位移（滑块保持原位）。
            Group {
                header
                Spacer(minLength: 0)
                Text("在画面上直接调曲线：点按 / 拖动加点，双击控制点删除，长按看原图")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .padding(.bottom, 6)
            }
            .opacity(model.draggingSlider == nil ? 1 : 0)
            .allowsHitTesting(model.draggingSlider == nil)
            if model.curveChannel == .parametric {
                // 参数曲线滑块（拖动中只留正在拖的那一个）。
                parametricSliders
            }
            Group {
                channelRow
            }
            .opacity(model.draggingSlider == nil ? 1 : 0)
            .allowsHitTesting(model.draggingSlider == nil)
        }
    }

    private var header: some View {
        HStack {
            Text("曲线 · \(model.curveChannel.rawValue)")
                .font(.subheadline.weight(.semibold))
            Spacer()
            Button("重置本通道") {
                model.resetCurve(model.curveChannel)
            }
            .font(.caption)
            .disabled(model.curveChannel == .parametric)
        }
        .padding(.horizontal, 16)
        .padding(.top, 8)
    }

    private var parametricSliders: some View {
        let values = model.state.curves.parametric
        return ScrollView {
            VStack(spacing: 0) {
                // 拖动某个滑块时其余只隐藏不位移（滑块保持原位）。
                ValueSliderRow(
                    label: "高光",
                    value: values.highlights,
                    range: -100...100,
                    decimals: 0,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        var updated = model.state.curves.parametric
                        updated.highlights = value
                        model.updateParametric(updated, commit: false)
                    },
                    onCommit: { model.commitEdit() }
                )
                .opacity(model.sliderVisible("高光") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("高光"))
                ValueSliderRow(
                    label: "中间调",
                    value: values.midtones,
                    range: -100...100,
                    decimals: 0,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        var updated = model.state.curves.parametric
                        updated.midtones = value
                        model.updateParametric(updated, commit: false)
                    },
                    onCommit: { model.commitEdit() }
                )
                .opacity(model.sliderVisible("中间调") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("中间调"))
                ValueSliderRow(
                    label: "阴影",
                    value: values.shadows,
                    range: -100...100,
                    decimals: 0,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        var updated = model.state.curves.parametric
                        updated.shadows = value
                        model.updateParametric(updated, commit: false)
                    },
                    onCommit: { model.commitEdit() }
                )
                .opacity(model.sliderVisible("阴影") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("阴影"))
                ValueSliderRow(
                    label: "高光分界",
                    value: values.highlightsSplit,
                    range: 0.5...1,
                    neutral: 0.75,
                    step: 0.05,
                    decimals: 2,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        var updated = model.state.curves.parametric
                        updated.highlightsSplit = value
                        model.updateParametric(updated, commit: false)
                    },
                    onCommit: { model.commitEdit() }
                )
                .opacity(model.sliderVisible("高光分界") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("高光分界"))
                ValueSliderRow(
                    label: "中间调分界",
                    value: values.midtonesSplit,
                    range: 0...0.5,
                    neutral: 0.25,
                    step: 0.05,
                    decimals: 2,
                    onBegin: { model.beginEdit() },
                    onChange: { value in
                        var updated = model.state.curves.parametric
                        updated.midtonesSplit = value
                        model.updateParametric(updated, commit: false)
                    },
                    onCommit: { model.commitEdit() }
                )
                .opacity(model.sliderVisible("中间调分界") ? 1 : 0)
                .allowsHitTesting(model.sliderVisible("中间调分界"))
            }
        }
        // 拖动滑块中：禁止面板上下滚动（只调值）。
        .scrollDisabled(model.draggingSlider != nil)
        // 参数通道下给滑块一个最小窗口，避免被 Spacer 挤没。
        .frame(minHeight: 120)
    }

    private var channelRow: some View {
        HStack(spacing: 10) {
            ForEach(CurveChannel.allCases) { channel in
                Button {
                    model.curveChannel = channel
                } label: {
                    channelLabel(channel)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 6)
                        .background(
                            RoundedRectangle(cornerRadius: 8)
                                .fill(
                                    model.curveChannel == channel
                                        ? Color.white.opacity(0.14)
                                        : Color.clear
                                )
                        )
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
    }

    @ViewBuilder
    private func channelLabel(_ channel: CurveChannel) -> some View {
        if channel == .parametric {
            Image(systemName: "point.3.connected.trianglepath.dotted")
                .font(.caption)
                .foregroundStyle(.white)
        } else {
            HStack(spacing: 5) {
                Circle()
                    .strokeBorder(channelColor(channel), lineWidth: 2.5)
                    .background(Circle().fill(channel == .rgb ? Color.white : Color.clear))
                    .frame(width: 16, height: 16)
                Text(channel.rawValue)
                    .font(.caption2)
                    .foregroundStyle(.white.opacity(0.85))
            }
        }
    }
}

/// 曲线通道颜色。
func channelColor(_ channel: CurveChannel) -> Color {
    switch channel {
    case .rgb: return Color.white
    case .red: return Color(red: 1.0, green: 0.35, blue: 0.35)
    case .green: return Color(red: 0.35, green: 0.9, blue: 0.45)
    case .blue: return Color(red: 0.4, green: 0.6, blue: 1.0)
    case .parametric: return Color(red: 1.0, green: 0.8, blue: 0.35)
    }
}

/// 曲线画布：网格 + 曲线 + 控制点。
///
/// 触摸：点按 / 拖动在空白处加点并调整；按住 0.5s 不动 = 看原图（曲线不动），
/// 双击中间控制点删除。
struct CurveEditorCanvas: View {
    let points: [CurvePoint]
    let channel: CurveChannel
    let parametric: ParametricValues
    let onChange: ([CurvePoint], Bool) -> Void
    let onParametricChange: (ParametricValues, Bool) -> Void
    /// 透明模式（叠在图片上直接调，无背景板）。
    var transparent = false
    /// 长按对比预览（true = 看原图，false = 松手恢复）；长按期间曲线不变化。
    var onHoldPreview: ((Bool) -> Void)? = nil

    @State private var draggingIndex = -1
    @State private var lastTapTime = Date.distantPast
    /// 本次触摸是否还在跟踪 / 是否已越过拖动阈值（区分轻点、拖动、长按）。
    @State private var touchActive = false
    @State private var touchMoved = false
    /// 长按对比中：曲线保持原样。
    @State private var holdingPreview = false
    @State private var holdWork: DispatchWorkItem?

    private var isParametric: Bool { channel == .parametric }

    var body: some View {
        GeometryReader { geo in
            let size = geo.size
            Canvas { context, canvasSize in
                drawGrid(&context, size: canvasSize)
                drawCurve(&context, size: canvasSize)
                if !isParametric {
                    drawPoints(&context, size: canvasSize)
                }
            }
            .background(Color.black.opacity(transparent ? 0.0 : 0.35))
            .clipShape(RoundedRectangle(cornerRadius: transparent ? 0 : 10))
            .contentShape(Rectangle())
            .gesture(curveGesture(size: size))
        }
    }

    private func curveGesture(size: CGSize) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { gesture in
                guard !isParametric else { return }
                if !touchActive {
                    touchActive = true
                    touchMoved = false
                    // 按住 0.5s 不动 = 看原图对比：曲线不变化，松手恢复。
                    let work = DispatchWorkItem {
                        guard touchActive, !touchMoved else { return }
                        holdingPreview = true
                        Haptics.medium()
                        onHoldPreview?(true)
                    }
                    holdWork = work
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.5, execute: work)
                }
                // 移动超过 8pt 才算拖动（轻点 / 长按都不移动曲线）。
                if !touchMoved,
                   hypot(gesture.translation.width, gesture.translation.height) > 8 {
                    touchMoved = true
                    holdWork?.cancel()
                    holdWork = nil
                }
                if holdingPreview || !touchMoved { return }
                if draggingIndex < 0 {
                    // 开始拖动：点到控制点就拖它，点到空白处就在起点生成一个点再拖。
                    let hit = nearestIndex(to: gesture.startLocation, in: size)
                    if hit >= 0 {
                        draggingIndex = hit
                    } else {
                        Haptics.light()
                        let (updated, index) = Self.inserting(
                            makePoint(at: gesture.startLocation, size: size),
                            into: points
                        )
                        draggingIndex = index
                        onChange(updated, false)
                        Haptics.prepare()
                        return
                    }
                    Haptics.prepare()
                }
                let index = draggingIndex
                guard index >= 0, index < points.count else { return }
                var updated = points
                let rawX = Float((gesture.location.x / size.width).clamped(to: 0...1))
                let rawY = Float((1 - gesture.location.y / size.height).clamped(to: 0...1))
                let left = index > 0 ? points[index - 1].x + 0.015 : 0
                let right = index < points.count - 1 ? points[index + 1].x - 0.015 : 1
                let x: Float
                if index == 0 {
                    x = 0
                } else if index == points.count - 1 {
                    x = 1
                } else {
                    x = rawX.clamped(to: min(left, right)...max(left, right))
                }
                updated[index] = CurvePoint(x: x, y: rawY)
                onChange(updated, false)
            }
            .onEnded { gesture in
                guard !isParametric else { return }
                holdWork?.cancel()
                holdWork = nil
                let wasActive = touchActive
                let wasHolding = holdingPreview
                touchActive = false
                touchMoved = false
                holdingPreview = false
                guard wasActive else { return }
                if wasHolding {
                    // 长按对比结束：曲线保持原样（本次触摸没有改过曲线）。
                    onHoldPreview?(false)
                    draggingIndex = -1
                    return
                }
                let isTap = hypot(gesture.translation.width, gesture.translation.height) < 8
                if isTap {
                    let hit = nearestIndex(to: gesture.startLocation, in: size)
                    if hit < 0 {
                        // 轻点空白处：在该位置加一个控制点。
                        Haptics.light()
                        let (updated, _) = Self.inserting(
                            makePoint(at: gesture.startLocation, size: size),
                            into: points
                        )
                        onChange(updated, true)
                        draggingIndex = -1
                        return
                    }
                    // 轻点在控制点上：双击（300ms 内两次）= 删除中间的控制点。
                    if hit > 0, hit < points.count - 1 {
                        let now = Date()
                        if now.timeIntervalSince(lastTapTime) < 0.32 {
                            lastTapTime = .distantPast
                            Haptics.light()
                            var updated = points
                            updated.remove(at: hit)
                            onChange(updated, true)
                            draggingIndex = -1
                            return
                        }
                        lastTapTime = now
                    }
                }
                onChange(points, true)
                draggingIndex = -1
            }
    }

    /// 触摸位置 → 新控制点（x 收在 0.02…0.98，避免挤掉端点）。
    private func makePoint(at location: CGPoint, size: CGSize) -> CurvePoint {
        CurvePoint(
            x: Float((location.x / max(size.width, 1)).clamped(to: 0.02...0.98)),
            y: Float((1 - location.y / max(size.height, 1)).clamped(to: 0...1))
        )
    }

    /// 把新控制点插进按 x 排序的点列，并返回它在新数组里的下标。
    private static func inserting(
        _ point: CurvePoint,
        into points: [CurvePoint]
    ) -> ([CurvePoint], Int) {
        let updated = (points + [point]).sorted { $0.x < $1.x }
        let index = updated.firstIndex { $0.x == point.x && $0.y == point.y } ?? 0
        return (updated, index)
    }

    private func nearestIndex(to location: CGPoint, in size: CGSize) -> Int {
        var best = -1
        var bestDistance: CGFloat = 30
        for (index, point) in points.enumerated() {
            let position = CGPoint(
                x: CGFloat(point.x) * size.width,
                y: (1 - CGFloat(point.y)) * size.height
            )
            let distance = hypot(position.x - location.x, position.y - location.y)
            if distance < bestDistance {
                bestDistance = distance
                best = index
            }
        }
        return best
    }

    private func drawGrid(_ context: inout GraphicsContext, size: CGSize) {
        var grid = Path()
        for step in 1..<4 {
            let ratio = CGFloat(step) / 4
            grid.move(to: CGPoint(x: size.width * ratio, y: 0))
            grid.addLine(to: CGPoint(x: size.width * ratio, y: size.height))
            grid.move(to: CGPoint(x: 0, y: size.height * ratio))
            grid.addLine(to: CGPoint(x: size.width, y: size.height * ratio))
        }
        context.stroke(grid, with: .color(.white.opacity(0.12)), lineWidth: 1)

        var diagonal = Path()
        diagonal.move(to: CGPoint(x: 0, y: size.height))
        diagonal.addLine(to: CGPoint(x: size.width, y: 0))
        context.stroke(diagonal, with: .color(.white.opacity(0.08)), lineWidth: 1)
    }

    private func drawCurve(_ context: inout GraphicsContext, size: CGSize) {
        let samples = 128
        var path = Path()
        for index in 0...samples {
            let x = Float(index) / Float(samples)
            let y = isParametric
                ? evaluateParametric(parametric, x: x)
                : evaluateCurve(points, x: x)
            let position = CGPoint(
                x: CGFloat(x) * size.width,
                y: (1 - CGFloat(y)) * size.height
            )
            if index == 0 {
                path.move(to: position)
            } else {
                path.addLine(to: position)
            }
        }
        context.stroke(
            path,
            with: .color(channelColor(channel)),
            style: StrokeStyle(lineWidth: 3, lineCap: .round)
        )
    }

    private func drawPoints(_ context: inout GraphicsContext, size: CGSize) {
        for point in points {
            let position = CGPoint(
                x: CGFloat(point.x) * size.width,
                y: (1 - CGFloat(point.y)) * size.height
            )
            let outer = CGRect(
                x: position.x - 9, y: position.y - 9, width: 18, height: 18
            )
            context.fill(Path(ellipseIn: outer), with: .color(.black.opacity(0.6)))
            let inner = outer.insetBy(dx: 3, dy: 3)
            context.fill(Path(ellipseIn: inner), with: .color(.white))
        }
    }
}
