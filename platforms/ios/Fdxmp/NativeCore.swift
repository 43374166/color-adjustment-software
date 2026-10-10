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
        guard let pointer, outLen >= 8 else { return nil }
        defer { fdxmp_bytes_free(pointer, outLen) }

        let payload = UnsafeBufferPointer(start: pointer, count: outLen)
        let width = Int(
            UInt32(payload[0]) << 24 | UInt32(payload[1]) << 16
                | UInt32(payload[2]) << 8 | UInt32(payload[3])
        )
        let height = Int(
            UInt32(payload[4]) << 24 | UInt32(payload[5]) << 16
                | UInt32(payload[6]) << 8 | UInt32(payload[7])
        )
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
