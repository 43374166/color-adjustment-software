import SwiftUI
import UIKit

/// 零拷贝预览图层：CAMetalLayer 直接显示核心 GPU 渲染结果（同一帧由核心上屏，
/// 不经过 CPU 读回）。图层存活期间把指针报给 `EditorModel`，销毁时报 nil。
///
/// 变换与 Image 路径保持一致：外层容器撑满预览区再 `scaleEffect` / `offset`，
/// 保证缩放 / 平移、顶部偏移的几何完全一样。
struct MetalPreviewView: UIViewRepresentable {
    /// 图层回调：pointer = 新图层，nil = 本图层销毁；token 用来区分新旧实例。
    let onLayer: (UnsafeMutableRawPointer?, UUID) -> Void

    final class Coordinator {
        let token = UUID()
        let onLayer: (UnsafeMutableRawPointer?, UUID) -> Void

        init(onLayer: @escaping (UnsafeMutableRawPointer?, UUID) -> Void) {
            self.onLayer = onLayer
        }
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(onLayer: onLayer)
    }

    func makeUIView(context: Context) -> PreviewMetalView {
        let view = PreviewMetalView()
        view.backgroundColor = .clear
        view.isOpaque = false
        view.isUserInteractionEnabled = false
        if let layer = view.metalLayer {
            layer.isOpaque = true
            layer.framebufferOnly = true
            onLayer(Unmanaged.passUnretained(layer).toOpaque(), context.coordinator.token)
        }
        return view
    }

    func updateUIView(_ view: PreviewMetalView, context: Context) {}

    static func dismantleUIView(_ view: PreviewMetalView, coordinator: Coordinator) {
        coordinator.onLayer(nil, coordinator.token)
    }
}

/// CAMetalLayer 宿主视图（不接收触摸：手势仍由预览区父层处理）。
final class PreviewMetalView: UIView {
    override class var layerClass: AnyClass { CAMetalLayer.self }

    var metalLayer: CAMetalLayer? {
        layer as? CAMetalLayer
    }
}
