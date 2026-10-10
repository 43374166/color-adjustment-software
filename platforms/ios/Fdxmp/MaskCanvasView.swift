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
    /// 拖动某条过渡边界（0 = 100% 一侧，1 = 0% 一侧）——只移动被抓住的那条线。
    case linearFeather(Int)
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

    /// 椭圆参数点（**内容坐标**）：中心 + R(rotation)·(ax·s·cos t, ay·s·sin t)。
    ///
    /// 旋转在像素等比空间里做（与核心 `radial_mask` 一致）：屏幕上是标准旋转椭圆。
    static func radialPoint(
        _ mask: MaskUi,
        angle: CGFloat,
        scale: CGFloat,
        fitted: CGRect
    ) -> CGPoint {
        let radians = CGFloat(mask.rotation) * .pi / 180
        let localX = mask.radii.width * scale * fitted.width * cos(angle)
        let localY = mask.radii.height * scale * fitted.height * sin(angle)
        let cosR = cos(radians)
        let sinR = sin(radians)
        let center = canvasPoint(mask.center, fitted: fitted)
        return CGPoint(
            x: center.x + localX * cosR - localY * sinR,
            y: center.y + localX * sinR + localY * cosR
        )
    }

    /// 椭圆半轴（内容点）。
    static func radialAxes(_ mask: MaskUi, fitted: CGRect) -> CGVector {
        CGVector(
            dx: max(mask.radii.width * fitted.width, 1e-4),
            dy: max(mask.radii.height * fitted.height, 1e-4)
        )
    }

    /// 点相对椭圆中心的局部向量（内容点，已去掉旋转）。
    static func inverseRotate(_ mask: MaskUi, point: CGPoint, fitted: CGRect) -> CGVector {
        let radians = CGFloat(mask.rotation) * .pi / 180
        let cosR = cos(radians)
        let sinR = sin(radians)
        let center = canvasPoint(mask.center, fitted: fitted)
        let dx = point.x - center.x
        let dy = point.y - center.y
        return CGVector(dx: dx * cosR + dy * sinR, dy: -dx * sinR + dy * cosR)
    }

    /// 椭圆参数角（内容坐标；用于旋转手柄跟手与边界判定）。
    static func localAngle(_ mask: MaskUi, point: CGPoint, fitted: CGRect) -> CGFloat {
        let local = inverseRotate(mask, point: point, fitted: fitted)
        let axes = radialAxes(mask, fitted: fitted)
        return atan2(local.dy / axes.dy, local.dx / axes.dx)
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
        // 中心线 / 过渡边界的共同方向 = 归一化空间垂线在内容空间的像。
        let dir = mask.linearDirection
        let across = canvasVector(CGVector(dx: -dir.dy, dy: dir.dx), fitted: fitted)
        let acrossLength = max(hypot(across.dx, across.dy), 1e-6)
        let normal = CGVector(dx: across.dx / acrossLength, dy: across.dy / acrossLength)
        // 中心线端点（旋转手柄）。
        let pins = [
            CGPoint(x: center.x - normal.dx * halfSpan, y: center.y - normal.dy * halfSpan),
            CGPoint(x: center.x + normal.dx * halfSpan, y: center.y + normal.dy * halfSpan),
        ]
        // 距离最近的手柄优先（旋转端点 vs 边界上的过渡手柄可能靠得很近）。
        var bestMode = MaskDragMode.none
        var bestDistance = gripRadius
        for pin in pins {
            let distance = hypot(pin.x - point.x, pin.y - point.y)
            if distance < bestDistance {
                bestDistance = distance
                bestMode = .linearRotate
            }
        }
        for (index, anchor) in [mask.start, mask.end].enumerated() {
            let grip = canvasPoint(anchor, fitted: fitted)
            let distance = hypot(grip.x - point.x, grip.y - point.y)
            if distance < bestDistance {
                bestDistance = distance
                bestMode = .linearFeather(index)
            }
        }
        if bestMode != .none {
            return bestMode
        }
        // 到三条线的距离用**归一化空间的投影参数 t** 度量（与核心 / 绘制完全一致，
        // 不能直接拿内容空间的垂距，否则非正方形图片上会差一条斜线）。
        let normalized = geometry.normalized(fromContent: point)
        let t = MaskDragSession.parameter(mask, normalized)
        let bandPixels = hypot(
            (mask.end.x - mask.start.x) * fitted.width,
            (mask.end.y - mask.start.y) * fitted.height
        )
        let centerDistance = abs(t - 0.5) * bandPixels
        let boundaryDistance = min(abs(t), abs(t - 1)) * bandPixels
        // 靠近某条过渡边界（比中心线更近）→ 只移动那条边界。
        if boundaryDistance + 4 < centerDistance, boundaryDistance < 26 {
            return .linearFeather(t < 0.5 ? 0 : 1)
        }
        // 中心线附近 → 平移。
        let offset = CGVector(dx: point.x - center.x, dy: point.y - center.y)
        let along = abs(offset.dx * normal.dx + offset.dy * normal.dy)
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
        let top = radialPoint(mask, angle: -.pi / 2, scale: 1, fitted: fitted)
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
            let grip = radialPoint(mask, angle: angle, scale: 1, fitted: fitted)
            let distance = hypot(grip.x - point.x, grip.y - point.y)
            if distance < bestDistance {
                bestDistance = distance
                bestMode = .radialRadius(index % 2)
            }
            let innerGrip = radialPoint(mask, angle: angle, scale: scale, fitted: fitted)
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
        let localAngle = localAngle(mask, point: point, fitted: fitted)
        let axis = abs(cos(localAngle)) >= abs(sin(localAngle)) ? 0 : 1
        let outerBoundary = radialPoint(mask, angle: localAngle, scale: 1, fitted: fitted)
        if hypot(outerBoundary.x - point.x, outerBoundary.y - point.y) < 26 {
            return .radialRadius(axis)
        }
        let innerBoundary = radialPoint(mask, angle: localAngle, scale: scale, fitted: fitted)
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
    /// 起手点（内容坐标，椭圆类几何用）。
    let startContent: CGPoint
    let geometry: MaskCanvasGeometry

    /// 外圈往外拖时内圈跟随比例（0 = 内圈不动，1 = 等比缩放）。
    static let outerFollow: CGFloat = 0.35

    /// 按当前触点（内容坐标）计算更新后的蒙版；无变化返回 nil。
    func updated(to content: CGPoint) -> MaskUi? {
        let point = geometry.normalized(fromContent: content)
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
            let delta = Self.angleDelta(from: fromContent, to: content, around: centerContent)
            return Self.rotatedLinear(snapshot, byContentRadians: delta, fitted: geometry.fitted)
        case .linearFeather(let index):
            // 只沿渐变方向移动被抓住的那条边界线（另一条保持不动）。
            let dir = snapshot.linearDirection
            let projected = Double(
                (point.x - startPoint.x) * dir.dx + (point.y - startPoint.y) * dir.dy
            )
            var copy = snapshot
            if index == 0 {
                let limited = min(projected, snapshot.linearWidth - 0.02)
                copy.start = CGPoint(
                    x: snapshot.start.x + dir.dx * CGFloat(limited),
                    y: snapshot.start.y + dir.dy * CGFloat(limited)
                )
            } else {
                let limited = max(projected, -(snapshot.linearWidth - 0.02))
                copy.end = CGPoint(
                    x: snapshot.end.x + dir.dx * CGFloat(limited),
                    y: snapshot.end.y + dir.dy * CGFloat(limited)
                )
            }
            return copy
        case .radialMove:
            let delta = CGPoint(x: point.x - startPoint.x, y: point.y - startPoint.y)
            var copy = snapshot
            copy.center = Self.clamped(snapshot.center, delta: delta)
            return copy
        case .radialRotate:
            let from = MaskHitTester.localAngle(snapshot, point: startContent, fitted: geometry.fitted)
            let to = MaskHitTester.localAngle(snapshot, point: content, fitted: geometry.fitted)
            let delta = Self.normalizedRadians(to - from)
            var copy = snapshot
            copy.rotation += Float(delta * 180 / .pi)
            return copy
        case .radialRadius(let axis):
            // 外圈选手柄：改该轴半径；内圈跟着走一点（羽化不会被拉成一条缝）。
            let fitted = geometry.fitted
            let local = MaskHitTester.inverseRotate(snapshot, point: content, fitted: fitted)
            var copy = snapshot
            let innerScale = MaskHitTester.innerScale(snapshot)
            if axis == 0 {
                let radius0 = snapshot.radii.width
                let radius1 = min(max(abs(local.dx) / max(fitted.width, 1e-4), 0.04), 1.5)
                copy.radii.width = radius1
                let inner0 = radius0 * innerScale
                let scale = (inner0 + Self.outerFollow * (radius1 - radius0)) / max(radius1, 1e-4)
                copy.radialFeather = Float((1 - min(max(scale, 0.02), 0.98)) * 100)
            } else {
                let radius0 = snapshot.radii.height
                let radius1 = min(max(abs(local.dy) / max(fitted.height, 1e-4), 0.04), 1.5)
                copy.radii.height = radius1
                let inner0 = radius0 * innerScale
                let scale = (inner0 + Self.outerFollow * (radius1 - radius0)) / max(radius1, 1e-4)
                copy.radialFeather = Float((1 - min(max(scale, 0.02), 0.98)) * 100)
            }
            return copy
        case .radialFeather(let axis):
            // 内圈选手柄：直接拖内圈，羽化 = 内圈到外圈的距离。
            let fitted = geometry.fitted
            let local = MaskHitTester.inverseRotate(snapshot, point: content, fitted: fitted)
            let axes = MaskHitTester.radialAxes(snapshot, fitted: fitted)
            let scale: CGFloat
            if axis == 0 {
                scale = abs(local.dx) / axes.dx
            } else {
                scale = abs(local.dy) / axes.dy
            }
            var copy = snapshot
            let clampedScale = min(max(scale, 0.02), 0.98)
            copy.radialFeather = Float((1 - clampedScale) * 100)
            return copy
        }
    }

    /// 归一化点的渐变投影参数：0 = 100% 边界（start），1 = 0% 边界（end）。
    static func parameter(_ mask: MaskUi, _ normalized: CGPoint) -> CGFloat {
        let dir = mask.linearDirection
        let width = max(mask.linearWidth, 1e-6)
        let dx = normalized.x - mask.start.x
        let dy = normalized.y - mask.start.y
        return (dx * dir.dx + dy * dir.dy) / CGFloat(width)
    }

    // MARK: 工具函数

    static func clamped(_ point: CGPoint, delta: CGPoint) -> CGPoint {
        CGPoint(
            x: min(max(point.x + delta.x, -0.5), 1.5),
            y: min(max(point.y + delta.y, -0.5), 1.5)
        )
    }

    /// 内容空间里，两条向量之间的有符号夹角。
    static func angleDelta(from: CGPoint, to: CGPoint, around center: CGPoint) -> CGFloat {
        let a = CGVector(dx: from.x - center.x, dy: from.y - center.y)
        let b = CGVector(dx: to.x - center.x, dy: to.y - center.y)
        return atan2(a.dx * b.dy - a.dy * b.dx, a.dx * b.dx + a.dy * b.dy)
    }

    /// 旋转线性渐变：把手柄方向（归一化垂线的像）按手指转角旋转，再反解渐变方向——
    /// 端点跟着手指走，三条线与实际过渡方向始终一致。
    static func rotatedLinear(
        _ mask: MaskUi,
        byContentRadians radians: CGFloat,
        fitted: CGRect
    ) -> MaskUi {
        let dir = mask.linearDirection
        var handle = canvasVector(CGVector(dx: -dir.dy, dy: dir.dx), fitted: fitted)
        let handleLength = hypot(handle.dx, handle.dy)
        guard handleLength > 1e-6 else { return mask }
        handle = CGVector(dx: handle.dx / handleLength, dy: handle.dy / handleLength)
        let cosR = cos(radians)
        let sinR = sin(radians)
        let rotated = CGVector(
            dx: handle.dx * cosR - handle.dy * sinR,
            dy: handle.dx * sinR + handle.dy * cosR
        )
        // 反解：n = 归一化空间的垂线 → d = (n.y, -n.x)。
        var normal = CGVector(dx: rotated.dx / fitted.width, dy: rotated.dy / fitted.height)
        let normalLength = hypot(normal.dx, normal.dy)
        guard normalLength > 1e-9 else { return mask }
        normal = CGVector(dx: normal.dx / normalLength, dy: normal.dy / normalLength)
        let newDir = CGVector(dx: normal.dy, dy: -normal.dx)
        var copy = mask
        let half = CGFloat(mask.linearWidth / 2)
        let center = mask.linearCenter
        copy.start = CGPoint(x: center.x - newDir.dx * half, y: center.y - newDir.dy * half)
        copy.end = CGPoint(x: center.x + newDir.dx * half, y: center.y + newDir.dy * half)
        return copy
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
            let content = geometry.contentPoint(point)
            let normalized = clampedNormalized(geometry.normalized(fromContent: content))
            if mask.kind == .brush {
                model.brushCursor = normalized
                model.beginBrushStroke(at: normalized)
                Haptics.light()
                brushing = true
                drag = nil
                return
            }
            // 手柄命中测试在内容坐标（预览图坐标系）里做：缩放 / 平移下位置才对得上。
            let mode = MaskHitTester.hitTest(
                mask: mask,
                at: content,
                geometry: geometry
            )
            guard mode != .none else { return }
            Haptics.light()
            drag = MaskDragSession(
                mode: mode,
                snapshot: mask,
                startPoint: normalized,
                startContent: content,
                geometry: geometry
            )
            dragging = true
            model.beginMaskDrag(label: Self.dragLabel(for: mode))
        case .singleMoved(let point):
            if brushing {
                let normalized = clampedNormalized(
                    geometry.normalized(fromContent: geometry.contentPoint(point))
                )
                model.brushCursor = normalized
                model.extendBrushStroke(to: normalized)
                return
            }
            guard let session = drag, dragging,
                  let updated = session.updated(to: geometry.contentPoint(point))
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
        case .twoFingerRotate(let radians, _):
            rotateSelectedMask(
                byViewRadians: radians,
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
        geometry: MaskCanvasGeometry,
        model: EditorModel
    ) {
        guard let mask = model.selectedMask,
              mask.kind == .linear || mask.kind == .radial
        else {
            return
        }
        // 椭圆 / 渐变的旋转现在都定义在像素等比空间，view 空间的转角可以直接用。
        if !rotationActive {
            rotationActive = true
            rotationAccumulated = 0
            rotationApplied = 0
            model.beginMaskDrag(label: "旋转蒙版")
        }
        rotationAccumulated += Double(radians)
        let step = rotationAccumulated - rotationApplied
        rotationApplied = rotationAccumulated
        guard abs(step) > 1e-5 else { return }
        model.updateSelectedMask { current in
            switch current.kind {
            case .linear:
                let rotated = MaskDragSession.rotatedLinear(
                    current,
                    byContentRadians: CGFloat(step),
                    fitted: geometry.fitted
                )
                current.start = rotated.start
                current.end = rotated.end
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

    /// 拖动模式 → 历史步骤名（撤销提示用）。
    private static func dragLabel(for mode: MaskDragMode) -> String {
        switch mode {
        case .linearMove, .radialMove:
            return "移动蒙版"
        case .linearRotate, .radialRotate:
            return "旋转蒙版"
        case .linearFeather:
            return "蒙版过渡"
        case .radialRadius:
            return "蒙版大小"
        case .radialFeather:
            return "蒙版羽化"
        case .none:
            return "调整蒙版"
        }
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
        // 三条线的方向 = 归一化空间垂线在内容空间的像（核心按归一化投影算 alpha，
        // 直接取内容空间垂线在非正方形图片上会与实际渐变方向差一条斜线）。
        let dir = mask.linearDirection
        let across = canvasVector(CGVector(dx: -dir.dy, dy: dir.dx), fitted: fitted)
        let acrossLength = max(hypot(across.dx, across.dy), 1e-6)
        let normal = CGVector(dx: across.dx / acrossLength, dy: across.dy / acrossLength)
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
        // 内圈（100% 边界）实线、外圈（0% 边界）虚线：中间是平滑过渡带。
        context.stroke(
            outer,
            with: .color(.white.opacity(0.55)),
            style: StrokeStyle(lineWidth: 1.5, dash: [6, 5])
        )
        context.stroke(inner, with: .color(.white.opacity(0.95)), lineWidth: 2)
        let center = canvasPoint(mask.center, fitted: fitted)
        // 旋转手柄（外圈顶部往外一小段）。
        let top = MaskHitTester.radialPoint(mask, angle: -.pi / 2, scale: 1, fitted: fitted)
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
            let outerGrip = MaskHitTester.radialPoint(mask, angle: angle, scale: 1, fitted: fitted)
            drawGrip(&context, at: outerGrip, color: .white, radius: 7)
            let innerGrip = MaskHitTester.radialPoint(
                mask,
                angle: angle,
                scale: innerScale,
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
            let point = MaskHitTester.radialPoint(
                mask,
                angle: angle,
                scale: scale,
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
