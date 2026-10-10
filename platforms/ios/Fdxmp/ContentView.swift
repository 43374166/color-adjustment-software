import Photos
import SwiftUI

/// fdxmp iOS 主界面。
///
/// 严格按规格实现 Lightroom 移动版式三大 Tab：设备（默认）/ 编辑 / 我的。
/// 设备 Tab：Photos 权限、网格 / 列表、搜索 / 排序 / 筛选、多选、全屏预览、导入 / 相机 / 分享。
/// 编辑 Tab：调色（亮度 / 颜色 / 效果 / 细节）+ 预设 + 导出到相册。
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
    @State private var editAsset: PHAsset?

    var body: some View {
        TabView(selection: $selection) {
            DeviceTabView { asset in
                editAsset = asset
                selection = .edit
            }
            .tabItem { Label(Tab.device.rawValue, systemImage: Tab.device.icon) }
            .tag(Tab.device)

            EditorTabView(asset: editAsset)
                .tabItem { Label(Tab.edit.rawValue, systemImage: Tab.edit.icon) }
                .tag(Tab.edit)

            placeholder(
                title: "我的",
                detail: "账号 / 缓存管理 / 预设管理 / 主题 / 关于（后续阶段接入）"
            )
            .tabItem { Label(Tab.me.rawValue, systemImage: Tab.me.icon) }
            .tag(Tab.me)
        }
        .preferredColorScheme(.dark)
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
