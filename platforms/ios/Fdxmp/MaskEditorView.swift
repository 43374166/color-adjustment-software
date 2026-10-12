import Foundation
import SwiftUI

/// 蒙版面板：新建（线性 / 径向 / 画笔）+ 列表（缩略图 / 显示隐藏）。
///
/// 调整页（创建蒙版后才从底部弹起）：该蒙版的局部调色 + 子 nav
/// （反转 / 复制 / 重置 / 重命名 / 删除，就在蒙版列表上面）。
/// 长按蒙版卡片打开「蒙版设置」：显示叠加 / 叠加色（默认红色）+ 形状参数 +
/// 蒙版羽化 + 不透明度 + 复制 / 粘贴设置。
struct MaskPanelView: View {
    @ObservedObject var model: EditorModel
    @State private var renameTargetId: String?
    @State private var renameText = ""
    /// 蒙版设置弹层对应的蒙版 id。
    @State private var settingsMaskId: String?
    /// 长按分类对比：按住期间 + 松手后一小段时间都吞掉这次点击，避免顺带切换分类。
    @State private var categoryHoldActive = false
    @State private var categorySuppressTapUntil = Date.distantPast

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
        // 长按蒙版卡片：打开「蒙版设置」（叠加 + 形状参数 + 羽化 / 不透明度 + 复制粘贴设置）。
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
                        // 显示叠加 + 叠加色（默认红色）
                        HStack(spacing: 14) {
                            Button {
                                Haptics.tick()
                                model.maskOverlayVisible.toggle()
                            } label: {
                                HStack(spacing: 6) {
                                    Image(systemName: model.maskOverlayVisible ? "eye" : "eye.slash")
                                        .font(.footnote)
                                    Text("显示叠加")
                                        .font(.footnote)
                                }
                                .foregroundStyle(
                                    model.maskOverlayVisible
                                        ? Color.accentColor
                                        : Color.white.opacity(0.85)
                                )
                            }
                            .buttonStyle(.plain)
                            Spacer(minLength: 0)
                            Text("叠加色")
                                .font(.footnote)
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
                                                mask.overlay == overlay
                                                    ? Color.white
                                                    : Color.white.opacity(0.25),
                                                lineWidth: mask.overlay == overlay ? 2.5 : 1
                                            )
                                        )
                                }
                                .buttonStyle(.plain)
                            }
                        }
                        .padding(.horizontal, 16)
                        .padding(.vertical, 6)
                        // 形状参数（按类型）
                        shapeControlRows(mask)
                        // 蒙版羽化（整体软边）
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
                        // 不透明度（从面板挪进设置里）
                        opacitySlider(mask)
                        // 复制 / 粘贴设置留在设置里；其余操作都在面板的子 nav 上。
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
                        }
                        .padding(.horizontal, 8)
                        .padding(.vertical, 8)
                    }
                }
                // 拖动滑块中：禁止弹层上下滚动（只调值）。
                .scrollDisabled(model.draggingSlider != nil)
            }
            .presentationDetents([.medium, .large])
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
        // 长按卡片：只打开「蒙版设置」（形状参数 / 不透明度 / 复制粘贴设置）。
        .onLongPressGesture(minimumDuration: 0.4) {
            Haptics.light()
            model.selectMask(mask.id)
            settingsMaskId = mask.id
        }
    }

    private var addCard: some View {
        Menu {
            Section("几何蒙版") {
                ForEach(MaskKindUi.allCases.filter { !$0.isSmart }) { kind in
                    Button {
                        model.addMask(kind)
                    } label: {
                        Label(kind.rawValue, systemImage: kind.icon)
                    }
                }
            }
            Section("智能蒙版（端侧识别）") {
                ForEach(MaskKindUi.allCases.filter { $0.isSmart }) { kind in
                    Button {
                        model.addMask(kind)
                    } label: {
                        Label(kind.rawValue, systemImage: kind.icon)
                    }
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

    // MARK: - 子 nav（放在蒙版列表上面）：反转 / 复制 / 重置 / 重命名 / 删除

    private var actionRow: some View {
        HStack(spacing: 0) {
            actionButton(
                "反转",
                icon: "circle.lefthalf.filled",
                active: model.selectedMask?.inverted ?? false
            ) {
                model.invertSelectedMask()
            }
            actionButton("复制", icon: "plus.square.on.square") {
                model.duplicateSelectedMask()
            }
            actionButton("重置", icon: "arrow.counterclockwise") {
                model.resetSelectedMask()
            }
            actionButton("重命名", icon: "pencil") {
                if let mask = model.selectedMask {
                    renameText = mask.name
                    renameTargetId = mask.id
                }
            }
            actionButton("删除", icon: "trash") {
                model.deleteSelectedMask()
            }
        }
        .padding(.horizontal, 8)
        .padding(.top, 4)
        .padding(.bottom, 2)
        // 拖动滑块时：只隐藏不位移（滑块保持原位）。
        .opacity(model.draggingSlider == nil ? 1 : 0)
        .allowsHitTesting(model.draggingSlider == nil)
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

    @ViewBuilder
    private var content: some View {
        if let mask = model.selectedMask {
            // 调整页（创建蒙版后才从底部弹起）：
            // 局部调色 → 子 nav（反转 / 复制 / 重置 / 重命名 / 删除）。
            // 羽化 / 叠加色 / 不透明度这些都在长按卡片的「蒙版设置」里。
            VStack(spacing: 0) {
                if mask.kind.isSmart {
                    smartStatusRow(mask)
                }
                adjustControls
                actionRow
            }
        } else {
            // 还没有蒙版：只留一句提示 + 新建行，创建后再弹起调整页。
            Text("点「＋ 新建」添加蒙版：线性渐变 / 径向渐变 / 画笔")
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 16)
        }
    }

    /// 智能蒙版状态行：生成中 / 失败重试 / 已生成提示。
    @ViewBuilder
    private func smartStatusRow(_ mask: MaskUi) -> some View {
        HStack(spacing: 8) {
            if model.smartMaskGenerating.contains(mask.id) {
                ProgressView()
                    .controlSize(.mini)
                Text("正在端侧识别\(mask.kind.rawValue)…")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else if let failure = model.smartMaskFailed[mask.id] {
                Image(systemName: "exclamationmark.triangle")
                    .font(.caption)
                    .foregroundStyle(.orange)
                Text(failure)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                Spacer(minLength: 0)
                Button("重新生成") {
                    Haptics.light()
                    model.retrySmartMask(mask.id)
                }
                .font(.caption.weight(.semibold))
                .foregroundStyle(Color.accentColor)
            } else {
                Image(systemName: "wand.and.stars")
                    .font(.caption)
                    .foregroundStyle(Color.accentColor)
                Text("已生成（端侧识别）：可调局部参数；长按卡片进「蒙版设置」调羽化 / 不透明度")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 4)
        .opacity(model.draggingSlider == nil ? 1 : 0)
        .allowsHitTesting(model.draggingSlider == nil)
    }

    // MARK: - 形状参数

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
            case .aiSubject, .aiPortrait, .aiFace, .aiSkin,
                .aiEyes, .aiEyebrows, .aiNose, .aiLips, .aiTeeth:
                hintText("智能蒙版由端侧识别生成（离线）；长按卡片可在「蒙版设置」里调羽化 / 不透明度")
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
            "内圈羽化",
            value: mask.radialFeather,
            range: 0...100,
            neutral: 30,
            step: 1
        ) { mask, value in
            var copy = mask
            copy.radialFeather = value
            return copy
        }
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
            "笔刷羽化",
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
        // 拖动某个滑块时其余只隐藏不位移（Lightroom 式沉浸，滑块保持原位）。
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
        .opacity(model.sliderVisible(label) ? 1 : 0)
        .allowsHitTesting(model.sliderVisible(label))
    }

    // MARK: - 该蒙版的局部调整

    private var adjustControls: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(spacing: 0) {
                    // 拖动某个滑块时其余只隐藏不位移（滑块保持原位）。
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
                        .opacity(model.sliderVisible(spec.label) ? 1 : 0)
                        .allowsHitTesting(model.sliderVisible(spec.label))
                    }
                }
                .padding(.vertical, 6)
            }
            // 拖动滑块中：禁止面板上下滚动（只调值）。
            .scrollDisabled(model.draggingSlider != nil)
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
                    categoryButton(category)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 3)
        }
    }

    /// 分类按钮（自己判定触摸）：轻点切换分类，长按只做"这一类参数复原"对比。
    private func categoryButton(_ category: EditCategory) -> some View {
        Button {
            // 长按对比松手时 Button 也会触发一次点击：这里吞掉，别顺带切换分类。
            // （两种回调顺序都兜住；即使手势回调缺失，下一次点按也能自救。）
            if categoryHoldActive || Date() < categorySuppressTapUntil {
                categoryHoldActive = false
                return
            }
            Haptics.tick()
            model.setCategory(category)
        } label: {
            VStack(spacing: 3) {
                Image(systemName: category.icon)
                    .font(.system(size: 13))
                Text(category.rawValue)
                    .font(.caption2)
            }
            .frame(width: 54)
            .padding(.vertical, 3)
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
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .simultaneousGesture(categoryHoldGesture(category))
    }

    /// 长按分类（0.5s）：临时把该蒙版这一类的局部参数归零，松手恢复（对比用）。
    /// 用系统长按识别 + 串接拖动；按住期间和松手后的短暂窗口都吞掉点击。
    private func categoryHoldGesture(_ category: EditCategory) -> some Gesture {
        LongPressGesture(minimumDuration: 0.5, maximumDistance: 40)
            .sequenced(before: DragGesture(minimumDistance: 0))
            .onChanged { value in
                if case .second(true, _) = value {
                    guard !categoryHoldActive else { return }
                    categoryHoldActive = true
                    Haptics.medium()
                    model.beginMaskCategoryPreview(category)
                }
            }
            .onEnded { _ in
                // 轻点（长按没成立）时不做任何事，别把这次正常点按吞掉。
                guard categoryHoldActive else { return }
                categoryHoldActive = false
                categorySuppressTapUntil = Date().addingTimeInterval(0.4)
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
