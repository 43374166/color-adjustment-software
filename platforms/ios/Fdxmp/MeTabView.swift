import SwiftUI
import UIKit

/// 「我的」页：编辑记录、内置预设、导出设置、关于。
struct MeTabView: View {
    @AppStorage(EditorLimits.exportFormatKey) private var exportFormat = EditorModel.ExportFormat.jpeg.rawValue
    @AppStorage(EditorLimits.exportQualityKey) private var exportQuality = 0.92
    @AppStorage(EditorLimits.exportKeepMetadataKey) private var exportKeepMetadata = true
    @AppStorage(EditorLimits.exportShareKey) private var exportShare = false
    /// 颜色均衡器总开关（全局，默认开启）。
    @AppStorage("fdxmp_eq_enabled") private var equalizerEnabled = true

    @State private var editCount = 0
    @State private var lastEdited: Date?
    @State private var showClearConfirm = false
    @State private var message: String?

    private var appVersion: String {
        Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "—"
    }

    private var buildNumber: String {
        Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "—"
    }

    var body: some View {
        ScrollView {
            VStack(spacing: 14) {
                headerCard
                editsCard
                editingCard
                presetsCard
                exportCard
                aboutCard
            }
            .padding(16)
        }
        .background(Color(white: 0.08))
        .onAppear(perform: reload)
        .confirmationDialog(
            "清除全部编辑记录？",
            isPresented: $showClearConfirm,
            titleVisibility: .visible
        ) {
            Button("清除", role: .destructive) {
                EditStore.clearAll()
                reload()
                message = "已清除全部编辑记录"
            }
            Button("取消", role: .cancel) {}
        } message: {
            Text("照片本身不会被改动，只删除 App 内保存的调色参数。")
        }
        .overlay(alignment: .bottom) {
            if let message {
                Text(message)
                    .font(.footnote)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
                    .background(.ultraThinMaterial, in: Capsule())
                    .padding(.bottom, 18)
                    .task(id: message) {
                        try? await Task.sleep(nanoseconds: 2_200_000_000)
                        self.message = nil
                    }
            }
        }
    }

    // MARK: - 头部

    private var headerCard: some View {
        HStack(spacing: 14) {
            RoundedRectangle(cornerRadius: 16)
                .fill(
                    LinearGradient(
                        colors: [Color.accentColor, Color.accentColor.opacity(0.55)],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )
                .frame(width: 62, height: 62)
                .overlay(
                    Text("fx")
                        .font(.system(size: 24, weight: .bold, design: .rounded))
                        .foregroundStyle(.white)
                )
            VStack(alignment: .leading, spacing: 4) {
                Text("fdxmp")
                    .font(.title3.weight(.semibold))
                Text("专业图像处理 · \(appVersion) (\(buildNumber))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
        }
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 16)
                .fill(Color.white.opacity(0.06))
        )
    }

    // MARK: - 编辑记录

    private var editsCard: some View {
        card("编辑记录") {
            row(label: "已编辑照片", value: "\(editCount) 张")
            row(
                label: "最近编辑",
                value: lastEdited.map { Self.formatter.string(from: $0) } ?? "暂无"
            )
            Button {
                showClearConfirm = true
            } label: {
                Text("清除全部编辑记录")
                    .font(.footnote)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 8)
                    .background(
                        RoundedRectangle(cornerRadius: 9)
                            .fill(Color.red.opacity(editCount > 0 ? 0.22 : 0.08))
                    )
                    .foregroundStyle(editCount > 0 ? Color.red : Color.secondary)
            }
            .buttonStyle(.plain)
            .disabled(editCount == 0)
        }
    }

    // MARK: - 内置预设

    private var presetsCard: some View {
        card("内置预设") {
            ForEach(Array(EditorSpec.presets.enumerated()), id: \.offset) { index, preset in
                if index > 0 {
                    Divider().overlay(Color.white.opacity(0.08))
                }
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(preset.name)
                            .font(.footnote)
                        Text(preset.description)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Text("\(preset.params.count) 项")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
                .padding(.vertical, 2)
            }
        }
    }

    // MARK: - 导出设置

    /// 编辑总开关：颜色均衡器默认开启，开关放在「我的」里。
    private var editingCard: some View {
        card("编辑") {
            Toggle(isOn: $equalizerEnabled) {
                Text("颜色均衡器")
                    .font(.footnote)
            }
            Text("默认开启（与 Windows 端一致）；关闭后所有照片都不跑均衡器。")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }

    private var exportCard: some View {
        card("导出") {
            HStack {
                Text("导出尺寸")
                    .font(.footnote)
                Spacer()
                Text("原图分辨率")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            HStack {
                Text("格式")
                    .font(.footnote)
                Spacer()
                Picker("格式", selection: $exportFormat) {
                    ForEach(EditorModel.ExportFormat.allCases) { format in
                        Text(format.rawValue).tag(format.rawValue)
                    }
                }
                .pickerStyle(.segmented)
                .frame(width: 190)
            }
            if EditorModel.ExportFormat(rawValue: exportFormat) == .jpeg {
                HStack {
                    Text("JPEG 质量")
                        .font(.footnote)
                    Slider(value: $exportQuality, in: 0.5...1, step: 0.02)
                    Text(String(format: "%.0f%%", exportQuality * 100))
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                        .frame(width: 44, alignment: .trailing)
                }
            }
            Toggle("保留拍摄信息（EXIF / GPS）", isOn: $exportKeepMetadata)
                .font(.footnote)
                .tint(Color.accentColor)
            Toggle("导出完成后分享", isOn: $exportShare)
                .font(.footnote)
                .tint(Color.accentColor)
            Text("导出会把成品按此尺寸 / 格式渲染后保存到相册「fdxmp」。")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - 关于

    private var aboutCard: some View {
        card("关于") {
            row(label: "App 版本", value: "\(appVersion) (\(buildNumber))")
            row(label: "Rust 核心", value: NativeCore.shared.version)
            Divider().overlay(Color.white.opacity(0.08))
            Link(
                destination: URL(string: "https://github.com/43374166/color-adjustment-software")!
            ) {
                HStack {
                    Text("GitHub 仓库")
                        .font(.footnote)
                    Spacer()
                    Image(systemName: "arrow.up.right")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                }
            }
            Button {
                if let url = URL(string: UIApplication.openSettingsURLString) {
                    UIApplication.shared.open(url)
                }
            } label: {
                HStack {
                    Text("打开系统设置")
                        .font(.footnote)
                    Spacer()
                    Image(systemName: "gearshape")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                }
            }
            .buttonStyle(.plain)
            Divider().overlay(Color.white.opacity(0.08))
            Text("MIT License · 照片处理全部在本地完成，不会上传。")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - 通用组件

    private func card<Content: View>(
        _ title: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title)
                .font(.footnote.weight(.semibold))
                .foregroundStyle(.secondary)
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 14)
                .fill(Color.white.opacity(0.06))
        )
    }

    private func row(label: String, value: String) -> some View {
        HStack {
            Text(label)
                .font(.footnote)
            Spacer()
            Text(value)
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
    }

    private func reload() {
        editCount = EditStore.count()
        lastEdited = EditStore.lastEditedAt()
    }

    private static let formatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "MM-dd HH:mm"
        return formatter
    }()
}

#Preview {
    MeTabView()
}
