import CoreGraphics
import SwiftUI

/// 蒙版叠加层：显示当前蒙版（画笔笔画 / 线性渐变 / 径向渐变）并接收手势。
struct MaskOverlayView: View {
    @ObservedObject var model: EditorModel
    /// 预览图像尺寸（未裁切）。
    let imageSize: CGSize

    @State private var dragStart: CGPoint?

    var body: some View {
        GeometryReader { geo in
            let fitted = fittedRect(imageSize: imageSize, in: geo.size)
            Canvas { context, _ in
                guard let mask = model.selectedMask else { return }
                switch mask.kind {
                case .brush:
                    drawBrush(&context, mask: mask, fitted: fitted)
                case .linear:
                    drawLinear(&context, mask: mask, fitted: fitted)
                case .radial:
                    drawRadial(&context, mask: mask, fitted: fitted)
                }
            }
            .contentShape(Rectangle())
            .gesture(gesture(fitted: fitted))
        }
    }

    // MARK: - 手势

    private func gesture(fitted: CGRect) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { value in
                guard let mask = model.selectedMask else { return }
                if dragStart == nil {
                    dragStart = value.startLocation
                    model.beginEdit()
                    let start = normalized(value.startLocation, fitted: fitted)
                    model.mutateSelectedMask { current in
                        switch current.kind {
                        case .brush:
                            current.strokes.append([start])
                        case .linear:
                            current.start = start
                            current.end = start
                        case .radial:
                            current.center = start
                            current.radii = CGSize(width: 0.05, height: 0.05)
                        }
                    }
                }
                let point = normalized(value.location, fitted: fitted)
                switch mask.kind {
                case .brush:
                    model.mutateSelectedMask { current in
                        guard !current.strokes.isEmpty else { return }
                        let lastIndex = current.strokes.count - 1
                        if let last = current.strokes[lastIndex].last,
                           hypot(last.x - point.x, last.y - point.y) < 0.004
                        {
                            return
                        }
                        current.strokes[lastIndex].append(point)
                    }
                case .linear:
                    model.mutateSelectedMask { $0.end = point }
                case .radial:
                    model.mutateSelectedMask { current in
                        let dx = abs(point.x - current.center.x)
                        let dy = abs(point.y - current.center.y)
                        current.radii = CGSize(
                            width: max(dx, 0.05),
                            height: max(dy, 0.05)
                        )
                    }
                }
            }
            .onEnded { _ in
                guard dragStart != nil else { return }
                dragStart = nil
                model.commitEdit()
            }
    }

    private func normalized(_ point: CGPoint, fitted: CGRect) -> CGPoint {
        guard fitted.width > 0, fitted.height > 0 else { return .zero }
        return CGPoint(
            x: ((point.x - fitted.minX) / fitted.width).clamped(to: 0...1),
            y: ((point.y - fitted.minY) / fitted.height).clamped(to: 0...1)
        )
    }

    // MARK: - 绘制

    private func drawBrush(
        _ context: inout GraphicsContext,
        mask: MaskUi,
        fitted: CGRect
    ) {
        let lineWidth = max(
            CGFloat(mask.brushSize) / 100 * max(fitted.width, fitted.height),
            6
        )
        for stroke in mask.strokes where stroke.count > 0 {
            var path = Path()
            for (index, point) in stroke.enumerated() {
                let position = CGPoint(
                    x: fitted.minX + point.x * fitted.width,
                    y: fitted.minY + point.y * fitted.height
                )
                if index == 0 {
                    path.move(to: position)
                } else {
                    path.addLine(to: position)
                }
            }
            if stroke.count == 1, let point = stroke.first {
                let position = CGPoint(
                    x: fitted.minX + point.x * fitted.width,
                    y: fitted.minY + point.y * fitted.height
                )
                context.fill(
                    Path(
                        ellipseIn: CGRect(
                            x: position.x - lineWidth / 2,
                            y: position.y - lineWidth / 2,
                            width: lineWidth,
                            height: lineWidth
                        )
                    ),
                    with: .color(maskColor(mask).opacity(0.4))
                )
            } else {
                context.stroke(
                    path,
                    with: .color(maskColor(mask).opacity(0.4)),
                    style: StrokeStyle(lineWidth: lineWidth, lineCap: .round, lineJoin: .round)
                )
            }
        }
    }

    private func drawLinear(
        _ context: inout GraphicsContext,
        mask: MaskUi,
        fitted: CGRect
    ) {
        let start = CGPoint(
            x: fitted.minX + mask.start.x * fitted.width,
            y: fitted.minY + mask.start.y * fitted.height
        )
        let end = CGPoint(
            x: fitted.minX + mask.end.x * fitted.width,
            y: fitted.minY + mask.end.y * fitted.height
        )
        var path = Path()
        path.move(to: start)
        path.addLine(to: end)
        context.stroke(
            path,
            with: .color(maskColor(mask)),
            style: StrokeStyle(lineWidth: 2, dash: [6, 4])
        )
        // 两端垂直于渐变方向的界线
        let dx = end.x - start.x
        let dy = end.y - start.y
        let length = max(hypot(dx, dy), 1)
        let normal = CGPoint(x: -dy / length, y: dx / length)
        let span = max(fitted.width, fitted.height)
        for center in [start, end] {
            var line = Path()
            line.move(
                to: CGPoint(x: center.x - normal.x * span, y: center.y - normal.y * span)
            )
            line.addLine(
                to: CGPoint(x: center.x + normal.x * span, y: center.y + normal.y * span)
            )
            context.stroke(line, with: .color(maskColor(mask).opacity(0.7)), lineWidth: 1)
        }
        for center in [start, end] {
            context.fill(
                Path(
                    ellipseIn: CGRect(
                        x: center.x - 6, y: center.y - 6, width: 12, height: 12
                    )
                ),
                with: .color(maskColor(mask))
            )
        }
    }

    private func drawRadial(
        _ context: inout GraphicsContext,
        mask: MaskUi,
        fitted: CGRect
    ) {
        let center = CGPoint(
            x: fitted.minX + mask.center.x * fitted.width,
            y: fitted.minY + mask.center.y * fitted.height
        )
        let radiusX = mask.radii.width * fitted.width
        let radiusY = mask.radii.height * fitted.height
        let rect = CGRect(
            x: center.x - radiusX,
            y: center.y - radiusY,
            width: radiusX * 2,
            height: radiusY * 2
        )
        context.stroke(
            Path(ellipseIn: rect),
            with: .color(maskColor(mask)),
            style: StrokeStyle(lineWidth: 2, dash: [6, 4])
        )
        let inner = CGFloat(max(min(mask.inner / 100, 0.98), 0))
        if inner > 0.01 {
            let innerRect = rect.insetBy(
                dx: rect.width / 2 * (1 - inner),
                dy: rect.height / 2 * (1 - inner)
            )
            context.stroke(
                Path(ellipseIn: innerRect),
                with: .color(maskColor(mask).opacity(0.5)),
                style: StrokeStyle(lineWidth: 1, dash: [4, 4])
            )
        }
        context.fill(
            Path(ellipseIn: CGRect(x: center.x - 5, y: center.y - 5, width: 10, height: 10)),
            with: .color(maskColor(mask))
        )
    }

    private func maskColor(_ mask: MaskUi) -> Color {
        mask.inverted
            ? Color(red: 0.35, green: 0.7, blue: 1.0)
            : Color(red: 1.0, green: 0.3, blue: 0.3)
    }
}

/// 蒙版面板：蒙版列表 + 形状参数 + 该蒙版的调色参数。
struct MaskPanelView: View {
    @ObservedObject var model: EditorModel

    var body: some View {
        VStack(spacing: 0) {
            chipRow
            subtabRow
            if let mask = model.selectedMask {
                switch model.maskTab {
                case .shape:
                    shapeControls(mask)
                case .adjust:
                    adjustControls
                }
            } else {
                Spacer()
                Text("点右上角「+」新建蒙版")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                Spacer()
            }
        }
    }

    // MARK: - 蒙版列表

    private var chipRow: some View {
        HStack(spacing: 8) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(model.state.masks) { mask in
                        Button {
                            model.selectMask(mask.id)
                        } label: {
                            HStack(spacing: 5) {
                                Image(systemName: mask.kind.icon)
                                    .font(.caption2)
                                Text(mask.name)
                                    .font(.caption2)
                            }
                            .padding(.horizontal, 10)
                            .padding(.vertical, 6)
                            .background(
                                Capsule().fill(
                                    model.selectedMaskId == mask.id
                                        ? Color.accentColor.opacity(0.35)
                                        : Color.white.opacity(0.08)
                                )
                            )
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.leading, 14)
            }
            Menu {
                ForEach(MaskKindUi.allCases) { kind in
                    Button {
                        model.addMask(kind)
                    } label: {
                        Label(kind.rawValue, systemImage: kind.icon)
                    }
                }
            } label: {
                Image(systemName: "plus")
                    .font(.footnote.weight(.semibold))
                    .padding(8)
                    .background(Circle().fill(Color.white.opacity(0.1)))
            }
            Button {
                model.deleteSelectedMask()
            } label: {
                Image(systemName: "trash")
                    .font(.footnote)
                    .padding(8)
                    .background(Circle().fill(Color.white.opacity(0.1)))
            }
            .disabled(model.selectedMask == nil)
            Button("完成") {
                model.selectTool(.edit)
            }
            .font(.caption)
            .foregroundStyle(Color.accentColor)
            .padding(.trailing, 12)
        }
        .padding(.top, 8)
    }

    private var subtabRow: some View {
        HStack(spacing: 18) {
            ForEach(MaskPanelTab.allCases) { tab in
                Button {
                    model.maskTab = tab
                } label: {
                    VStack(spacing: 3) {
                        Text(tab.rawValue)
                            .font(.caption)
                            .foregroundStyle(
                                model.maskTab == tab ? Color.white : Color.white.opacity(0.55)
                            )
                        Rectangle()
                            .fill(model.maskTab == tab ? Color.accentColor : Color.clear)
                            .frame(width: 26, height: 2)
                    }
                }
                .buttonStyle(.plain)
            }
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.top, 8)
    }

    // MARK: - 形状参数

    @ViewBuilder
    private func shapeControls(_ mask: MaskUi) -> some View {
        ScrollView {
            VStack(spacing: 0) {
                switch mask.kind {
                case .brush:
                    sliderRow(
                        label: "大小",
                        value: mask.brushSize,
                        range: 2...50,
                        neutral: 12,
                        step: 1,
                        keyPath: \.brushSize
                    )
                    sliderRow(
                        label: "笔刷羽化",
                        value: mask.brushFeather,
                        range: 0...100,
                        neutral: 50,
                        step: 5,
                        keyPath: \.brushFeather
                    )
                    sliderRow(
                        label: "流动",
                        value: mask.brushFlow,
                        range: 0...100,
                        neutral: 100,
                        step: 5,
                        keyPath: \.brushFlow
                    )
                    sliderRow(
                        label: "密度",
                        value: mask.brushDensity,
                        range: 0...100,
                        neutral: 100,
                        step: 5,
                        keyPath: \.brushDensity
                    )
                case .linear:
                    Text("在照片上拖动设置渐变方向，线条位置即渐变过渡。")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 18)
                        .padding(.vertical, 6)
                case .radial:
                    Text("在照片上从中心向外拖动调整范围。")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 18)
                        .padding(.vertical, 6)
                    sliderRow(
                        label: "内圈",
                        value: mask.inner,
                        range: 0...98,
                        neutral: 75,
                        step: 5,
                        keyPath: \.inner
                    )
                    sliderRow(
                        label: "旋转",
                        value: mask.rotation,
                        range: 0...360,
                        neutral: 0,
                        step: 15,
                        keyPath: \.rotation
                    )
                }
                sliderRow(
                    label: "蒙版羽化",
                    value: mask.feather,
                    range: 0...100,
                    neutral: 50,
                    step: 5,
                    keyPath: \.feather
                )
                sliderRow(
                    label: "不透明度",
                    value: mask.opacity,
                    range: 0...100,
                    neutral: 100,
                    step: 5,
                    keyPath: \.opacity
                )
                HStack {
                    Toggle("反转蒙版", isOn: Binding(
                        get: { model.selectedMask?.inverted ?? false },
                        set: { newValue in
                            model.applyEdit { state in
                                guard let id = model.selectedMaskId,
                                      let index = state.masks.firstIndex(where: { $0.id == id })
                                else {
                                    return
                                }
                                state.masks[index].inverted = newValue
                            }
                        }
                    ))
                    .font(.caption)
                    if mask.kind == .brush {
                        Spacer()
                        Button("清除笔画") {
                            model.applyEdit { state in
                                guard let id = model.selectedMaskId,
                                      let index = state.masks.firstIndex(where: { $0.id == id })
                                else {
                                    return
                                }
                                state.masks[index].strokes.removeAll()
                            }
                        }
                        .font(.caption)
                    }
                }
                .padding(.horizontal, 18)
                .padding(.vertical, 6)
            }
        }
    }

    /// 蒙版形状参数滑块（写入选中蒙版）。
    private func sliderRow(
        label: String,
        value: Float,
        range: ClosedRange<Float>,
        neutral: Float,
        step: Float,
        keyPath: WritableKeyPath<MaskUi, Float>
    ) -> some View {
        ValueSliderRow(
            label: label,
            value: value,
            range: range,
            neutral: neutral,
            step: step,
            decimals: 0,
            onBegin: { model.beginEdit() },
            onChange: { newValue in
                model.mutateSelectedMask { $0[keyPath: keyPath] = newValue }
                model.touchEdit()
            },
            onCommit: { model.commitEdit() }
        )
    }

    // MARK: - 该蒙版的调色参数

    private var adjustControls: some View {
        VStack(spacing: 0) {
            categoryRow
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(maskSliders) { spec in
                        let value = model.selectedMask?.values[spec.key] ?? spec.defaultValue
                        ValueSliderRow(
                            label: spec.label,
                            value: value,
                            range: spec.range,
                            neutral: spec.defaultValue,
                            step: step(for: spec),
                            decimals: 0,
                            onBegin: { model.beginEdit() },
                            onChange: { model.updateMaskValue(spec.key, $0) },
                            onCommit: { model.commitEdit() }
                        )
                    }
                }
                .padding(.vertical, 6)
            }
        }
    }

    private var maskSliders: [SliderSpec] {
        EditorSpec.visibleSliders(
            category: model.category,
            effectTab: model.effectTab,
            detailTab: model.detailTab
        ).filter { spec in
            spec.category != .detail
        }
    }

    private var categoryRow: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 12) {
                ForEach([EditCategory.light, .color, .effects]) { category in
                    Button {
                        model.category = category
                    } label: {
                        VStack(spacing: 5) {
                            Image(systemName: category.icon)
                                .font(.system(size: 15))
                            Text(category.rawValue)
                                .font(.caption2)
                        }
                        .frame(width: 56)
                        .padding(.vertical, 6)
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

    private func step(for spec: SliderSpec) -> Float {
        if spec.key == "exposure" {
            return 0.1
        }
        if spec.range.upperBound - spec.range.lowerBound <= 3 {
            return 0.05
        }
        return 5
    }
}
