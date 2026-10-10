import SwiftUI

/// 蒙版面板：新建（线性 / 径向 / 画笔）+ 列表（缩略图 / 显示隐藏 / 复制 / 重命名 / 删除）
/// + 通用操作（反转 / 重置 / 显示叠加）+ 形状参数 + 该蒙版的局部调整。
struct MaskPanelView: View {
    @ObservedObject var model: EditorModel
    @State private var renameTargetId: String?
    @State private var renameText = ""
    /// 蒙版设置弹层对应的蒙版 id。
    @State private var settingsMaskId: String?
    /// 长按分类预览后，短时间内忽略这次点击。
    @State private var suppressCategoryTapUntil = Date.distantPast

    var body: some View {
        VStack(spacing: 0) {
            content
            Divider()
                .overlay(Color.white.opacity(0.08))
                .opacity(model.draggingSlider == nil ? 1 : 0)
            listRow
                .opacity(model.draggingSlider == nil ? 1 : 0)
                .allowsHitTesting(model.draggingSlider == nil)
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
        // 长按蒙版卡片：蒙版设置（羽化 / 不透明度 / 反转 / 叠加色 / 形状参数 / 复制删除等）。
        .sheet(isPresented: settingsBinding) {
            settingsSheet
        }
    }

    private var settingsBinding: Binding<Bool> {
        Binding(
            get: { settingsMaskId != nil },
            set: { if !$0 { settingsMaskId = nil } }
        )
    }

    private var renameAlertBinding: Binding<Bool> {
        Binding(
            get: { renameTargetId != nil },
            set: { if !$0 { renameTargetId = nil } }
        )
    }

    // MARK: - 最下：蒙版列表（缩略图 + 显示 / 隐藏）+ 新建 + 完成

    private var listRow: some View {
        HStack(spacing: 4) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 10) {
                    ForEach(model.state.masks) { mask in
                        maskCard(mask)
                    }
                    addCard
                }
                .padding(.leading, 14)
                .padding(.vertical, 6)
            }
            Button("完成") {
                model.selectTool(.edit)
            }
            .font(.subheadline.weight(.semibold))
            .foregroundStyle(Color.accentColor)
            .padding(.horizontal, 12)
        }
    }

    // MARK: - 蒙版设置弹层（长按卡片打开）

    @ViewBuilder
    private var settingsSheet: some View {
        if let mask = model.selectedMask {
            VStack(spacing: 0) {
                HStack {
                    Text("蒙版设置 · \(mask.name)")
                        .font(.subheadline.weight(.semibold))
                        .lineLimit(1)
                    Spacer()
                    Button("完成") {
                        settingsMaskId = nil
                    }
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Color.accentColor)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                Divider().overlay(Color.white.opacity(0.08))
                ScrollView {
                    VStack(spacing: 0) {
                        HStack(spacing: 14) {
                            Toggle("反转", isOn: Binding(
                                get: { mask.inverted },
                                set: { newValue in
                                    model.applyEdit("反转蒙版") { state in
                                        guard let index = state.masks.firstIndex(where: { $0.id == mask.id })
                                        else { return }
                                        state.masks[index].inverted = newValue
                                    }
                                }
                            ))
                            .font(.footnote)
                            .tint(Color.accentColor)
                            Toggle("显示叠加", isOn: Binding(
                                get: { model.maskOverlayVisible },
                                set: { model.maskOverlayVisible = $0 }
                            ))
                            .font(.footnote)
                            .tint(Color.accentColor)
                        }
                        .padding(.horizontal, 16)
                        .padding(.vertical, 6)
                        // 叠加颜色
                        HStack(spacing: 14) {
                            Text("叠加色")
                                .font(.footnote)
                            Spacer()
                            ForEach(MaskOverlayColor.allCases) { overlay in
                                Button {
                                    Haptics.tick()
                                    model.applyEdit("叠加颜色") { state in
                                        guard let index = state.masks.firstIndex(where: { $0.id == mask.id })
                                        else { return }
                                        state.masks[index].overlay = overlay
                                    }
                                } label: {
                                    Circle()
                                        .fill(overlayTint(overlay))
                                        .frame(width: 22, height: 22)
                                        .overlay(
                                            Circle().stroke(
                                                mask.overlay == overlay ? Color.white : Color.white.opacity(0.25),
                                                lineWidth: mask.overlay == overlay ? 2.5 : 1
                                            )
                                        )
                                }
                                .buttonStyle(.plain)
                            }
                        }
                        .padding(.horizontal, 16)
                        .padding(.vertical, 6)
                        // 形状参数（按类型）+ 蒙版羽化 / 不透明度
                        shapeControlRows(mask)
                        opacitySlider(mask)
                        maskSlider(
                            "蒙版羽化",
                            value: mask.feather,
                            range: 0...100,
                            neutral: 0,
                            step: 1
                        ) { target, value in
                            var copy = target
                            copy.feather = value
                            return copy
                        }
                        HStack(spacing: 13) {
                            actionButton(
                                "复制设置",
                                icon: "doc.on.doc"
                            ) {
                                Haptics.light()
                                model.copySelectedMaskSettings()
                            }
                            actionButton(
                                "粘贴设置",
                                icon: "doc.on.clipboard",
                                enabled: model.canPasteMaskSettings
                            ) {
                                Haptics.light()
                                model.pasteMaskSettings()
                            }
                            actionButton("重命名", icon: "pencil") {
                                renameText = mask.name
                                renameTargetId = mask.id
                            }
                            actionButton("复制蒙版", icon: "plus.square.on.square") {
                                model.duplicateSelectedMask()
                                settingsMaskId = nil
                            }
                            actionButton("重置", icon: "arrow.counterclockwise") {
                                model.resetSelectedMask()
                            }
                            actionButton("删除", icon: "trash") {
                                model.deleteSelectedMask()
                                settingsMaskId = nil
                            }
                        }
                        .padding(.horizontal, 8)
                        .padding(.vertical, 8)
                    }
                }
            }
            .presentationDetents([.medium, .large])
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
        // 长按卡片：打开蒙版设置（羽化 / 不透明度 / 反转 / 形状参数 / 复制删除等）。
        .onLongPressGesture(minimumDuration: 0.4) {
            Haptics.light()
            model.selectMask(mask.id)
            renameText = mask.name
            settingsMaskId = mask.id
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
            Menu {
                ForEach(MaskOverlayColor.allCases) { overlay in
                    Button {
                        Haptics.tick()
                        model.applyEdit("叠加颜色") { state in
                            guard let id = model.selectedMaskId,
                                  let index = state.masks.firstIndex(where: { $0.id == id })
                            else { return }
                            state.masks[index].overlay = overlay
                        }
                    } label: {
                        HStack {
                            Text(overlay.rawValue)
                            if model.selectedMask?.overlay == overlay {
                                Image(systemName: "checkmark")
                            }
                        }
                    }
                }
            } label: {
                VStack(spacing: 2) {
                    Circle()
                        .fill(overlayTint(model.selectedMask?.overlay ?? .red))
                        .frame(width: 15, height: 15)
                        .overlay(Circle().stroke(Color.white.opacity(0.35), lineWidth: 1))
                    Text("叠加色")
                        .font(.system(size: 9))
                }
                .frame(maxWidth: .infinity)
                .foregroundStyle(model.selectedMask == nil ? Color.white.opacity(0.3) : Color.white.opacity(0.85))
            }
            .disabled(model.selectedMask == nil)
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

    /// 叠加颜色 → 色块。
    private func overlayTint(_ overlay: MaskOverlayColor) -> Color {
        switch overlay {
        case .red: return Color(red: 1.0, green: 0.25, blue: 0.25)
        case .green: return Color(red: 0.3, green: 0.85, blue: 0.4)
        case .blue: return Color(red: 0.3, green: 0.55, blue: 1.0)
        case .white: return Color.white
        case .black: return Color(white: 0.25)
        }
    }

    // MARK: - 子页：蒙版（形状）/ 调整（局部调色）

    private var subtabRow: some View {
        HStack(spacing: 18) {
            ForEach(MaskPanelTab.allCases) { tab in
                Button {
                    model.setMaskTab(tab)
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
            if let mask = model.selectedMask {
                Text(mask.name)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .padding(.horizontal, 16)
        .padding(.top, 10)
        .padding(.bottom, 4)
    }

    @ViewBuilder
    private var content: some View {
        if let mask = model.selectedMask {
            // 面板只留调整内容 + 不透明度；形状设置在长按卡片的蒙版设置里。
            VStack(spacing: 0) {
                opacitySlider(mask)
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
            shapeControlRows(mask)
        }
    }

    /// 形状参数行（不含滚动容器；设置弹层里直接嵌这段）。
    private func shapeControlRows(_ mask: MaskUi) -> some View {
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
        .opacity(model.draggingSlider == nil ? 1 : 0)
        .allowsHitTesting(model.draggingSlider == nil)
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
        .opacity(model.draggingSlider == nil ? 1 : 0)
        .allowsHitTesting(model.draggingSlider == nil)
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

    @ViewBuilder
    private func hintText(_ text: String) -> some View {
        // 拖动滑块时只留滑块（提示文字隐去但保留高度，滑块位置不跳）。
        Text(text)
            .font(.system(size: 10))
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 18)
            .padding(.bottom, 2)
            .opacity(model.draggingSlider == nil ? 1 : 0)
    }

    /// 蒙版形状参数滑块（拖动中实时渲染，松手记一步历史）。
    @ViewBuilder
    private func maskSlider(
        _ label: String,
        value: Float,
        range: ClosedRange<Float>,
        neutral: Float,
        step: Float,
        decimals: Int = 0,
        apply: @escaping (MaskUi, Float) -> MaskUi
    ) -> some View {
        // 拖动某个滑块时只保留它自己（Lightroom 式沉浸）。
        if model.sliderVisible(label) {
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
    }

    // MARK: - 该蒙版的局部调整

    private var adjustControls: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(maskSliders.filter { model.sliderVisible($0.label) }) { spec in
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
            Divider()
                .overlay(Color.white.opacity(0.08))
                .opacity(model.draggingSlider == nil ? 1 : 0)
            categoryRow
                .opacity(model.draggingSlider == nil ? 1 : 0)
                .allowsHitTesting(model.draggingSlider == nil)
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
                        guard Date() >= suppressCategoryTapUntil else { return }
                        model.setCategory(category)
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
                    .simultaneousGesture(categoryHoldGesture(category))
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
        }
    }

    /// 长按分类（0.5s）：临时把该蒙版这一类的局部参数归零，松手恢复（对比用）。
    private func categoryHoldGesture(_ category: EditCategory) -> some Gesture {
        LongPressGesture(minimumDuration: 0.5, maximumDistance: 40)
            .sequenced(before: DragGesture(minimumDistance: 0))
            .onChanged { value in
                if case .second(true, _) = value {
                    if model.previewMaskCategory != category {
                        Haptics.medium()
                        model.beginMaskCategoryPreview(category)
                    }
                    suppressCategoryTapUntil = Date().addingTimeInterval(0.4)
                }
            }
            .onEnded { _ in
                model.endMaskCategoryPreview()
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
