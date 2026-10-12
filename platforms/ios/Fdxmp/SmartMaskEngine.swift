import CoreGraphics
import CoreImage
import CoreVideo
import Foundation
import Vision

/// 智能蒙版生成（iOS / Vision）：主体 / 人像用像素级分割，
/// 面部 / 皮肤 / 五官用关键点多边形（交给 Rust 核心光栅化 + 羽化 + 缓存）。
///
/// 全部在端侧完成，默认不联网、不上传任何图像。
enum SmartMaskEngine {
    /// 生成结果：像素平面（主体 / 人像）或多边形（五官）。
    enum Outcome {
        /// 与提交尺寸一致的 f32 权重（行优先，0..1）。
        case plane(width: Int, height: Int, data: [Float])
        /// 归一化多边形（原点左上角）。
        case polygons(include: [[CGPoint]], exclude: [[CGPoint]])
    }

    enum EngineError: LocalizedError {
        case noImage
        case noPerson
        case noFace
        case unsupported(String)
        case visionFailed(String)

        var errorDescription: String? {
            switch self {
            case .noImage: return "图像无效"
            case .noPerson: return "没有检测到人物（换个构图或换张照片试试）"
            case .noFace: return "没有检测到人脸（五官蒙版需要清晰正脸）"
            case let .unsupported(feature): return "当前系统不支持：\(feature)"
            case let .visionFailed(message): return "Vision 分析失败：\(message)"
            }
        }
    }

    /// 生成指定类型的蒙版（阻塞调用，必须在工作线程执行）。
    static func generate(kind: MaskKindUi, buffer: RgbaBuffer) throws -> Outcome {
        guard
            buffer.width > 0,
            buffer.height > 0,
            buffer.pixels.count >= buffer.width * buffer.height * 4
        else {
            throw EngineError.noImage
        }
        guard let cgImage = makeCGImage(buffer: buffer) else {
            throw EngineError.noImage
        }
        switch kind {
        case .aiSubject:
            if #available(iOS 17.0, *) {
                if let plane = try foregroundInstanceMask(cgImage: cgImage, buffer: buffer) {
                    return plane
                }
            }
            return try personMask(cgImage: cgImage, buffer: buffer)
        case .aiPortrait:
            return try personMask(cgImage: cgImage, buffer: buffer)
        default:
            return try facePartMask(kind: kind, cgImage: cgImage)
        }
    }

    // MARK: - 像素分割（主体 / 人像）

    /// 主体（iOS 17+ Vision 前景实例分割）。
    @available(iOS 17.0, *)
    private static func foregroundInstanceMask(
        cgImage: CGImage,
        buffer: RgbaBuffer
    ) throws -> Outcome? {
        let request = VNGenerateForegroundInstanceMaskRequest()
        let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
        do {
            try handler.perform([request])
        } catch {
            return nil
        }
        guard let observation = request.results?.first else { return nil }
        guard let maskPixelBuffer = try? observation.generateScaledMaskForImage(
            forInstances: observation.allInstances,
            from: handler
        ) else {
            return nil
        }
        return pixelBufferToPlane(maskPixelBuffer, targetWidth: buffer.width, targetHeight: buffer.height)
    }

    /// 人像（Vision 人物分割；.accurate 质量最高）。
    private static func personMask(cgImage: CGImage, buffer: RgbaBuffer) throws -> Outcome {
        let request = VNGeneratePersonSegmentationRequest()
        request.qualityLevel = .accurate
        request.outputPixelFormat = kCVPixelFormatType_OneComponent8
        let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
        do {
            try handler.perform([request])
        } catch {
            throw EngineError.visionFailed(error.localizedDescription)
        }
        guard let observation = request.results?.first else {
            throw EngineError.noPerson
        }
        guard let plane = pixelBufferToPlane(
            observation.pixelBuffer,
            targetWidth: buffer.width,
            targetHeight: buffer.height
        ) else {
            throw EngineError.visionFailed("分割结果解析失败")
        }
        return plane
    }

    // MARK: - 人脸关键点（面部 / 皮肤 / 五官）

    /// 人像分割 α → f32 平面（供核心「平台人像蒙版」用；MODNet 不随包时补位）。
    ///
    /// 与 `personMask` 同一条 Vision 路径，但直接返回平面数据（不经过多边形/位图）。
    static func personMaskPlane(
        cgImage: CGImage,
        buffer: RgbaBuffer
    ) -> (data: [Float], width: Int, height: Int)? {
        let request = VNGeneratePersonSegmentationRequest()
        request.qualityLevel = .accurate
        request.outputPixelFormat = kCVPixelFormatType_OneComponent8
        let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
        do {
            try handler.perform([request])
        } catch {
            return nil
        }
        guard let observation = request.results?.first,
              let outcome = pixelBufferToPlane(
                  observation.pixelBuffer,
                  targetWidth: buffer.width,
                  targetHeight: buffer.height
              ),
              case let .plane(width, height, data) = outcome
        else {
            return nil
        }
        guard width > 0, height > 0 else { return nil }
        return (data, width, height)
    }

    private static func facePartMask(kind: MaskKindUi, cgImage: CGImage) throws -> Outcome {
        let request = VNDetectFaceLandmarksRequest()
        let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
        do {
            try handler.perform([request])
        } catch {
            throw EngineError.visionFailed(error.localizedDescription)
        }
        guard
            let face = request.results?
                .filter({ $0.landmarks != nil })
                .max(by: { area($0.boundingBox) < area($1.boundingBox) })
        else {
            throw EngineError.noFace
        }
        guard let landmarks = face.landmarks else { throw EngineError.noFace }

        // Vision：归一化坐标、原点左下角；转为原点左上角（与图像一致）。
        func convert(_ points: [CGPoint]) -> [CGPoint] {
            points.map { CGPoint(x: $0.x, y: 1 - $0.y) }
        }

        // 面部椭圆（由人脸框内缩一点，覆盖额头 / 下巴）。
        let box = face.boundingBox
        let ellipse = ellipsePolygon(
            rect: CGRect(
                x: box.origin.x,
                y: 1 - box.origin.y - box.height,
                width: box.width,
                height: box.height
            ),
            inset: 0.06,
            segments: 40
        )

        let eyes = (landmarks.leftEye?.normalizedPoints ?? []) + (landmarks.rightEye?.normalizedPoints ?? [])
        let eyebrows =
            (landmarks.leftEyebrow?.normalizedPoints ?? [])
            + (landmarks.rightEyebrow?.normalizedPoints ?? [])
        let nose =
            (landmarks.nose?.normalizedPoints ?? []) + (landmarks.noseCrest?.normalizedPoints ?? [])
        let outerLips = landmarks.outerLips?.normalizedPoints ?? []
        let innerLips = landmarks.innerLips?.normalizedPoints ?? []

        func require(_ points: [CGPoint], feature: String) throws -> [CGPoint] {
            let converted = convert(points)
            guard converted.count >= 3 else {
                throw EngineError.unsupported(feature)
            }
            return converted
        }

        switch kind {
        case .aiFace:
            return .polygons(include: [ellipse], exclude: [])
        case .aiSkin:
            var exclude: [[CGPoint]] = []
            if eyes.count >= 3 { exclude.append(convert(eyes)) }
            if eyebrows.count >= 3 { exclude.append(convert(eyebrows)) }
            if outerLips.count >= 3 { exclude.append(convert(outerLips)) }
            return .polygons(include: [ellipse], exclude: exclude)
        case .aiEyes:
            return .polygons(include: [try require(eyes, feature: "眼睛关键点")], exclude: [])
        case .aiEyebrows:
            return .polygons(include: [try require(eyebrows, feature: "眉毛关键点")], exclude: [])
        case .aiNose:
            return .polygons(include: [try require(nose, feature: "鼻子关键点")], exclude: [])
        case .aiLips:
            return .polygons(include: [try require(outerLips, feature: "嘴唇关键点")], exclude: [])
        case .aiTeeth:
            // 牙齿 ≈ 口腔内部（内唇轮廓）。
            return .polygons(include: [try require(innerLips, feature: "口腔关键点")], exclude: [])
        default:
            throw EngineError.unsupported(kind.rawValue)
        }
    }

    // MARK: - 工具

    /// 浮点平面 → CGFloat 面积。
    private static func area(_ rect: CGRect) -> CGFloat {
        rect.width * rect.height
    }

    /// 椭圆多边形（归一化坐标；`inset` 为向内收缩比例）。
    private static func ellipsePolygon(rect: CGRect, inset: CGFloat, segments: Int) -> [CGPoint] {
        let rx = rect.width / 2 * (1 - inset)
        let ry = rect.height / 2 * (1 - inset)
        let cx = rect.midX
        let cy = rect.midY
        var points: [CGPoint] = []
        points.reserveCapacity(segments)
        for index in 0..<segments {
            let angle = 2 * CGFloat.pi * CGFloat(index) / CGFloat(segments)
            points.append(
                CGPoint(
                    x: min(max(cx + rx * cos(angle), 0), 1),
                    y: min(max(cy + ry * sin(angle), 0), 1)
                )
            )
        }
        return points
    }

    /// RGBA8 缓冲 → CGImage（与渲染用的同一缓冲，坐标一一对应）。
    static func makeCGImage(buffer: RgbaBuffer) -> CGImage? {
        let colorSpace = CGColorSpaceCreateDeviceRGB()
        let data = Data(buffer.pixels)
        guard let provider = CGDataProvider(data: data as CFData) else { return nil }
        return CGImage(
            width: buffer.width,
            height: buffer.height,
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: buffer.width * 4,
            space: colorSpace,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
            provider: provider,
            decode: nil,
            shouldInterpolate: true,
            intent: .defaultIntent
        )
    }

    /// CVPixelBuffer（8 位单通道或 32 位浮点单通道）→ 目标尺寸 f32 平面。
    private static func pixelBufferToPlane(
        _ pixelBuffer: CVPixelBuffer,
        targetWidth: Int,
        targetHeight: Int
    ) -> Outcome? {
        CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }
        let width = CVPixelBufferGetWidth(pixelBuffer)
        let height = CVPixelBufferGetHeight(pixelBuffer)
        guard width > 0, height > 0, let base = CVPixelBufferGetBaseAddress(pixelBuffer) else {
            return nil
        }
        let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
        let format = CVPixelBufferGetPixelFormatType(pixelBuffer)
        let isFloat = format == kCVPixelFormatType_OneComponent32Float

        // 双线性缩放到目标尺寸（源与目标归一化坐标对应）。
        var data = [Float](repeating: 0, count: targetWidth * targetHeight)
        for y in 0..<targetHeight {
            let fy = (Float(y) + 0.5) * Float(height) / Float(targetHeight) - 0.5
            let y0 = max(0, min(height - 1, Int(fy.rounded(.down))))
            let y1 = min(height - 1, y0 + 1)
            let ty = max(0, min(1, fy - Float(y0)))
            for x in 0..<targetWidth {
                let fx = (Float(x) + 0.5) * Float(width) / Float(targetWidth) - 0.5
                let x0 = max(0, min(width - 1, Int(fx.rounded(.down))))
                let x1 = min(width - 1, x0 + 1)
                let tx = max(0, min(1, fx - Float(x0)))

                func sample(_ px: Int, _ py: Int) -> Float {
                    if isFloat {
                        let row = base.advanced(by: py * bytesPerRow).assumingMemoryBound(to: Float.self)
                        return row[px]
                    }
                    let row = base.advanced(by: py * bytesPerRow).assumingMemoryBound(to: UInt8.self)
                    return Float(row[px]) / 255.0
                }
                let top = sample(x0, y0) * (1 - tx) + sample(x1, y0) * tx
                let bottom = sample(x0, y1) * (1 - tx) + sample(x1, y1) * tx
                data[y * targetWidth + x] = max(0, min(1, top * (1 - ty) + bottom * ty))
            }
        }
        return .plane(width: targetWidth, height: targetHeight, data: data)
    }
}

/// 多边形 JSON（与核心 `fdxmp_mask_polygons_submit` 对齐）。
enum SmartMaskPolygons {
    /// 生成 `{"include": [[[x, y], ...]], "exclude": [...]}` 文本。
    static func json(include: [[CGPoint]], exclude: [[CGPoint]]) -> String {
        func polygonJson(_ points: [CGPoint]) -> String {
            let items = points.map { point in
                let x = (Double(point.x) * 100000).rounded() / 100000
                let y = (Double(point.y) * 100000).rounded() / 100000
                return "[\(x),\(y)]"
            }
            return "[\(items.joined(separator: ","))]"
        }
        let includeText = include.map(polygonJson).joined(separator: ",")
        let excludeText = exclude.map(polygonJson).joined(separator: ",")
        return "{\"include\":[\(includeText)],\"exclude\":[\(excludeText)]}"
    }
}
