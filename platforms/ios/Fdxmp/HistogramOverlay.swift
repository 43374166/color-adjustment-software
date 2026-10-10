import SwiftUI

/// 直方图数据：R / G / B / 亮度，各 256 个桶。
struct HistogramData: Equatable {
    var red: [Int]
    var green: [Int]
    var blue: [Int]
    var luma: [Int]

    init(red: [Int] = [], green: [Int] = [], blue: [Int] = [], luma: [Int] = []) {
        self.red = red
        self.green = green
        self.blue = blue
        self.luma = luma
    }

    /// 从 RGBA8 像素采样计算（最多约 25 万采样点）。
    init?(pixels: [UInt8], width: Int, height: Int) {
        guard width > 0, height > 0, pixels.count >= width * height * 4 else {
            return nil
        }
        var red = [Int](repeating: 0, count: 256)
        var green = [Int](repeating: 0, count: 256)
        var blue = [Int](repeating: 0, count: 256)
        var luma = [Int](repeating: 0, count: 256)
        let count = width * height
        let step = max(1, count / 250_000)
        var index = 0
        while index < count {
            let offset = index * 4
            let r = Int(pixels[offset])
            let g = Int(pixels[offset + 1])
            let b = Int(pixels[offset + 2])
            red[r] += 1
            green[g] += 1
            blue[b] += 1
            let l = (r * 2126 + g * 7152 + b * 722) / 10_000
            luma[min(max(l, 0), 255)] += 1
            index += step
        }
        self.red = red
        self.green = green
        self.blue = blue
        self.luma = luma
    }
}

/// Lightroom 风格的直方图叠加卡片。
struct HistogramOverlay: View {
    let data: HistogramData?

    var body: some View {
        Canvas { context, size in
            guard let data, !data.red.isEmpty else { return }
            drawChannel(
                &context, size: size, bins: data.luma,
                color: Color.white.opacity(0.18), fill: true
            )
            drawChannel(
                &context, size: size, bins: data.blue,
                color: Color(red: 0.27, green: 0.53, blue: 1.0).opacity(0.85),
                fill: false
            )
            drawChannel(
                &context, size: size, bins: data.green,
                color: Color(red: 0.27, green: 0.87, blue: 0.4).opacity(0.85),
                fill: false
            )
            drawChannel(
                &context, size: size, bins: data.red,
                color: Color(red: 1.0, green: 0.27, blue: 0.27).opacity(0.85),
                fill: false
            )
        }
        .background(Color.black.opacity(0.55))
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }

    private func drawChannel(
        _ context: inout GraphicsContext,
        size: CGSize,
        bins: [Int],
        color: Color,
        fill: Bool
    ) {
        guard size.width > 0, size.height > 0, bins.count > 1 else { return }
        let peak = max(bins.max() ?? 1, 1)
        var path = Path()
        path.move(to: CGPoint(x: 0, y: size.height))
        for (index, bin) in bins.enumerated() {
            let x = size.width * CGFloat(index) / CGFloat(bins.count - 1)
            let normalized = sqrt(Double(bin) / Double(peak))
            let y = size.height * (1 - CGFloat(normalized))
            path.addLine(to: CGPoint(x: x, y: y))
        }
        path.addLine(to: CGPoint(x: size.width, y: size.height))
        path.closeSubpath()
        if fill {
            context.fill(path, with: .color(color))
        } else {
            context.stroke(path, with: .color(color), lineWidth: 1.5)
        }
    }
}
