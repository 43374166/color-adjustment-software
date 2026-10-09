import SwiftUI

/// fdxmp iOS 主界面。
///
/// 严格按规格实现 Lightroom 移动版式三大 Tab：设备（默认）/ 编辑 / 我的。
/// 设备 Tab 已接入 Photos（权限、网格 / 列表、搜索 / 排序 / 筛选、多选、
/// 全屏预览、导入 / 相机 / 分享）；编辑与我的待后续阶段。
struct ContentView: View {
    enum Tab: String, CaseIterable, Identifiable {
        case device = "设备"
        case edit = "编辑"
        case me = "我的"

        var id: String { rawValue }

        var icon: String {
            switch self {
            case .device: return "photo.on.rectangle"
            case .edit: return "slider.horizontal.3"
            case .me: return "person.crop.circle"
            }
        }
    }

    @State private var selection: Tab = .device
    @State private var pendingEditName: String?

    var body: some View {
        TabView(selection: $selection) {
            DeviceTabView { asset in
                pendingEditName = asset.displayName
                selection = .edit
            }
            .tabItem { Label(Tab.device.rawValue, systemImage: Tab.device.icon) }
            .tag(Tab.device)

            placeholder(
                title: "编辑",
                detail: pendingEditName.map {
                    "已选择：\($0)\n\n调色、曲线、蒙版、导出等编辑界面在阶段 6 接入。"
                } ?? "还没有选择照片。\n\n去「设备」Tab 点开一张照片，再点「编辑」。"
            )
            .tabItem { Label(Tab.edit.rawValue, systemImage: Tab.edit.icon) }
            .tag(Tab.edit)

            placeholder(
                title: "我的",
                detail: "账号 / 缓存管理 / 预设管理 / 主题 / 关于（阶段 6 接入）"
            )
            .tabItem { Label(Tab.me.rawValue, systemImage: Tab.me.icon) }
            .tag(Tab.me)
        }
        .preferredColorScheme(.dark)
        .overlay(alignment: .top) {
            Text("Rust core：\(NativeCore.shared.version)")
                .font(.caption2)
                .foregroundStyle(.tertiary)
                .padding(.top, 2)
        }
    }

    private func placeholder(title: String, detail: String) -> some View {
        VStack(spacing: 12) {
            Text("fdxmp · \(title)")
                .font(.headline)
            Text(detail)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
        .padding(24)
    }
}

#Preview {
    ContentView()
}
