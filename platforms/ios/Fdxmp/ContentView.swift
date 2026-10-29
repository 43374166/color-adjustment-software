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
    /// 进入编辑会话的来源（关闭时回到哪里）。
    @State private var editOrigin: Tab = .device
    /// 进入编辑会话时的照片队列（编辑器内左右滑动切上一张 / 下一张）。
    @State private var editQueue: [PHAsset] = []
    /// 当前编辑会话所属项目（nil = 设备 / 最近编辑单张：不做左右切换、不显示同步）。
    @State private var editProject: Project?

    var body: some View {
        TabView(selection: $selection) {
            DeviceTabView { asset, queue in
                openEditor(asset: asset, queue: queue, origin: .device)
            }
            .tabItem { Label(Tab.device.rawValue, systemImage: Tab.device.icon) }
            .tag(Tab.device)

            Group {
                if let asset = editAsset {
                    // 编辑会话全屏：隐藏底部三大 Tab，只保留编辑器自身的四个工具。
                    EditorTabView(
                        asset: asset,
                        project: editProject,
                        prevAsset: neighborAsset(offset: -1),
                        nextAsset: neighborAsset(offset: 1),
                        queue: editQueue,
                        onClose: { closeEditor() },
                        onSwipePrev: { stepEditor(by: -1) },
                        onSwipeNext: { stepEditor(by: 1) },
                        canSwipePrev: canStepBackward,
                        canSwipeNext: canStepForward
                    )
                    .toolbar(.hidden, for: .tabBar)
                } else {
                    RecentlyEditedView(
                        onOpen: { asset, queue in
                            openEditor(asset: asset, queue: queue, origin: .edit)
                        },
                        onOpenProject: { project, assets, startAt in
                            openProject(project, assets: assets, startAt: startAt)
                        }
                    )
                    .toolbar(.visible, for: .tabBar)
                }
            }
            .tabItem { Label(Tab.edit.rawValue, systemImage: Tab.edit.icon) }
            .tag(Tab.edit)

            MeTabView()
                .tabItem { Label(Tab.me.rawValue, systemImage: Tab.me.icon) }
                .tag(Tab.me)
        }
        .preferredColorScheme(.dark)
    }

    // MARK: - 编辑会话（含左右滑动切图）

    /// 当前编辑照片在队列中的位置。
    private var editIndex: Int? {
        guard let editAsset else { return nil }
        return editQueue.firstIndex { $0.localIdentifier == editAsset.localIdentifier }
    }

    private var canStepBackward: Bool {
        // 设备 / 最近编辑的单张编辑不做左右切换；项目内可以切项目照片。
        guard editProject != nil, let index = editIndex else { return false }
        return index > 0
    }

    private var canStepForward: Bool {
        guard editProject != nil, let index = editIndex else { return false }
        return index + 1 < editQueue.count
    }

    /// 当前照片在队列中的前 / 后邻居（相册式滑动的预览用）。
    private func neighborAsset(offset: Int) -> PHAsset? {
        guard let index = editIndex else { return nil }
        let target = index + offset
        guard editQueue.indices.contains(target) else { return nil }
        return editQueue[target]
    }

    /// 打开编辑会话：带上当前列表（设备页网格 / 列表 / 全屏预览、最近编辑列表）。
    private func openEditor(asset: PHAsset, queue: [PHAsset], origin: Tab) {
        var unique = uniqueAssets(queue)
        // 极端情况下调用方传进来的队列不含当前照片：补进去，避免“只能看一张”。
        if !unique.contains(where: { $0.localIdentifier == asset.localIdentifier }) {
            unique.insert(asset, at: 0)
        }
        withAnimation(.easeInOut(duration: 0.25)) {
            editAsset = asset
            editQueue = unique
            editOrigin = origin
            editProject = nil
            selection = .edit
        }
    }

    /// 打开项目编辑会话（队列 = 项目内照片，可左右切换 / 一键同步）。
    private func openProject(_ project: Project, assets: [PHAsset], startAt: PHAsset? = nil) {
        let queue = uniqueAssets(assets)
        let start = startAt.flatMap { target in
            queue.first { $0.localIdentifier == target.localIdentifier }
        } ?? queue.first
        guard let first = start else { return }
        withAnimation(.easeInOut(duration: 0.25)) {
            editAsset = first
            editQueue = queue
            editOrigin = .edit
            editProject = project
            selection = .edit
        }
    }

    /// 去掉重复的 asset id（项目 JSON / PHPicker 历史数据里可能有重复），保持原顺序。
    private func uniqueAssets(_ assets: [PHAsset]) -> [PHAsset] {
        var seen = Set<String>()
        return assets.filter { seen.insert($0.localIdentifier).inserted }
    }

    private func closeEditor() {
        withAnimation(.easeInOut(duration: 0.25)) {
            editAsset = nil
            editProject = nil
            selection = editOrigin
        }
    }

    /// 编辑器内滑动切上一张 / 下一张（offset = ±1）。
    ///
    /// 这里**不要**用 `withAnimation` 包住 asset 切换：左右滑动的位移动画由
    /// EditorTabView 自己控制，父层再动画一次会让旧图淡出 / 新图淡入，
    /// 表现为“先闪一下别的照片再跳回来”。
    private func stepEditor(by offset: Int) {
        guard let index = editIndex else { return }
        let target = index + offset
        guard editQueue.indices.contains(target) else { return }
        editAsset = editQueue[target]
    }
}

#Preview {
    ContentView()
}
