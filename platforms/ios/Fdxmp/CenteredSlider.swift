import SwiftUI

/// 通用数值滑块行（标签 + 数值 + 中性点滑块），用于 HSL / 分级 / 参数曲线等面板。
struct ValueSliderRow: View {
    let label: String
    let value: Float
    let range: ClosedRange<Float>
    var neutral: Float = 0
    var step: Float = 5
    var decimals: Int = 0
    let onBegin: () -> Void
    let onChange: (Float) -> Void
    let onCommit: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text(label)
                    .font(.caption)
                Spacer()
                Text(text)
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 18)
            CenteredSlider(
                value: value,
                range: range,
                neutral: neutral,
                step: step,
                onBegin: onBegin,
                onChange: onChange,
                onCommit: onCommit
            )
            .padding(.horizontal, 18)
        }
        .padding(.vertical, 3)
    }

    private var text: String {
        let formatted: String
        switch decimals {
        case 0: formatted = String(format: "%.0f", value)
        case 1: formatted = String(format: "%.1f", value)
        default: formatted = String(format: "%.2f", value)
        }
        if value > neutral, neutral == 0, decimals == 0 {
            return "+\(formatted)"
        }
        return formatted
    }
}

/// 中性点锚定的滑块（Lightroom 风格）：
///
/// - 进度条从中性值画到当前值，等于中性值时不显示进度；
/// - 点击圆点右侧一次 +step，左侧一次 -step；
/// - 拖动连续调整；双击复位到默认值。
struct CenteredSlider: View {
    let value: Float
    let range: ClosedRange<Float>
    var neutral: Float = 0
    var step: Float = 5
    let onBegin: () -> Void
    let onChange: (Float) -> Void
    let onCommit: () -> Void

    @State private var gestureStartValue: Float?
    @State private var didDrag = false
    @State private var lastTapTime = Date.distantPast

    var body: some View {
        GeometryReader { geo in
            let width = max(geo.size.width, 1)
            let span = max(range.upperBound - range.lowerBound, 1e-6)
            let thumbX = position(for: value, width: width, span: span)
            let neutralX = position(
                for: neutral.clamped(to: range),
                width: width,
                span: span
            )
            ZStack(alignment: .topLeading) {
                // 轨道
                Capsule()
                    .fill(Color(white: 0.24))
                    .frame(width: width, height: 5)
                    .offset(y: 18.5)
                // 中性点刻度
                Rectangle()
                    .fill(Color.white.opacity(0.4))
                    .frame(width: 2, height: 18)
                    .offset(x: neutralX - 1, y: 12)
                // 进度（从中性点画到当前值）
                if abs(thumbX - neutralX) > 0.5 {
                    Capsule()
                        .fill(Color.accentColor)
                        .frame(width: abs(thumbX - neutralX), height: 5)
                        .offset(x: min(thumbX, neutralX), y: 18.5)
                }
                // 圆点
                Circle()
                    .fill(Color(white: 0.06))
                    .frame(width: 26, height: 26)
                    .overlay(
                        Circle()
                            .fill(Color.accentColor)
                            .frame(width: 19, height: 19)
                    )
                    .offset(x: thumbX - 13, y: 8)
            }
            .frame(width: width, height: 42)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { gesture in
                        if gestureStartValue == nil {
                            gestureStartValue = value
                            didDrag = false
                            onBegin()
                        }
                        if abs(gesture.translation.width) > 10 {
                            didDrag = true
                        }
                        guard didDrag else { return }
                        let ratio = (gesture.location.x / width).clamped(to: 0...1)
                        let target = range.lowerBound + Float(ratio) * span
                        onChange(target.clamped(to: range))
                    }
                    .onEnded { gesture in
                        gestureStartValue = nil
                        if didDrag {
                            didDrag = false
                            onCommit()
                            return
                        }
                        // 轻点：双击复位，单击按圆点位置 ±step。
                        let now = Date()
                        let doubleTap = now.timeIntervalSince(lastTapTime) < 0.32
                        lastTapTime = now
                        if doubleTap {
                            onChange(neutral.clamped(to: range))
                        } else {
                            let delta: Float = gesture.startLocation.x >= thumbX ? step : -step
                            let next = (value + delta).clamped(to: range)
                            if next != value {
                                onChange(next)
                            }
                        }
                        onCommit()
                    }
            )
        }
        .frame(height: 42)
    }

    private func position(for target: Float, width: CGFloat, span: Float) -> CGFloat {
        CGFloat(((target - range.lowerBound) / span).clamped(to: 0...1)) * width
    }
}
