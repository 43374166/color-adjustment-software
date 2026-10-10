import UIKit

/// 轻量触觉反馈（设备不支持时自动忽略）。
///
/// 手感和 Lightroom 对齐：滑块跨中性点 / 点按步进是"咔哒"的选择感，
/// 归零 / 落笔 / 曲线加减点是轻击，长按进入对比预览是中击。
enum Haptics {
    private static let selectionGenerator = UISelectionFeedbackGenerator()
    private static let lightGenerator = UIImpactFeedbackGenerator(style: .light)
    private static let mediumGenerator = UIImpactFeedbackGenerator(style: .medium)

    /// 预热 Taptic Engine（进入编辑页 / 开始拖动时调用，降低首次延迟）。
    static func prepare() {
        selectionGenerator.prepare()
        lightGenerator.prepare()
    }

    /// 选择感：滑块跨越中性点、点击步进、切换工具 / 分类。
    static func tick() {
        selectionGenerator.selectionChanged()
        selectionGenerator.prepare()
    }

    /// 轻击：归零、画笔落笔、曲线加点 / 删点、展开收起。
    static func light() {
        lightGenerator.impactOccurred()
        lightGenerator.prepare()
    }

    /// 中击：长按进入对比预览（看原图 / 分类归零）。
    static func medium() {
        mediumGenerator.impactOccurred()
        mediumGenerator.prepare()
    }
}
