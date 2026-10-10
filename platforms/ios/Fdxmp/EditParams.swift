import Foundation

/// 调色分类（底部面板的一级 Tab）。
enum EditCategory: String, CaseIterable, Identifiable {
    case light = "亮度"
    case color = "颜色"
    case effects = "效果"
    case detail = "细节"

    var id: String { rawValue }
}

/// 效果面板子页。
enum EffectTab: String, CaseIterable, Identifiable {
    case effects = "效果"
    case vignette = "晕影"
    case grain = "颗粒"

    var id: String { rawValue }
}

/// 细节面板子页。
enum DetailTab: String, CaseIterable, Identifiable {
    case sharpen = "锐化"
    case noise = "降噪"

    var id: String { rawValue }
}

/// 一个滑块的元数据（与 Rust `EditParams` 字段一一对应）。
struct SliderSpec: Identifiable {
    /// JSON 字段名。
    let key: String
    /// 界面标签。
    let label: String
    /// 所属分类。
    let category: EditCategory
    /// JSON 分组路径（如 `light` / `detail.sharpen`）。
    let group: String
    /// 取值范围。
    let range: ClosedRange<Float>
    /// 默认值（也是双击复位值）。
    let defaultValue: Float
    /// 效果面板子页（仅效果分类）。
    var effectTab: EffectTab? = nil
    /// 细节面板子页（仅细节分类）。
    var detailTab: DetailTab? = nil

    var id: String { key }
}

/// 内置预设。
struct BuiltinPreset: Identifiable {
    let name: String
    let description: String
    let params: [String: Float]

    var id: String { name }
}

/// 参数规格表：与 Android `EDITOR_SLIDERS` 保持同一套字段与量程。
enum EditorSpec {
    static let sliders: [SliderSpec] = [
        // 亮度
        SliderSpec(key: "exposure", label: "曝光度", category: .light, group: "light", range: -5...5, defaultValue: 0),
        SliderSpec(key: "contrast", label: "对比度", category: .light, group: "light", range: -100...100, defaultValue: 0),
        SliderSpec(key: "highlights", label: "高光", category: .light, group: "light", range: -100...100, defaultValue: 0),
        SliderSpec(key: "shadows", label: "阴影", category: .light, group: "light", range: -100...100, defaultValue: 0),
        SliderSpec(key: "whites", label: "白色", category: .light, group: "light", range: -100...100, defaultValue: 0),
        SliderSpec(key: "blacks", label: "黑色", category: .light, group: "light", range: -100...100, defaultValue: 0),
        // 颜色
        SliderSpec(key: "temperature", label: "色温", category: .color, group: "color", range: -100...100, defaultValue: 0),
        SliderSpec(key: "tint", label: "色调", category: .color, group: "color", range: -100...100, defaultValue: 0),
        SliderSpec(key: "vibrance", label: "自然饱和度", category: .color, group: "color", range: -100...100, defaultValue: 0),
        SliderSpec(key: "saturation", label: "饱和度", category: .color, group: "color", range: -100...100, defaultValue: 0),
        // 效果
        SliderSpec(key: "texture", label: "纹理", category: .effects, group: "effects", range: -100...100, defaultValue: 0, effectTab: .effects),
        SliderSpec(key: "clarity", label: "清晰度", category: .effects, group: "effects", range: -100...100, defaultValue: 0, effectTab: .effects),
        SliderSpec(key: "dehaze", label: "去雾", category: .effects, group: "effects", range: -100...100, defaultValue: 0, effectTab: .effects),
        SliderSpec(key: "vignette", label: "暗角", category: .effects, group: "effects", range: -100...100, defaultValue: 0, effectTab: .vignette),
        SliderSpec(key: "vignette_midpoint", label: "中点", category: .effects, group: "effects", range: 0...100, defaultValue: 50, effectTab: .vignette),
        SliderSpec(key: "vignette_feather", label: "羽化", category: .effects, group: "effects", range: 0...100, defaultValue: 50, effectTab: .vignette),
        SliderSpec(key: "grain", label: "颗粒", category: .effects, group: "effects", range: 0...100, defaultValue: 0, effectTab: .grain),
        SliderSpec(key: "grain_size", label: "大小", category: .effects, group: "effects", range: 0...100, defaultValue: 0, effectTab: .grain),
        SliderSpec(key: "grain_roughness", label: "粗糙度", category: .effects, group: "effects", range: 0...100, defaultValue: 50, effectTab: .grain),
        // 细节 · 锐化
        SliderSpec(key: "amount", label: "数量", category: .detail, group: "detail.sharpen", range: 0...150, defaultValue: 0, detailTab: .sharpen),
        SliderSpec(key: "radius", label: "半径", category: .detail, group: "detail.sharpen", range: 0.5...3, defaultValue: 1, detailTab: .sharpen),
        SliderSpec(key: "detail", label: "细节", category: .detail, group: "detail.sharpen", range: 0...100, defaultValue: 25, detailTab: .sharpen),
        SliderSpec(key: "masking", label: "蒙版", category: .detail, group: "detail.sharpen", range: 0...100, defaultValue: 0, detailTab: .sharpen),
        // 细节 · 降噪
        SliderSpec(key: "luminance", label: "亮度", category: .detail, group: "detail.noise", range: 0...100, defaultValue: 0, detailTab: .noise),
        SliderSpec(key: "luminance_detail", label: "亮度细节", category: .detail, group: "detail.noise", range: 0...100, defaultValue: 50, detailTab: .noise),
        SliderSpec(key: "luminance_contrast", label: "亮度对比度", category: .detail, group: "detail.noise", range: 0...100, defaultValue: 0, detailTab: .noise),
        SliderSpec(key: "color", label: "颜色", category: .detail, group: "detail.noise", range: 0...100, defaultValue: 0, detailTab: .noise),
        SliderSpec(key: "color_detail", label: "颜色细节", category: .detail, group: "detail.noise", range: 0...100, defaultValue: 50, detailTab: .noise),
        SliderSpec(key: "color_smoothness", label: "颜色平滑度", category: .detail, group: "detail.noise", range: 0...100, defaultValue: 50, detailTab: .noise),
    ]

    /// 内置预设（与 Android 相同的六组）。
    static let presets: [BuiltinPreset] = [
        BuiltinPreset(name: "原片", description: "全部归零", params: [:]),
        BuiltinPreset(
            name: "人像",
            description: "柔和肤色 · 提亮阴影",
            params: [
                "contrast": 8, "highlights": -15, "shadows": 20,
                "vibrance": 15, "temperature": 6, "clarity": -5,
            ]
        ),
        BuiltinPreset(
            name: "风景",
            description: "通透 · 强化色彩层次",
            params: [
                "contrast": 15, "highlights": -25, "shadows": 15,
                "vibrance": 25, "saturation": 8, "clarity": 12,
            ]
        ),
        BuiltinPreset(
            name: "黑白",
            description: "去色 · 高对比",
            params: ["saturation": -100, "contrast": 18, "clarity": 10]
        ),
        BuiltinPreset(
            name: "电影",
            description: "青橙 · 颗粒质感",
            params: [
                "contrast": 12, "highlights": -20, "shadows": -10,
                "temperature": -6, "vibrance": 10, "grain": 18,
            ]
        ),
        BuiltinPreset(
            name: "HDR",
            description: "压高光 · 提阴影",
            params: [
                "highlights": -40, "shadows": 35, "whites": 10,
                "blacks": -10, "clarity": 15, "vibrance": 12,
            ]
        ),
    ]

    /// 全部滑块默认值。
    static func defaultValues() -> [String: Float] {
        var values: [String: Float] = [:]
        for spec in sliders {
            values[spec.key] = spec.defaultValue
        }
        return values
    }

    /// 指定分类 / 子页的滑块。
    static func visibleSliders(
        category: EditCategory,
        effectTab: EffectTab,
        detailTab: DetailTab
    ) -> [SliderSpec] {
        sliders.filter { spec in
            guard spec.category == category else { return false }
            switch category {
            case .effects: return spec.effectTab == effectTab
            case .detail: return spec.detailTab == detailTab
            default: return true
            }
        }
    }

    /// 数值文案（曝光 2 位小数，半径 1 位，其余整数）。
    static func formatted(_ spec: SliderSpec, _ value: Float) -> String {
        let text: String
        if spec.key == "exposure" {
            text = String(format: "%.2f", value)
        } else if spec.range.upperBound - spec.range.lowerBound <= 3 {
            text = String(format: "%.1f", value)
        } else {
            text = String(format: "%.0f", value)
        }
        if value > spec.defaultValue {
            return "+\(text)"
        }
        return text
    }
}

/// 数值钳制。
extension Comparable {
    func clamped(to range: ClosedRange<Self>) -> Self {
        min(max(self, range.lowerBound), range.upperBound)
    }
}

/// 编辑记录存储：按照片（PHAsset localIdentifier）保存 EditParams JSON。
enum EditStore {
    private static let key = "fdxmp_ios_edit_records"

    static func json(assetId: String) -> String? {
        let records = UserDefaults.standard.dictionary(forKey: key) as? [String: String]
        return records?[assetId]
    }

    static func save(assetId: String, json: String) {
        var records = UserDefaults.standard.dictionary(forKey: key) as? [String: String] ?? [:]
        records[assetId] = json
        UserDefaults.standard.set(records, forKey: key)
    }
}
