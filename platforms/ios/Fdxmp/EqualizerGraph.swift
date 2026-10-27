import SwiftUI

/// 颜色均衡器的曲线图（与 Windows 版一致）。
///
/// 彩色背景 + 平滑曲线 + 8 个可拖动节点 + 底部色相参考条；
/// 曲线数据来自 Rust 核心（`fdxmp_equalizer_curve`），所见即所调。
struct EqualizerGraph: View {
    /// 0 色相 / 1 饱和度 / 2 亮度。
    let channel: Int
    let state: EqualizerState
    let onBegin: () -> Void
    let onChange: (EqualizerState) -> Void
    let onCommit: () -> Void

    @State private var activeNode: Int?
    @State private var dragStart: CGPoint?
    @State private var dragMoved = false
    @State private var lastTapNode: Int?
    @State private var lastTapTime = Date.distantPast

    /// 8 个节点等距 45°（与 Rust 的 RBF 节点一致，不是 Lightroom 的 8 个色带中心）。
    private static let bandHues: [Double] = [0, 45, 90, 135, 180, 225, 270, 315]

    private var stripHeight: CGFloat { 12 }

    var body: some View {
        let lut = curveLut()
        GeometryReader { geometry in
            let size = geometry.size
            let graphHeight = max(size.height - stripHeight - 2, 60)
            Canvas { context, canvasSize in
                drawBackground(&context, size: canvasSize, graphHeight: graphHeight)
                drawGrid(&context, size: canvasSize, graphHeight: graphHeight)
                drawCurve(&context, lut: lut, width: canvasSize.width, graphHeight: graphHeight)
                drawNodes(&context, size: canvasSize, graphHeight: graphHeight)
                drawHueStrip(&context, size: canvasSize, graphHeight: graphHeight)
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { value in
                        handleDrag(value.location, width: size.width, graphHeight: graphHeight)
                    }
                    .onEnded { _ in
                        finishDrag()
                    }
            )
        }
    }

    // MARK: - 曲线数据

    private func curveLut() -> [Float] {
        let object: [String: Any] = ["equalizer": EditJSON.equalizerJson(state)]
        guard
            let data = try? JSONSerialization.data(withJSONObject: object),
            let json = String(data: data, encoding: .utf8)
        else {
            return []
        }
        return NativeCore.shared.equalizerCurve(channel: channel, paramsJson: json)
    }

    // MARK: - 数值映射

    private func value(_ index: Int) -> Float {
        switch channel {
        case 0: return state.hue[index]
        case 1: return state.saturation[index]
        default: return state.brightness[index]
        }
    }

    /// 归一化到 0..1（0 = 底、1 = 顶）。
    private func normalized(_ value: Float) -> Double {
        let raw = channel == 0
            ? (Double(value) + 180) / 360
            : Double(value) / 2
        return min(max(raw, 0), 1)
    }

    private func yPosition(_ value: Float, graphHeight: CGFloat) -> CGFloat {
        graphHeight * CGFloat(1 - normalized(value))
    }

    private func value(atY y: CGFloat, graphHeight: CGFloat) -> Float {
        let t = 1 - Double(min(max(y / graphHeight, 0), 1))
        return channel == 0 ? Float(t * 360 - 180) : Float(t * 2)
    }

    private func nodeX(_ index: Int, width: CGFloat) -> CGFloat {
        let degrees = (Self.bandHues[index] + Double(state.hueShift))
            .truncatingRemainder(dividingBy: 360)
        let wrapped = degrees < 0 ? degrees + 360 : degrees
        return width * CGFloat(wrapped / 360)
    }

    // MARK: - 绘制

    private func drawBackground(
        _ context: inout GraphicsContext,
        size: CGSize,
        graphHeight: CGFloat
    ) {
        context.fill(
            Path(CGRect(x: 0, y: 0, width: size.width, height: graphHeight)),
            with: .color(Color(white: 0.10))
        )
        // 横向彩虹（每 5° 一段）。
        let segments = 72
        for index in 0..<segments {
            let start = size.width * CGFloat(index) / CGFloat(segments)
            let width = size.width / CGFloat(segments) + 0.5
            let hue = Double(index) / Double(segments)
            let color = Color(hue: hue, saturation: 0.65, brightness: 0.62)
            context.fill(
                Path(CGRect(x: start, y: 0, width: width, height: graphHeight)),
                with: .color(color)
            )
        }
        // 纵向遮罩：色相页上下压暗，饱和度页向上提饱和，亮度页向上提亮。
        let overlay: Gradient
        switch channel {
        case 0:
            overlay = Gradient(colors: [
                Color.white.opacity(0.16),
                Color.clear,
                Color.black.opacity(0.16),
            ])
        case 1:
            overlay = Gradient(colors: [
                Color.white.opacity(0.55),
                Color.white.opacity(0.05),
            ])
        default:
            overlay = Gradient(colors: [
                Color.black.opacity(0.0),
                Color.black.opacity(0.55),
            ])
        }
        context.fill(
            Path(CGRect(x: 0, y: 0, width: size.width, height: graphHeight)),
            with: .linearGradient(overlay, startPoint: .zero, endPoint: CGPoint(x: 0, y: graphHeight))
        )
    }

    private func drawGrid(_ context: inout GraphicsContext, size: CGSize, graphHeight: CGFloat) {
        var grid = Path()
        for step in 1..<8 {
            let x = size.width * CGFloat(step) / 8
            grid.move(to: CGPoint(x: x, y: 0))
            grid.addLine(to: CGPoint(x: x, y: graphHeight))
        }
        for step in 1..<4 {
            let y = graphHeight * CGFloat(step) / 4
            grid.move(to: CGPoint(x: 0, y: y))
            grid.addLine(to: CGPoint(x: size.width, y: y))
        }
        context.stroke(grid, with: .color(.black.opacity(0.18)), lineWidth: 1)
        // 零线
        var zero = Path()
        zero.move(to: CGPoint(x: 0, y: graphHeight / 2))
        zero.addLine(to: CGPoint(x: size.width, y: graphHeight / 2))
        context.stroke(zero, with: .color(.white.opacity(0.30)), lineWidth: 1)
    }

    private func drawCurve(
        _ context: inout GraphicsContext,
        lut: [Float],
        width: CGFloat,
        graphHeight: CGFloat
    ) {
        guard lut.count == 512 else { return }
        var path = Path()
        let samples = 360
        for step in 0...samples {
            let degrees = Double(step)
            let index = Int(((degrees + 200) / 360 * 512).rounded()) % 512
            var raw = lut[index]
            if channel == 0 {
                raw = (raw * 180 / .pi)
            }
            let point = CGPoint(
                x: width * CGFloat(degrees / 360),
                y: yPosition(raw, graphHeight: graphHeight)
            )
            if step == 0 {
                path.move(to: point)
            } else {
                path.addLine(to: point)
            }
        }
        context.stroke(path, with: .color(.white.opacity(0.92)), lineWidth: 1.6)
    }

    private func drawNodes(_ context: inout GraphicsContext, size: CGSize, graphHeight: CGFloat) {
        for index in 0..<8 {
            let x = nodeX(index, width: size.width)
            let y = yPosition(value(index), graphHeight: graphHeight)
            drawNode(&context, at: CGPoint(x: x, y: y), index: index, graphHeight: graphHeight)
            // 周期闭合：贴着左/右边缘的节点在对侧再画一次（红点在两端各一半）。
            if x < 10 {
                drawNode(
                    &context,
                    at: CGPoint(x: x + size.width, y: y),
                    index: index,
                    graphHeight: graphHeight
                )
            } else if x > size.width - 10 {
                drawNode(
                    &context,
                    at: CGPoint(x: x - size.width, y: y),
                    index: index,
                    graphHeight: graphHeight
                )
            }
        }
    }

    private func drawNode(
        _ context: inout GraphicsContext,
        at position: CGPoint,
        index: Int,
        graphHeight: CGFloat
    ) {
        if activeNode == index {
            var pillar = Path()
            pillar.move(to: CGPoint(x: position.x, y: graphHeight / 2))
            pillar.addLine(to: position)
            context.stroke(pillar, with: .color(.white.opacity(0.35)), lineWidth: 4)
        }
        let dot = Path(
            ellipseIn: CGRect(
                x: position.x - 6,
                y: position.y - 6,
                width: 12,
                height: 12
            )
        )
        context.fill(dot, with: .color(Color(white: 0.12)))
        context.stroke(dot, with: .color(nodeColor(index)), lineWidth: 2.5)
        if activeNode == index {
            let ring = Path(
                ellipseIn: CGRect(
                    x: position.x - 9,
                    y: position.y - 9,
                    width: 18,
                    height: 18
                )
            )
            context.stroke(ring, with: .color(.white.opacity(0.85)), lineWidth: 1.2)
        }
    }

    private func drawHueStrip(
        _ context: inout GraphicsContext,
        size: CGSize,
        graphHeight: CGFloat
    ) {
        let top = graphHeight + 2
        let height = size.height - top
        guard height > 2 else { return }
        let segments = 72
        for index in 0..<segments {
            let x = size.width * CGFloat(index) / CGFloat(segments)
            let width = size.width / CGFloat(segments) + 0.5
            let color = Color(hue: Double(index) / Double(segments), saturation: 0.7, brightness: 0.8)
            context.fill(
                Path(CGRect(x: x, y: top, width: width, height: height)),
                with: .color(color)
            )
        }
    }

    // MARK: - 交互

    /// 节点圆环颜色 = 该节点当前的色相（等距 45° + 整体旋转）。
    private func nodeColor(_ index: Int) -> Color {
        let degrees = (Self.bandHues[index] + Double(state.hueShift))
            .truncatingRemainder(dividingBy: 360)
        let wrapped = degrees < 0 ? degrees + 360 : degrees
        return Color(hue: wrapped / 360, saturation: 0.9, brightness: 0.95)
    }

    private func handleDrag(_ location: CGPoint, width: CGFloat, graphHeight: CGFloat) {
        guard location.y <= graphHeight + 6 else { return }
        if activeNode == nil {
            var best = 0
            var bestDistance = CGFloat.greatestFiniteMagnitude
            for index in 0..<8 {
                let distance = abs(location.x - nodeX(index, width: width))
                if distance < bestDistance {
                    bestDistance = distance
                    best = index
                }
            }
            guard bestDistance < 30 else { return }
            activeNode = best
            dragStart = location
            dragMoved = false
            onBegin()
        }
        if let start = dragStart {
            let dx = location.x - start.x
            let dy = location.y - start.y
            if sqrt(dx * dx + dy * dy) > 6 {
                dragMoved = true
            }
        }
        // 点一下只选中（不跳值）；真正拖动才改值。
        guard dragMoved, let index = activeNode else { return }
        let updated = value(atY: location.y, graphHeight: graphHeight)
        var next = state
        switch channel {
        case 0:
            next.hue[index] = updated
        case 1:
            next.saturation[index] = max(updated, 0)
        default:
            next.brightness[index] = max(updated, 0)
        }
        onChange(next)
    }

    /// 松手：拖动提交一步历史；原地双击复位该节点。
    private func finishDrag() {
        defer {
            activeNode = nil
            dragStart = nil
            dragMoved = false
        }
        guard let index = activeNode else { return }
        if dragMoved {
            onCommit()
            return
        }
        let now = Date()
        if lastTapNode == index, now.timeIntervalSince(lastTapTime) < 0.4 {
            resetNode(index)
            lastTapNode = nil
        } else {
            lastTapNode = index
            lastTapTime = now
        }
    }

    /// 双击复位：色相回 0°，饱和度 / 亮度回 1.0（与 Windows / Windows 端一致）。
    private func resetNode(_ index: Int) {
        var next = state
        switch channel {
        case 0:
            next.hue[index] = 0
        case 1:
            next.saturation[index] = 1
        default:
            next.brightness[index] = 1
        }
        onChange(next)
        onCommit()
    }
}
