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

/// 蒙版类型（首期三种：线性渐变 / 径向渐变 / 画笔）。
enum MaskKindUi: String, CaseIterable, Identifiable {
    case linear = "线性渐变"
    case radial = "径向渐变"
    case brush = "画笔"

    var id: String { rawValue }

    var jsonName: String {
        switch self {
        case .brush: return "brush"
        case .linear: return "linear_gradient"
        case .radial: return "radial_gradient"
        }
    }

    var icon: String {
        switch self {
        case .brush: return "paintbrush.pointed"
        case .linear: return "line.diagonal"
        case .radial: return "circle.dashed"
        }
    }

    static func from(jsonName: String) -> MaskKindUi? {
        allCases.first { $0.jsonName == jsonName }
    }
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
                let typeName = kindObject["type"] as? String,
                let kind = MaskKindUi.from(jsonName: typeName)
            else {
                return nil
            }
            let data = kindObject["data"] as? [String: Any] ?? [:]
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
