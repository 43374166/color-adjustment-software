import Photos
import SwiftUI

/// 编辑 Tab 的首页：最近编辑过的照片（用于"下次进来还记得"）。
struct RecentlyEditedView: View {
    /// 打开编辑页：照片 + 整个最近编辑列表（编辑页内可左右滑动切上一张 / 下一张）。
    let onOpen: (PHAsset, [PHAsset]) -> Void
    /// 打开项目（照片队列 = 项目内照片；startAt = 从哪张开始）。
    var onOpenProject: (Project, [PHAsset], PHAsset) -> Void = { _, _, _ in }

    @State private var entries: [RecentEntry] = []
    @State private var projects: [Project] = []
    /// 展开的项目（点项目行展开 / 收起，展开后显示项目内照片）。
    @State private var expandedProjects: Set<String> = []
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
                guard let first = assets.first else { return }
                let project = ProjectStore.create(assetIds: assets.map(\.localIdentifier))
                reload()
                onOpenProject(project, assets, first)
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
                let expanded = expandedProjects.contains(project.id)
                let assets = ProjectStore.assets(for: project)
                Button {
                    // 点项目行：展开 / 收起（看照片）；再点照片进入项目编辑。
                    Haptics.tick()
                    withAnimation(.easeInOut(duration: 0.22)) {
                        if expanded {
                            expandedProjects.remove(project.id)
                        } else {
                            expandedProjects.insert(project.id)
                        }
                    }
                } label: {
                    projectRow(project, expanded: expanded, assetCount: assets.count)
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
                if expanded {
                    // 项目内的照片比项目缩进一点（层级关系一眼可见）。
                    ForEach(assets, id: \.localIdentifier) { photo in
                        Button {
                            onOpenProject(project, assets, photo)
                        } label: {
                            projectPhotoRow(photo)
                        }
                        .buttonStyle(.plain)
                        .padding(.leading, 22)
                    }
                    if assets.isEmpty {
                        Text("项目里的照片已从图库移除")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .padding(.leading, 24)
                    } else if project.assetIds.count > assets.count {
                        Text("\(project.assetIds.count - assets.count) 张照片不可用（可能已从图库删除或权限受限）")
                            .font(.caption2)
                            .foregroundStyle(Color.orange.opacity(0.9))
                            .padding(.leading, 24)
                    }
                }
            }
        }
    }

    private func projectRow(_ project: Project, expanded: Bool, assetCount: Int) -> some View {
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
                Text("\(assetCount) 张照片 · \(Self.formatter.string(from: project.createdAt))")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            // 箭头随展开 / 收起旋转。
            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
                .rotationEffect(.degrees(expanded ? 90 : 0))
        }
        .padding(10)
        .background(
            RoundedRectangle(cornerRadius: 10)
                .fill(Color.white.opacity(0.06))
        )
        .animation(.easeInOut(duration: 0.22), value: expanded)
    }

    /// 项目内的照片行（缩进显示，点一下直接从这张进项目编辑）。
    private func projectPhotoRow(_ asset: PHAsset) -> some View {
        HStack(spacing: 10) {
            RecentThumbnail(asset: asset)
                .frame(width: 44, height: 44)
            Text(asset.displayName)
                .font(.footnote)
                .lineLimit(1)
            Spacer()
            Image(systemName: "chevron.right")
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 4)
        .padding(.horizontal, 10)
        .background(
            RoundedRectangle(cornerRadius: 8)
                .fill(Color.white.opacity(0.04))
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
        // 项目里的照片挂在项目下面（展开可见），不再出现在「最近编辑」顶层——
        // 避免"项目内编辑的照片和项目同级"的显示。
        let projectAssetIds = Set(projects.flatMap(\.assetIds))
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
            guard !projectAssetIds.contains(item.assetId) else { return nil }
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
struct RecentThumbnail: View {
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
