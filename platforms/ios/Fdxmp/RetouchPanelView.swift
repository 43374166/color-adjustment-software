import SwiftUI

/// AI 修图全屏加载动画（识别五官 / 去瑕疵 / 磨皮 / 肤色 / 塑形）。
struct RetouchProgressOverlay: View {
    let text: String

    @State private var pulse = false

    var body: some View {
        ZStack {
            Color.black.opacity(0.45)
                .ignoresSafeArea()
            VStack(spacing: 14) {
                ZStack {
                    Circle()
                        .stroke(Color.accentColor.opacity(0.25), lineWidth: 4)
                        .frame(width: 56, height: 56)
                    Circle()
                        .trim(from: 0, to: 0.3)
                        .stroke(
                            Color.accentColor,
                            style: StrokeStyle(lineWidth: 4, lineCap: .round)
                        )
                        .frame(width: 56, height: 56)
                        .rotationEffect(.degrees(pulse ? 360 : 0))
                        .animation(
                            .linear(duration: 1.0).repeatForever(autoreverses: false),
                            value: pulse
                        )
                }
                .onAppear { pulse = true }
                Text("AI 修图")
                    .font(.subheadline.weight(.semibold))
                Text(text)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .contentTransition(.opacity)
            }
            .padding(26)
            .background(
                RoundedRectangle(cornerRadius: 18)
                    .fill(Color(white: 0.12).opacity(0.95))
            )
            .overlay(
                RoundedRectangle(cornerRadius: 18)
                    .stroke(Color.white.opacity(0.08))
            )
        }
        .allowsHitTesting(true)
    }
}

/// 修图面板（v1.5.0）：一键修图 + 分组滑杆（去瑕疵 / 磨皮 / 肤色 / 塑形）。
///
/// 修图只改像素（在核心跑），与「编辑」Tab 的调色互不影响，可随时切 Tab 交替使用。
struct RetouchPanelView: View {
    @ObservedObject var model: EditorModel

    var body: some View {
        VStack(spacing: 0) {
            oneTapRow
            Divider()
                .overlay(Color.white.opacity(0.08))
            sliders
        }
    }

    // MARK: - 一键修图

    private var oneTapRow: some View {
        VStack(spacing: 6) {
            Button {
                model.oneTapRetouch()
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: model.retouchAppliedReady ? "sparkles" : "wand.and.rays")
                    Text(model.retouchAppliedReady ? "重新一键修图" : "一键修图")
                        .font(.subheadline.weight(.semibold))
                    if model.retouchAppliedReady {
                        Text("已修图")
                            .font(.caption2)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(
                                Capsule().fill(Color.green.opacity(0.25))
                            )
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 10)
                .background(
                    RoundedRectangle(cornerRadius: 12)
                        .fill(Color.accentColor.opacity(model.retouchStage == nil ? 0.22 : 0.1))
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 12)
                        .stroke(Color.accentColor.opacity(0.5))
                )
            }
            .buttonStyle(.plain)
            .disabled(model.retouchStage != nil)
            .opacity(model.retouchStage == nil ? 1 : 0.6)

            if let analysis = model.retouchAnalysis {
                HStack(spacing: 6) {
                    Image(systemName: analysis.hasFace ? "checkmark.circle" : "exclamationmark.circle")
                        .font(.caption2)
                        .foregroundStyle(analysis.hasFace ? Color.green : Color.orange)
                    Text(statusText(analysis))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                Text("自动识别五官 → 去瑕疵 / 磨皮 / 肤色 / 塑形；识别时会显示加载动画")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(.horizontal, 14)
        .padding(.top, 10)
        .padding(.bottom, 6)
        .opacity(model.draggingSlider == nil ? 1 : 0)
        .allowsHitTesting(model.draggingSlider == nil)
    }

    private func statusText(_ analysis: RetouchAnalysisUi) -> String {
        if !analysis.hasFace {
            return "未检测到人脸：磨皮 / 去瑕疵 / 肤色按全图处理"
        }
        if !analysis.hasLandmarks {
            return "已识别人脸（五官关键点不完整，已跳过塑形）"
        }
        return "已识别五官 478 点：塑形 / 磨皮 / 肤色全部可用"
    }

    // MARK: - 滑杆

    private var sliders: some View {
        ScrollView {
            LazyVStack(spacing: 0) {
                sectionTitle("皮肤")
                retouchSlider(
                    "去瑕疵",
                    value: model.state.retouch.blemish,
                    neutral: 0
                ) { $0.blemish = $1 }
                retouchSlider(
                    "磨皮 · 低频（光影）",
                    value: model.state.retouch.smoothLow,
                    neutral: 0
                ) { $0.smoothLow = $1 }
                retouchSlider(
                    "磨皮 · 中频（纹理）",
                    value: model.state.retouch.smoothMid,
                    neutral: 0
                ) { $0.smoothMid = $1 }
                retouchSlider(
                    "磨皮 · 高频（毛孔）",
                    value: model.state.retouch.smoothHigh,
                    neutral: 0
                ) { $0.smoothHigh = $1 }
                retouchSlider(
                    "统一肤色",
                    value: model.state.retouch.skinTone,
                    neutral: 0
                ) { $0.skinTone = $1 }

                sectionTitle("脸部塑形")
                retouchSlider("瘦脸", value: model.state.retouch.faceSlim, neutral: 0) {
                    $0.faceSlim = $1
                }
                retouchSlider("下巴", value: model.state.retouch.chin, neutral: 0) {
                    $0.chin = $1
                }
                retouchSlider("大眼", value: model.state.retouch.eyeEnlarge, neutral: 0) {
                    $0.eyeEnlarge = $1
                }
                retouchSlider("瘦鼻", value: model.state.retouch.noseSlim, neutral: 0) {
                    $0.noseSlim = $1
                }

                sectionTitle("身体塑形")
                retouchSlider("瘦身（腰腹）", value: model.state.retouch.bodySlim, neutral: 0) {
                    $0.bodySlim = $1
                }

                HStack {
                    Button("重置修图") {
                        Haptics.light()
                        model.resetRetouch()
                    }
                    .font(.caption)
                    .foregroundStyle(
                        model.state.retouch.isActive ? Color.accentColor : Color.white.opacity(0.3)
                    )
                    .disabled(!model.state.retouch.isActive)
                    Spacer()
                    Text("修图与调色分开：切到「编辑」Tab 继续调色")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
                .opacity(model.draggingSlider == nil ? 1 : 0)
                .allowsHitTesting(model.draggingSlider == nil)
            }
            .padding(.vertical, 4)
        }
        .scrollDisabled(model.draggingSlider != nil)
    }

    private func sectionTitle(_ text: String) -> some View {
        Text(text)
            .font(.caption2.weight(.semibold))
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .padding(.top, 8)
            .padding(.bottom, 2)
            .opacity(model.draggingSlider == nil ? 1 : 0)
    }

    private func retouchSlider(
        _ label: String,
        value: Float,
        neutral: Float,
        apply: @escaping (inout RetouchUi, Float) -> Void
    ) -> some View {
        ValueSliderRow(
            label: label,
            value: value,
            range: 0...100,
            neutral: neutral,
            step: 5,
            decimals: 0,
            onBegin: { model.beginEdit() },
            onChange: { newValue in
                model.updateRetouch { apply(&$0, newValue) }
            },
            onCommit: {
                model.commitEdit()
                model.recomputeRetouch()
            }
        )
        .opacity(model.sliderVisible(label) ? 1 : 0)
        .allowsHitTesting(model.sliderVisible(label))
    }
}
