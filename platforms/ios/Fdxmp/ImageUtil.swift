import CoreGraphics
import UIKit

/// 计算图像在给定区域内 aspect-fit 后的显示矩形（居中）。
func fittedRect(imageSize: CGSize, in bounds: CGSize) -> CGRect {
    guard imageSize.width > 0, imageSize.height > 0,
          bounds.width > 0, bounds.height > 0
    else {
        return CGRect(origin: .zero, size: bounds)
    }
    let scale = min(bounds.width / imageSize.width, bounds.height / imageSize.height)
    let size = CGSize(width: imageSize.width * scale, height: imageSize.height * scale)
    return CGRect(
        x: (bounds.width - size.width) / 2,
        y: (bounds.height - size.height) / 2,
        width: size.width,
        height: size.height
    )
}

/// 行优先 RGBA8 像素缓冲区。
struct RgbaBuffer {
    var pixels: [UInt8]
    var width: Int
    var height: Int
}

/// UIImage ⇄ RGBA8 转换（sRGB、premultipliedLast、byteOrder32Big）。
enum ImageUtil {
    /// 把 UIImage 规范化（方向 / 尺寸）并取出 RGBA8 像素。
    static func rgbaBuffer(from image: UIImage, maxEdge: Int) -> RgbaBuffer? {
        let normalized = redraw(image, maxEdge: maxEdge)
        guard let cgImage = normalized.cgImage else { return nil }
        let width = cgImage.width
        let height = cgImage.height
        guard width > 0, height > 0 else { return nil }

        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        let colorSpace = CGColorSpace(name: CGColorSpace.sRGB)
            ?? CGColorSpaceCreateDeviceRGB()
        let bitmapInfo = CGImageAlphaInfo.premultipliedLast.rawValue
            | CGBitmapInfo.byteOrder32Big.rawValue
        var drawn = false
        pixels.withUnsafeMutableBytes { raw in
            guard let context = CGContext(
                data: raw.baseAddress,
                width: width,
                height: height,
                bitsPerComponent: 8,
                bytesPerRow: width * 4,
                space: colorSpace,
                bitmapInfo: bitmapInfo
            ) else { return }
            context.interpolationQuality = .high
            context.draw(cgImage, in: CGRect(x: 0, y: 0, width: width, height: height))
            drawn = true
        }
        guard drawn else { return nil }
        return RgbaBuffer(pixels: pixels, width: width, height: height)
    }

    /// RGBA8 像素转 UIImage（与 `rgbaBuffer` 使用同一像素格式）。
    static func image(from buffer: RgbaBuffer) -> UIImage? {
        let data = Data(buffer.pixels)
        guard let provider = CGDataProvider(data: data as CFData) else { return nil }
        let colorSpace = CGColorSpace(name: CGColorSpace.sRGB)
            ?? CGColorSpaceCreateDeviceRGB()
        let bitmapInfo = CGBitmapInfo(
            rawValue: CGImageAlphaInfo.premultipliedLast.rawValue
                | CGBitmapInfo.byteOrder32Big.rawValue
        )
        guard let cgImage = CGImage(
            width: buffer.width,
            height: buffer.height,
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: buffer.width * 4,
            space: colorSpace,
            bitmapInfo: bitmapInfo,
            provider: provider,
            decode: nil,
            shouldInterpolate: true,
            intent: .defaultIntent
        ) else { return nil }
        return UIImage(cgImage: cgImage)
    }

    /// 按最长边缩放并规范化方向（绘制一次同时完成方向校正与缩放）。
    private static func redraw(_ image: UIImage, maxEdge: Int) -> UIImage {
        let pixelWidth = image.size.width * image.scale
        let pixelHeight = image.size.height * image.scale
        let longest = max(pixelWidth, pixelHeight)
        let scale: CGFloat = longest > CGFloat(maxEdge)
            ? CGFloat(maxEdge) / longest
            : 1
        let target = CGSize(
            width: max(1, (pixelWidth * scale).rounded()),
            height: max(1, (pixelHeight * scale).rounded())
        )
        let format = UIGraphicsImageRendererFormat.default()
        format.scale = 1
        format.opaque = false
        format.preferredRange = .standard
        return UIGraphicsImageRenderer(size: target, format: format).image { _ in
            image.draw(in: CGRect(origin: .zero, size: target))
        }
    }
}
