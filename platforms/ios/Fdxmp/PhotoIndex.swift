import Photos
import UIKit

/// 照片元信息索引条目（文件名 / 是否 RAW）。
struct PhotoIndexEntry: Codable {
    var name: String
    var isRaw: Bool
}

/// 文件名缓存（`PHAssetResource` 查询较慢，界面上会反复用到）。
///
/// 进程内共享：设备页、最近编辑、编辑页标题都读这里，同一张照片只查一次 PhotoKit。
enum PhotoNames {
    private static let cache = NSCache<NSString, NSString>()

    static func name(for asset: PHAsset) -> String {
        let key = asset.localIdentifier as NSString
        if let cached = cache.object(forKey: key) {
            return cached as String
        }
        let name = PHAssetResource.assetResources(for: asset).first?.originalFilename
            ?? "未命名照片"
        cache.setObject(name as NSString, forKey: key)
        return name
    }

    /// 索引构建时已经查过资源，顺手把名字写进缓存，避免后面再查一次。
    static func remember(_ name: String, for asset: PHAsset) {
        cache.setObject(name as NSString, forKey: asset.localIdentifier as NSString)
    }
}

/// 索引落盘：冷启动先恢复上一轮的索引，切筛选 / 搜索立刻可用。
///
/// 只保存「文件名 + 是否 RAW」这种小字段（1 万张约 400KB），
/// 后台只补新增照片，不会因为拍一张 / 导入一张就让整库重建。
enum PhotoIndexStore {
    private static let fileName = "photo-index-v1.json"
    private static let saveQueue = DispatchQueue(label: "fdxmp.ios.photo-index", qos: .utility)

    private static var fileURL: URL? {
        FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first?
            .appendingPathComponent(fileName)
    }

    static func load() -> [String: PhotoIndexEntry] {
        guard let fileURL, let data = try? Data(contentsOf: fileURL) else { return [:] }
        return (try? JSONDecoder().decode([String: PhotoIndexEntry].self, from: data)) ?? [:]
    }

    static func save(_ entries: [String: PhotoIndexEntry]) {
        saveQueue.async {
            guard let fileURL, let data = try? JSONEncoder().encode(entries) else { return }
            try? data.write(to: fileURL, options: .atomic)
        }
    }
}

/// 缩略图内存缓存：退出编辑页 / 关掉全屏预览再回来时直接命中。
///
/// 以前每个格子都靠 `onAppear` 重新向 PhotoKit 请求，`fullScreenCover` 一关，
/// 整屏（RAW 更慢）重新排队渲染，看起来就是"退出来又要加载十几秒"。
final class ThumbnailCache {
    static let shared = ThumbnailCache()

    private let cache = NSCache<NSString, UIImage>()

    private init() {
        cache.countLimit = 600
        // 网格 480px ≈ 0.9MB/张、列表 240px ≈ 0.2MB/张：96MB 够放一屏多，
        // 超出由 NSCache 自动淘汰（未命中只是重新请求，不会出错）。
        cache.totalCostLimit = 96 * 1024 * 1024
    }

    func image(for key: String) -> UIImage? {
        cache.object(forKey: key as NSString)
    }

    func store(_ image: UIImage, for key: String) {
        let pixels = image.size.width * image.size.height * image.scale * image.scale
        cache.setObject(image, forKey: key as NSString, cost: Int(pixels * 4))
    }

    func removeAll() {
        cache.removeAllObjects()
    }
}
