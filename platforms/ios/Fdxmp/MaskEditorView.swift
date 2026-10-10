import SwiftUI

/// 蒙版面板：新建（线性 / 径向 / 画笔）+ 列表（缩略图 / 显示隐藏 / 复制 / 重命名 / 删除）
/// + 通用操作（反转 / 重置 / 显示叠加）+ 形状参数 + 该蒙版的局部调整。
struct MaskPanelView: View {
    @ObservedObject var model: EditorModel
    @State private var renameTargetId: String?
    @State private var renameText = ""

    var body: some View {
        VStack(spacing: 0) {
            headerRow
            chipRow
            actionRow
            subtabRow
            Divider()
                .overlay(Color.white.opacity(0.08))
            content
        }
        .alert("重命名蒙版", isPresented: renameAlertBinding) {
            TextField("名称", text: $renameText)
            Button("取消", role: .cancel) {
                renameTargetId = nil
            }
            Button("确定") {
                model.renameSelectedMask(renameText)
                renameTargetId = nil
            }
        }
    }

    private var renameAlertBinding: Binding<Bool> {
        Binding(
            get: { renameTargetId != nil },
            set: { if !$0 { renameTargetId = nil } }
        )
    }

    // MARK: - 顶部：标题 + 完成

    private var headerRow: some View {
        HStack {
            Text("蒙版")
                .font(.subheadline.weight(.semibold))
            if let mask = model.selectedMask {
                Text(mask.name)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer()
            Button("完成") {
                model.selectTool(.edit)
            }
            .font(.caption)
            .foregroundStyle(Color.accentColor)
        }
        .padding(.horizontal, 16)
        .padding(.top, 10)
    }

    // MARK: - 蒙版列表（横向卡片：缩略图 + 名称 + 显示 / 隐藏）

    private var chipRow: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 10) {
                ForEach(model.state.masks) { mask in
                    maskCard(mask)
                }
                addCard
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
        }
    }

    private func maskCard(_ mask: MaskUi) -> some View {
        let selected = model.selectedMaskId == mask.id
        return VStack(spacing: 3) {
            ZStack(alignment: .topTrailing) {
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color.black.opacity(0.5))
                    .frame(width: 54, height: 54)
                    .overlay(
                        Group {
                            if let thumbnail = model.maskThumbnails[mask.id] {
                                Image(uiImage: thumbnail)
                                    .resizable()
                                    .scaledToFit()
                                    .frame(width: 54, height: 54)
                                    .clipShape(RoundedRectangle(cornerRadius: 8))
                            } else {
                                Image(systemName: mask.kind.icon)
                                    .font(.system(size: 18))
                                    .foregroundStyle(.white.opacity(0.5))
                            }
                        }
                    )
                    .opacity(mask.visible ? 1 : 0.4)
                Button {
                    model.toggleMaskVisible(mask.id)
                } label: {
                    Image(systemName: mask.visible ? "eye" : "eye.slash")
                        .font(.system(size: 9, weight: .semibold))
                        .padding(3)
                        .background(Circle().fill(Color.black.opacity(0.65)))
                        .foregroundStyle(mask.visible ? Color.white : Color.white.opacity(0.55))
                }
                .buttonStyle(.plain)
                .offset(x: 4, y: -4)
            }
            Text(mask.name)
                .font(.system(size: 9))
                .lineLimit(1)
                .frame(width: 58)
                .foregroundStyle(selected ? Color.white : Color.white.opacity(0.65))
        }
        .padding(4)
        .background(
            RoundedRectangle(cornerRadius: 10)
                .fill(
                    selected
                        ? Color.accentColor.opacity(0.3)
                        : Color.white.opacity(0.06)
                )
        )
        .overlay(
            RoundedRectangle(cornerRadius: 10)
                .stroke(selected ? Color.accentColor : Color.white.opacity(0.08))
        )
        .contentShape(RoundedRectangle(cornerRadius: 10))
        .onTapGesture {
            model.selectMask(mask.id)
        }
        .contextMenu {
            Button {
                renameText = mask.name
                renameTargetId = mask.id
            } label: {
                Label("重命名", systemImage: "pencil")
            }
            Button {
                model.selectMask(mask.id)
                model.duplicateSelectedMask()
            } label: {
                Label("复制", systemImage: "plus.square.on.square")
            }
            Button(role: .destructive) {
                model.selectMask(mask.id)
                model.deleteSelectedMask()
            } label: {
                Label("删除", systemImage: "trash")
            }
        }
    }

    private var addCard: some View {
        Menu {
            ForEach(MaskKindUi.allCases) { kind in
                Button {
                    model.addMask(kind)
                } label: {
                    Label(kind.rawValue, systemImage: kind.icon)
                }
            }
        } label: {
            VStack(spacing: 3) {
                ZStack {
                    RoundedRectangle(cornerRadius: 8)
                        .fill(Color.white.opacity(0.08))
                        .frame(width: 54, height: 54)
                    Image(systemName: "plus")
                        .font(.system(size: 18))
                        .foregroundStyle(Color.accentColor)
                }
                Text("新建")
                    .font(.system(size: 9))
                    .foregroundStyle(.secondary)
            }
            .padding(4)
        }
    }

    // MARK: - 通用操作

    private var actionRow: some View {
        HStack(spacing: 0) {
            actionButton(
                "反转",
                icon: "circle.lefthalf.filled",
                enabled: model.selectedMask != nil,
                active: model.selectedMask?.inverted ?? false
            ) {
                model.invertSelectedMask()
            }
            actionButton(
                "重置",
                icon: "arrow.counterclockwise",
                enabled: model.selectedMask != nil
            ) {
                model.resetSelectedMask()
            }
            actionButton(
                "叠加",
                icon: "square.on.square",
                active: model.maskOverlayVisible
            ) {
                model.maskOverlayVisible.toggle()
            }
            actionButton(
                "复制",
                icon: "plus.square.on.square",
                enabled: model.selectedMask != nil
            ) {
                model.duplicateSelectedMask()
            }
            actionButton(
                "重命名",
                icon: "pencil",
                enabled: model.selectedMask != nil
            ) {
                if let mask = model.selectedMask {
                    renameText = mask.name
                    renameTargetId = mask.id
                }
            }
            actionButton(
                "删除",
                icon: "trash",
                enabled: model.selectedMask != nil
            ) {
                model.deleteSelectedMask()
            }
        }
        .padding(.horizontal, 8)
        .padding(.bottom, 4)
    }

    private func actionButton(
        _ title: String,
        icon: String,
        enabled: Bool = true,
        active: Bool = false,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            VStack(spacing: 2) {
                Image(systemName: icon)
                    .font(.system(size: 15))
                Text(title)
                    .font(.system(size: 9))
            }
            .frame(maxWidth: .infinity)
            .foregroundStyle(
                enabled
                    ? (active ? Color.accentColor : Color.white.opacity(0.85))
                    : Color.white.opacity(0.3)
            )
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
    }

    // MARK: - 子页：蒙版（形状）/ 调整（局部调色）

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
        .padding(.top, 4)
        .padding(.bottom, 4)
    }

    @ViewBuilder
    private var content: some View {
        if let mask = model.selectedMask {
            switch model.maskTab {
            case .shape:
                shapeControls(mask)
            case .adjust:
                adjustControls
            }
        } else {
            VStack(spacing: 8) {
                Spacer()
                Text("点「＋ 新建」添加蒙版：线性渐变 / 径向渐变 / 画笔")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
            }
        }
    }

    // MARK: - 形状参数

    private func shapeControls(_ mask: MaskUi) -> some View {
        ScrollView {
            VStack(spacing: 0) {
                switch mask.kind {
                case .linear:
                    linearControls(mask)
                case .radial:
                    radialControls(mask)
                case .brush:
                    brushControls(mask)
                }
            }
            .padding(.vertical, 4)
        }
    }

    @ViewBuilder
    private func linearControls(_ mask: MaskUi) -> some View {
        hintText("拖中心线平移 · 拖端点旋转 · 拖黄色手柄只移动那条过渡边界（两条线独立）")
        maskSlider(
            "位置",
            value: mask.linearPositionPercent,
            range: -100...100,
            neutral: 0,
            step: 2
        ) { mask, value in
            mask.settingLinearPosition(value)
        }
        maskSlider(
            "角度",
            value: mask.linearAngleWrapped,
            range: 0...180,
            neutral: 90,
            step: 1
        ) { mask, value in
            mask.settingLinearAngle(value)
        }
        maskSlider(
            "过渡",
            value: Float(mask.linearWidth * 100),
            range: 2...200,
            neutral: 30,
            step: 2
        ) { mask, value in
            mask.settingLinearWidth(Double(value) / 100)
        }
        opacitySlider(mask)
    }

    @ViewBuilder
    private func radialControls(_ mask: MaskUi) -> some View {
        hintText("拖白点缩放外圈（内圈跟着走一点）· 拖黄点直接调内圈羽化 · 拖蓝点旋转")
        maskSlider(
            "位置 X",
            value: Float(mask.center.x * 100),
            range: 0...100,
            neutral: 50,
            step: 1
        ) { mask, value in
            var copy = mask
            copy.center.x = CGFloat(value / 100)
            return copy
        }
        maskSlider(
            "位置 Y",
            value: Float(mask.center.y * 100),
            range: 0...100,
            neutral: 50,
            step: 1
        ) { mask, value in
            var copy = mask
            copy.center.y = CGFloat(value / 100)
            return copy
        }
        maskSlider(
            "半径 X",
            value: Float(mask.radii.width * 100),
            range: 4...150,
            neutral: 42,
            step: 1
        ) { mask, value in
            var copy = mask
            copy.radii.width = CGFloat(value / 100)
            return copy
        }
        maskSlider(
            "半径 Y",
            value: Float(mask.radii.height * 100),
            range: 4...150,
            neutral: 30,
            step: 1
        ) { mask, value in
            var copy = mask
            copy.radii.height = CGFloat(value / 100)
            return copy
        }
        maskSlider(
            "旋转",
            value: mask.rotationWrapped,
            range: 0...180,
            neutral: 0,
            step: 1
        ) { mask, value in
            var copy = mask
            copy.rotation = value
            return copy
        }
        maskSlider(
            "羽化",
            value: mask.radialFeather,
            range: 0...100,
            neutral: 30,
            step: 1
        ) { mask, value in
            var copy = mask
            copy.radialFeather = value
            return copy
        }
        opacitySlider(mask)
    }

    @ViewBuilder
    private func brushControls(_ mask: MaskUi) -> some View {
        hintText("在画面上单指涂抹添加蒙版；双指缩放 / 平移画布")
        Picker(
            "",
            selection: Binding(
                get: { model.brushErase },
                set: { model.brushErase = $0 }
            )
        ) {
            Text("画笔").tag(false)
            Text("擦除").tag(true)
        }
        .pickerStyle(.segmented)
        .padding(.horizontal, 16)
        .padding(.vertical, 4)
        maskSlider(
            "大小",
            value: mask.brushSize,
            range: 2...100,
            neutral: 30,
            step: 1
        ) { mask, value in
            var copy = mask
            copy.brushSize = value
            return copy
        }
        maskSlider(
            "羽化",
            value: mask.brushFeather,
            range: 0...100,
            neutral: 50,
            step: 1
        ) { mask, value in
            var copy = mask
            copy.brushFeather = value
            return copy
        }
        maskSlider(
            "流动",
            value: mask.brushFlow,
            range: 0...100,
            neutral: 100,
            step: 1
        ) { mask, value in
            var copy = mask
            copy.brushFlow = value
            return copy
        }
        maskSlider(
            "密度",
            value: mask.brushDensity,
            range: 0...100,
            neutral: 100,
            step: 1
        ) { mask, value in
            var copy = mask
            copy.brushDensity = value
            return copy
        }
        opacitySlider(mask)
        HStack(spacing: 14) {
            Text("参数只影响之后的笔画；已画的笔画保留落笔设置。")
                .font(.system(size: 10))
                .foregroundStyle(.secondary)
            Spacer()
            Button("撤销一笔") {
                model.undoLastStroke()
            }
            .font(.caption)
            .foregroundStyle(Color.accentColor)
            .disabled(mask.strokes.isEmpty)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 6)
    }

    private func opacitySlider(_ mask: MaskUi) -> some View {
        maskSlider(
            "不透明度",
            value: mask.opacity,
            range: 0...100,
            neutral: 100,
            step: 1
        ) { mask, value in
            var copy = mask
            copy.opacity = value
            return copy
        }
    }

    private func hintText(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 10))
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 18)
            .padding(.bottom, 2)
    }

    /// 蒙版形状参数滑块（拖动中实时渲染，松手记一步历史）。
    private func maskSlider(
        _ label: String,
        value: Float,
        range: ClosedRange<Float>,
        neutral: Float,
        step: Float,
        decimals: Int = 0,
        apply: @escaping (MaskUi, Float) -> MaskUi
    ) -> some View {
        ValueSliderRow(
            label: label,
            value: value,
            range: range,
            neutral: neutral,
            step: step,
            decimals: decimals,
            onBegin: { model.beginEdit() },
            onChange: { newValue in
                model.updateSelectedMask { $0 = apply($0, newValue) }
            },
            onCommit: { model.commitEdit() }
        )
    }

    // MARK: - 该蒙版的局部调整

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
                            decimals: spec.key == "exposure"
                                ? 2
                                : (spec.range.upperBound - spec.range.lowerBound <= 3 ? 1 : 0),
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
            .padding(.horizontal, 12)
            .padding(.top, 6)
        }
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
