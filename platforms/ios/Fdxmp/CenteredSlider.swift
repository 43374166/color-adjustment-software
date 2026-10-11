import Foundation
import SwiftUI

/// 正在横向拖动的滑块信息（标签 + 显示数值）——
/// 面板据此进入沉浸模式，画面顶部的数值浮层也用它。
struct SliderDragInfo: Equatable {
    var label: String
    var text: String
}

/// 拖动态上报（Lightroom 式沉浸：拖动时只留这一个滑块 + 顶部数值）。
struct SliderDragPreferenceKey: PreferenceKey {
    static let defaultValue: SliderDragInfo? = nil

    static func reduce(value: inout SliderDragInfo?, nextValue: () -> SliderDragInfo?) {
        if let next = nextValue() {
            value = next
        }
    }
}

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
    /// 是否正在横向拖动（点击步进不算）。
    @State private var dragging = false
    /// 轨道渐变配色（如色温蓝→黄、色调绿→洋红）；nil = 纯色轨道。
    var trackColors: [Color]? = nil

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
                onCommit: onCommit,
                onDraggingChange: { dragging = $0 },
                trackColors: trackColors
            )
            .padding(.horizontal, 18)
        }
        // 两行滑块之间收紧一点（少占画面）。
        .padding(.vertical, 1)
        // 上报拖动状态 + 实时数值：面板隐藏其它 UI，画面顶部显示大号数值。
        .preference(
            key: SliderDragPreferenceKey.self,
            value: dragging ? SliderDragInfo(label: label, text: text) : nil
        )
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
    /// 横向拖动（>8pt）开始 / 结束回调；点击步进不触发。
    var onDraggingChange: ((Bool) -> Void)? = nil
    /// 轨道渐变配色（色温 / 色调 / 饱和度 / 色相等）；nil = 纯色轨道。
    var trackColors: [Color]? = nil

    @State private var gestureActive = false
    @State private var didDrag = false
    @State private var maxDistance: CGFloat = 0
    @State private var lastTapTime = Date.distantPast
    /// 长按进入微调（拖动速度降到 1/5）。
    @State private var fineMode = false
    @State private var fineAnchorX: CGFloat?
    @State private var fineAnchorValue: Float = 0
    /// 本次触摸的方向判定（只在越过阈值时决定一次）：
    /// 横向 = 调这个滑块，纵向 = 翻面板找参数，两者互不干扰。
    @State private var dragAxis: Axis?

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
                if let trackColors, trackColors.count >= 2 {
                    // 带颜色区分的轨道（色温 / 色调 / 饱和度 / 色相…）。
                    Capsule()
                        .fill(
                            LinearGradient(
                                colors: trackColors,
                                startPoint: .leading,
                                endPoint: .trailing
                            )
                        )
                        .frame(width: width, height: 5)
                        .offset(y: 18.5)
                        .opacity(0.9)
                } else {
                    Capsule()
                        .fill(Color(white: 0.24))
                        .frame(width: width, height: 5)
                        .offset(y: 18.5)
                }
                // 进度（从中性点画到当前值）
                if abs(thumbX - neutralX) > 0.5 {
                    Capsule()
                        .fill(Color.accentColor)
                        .frame(width: abs(thumbX - neutralX), height: 5)
                        .offset(x: min(thumbX, neutralX), y: 18.5)
                }
                // 初始化（中性点）标记：很小的白色圆点，压在轨道中间。
                Circle()
                    .fill(Color.white.opacity(0.9))
                    .frame(width: 6, height: 6)
                    .offset(x: neutralX - 3, y: 18)
                // 圆点：外圈白色、内点浅蓝；内圈整圈填浅蓝，不再露出黑底。
                Circle()
                    .fill(Color(red: 0.45, green: 0.72, blue: 1.0).opacity(0.55))
                    .frame(width: 26, height: 26)
                    .overlay(
                        Circle()
                            .fill(Color(red: 0.45, green: 0.72, blue: 1.0).opacity(0.9))
                            .frame(width: 15, height: 15)
                    )
                    .overlay(
                        // 外圈白色（和 Lightroom 的圆点一致）。
                        Circle().strokeBorder(Color.white.opacity(0.95), lineWidth: 2.5)
                    )
                    .offset(x: thumbX - 13, y: 8)
            }
            .frame(width: width, height: 42)
            .contentShape(Rectangle())
            // 用 simultaneousGesture：纵向滑动交给外层 ScrollView 滚动，
            // 只有明显的横向拖动才调整数值（否则面板无法上下滚动）。
            .simultaneousGesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { gesture in
                        if !gestureActive {
                            gestureActive = true
                            didDrag = false
                            maxDistance = 0
                            dragAxis = nil
                            onBegin()
                        }
                        maxDistance = max(
                            maxDistance,
                            hypot(gesture.translation.width, gesture.translation.height)
                        )
                        // 长按后进入微调：只按相对位移慢慢走。
                        if fineMode {
                            if fineAnchorX == nil {
                                fineAnchorX = gesture.location.x
                                fineAnchorValue = value
                                if !didDrag {
                                    didDrag = true
                                    onDraggingChange?(true)
                                }
                            }
                            let delta = (gesture.location.x - (fineAnchorX ?? gesture.location.x)) / width
                            let target = (fineAnchorValue + Float(delta) * span * 0.2)
                                .clamped(to: range)
                            if target != value {
                                onChange(target)
                            }
                            return
                        }
                        // 方向判定只在越过阈值时做一次：横向 = 调这个滑块，
                        // 纵向 = 翻面板找参数。中途不再改变判定——滑动不会突然失效，
                        // 也不会滑到一半顺带把面板滚起来。
                        if dragAxis == nil, maxDistance > 9 {
                            dragAxis = abs(gesture.translation.width) > abs(gesture.translation.height)
                                ? .horizontal
                                : .vertical
                        }
                        guard dragAxis == .horizontal else { return }
                        if !didDrag {
                            didDrag = true
                            onDraggingChange?(true)
                            Haptics.prepare()
                        }
                        let ratio = (gesture.location.x / width).clamped(to: 0...1)
                        let target = range.lowerBound + Float(ratio) * span
                        let clamped = target.clamped(to: range)
                        // 跨过中性点给一次"咔哒"（LR 手感）。
                        if value != neutral, (value - neutral) * (clamped - neutral) <= 0 {
                            Haptics.tick()
                        }
                        onChange(clamped)
                    }
                    .onEnded { gesture in
                        guard gestureActive else { return }
                        gestureActive = false
                        let axis = dragAxis
                        dragAxis = nil
                        if fineMode {
                            fineMode = false
                            fineAnchorX = nil
                            if didDrag {
                                didDrag = false
                                onDraggingChange?(false)
                                onCommit()
                            }
                            return
                        }
                        if axis == .vertical {
                            // 判定为翻面板滚动：本次触摸不调值、不步进。
                            return
                        }
                        if didDrag {
                            didDrag = false
                            onDraggingChange?(false)
                            onCommit()
                            return
                        }
                        // 移动距离过大（纵向滚动）时不当作点击。
                        guard maxDistance < 8 else { return }
                        // 轻点：
                        // - 单击 → 立即执行 ±step（不再等 0.3s 判定双击，点击没有延迟）；
                        // - 双击（0.3s 内的第二击，且落在圆点附近）→ 归零（默认值），
                        //   覆盖第一击的步进；双击在轨道上 → 再走一步。
                        let now = Date()
                        let isDoubleTap = now.timeIntervalSince(lastTapTime) < 0.30
                        let onThumb = abs(gesture.startLocation.x - thumbX) <= 30
                        if isDoubleTap {
                            lastTapTime = .distantPast
                            if onThumb {
                                Haptics.light()
                                onChange(neutral.clamped(to: range))
                                onCommit()
                            } else {
                                applyStep(from: gesture.startLocation.x, thumbX: thumbX)
                            }
                        } else {
                            lastTapTime = now
                            applyStep(from: gesture.startLocation.x, thumbX: thumbX)
                        }
                    }
            )
            // 按住 0.4s 进入微调（拖动速度 1/5，用于精细对数值）。
            .simultaneousGesture(
                LongPressGesture(minimumDuration: 0.4, maximumDistance: 12)
                    .onEnded { _ in
                        guard !fineMode else { return }
                        fineMode = true
                        fineAnchorX = nil
                        Haptics.light()
                    }
            )
        }
        .frame(height: 42)
    }

    /// 单击步进：圆点左侧 -step，右侧 +step。
    private func applyStep(from x: CGFloat, thumbX: CGFloat) {
        let delta: Float = x >= thumbX ? step : -step
        let next = (value + delta).clamped(to: range)
        if next != value {
            Haptics.tick()
            onChange(next)
            onCommit()
        }
    }

    private func position(for target: Float, width: CGFloat, span: Float) -> CGFloat {
        CGFloat(((target - range.lowerBound) / span).clamped(to: 0...1)) * width
    }
}
