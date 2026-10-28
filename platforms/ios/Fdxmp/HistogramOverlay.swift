import SwiftUI

/// 直方图数据：R / G / B / 亮度，各 256 个桶；外加 RGB 波形（每列 × 每桶）。
struct HistogramData: Equatable {
    var red: [Int]
    var green: [Int]
    var blue: [Int]
    var luma: [Int]
    /// 波形：`[通道][列][桶]`，通道顺序 R/G/B，列 256、桶 128。
    /// GPU 统计路径拿不到列信息，这时为空数组，波形样式回退成填充样式。
    var waveform: [UInt16]

    init(
        red: [Int] = [],
        green: [Int] = [],
        blue: [Int] = [],
        luma: [Int] = [],
        waveform: [UInt16] = []
    ) {
        self.red = red
        self.green = green
        self.blue = blue
        self.luma = luma
        self.waveform = waveform
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
        var waveform = [UInt16](repeating: 0, count: 3 * 256 * 128)
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

            let column = (index % width) * 256 / width
            let base = column * 128
            let plane = 256 * 128
            waveform[base + r / 2] += 1
            waveform[plane + base + g / 2] += 1
            waveform[2 * plane + base + b / 2] += 1

            index += step
        }
        self.red = red
        self.green = green
        self.blue = blue
        self.luma = luma
        self.waveform = waveform
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

/// 直方图显示样式（点一下切换）。
enum HistogramStyle: String, CaseIterable {
    case filled = "填充"
    case waveform = "波形"

    var next: HistogramStyle {
        let all = HistogramStyle.allCases
        let index = all.firstIndex(of: self) ?? 0
        return all[(index + 1) % all.count]
    }
}

/// 直方图叠加卡片。
///
/// 点一下：切换填充 / RGB 波形；长按 0.5s：显示 / 隐藏剪裁警告。
struct HistogramOverlay: View {
    let data: HistogramData?

    @State private var style: HistogramStyle = .filled
    @State private var showClipping = true

    private let redColor = Color(red: 1.0, green: 0.27, blue: 0.27)
    private let greenColor = Color(red: 0.27, green: 0.87, blue: 0.4)
    private let blueColor = Color(red: 0.27, green: 0.53, blue: 1.0)

    var body: some View {
        Canvas { context, size in
            guard let data, !data.red.isEmpty else { return }
            if style == .waveform, data.waveform.count == 3 * 256 * 128 {
                drawWaveform(&context, size: size, data: data)
            } else {
                drawFilled(&context, size: size, data: data)
            }
            if showClipping {
                drawClippingWarnings(&context, size: size, data: data)
            }
            // 样式标签（左下角小字）。
            context.draw(
                Text(style.rawValue)
                    .font(.system(size: 9))
                    .foregroundColor(Color.white.opacity(0.55)),
                at: CGPoint(x: 18, y: size.height - 9)
            )
        }
        // 半透明纯色底（不要毛玻璃）：透出后面的画面，又不至于看不清直方图。
        .background(Color.black.opacity(0.30), in: RoundedRectangle(cornerRadius: 8))
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .stroke(Color.white.opacity(0.10), lineWidth: 0.5)
        )
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .contentShape(Rectangle())
        .onTapGesture {
            style = style.next
            Haptics.tick()
        }
        .onLongPressGesture(minimumDuration: 0.5) {
            showClipping.toggle()
            Haptics.light()
        }
    }

    /// 填充样式：半透明底 + 三通道半透明色块 + 亮度白线。
    private func drawFilled(_ context: inout GraphicsContext, size: CGSize, data: HistogramData) {
        context.fill(
            Path(CGRect(origin: .zero, size: size)),
            with: .color(Color.black.opacity(0.22))
        )
        drawChannel(&context, size: size, bins: data.red, color: redColor.opacity(0.55), fill: true)
        drawChannel(&context, size: size, bins: data.green, color: greenColor.opacity(0.55), fill: true)
        drawChannel(&context, size: size, bins: data.blue, color: blueColor.opacity(0.55), fill: true)
        drawChannel(&context, size: size, bins: data.luma, color: .white.opacity(0.8), fill: false)
    }

    /// 波形样式：横轴画面列、纵轴亮度，颜色是各通道在该列该亮度上的占比。
    private func drawWaveform(_ context: inout GraphicsContext, size: CGSize, data: HistogramData) {
        context.fill(
            Path(CGRect(origin: .zero, size: size)),
            with: .color(Color.black.opacity(0.30))
        )
        let columns = 256
        let bins = 128
        let cellWidth = size.width / CGFloat(columns)
        let cellHeight = size.height / CGFloat(bins)
        let colors = [redColor, greenColor, blueColor]

        for channel in 0..<3 {
            let plane = channel * columns * bins
            for column in 0..<columns {
                let base = plane + column * bins
                var peak = 1
                for bin in 0..<bins {
                    peak = max(peak, Int(data.waveform[base + bin]))
                }
                let inverse = 1.0 / Double(peak)
                var runStart: Int?
                for bin in 0...bins {
                    var lit = false
                    if bin < bins {
                        let intensity = (Double(data.waveform[base + bin]) * inverse).squareRoot()
                        lit = intensity > 0.06
                    }
                    if lit, runStart == nil { runStart = bin }
                    if !lit, let start = runStart {
                        let top = size.height - CGFloat(bin) * cellHeight
                        let rect = CGRect(
                            x: CGFloat(column) * cellWidth,
                            y: top,
                            width: cellWidth + 0.5,
                            height: CGFloat(bin - start) * cellHeight + 0.5
                        )
                        context.fill(Path(rect), with: .color(colors[channel].opacity(0.5)))
                        runStart = nil
                    }
                }
            }
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
