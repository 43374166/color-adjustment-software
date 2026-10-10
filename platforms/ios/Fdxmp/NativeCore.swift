import Foundation

/// Rust 核心桥接（C ABI，对应 crates/ffi 与 crates/ffi/include/fdxmp.h）。
///
/// iOS 走「系统解码 + Rust 调色」：PhotoKit 负责解码 HEIC / JPEG / RAW，
/// 再把 RGBA8 像素交给 `fdxmp_render_memory_rgba` 应用编辑参数。
/// 因此 iOS 构建不需要 LibRaw / libheif（`raw` / `heif` feature 保持关闭）。
final class NativeCore {
    static let shared = NativeCore()

    /// 渲染结果：行优先 RGBA8 像素。
    struct RenderResult {
        let pixels: [UInt8]
        let width: Int
        let height: Int
    }

    /// 核心版本（通过 C ABI `fdxmp_version`）。
    let version: String

    /// ABI 版本（通过 C ABI `fdxmp_abi_version`）。
    let abiVersion: UInt32

    private init() {
        if let raw = fdxmp_version() {
            version = String(cString: raw)
        } else {
            version = "未知"
        }
        abiVersion = fdxmp_abi_version()
    }

    /// 最近一次失败原因（thread-local，须在调用线程读取）。
    func lastError() -> String? {
        guard let message = fdxmp_last_error_message() else { return nil }
        defer { fdxmp_string_free(message) }
        return String(cString: message)
    }

    /// 对内存 RGBA8 图像应用 `EditParams` JSON，返回渲染后的像素。
    ///
    /// 阻塞调用，必须在工作线程执行；失败返回 nil（原因见 `lastError()`）。
    func render(buffer: RgbaBuffer, paramsJson: String) -> RenderResult? {
        var outWidth: UInt32 = 0
        var outHeight: UInt32 = 0
        var outLen = 0
        let pointer = buffer.pixels.withUnsafeBufferPointer { pixels in
            paramsJson.withCString { params in
                fdxmp_render_memory_rgba(
                    pixels.baseAddress,
                    UInt32(buffer.width),
                    UInt32(buffer.height),
                    params,
                    &outWidth,
                    &outHeight,
                    &outLen
                )
            }
        }
        return decodePayload(pointer: pointer, outLen: outLen)
    }

    /// 生成单个蒙版的叠加层（红色半透明，预乘 RGBA8），供画布叠加与列表缩略图使用。
    ///
    /// 阻塞调用，必须在工作线程执行；失败返回 nil（原因见 `lastError()`）。
    func maskOverlay(paramsJson: String, maskId: String, width: Int, height: Int) -> RenderResult? {
        guard width > 0, height > 0 else { return nil }
        var outWidth: UInt32 = 0
        var outHeight: UInt32 = 0
        var outLen = 0
        let pointer = maskId.withCString { mask in
            paramsJson.withCString { params in
                fdxmp_mask_overlay_rgba(
                    params,
                    mask,
                    UInt32(width),
                    UInt32(height),
                    &outWidth,
                    &outHeight,
                    &outLen
                )
            }
        }
        return decodePayload(pointer: pointer, outLen: outLen)
    }

    /// 只渲染整帧的一块区域（放大 1:1 看细节；暗角 / 颗粒与蒙版按整帧坐标算）。
    ///
    /// `buffer` 是区域像素（已含余量），各坐标都是**该缓冲所对应源图的像素坐标**。
    /// 阻塞调用，必须在工作线程执行；失败返回 nil（条件不满足时见 `lastError()`）。
    func renderRegion(
        buffer: RgbaBuffer,
        originX: Int,
        originY: Int,
        visibleX: Int,
        visibleY: Int,
        visibleWidth: Int,
        visibleHeight: Int,
        fullWidth: Int,
        fullHeight: Int,
        paramsJson: String
    ) -> RenderResult? {
        var outWidth: UInt32 = 0
        var outHeight: UInt32 = 0
        var outLen = 0
        let pointer = buffer.pixels.withUnsafeBufferPointer { pixels in
            paramsJson.withCString { params in
                fdxmp_render_region_rgba(
                    pixels.baseAddress,
                    UInt32(buffer.width),
                    UInt32(buffer.height),
                    params,
                    UInt32(max(originX, 0)),
                    UInt32(max(originY, 0)),
                    UInt32(max(visibleX, 0)),
                    UInt32(max(visibleY, 0)),
                    UInt32(max(visibleWidth, 1)),
                    UInt32(max(visibleHeight, 1)),
                    UInt32(max(fullWidth, 1)),
                    UInt32(max(fullHeight, 1)),
                    &outWidth,
                    &outHeight,
                    &outLen
                )
            }
        }
        return decodePayload(pointer: pointer, outLen: outLen)
    }

    /// 解析 C ABI 返回的「8 字节大端宽高 + RGBA8 像素」缓冲区。
    private func decodePayload(pointer: UnsafeMutablePointer<UInt8>?, outLen: Int) -> RenderResult? {
        guard let pointer, outLen >= 8 else { return nil }
        defer { fdxmp_bytes_free(pointer, outLen) }

        let payload = UnsafeBufferPointer(start: pointer, count: outLen)
        // 显式拆成单字节位移，避免 Release（WMO）下类型检查超时。
        let widthHigh = UInt32(payload[0]) << 24
        let widthMid = UInt32(payload[1]) << 16
        let widthLow = UInt32(payload[2]) << 8
        let widthRaw = widthHigh | widthMid | widthLow | UInt32(payload[3])
        let heightHigh = UInt32(payload[4]) << 24
        let heightMid = UInt32(payload[5]) << 16
        let heightLow = UInt32(payload[6]) << 8
        let heightRaw = heightHigh | heightMid | heightLow | UInt32(payload[7])
        let width = Int(widthRaw)
        let height = Int(heightRaw)
        guard width > 0, height > 0, outLen >= 8 + width * height * 4 else {
            return nil
        }
        return RenderResult(
            pixels: Array(payload[8..<outLen]),
            width: width,
            height: height
        )
    }
}
