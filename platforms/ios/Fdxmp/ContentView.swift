import SwiftUI

/// fdxmp iOS 主界面（阶段 5 骨架）
///
/// 严格按规格实现 Lightroom 移动版式三大 Tab：设备（默认）/ 编辑 / 我的。
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

    var body: some View {
        TabView(selection: $selection) {
            placeholder(
                title: "设备",
                detail: "相册网格 / 列表、RAW 与常见格式、全屏预览（阶段 5 接入）"
            )
            .tabItem { Label(Tab.device.rawValue, systemImage: Tab.device.icon) }
            .tag(Tab.device)

            placeholder(
                title: "编辑",
                detail: "已编辑记录 + 缩略图 + 预设名（阶段 5 接入）"
            )
            .tabItem { Label(Tab.edit.rawValue, systemImage: Tab.edit.icon) }
            .tag(Tab.edit)

            placeholder(
                title: "我的",
                detail: "账号 / 缓存管理 / 预设管理 / 主题 / 关于（阶段 5 接入）"
            )
            .tabItem { Label(Tab.me.rawValue, systemImage: Tab.me.icon) }
            .tag(Tab.me)
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
            Text("Rust core：\(NativeCore.shared.version)")
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .padding()
    }
}

#Preview {
    ContentView()
}

