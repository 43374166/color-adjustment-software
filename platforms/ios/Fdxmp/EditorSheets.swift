import Photos
import SwiftUI

/// 编辑器里的底部弹框（从下往上）。
enum EditorSheetKind: String, Identifiable {
    case more
    case export
    case sync

    var id: String { rawValue }
}

/// 「•••」更多：导出 / 同步 / 重置（从下往上弹框）。
struct EditorMoreSheet: View {
    let canSync: Bool
    let onExport: () -> Void
    let onSync: () -> Void
    let onReset: () -> Void

    var body: some View {
        VStack(spacing: 10) {
            Capsule()
                .fill(Color.white.opacity(0.25))
                .frame(width: 36, height: 4)
                .padding(.top, 8)
            Text("更多")
                .font(.subheadline.weight(.semibold))
                .padding(.bottom, 2)
            actionRow("导出", icon: "square.and.arrow.up", action: onExport)
            if canSync {
                actionRow("同步", icon: "square.on.square", action: onSync)
            }
            actionRow(
                "重置全部编辑",
                icon: "arrow.counterclockwise",
                destructive: true,
                action: onReset
            )
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 16)
        .background(Color(white: 0.10))
    }

    private func actionRow(
        _ title: String,
        icon: String,
        destructive: Bool = false,
        action: @escaping () -> Void
    ) -> some View {
        Button {
            Haptics.light()
            action()
        } label: {
            HStack(spacing: 12) {
                Image(systemName: icon)
                    .font(.body)
                    .frame(width: 22)
                Text(title)
                    .font(.subheadline)
                Spacer()
                Image(systemName: "chevron.right")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
            .padding(.vertical, 12)
            .padding(.horizontal, 14)
            .background(
                RoundedRectangle(cornerRadius: 12)
                    .fill(Color.white.opacity(0.06))
            )
            .foregroundStyle(destructive ? Color.red.opacity(0.9) : Color.white)
        }
        .buttonStyle(.plain)
    }
}

/// 照片多选弹框（导出 / 同步共用）：选择要处理的照片。
struct PhotoSelectionSheet: View {
    let title: String
    let confirmTitle: String
    let assets: [PHAsset]
    let currentAssetId: String?
    /// 同步时显示范围选择（导出传 nil）。
    let syncScope: Binding<EditorModel.SyncScope>?
    /// 导出时显示「高清增强」开关（同步传 nil）。
    var enhance: Binding<Bool>? = nil
    @Binding var selection: Set<String>
    let onConfirm: ([String]) -> Void

    var body: some View {
        VStack(spacing: 0) {
            Capsule()
                .fill(Color.white.opacity(0.25))
                .frame(width: 36, height: 4)
                .padding(.top, 8)
                .padding(.bottom, 6)
            HStack {
                Text(title)
                    .font(.subheadline.weight(.semibold))
                Spacer()
                Button(selection.count == assets.count ? "全不选" : "全选") {
                    Haptics.tick()
                    if selection.count == assets.count {
                        selection.removeAll()
                    } else {
                        selection = Set(assets.map(\.localIdentifier))
                    }
                }
                .font(.caption)
                .foregroundStyle(Color.accentColor)
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 6)
            if let syncScope {
                Picker("", selection: syncScope) {
                    ForEach(EditorModel.SyncScope.allCases) { scope in
                        Text(scope.rawValue).tag(scope)
                    }
                }
                .pickerStyle(.segmented)
                .padding(.horizontal, 16)
                .padding(.bottom, 8)
            }
            if let enhance {
                Button {
                    Haptics.tick()
                    enhance.wrappedValue.toggle()
                } label: {
                    HStack(spacing: 8) {
                        Image(systemName: enhance.wrappedValue ? "checkmark.square.fill" : "square")
                            .foregroundStyle(
                                enhance.wrappedValue
                                    ? Color.accentColor
                                    : Color.white.opacity(0.4)
                            )
                        VStack(alignment: .leading, spacing: 2) {
                            Text("高清增强（×2 超分）")
                                .font(.footnote)
                            Text("Real-ESRGAN 提升细节；长边超过 4096 自动跳过")
                                .font(.caption2)
                                .foregroundStyle(Color.white.opacity(0.45))
                        }
                        Spacer()
                    }
                    .padding(.horizontal, 16)
                    .padding(.bottom, 8)
                }
                .buttonStyle(.plain)
            }
            ScrollView {
                LazyVStack(spacing: 6) {
                    ForEach(assets, id: \.localIdentifier) { item in
                        let id = item.localIdentifier
                        Button {
                            Haptics.tick()
                            if selection.contains(id) {
                                selection.remove(id)
                            } else {
                                selection.insert(id)
                            }
                        } label: {
                            HStack(spacing: 10) {
                                RecentThumbnail(asset: item)
                                    .frame(width: 44, height: 44)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(item.displayName)
                                        .font(.footnote)
                                        .lineLimit(1)
                                    if id == currentAssetId {
                                        Text("当前照片")
                                            .font(.caption2)
                                            .foregroundStyle(Color.accentColor)
                                    }
                                }
                                Spacer()
                                Image(
                                    systemName: selection.contains(id)
                                        ? "checkmark.circle.fill"
                                        : "circle"
                                )
                                .font(.body)
                                .foregroundStyle(
                                    selection.contains(id)
                                        ? Color.accentColor
                                        : Color.white.opacity(0.3)
                                )
                            }
                            .padding(8)
                            .background(
                                RoundedRectangle(cornerRadius: 10)
                                    .fill(Color.white.opacity(0.05))
                            )
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, 14)
            }
            Divider().overlay(Color.white.opacity(0.08))
            Button {
                Haptics.medium()
                onConfirm(assets.map(\.localIdentifier).filter { selection.contains($0) })
            } label: {
                Text(confirmTitle)
                    .font(.subheadline.weight(.semibold))
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 12)
                    .background(
                        RoundedRectangle(cornerRadius: 12)
                            .fill(Color.accentColor.opacity(selection.isEmpty ? 0.15 : 0.3))
                    )
                    .foregroundStyle(selection.isEmpty ? Color.white.opacity(0.4) : Color.accentColor)
            }
            .buttonStyle(.plain)
            .disabled(selection.isEmpty)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
        }
        .background(Color(white: 0.10))
    }
}
