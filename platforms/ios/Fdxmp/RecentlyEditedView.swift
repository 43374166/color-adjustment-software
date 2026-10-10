import Photos
import SwiftUI

/// 编辑 Tab 的首页：最近编辑过的照片（用于"下次进来还记得"）。
struct RecentlyEditedView: View {
    /// 打开编辑页：照片 + 整个最近编辑列表（编辑页内可左右滑动切上一张 / 下一张）。
    let onOpen: (PHAsset, [PHAsset]) -> Void

    @State private var entries: [RecentEntry] = []

    fileprivate struct RecentEntry: Identifiable {
        let asset: PHAsset
        let date: Date

        var id: String { asset.localIdentifier }
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            if entries.isEmpty {
                emptyState
            } else {
                list
            }
        }
        .background(Color(white: 0.08))
        .onAppear(perform: reload)
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
