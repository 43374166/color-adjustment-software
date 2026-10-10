import SwiftUI
import UIKit

// MARK: - 触摸事件

/// 蒙版画布触摸事件（view 空间坐标）。
enum MaskTouchEvent {
    /// 单指按下。
    case singleBegan(CGPoint)
    /// 单指移动。
    case singleMoved(CGPoint)
    /// 单指抬起。
    case singleEnded
    /// 单指操作被第二根手指打断（拖动 / 涂抹中途）。
    case singleCancelled
    /// 双指平移（相对上一次的增量）。
    case twoFingerPan(CGSize)
    /// 双指缩放（相对上一次的倍率增量）。
    case twoFingerPinch(CGFloat)
    /// 双指旋转（相对上一次的弧度增量 + 旋转中心，view 坐标）。
    case twoFingerRotate(CGFloat, CGPoint)
    /// 双指手势结束（抬起一根手指后进入锁定，直到全部抬起）。
    case twoFingerEnded
}

// MARK: - UIKit 触摸层

/// 蒙版画布触摸层：单指操作蒙版（涂抹 / 拖动控件），双指缩放平移画布。
///
/// SwiftUI 手势拿不到"几根手指"，这里用 UIKit 原始触摸事件自己分发：
/// - 单指：画笔涂抹 / 控件拖动；
/// - 第二根手指落下：取消单指操作，切换为双指（平移 / 缩放 / 旋转蒙版）；
/// - 抬起一根手指：进入锁定，直到全部抬起（避免残留手指误画）。
struct MaskTouchLayer: UIViewRepresentable {
    let onEvent: (MaskTouchEvent) -> Void

    func makeUIView(context: Context) -> MaskTouchUIView {
        let view = MaskTouchUIView()
        view.onEvent = onEvent
        return view
    }

    func updateUIView(_ uiView: MaskTouchUIView, context: Context) {
        uiView.onEvent = onEvent
    }
}

/// 跟踪多指触摸并输出事件。
final class MaskTouchUIView: UIView {
    var onEvent: ((MaskTouchEvent) -> Void)?

    private enum Mode {
        case idle
        case single
        case multi
        /// 双指后抬起一根手指：忽略残留手指，直到全部抬起。
        case locked
    }

    private var mode: Mode = .idle
    private var positions: [UITouch: CGPoint] = [:]
    private var lastCentroid: CGPoint = .zero
    private var lastDistance: CGFloat = 0
    private var lastAngle: CGFloat = 0

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .clear
        isOpaque = false
        isMultipleTouchEnabled = true
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) 未实现")
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            positions[touch] = touch.location(in: self)
        }
        switch mode {
        case .idle, .locked:
            if positions.count == 1, let point = positions.values.first {
                mode = .single
                onEvent?(.singleBegan(point))
            } else if positions.count >= 2 {
                mode = .multi
                resetMultiBaseline()
            }
        case .single:
            if positions.count >= 2 {
                mode = .multi
                onEvent?(.singleCancelled)
                resetMultiBaseline()
            }
        case .multi:
            resetMultiBaseline()
        }
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            positions[touch] = touch.location(in: self)
        }
        switch mode {
        case .single:
            if positions.count == 1, let point = positions.values.first {
                onEvent?(.singleMoved(point))
            }
        case .multi:
            emitMultiDeltas()
        case .idle, .locked:
            break
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        finish(touches)
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        finish(touches)
    }

    private func finish(_ touches: Set<UITouch>) {
        for touch in touches {
            positions.removeValue(forKey: touch)
        }
        switch mode {
        case .single:
            onEvent?(.singleEnded)
            if positions.isEmpty {
                mode = .idle
            }
        case .multi:
            if positions.count < 2 {
                onEvent?(.twoFingerEnded)
                mode = positions.isEmpty ? .idle : .locked
            }
        case .idle, .locked:
            if positions.isEmpty {
                mode = .idle
            }
        }
    }

    private func resetMultiBaseline() {
        let points = Array(positions.values)
        guard points.count >= 2 else { return }
        lastCentroid = CGPoint(
            x: (points[0].x + points[1].x) / 2,
            y: (points[0].y + points[1].y) / 2
        )
        lastDistance = hypot(points[1].x - points[0].x, points[1].y - points[0].y)
        lastAngle = atan2(points[1].y - points[0].y, points[1].x - points[0].x)
    }

    private func emitMultiDeltas() {
        let points = Array(positions.values)
        guard points.count >= 2 else { return }
        let centroid = CGPoint(
            x: points.reduce(0) { $0 + $1.x } / CGFloat(points.count),
            y: points.reduce(0) { $0 + $1.y } / CGFloat(points.count)
        )
        let distance = hypot(points[1].x - points[0].x, points[1].y - points[0].y)
        let angle = atan2(points[1].y - points[0].y, points[1].x - points[0].x)

        let delta = CGSize(width: centroid.x - lastCentroid.x, height: centroid.y - lastCentroid.y)
        if abs(delta.width) > 0.01 || abs(delta.height) > 0.01 {
            onEvent?(.twoFingerPan(delta))
        }
        if lastDistance > 1, distance > 1 {
            let ratio = distance / lastDistance
            if abs(ratio - 1) > 0.002 {
                onEvent?(.twoFingerPinch(ratio))
            }
        }
        var deltaAngle = angle - lastAngle
        while deltaAngle > .pi {
            deltaAngle -= 2 * .pi
        }
        while deltaAngle < -.pi {
            deltaAngle += 2 * .pi
        }
        // 字典遍历顺序翻转会造成 ≈π 的假跳变，直接忽略。
        if abs(deltaAngle) > 0.001, abs(deltaAngle) < 0.7 {
            onEvent?(.twoFingerRotate(deltaAngle, centroid))
        }
        lastCentroid = centroid
        lastDistance = distance
        lastAngle = angle
    }
}

// MARK: - 画布几何

/// 蒙版画布的显示几何：预览图在视图里的矩形 + 画布缩放 / 平移。
struct MaskCanvasGeometry {
    let imageSize: CGSize
    let viewSize: CGSize
    let zoom: CGFloat
    let pan: CGSize

    /// 预览图（aspect-fit）内容坐标矩形。
    var fitted: CGRect {
        fittedRect(imageSize: imageSize, in: viewSize)
    }

    /// view 坐标 → 内容坐标（去掉画布缩放 / 平移）。
    func contentPoint(_ point: CGPoint) -> CGPoint {
        let center = CGPoint(x: viewSize.width / 2, y: viewSize.height / 2)
        let scale = max(zoom, 0.0001)
        return CGPoint(
            x: center.x + (point.x - center.x - pan.width) / scale,
            y: center.y + (point.y - center.y - pan.height) / scale
        )
    }

    /// view 坐标 → 归一化图像坐标（未钳制）。
    func normalized(_ point: CGPoint) -> CGPoint {
        normalized(fromContent: contentPoint(point))
    }

    /// 内容坐标 → 归一化图像坐标。
    func normalized(fromContent point: CGPoint) -> CGPoint {
        guard fitted.width > 0, fitted.height > 0 else { return .zero }
        return CGPoint(
            x: (point.x - fitted.minX) / fitted.width,
            y: (point.y - fitted.minY) / fitted.height
        )
    }

    /// 归一化图像坐标 → 内容坐标。
    func contentPoint(normalized point: CGPoint) -> CGPoint {
        CGPoint(
            x: fitted.minX + point.x * fitted.width,
            y: fitted.minY + point.y * fitted.height
        )
    }
}

/// 归一化图像坐标 → 内容坐标（绘制用）。
func canvasPoint(_ normalized: CGPoint, fitted: CGRect) -> CGPoint {
    CGPoint(
        x: fitted.minX + normalized.x * fitted.width,
        y: fitted.minY + normalized.y * fitted.height
    )
}

/// 归一化方向 → 内容坐标方向（保留长度比例，用于垂线 / 手柄布局）。
func canvasVector(_ vector: CGVector, fitted: CGRect) -> CGVector {
    CGVector(dx: vector.dx * fitted.width, dy: vector.dy * fitted.height)
}

// MARK: - 命中测试

/// 蒙版画布控件拖动模式。
enum MaskDragMode: Equatable {
    case none
    /// 平移中心线。
    case linearMove
    /// 拖动端点旋转。
    case linearRotate
    /// 拖动过渡 / 羽化手柄改变带宽度。
    case linearFeather
    /// 移动椭圆中心。
    case radialMove
    /// 旋转椭圆。
    case radialRotate
    /// 缩放半径（0 = 半径 X，1 = 半径 Y）。
    case radialRadius(Int)
    /// 调整内圈（羽化）（0 = X 向，1 = Y 向）。
    case radialFeather(Int)
}

/// 蒙版控件命中测试与几何工具。
enum MaskHitTester {
    /// 手柄命中半径（view 点）。
    static let gripRadius: CGFloat = 32
    /// 线条命中距离（view 点）。
    static let lineDistance: CGFloat = 30
    /// 径向旋转手柄外置距离（view 点）。
    static let rotateOffset: CGFloat = 46
    /// 中心线两端点距中心的距离（内容点，比例于短边）。
    static let linearPinHalfSpanRatio: CGFloat = 0.40
    /// 椭圆四向手柄角度。
    static let cardinalAngles: [CGFloat] = [0, .pi / 2, .pi, -.pi / 2]

    /// 椭圆参数点（归一化坐标）：中心 + R(rotation)·(rx·s·cos t, ry·s·sin t)。
    static func radialPoint(_ mask: MaskUi, angle: CGFloat, scale: CGFloat) -> CGPoint {
        let radians = CGFloat(mask.rotation) * .pi / 180
        let localX = mask.radii.width * scale * cos(angle)
        let localY = mask.radii.height * scale * sin(angle)
        let cosR = cos(radians)
        let sinR = sin(radians)
        return CGPoint(
            x: mask.center.x + localX * cosR - localY * sinR,
            y: mask.center.y + localX * sinR + localY * cosR
        )
    }

    /// 内圈缩放（1 = 外圈，0.02 = 几乎全过渡）。
    static func innerScale(_ mask: MaskUi) -> CGFloat {
        min(max(1 - CGFloat(mask.radialFeather) / 100, 0.02), 0.98)
    }

    static func hitTest(mask: MaskUi, at point: CGPoint, geometry: MaskCanvasGeometry) -> MaskDragMode {
        switch mask.kind {
        case .linear: return hitTestLinear(mask, point, geometry)
        case .radial: return hitTestRadial(mask, point, geometry)
        case .brush: return .none
        }
    }

    private static func hitTestLinear(
        _ mask: MaskUi,
        _ point: CGPoint,
        _ geometry: MaskCanvasGeometry
    ) -> MaskDragMode {
        let fitted = geometry.fitted
        guard fitted.width > 0, fitted.height > 0 else { return .none }
        let center = canvasPoint(mask.linearCenter, fitted: fitted)
        let halfSpan = min(fitted.width, fitted.height) * linearPinHalfSpanRatio
        let dir = canvasVector(mask.linearDirection, fitted: fitted)
        let length = max(hypot(dir.dx, dir.dy), 1e-6)
        let normal = CGVector(dx: -dir.dy / length, dy: dir.dx / length)
        // 中心线端点（旋转手柄）。
        let pins = [
            CGPoint(x: center.x - normal.dx * halfSpan, y: center.y - normal.dy * halfSpan),
            CGPoint(x: center.x + normal.dx * halfSpan, y: center.y + normal.dy * halfSpan),
        ]
        // 距离最近的手柄优先（旋转端点 vs 边界上的羽化手柄可能靠得很近）。
        var bestMode = MaskDragMode.none
        var bestDistance = gripRadius
        for pin in pins {
            let distance = hypot(pin.x - point.x, pin.y - point.y)
            if distance < bestDistance {
                bestDistance = distance
                bestMode = .linearRotate
            }
        }
        for anchor in [mask.start, mask.end] {
            let grip = canvasPoint(anchor, fitted: fitted)
            let distance = hypot(grip.x - point.x, grip.y - point.y)
            if distance < bestDistance {
                bestDistance = distance
                bestMode = .linearFeather
            }
        }
        if bestMode != .none {
            return bestMode
        }
        // 中心线 / 边界线距离（都沿渐变方向度量）。
        let offset = CGVector(dx: point.x - center.x, dy: point.y - center.y)
        let centerDistance = abs(offset.dx * normal.dx + offset.dy * normal.dy)
        let gradient = CGVector(dx: dir.dx / length, dy: dir.dy / length)
        let anchorA = canvasPoint(mask.start, fitted: fitted)
        let anchorB = canvasPoint(mask.end, fitted: fitted)
        let distanceA = abs((point.x - anchorA.x) * gradient.dx + (point.y - anchorA.y) * gradient.dy)
        let distanceB = abs((point.x - anchorB.x) * gradient.dx + (point.y - anchorB.y) * gradient.dy)
        let boundaryDistance = min(distanceA, distanceB)
        // 靠近某条过渡边界（比中心线更近）→ 调整过渡范围。
        if boundaryDistance + 4 < centerDistance, boundaryDistance < 26 {
            return .linearFeather
        }
        // 中心线附近 → 平移。
        let along = abs(offset.dx * normal.dy - offset.dy * normal.dx)
        if centerDistance < lineDistance, along < halfSpan + 40 {
            return .linearMove
        }
        // 兜底：拖动蒙版区域也平移。
        return .linearMove
    }

    private static func hitTestRadial(
        _ mask: MaskUi,
        _ point: CGPoint,
        _ geometry: MaskCanvasGeometry
    ) -> MaskDragMode {
        let fitted = geometry.fitted
        guard fitted.width > 0, fitted.height > 0 else { return .none }
        let center = canvasPoint(mask.center, fitted: fitted)
        let scale = innerScale(mask)
        // 全部手柄一起比距离：最近者优先（内圈手柄可能与外圈手柄重叠）。
        var bestMode = MaskDragMode.none
        var bestDistance = gripRadius
        // 旋转手柄（外圈顶部再外置一段）。
        let top = canvasPoint(radialPoint(mask, angle: -.pi / 2, scale: 1), fitted: fitted)
        let directionX = top.x - center.x
        let directionY = top.y - center.y
        let directionLength = max(hypot(directionX, directionY), 1e-6)
        let rotatePoint = CGPoint(
            x: top.x + directionX / directionLength * rotateOffset,
            y: top.y + directionY / directionLength * rotateOffset
        )
        let rotateDistance = hypot(rotatePoint.x - point.x, rotatePoint.y - point.y)
        if rotateDistance < bestDistance {
            bestDistance = rotateDistance
            bestMode = .radialRotate
        }
        // 外圈 / 内圈四向手柄。
        for (index, angle) in cardinalAngles.enumerated() {
            let grip = canvasPoint(radialPoint(mask, angle: angle, scale: 1), fitted: fitted)
            let distance = hypot(grip.x - point.x, grip.y - point.y)
            if distance < bestDistance {
                bestDistance = distance
                bestMode = .radialRadius(index % 2)
            }
            let innerGrip = canvasPoint(
                radialPoint(mask, angle: angle, scale: scale),
                fitted: fitted
            )
            let innerDistance = hypot(innerGrip.x - point.x, innerGrip.y - point.y)
            if innerDistance < bestDistance {
                bestDistance = innerDistance
                bestMode = .radialFeather(index % 2)
            }
        }
        if bestMode != .none {
            return bestMode
        }
        // 外圈 / 内圈边界附近 → 按局部角度决定缩放哪条半径（拖动边界即缩放）。
        // 注意：此处的 point 已是内容坐标。
        let normalized = geometry.normalized(fromContent: point)
        let localAngle = MaskDragSession.localAngle(mask, normalized)
        let axis = abs(cos(localAngle)) >= abs(sin(localAngle)) ? 0 : 1
        let outerBoundary = canvasPoint(
            radialPoint(mask, angle: localAngle, scale: 1),
            fitted: fitted
        )
        if hypot(outerBoundary.x - point.x, outerBoundary.y - point.y) < 26 {
            return .radialRadius(axis)
        }
        let innerBoundary = canvasPoint(
            radialPoint(mask, angle: localAngle, scale: scale),
            fitted: fitted
        )
        if hypot(innerBoundary.x - point.x, innerBoundary.y - point.y) < 22 {
            return .radialFeather(axis)
        }
        // 其余位置：移动。
        return .radialMove
    }
}

// MARK: - 拖动会话

/// 一次蒙版控件拖动：用起手快照 + 起点计算，避免累积漂移。
struct MaskDragSession {
    let mode: MaskDragMode
    let snapshot: MaskUi
    /// 起手点（归一化图像坐标）。
    let startPoint: CGPoint
    let geometry: MaskCanvasGeometry

    /// 按当前触点计算更新后的蒙版；无变化返回 nil。
    func updated(to point: CGPoint) -> MaskUi? {
        switch mode {
        case .none:
            return nil
        case .linearMove:
            let delta = CGPoint(x: point.x - startPoint.x, y: point.y - startPoint.y)
            var copy = snapshot
            copy.start = Self.clamped(snapshot.start, delta: delta)
            copy.end = Self.clamped(snapshot.end, delta: delta)
            return copy
        case .linearRotate:
            let center = snapshot.linearCenter
            let centerContent = geometry.contentPoint(normalized: center)
            let fromContent = geometry.contentPoint(normalized: startPoint)
            let toContent = geometry.contentPoint(normalized: point)
            let delta = Self.angleDelta(from: fromContent, to: toContent, around: centerContent)
            return Self.rotatedLinear(snapshot, byContentRadians: delta, fitted: geometry.fitted)
        case .linearFeather:
            let normal = Self.unitNormal(snapshot)
            let offset = (point.x - snapshot.linearCenter.x) * normal.dx
                + (point.y - snapshot.linearCenter.y) * normal.dy
            let offsetStart = (startPoint.x - snapshot.linearCenter.x) * normal.dx
                + (startPoint.y - snapshot.linearCenter.y) * normal.dy
            let width = snapshot.linearWidth + 2 * Double(offset - offsetStart)
            return snapshot.settingLinearWidth(width)
        case .radialMove:
            let delta = CGPoint(x: point.x - startPoint.x, y: point.y - startPoint.y)
            var copy = snapshot
            copy.center = Self.clamped(snapshot.center, delta: delta)
            return copy
        case .radialRotate:
            let from = Self.localAngle(snapshot, startPoint)
            let to = Self.localAngle(snapshot, point)
            let delta = Self.normalizedRadians(to - from)
            var copy = snapshot
            copy.rotation += Float(delta * 180 / .pi)
            return copy
        case .radialRadius(let axis):
            let local = Self.inverseRotate(snapshot, point: point)
            var copy = snapshot
            if axis == 0 {
                copy.radii.width = min(max(abs(local.dx), 0.04), 1.5)
            } else {
                copy.radii.height = min(max(abs(local.dy), 0.04), 1.5)
            }
            return copy
        case .radialFeather(let axis):
            let local = Self.inverseRotate(snapshot, point: point)
            let scale: CGFloat
            if axis == 0 {
                scale = abs(local.dx) / max(snapshot.radii.width, 1e-4)
            } else {
                scale = abs(local.dy) / max(snapshot.radii.height, 1e-4)
            }
            var copy = snapshot
            let clampedScale = min(max(scale, 0.02), 0.98)
            copy.radialFeather = Float((1 - clampedScale) * 100)
            return copy
        }
    }

    // MARK: 工具函数

    static func clamped(_ point: CGPoint, delta: CGPoint) -> CGPoint {
        CGPoint(
            x: min(max(point.x + delta.x, -0.5), 1.5),
            y: min(max(point.y + delta.y, -0.5), 1.5)
        )
    }

    /// 渐变方向在归一化空间的单位法线（指向 0% 一侧）。
    static func unitNormal(_ mask: MaskUi) -> CGVector {
        let dir = mask.linearDirection
        return CGVector(dx: -dir.dy, dy: dir.dx)
    }

    /// 内容空间里，两条向量之间的有符号夹角。
    static func angleDelta(from: CGPoint, to: CGPoint, around center: CGPoint) -> CGFloat {
        let a = CGVector(dx: from.x - center.x, dy: from.y - center.y)
        let b = CGVector(dx: to.x - center.x, dy: to.y - center.y)
        return atan2(a.dx * b.dy - a.dy * b.dx, a.dx * b.dx + a.dy * b.dy)
    }

    /// 旋转线性渐变：在内容空间旋转，再换算回归一化方向（手柄跟手）。
    static func rotatedLinear(_ mask: MaskUi, byContentRadians radians: CGFloat, fitted: CGRect) -> MaskUi {
        let dir = mask.linearDirection
        var contentDir = CGVector(dx: dir.dx * fitted.width, dy: dir.dy * fitted.height)
        let length = hypot(contentDir.dx, contentDir.dy)
        guard length > 1e-6 else { return mask }
        contentDir = CGVector(dx: contentDir.dx / length, dy: contentDir.dy / length)
        let cosR = cos(radians)
        let sinR = sin(radians)
        let rotated = CGVector(
            dx: contentDir.dx * cosR - contentDir.dy * sinR,
            dy: contentDir.dx * sinR + contentDir.dy * cosR
        )
        var normalized = CGVector(dx: rotated.dx / fitted.width, dy: rotated.dy / fitted.height)
        let normLength = hypot(normalized.dx, normalized.dy)
        guard normLength > 1e-9 else { return mask }
        normalized = CGVector(dx: normalized.dx / normLength, dy: normalized.dy / normLength)
        var copy = mask
        let half = CGFloat(mask.linearWidth / 2)
        let center = mask.linearCenter
        copy.start = CGPoint(x: center.x - normalized.dx * half, y: center.y - normalized.dy * half)
        copy.end = CGPoint(x: center.x + normalized.dx * half, y: center.y + normalized.dy * half)
        return copy
    }

    /// 椭圆局部参数角（用于旋转手柄跟手）。
    static func localAngle(_ mask: MaskUi, _ point: CGPoint) -> CGFloat {
        let local = inverseRotate(mask, point: point)
        return atan2(
            local.dy / max(mask.radii.height, 1e-4),
            local.dx / max(mask.radii.width, 1e-4)
        )
    }

    /// 去掉椭圆旋转后的局部向量（与核心 radial_mask 同一套公式）。
    static func inverseRotate(_ mask: MaskUi, point: CGPoint) -> CGVector {
        let radians = CGFloat(mask.rotation) * .pi / 180
        let cosR = cos(radians)
        let sinR = sin(radians)
        let dx = point.x - mask.center.x
        let dy = point.y - mask.center.y
        return CGVector(dx: dx * cosR + dy * sinR, dy: -dx * sinR + dy * cosR)
    }

    static func normalizedRadians(_ radians: CGFloat) -> CGFloat {
        var value = radians
        while value > .pi {
            value -= 2 * .pi
        }
        while value < -.pi {
            value += 2 * .pi
        }
        return value
    }
}

// MARK: - 交互控制器

/// 把触摸事件翻译成蒙版 / 画布操作（SwiftUI 侧的薄控制器）。
@MainActor
final class MaskCanvasController {
    private var drag: MaskDragSession?
    private var brushing = false
    private var dragging = false
    private var rotationActive = false
    private var rotationAccumulated: Double = 0
    private var rotationApplied: Double = 0

    func handle(
        _ event: MaskTouchEvent,
        geometry: MaskCanvasGeometry,
        model: EditorModel,
        zoom: Binding<CGFloat>,
        pan: Binding<CGSize>
    ) {
        switch event {
        case .singleBegan(let point):
            guard let mask = model.selectedMask else { return }
            let normalized = clampedNormalized(geometry.normalized(point))
            if mask.kind == .brush {
                model.brushCursor = normalized
                model.beginBrushStroke(at: normalized)
                brushing = true
                drag = nil
                return
            }
            // 手柄命中测试在内容坐标（预览图坐标系）里做：缩放 / 平移下位置才对得上。
            let mode = MaskHitTester.hitTest(
                mask: mask,
                at: geometry.contentPoint(point),
                geometry: geometry
            )
            guard mode != .none else { return }
            drag = MaskDragSession(
                mode: mode,
                snapshot: mask,
                startPoint: normalized,
                geometry: geometry
            )
            dragging = true
            model.beginMaskDrag()
        case .singleMoved(let point):
            if brushing {
                let normalized = clampedNormalized(geometry.normalized(point))
                model.brushCursor = normalized
                model.extendBrushStroke(to: normalized)
                return
            }
            guard let session = drag, dragging,
                  let updated = session.updated(to: clampedNormalized(geometry.normalized(point)))
            else {
                return
            }
            model.updateSelectedMask { $0 = updated }
        case .singleEnded, .singleCancelled:
            finishSingle(model: model)
        case .twoFingerPan(let delta):
            pan.wrappedValue = CGSize(
                width: pan.wrappedValue.width + delta.width,
                height: pan.wrappedValue.height + delta.height
            )
        case .twoFingerPinch(let scale):
            let newZoom = (zoom.wrappedValue * scale).clamped(to: 1...6)
            zoom.wrappedValue = newZoom
            if newZoom <= 1.01 {
                pan.wrappedValue = .zero
            }
        case .twoFingerRotate(let radians, let centroid):
            rotateSelectedMask(
                byViewRadians: radians,
                centroid: centroid,
                geometry: geometry,
                model: model
            )
        case .twoFingerEnded:
            if rotationActive {
                rotationActive = false
                model.endMaskDrag()
            }
        }
    }

    /// 退出蒙版工具 / 切换蒙版时清理交互状态。
    func reset() {
        drag = nil
        brushing = false
        dragging = false
        rotationActive = false
        rotationAccumulated = 0
        rotationApplied = 0
    }

    private func finishSingle(model: EditorModel) {
        if brushing {
            brushing = false
            model.endBrushStroke()
        }
        if dragging {
            dragging = false
            drag = nil
            model.endMaskDrag()
        }
    }

    private func rotateSelectedMask(
        byViewRadians radians: CGFloat,
        centroid: CGPoint,
        geometry: MaskCanvasGeometry,
        model: EditorModel
    ) {
        guard let mask = model.selectedMask,
              mask.kind == .linear || mask.kind == .radial
        else {
            return
        }
        // view 空间的旋转角 → 归一化空间的等效角度（各向异性缩放会改变角度）。
        let centerContent = geometry.contentPoint(normalized: mask.kind == .linear ? mask.linearCenter : mask.center)
        let centroidContent = geometry.contentPoint(centroid)
        let cosR = cos(radians)
        let sinR = sin(radians)
        let vx = centroidContent.x - centerContent.x
        let vy = centroidContent.y - centerContent.y
        let rotated = CGPoint(x: vx * cosR - vy * sinR, y: vx * sinR + vy * cosR)
        let fitted = geometry.fitted
        guard fitted.width > 0, fitted.height > 0 else { return }
        let a = CGVector(dx: vx / fitted.width, dy: vy / fitted.height)
        let b = CGVector(dx: rotated.x / fitted.width, dy: rotated.y / fitted.height)
        // 归一化空间里的等效旋转角。
        let delta = MaskDragSession.normalizedRadians(
            atan2(b.dy, b.dx) - atan2(a.dy, a.dx)
        )
        if !rotationActive {
            rotationActive = true
            rotationAccumulated = 0
            rotationApplied = 0
            model.beginMaskDrag()
        }
        rotationAccumulated += Double(MaskDragSession.normalizedRadians(delta))
        let step = rotationAccumulated - rotationApplied
        rotationApplied = rotationAccumulated
        guard abs(step) > 1e-5 else { return }
        model.updateSelectedMask { current in
            switch current.kind {
            case .linear:
                let angle = Double(current.linearAngleDegrees) * .pi / 180 + step
                let degrees = Float(angle * 180 / .pi)
                let rotatedMask = current.settingLinearAngle(degrees)
                current.start = rotatedMask.start
                current.end = rotatedMask.end
            case .radial:
                current.rotation += Float(step * 180 / .pi)
            case .brush:
                break
            }
        }
    }

    private func clampedNormalized(_ point: CGPoint) -> CGPoint {
        CGPoint(
            x: min(max(point.x, 0), 1),
            y: min(max(point.y, 0), 1)
        )
    }
}

// MARK: - 叠加层

/// 蒙版叠加层：真实 alpha 的红色覆盖 + 控件（中心线 / 端点 / 过渡边界 / 椭圆 / 手柄）。
///
/// 外层与预览图使用同一套缩放 / 平移，画布坐标直接对应图像坐标。
struct MaskOverlayLayer: View {
    @ObservedObject var model: EditorModel
    let imageSize: CGSize
    let viewSize: CGSize
    let zoom: CGFloat
    let pan: CGSize

    var body: some View {
        ZStack {
            if let mask = model.selectedMask {
                if model.maskOverlayVisible, let overlay = model.maskOverlays[mask.id] {
                    Image(uiImage: overlay)
                        .resizable()
                        .scaledToFit()
                        .frame(width: viewSize.width, height: viewSize.height)
                }
                Canvas { context, size in
                    let fitted = fittedRect(imageSize: imageSize, in: size)
                    drawControls(&context, mask: mask, fitted: fitted)
                }
                .frame(width: viewSize.width, height: viewSize.height)
            }
        }
        .frame(width: viewSize.width, height: viewSize.height)
        .allowsHitTesting(false)
        .scaleEffect(zoom)
        .offset(pan)
    }

    // MARK: 控件绘制（内容坐标；外层统一缩放）

    private func drawControls(_ context: inout GraphicsContext, mask: MaskUi, fitted: CGRect) {
        switch mask.kind {
        case .linear:
            drawLinear(&context, mask: mask, fitted: fitted)
        case .radial:
            drawRadial(&context, mask: mask, fitted: fitted)
        case .brush:
            drawBrushCursor(&context, mask: mask, fitted: fitted)
        }
    }

    private func drawLinear(_ context: inout GraphicsContext, mask: MaskUi, fitted: CGRect) {
        let center = canvasPoint(mask.linearCenter, fitted: fitted)
        let dir = canvasVector(mask.linearDirection, fitted: fitted)
        let length = max(hypot(dir.dx, dir.dy), 1e-6)
        let normal = CGVector(dx: -dir.dy / length, dy: dir.dx / length)
        let span = max(fitted.width, fitted.height) * 1.2
        // 过渡边界（100% / 0% 两条线）。
        for anchor in [mask.start, mask.end] {
            let point = canvasPoint(anchor, fitted: fitted)
            var path = Path()
            path.move(to: CGPoint(x: point.x - normal.dx * span, y: point.y - normal.dy * span))
            path.addLine(to: CGPoint(x: point.x + normal.dx * span, y: point.y + normal.dy * span))
            context.stroke(path, with: .color(.white.opacity(0.7)), lineWidth: 1.5)
        }
        // 50% 中心线（虚线，端点可拖动旋转）。
        let halfSpan = min(fitted.width, fitted.height) * MaskHitTester.linearPinHalfSpanRatio
        let pinA = CGPoint(x: center.x - normal.dx * halfSpan, y: center.y - normal.dy * halfSpan)
        let pinB = CGPoint(x: center.x + normal.dx * halfSpan, y: center.y + normal.dy * halfSpan)
        var centerLine = Path()
        centerLine.move(to: pinA)
        centerLine.addLine(to: pinB)
        context.stroke(
            centerLine,
            with: .color(.white.opacity(0.9)),
            style: StrokeStyle(lineWidth: 1.5, dash: [7, 5])
        )
        drawGrip(&context, at: pinA, color: .white, radius: 8)
        drawGrip(&context, at: pinB, color: .white, radius: 8)
        // 过渡 / 羽化手柄（黄色，落在两条边界线上）。
        for anchor in [mask.start, mask.end] {
            drawGrip(
                &context,
                at: canvasPoint(anchor, fitted: fitted),
                color: Color(red: 1.0, green: 0.78, blue: 0.2),
                radius: 6.5
            )
        }
    }

    private func drawRadial(_ context: inout GraphicsContext, mask: MaskUi, fitted: CGRect) {
        let innerScale = MaskHitTester.innerScale(mask)
        let outer = radialPath(mask, scale: 1, fitted: fitted)
        let inner = radialPath(mask, scale: innerScale, fitted: fitted)
        context.stroke(outer, with: .color(.white.opacity(0.9)), lineWidth: 2)
        context.stroke(
            inner,
            with: .color(.white.opacity(0.5)),
            style: StrokeStyle(lineWidth: 1.5, dash: [6, 5])
        )
        let center = canvasPoint(mask.center, fitted: fitted)
        // 旋转手柄（外圈顶部往外一小段）。
        let top = canvasPoint(
            MaskHitTester.radialPoint(mask, angle: -.pi / 2, scale: 1),
            fitted: fitted
        )
        let directionX = top.x - center.x
        let directionY = top.y - center.y
        let directionLength = max(hypot(directionX, directionY), 1e-6)
        let rotatePoint = CGPoint(
            x: top.x + directionX / directionLength * MaskHitTester.rotateOffset,
            y: top.y + directionY / directionLength * MaskHitTester.rotateOffset
        )
        var connector = Path()
        connector.move(to: top)
        connector.addLine(to: rotatePoint)
        context.stroke(connector, with: .color(.white.opacity(0.7)), lineWidth: 1)
        drawGrip(&context, at: rotatePoint, color: Color(red: 0.36, green: 0.68, blue: 1.0), radius: 8)
        // 外圈 / 内圈四向手柄。
        for angle in MaskHitTester.cardinalAngles {
            let outerGrip = canvasPoint(
                MaskHitTester.radialPoint(mask, angle: angle, scale: 1),
                fitted: fitted
            )
            drawGrip(&context, at: outerGrip, color: .white, radius: 7)
            let innerGrip = canvasPoint(
                MaskHitTester.radialPoint(mask, angle: angle, scale: innerScale),
                fitted: fitted
            )
            drawGrip(
                &context,
                at: innerGrip,
                color: Color(red: 1.0, green: 0.78, blue: 0.2),
                radius: 5.5
            )
        }
        drawGrip(&context, at: center, color: .white, radius: 4.5)
    }

    private func drawBrushCursor(_ context: inout GraphicsContext, mask: MaskUi, fitted: CGRect) {
        guard let cursor = model.brushCursor else { return }
        let radius = CGFloat(mask.brushSize) / 100 * max(fitted.width, fitted.height) / 2
        let point = canvasPoint(cursor, fitted: fitted)
        let rect = CGRect(
            x: point.x - radius,
            y: point.y - radius,
            width: radius * 2,
            height: radius * 2
        )
        let tint: Color = model.brushErase ? .red : .white
        context.fill(Path(ellipseIn: rect), with: .color(tint.opacity(0.12)))
        context.stroke(Path(ellipseIn: rect), with: .color(tint.opacity(0.9)), lineWidth: 1.5)
    }

    private func radialPath(_ mask: MaskUi, scale: CGFloat, fitted: CGRect) -> Path {
        var path = Path()
        let segments = 72
        for index in 0...segments {
            let angle = CGFloat(index) / CGFloat(segments) * 2 * .pi
            let point = canvasPoint(
                MaskHitTester.radialPoint(mask, angle: angle, scale: scale),
                fitted: fitted
            )
            if index == 0 {
                path.move(to: point)
            } else {
                path.addLine(to: point)
            }
        }
        path.closeSubpath()
        return path
    }

    private func drawGrip(
        _ context: inout GraphicsContext,
        at point: CGPoint,
        color: Color,
        radius: CGFloat
    ) {
        let rect = CGRect(
            x: point.x - radius,
            y: point.y - radius,
            width: radius * 2,
            height: radius * 2
        )
        context.fill(Path(ellipseIn: rect), with: .color(color))
        context.stroke(Path(ellipseIn: rect), with: .color(.black.opacity(0.35)), lineWidth: 1)
    }
}
