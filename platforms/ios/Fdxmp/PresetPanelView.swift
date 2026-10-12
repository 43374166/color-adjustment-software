import Foundation
import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// 预设面板：系统预设 + 用户文件夹（父是文件夹、子是预设，竖向排列）。
///
/// - 「新建文件夹」自己建文件夹；「存为预设」把当前调色存进某个文件夹（可勾选包含哪些调整）；
/// - 除了系统预设（含系统文件夹），其余预设 / 文件夹长按都能删除；
/// - 支持导入 `.fdxmp` / Lightroom `.xmp`、导出 `.fdxmp`、复制 / 粘贴预设；
/// - 选中预设后，「强度」出现在该预设的缩略图上（在缩略图上左右拖动调整）。
struct PresetPanelView: View {
    @ObservedObject var model: EditorModel

    /// 收起的文件夹（默认全部展开；"system" = 系统预设）。
    @State private var collapsedFolders: Set<String> = []
    private static let systemFolderRef = "system"
    /// `.fdxmp` 文件类型（未注册时退回通用数据）。
    private static let fdxmpType = UTType(filenameExtension: "fdxmp") ?? .data
    /// XMP 文件类型。
    private static let xmpType = UTType(filenameExtension: "xmp") ?? .xml
    @State private var showSaveSheet = false
    @State private var showNewFolder = false
    @State private var newFolderName = ""
    @State private var pendingPresetDelete: UserPreset?
    @State private var pendingFolderDelete: PresetFolder?
    @State private var showFdxmpImporter = false
    @State private var showXmpImporter = false
    @State private var exportItem: ExportItem?
    /// 缩略图上拖动调强度：起始值 / 方向判定。
    @State private var strengthDragStart: Float?
    @State private var strengthDragAxis: Axis?

    /// 导出分享项（URL 不是 Identifiable，包一层）。
    private struct ExportItem: Identifiable {
        let id = UUID()
        let url: URL
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            presetList
        }
        .sheet(isPresented: $showSaveSheet) {
            SavePresetSheet(model: model) { folderId in
                collapsedFolders.remove(folderId)
            }
        }
        .sheet(item: $exportItem) { item in
            ShareSheet(items: [item.url])
        }
        .fileImporter(
            isPresented: $showFdxmpImporter,
            allowedContentTypes: [Self.fdxmpType, .data]
        ) { result in
            guard case let .success(url) = result else { return }
            importFdxmp(from: url)
        }
        .fileImporter(
            isPresented: $showXmpImporter,
            allowedContentTypes: [Self.xmpType, .xml]
        ) { result in
            guard case let .success(url) = result else { return }
            importXmp(from: url)
        }
        .alert("新建文件夹", isPresented: $showNewFolder) {
            TextField("文件夹名称", text: $newFolderName)
            Button("创建") {
                if let folder = model.createPresetFolder(name: newFolderName) {
                    Haptics.light()
                    collapsedFolders.remove(folder.id)
                }
                newFolderName = ""
            }
            .disabled(newFolderName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            Button("取消", role: .cancel) {
                newFolderName = ""
            }
        } message: {
            Text("预设会按文件夹分组显示，长按文件夹可以删除。")
        }
        .alert(
            "删除预设",
            isPresented: presetDeleteBinding,
            presenting: pendingPresetDelete
        ) { preset in
            Button("删除", role: .destructive) {
                model.deleteUserPreset(preset.id)
            }
            Button("取消", role: .cancel) {}
        } message: { preset in
            Text("「\(preset.name)」删除后不能恢复。")
        }
        .alert(
            "删除文件夹",
            isPresented: folderDeleteBinding,
            presenting: pendingFolderDelete
        ) { folder in
            Button("删除", role: .destructive) {
                model.deletePresetFolder(folder.id)
                collapsedFolders.remove(folder.id)
            }
            Button("取消", role: .cancel) {}
        } message: { folder in
            folderDeleteMessage(folder)
        }
    }

    private func folderDeleteMessage(_ folder: PresetFolder) -> Text {
        let count = model.userPresets.filter { $0.folderId == folder.id }.count
        if count > 0 {
            return Text("「\(folder.name)」里的 \(count) 个预设会一起删除，不能恢复。")
        }
        return Text("「\(folder.name)」是空文件夹，删除后不能恢复。")
    }

    private var presetDeleteBinding: Binding<Bool> {
        Binding(
            get: { pendingPresetDelete != nil },
            set: { if !$0 { pendingPresetDelete = nil } }
        )
    }

    /// 读取并导入 `.fdxmp` 文件。
    private func importFdxmp(from url: URL) {
        let accessed = url.startAccessingSecurityScopedResource()
        defer { if accessed { url.stopAccessingSecurityScopedResource() } }
        guard let data = try? Data(contentsOf: url) else {
            model.showMessage("导入失败：无法读取文件")
            return
        }
        if let preset = model.importPresetFile(data) {
            Haptics.light()
            collapsedFolders.remove(preset.folderId)
        }
    }

    /// 读取并导入 Lightroom `.xmp` 文件。
    private func importXmp(from url: URL) {
        let accessed = url.startAccessingSecurityScopedResource()
        defer { if accessed { url.stopAccessingSecurityScopedResource() } }
        guard let text = try? String(contentsOf: url, encoding: .utf8) else {
            model.showMessage("导入失败：无法读取 XMP")
            return
        }
        if let preset = model.importPresetXmp(text) {
            Haptics.light()
            collapsedFolders.remove(preset.folderId)
        }
    }

    private var folderDeleteBinding: Binding<Bool> {
        Binding(
            get: { pendingFolderDelete != nil },
            set: { if !$0 { pendingFolderDelete = nil } }
        )
    }

    // MARK: - 顶部操作

    private var header: some View {
        HStack(spacing: 14) {
            // 预设工具下底部 tabbar 隐藏：面板里留一个「完成」回到编辑。
            Button {
                Haptics.tick()
                model.selectTool(.edit)
            } label: {
                Text("完成")
                    .font(.footnote.weight(.semibold))
            }
            Button {
                showNewFolder = true
            } label: {
                Label("新建文件夹", systemImage: "folder.badge.plus")
            }
            Spacer(minLength: 0)
            Menu {
                Button {
                    showFdxmpImporter = true
                } label: {
                    Label("导入预设（.fdxmp）", systemImage: "square.and.arrow.down")
                }
                Button {
                    showXmpImporter = true
                } label: {
                    Label("导入 Lightroom XMP", systemImage: "square.and.arrow.down.on.square")
                }
                Button {
                    pastePreset()
                } label: {
                    Label("粘贴预设", systemImage: "doc.on.clipboard")
                }
            } label: {
                Label("导入", systemImage: "square.and.arrow.down")
            }
            Button {
                showSaveSheet = true
            } label: {
                Label("存为预设", systemImage: "plus.circle")
            }
        }
        .font(.footnote)
        .foregroundStyle(Color.accentColor)
        .padding(.horizontal, 16)
        .padding(.top, 10)
        .padding(.bottom, 6)
        // 拖动强度滑块时：操作行只隐藏不位移（滑块保持原位）。
        .opacity(model.draggingSlider == nil ? 1 : 0)
        .allowsHitTesting(model.draggingSlider == nil)
    }

    // MARK: - 预设树

    private var presetList: some View {
        ScrollView {
            LazyVStack(spacing: 0) {
                folderHeader(
                    ref: Self.systemFolderRef,
                    name: "系统预设",
                    count: EditorSpec.presets.count,
                    deletable: false
                )
                if !collapsedFolders.contains(Self.systemFolderRef) {
                    ForEach(EditorSpec.presets) { preset in
                        builtinRow(preset)
                    }
                }
                ForEach(model.presetFolders) { folder in
                    folderHeader(
                        ref: folder.id,
                        name: folder.name,
                        count: presets(in: folder).count,
                        deletable: true,
                        onDelete: { pendingFolderDelete = folder }
                    )
                    if !collapsedFolders.contains(folder.id) {
                        if presets(in: folder).isEmpty {
                            Text("还没有预设：点右上角「存为预设」把当前调色存进来")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(.leading, 44)
                                .padding(.trailing, 16)
                                .padding(.vertical, 8)
                        } else {
                            ForEach(presets(in: folder)) { preset in
                                userPresetRow(preset)
                            }
                        }
                    }
                }
                if model.presetFolders.isEmpty {
                    Text("点「新建文件夹」建一个自己的文件夹，再用「存为预设」保存当前调色。")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 18)
                        .padding(.vertical, 10)
                }
            }
            .padding(.bottom, 8)
        }
        .scrollDisabled(model.draggingSlider != nil)
        .opacity(model.draggingSlider == nil ? 1 : 0)
        .allowsHitTesting(model.draggingSlider == nil)
    }

    private func presets(in folder: PresetFolder) -> [UserPreset] {
        model.userPresets.filter { $0.folderId == folder.id }
    }

    private func folderHeader(
        ref: String,
        name: String,
        count: Int,
        deletable: Bool,
        onDelete: (() -> Void)? = nil
    ) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "chevron.right")
                .font(.system(size: 10, weight: .semibold))
                .foregroundStyle(.secondary)
                .rotationEffect(.degrees(collapsedFolders.contains(ref) ? 0 : 90))
            Image(systemName: "folder.fill")
                .font(.caption)
                .foregroundStyle(Color.accentColor.opacity(0.85))
            Text(name)
                .font(.footnote.weight(.medium))
            Text("\(count)")
                .font(.caption2)
                .foregroundStyle(.secondary)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 9)
        .contentShape(Rectangle())
        .onTapGesture {
            Haptics.light()
            withAnimation(.easeInOut(duration: 0.18)) {
                if collapsedFolders.contains(ref) {
                    collapsedFolders.remove(ref)
                } else {
                    collapsedFolders.insert(ref)
                }
            }
        }
        .contextMenu {
            if deletable, let onDelete {
                Button(role: .destructive, action: onDelete) {
                    Label("删除文件夹", systemImage: "trash")
                }
            } else {
                Button {} label: {
                    Label("系统预设不能删除", systemImage: "lock")
                }
                .disabled(true)
            }
        }
    }

    private func builtinRow(_ preset: BuiltinPreset) -> some View {
        presetRow(
            ref: preset.ref,
            name: preset.name,
            subtitle: preset.description,
            snapshot: model.presetSnapshot(preset),
            selected: model.presetRef == preset.ref,
            deletable: false,
            onDelete: nil,
            onTap: {
                // 已选中且正在调强度：轻点不重置（避免手一碰强度回到 100）。
                guard !(model.presetRef == preset.ref && model.canAdjustPresetStrength)
                else { return }
                Haptics.light()
                model.applyPreset(preset)
            }
        )
    }

    private func userPresetRow(_ preset: UserPreset) -> some View {
        presetRow(
            ref: preset.ref,
            name: preset.name,
            subtitle: preset.sectionSummary,
            snapshot: model.presetSnapshot(preset),
            selected: model.presetRef == preset.ref,
            deletable: true,
            onDelete: { pendingPresetDelete = preset },
            onTap: {
                guard !(model.presetRef == preset.ref && model.canAdjustPresetStrength)
                else { return }
                Haptics.light()
                model.applyUserPreset(preset)
            }
        )
    }

    private func presetRow(
        ref: String,
        name: String,
        subtitle: String,
        snapshot: EditState,
        selected: Bool,
        deletable: Bool,
        onDelete: (() -> Void)?,
        onTap: @escaping () -> Void
    ) -> some View {
        // 选中且可调强度：缩略图变成强度控件（点 / 拖缩略图调 0 = 应用前，100 = 预设原样）。
        let adjustable = selected && model.canAdjustPresetStrength
        return HStack(spacing: 10) {
            presetThumbnail(ref: ref, selected: selected, adjustable: adjustable)
            VStack(alignment: .leading, spacing: 2) {
                Text(name)
                    .font(.footnote)
                    .lineLimit(1)
                if adjustable {
                    Text("强度 \(Int(model.presetStrength.rounded()))")
                        .font(.caption2.monospacedDigit())
                        .foregroundStyle(Color.accentColor)
                } else {
                    Text(subtitle)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            Spacer(minLength: 0)
            if selected {
                Image(systemName: "checkmark")
                    .font(.caption)
                    .foregroundStyle(Color.accentColor)
            }
        }
        .padding(.leading, 44)
        .padding(.trailing, 16)
        .padding(.vertical, 8)
        .background(selected ? Color.white.opacity(0.07) : Color.clear)
        .contentShape(Rectangle())
        .onTapGesture(perform: onTap)
        .contextMenu {
            Button {
                model.copyPresetToPasteboard(name: name, state: snapshot)
            } label: {
                Label("复制预设", systemImage: "doc.on.doc")
            }
            Button {
                exportPreset(name: name, state: snapshot)
            } label: {
                Label("导出预设", systemImage: "square.and.arrow.up")
            }
            if deletable, let onDelete {
                Button(role: .destructive, action: onDelete) {
                    Label("删除预设", systemImage: "trash")
                }
            } else {
                Button {} label: {
                    Label("系统预设不能删除", systemImage: "lock")
                }
                .disabled(true)
            }
        }
    }

    /// 预设缩略图：选中后底部出现强度进度条，触摸 / 拖动即调整强度。
    @ViewBuilder
    private func presetThumbnail(
        ref: String,
        selected: Bool,
        adjustable: Bool
    ) -> some View {
        let content = ZStack(alignment: .bottom) {
            RoundedRectangle(cornerRadius: 6)
                .fill(Color.black.opacity(0.4))
            if let thumbnail = model.presetThumbnails[ref] {
                Image(uiImage: thumbnail)
                    .resizable()
                    .scaledToFill()
                    .frame(width: 64, height: 42)
            }
            if adjustable {
                // 底部强度进度条：白色填充 = 当前强度。
                ZStack(alignment: .leading) {
                    Rectangle().fill(Color.black.opacity(0.5))
                    Rectangle()
                        .fill(Color.white.opacity(0.95))
                        .frame(width: 64 * CGFloat(model.presetStrength / 100))
                }
                .frame(height: 4)
            }
        }
        .frame(width: 64, height: 42)
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .overlay(
            RoundedRectangle(cornerRadius: 6)
                .stroke(
                    selected ? Color.accentColor : Color.white.opacity(0.12),
                    lineWidth: selected ? 2 : 1
                )
        )
        .contentShape(Rectangle())
        Group {
            if adjustable {
                // 强度拖动用 simultaneousGesture：纵向拖动仍交给列表滚动，
                // 横向拖动 / 轻点才调强度（轻点行其它区域不会重置，见 onTap 保护）。
                content
                    .simultaneousGesture(strengthGesture())
            } else {
                content
            }
        }
    }

    /// 在缩略图上触摸 / 横向拖动调整预设强度（0..100）：
    /// - 横向拖动：以按下时的强度为起点，相对调整（纵向拖动仍交给列表滚动）；
    /// - 轻点：按点击位置直接设定强度（点右端 = 100）。
    private func strengthGesture() -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { value in
                if strengthDragStart == nil {
                    strengthDragStart = model.presetStrength
                    strengthDragAxis = nil
                }
                // 方向只在越过阈值时判定一次：横向 = 调强度，纵向 = 滚列表。
                if strengthDragAxis == nil {
                    let distance = hypot(value.translation.width, value.translation.height)
                    guard distance > 7 else { return }
                    strengthDragAxis = abs(value.translation.width) >= abs(value.translation.height)
                        ? Axis.horizontal
                        : Axis.vertical
                    if strengthDragAxis == .horizontal {
                        model.beginPresetStrengthEdit()
                        Haptics.light()
                    }
                }
                guard strengthDragAxis == .horizontal else { return }
                let next = (strengthDragStart ?? model.presetStrength)
                    + Float(value.translation.width / 1.2)
                model.updatePresetStrength(next.clamped(to: 0...100))
            }
            .onEnded { value in
                defer {
                    strengthDragStart = nil
                    strengthDragAxis = nil
                }
                guard strengthDragStart != nil else { return }
                switch strengthDragAxis {
                case .some(.horizontal):
                    model.commitPresetStrengthEdit()
                case .some(.vertical):
                    return
                case .none:
                    model.beginPresetStrengthEdit()
                    let ratio = Float(value.location.x / 64).clamped(to: 0...1)
                    model.updatePresetStrength(ratio * 100)
                    model.commitPresetStrengthEdit()
                }
            }
    }

    /// 粘贴剪贴板里的预设（没有可粘贴内容时给出提示）。
    private func pastePreset() {
        if let preset = model.pastePresetFromPasteboard() {
            Haptics.light()
            collapsedFolders.remove(preset.folderId)
        }
    }

    /// 导出预设为 `.fdxmp` 并弹出系统分享面板。
    private func exportPreset(name: String, state: EditState) {
        guard let url = model.exportPresetFile(name: name, state: state) else { return }
        Haptics.light()
        exportItem = ExportItem(url: url)
    }
}

/// 系统分享面板（导出预设文件用）。
private struct ShareSheet: UIViewControllerRepresentable {
    let items: [Any]

    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: items, applicationActivities: nil)
    }

    func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
}

/// 保存预设弹层：命名 + 选文件夹（可现场新建）+ 勾选包含的调整分组。
struct SavePresetSheet: View {
    @ObservedObject var model: EditorModel
    /// 保存成功后回调目标文件夹 id（面板据此展开该文件夹）。
    var onSaved: (String) -> Void = { _ in }

    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var folderId: String?
    @State private var sections: Set<PresetSection> = Set(PresetSection.allCases)
    @State private var creatingFolder = false
    @State private var newFolderName = ""

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider().overlay(Color.white.opacity(0.08))
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    nameSection
                    folderSection
                    sectionsSection
                }
                .padding(.horizontal, 18)
                .padding(.vertical, 14)
            }
            Divider().overlay(Color.white.opacity(0.08))
            saveButton
        }
        .presentationDetents([.medium, .large])
        .onAppear {
            if folderId == nil {
                folderId = model.presetFolders.first?.id
            }
        }
    }

    private var header: some View {
        HStack {
            Text("保存预设")
                .font(.subheadline.weight(.semibold))
            Spacer()
            Button("取消") {
                dismiss()
            }
            .font(.footnote)
            .foregroundStyle(Color.accentColor)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 12)
    }

    private var nameSection: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("名称")
                .font(.caption)
                .foregroundStyle(.secondary)
            TextField("例如：夏日蓝调", text: $name)
                .textFieldStyle(.roundedBorder)
                .font(.footnote)
        }
    }

    private var folderSection: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text("文件夹")
                .font(.caption)
                .foregroundStyle(.secondary)
            if model.presetFolders.isEmpty, !creatingFolder {
                Text("还没有文件夹，先新建一个。")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                    .padding(.bottom, 2)
            }
            ForEach(model.presetFolders) { folder in
                folderRow(folder)
            }
            newFolderRow
        }
    }

    private func folderRow(_ folder: PresetFolder) -> some View {
        Button {
            folderId = folder.id
        } label: {
            HStack(spacing: 8) {
                Image(systemName: folderId == folder.id ? "largecircle.fill.circle" : "circle")
                    .font(.footnote)
                    .foregroundStyle(folderId == folder.id ? Color.accentColor : Color.secondary)
                Text(folder.name)
                    .font(.footnote)
                Spacer(minLength: 0)
            }
            .padding(.vertical, 7)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    @ViewBuilder
    private var newFolderRow: some View {
        if creatingFolder {
            HStack(spacing: 8) {
                TextField("文件夹名称", text: $newFolderName)
                    .textFieldStyle(.roundedBorder)
                    .font(.footnote)
                Button("创建") {
                    createFolder()
                }
                .font(.footnote)
                .foregroundStyle(Color.accentColor)
                .disabled(newFolderName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
            .padding(.vertical, 4)
        } else {
            Button {
                creatingFolder = true
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: "folder.badge.plus")
                        .font(.footnote)
                        .foregroundStyle(Color.accentColor)
                    Text("新建文件夹")
                        .font(.footnote)
                        .foregroundStyle(Color.accentColor)
                    Spacer(minLength: 0)
                }
                .padding(.vertical, 7)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
    }

    private var sectionsSection: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text("包含调整")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer(minLength: 0)
                Button(allSelected ? "全不选" : "全选") {
                    if allSelected {
                        sections.removeAll()
                    } else {
                        sections = Set(PresetSection.allCases)
                    }
                }
                .font(.caption2)
                .foregroundStyle(Color.accentColor)
            }
            ForEach(PresetSection.allCases) { section in
                sectionRow(section)
            }
        }
    }

    private var allSelected: Bool {
        sections.count == PresetSection.allCases.count
    }

    private func sectionRow(_ section: PresetSection) -> some View {
        Button {
            Haptics.tick()
            if sections.contains(section) {
                sections.remove(section)
            } else {
                sections.insert(section)
            }
        } label: {
            HStack(spacing: 8) {
                Text(section.rawValue)
                    .font(.footnote)
                Spacer(minLength: 0)
                Image(systemName: sections.contains(section) ? "checkmark.circle.fill" : "circle")
                    .font(.footnote)
                    .foregroundStyle(
                        sections.contains(section) ? Color.accentColor : Color.secondary
                    )
            }
            .padding(.vertical, 7)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private var canSave: Bool {
        folderId != nil
            && !sections.isEmpty
            && !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private var saveButton: some View {
        Button {
            save()
        } label: {
            Text("保存")
                .font(.subheadline.weight(.semibold))
                .frame(maxWidth: .infinity)
                .padding(.vertical, 12)
                .background(
                    RoundedRectangle(cornerRadius: 10)
                        .fill(canSave ? Color.accentColor.opacity(0.9) : Color.white.opacity(0.08))
                )
                .foregroundStyle(canSave ? Color.white : Color.secondary)
        }
        .buttonStyle(.plain)
        .disabled(!canSave)
        .padding(.horizontal, 18)
        .padding(.vertical, 12)
    }

    private func createFolder() {
        guard let folder = model.createPresetFolder(name: newFolderName) else { return }
        Haptics.light()
        folderId = folder.id
        newFolderName = ""
        creatingFolder = false
    }

    private func save() {
        guard let folderId,
              let preset = model.saveUserPreset(
                name: name,
                folderId: folderId,
                sections: sections
              )
        else { return }
        Haptics.light()
        onSaved(preset.folderId)
        dismiss()
    }
}
