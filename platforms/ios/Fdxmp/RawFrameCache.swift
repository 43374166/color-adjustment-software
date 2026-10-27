import Foundation

/// 一张 RAW 解码结果的缓存实体：原始字节 + 预览缓冲 +（可选）细节缓冲。
final class RawFrameEntry: NSObject {
    let data: Data
    let base: RgbaBuffer
    var detail: RgbaBuffer?

    init(data: Data, base: RgbaBuffer, detail: RgbaBuffer? = nil) {
        self.data = data
        self.base = base
        self.detail = detail
        super.init()
    }

    /// 缓存容量按字节算（NSCache 的 totalCostLimit 用它淘汰）。
    var byteCost: Int {
        data.count + base.pixels.count + (detail?.pixels.count ?? 0)
    }
}

/// 最近打开过的 RAW 帧缓存。
///
/// 进一次编辑页的成本是：PhotoKit 取原始字节（可能是 iCloud 下载，几十 MB）
/// → LibRaw 半尺寸解调（1–4s）→ 转 RGBA8 → CPU 首帧渲染。
/// 以前退出编辑页 `EditorModel` 就被销毁，点回去整条链路重跑一遍
/// （用户看到的"退出来再进去又要加载十几秒"）。这里按「资产 + 修改时间 + 预览尺寸」
/// 缓存最近 1–2 张，退出再进直接命中。
///
/// NSCache 线程安全，并且收到内存警告时会自动清空——缓存未命中只是重走一遍解码，
/// 不会像自己维护的强引用缓存那样把 App 顶到 jetsam。
final class RawFrameCache {
    static let shared = RawFrameCache()

    private let cache = NSCache<NSString, RawFrameEntry>()

    private init() {
        cache.countLimit = 2
        // 24MP ProRAW 的 DNG 约 25–90MB，预览 + 细节缓冲约 26MB：
        // 两套全留太顶，上限压到 160MB，超出自动淘汰。
        cache.totalCostLimit = 160 * 1024 * 1024
    }

    static func key(assetId: String, maxEdge: Int, version: TimeInterval?) -> String {
        "\(assetId)|\(maxEdge)|\(Int(version ?? 0))"
    }

    func entry(for key: String) -> RawFrameEntry? {
        cache.object(forKey: key as NSString)
    }

    func store(_ entry: RawFrameEntry, for key: String) {
        cache.setObject(entry, forKey: key as NSString, cost: entry.byteCost)
    }

    func removeAll() {
        cache.removeAllObjects()
    }
}
