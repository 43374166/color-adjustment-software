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
    func decodePayload(pointer: UnsafeMutablePointer<UInt8>?, outLen: Int) -> RenderResult? {
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

    // MARK: - 预设（.fdxmp / XMP）

    /// 从 `.fdxmp` 字节流读取预设元数据 + 参数 JSON（`{name, author, params}`）。
    func presetJsonFromFdxmp(_ data: Data) -> String? {
        guard !data.isEmpty else { return nil }
        let pointer = data.withUnsafeBytes { raw -> UnsafeMutablePointer<CChar>? in
            guard let base = raw.bindMemory(to: UInt8.self).baseAddress else { return nil }
            return fdxmp_preset_json_from_bytes(base, data.count)
        }
        guard let pointer else { return nil }
        defer { fdxmp_string_free(pointer) }
        return String(cString: pointer)
    }

    /// 从 Lightroom XMP 文本读取预设（`{name, author, params}`）。
    func presetJsonFromXmp(_ text: String) -> String? {
        let pointer = text.withCString { fdxmp_preset_json_from_xmp($0) }
        guard let pointer else { return nil }
        defer { fdxmp_string_free(pointer) }
        return String(cString: pointer)
    }

    /// 由参数 JSON 打包 `.fdxmp` 字节流（导出预设）。
    func presetBytes(paramsJson: String, name: String, author: String) -> [UInt8]? {
        var outLen = 0
        let pointer = paramsJson.withCString { json in
            name.withCString { presetName in
                author.withCString { presetAuthor in
                    fdxmp_preset_bytes_from_params_json(
                        json,
                        presetName,
                        presetAuthor,
                        &outLen
                    )
                }
            }
        }
        guard let pointer, outLen > 0 else { return nil }
        defer { fdxmp_bytes_free(pointer, outLen) }
        return Array(UnsafeBufferPointer(start: pointer, count: outLen))
    }
}

/// GPU 预览会话（Metal；核心 `gpu` feature）。
///
/// 源图只上传一次，参数变化时重渲染并回读；创建失败（模拟器 / 老设备 /
/// 驱动不足 / 未启用 feature）返回 nil，调用方自动回退 CPU 预览。
/// 只在 `EditorModel.renderQueue` 这类工作线程「创建 → 渲染 → 释放」串行使用。
final class GpuPreviewSession {
    private var handle: UnsafeMutableRawPointer?

    init?(buffer: RgbaBuffer) {
        let pointer = buffer.pixels.withUnsafeBufferPointer { pixels in
            fdxmp_gpu_preview_create(
                pixels.baseAddress,
                UInt32(buffer.width),
                UInt32(buffer.height)
            )
        }
        guard let pointer else { return nil }
        handle = pointer
    }

    deinit {
        if let handle {
            fdxmp_gpu_preview_destroy(handle)
        }
    }

    /// GPU 后端名称（"metal" / "dx12" / "vulkan"…；失败返回 nil）。
    var backend: String? {
        guard let handle, let raw = fdxmp_gpu_preview_backend(handle) else { return nil }
        defer { fdxmp_string_free(raw) }
        return String(cString: raw)
    }

    /// 按参数 JSON 渲染一帧。
    ///
    /// 返回 nil 表示本帧不适合 GPU（如「裁剪 + 蒙版」组合）或渲染失败，
    /// 调用方应回退 CPU 渲染；失败原因见 `NativeCore.shared.lastError()`。
    func render(paramsJson: String) -> NativeCore.RenderResult? {
        guard let handle else { return nil }
        var outWidth: UInt32 = 0
        var outHeight: UInt32 = 0
        var outLen = 0
        let pointer = paramsJson.withCString { params in
            fdxmp_gpu_preview_render(handle, params, &outWidth, &outHeight, &outLen)
        }
        return NativeCore.shared.decodePayload(pointer: pointer, outLen: outLen)
    }

    /// 零拷贝上屏：把 CAMetalLayer 挂到会话上（失败返回 false，继续走回读路径）。
    func attachLayer(_ layer: UnsafeMutableRawPointer) -> Bool {
        guard let handle else { return false }
        return fdxmp_gpu_preview_attach_layer(handle, layer) == 0
    }

    /// 渲染一帧并直接上屏（不读回像素）；返回输出尺寸，失败返回 nil（回退回读路径）。
    func present(paramsJson: String) -> (width: Int, height: Int)? {
        guard let handle else { return nil }
        var outWidth: UInt32 = 0
        var outHeight: UInt32 = 0
        let code = paramsJson.withCString { params in
            fdxmp_gpu_preview_present(handle, params, &outWidth, &outHeight)
        }
        guard code == 0, outWidth > 0, outHeight > 0 else { return nil }
        return (Int(outWidth), Int(outHeight))
    }

    /// GPU 直方图（4×256：R / G / B / 亮度；基于最近一帧显示纹理）。
    func histogram() -> HistogramData? {
        guard let handle else { return nil }
        var outLen = 0
        let pointer = fdxmp_gpu_preview_histogram(handle, &outLen)
        guard let pointer, outLen >= 4096 else { return nil }
        defer { fdxmp_bytes_free(pointer, outLen) }
        let payload = UnsafeBufferPointer(start: pointer, count: outLen)
        var bins = [Int](repeating: 0, count: 1024)
        for index in 0..<1024 {
            let offset = index * 4
            let value = UInt32(payload[offset])
                | (UInt32(payload[offset + 1]) << 8)
                | (UInt32(payload[offset + 2]) << 16)
                | (UInt32(payload[offset + 3]) << 24)
            bins[index] = Int(value)
        }
        return HistogramData(
            red: Array(bins[0..<256]),
            green: Array(bins[256..<512]),
            blue: Array(bins[512..<768]),
            luma: Array(bins[768..<1024])
        )
    }
}
