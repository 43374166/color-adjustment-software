import Foundation
import Photos
import PhotosUI
import SwiftUI

/// 一个项目：一组被添加进来的照片（编辑 Tab 的「＋」创建）。
struct Project: Identifiable, Codable, Equatable {
    var id: String
    var name: String
    /// 项目内的照片（PHAsset localIdentifier，按添加顺序）。
    var assetIds: [String]
    var createdAt: Date
}

/// 项目持久化（Application Support/fdxmp/projects.json）。
enum ProjectStore {
    private static var fileURL: URL {
        let base = FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first ?? FileManager.default.temporaryDirectory
        let dir = base.appendingPathComponent("fdxmp", isDirectory: true)
        if !FileManager.default.fileExists(atPath: dir.path) {
            try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        }
        return dir.appendingPathComponent("projects.json")
    }

    static func load() -> [Project] {
        guard let data = try? Data(contentsOf: fileURL) else { return [] }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return (try? decoder.decode([Project].self, from: data)) ?? []
    }

    @discardableResult
    static func save(_ projects: [Project]) -> Bool {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? encoder.encode(projects) else { return false }
        return (try? data.write(to: fileURL, options: .atomic)) != nil
    }

    /// 新建项目（默认名：项目 N）。
    @discardableResult
    static func create(assetIds: [String]) -> Project {
        var projects = load()
        var seen = Set<String>()
        let uniqueIds = assetIds.filter { seen.insert($0).inserted }
        let project = Project(
            id: UUID().uuidString,
            name: "项目 \(projects.count + 1)",
            assetIds: uniqueIds,
            createdAt: Date()
        )
        projects.insert(project, at: 0)
        save(projects)
        return project
    }

    static func remove(id: String) {
        var projects = load()
        projects.removeAll { $0.id == id }
        save(projects)
    }

    static func rename(id: String, name: String) {
        var projects = load()
        if let index = projects.firstIndex(where: { $0.id == id }) {
            projects[index].name = name
            save(projects)
        }
    }

    /// 把资产 id 解析成 PHAsset（顺序与 id 一致，找不到的跳过，重复 id 只保留第一次）。
    static func assets(for project: Project) -> [PHAsset] {
        guard !project.assetIds.isEmpty else { return [] }
        let result = PHAsset.fetchAssets(withLocalIdentifiers: project.assetIds, options: nil)
        var map: [String: PHAsset] = [:]
        result.enumerateObjects { asset, _, _ in
            map[asset.localIdentifier] = asset
        }
        var seen = Set<String>()
        return project.assetIds.compactMap { id in
            guard seen.insert(id).inserted else { return nil }
            return map[id]
        }
    }
}

/// 系统相册多选（PHPicker）：只选图片，返回 PHAsset（本地库照片）。
struct PhotoPickerView: UIViewControllerRepresentable {
    /// 选择完成（1 张及以上）。
    let onPicked: ([PHAsset]) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(onPicked: onPicked)
    }

    func makeUIViewController(context: Context) -> PHPickerViewController {
        var configuration = PHPickerConfiguration(photoLibrary: .shared())
        configuration.filter = .images
        configuration.selectionLimit = 0 // 不限张数
        // 按用户点选顺序返回结果（否则相册式左右滑动的顺序会和添加顺序不一致）。
        configuration.selection = .ordered
        configuration.preferredAssetRepresentationMode = .current
        let controller = PHPickerViewController(configuration: configuration)
        controller.delegate = context.coordinator
        return controller
    }

    func updateUIViewController(_ uiViewController: PHPickerViewController, context: Context) {}

    final class Coordinator: NSObject, PHPickerViewControllerDelegate {
        private let onPicked: ([PHAsset]) -> Void

        init(onPicked: @escaping ([PHAsset]) -> Void) {
            self.onPicked = onPicked
        }

        func picker(_ picker: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
            picker.dismiss(animated: true)
            let identifiers = results.compactMap { $0.assetIdentifier }
            guard !identifiers.isEmpty else { return }
            var seen = Set<String>()
            let uniqueIdentifiers = identifiers.filter { seen.insert($0).inserted }
            let fetched = PHAsset.fetchAssets(withLocalIdentifiers: uniqueIdentifiers, options: nil)
            var map: [String: PHAsset] = [:]
            fetched.enumerateObjects { asset, _, _ in
                map[asset.localIdentifier] = asset
            }
            let assets = uniqueIdentifiers.compactMap { map[$0] }
            guard !assets.isEmpty else { return }
            onPicked(assets)
        }
    }
}
