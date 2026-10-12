import Photos
import SwiftUI

/// 编辑 Tab 的首页：最近编辑过的照片（用于"下次进来还记得"）。
struct RecentlyEditedView: View {
    /// 打开编辑页：照片 + 整个最近编辑列表（编辑页内可左右滑动切上一张 / 下一张）。
    let onOpen: (PHAsset, [PHAsset]) -> Void
    /// 打开项目（照片队列 = 项目内照片）。
    var onOpenProject: (Project, [PHAsset]) -> Void = { _, _ in }

    @State private var entries: [RecentEntry] = []
    @State private var projects: [Project] = []
    @State private var showPicker = false
    @State private var renameTarget: Project?
    @State private var renameText = ""

    fileprivate struct RecentEntry: Identifiable {
        let asset: PHAsset
        let date: Date

        var id: String { asset.localIdentifier }
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            if entries.isEmpty && projects.isEmpty {
                emptyState
            } else {
                list
            }
        }
        .background(Color(white: 0.08))
        .onAppear(perform: reload)
        .sheet(isPresented: $showPicker) {
            PhotoPickerView { assets in
                showPicker = false
                let project = ProjectStore.create(assetIds: assets.map(\.localIdentifier))
                reload()
                onOpenProject(project, assets)
            }
            .ignoresSafeArea()
        }
        .alert("重命名项目", isPresented: renameAlertBinding) {
            TextField("名称", text: $renameText)
            Button("取消", role: .cancel) { renameTarget = nil }
            Button("确定") {
                if let target = renameTarget {
                    ProjectStore.rename(id: target.id, name: renameText)
                    reload()
                }
                renameTarget = nil
            }
        }
    }

    private var renameAlertBinding: Binding<Bool> {
        Binding(
            get: { renameTarget != nil },
            set: { if !$0 { renameTarget = nil } }
        )
    }

    private var header: some View {
        HStack {
            Text("最近编辑")
                .font(.headline)
            Spacer()
            if !entries.isEmpty {
                Text("\(entries.count) 张")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            // 新建项目：选本机照片 → 自动创建项目并进入。
            Button {
                Haptics.light()
                showPicker = true
            } label: {
                Image(systemName: "plus.circle.fill")
                    .font(.title3)
                    .foregroundStyle(Color.accentColor)
            }
            .accessibilityLabel("新建项目（选择本机照片）")
        }
        .padding(.horizontal, 16)
        .padding(.top, 14)
        .padding(.bottom, 8)
        .frame(maxWidth: .infinity)
        .background(Color(white: 0.11))
    }

    private var list: some View {
        ScrollView {
            LazyVStack(spacing: 8) {
                if !projects.isEmpty {
                    projectsSection
                }
                ForEach(entries) { entry in
                    Button {
                        onOpen(entry.asset, entries.map(\.asset))
                    } label: {
                        row(entry)
                    }
                    .buttonStyle(.plain)
                    .contextMenu {
                        Button(role: .destructive) {
                            EditStore.remove(assetId: entry.id)
                            reload()
                        } label: {
                            Label("删除编辑记录", systemImage: "trash")
                        }
                    }
                }
            }
            .padding(12)
        }
    }

    /// 项目列表：只显示项目里添加的那几张照片。
    private var projectsSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("项目")
                .font(.footnote.weight(.semibold))
                .foregroundStyle(.secondary)
                .padding(.leading, 2)
            ForEach(projects) { project in
                Button {
                    let assets = ProjectStore.assets(for: project)
                    guard !assets.isEmpty else { return }
                    onOpenProject(project, assets)
                } label: {
                    projectRow(project)
                }
                .buttonStyle(.plain)
                .contextMenu {
                    Button {
                        renameText = project.name
                        renameTarget = project
                    } label: {
                        Label("重命名", systemImage: "pencil")
                    }
                    Button(role: .destructive) {
                        ProjectStore.remove(id: project.id)
                        reload()
                    } label: {
                        Label("删除项目（照片保留在图库）", systemImage: "trash")
                    }
                }
            }
        }
    }

    private func projectRow(_ project: Project) -> some View {
        HStack(spacing: 12) {
            ZStack {
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color.accentColor.opacity(0.18))
                    .frame(width: 52, height: 52)
                Image(systemName: "rectangle.stack.badge.plus")
                    .font(.system(size: 18))
                    .foregroundStyle(Color.accentColor)
            }
            VStack(alignment: .leading, spacing: 3) {
                Text(project.name)
                    .font(.subheadline.weight(.medium))
                    .lineLimit(1)
                Text("\(project.assetIds.count) 张照片 · \(Self.formatter.string(from: project.createdAt))")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(10)
        .background(
            RoundedRectangle(cornerRadius: 10)
                .fill(Color.white.opacity(0.06))
        )
    }

    private func row(_ entry: RecentEntry) -> some View {
        HStack(spacing: 12) {
            RecentThumbnail(asset: entry.asset)
            VStack(alignment: .leading, spacing: 3) {
                Text(entry.asset.displayName)
                    .font(.subheadline)
                    .lineLimit(1)
                Text("编辑于 \(Self.formatter.string(from: entry.date))")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(10)
        .background(
            RoundedRectangle(cornerRadius: 10)
                .fill(Color.white.opacity(0.06))
        )
    }

    private var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "clock.arrow.circlepath")
                .font(.system(size: 44))
                .foregroundStyle(Color.accentColor)
            Text("还没有编辑过的照片")
                .font(.headline)
            Text("去「设备」Tab 点一张照片开始编辑，\n这里会记住你的编辑记录，下次直接继续。")
                .font(.footnote)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(32)
    }

    private func reload() {
        projects = ProjectStore.load()
        let recent = EditStore.recentEdits(limit: 30)
        let ids = recent.map(\.assetId)
        guard !ids.isEmpty else {
            entries = []
            return
        }
        let result = PHAsset.fetchAssets(withLocalIdentifiers: ids, options: nil)
        var assets: [String: PHAsset] = [:]
        result.enumerateObjects { asset, _, _ in
            assets[asset.localIdentifier] = asset
        }
        entries = recent.compactMap { item in
            guard let asset = assets[item.assetId] else { return nil }
            return RecentEntry(asset: asset, date: item.date)
        }
    }

    private static let formatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "MM-dd HH:mm"
        return formatter
    }()
}

/// 最近编辑列表的缩略图（不依赖设备页的 PhotoLibraryModel）。
private struct RecentThumbnail: View {
    let asset: PHAsset

    @State private var image: UIImage?
    private static let manager = PHCachingImageManager()

    var body: some View {
        Color(white: 0.14)
            .overlay {
                if let image {
                    Image(uiImage: image)
                        .resizable()
                        .scaledToFill()
                } else {
                    Image(systemName: "photo")
                        .foregroundStyle(.secondary)
                }
            }
            .frame(width: 60, height: 60)
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .onAppear(perform: load)
    }

    private func load() {
        let options = PHImageRequestOptions()
        options.deliveryMode = .fastFormat
        options.resizeMode = .fast
        options.isNetworkAccessAllowed = true
        Self.manager.requestImage(
            for: asset,
            targetSize: CGSize(width: 160, height: 160),
            contentMode: .aspectFill,
            options: options
        ) { result, _ in
            guard let result else { return }
            DispatchQueue.main.async {
                image = result
            }
        }
    }
}
