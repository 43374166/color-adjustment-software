import CoreGraphics
import Foundation

// MARK: - 曲线

/// 曲线控制点（归一化坐标）。
struct CurvePoint: Equatable {
    var x: Float
    var y: Float
}

/// 曲线通道。
enum CurveChannel: String, CaseIterable, Identifiable {
    case rgb = "白色"
    case red = "红"
    case green = "绿"
    case blue = "蓝"
    case parametric = "参数"

    var id: String { rawValue }

    /// JSON 字段名。
    var key: String {
        switch self {
        case .rgb: return "rgb"
        case .red: return "red"
        case .green: return "green"
        case .blue: return "blue"
        case .parametric: return "parametric"
        }
    }
}

/// 参数曲线（高光 / 中间调 / 阴影 + 两个分界）。
struct ParametricValues: Equatable {
    var highlights: Float = 0
    var midtones: Float = 0
    var shadows: Float = 0
    var highlightsSplit: Float = 0.75
    var midtonesSplit: Float = 0.25
}

/// 曲线状态。
struct CurveState: Equatable {
    var rgb: [CurvePoint] = CurveState.identity
    var red: [CurvePoint] = CurveState.identity
    var green: [CurvePoint] = CurveState.identity
    var blue: [CurvePoint] = CurveState.identity
    var parametric = ParametricValues()

    static let identity: [CurvePoint] = [
        CurvePoint(x: 0, y: 0),
        CurvePoint(x: 1, y: 1),
    ]

    func points(_ channel: CurveChannel) -> [CurvePoint] {
        switch channel {
        case .rgb: return rgb
        case .red: return red
        case .green: return green
        case .blue: return blue
        case .parametric: return rgb
        }
    }

    mutating func setPoints(_ channel: CurveChannel, _ points: [CurvePoint]) {
        switch channel {
        case .rgb: rgb = points
        case .red: red = points
        case .green: green = points
        case .blue: blue = points
        case .parametric: break
        }
    }

    mutating func reset() {
        self = CurveState()
    }

    /// 是否为恒等曲线（用于"原片"判断）。
    var isIdentity: Bool {
        rgb == Self.identity && red == Self.identity && green == Self.identity
            && blue == Self.identity && parametric == ParametricValues()
    }
}

// MARK: - HSL

/// HSL 色带。
enum HslBandKind: String, CaseIterable, Identifiable {
    case red = "红"
    case orange = "橙"
    case yellow = "黄"
    case green = "绿"
    case aqua = "青"
    case blue = "蓝"
    case purple = "紫"
    case magenta = "洋红"

    var id: String { rawValue }

    /// JSON 字段名。
    var key: String {
        switch self {
        case .red: return "red"
        case .orange: return "orange"
        case .yellow: return "yellow"
        case .green: return "green"
        case .aqua: return "aqua"
        case .blue: return "blue"
        case .purple: return "purple"
        case .magenta: return "magenta"
        }
    }

    /// 色带标识色（用于色块）。
    var tint: (r: Double, g: Double, b: Double) {
        switch self {
        case .red: return (1.0, 0.30, 0.30)
        case .orange: return (1.0, 0.60, 0.24)
        case .yellow: return (1.0, 0.88, 0.30)
        case .green: return (0.26, 0.85, 0.42)
        case .aqua: return (0.24, 0.85, 0.85)
        case .blue: return (0.30, 0.55, 1.0)
        case .purple: return (0.61, 0.42, 1.0)
        case .magenta: return (1.0, 0.37, 0.82)
        }
    }
}

/// 单个色带的 HSL 值（-100..100）。
struct HslBand: Equatable {
    var hue: Float = 0
    var saturation: Float = 0
    var luminance: Float = 0
}

/// HSL 状态。
struct HslState: Equatable {
    var bands: [HslBandKind: HslBand] = HslState.defaultBands()

    static func defaultBands() -> [HslBandKind: HslBand] {
        var bands: [HslBandKind: HslBand] = [:]
        for kind in HslBandKind.allCases {
            bands[kind] = HslBand()
        }
        return bands
    }

    subscript(kind: HslBandKind) -> HslBand {
        get { bands[kind] ?? HslBand() }
        set { bands[kind] = newValue }
    }

    mutating func reset() {
        self = HslState()
    }
}

// MARK: - 颜色分级

/// 分级区域。
enum GradingZoneKind: String, CaseIterable, Identifiable {
    case shadows = "阴影"
    case midtones = "中间调"
    case highlights = "高光"

    var id: String { rawValue }

    var key: String {
        switch self {
        case .shadows: return "shadows"
        case .midtones: return "midtones"
        case .highlights: return "highlights"
        }
    }
}

/// 单个分级区域：色相 0..360 / 饱和度 0..100 / 明亮度 -100..100。
struct GradingZone: Equatable {
    var hue: Float = 0
    var saturation: Float = 0
    var luminance: Float = 0
}

/// 颜色分级状态。
struct GradingState: Equatable {
    var shadows = GradingZone()
    var midtones = GradingZone()
    var highlights = GradingZone()
    var blending: Float = 50
    var balance: Float = 0

    subscript(kind: GradingZoneKind) -> GradingZone {
        get {
            switch kind {
            case .shadows: return shadows
            case .midtones: return midtones
            case .highlights: return highlights
            }
        }
        set {
            switch kind {
            case .shadows: shadows = newValue
            case .midtones: midtones = newValue
            case .highlights: highlights = newValue
            }
        }
    }

    mutating func reset() {
        self = GradingState()
    }
}

// MARK: - 裁切 / 变换

/// 裁切比例。
enum CropAspect: String, CaseIterable, Identifiable {
    case free = "自由"
    case square = "1:1"
    case fourThree = "4:3"
    case threeTwo = "3:2"
    case sixteenNine = "16:9"

    var id: String { rawValue }

    /// JSON 字段名（Rust `AspectRatio` 的 snake_case 值）。
    var jsonName: String {
        switch self {
        case .free: return "free"
        case .square: return "square"
        case .fourThree: return "four_three"
        case .threeTwo: return "three_two"
        case .sixteenNine: return "sixteen_nine"
        }
    }

    /// 宽高比（自由裁切为 nil）。
    var ratio: CGFloat? {
        switch self {
        case .free: return nil
        case .square: return 1
        case .fourThree: return 4.0 / 3.0
        case .threeTwo: return 3.0 / 2.0
        case .sixteenNine: return 16.0 / 9.0
        }
    }

    static func from(jsonName: String) -> CropAspect {
        allCases.first { $0.jsonName == jsonName } ?? .free
    }
}

/// 裁切状态：归一化矩形 + 拉直 + 翻转。
struct CropState: Equatable {
    var rect: CGRect = CGRect(x: 0, y: 0, width: 1, height: 1)
    var aspect: CropAspect = .free
    var rotation: Float = 0
    var flipHorizontal = false
    var flipVertical = false
    /// 构图网格样式（与核心 `CropGrid` 的 JSON 值对齐）。
    var grid: CropGridUi = .thirds

    var isIdentity: Bool {
        rect == CGRect(x: 0, y: 0, width: 1, height: 1)
            && rotation == 0 && !flipHorizontal && !flipVertical
    }

    mutating func reset() {
        self = CropState()
    }
}

/// 裁切构图网格（关 / 三分 / 网格 / 对角 / 黄金比例）。
enum CropGridUi: String, CaseIterable, Identifiable {
    case none = "关"
    case thirds = "三分"
    case grid = "网格"
    case diagonal = "对角"
    case golden = "黄金"

    var id: String { rawValue }

    /// JSON 字段名（Rust `CropGrid` 的 snake_case 值）。
    var jsonName: String {
        switch self {
        case .none: return "none"
        case .thirds: return "thirds"
        case .grid: return "grid"
        case .diagonal: return "diagonal"
        case .golden: return "golden_ratio"
        }
    }

    static func from(jsonName: String) -> CropGridUi {
        allCases.first { $0.jsonName == jsonName } ?? .thirds
    }
}

/// 归一化裁剪矩形（保证 0 ≤ x/y，x+w ≤ 1，宽高 ≥ 0.02）。
func clampedCropRect(_ rect: CGRect) -> CGRect {
    let width = min(max(rect.width, 0.02), 1)
    let height = min(max(rect.height, 0.02), 1)
    let x = min(max(rect.minX, 0), 1 - width)
    let y = min(max(rect.minY, 0), 1 - height)
    return CGRect(x: x, y: y, width: width, height: height)
}

// MARK: - 蒙版

/// AI 修图参数（与核心 `retouch::RetouchParams` 字段一一对应；0..100 强度）。
struct RetouchUi: Equatable {
    var blemish: Float = 0
    var smoothLow: Float = 0
    var smoothMid: Float = 0
    var smoothHigh: Float = 0
    var skinTone: Float = 0
    var faceSlim: Float = 0
    var chin: Float = 0
    var eyeEnlarge: Float = 0
    var noseSlim: Float = 0
    var bodySlim: Float = 0

    /// 完全未修图。
    var isIdentity: Bool { self == RetouchUi() }

    /// 是否已有任何修图（UI 用来决定显示「一键修图」还是滑杆）。
    var isActive: Bool { !isIdentity }

    /// JSON 字典（核心字段名）。
    var jsonObject: [String: Double] {
        [
            "blemish": Double(blemish),
            "smooth_low": Double(smoothLow),
            "smooth_mid": Double(smoothMid),
            "smooth_high": Double(smoothHigh),
            "skin_tone": Double(skinTone),
            "face_slim": Double(faceSlim),
            "chin": Double(chin),
            "eye_enlarge": Double(eyeEnlarge),
            "nose_slim": Double(noseSlim),
            "body_slim": Double(bodySlim),
        ]
    }

    /// 序列化为核心可读的 JSON（fdxmp_retouch_apply 的 params_json）。
    var jsonString: String {
        let object = jsonObject
        guard
            let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
            let text = String(data: data, encoding: .utf8)
        else {
            return "{}"
        }
        return text
    }

    static func from(_ value: Any?) -> RetouchUi {
        var params = RetouchUi()
        guard let root = value as? [String: Any] else { return params }
        func number(_ key: String) -> Float {
            (root[key] as? NSNumber)?.floatValue ?? 0
        }
        params.blemish = number("blemish")
        params.smoothLow = number("smooth_low")
        params.smoothMid = number("smooth_mid")
        params.smoothHigh = number("smooth_high")
        params.skinTone = number("skin_tone")
        params.faceSlim = number("face_slim")
        params.chin = number("chin")
        params.eyeEnlarge = number("eye_enlarge")
        params.noseSlim = number("nose_slim")
        params.bodySlim = number("body_slim")
        return params
    }

    /// 一键修图默认强度（按识别结果微调：无脸跳过脸部、无人像跳过瘦身）。
    static func autoDefaults(faces: Int, landmarks: Int, person: Bool) -> RetouchUi {
        var params = RetouchUi(
            blemish: 55,
            smoothLow: 35,
            smoothMid: 45,
            smoothHigh: 35,
            skinTone: 40
        )
        if faces > 0 && landmarks > 200 {
            params.faceSlim = 30
            params.chin = 15
            params.eyeEnlarge = 15
            params.noseSlim = 12
        }
        if person {
            params.bodySlim = 25
        }
        return params
    }
}

/// 修图识别结果（核心 `fdxmp_retouch_analyze` 返回的 JSON）。
struct RetouchAnalysisUi: Equatable {
    var faces: Int = 0
    var landmarks: Int = 0
    var skin: Bool = false
    var person: Bool = false
    var warnings: [String] = []

    static func from(json: String) -> RetouchAnalysisUi? {
        guard
            let data = json.data(using: .utf8),
            let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        else {
            return nil
        }
        var analysis = RetouchAnalysisUi()
        analysis.faces = (root["faces"] as? NSNumber)?.intValue ?? 0
        analysis.landmarks = (root["landmarks"] as? NSNumber)?.intValue ?? 0
        analysis.skin = (root["skin"] as? NSNumber)?.boolValue ?? false
        analysis.person = (root["person"] as? NSNumber)?.boolValue ?? false
        analysis.warnings = (root["warnings"] as? [String]) ?? []
        return analysis
    }

    /// 已识别人脸（五官可用）。
    var hasFace: Bool { faces > 0 }
    /// 已有五官关键点（塑形可用）。
    var hasLandmarks: Bool { landmarks > 200 }
}

/// 修图预设分类（预设面板顶部的分类）。
enum RetouchPresetCategory: String, CaseIterable, Identifiable {
    case hot = "热门"
    case portrait = "人像"
    case film = "胶片"
    case texture = "质感"

    var id: String { rawValue }
}

/// 修图预设（预设面板的一张卡：修图参数 + 配套调色；缩略图由核心实时渲染）。
struct RetouchPresetUi: Identifiable, Equatable {
    let id: String
    /// 卡片名称（写真 / 原生肌 / 奶油肌 …）。
    let name: String
    let category: RetouchPresetCategory
    /// 修图参数。
    let retouch: RetouchUi
    /// 配套调色（与 EditorSpec 滑块同 key；空 = 只修图不调色）。
    let color: [String: Float]

    /// 原图（没有修图）：清空修图 + 调色。
    static let original = RetouchPresetUi(
        id: "original",
        name: "原图",
        category: .hot,
        retouch: RetouchUi(),
        color: [:]
    )

    /// 预设目录。
    static let catalog: [RetouchPresetUi] = [
        // 热门
        RetouchPresetUi(
            id: "hot-xiezhen",
            name: "写真",
            category: .hot,
            retouch: RetouchUi(
                blemish: 60, smoothLow: 40, smoothMid: 50, smoothHigh: 38, skinTone: 45,
                faceSlim: 28, chin: 14, eyeEnlarge: 14, noseSlim: 10, bodySlim: 15
            ),
            color: ["exposure": 0.15, "contrast": 8, "vibrance": 10, "shadows": 8, "temperature": 4]
        ),
        RetouchPresetUi(
            id: "hot-naiyouji",
            name: "奶油肌",
            category: .hot,
            retouch: RetouchUi(
                blemish: 65, smoothLow: 50, smoothMid: 62, smoothHigh: 48, skinTone: 60,
                faceSlim: 20, chin: 10, eyeEnlarge: 10, noseSlim: 8
            ),
            color: ["exposure": 0.35, "contrast": -4, "vibrance": 8, "shadows": 14, "temperature": 6, "blacks": 6]
        ),
        RetouchPresetUi(
            id: "hot-baiyueguang",
            name: "白月光",
            category: .hot,
            retouch: RetouchUi(
                blemish: 55, smoothLow: 45, smoothMid: 55, smoothHigh: 42, skinTone: 70,
                faceSlim: 18, chin: 8, eyeEnlarge: 12, noseSlim: 8
            ),
            color: ["exposure": 0.45, "contrast": 4, "vibrance": 6, "saturation": -6, "temperature": -4, "highlights": -12]
        ),
        // 人像
        RetouchPresetUi(
            id: "portrait-yuanshengji",
            name: "原生肌",
            category: .portrait,
            retouch: RetouchUi(
                blemish: 45, smoothLow: 28, smoothMid: 30, smoothHigh: 22, skinTone: 35,
                faceSlim: 10
            ),
            color: ["exposure": 0.1, "contrast": 3, "vibrance": 6, "shadows": 6]
        ),
        RetouchPresetUi(
            id: "portrait-ziranguang",
            name: "自然光",
            category: .portrait,
            retouch: RetouchUi(
                blemish: 50, smoothLow: 35, smoothMid: 42, smoothHigh: 32, skinTone: 42,
                faceSlim: 20, chin: 10, eyeEnlarge: 10
            ),
            color: ["exposure": 0.25, "contrast": 6, "highlights": -10, "shadows": 12, "vibrance": 8]
        ),
        RetouchPresetUi(
            id: "portrait-rouguang",
            name: "柔光",
            category: .portrait,
            retouch: RetouchUi(
                blemish: 55, smoothLow: 42, smoothMid: 48, smoothHigh: 36, skinTone: 48,
                faceSlim: 24, chin: 12, eyeEnlarge: 12, noseSlim: 8
            ),
            color: ["exposure": 0.3, "contrast": -6, "highlights": -18, "shadows": 16, "vibrance": 6, "temperature": 5]
        ),
        // 胶片
        RetouchPresetUi(
            id: "film-jiaopian",
            name: "胶片",
            category: .film,
            retouch: RetouchUi(
                blemish: 45, smoothLow: 25, smoothMid: 30, smoothHigh: 20, skinTone: 35
            ),
            color: ["contrast": 10, "vibrance": -6, "saturation": -10, "grain": 25, "temperature": 6, "shadows": 10]
        ),
        RetouchPresetUi(
            id: "film-gangfeng",
            name: "港风",
            category: .film,
            retouch: RetouchUi(
                blemish: 40, smoothLow: 22, smoothMid: 28, smoothHigh: 18, skinTone: 40,
                faceSlim: 15
            ),
            color: ["contrast": 14, "saturation": -6, "vibrance": -4, "temperature": 10, "tint": -6, "grain": 30, "shadows": 8]
        ),
        RetouchPresetUi(
            id: "film-fugu",
            name: "复古",
            category: .film,
            retouch: RetouchUi(
                blemish: 40, smoothLow: 20, smoothMid: 26, smoothHigh: 18, skinTone: 38
            ),
            color: ["contrast": 8, "saturation": -14, "temperature": 12, "grain": 35, "blacks": 10, "highlights": -8]
        ),
        // 质感
        RetouchPresetUi(
            id: "texture-zazhi",
            name: "杂志",
            category: .texture,
            retouch: RetouchUi(
                blemish: 55, smoothLow: 30, smoothMid: 24, smoothHigh: 16, skinTone: 40,
                faceSlim: 18, chin: 8, eyeEnlarge: 8
            ),
            color: ["contrast": 12, "clarity": 12, "vibrance": 4, "shadows": -6, "highlights": -10]
        ),
        RetouchPresetUi(
            id: "texture-lengdiao",
            name: "冷调",
            category: .texture,
            retouch: RetouchUi(
                blemish: 50, smoothLow: 28, smoothMid: 26, smoothHigh: 18, skinTone: 45
            ),
            color: ["contrast": 10, "temperature": -12, "tint": -4, "vibrance": 6, "blacks": -6]
        ),
        RetouchPresetUi(
            id: "texture-yingdiao",
            name: "硬调",
            category: .texture,
            retouch: RetouchUi(
                blemish: 60, smoothLow: 32, smoothMid: 20, smoothHigh: 12, skinTone: 40,
                faceSlim: 20, chin: 10, eyeEnlarge: 10, noseSlim: 10
            ),
            color: ["contrast": 18, "clarity": 16, "vibrance": -4, "saturation": -6, "shadows": -10]
        ),
    ]

    /// 某分类下的预设（原图永远排第一）。
    static func presets(in category: RetouchPresetCategory) -> [RetouchPresetUi] {
        catalog.filter { $0.category == category }
    }
}

/// 蒙版类型（几何三种：线性渐变 / 径向渐变 / 画笔；v1.4.0 增加智能蒙版）。
enum MaskKindUi: String, CaseIterable, Identifiable {
    case linear = "线性渐变"
    case radial = "径向渐变"
    case brush = "画笔"
    // v1.4.0 智能蒙版（Vision）：主体 / 人像 / 五官。
    case aiSubject = "智能主体"
    case aiPortrait = "智能人像"
    case aiFace = "智能面部"
    case aiSkin = "智能皮肤"
    case aiEyes = "智能眼睛"
    case aiEyebrows = "智能眉毛"
    case aiNose = "智能鼻子"
    case aiLips = "智能嘴唇"
    case aiTeeth = "智能牙齿"

    var id: String { rawValue }

    var jsonName: String {
        switch self {
        case .brush: return "brush"
        case .linear: return "linear_gradient"
        case .radial: return "radial_gradient"
        case .aiSubject: return "ai_subject"
        case .aiPortrait, .aiFace, .aiSkin, .aiEyes, .aiEyebrows, .aiNose, .aiLips, .aiTeeth:
            return "ai_people"
        }
    }

    var icon: String {
        switch self {
        case .brush: return "paintbrush.pointed"
        case .linear: return "line.diagonal"
        case .radial: return "circle.dashed"
        case .aiSubject: return "person.crop.rectangle"
        case .aiPortrait: return "person.fill"
        case .aiFace: return "face.smiling"
        case .aiSkin: return "wand.and.stars"
        case .aiEyes: return "eye"
        case .aiEyebrows: return "eyebrow"
        case .aiNose: return "triangle"
        case .aiLips: return "mouth"
        case .aiTeeth: return "square.grid.2x2"
        }
    }

    /// 是否需要智能（Vision）推理。
    var isSmart: Bool {
        switch self {
        case .linear, .radial, .brush: return false
        default: return true
        }
    }

    /// 按 JSON 类型名恢复（不含部位）。
    static func from(jsonName: String) -> MaskKindUi? {
        allCases.first { $0.jsonName == jsonName }
    }

    /// ai_people 的部位（几何蒙版为 nil）。
    var peoplePart: PeoplePartUi? {
        switch self {
        case .aiPortrait: return .body
        case .aiFace: return .face
        case .aiSkin: return .skin
        case .aiEyes: return .eyes
        case .aiEyebrows: return .eyebrows
        case .aiNose: return .nose
        case .aiLips: return .lips
        case .aiTeeth: return .teeth
        default: return nil
        }
    }

    /// 由 JSON 类型 + 部位恢复（解析用）。
    static func from(jsonName: String, part: String?) -> MaskKindUi? {
        guard jsonName == "ai_people" else {
            return from(jsonName: jsonName)
        }
        switch part {
        case "body": return .aiPortrait
        case "face": return .aiFace
        case "skin": return .aiSkin
        case "eyes": return .aiEyes
        case "eyebrows": return .aiEyebrows
        case "nose": return .aiNose
        case "lips": return .aiLips
        case "teeth": return .aiTeeth
        default: return .aiPortrait
        }
    }
}

/// 智能蒙版「人物部位」（与核心 `PeoplePart` 的 snake_case JSON 值对齐）。
enum PeoplePartUi: String, CaseIterable {
    case body
    case face
    case skin
    case eyes
    case eyebrows
    case nose
    case lips
    case teeth
}

/// 一条画笔笔画：点序列 + 落笔时的参数快照 + 是否擦除。
///
/// 参数快照让"改动画笔大小 / 羽毛后只影响后续笔画"，已画好的笔画保持原样；
/// 点序列是归一化坐标，可二次编辑（不是位图）。
struct BrushStrokeUi: Equatable {
    var points: [CGPoint]
    var erase = false
    var size: Float = 30
    var feather: Float = 50
    var flow: Float = 100
    var density: Float = 100
}

/// 蒙版叠加颜色（与核心 `OverlayColor` 的 JSON 值对齐）。
enum MaskOverlayColor: String, CaseIterable, Identifiable {
    case red = "红"
    case green = "绿"
    case blue = "蓝"
    case white = "白"
    case black = "黑"

    var id: String { rawValue }

    var jsonName: String {
        switch self {
        case .red: return "red"
        case .green: return "green"
        case .blue: return "blue"
        case .white: return "white"
        case .black: return "black"
        }
    }

    static func from(jsonName: String) -> MaskOverlayColor {
        allCases.first { $0.jsonName == jsonName } ?? .red
    }
}

/// 单个蒙版的几何 / 笔迹 / 局部调整参数。
struct MaskUi: Identifiable, Equatable {
    var id: String
    var name: String
    var kind: MaskKindUi
    /// 可见（隐藏的蒙版不参与渲染与叠加）。
    var visible = true
    var inverted = false
    /// 通用羽化 0..100（v0.3 遗留字段；新版三种蒙版各有自己的过渡参数）。
    var feather: Float = 0
    /// 蒙版不透明度 0..100。
    var opacity: Float = 100
    /// 蒙版叠加颜色。
    var overlay: MaskOverlayColor = .red
    /// 该蒙版的调色参数（与全局滑块同一套 key，只含 light / color / effects）。
    var values: [String: Float] = EditorSpec.defaultValues()

    // 画笔（参数用于后续笔画；每条笔画自带快照）
    /// 笔刷大小（长边百分比，2..100）。
    var brushSize: Float = 30
    var brushFeather: Float = 50
    var brushFlow: Float = 100
    var brushDensity: Float = 100
    /// 笔画列表（归一化坐标点序列）。
    var strokes: [BrushStrokeUi] = []

    // 线性渐变
    /// 过渡带 100% 一侧的锚点（归一化，边界线过该点、垂直于渐变方向）。
    var start = CGPoint(x: 0.5, y: 0.3)
    /// 过渡带 0% 一侧的锚点。
    var end = CGPoint(x: 0.5, y: 0.7)

    // 径向渐变
    var center = CGPoint(x: 0.5, y: 0.5)
    var radii = CGSize(width: 0.4, height: 0.3)
    var rotation: Float = 0
    /// 径向羽化 0..100（0 = 硬边缘，100 = 从中心就开始过渡）。
    var radialFeather: Float = 25

    /// 该蒙版是否偏离默认参数（有实际效果）。
    var hasAdjustments: Bool {
        values != EditorSpec.defaultValues()
    }
}

// MARK: - 线性渐变几何（中心线 / 端点 / 过渡边界）

extension MaskUi {
    /// 过渡带中心（50% 中心线过该点）。
    var linearCenter: CGPoint {
        CGPoint(x: (start.x + end.x) / 2, y: (start.y + end.y) / 2)
    }

    /// 渐变方向单位向量（start → end）。
    var linearDirection: CGVector {
        let dx = end.x - start.x
        let dy = end.y - start.y
        let length = (dx * dx + dy * dy).squareRoot()
        guard length > 1e-6 else { return CGVector(dx: 0, dy: 1) }
        return CGVector(dx: dx / length, dy: dy / length)
    }

    /// 过渡带宽度（归一化投影长度）。
    var linearWidth: Double {
        Double(hypot(end.x - start.x, end.y - start.y))
    }

    /// 渐变方向角（度，0 = 向右，顺时针为正 —— 与图像 y 轴向下一致）。
    var linearAngleDegrees: Float {
        let dir = linearDirection
        return Float(atan2(dir.dy, dir.dx) * 180 / .pi)
    }

    /// 显示用的角度（归一化到 0..<180，渐变方向本身对称）。
    var linearAngleWrapped: Float {
        var value = linearAngleDegrees.truncatingRemainder(dividingBy: 180)
        if value < 0 {
            value += 180
        }
        return value
    }

    /// 显示用的径向旋转角（归一化到 0..<180）。
    var rotationWrapped: Float {
        var value = rotation.truncatingRemainder(dividingBy: 180)
        if value < 0 {
            value += 180
        }
        return value
    }

    /// 中心线在法线方向上的投影位置（-100..100，0 = 过画面中心）。
    var linearPositionPercent: Float {
        let normal = CGVector(dx: -linearDirection.dy, dy: linearDirection.dx)
        let offset = linearCenter.x * normal.dx + linearCenter.y * normal.dy
        return Float(offset * 100)
    }

    /// 设置过渡带宽度（保持中心与方向不变）。
    func settingLinearWidth(_ width: Double) -> MaskUi {
        var copy = self
        let clamped = min(max(width, 0.02), 2)
        let dir = linearDirection
        let center = linearCenter
        let half = CGFloat(clamped / 2)
        copy.start = CGPoint(x: center.x - dir.dx * half, y: center.y - dir.dy * half)
        copy.end = CGPoint(x: center.x + dir.dx * half, y: center.y + dir.dy * half)
        return copy
    }

    /// 绕中心旋转到指定角度（度）。
    func settingLinearAngle(_ degrees: Float) -> MaskUi {
        var copy = self
        let radians = Float(degrees) * .pi / 180
        let half = CGFloat(linearWidth / 2)
        let dir = CGVector(dx: CGFloat(cos(radians)), dy: CGFloat(sin(radians)))
        let center = linearCenter
        copy.start = CGPoint(x: center.x - dir.dx * half, y: center.y - dir.dy * half)
        copy.end = CGPoint(x: center.x + dir.dx * half, y: center.y + dir.dy * half)
        return copy
    }

    /// 平移中心线，使法线投影位置为 `percent`（-100..100）。
    func settingLinearPosition(_ percent: Float) -> MaskUi {
        var copy = self
        let normal = CGVector(dx: -linearDirection.dy, dy: linearDirection.dx)
        let current = Float(
            (linearCenter.x * normal.dx + linearCenter.y * normal.dy) * 100
        )
        let delta = CGFloat((percent - current) / 100)
        copy.start = CGPoint(x: start.x + normal.dx * delta, y: start.y + normal.dy * delta)
        copy.end = CGPoint(x: end.x + normal.dx * delta, y: end.y + normal.dy * delta)
        return copy
    }
}

// MARK: - 完整编辑状态

/// 一次编辑的完整状态（用于撤销 / 重做 / 保存）。
struct EditState: Equatable {
    var values: [String: Float] = EditorSpec.defaultValues()
    var curves = CurveState()
    var hsl = HslState()
    var grading = GradingState()
    /// 光学：删除色差（固定强度横向色差校正）。
    var removeChromaticAberration = false
    /// 光学：启用配置文件校正（核心内置档案：畸变 -0.06）。
    var profileCorrection = false
    var crop = CropState()
    var masks: [MaskUi] = []
    /// AI 修图参数（v1.5.0；修图只改像素，调色走 values / curves 等）。
    var retouch = RetouchUi()
}

// MARK: - JSON

/// EditParams JSON 的构造与解析（与 Rust `params.rs` / Android `buildParamsJson` 对齐）。
enum EditJSON {
    // MARK: 构造

    static func build(state: EditState, applyCropRect: Bool) -> String {
        var root: [String: Any] = ["version": 1]
        root["light"] = group(state.values, "light")
        root["color"] = group(state.values, "color")
        root["effects"] = group(state.values, "effects")
        root["detail"] = [
            "sharpen": group(state.values, "detail.sharpen"),
            "noise": group(state.values, "detail.noise"),
        ]
        root["curves"] = curvesJson(state.curves)
        root["hsl"] = hslJson(state.hsl)
        root["grading"] = gradingJson(state.grading)
        root["lens"] = [
            "profile_correction": state.profileCorrection,
            "chromatic_aberration": state.removeChromaticAberration ? 25.0 : 0.0,
            "vignette": 0.0,
        ]
        root["crop"] = cropJson(state.crop, applyRect: applyCropRect)
        root["masks"] = state.masks.map(maskJson)
        root["retouch"] = state.retouch.jsonObject
        return serialize(root)
    }

    /// 基础滑块分组（用于蒙版局部的 adjustments）。
    private static func group(_ values: [String: Float], _ group: String) -> [String: Double] {
        var result: [String: Double] = [:]
        for spec in EditorSpec.sliders where spec.group == group {
            let raw = values[spec.key] ?? spec.defaultValue
            result[spec.key] = (Double(raw) * 1000).rounded() / 1000
        }
        return result
    }

    private static func curvesJson(_ curves: CurveState) -> [String: Any] {
        func pointsJson(_ points: [CurvePoint]) -> [[String: Double]] {
            points.map { ["x": Double($0.x), "y": Double($0.y)] }
        }
        return [
            "rgb": ["points": pointsJson(curves.rgb)],
            "red": ["points": pointsJson(curves.red)],
            "green": ["points": pointsJson(curves.green)],
            "blue": ["points": pointsJson(curves.blue)],
            "parametric": [
                "highlights": Double(curves.parametric.highlights),
                "midtones": Double(curves.parametric.midtones),
                "shadows": Double(curves.parametric.shadows),
                "highlights_split": Double(curves.parametric.highlightsSplit),
                "midtones_split": Double(curves.parametric.midtonesSplit),
            ],
        ]
    }

    private static func hslJson(_ hsl: HslState) -> [String: Any] {
        var result: [String: Any] = [:]
        for kind in HslBandKind.allCases {
            let band = hsl[kind]
            result[kind.key] = [
                "hue": Double(band.hue),
                "saturation": Double(band.saturation),
                "luminance": Double(band.luminance),
            ]
        }
        return result
    }

    private static func gradingJson(_ grading: GradingState) -> [String: Any] {
        func zoneJson(_ zone: GradingZone) -> [String: Double] {
            [
                "hue": Double(zone.hue),
                "saturation": Double(zone.saturation),
                "luminance": Double(zone.luminance),
            ]
        }
        return [
            "shadows": zoneJson(grading.shadows),
            "midtones": zoneJson(grading.midtones),
            "highlights": zoneJson(grading.highlights),
            "blending": Double(grading.blending),
            "balance": Double(grading.balance),
        ]
    }

    private static func cropJson(_ crop: CropState, applyRect: Bool) -> [String: Any] {
        let rect = applyRect
            ? crop.rect
            : CGRect(x: 0, y: 0, width: 1, height: 1)
        return [
            "rect": [
                Double(rect.origin.x),
                Double(rect.origin.y),
                Double(rect.size.width),
                Double(rect.size.height),
            ],
            "aspect": crop.aspect.jsonName,
            "rotation_deg": Double(crop.rotation),
            "flip_horizontal": crop.flipHorizontal,
            "flip_vertical": crop.flipVertical,
            "grid": crop.grid.jsonName,
            "auto_compose": false,
        ]
    }

    private static func maskJson(_ mask: MaskUi) -> [String: Any] {
        var kindData: [String: Any] = [:]
        switch mask.kind {
        case .brush:
            kindData = [
                "size": Double(mask.brushSize),
                "feather": Double(mask.brushFeather),
                "flow": Double(mask.brushFlow),
                "density": Double(mask.brushDensity),
                "auto_mask": false,
                "strokes": mask.strokes.map { stroke in
                    [
                        "points": stroke.points.map { [Double($0.x), Double($0.y)] },
                        "erase": stroke.erase,
                        "size": Double(stroke.size),
                        "feather": Double(stroke.feather),
                        "flow": Double(stroke.flow),
                        "density": Double(stroke.density),
                    ] as [String: Any]
                },
            ]
        case .linear:
            kindData = [
                "start": [Double(mask.start.x), Double(mask.start.y)],
                "end": [Double(mask.end.x), Double(mask.end.y)],
            ]
        case .radial:
            // 羽化 → 内圈缩放（内圈越大过渡越窄；0 = 硬边缘）。
            let inner = max(0.02, min(0.98, 1 - Double(mask.radialFeather) / 100))
            kindData = [
                "center": [Double(mask.center.x), Double(mask.center.y)],
                "radii": [Double(mask.radii.width), Double(mask.radii.height)],
                "rotation": Double(mask.rotation),
                "inner": inner,
            ]
        case .aiSubject:
            kindData = [:]
        case .aiPortrait, .aiFace, .aiSkin, .aiEyes, .aiEyebrows, .aiNose, .aiLips, .aiTeeth:
            kindData = ["part": mask.kind.peoplePart?.rawValue ?? "body"]
        }
        return [
            "id": mask.id,
            "name": mask.name,
            "kind": ["type": mask.kind.jsonName, "data": kindData],
            "visible": mask.visible,
            "inverted": mask.inverted,
            "feather": Double(mask.feather),
            "opacity": Double(mask.opacity),
            "overlay": mask.overlay.jsonName,
            "adjustments": [
                "light": group(mask.values, "light"),
                "color": group(mask.values, "color"),
                "effects": group(mask.values, "effects"),
            ],
        ]
    }

    private static func serialize(_ root: [String: Any]) -> String {
        guard
            let data = try? JSONSerialization.data(withJSONObject: root, options: [.sortedKeys]),
            let text = String(data: data, encoding: .utf8)
        else {
            return "{\"version\":1}"
        }
        return text
    }

    // MARK: 解析

    static func parse(_ json: String) -> EditState {
        var state = EditState()
        guard let root = jsonObject(json) else { return state }
        for spec in EditorSpec.sliders {
            var node: Any? = root
            for key in spec.group.split(separator: ".") {
                node = (node as? [String: Any])?[String(key)]
            }
            if let number = (node as? [String: Any])?[spec.key] as? NSNumber {
                state.values[spec.key] = number.floatValue
            }
        }
        state.curves = parseCurves(root["curves"])
        state.hsl = parseHsl(root["hsl"])
        state.grading = parseGrading(root["grading"])
        if let lens = root["lens"] as? [String: Any] {
            state.profileCorrection = (lens["profile_correction"] as? Bool) ?? false
            let ca = (lens["chromatic_aberration"] as? NSNumber)?.floatValue ?? 0
            state.removeChromaticAberration = ca != 0
        }
        state.crop = parseCrop(root["crop"])
        state.masks = parseMasks(root["masks"])
        state.retouch = RetouchUi.from(root["retouch"])
        return state
    }

    private static func jsonObject(_ json: String) -> [String: Any]? {
        guard
            let data = json.data(using: .utf8),
            let object = try? JSONSerialization.jsonObject(with: data)
        else {
            return nil
        }
        return object as? [String: Any]
    }

    private static func parseCurves(_ value: Any?) -> CurveState {
        var curves = CurveState()
        guard let root = value as? [String: Any] else { return curves }
        func points(_ key: String) -> [CurvePoint] {
            guard
                let channel = root[key] as? [String: Any],
                let list = channel["points"] as? [[String: Any]]
            else {
                return CurveState.identity
            }
            let parsed = list.compactMap { item -> CurvePoint? in
                guard
                    let x = (item["x"] as? NSNumber)?.floatValue,
                    let y = (item["y"] as? NSNumber)?.floatValue
                else {
                    return nil
                }
                return CurvePoint(x: x, y: y)
            }.sorted { $0.x < $1.x }
            return parsed.count >= 2 ? parsed : CurveState.identity
        }
        curves.rgb = points("rgb")
        curves.red = points("red")
        curves.green = points("green")
        curves.blue = points("blue")
        if let parametric = root["parametric"] as? [String: Any] {
            func number(_ key: String, _ fallback: Float) -> Float {
                (parametric[key] as? NSNumber)?.floatValue ?? fallback
            }
            curves.parametric = ParametricValues(
                highlights: number("highlights", 0),
                midtones: number("midtones", 0),
                shadows: number("shadows", 0),
                highlightsSplit: number("highlights_split", 0.75),
                midtonesSplit: number("midtones_split", 0.25)
            )
        }
        return curves
    }

    private static func parseHsl(_ value: Any?) -> HslState {
        var hsl = HslState()
        guard let root = value as? [String: Any] else { return hsl }
        for kind in HslBandKind.allCases {
            guard let band = root[kind.key] as? [String: Any] else { continue }
            hsl[kind] = HslBand(
                hue: (band["hue"] as? NSNumber)?.floatValue ?? 0,
                saturation: (band["saturation"] as? NSNumber)?.floatValue ?? 0,
                luminance: (band["luminance"] as? NSNumber)?.floatValue ?? 0
            )
        }
        return hsl
    }

    private static func parseGrading(_ value: Any?) -> GradingState {
        var grading = GradingState()
        guard let root = value as? [String: Any] else { return grading }
        func zone(_ key: String) -> GradingZone {
            guard let item = root[key] as? [String: Any] else { return GradingZone() }
            return GradingZone(
                hue: (item["hue"] as? NSNumber)?.floatValue ?? 0,
                saturation: (item["saturation"] as? NSNumber)?.floatValue ?? 0,
                luminance: (item["luminance"] as? NSNumber)?.floatValue ?? 0
            )
        }
        grading.shadows = zone("shadows")
        grading.midtones = zone("midtones")
        grading.highlights = zone("highlights")
        grading.blending = (root["blending"] as? NSNumber)?.floatValue ?? 50
        grading.balance = (root["balance"] as? NSNumber)?.floatValue ?? 0
        return grading
    }

    private static func parseCrop(_ value: Any?) -> CropState {
        var crop = CropState()
        guard let root = value as? [String: Any] else { return crop }
        if let rect = root["rect"] as? [NSNumber], rect.count >= 4 {
            crop.rect = clampedCropRect(
                CGRect(
                    x: CGFloat(rect[0].floatValue),
                    y: CGFloat(rect[1].floatValue),
                    width: CGFloat(rect[2].floatValue),
                    height: CGFloat(rect[3].floatValue)
                )
            )
        }
        if let aspect = root["aspect"] as? String {
            crop.aspect = CropAspect.from(jsonName: aspect)
        }
        crop.rotation = (root["rotation_deg"] as? NSNumber)?.floatValue ?? 0
        crop.flipHorizontal = (root["flip_horizontal"] as? NSNumber)?.boolValue ?? false
        crop.flipVertical = (root["flip_vertical"] as? NSNumber)?.boolValue ?? false
        if let grid = root["grid"] as? String {
            crop.grid = CropGridUi.from(jsonName: grid)
        }
        return crop
    }

    private static func parseMasks(_ value: Any?) -> [MaskUi] {
        guard let list = value as? [[String: Any]] else { return [] }
        return list.compactMap { item -> MaskUi? in
            guard
                let kindObject = item["kind"] as? [String: Any],
                let typeName = kindObject["type"] as? String
            else {
                return nil
            }
            let data = kindObject["data"] as? [String: Any] ?? [:]
            guard let kind = MaskKindUi.from(jsonName: typeName, part: data["part"] as? String)
            else { return nil }
            var mask = MaskUi(
                id: item["id"] as? String ?? UUID().uuidString,
                name: item["name"] as? String ?? kind.rawValue,
                kind: kind
            )
            mask.inverted = (item["inverted"] as? NSNumber)?.boolValue ?? false
            mask.visible = (item["visible"] as? NSNumber)?.boolValue ?? true
            mask.feather = (item["feather"] as? NSNumber)?.floatValue ?? 0
            mask.opacity = (item["opacity"] as? NSNumber)?.floatValue ?? 100
            if let overlay = item["overlay"] as? String {
                mask.overlay = MaskOverlayColor.from(jsonName: overlay)
            }
            if let adjustments = item["adjustments"] as? [String: Any] {
                mask.values = parseAdjustments(adjustments)
            }
            switch kind {
            case .brush:
                mask.brushSize = (data["size"] as? NSNumber)?.floatValue ?? 30
                mask.brushFeather = (data["feather"] as? NSNumber)?.floatValue ?? 50
                mask.brushFlow = (data["flow"] as? NSNumber)?.floatValue ?? 100
                mask.brushDensity = (data["density"] as? NSNumber)?.floatValue ?? 100
                mask.strokes = parseBrushStrokes(data["strokes"], mask: mask)
            case .linear:
                if let start = data["start"] as? [NSNumber], start.count >= 2 {
                    mask.start = CGPoint(
                        x: CGFloat(start[0].floatValue), y: CGFloat(start[1].floatValue)
                    )
                }
                if let end = data["end"] as? [NSNumber], end.count >= 2 {
                    mask.end = CGPoint(
                        x: CGFloat(end[0].floatValue), y: CGFloat(end[1].floatValue)
                    )
                }
            case .radial:
                if let center = data["center"] as? [NSNumber], center.count >= 2 {
                    mask.center = CGPoint(
                        x: CGFloat(center[0].floatValue), y: CGFloat(center[1].floatValue)
                    )
                }
                if let radii = data["radii"] as? [NSNumber], radii.count >= 2 {
                    mask.radii = CGSize(
                        width: CGFloat(radii[0].floatValue),
                        height: CGFloat(radii[1].floatValue)
                    )
                }
                mask.rotation = (data["rotation"] as? NSNumber)?.floatValue ?? 0
                let inner = (data["inner"] as? NSNumber)?.floatValue ?? 0.75
                // 兼容两种写法：0..1 分数（Rust / 新版）或 0..100 百分比（旧版）。
                let innerFraction = inner <= 1 ? inner : inner / 100
                mask.radialFeather = max(0, min(100, (1 - innerFraction) * 100))
            case .aiSubject, .aiPortrait, .aiFace, .aiSkin,
                .aiEyes, .aiEyebrows, .aiNose, .aiLips, .aiTeeth:
                // 智能蒙版：栅格由 Vision 生成并提交给核心缓存（无几何参数）。
                break
            }
            return mask
        }
    }

    /// 解析画笔笔画：v0.4 对象格式（点 + 参数快照 + 擦除）与 v0.3 纯点数组格式。
    private static func parseBrushStrokes(_ value: Any?, mask: MaskUi) -> [BrushStrokeUi] {
        guard let list = value as? [Any] else { return [] }
        return list.compactMap { item -> BrushStrokeUi? in
            if let object = item as? [String: Any] {
                guard let rawPoints = object["points"] as? [[NSNumber]] else { return nil }
                let points = rawPoints.compactMap { pair -> CGPoint? in
                    pair.count >= 2
                        ? CGPoint(x: CGFloat(pair[0].floatValue), y: CGFloat(pair[1].floatValue))
                        : nil
                }
                guard !points.isEmpty else { return nil }
                return BrushStrokeUi(
                    points: points,
                    erase: (object["erase"] as? NSNumber)?.boolValue ?? false,
                    size: (object["size"] as? NSNumber)?.floatValue ?? mask.brushSize,
                    feather: (object["feather"] as? NSNumber)?.floatValue ?? mask.brushFeather,
                    flow: (object["flow"] as? NSNumber)?.floatValue ?? mask.brushFlow,
                    density: (object["density"] as? NSNumber)?.floatValue ?? mask.brushDensity
                )
            }
            // v0.3：纯点数组。
            if let legacy = item as? [[NSNumber]] {
                let points = legacy.compactMap { pair -> CGPoint? in
                    pair.count >= 2
                        ? CGPoint(x: CGFloat(pair[0].floatValue), y: CGFloat(pair[1].floatValue))
                        : nil
                }
                guard !points.isEmpty else { return nil }
                return BrushStrokeUi(
                    points: points,
                    size: mask.brushSize,
                    feather: mask.brushFeather,
                    flow: mask.brushFlow,
                    density: mask.brushDensity
                )
            }
            return nil
        }
    }

    /// 蒙版 adjustments → 滑块字典（与全局滑块同 key）。
    static func parseAdjustments(_ root: [String: Any]) -> [String: Float] {
        var values = EditorSpec.defaultValues()
        for spec in EditorSpec.sliders {
            guard spec.group.hasPrefix("light")
                || spec.group.hasPrefix("color")
                || spec.group.hasPrefix("effects")
            else {
                continue
            }
            var node: Any? = root
            for key in spec.group.split(separator: ".") {
                node = (node as? [String: Any])?[String(key)]
            }
            if let number = (node as? [String: Any])?[spec.key] as? NSNumber {
                values[spec.key] = number.floatValue
            }
        }
        return values
    }
}

// MARK: - 曲线求值（与 Rust `eval_curve` / `build_parametric_lut` 一致）

/// 单调三次 Hermite 曲线求值（Fritsch–Carlson，控制点处不过冲）。
func evaluateCurve(_ points: [CurvePoint], x: Float) -> Float {
    if points.isEmpty {
        return x
    }
    if points.count == 1 {
        return points[0].y
    }
    if x <= points[0].x {
        return points[0].y
    }
    guard let last = points.last else { return x }
    if x >= last.x {
        return last.y
    }
    var index = 0
    while index + 2 < points.count && x > points[index + 1].x {
        index += 1
    }
    let left = points[index]
    let right = points[index + 1]
    let span = max(right.x - left.x, 1e-6)
    let t = ((x - left.x) / span).clamped(to: 0...1)
    let m0 = curveTangent(points, index)
    let m1 = curveTangent(points, index + 1)
    let t2 = t * t
    let t3 = t2 * t
    let h00 = 2 * t3 - 3 * t2 + 1
    let h10 = t3 - 2 * t2 + t
    let h01 = -2 * t3 + 3 * t2
    let h11 = t3 - t2
    let value = h00 * left.y + h10 * span * m0 + h01 * right.y + h11 * span * m1
    return value.clamped(to: 0...1)
}

private func curveTangent(_ points: [CurvePoint], _ index: Int) -> Float {
    func secant(_ i: Int) -> Float {
        let span = max(points[i + 1].x - points[i].x, 1e-6)
        return (points[i + 1].y - points[i].y) / span
    }
    if index == 0 {
        return secant(0)
    }
    if index + 1 >= points.count {
        return secant(points.count - 2)
    }
    let previous = secant(index - 1)
    let next = secant(index)
    if previous * next <= 0 {
        return 0
    }
    let slope = (previous + next) * 0.5
    let limit = 3 * min(abs(previous), abs(next))
    return slope.clamped(to: -limit...limit)
}

/// 参数曲线求值（与 Rust `build_parametric_lut` 同公式）。
func evaluateParametric(_ values: ParametricValues, x: Float) -> Float {
    let highlightsSplit = values.highlightsSplit <= 0
        ? 0.75
        : values.highlightsSplit.clamped(to: 0.5...1)
    let midtonesSplit = values.midtonesSplit <= 0
        ? 0.25
        : values.midtonesSplit.clamped(to: 0...0.5)
    let upper = max(highlightsSplit, midtonesSplit + 0.1)
    let weightHighlights = smoothstep(midtonesSplit, upper, x)
    let weightShadows = 1 - weightHighlights
    let weightMidtones = max(1 - weightHighlights - weightShadows, 0)
    let delta = (values.highlights / 100) * weightHighlights * 0.35
        + (values.midtones / 100) * weightMidtones * 0.35
        + (values.shadows / 100) * weightShadows * 0.35
    return (x + delta).clamped(to: 0...1)
}

/// GLSL 风格 smoothstep。
func smoothstep(_ edge0: Float, _ edge1: Float, _ x: Float) -> Float {
    let t = ((x - edge0) / max(edge1 - edge0, 1e-6)).clamped(to: 0...1)
    return t * t * (3 - 2 * t)
}
