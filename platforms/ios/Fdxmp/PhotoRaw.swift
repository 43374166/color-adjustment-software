import Foundation
import Photos

/// RAW / DNG 照片（iPhone ProRAW、相机 / Lightroom 导出的 DNG）检测与原始数据读取。
///
/// PhotoKit 能把 ProRAW 渲染成普通图片，但「真 RAW 解码」交给 Rust 核心的 LibRaw
/// （rsraw）：从 `PHAssetResource` 取原始文件字节，经 `NativeCore.decodeRaw`
/// 解调成 RGBA8 后进入调色管线（与桌面端同一条 LibRaw 路径）。
enum PhotoRaw {
    /// 支持的 RAW 扩展名（与 Rust 核心 `decode.rs` 的 `RAW_EXTENSIONS` 保持一致）。
    static let rawExtensions: Set<String> = [
        "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "dng", "raf",
        "orf", "rw2", "pef", "srw", "raw", "rwl", "3fr", "fff", "iiq", "mos",
        "mrw", "x3f",
    ]

    /// 资产是否为 RAW（原始资源里有 RAW 文件；ProRAW 的 DNG 在 `.alternatePhoto`）。
    static func isRawAsset(_ asset: PHAsset) -> Bool {
        rawResource(for: asset) != nil
    }

    /// 原始 RAW 资源：优先 `.alternatePhoto`（ProRAW / RAW+JPEG 的 RAW 面），
    /// 否则取第一个 UTI / 扩展名像 RAW 的资源。
    ///
    /// 用资源判定而不是 `PHAssetMediaSubtype`：不同 SDK 的 RAW 子类型命名不一致，
    /// 而 UTI（`*-raw-image`）与文件名扩展名跨系统版本稳定。
    static func rawResource(for asset: PHAsset) -> PHAssetResource? {
        let resources = PHAssetResource.assetResources(for: asset)
        let rawCandidates = resources.filter(isRaw)
        return rawCandidates.first { $0.type == .alternatePhoto } ?? rawCandidates.first
    }

    /// 资源本身是否为 RAW（UTI 以 raw-image 结尾，或文件名是 RAW 扩展名）。
    private static func isRaw(_ resource: PHAssetResource) -> Bool {
        if resource.uniformTypeIdentifier.lowercased().contains("raw-image") {
            return true
        }
        let ext = (resource.originalFilename as NSString).pathExtension.lowercased()
        return rawExtensions.contains(ext)
    }

    /// 读取原始 RAW 字节（iCloud 照片允许联网拉取）。
    ///
    /// 回调在后台线程；数据可能较大（几十 MB），调用方应转交工作线程解码。
    static func requestData(for asset: PHAsset, completion: @escaping (Data?) -> Void) {
        guard let resource = rawResource(for: asset) else {
            completion(nil)
            return
        }
        let options = PHAssetResourceRequestOptions()
        options.isNetworkAccessAllowed = true
        var data = Data()
        PHAssetResourceManager.default().requestData(
            for: resource,
            options: options
        ) { chunk in
            data.append(chunk)
        } completionHandler: { error in
            if error != nil || data.isEmpty {
                completion(nil)
            } else {
                completion(data)
            }
        }
    }
}
