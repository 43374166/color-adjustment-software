import Foundation

/// Rust 核心桥接（C ABI，对应 crates/ffi 与 crates/ffi/include/fdxmp.h）。
///
/// iOS 走「解码 + Rust 调色」：HEIC / JPEG 由 PhotoKit 解码后把 RGBA8 像素交给
/// `fdxmp_render_memory_rgba`；相机 RAW / DNG（ProRAW 等）走 Rust 核心的
/// LibRaw（`fdxmp_decode_raw_rgba8`）解调，预览 / 导出与桌面端同源。
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

    /// 8bit sRGB → OkLCh（L、C、色相弧度）。
    ///
    /// "点颜色"取色器用：结果直接就是 `PointColorSample` 的 `lum` / `chroma` / `hue`，
    /// 与调色段共用核心的同一套矩阵（失败返回 nil）。
    func oklch(r: UInt8, g: UInt8, b: UInt8) -> (l: Float, c: Float, h: Float)? {
        var out: [Float] = [0, 0, 0]
        let code = out.withUnsafeMutableBufferPointer { buffer -> Int32 in
            fdxmp_color_oklch_from_srgb8(r, g, b, buffer.baseAddress)
        }
        guard code == 0 else { return nil }
        return (out[0], out[1], out[2])
    }

    /// 颜色均衡器曲线（512 点）：`channel` 0 色相 / 1 饱和度 / 2 亮度。
    ///
    /// 与渲染走同一份 Rust 算法，UI 画出来的曲线就是实际生效的曲线。
    func equalizerCurve(channel: Int, paramsJson: String) -> [Float] {
        var out = [Float](repeating: 0, count: 512)
        let code = paramsJson.withCString { json in
            out.withUnsafeMutableBufferPointer { buffer -> Int32 in
                fdxmp_equalizer_curve(UInt32(channel), json, buffer.baseAddress)
            }
        }
        return code == 0 ? out : []
    }

    /// 分块渲染整帧（导出用，原图分辨率）：内部按条带切分，几何为裁切/翻转时也能保持内存受控。
    ///
    /// 阻塞调用，必须在工作线程执行；失败返回 nil（原因见 `lastError()`）。
    func renderTiled(buffer: RgbaBuffer, paramsJson: String, stripPixels: Int = 2_000_000)
        -> RenderResult? {
        var outWidth: UInt32 = 0
        var outHeight: UInt32 = 0
        var outLen = 0
        let pointer = buffer.pixels.withUnsafeBufferPointer { pixels in
            paramsJson.withCString { params in
                fdxmp_render_memory_tiled_rgba(
                    pixels.baseAddress,
                    UInt32(buffer.width),
                    UInt32(buffer.height),
                    params,
                    UInt32(max(stripPixels, 1)),
                    &outWidth,
                    &outHeight,
                    &outLen
                )
            }
        }
        return decodePayload(pointer: pointer, outLen: outLen)
    }

    /// 对内存 RGBA8 图像应用 `EditParams` JSON，返回渲染后的像素。
    ///
    /// 阻塞调用，必须在工作线程执行；失败返回 nil（原因见 `lastError()`）。
    func render(buffer: RgbaBuffer, paramsJson: String) -> RenderResult? {        var outWidth: UInt32 = 0
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

    /// 用 LibRaw（rsraw）解码相机 RAW / DNG 字节，返回 RGBA8 缓冲。
    ///
    /// `fullSize = false` 走半尺寸解调（预览，快）；`true` 走全尺寸解调（导出）。
    /// 输出按 `maxEdge` 等比缩小（不放大）。阻塞调用，必须在工作线程执行；
    /// 失败返回 nil（原因见 `lastError()`）。
    func decodeRaw(data: Data, maxEdge: Int, fullSize: Bool) -> RgbaBuffer? {
        guard !data.isEmpty else { return nil }
        var outWidth: UInt32 = 0
        var outHeight: UInt32 = 0
        var outLen = 0
        let pointer = data.withUnsafeBytes { bytes in
            fdxmp_decode_raw_rgba8(
                bytes.bindMemory(to: UInt8.self).baseAddress,
                data.count,
                UInt32(max(maxEdge, 64)),
                fullSize ? 1 : 0,
                &outWidth,
                &outHeight,
                &outLen
            )
        }
        guard let result = decodePayload(pointer: pointer, outLen: outLen) else { return nil }
        return RgbaBuffer(pixels: result.pixels, width: result.width, height: result.height)
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

    // MARK: - 智能蒙版（v1.4.0）

    /// 提交权重栅格（f32 0..1，行优先）：核心缓存后供渲染 / 叠加 / 导出复用。
    ///
    /// 阻塞调用，必须在工作线程执行；失败返回 false（原因见 `lastError()`）。
    func submitMaskRaster(
        paramsJson: String,
        maskId: String,
        width: Int,
        height: Int,
        weights: [Float]
    ) -> Bool {
        let pointer = weights.withUnsafeBufferPointer { buffer in
            maskId.withCString { mask in
                paramsJson.withCString { params in
                    fdxmp_mask_raster_submit(
                        params,
                        mask,
                        UInt32(width),
                        UInt32(height),
                        buffer.baseAddress,
                        weights.count
                    )
                }
            }
        }
        return pointer == 0
    }

    /// 提交多边形蒙版（五官关键点，归一化坐标）：核心负责光栅化与羽化。
    func submitMaskPolygons(
        paramsJson: String,
        maskId: String,
        width: Int,
        height: Int,
        polygonsJson: String,
        feather: Float
    ) -> Bool {
        let code = polygonsJson.withCString { polygons in
            maskId.withCString { mask in
                paramsJson.withCString { params in
                    fdxmp_mask_polygons_submit(
                        params,
                        mask,
                        UInt32(width),
                        UInt32(height),
                        polygons,
                        feather
                    )
                }
            }
        }
        return code == 0
    }

    /// 蒙版栅格是否已生成。
    func maskRasterReady(paramsJson: String, maskId: String) -> Bool {
        let code = maskId.withCString { mask in
            paramsJson.withCString { params in
                fdxmp_mask_raster_ready(params, mask)
            }
        }
        return code == 1
    }

    /// 清空智能蒙版栅格缓存（切换图片时调用）。
    @discardableResult
    func clearMaskRasters() -> Bool {
        fdxmp_mask_raster_clear() == 0
    }

    /// 用随包 ONNX 模型生成智能蒙版（双阶段流水线），写入共享缓存。
    ///
    /// 返回：0 = 成功；2 = 模型/运行库不可用或该类型不支持（调用方回退 Vision）；
    /// 其它 = 推理失败（原因见 `lastError()`）。
    /// 阻塞调用（首次约数秒，之后同图命中缓存），必须在工作线程执行。
    func smartMaskGenerate(paramsJson: String, maskId: String, buffer: RgbaBuffer) -> Int32 {
        buffer.pixels.withUnsafeBufferPointer { pixels in
            maskId.withCString { mask in
                paramsJson.withCString { params in
                    fdxmp_smartmask_generate(
                        params,
                        mask,
                        pixels.baseAddress,
                        UInt32(buffer.width),
                        UInt32(buffer.height)
                    )
                }
            }
        }
    }

    /// 随包 ONNX 模型是否可用（1 = 就绪，0 = 不可用）。
    var smartMaskRuntimeReady: Bool {
        fdxmp_smartmask_runtime_ready() == 1
    }

    // MARK: - AI 修图（v1.5.0）

    /// 修图识别（一键修图第一步）：五官 478 点 + 皮肤 / 人像蒙版。
    ///
    /// 返回解析后的 JSON 字符串（`{faces, landmarks, skin, person, warnings}`）；
    /// 阻塞调用（数秒），必须在工作线程执行；失败返回 nil（原因见 `lastError()`）。
    func retouchAnalyze(buffer: RgbaBuffer) -> String? {
        let pointer = buffer.pixels.withUnsafeBufferPointer { pixels in
            fdxmp_retouch_analyze(
                pixels.baseAddress,
                UInt32(buffer.width),
                UInt32(buffer.height)
            )
        }
        guard let pointer else { return nil }
        defer { fdxmp_string_free(pointer) }
        return String(cString: pointer)
    }

    /// 修图应用：按 RetouchParams JSON 处理图像，返回新的 RGBA 缓冲。
    ///
    /// 阻塞调用（同图第二次起会复用识别结果），必须在工作线程执行。
    func retouchApply(paramsJson: String, buffer: RgbaBuffer) -> RgbaBuffer? {
        var outWidth: UInt32 = 0
        var outHeight: UInt32 = 0
        var outLen = 0
        let pointer = buffer.pixels.withUnsafeBufferPointer { pixels in
            paramsJson.withCString { params in
                fdxmp_retouch_apply(
                    params,
                    pixels.baseAddress,
                    UInt32(buffer.width),
                    UInt32(buffer.height),
                    &outWidth,
                    &outHeight,
                    &outLen
                )
            }
        }
        guard let result = decodePayload(pointer: pointer, outLen: outLen) else { return nil }
        return RgbaBuffer(pixels: result.pixels, width: result.width, height: result.height)
    }

    /// 提交平台人像蒙版（Vision 人像分割 α；MODNet 不随包时补位）。
    @discardableResult
    func retouchSubmitPersonMask(buffer: RgbaBuffer, plane: [Float], planeWidth: Int, planeHeight: Int) -> Bool {
        let code = plane.withUnsafeBufferPointer { weights in
            buffer.pixels.withUnsafeBufferPointer { pixels in
                fdxmp_retouch_submit_person_mask(
                    pixels.baseAddress,
                    UInt32(buffer.width),
                    UInt32(buffer.height),
                    weights.baseAddress,
                    UInt32(planeWidth),
                    UInt32(planeHeight)
                )
            }
        }
        return code == 0
    }

    /// 提交平台姿态（Vision 人脸关键点 + 身体关节；身体 / 脸部塑形用）。
    ///
    /// `poseJson` 坐标归一化 0..1（原点左上）；与图像指纹绑定，换图自动失效。
    @discardableResult
    func retouchSubmitPose(buffer: RgbaBuffer, poseJson: String) -> Bool {
        let code = poseJson.withCString { pose in
            buffer.pixels.withUnsafeBufferPointer { pixels in
                fdxmp_retouch_submit_pose(
                    pixels.baseAddress,
                    UInt32(buffer.width),
                    UInt32(buffer.height),
                    pose
                )
            }
        }
        return code == 0
    }

    /// AI 修图进度文本（阶段名；空串 = 空闲）。UI 修图期间轮询显示加载文案。
    func retouchProgress() -> String {
        guard let pointer = fdxmp_retouch_progress() else { return "" }
        defer { fdxmp_string_free(pointer) }
        return String(cString: pointer)
    }

    /// 导出增强：Real-ESRGAN 超分（`targetMaxEdge` = 目标最长边；0 = 4 倍）。
    ///
    /// 阻塞调用（数秒～数十秒），必须在工作线程执行；失败返回 nil（原因见 `lastError()`）。
    func retouchUpscale(buffer: RgbaBuffer, targetMaxEdge: Int) -> RgbaBuffer? {
        var outWidth: UInt32 = 0
        var outHeight: UInt32 = 0
        var outLen = 0
        let pointer = buffer.pixels.withUnsafeBufferPointer { pixels in
            fdxmp_retouch_upscale(
                pixels.baseAddress,
                UInt32(buffer.width),
                UInt32(buffer.height),
                UInt32(max(targetMaxEdge, 0)),
                &outWidth,
                &outHeight,
                &outLen
            )
        }
        guard let result = decodePayload(pointer: pointer, outLen: outLen) else { return nil }
        return RgbaBuffer(pixels: result.pixels, width: result.width, height: result.height)
    }

    /// 污点修复（手动祛痘笔）：x / y 归一化、radius 像素；返回修好的缓冲。
    func healApply(
        buffer: RgbaBuffer,
        x: Float,
        y: Float,
        radius: Float,
        strength: Float
    ) -> RgbaBuffer? {
        var outWidth: UInt32 = 0
        var outHeight: UInt32 = 0
        var outLen = 0
        let pointer = buffer.pixels.withUnsafeBufferPointer { pixels in
            fdxmp_heal_apply(
                pixels.baseAddress,
                UInt32(buffer.width),
                UInt32(buffer.height),
                x,
                y,
                radius,
                strength,
                &outWidth,
                &outHeight,
                &outLen
            )
        }
        guard let result = decodePayload(pointer: pointer, outLen: outLen) else { return nil }
        return RgbaBuffer(pixels: result.pixels, width: result.width, height: result.height)
    }

    /// 液化：按笔画 JSON 推挤变形，返回处理后的缓冲。
    func liquifyApply(buffer: RgbaBuffer, strokesJson: String) -> RgbaBuffer? {
        var outWidth: UInt32 = 0
        var outHeight: UInt32 = 0
        var outLen = 0
        let pointer = strokesJson.withCString { strokes in
            buffer.pixels.withUnsafeBufferPointer { pixels in
                fdxmp_liquify_apply(
                    pixels.baseAddress,
                    UInt32(buffer.width),
                    UInt32(buffer.height),
                    strokes,
                    &outWidth,
                    &outHeight,
                    &outLen
                )
            }
        }
        guard let result = decodePayload(pointer: pointer, outLen: outLen) else { return nil }
        return RgbaBuffer(pixels: result.pixels, width: result.width, height: result.height)
    }

    /// 智能蒙版模型清单与状态（JSON；macOS / 桌面端 ONNX 用，iOS 走 Vision）。
    func smartMaskModelsJson() -> String? {
        guard let pointer = fdxmp_smartmask_models_json() else { return nil }
        defer { fdxmp_string_free(pointer) }
        return String(cString: pointer)
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
