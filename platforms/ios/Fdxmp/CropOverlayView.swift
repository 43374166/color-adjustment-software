import SwiftUI

/// 裁切叠加层：可拖动的裁剪框 + 三分线 + 角点手柄。
struct CropOverlayView: View {
    @ObservedObject var model: EditorModel
    /// 预览图像的像素尺寸（未裁切）。
    let imageSize: CGSize
    /// 拉直刻度盘拖动中：临时显示网格（不写回裁切参数，松手恢复原网格）。
    var forceGrid = false

    private enum DragMode {
        case none
        case move
        case topLeft
        case topRight
        case bottomLeft
        case bottomRight
    }

    @State private var mode: DragMode = .none
    @State private var startRect: CGRect = .zero
    @State private var startLocation: CGPoint = .zero

    var body: some View {
        GeometryReader { geo in
            let bounds = geo.size
            let fitted = fittedRect(imageSize: imageSize, in: bounds)
            let rect = cropRectInPoints(fitted: fitted)
            Canvas { context, _ in
                drawDimming(&context, bounds: bounds, rect: rect)
                drawGrid(&context, rect: rect)
                drawFrame(&context, rect: rect)
            }
            .contentShape(Rectangle())
            .gesture(dragGesture(fitted: fitted))
        }
    }

    // MARK: - 手势

    private func dragGesture(fitted: CGRect) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { gesture in
                if mode == .none {
                    let hit = hitMode(at: gesture.startLocation, rect: cropRectInPoints(fitted: fitted))
                    guard hit != .none else { return }
                    mode = hit
                    startRect = cropRectInPoints(fitted: fitted)
                    startLocation = gesture.startLocation
                    model.beginEdit()
                }
                let delta = CGSize(
                    width: gesture.location.x - startLocation.x,
                    height: gesture.location.y - startLocation.y
                )
                let updated: CGRect
                if mode == .move {
                    updated = movedRect(start: startRect, delta: delta, fitted: fitted)
                } else {
                    updated = resizedRect(
                        mode: mode,
                        start: startRect,
                        delta: delta,
                        fitted: fitted,
                        ratio: model.state.crop.aspect.ratio
                    )
                }
                model.updateCropRect(normalized(updated, fitted: fitted), commit: false)
            }
            .onEnded { _ in
                guard mode != .none else { return }
                mode = .none
                Haptics.light()
                // 越界阻尼回弹 + 记一步历史（模型内部缓动，Canvas 每帧重绘）。
                model.finishCropDrag()
            }
    }

    private func hitMode(at point: CGPoint, rect: CGRect) -> DragMode {
        let handles: [(DragMode, CGPoint)] = [
            (.topLeft, CGPoint(x: rect.minX, y: rect.minY)),
            (.topRight, CGPoint(x: rect.maxX, y: rect.minY)),
            (.bottomLeft, CGPoint(x: rect.minX, y: rect.maxY)),
            (.bottomRight, CGPoint(x: rect.maxX, y: rect.maxY)),
        ]
        for (handle, position) in handles
        where hypot(position.x - point.x, position.y - point.y) <= 40 {
            return handle
        }
        if rect.contains(point) {
            return .move
        }
        return .none
    }

    private func movedRect(start: CGRect, delta: CGSize, fitted: CGRect) -> CGRect {
        let maxX = max(fitted.maxX - start.width, fitted.minX)
        let maxY = max(fitted.maxY - start.height, fitted.minY)
        // 允许最多 5% 的阻尼越界：拖出边界时手感变"软"，松手回弹。
        let x = damped(start.minX + delta.width, min: fitted.minX, max: maxX, overshoot: fitted.width * 0.05)
        let y = damped(start.minY + delta.height, min: fitted.minY, max: maxY, overshoot: fitted.height * 0.05)
        return CGRect(x: x, y: y, width: start.width, height: start.height)
    }

    /// 越界阻尼：正常范围内原样返回，越界部分只保留 30%（上限 overshoot）。
    private func damped(_ value: CGFloat, min lo: CGFloat, max hi: CGFloat, overshoot: CGFloat) -> CGFloat {
        if value < lo {
            return max(lo - overshoot, lo - (lo - value) * 0.3)
        }
        if value > hi {
            return min(hi + overshoot, hi + (value - hi) * 0.3)
        }
        return value
    }

    private func resizedRect(
        mode: DragMode,
        start: CGRect,
        delta: CGSize,
        fitted: CGRect,
        ratio: CGFloat?
    ) -> CGRect {
        var minX = start.minX
        var maxX = start.maxX
        var minY = start.minY
        var maxY = start.maxY
        switch mode {
        case .topLeft:
            minX += delta.width
            minY += delta.height
        case .topRight:
            maxX += delta.width
            minY += delta.height
        case .bottomLeft:
            minX += delta.width
            maxY += delta.height
        case .bottomRight:
            maxX += delta.width
            maxY += delta.height
        case .move, .none:
            break
        }
        minX = minX.clamped(to: fitted.minX...fitted.maxX)
        maxX = maxX.clamped(to: fitted.minX...fitted.maxX)
        minY = minY.clamped(to: fitted.minY...fitted.maxY)
        maxY = maxY.clamped(to: fitted.minY...fitted.maxY)

        var width = max(maxX - minX, 48)
        var height = max(maxY - minY, 48)
        if let ratio, ratio > 0 {
            height = width / ratio
            if height > fitted.height {
                height = fitted.height
                width = height * ratio
            }
            if width > fitted.width {
                width = fitted.width
                height = width / ratio
            }
        }
        let anchorX: CGFloat = (mode == .topLeft || mode == .bottomLeft) ? maxX : minX
        let anchorY: CGFloat = (mode == .topLeft || mode == .topRight) ? maxY : minY
        var originX = (mode == .topLeft || mode == .bottomLeft) ? anchorX - width : anchorX
        var originY = (mode == .topLeft || mode == .topRight) ? anchorY - height : anchorY
        originX = originX.clamped(
            to: fitted.minX...max(fitted.maxX - width, fitted.minX)
        )
        originY = originY.clamped(
            to: fitted.minY...max(fitted.maxY - height, fitted.minY)
        )
        return CGRect(x: originX, y: originY, width: width, height: height)
    }

    // MARK: - 坐标换算

    private func cropRectInPoints(fitted: CGRect) -> CGRect {
        // 拖动中允许阻尼越界，这里用原始矩形绘制（提交时会夹回合法范围）。
        let rect = model.state.crop.rect
        return CGRect(
            x: fitted.minX + rect.minX * fitted.width,
            y: fitted.minY + rect.minY * fitted.height,
            width: rect.width * fitted.width,
            height: rect.height * fitted.height
        )
    }

    private func normalized(_ rect: CGRect, fitted: CGRect) -> CGRect {
        guard fitted.width > 0, fitted.height > 0 else { return model.state.crop.rect }
        return CGRect(
            x: (rect.minX - fitted.minX) / fitted.width,
            y: (rect.minY - fitted.minY) / fitted.height,
            width: rect.width / fitted.width,
            height: rect.height / fitted.height
        )
    }

    // MARK: - 绘制

    private func drawDimming(_ context: inout GraphicsContext, bounds: CGSize, rect: CGRect) {
        let color = Color.black.opacity(0.55)
        let regions = [
            CGRect(x: 0, y: 0, width: bounds.width, height: max(rect.minY, 0)),
            CGRect(
                x: 0, y: rect.maxY,
                width: bounds.width, height: max(bounds.height - rect.maxY, 0)
            ),
            CGRect(x: 0, y: rect.minY, width: max(rect.minX, 0), height: rect.height),
            CGRect(
                x: rect.maxX, y: rect.minY,
                width: max(bounds.width - rect.maxX, 0), height: rect.height
            ),
        ]
        for region in regions where region.width > 0 && region.height > 0 {
            context.fill(Path(region), with: .color(color))
        }
    }

    private func drawGrid(_ context: inout GraphicsContext, rect: CGRect) {
        // 拉直刻度盘拖动中默认铺 4×4 网格，方便对地平线 / 建筑垂线；
        // 只在拖动期间覆盖显示，松手后仍回到用户选的构图网格。
        let style: CropGridUi = forceGrid ? .grid : model.state.crop.grid
        var grid = Path()
        switch style {
        case .none:
            return
        case .thirds:
            addGridLines(&grid, rect: rect, ratios: [1.0 / 3, 2.0 / 3])
        case .grid:
            addGridLines(&grid, rect: rect, ratios: [0.25, 0.5, 0.75])
        case .golden:
            addGridLines(&grid, rect: rect, ratios: [0.382, 0.618])
        case .diagonal:
            grid.move(to: CGPoint(x: rect.minX, y: rect.minY))
            grid.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY))
            grid.move(to: CGPoint(x: rect.maxX, y: rect.minY))
            grid.addLine(to: CGPoint(x: rect.minX, y: rect.maxY))
        }
        context.stroke(
            grid,
            with: .color(.white.opacity(forceGrid ? 0.5 : 0.35)),
            lineWidth: 1
        )
    }

    /// 按比例加横竖构图线。
    private func addGridLines(_ grid: inout Path, rect: CGRect, ratios: [CGFloat]) {
        for ratio in ratios {
            let x = rect.minX + rect.width * ratio
            let y = rect.minY + rect.height * ratio
            grid.move(to: CGPoint(x: x, y: rect.minY))
            grid.addLine(to: CGPoint(x: x, y: rect.maxY))
            grid.move(to: CGPoint(x: rect.minX, y: y))
            grid.addLine(to: CGPoint(x: rect.maxX, y: y))
        }
    }

    private func drawFrame(_ context: inout GraphicsContext, rect: CGRect) {
        context.stroke(
            Path(rect),
            with: .color(.white),
            style: StrokeStyle(lineWidth: 2)
        )
        // 角点手柄（L 形）
        let length: CGFloat = 22
        let thickness: CGFloat = 4
        var handles = Path()
        let corners = [
            (CGPoint(x: rect.minX, y: rect.minY), CGPoint(x: 1, y: 0), CGPoint(x: 0, y: 1)),
            (CGPoint(x: rect.maxX, y: rect.minY), CGPoint(x: -1, y: 0), CGPoint(x: 0, y: 1)),
            (CGPoint(x: rect.minX, y: rect.maxY), CGPoint(x: 1, y: 0), CGPoint(x: 0, y: -1)),
            (CGPoint(x: rect.maxX, y: rect.maxY), CGPoint(x: -1, y: 0), CGPoint(x: 0, y: -1)),
        ]
        for (corner, dx, dy) in corners {
            handles.move(to: corner)
            handles.addLine(
                to: CGPoint(x: corner.x + dx.x * length, y: corner.y + dy.x * length)
            )
            handles.move(to: corner)
            handles.addLine(
                to: CGPoint(x: corner.x + dx.y * length, y: corner.y + dy.y * length)
            )
        }
        context.stroke(
            handles,
            with: .color(.white),
            style: StrokeStyle(lineWidth: thickness, lineCap: .round)
        )
    }
}
