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

    /// 高光剪裁比例（任一通道 ≥ 254 的采样占比，0..1）。
    var highlightClipping: Float {
        clippedRatio(channels: [red, green, blue], bins: 254...255)
    }

    /// 阴影剪裁比例（任一通道 ≤ 1 的采样占比，0..1）。
    var shadowClipping: Float {
        clippedRatio(channels: [red, green, blue], bins: 0...1)
    }

    /// 是否有值得提示的剪裁（0.05% 以上）。
    var hasClipping: Bool {
        highlightClipping > 0.0005 || shadowClipping > 0.0005
    }

    private func clippedRatio(channels: [[Int]], bins: ClosedRange<Int>) -> Float {
        var clipped = 0
        var total = 0
        for channel in channels where channel.count == 256 {
            for index in bins {
                clipped += channel[index]
            }
            total += channel.reduce(0, +)
        }
        guard total > 0 else { return 0 }
        return Float(clipped) / Float(total)
    }
}

/// 直方图显示通道。
enum HistogramMode: String, CaseIterable {
    case rgb = "RGB"
    case luma = "亮度"
    case red = "红"
    case green = "绿"
    case blue = "蓝"

    /// 点一下切下一个通道。
    var next: HistogramMode {
        let all = HistogramMode.allCases
        let index = all.firstIndex(of: self) ?? 0
        return all[(index + 1) % all.count]
    }
}

/// Lightroom 风格的直方图叠加卡片。
///
/// 点一下：切换 RGB / 亮度 / 单通道；长按 0.5s：显示 / 隐藏剪裁警告。
struct HistogramOverlay: View {
    let data: HistogramData?

    @State private var mode: HistogramMode = .rgb
    @State private var showClipping = true

    var body: some View {
        Canvas { context, size in
            guard let data, !data.red.isEmpty else { return }
            drawChannels(&context, size: size, data: data)
            if showClipping {
                drawClippingWarnings(&context, size: size, data: data)
            }
            // 通道标签（左下角小字）。
            context.draw(
                Text(mode.rawValue)
                    .font(.system(size: 9))
                    .foregroundColor(Color.white.opacity(0.55)),
                at: CGPoint(x: 18, y: size.height - 9)
            )
        }
        .background(Color.black.opacity(0.55))
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .contentShape(Rectangle())
        .onTapGesture {
            mode = mode.next
            Haptics.tick()
        }
        .onLongPressGesture(minimumDuration: 0.5) {
            showClipping.toggle()
            Haptics.light()
        }
    }

    /// 按当前模式画通道。
    private func drawChannels(_ context: inout GraphicsContext, size: CGSize, data: HistogramData) {
        let red = Color(red: 1.0, green: 0.27, blue: 0.27)
        let green = Color(red: 0.27, green: 0.87, blue: 0.4)
        let blue = Color(red: 0.27, green: 0.53, blue: 1.0)
        switch mode {
        case .rgb:
            drawChannel(&context, size: size, bins: data.luma, color: .white.opacity(0.18), fill: true)
            drawChannel(&context, size: size, bins: data.blue, color: blue.opacity(0.85), fill: false)
            drawChannel(&context, size: size, bins: data.green, color: green.opacity(0.85), fill: false)
            drawChannel(&context, size: size, bins: data.red, color: red.opacity(0.85), fill: false)
        case .luma:
            drawChannel(&context, size: size, bins: data.luma, color: .white.opacity(0.7), fill: true)
        case .red:
            drawChannel(&context, size: size, bins: data.red, color: red.opacity(0.85), fill: true)
        case .green:
            drawChannel(&context, size: size, bins: data.green, color: green.opacity(0.85), fill: true)
        case .blue:
            drawChannel(&context, size: size, bins: data.blue, color: blue.opacity(0.85), fill: true)
        }
    }

    /// 剪裁警告：左上蓝三角 = 阴影剪裁，右上红三角 = 高光剪裁（LR 习惯）。
    private func drawClippingWarnings(
        _ context: inout GraphicsContext,
        size: CGSize,
        data: HistogramData
    ) {
        let side: CGFloat = 7
        let highlight = data.highlightClipping > 0.0005
        let shadow = data.shadowClipping > 0.0005

        var left = Path()
        left.move(to: CGPoint(x: 3, y: 3))
        left.addLine(to: CGPoint(x: 3 + side, y: 3))
        left.addLine(to: CGPoint(x: 3, y: 3 + side))
        left.closeSubpath()
        context.fill(
            left,
            with: .color(shadow ? Color(red: 0.35, green: 0.62, blue: 1.0) : .white.opacity(0.15))
        )

        var right = Path()
        right.move(to: CGPoint(x: size.width - 3, y: 3))
        right.addLine(to: CGPoint(x: size.width - 3 - side, y: 3))
        right.addLine(to: CGPoint(x: size.width - 3, y: 3 + side))
        right.closeSubpath()
        context.fill(
            right,
            with: .color(highlight ? Color(red: 1.0, green: 0.3, blue: 0.3) : .white.opacity(0.15))
        )

        if shadow, data.shadowClipping > 0.005 {
            context.draw(
                Text("\(Int((data.shadowClipping * 100).rounded()))%")
                    .font(.system(size: 8).monospacedDigit())
                    .foregroundColor(Color.white.opacity(0.7)),
                at: CGPoint(x: 24, y: 9)
            )
        }
        if highlight, data.highlightClipping > 0.005 {
            context.draw(
                Text("\(Int((data.highlightClipping * 100).rounded()))%")
                    .font(.system(size: 8).monospacedDigit())
                    .foregroundColor(Color.white.opacity(0.7)),
                at: CGPoint(x: size.width - 24, y: 9)
            )
        }
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
