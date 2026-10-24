import Foundation

/// 保存预设时可以勾选的调整分组（勾上的才会写进预设 / 应用时覆盖）。
enum PresetSection: String, CaseIterable, Identifiable {
    case light = "亮度"
    case color = "颜色"
    case hsl = "HSL"
    case grading = "颜色分级"
    case effects = "效果"
    case detail = "细节"
    case curves = "曲线"

    var id: String { rawValue }
}

/// 预设文件夹（父）：用户自建；系统预设固定放在「系统预设」里。
struct PresetFolder: Identifiable, Equatable {
    var id: String
    var name: String
    var createdAt: Date
}

/// 用户预设（子）：属于某个文件夹，只包含勾选过的调整分组。
struct UserPreset: Identifiable, Equatable {
    var id: String
    var name: String
    var folderId: String
    var createdAt: Date
    /// 保存时勾选的分组（按 `PresetSection.allCases` 顺序）。
    var sections: [PresetSection]
    /// 滑块值（只含勾选分组里的字段）。
    var values: [String: Float]
    /// HSL：色带 key → [hue, saturation, luminance]。
    var hsl: [String: [Float]]
    /// 颜色分级：区域 key → [hue, saturation, luminance]，另含 blending / balance。
    var grading: [String: [Float]]
    /// 点曲线：通道 key（rgb / red / green / blue）→ [[x, y], …]。
    var curves: [String: [[Float]]]
    /// 参数曲线：highlights / midtones / shadows / highlights_split / midtones_split。
    var parametric: [String: Float]
    /// 相机校准（red_hue / red_sat / green_hue / green_sat / blue_hue / blue_sat / shadows_tint）。
    var calibration: [String: Float] = [:]
    /// 黑白模式（与 8 色带混色一起属于「颜色」分组）。
    var blackWhite: Bool = false
    /// 黑白混色：色带 key → 值。
    var bwMix: [String: Float] = [:]
    /// 点颜色样本（每个样本一个字典：lum / chroma / hue / hue_shift / …）。
    var pointColors: [[String: Float]] = []

    /// 列表 / 缩略图用的稳定引用。
    var ref: String { "u:" + id }

    /// 行内副标题：包含哪些分组。
    var sectionSummary: String {
        sections.map(\.rawValue).joined(separator: " · ")
    }

    /// 从当前状态生成用户预设（只收集勾选的分组）。
    static func make(
        from state: EditState,
        name: String,
        folderId: String,
        sections: Set<PresetSection>
    ) -> UserPreset {
        var values: [String: Float] = [:]
        for spec in EditorSpec.sliders {
            guard let section = Self.section(for: spec.category),
                  sections.contains(section) else { continue }
            values[spec.key] = state.values[spec.key] ?? spec.defaultValue
        }
        var hsl: [String: [Float]] = [:]
        if sections.contains(.hsl) {
            for kind in HslBandKind.allCases {
                let band = state.hsl[kind]
                hsl[kind.key] = [band.hue, band.saturation, band.luminance]
            }
        }
        var grading: [String: [Float]] = [:]
        if sections.contains(.grading) {
            for kind in GradingZoneKind.allCases {
                let zone = state.grading[kind]
                grading[kind.key] = [zone.hue, zone.saturation, zone.luminance]
            }
            grading["blending"] = [state.grading.blending]
            grading["balance"] = [state.grading.balance]
        }
        var curves: [String: [[Float]]] = [:]
        var parametric: [String: Float] = [:]
        if sections.contains(.curves) {
            curves["rgb"] = state.curves.rgb.map { [$0.x, $0.y] }
            curves["red"] = state.curves.red.map { [$0.x, $0.y] }
            curves["green"] = state.curves.green.map { [$0.x, $0.y] }
            curves["blue"] = state.curves.blue.map { [$0.x, $0.y] }
            parametric = [
                "highlights": state.curves.parametric.highlights,
                "midtones": state.curves.parametric.midtones,
                "shadows": state.curves.parametric.shadows,
                "highlights_split": state.curves.parametric.highlightsSplit,
                "midtones_split": state.curves.parametric.midtonesSplit,
            ]
        }
        // 相机校准 / 黑白 / 点颜色：都属于「颜色」分组（v1.7 新增面板）。
        var calibration: [String: Float] = [:]
        var blackWhite = false
        var bwMix: [String: Float] = [:]
        var pointColors: [[String: Float]] = []
        if sections.contains(.color) {
            calibration = [
                "red_hue": state.calibration.redHue,
                "red_sat": state.calibration.redSat,
                "green_hue": state.calibration.greenHue,
                "green_sat": state.calibration.greenSat,
                "blue_hue": state.calibration.blueHue,
                "blue_sat": state.calibration.blueSat,
                "shadows_tint": state.calibration.shadowsTint,
            ]
            blackWhite = state.blackWhite
            for kind in HslBandKind.allCases {
                bwMix[kind.key] = state.bwMix[kind]
            }
            pointColors = state.pointColors.map { sample in
                [
                    "lum": sample.lum,
                    "chroma": sample.chroma,
                    "hue": sample.hue,
                    "hue_shift": sample.hueShift,
                    "sat_shift": sample.satShift,
                    "lum_shift": sample.lumShift,
                    "variance": sample.variance,
                    "range": sample.range,
                    "hue_range": sample.hueRange,
                    "sat_range": sample.satRange,
                    "lum_range": sample.lumRange,
                ]
            }
        }
        return UserPreset(
            id: UUID().uuidString,
            name: name,
            folderId: folderId,
            createdAt: Date(),
            sections: PresetSection.allCases.filter { sections.contains($0) },
            values: values,
            hsl: hsl,
            grading: grading,
            curves: curves,
            parametric: parametric,
            calibration: calibration,
            blackWhite: blackWhite,
            bwMix: bwMix,
            pointColors: pointColors
        )
    }

    /// 分类 → 分组（滑块用）。
    static func section(for category: EditCategory) -> PresetSection? {
        switch category {
        case .light: return .light
        case .color: return .color
        case .effects: return .effects
        case .detail: return .detail
        case .curves: return .curves
        case .optics: return nil
        }
    }

    /// 把预设里勾选的分组应用到状态；没勾选的分组保持原样。
    func apply(to state: inout EditState) {
        let included = Set(sections)
        if included.contains(.light) { applyValues(to: &state, category: .light) }
        if included.contains(.color) { applyValues(to: &state, category: .color) }
        if included.contains(.effects) { applyValues(to: &state, category: .effects) }
        if included.contains(.detail) { applyValues(to: &state, category: .detail) }
        if included.contains(.hsl) { state.hsl = decodedHsl }
        if included.contains(.grading) { state.grading = decodedGrading }
        if included.contains(.curves) { state.curves = decodedCurves }
        // 相机校准 / 黑白 / 点颜色与「颜色」分组一起应用。
        if included.contains(.color) {
            state.calibration = decodedCalibration
            state.blackWhite = blackWhite
            state.bwMix = decodedBwMix
            state.pointColors = decodedPointColors
        }
    }

    /// 解码相机校准快照。
    private var decodedCalibration: CalibrationState {
        var result = CalibrationState()
        result.redHue = calibration["red_hue"] ?? 0
        result.redSat = calibration["red_sat"] ?? 0
        result.greenHue = calibration["green_hue"] ?? 0
        result.greenSat = calibration["green_sat"] ?? 0
        result.blueHue = calibration["blue_hue"] ?? 0
        result.blueSat = calibration["blue_sat"] ?? 0
        result.shadowsTint = calibration["shadows_tint"] ?? 0
        return result
    }

    private var decodedBwMix: BwMixState {
        var result = BwMixState()
        for kind in HslBandKind.allCases {
            if let value = bwMix[kind.key] {
                result[kind] = value
            }
        }
        return result
    }

    private var decodedPointColors: [PointColorSample] {
        pointColors.prefix(pointColorSampleLimit).map { raw in
            PointColorSample(
                lum: raw["lum"] ?? 0.5,
                chroma: raw["chroma"] ?? 0.1,
                hue: raw["hue"] ?? 0,
                hueShift: raw["hue_shift"] ?? 0,
                satShift: raw["sat_shift"] ?? 0,
                lumShift: raw["lum_shift"] ?? 0,
                variance: raw["variance"] ?? 0,
                range: raw["range"] ?? 50,
                hueRange: raw["hue_range"] ?? 50,
                satRange: raw["sat_range"] ?? 50,
                lumRange: raw["lum_range"] ?? 50
            )
        }
    }

    private func applyValues(to state: inout EditState, category: EditCategory) {
        for spec in EditorSpec.sliders where spec.category == category {
            state.values[spec.key] = values[spec.key] ?? spec.defaultValue
        }
    }

    /// 解码 HSL 快照。
    private var decodedHsl: HslState {
        var result = HslState()
        for kind in HslBandKind.allCases {
            guard let raw = hsl[kind.key], raw.count >= 3 else { continue }
            result[kind] = HslBand(hue: raw[0], saturation: raw[1], luminance: raw[2])
        }
        return result
    }

    /// 解码颜色分级快照。
    private var decodedGrading: GradingState {
        var result = GradingState()
        for kind in GradingZoneKind.allCases {
            guard let raw = grading[kind.key], raw.count >= 3 else { continue }
            result[kind] = GradingZone(hue: raw[0], saturation: raw[1], luminance: raw[2])
        }
        if let raw = grading["blending"]?.first { result.blending = raw }
        if let raw = grading["balance"]?.first { result.balance = raw }
        return result
    }

    /// 解码曲线快照。
    private var decodedCurves: CurveState {
        var result = CurveState()
        func points(_ key: String) -> [CurvePoint]? {
            guard let raw = curves[key], !raw.isEmpty else { return nil }
            return raw.compactMap { pair in
                pair.count >= 2 ? CurvePoint(x: pair[0], y: pair[1]) : nil
            }
        }
        if let rgb = points("rgb") { result.rgb = rgb }
        if let red = points("red") { result.red = red }
        if let green = points("green") { result.green = green }
        if let blue = points("blue") { result.blue = blue }
        var values = ParametricValues()
        values.highlights = parametric["highlights"] ?? values.highlights
        values.midtones = parametric["midtones"] ?? values.midtones
        values.shadows = parametric["shadows"] ?? values.shadows
        values.highlightsSplit = parametric["highlights_split"] ?? values.highlightsSplit
        values.midtonesSplit = parametric["midtones_split"] ?? values.midtonesSplit
        result.parametric = values
        return result
    }
}

extension BuiltinPreset {
    /// 列表 / 缩略图用的稳定引用（系统预设）。
    var ref: String { "b:" + name }
}

/// 预设库的本地存储（UserDefaults，JSON 文本）。
enum PresetLibraryCoding {
    private static let foldersKey = "fdxmp_ios_preset_folders"
    private static let presetsKey = "fdxmp_ios_user_presets"

    /// 读取预设库（无数据 / 解析失败 → 空库）。
    static func load() -> (folders: [PresetFolder], presets: [UserPreset]) {
        (
            decodeFolders(UserDefaults.standard.string(forKey: foldersKey)),
            decodePresets(UserDefaults.standard.string(forKey: presetsKey))
        )
    }

    static func save(folders: [PresetFolder], presets: [UserPreset]) {
        let defaults = UserDefaults.standard
        if let json = encodeFolders(folders) {
            defaults.set(json, forKey: foldersKey)
        }
        if let json = encodePresets(presets) {
            defaults.set(json, forKey: presetsKey)
        }
    }

    // MARK: 编码

    private static func encodeFolders(_ folders: [PresetFolder]) -> String? {
        let list: [[String: Any]] = folders.map { folder in
            [
                "id": folder.id,
                "name": folder.name,
                "created": folder.createdAt.timeIntervalSince1970,
            ]
        }
        return jsonString(list)
    }

    private static func encodePresets(_ presets: [UserPreset]) -> String? {
        let list: [[String: Any]] = presets.map { preset in
            let curves: [String: [[Double]]] = preset.curves.mapValues { rows in
                rows.map { row in row.map { Double($0) } }
            }
            let values: [String: Double] = preset.values.mapValues { Double($0) }
            let parametric: [String: Double] = preset.parametric.mapValues { Double($0) }
            let hsl: [String: [Double]] = preset.hsl.mapValues { band in
                band.map { Double($0) }
            }
            let grading: [String: [Double]] = preset.grading.mapValues { zone in
                zone.map { Double($0) }
            }
            let calibration: [String: Double] = preset.calibration.mapValues { Double($0) }
            let bwMix: [String: Double] = preset.bwMix.mapValues { Double($0) }
            let pointColors: [[String: Double]] = preset.pointColors.map { sample in
                sample.mapValues { Double($0) }
            }
            var item: [String: Any] = [
                "id": preset.id,
                "name": preset.name,
                "folder": preset.folderId,
                "created": preset.createdAt.timeIntervalSince1970,
                "sections": preset.sections.map(\.rawValue),
                "values": values,
                "parametric": parametric,
            ]
            if !curves.isEmpty { item["curves"] = curves }
            if !hsl.isEmpty { item["hsl"] = hsl }
            if !grading.isEmpty { item["grading"] = grading }
            if !calibration.isEmpty { item["calibration"] = calibration }
            if preset.blackWhite { item["black_white"] = true }
            if !bwMix.isEmpty { item["bw_mix"] = bwMix }
            if !pointColors.isEmpty { item["point_colors"] = pointColors }
            return item
        }
        return jsonString(list)
    }

    private static func jsonString(_ object: Any) -> String? {
        guard let data = try? JSONSerialization.data(withJSONObject: object) else { return nil }
        return String(data: data, encoding: .utf8)
    }

    // MARK: 解码

    private static func decodeFolders(_ json: String?) -> [PresetFolder] {
        guard let json, let data = json.data(using: .utf8) else { return [] }
        guard let object = try? JSONSerialization.jsonObject(with: data),
              let list = object as? [[String: Any]] else { return [] }
        return list.compactMap { item in
            guard let id = item["id"] as? String,
                  let name = item["name"] as? String else { return nil }
            let created = (item["created"] as? NSNumber)?.doubleValue ?? 0
            return PresetFolder(
                id: id,
                name: name,
                createdAt: Date(timeIntervalSince1970: created)
            )
        }
    }

    private static func decodePresets(_ json: String?) -> [UserPreset] {
        guard let json, let data = json.data(using: .utf8) else { return [] }
        guard let object = try? JSONSerialization.jsonObject(with: data),
              let list = object as? [[String: Any]] else { return [] }
        return list.compactMap { item in
            guard let id = item["id"] as? String,
                  let name = item["name"] as? String,
                  let folder = item["folder"] as? String else { return nil }
            let created = (item["created"] as? NSNumber)?.doubleValue ?? 0
            let sections = (item["sections"] as? [String] ?? [])
                .compactMap { PresetSection(rawValue: $0) }
            let values = floatDict(item["values"])
            let parametric = floatDict(item["parametric"])
            var hsl: [String: [Float]] = [:]
            if let raw = item["hsl"] as? [String: Any] {
                for (key, value) in raw {
                    hsl[key] = floats(value)
                }
            }
            var grading: [String: [Float]] = [:]
            if let raw = item["grading"] as? [String: Any] {
                for (key, value) in raw {
                    grading[key] = floats(value)
                }
            }
            var curves: [String: [[Float]]] = [:]
            if let raw = item["curves"] as? [String: Any] {
                for (key, value) in raw {
                    guard let rows = value as? [Any] else { continue }
                    curves[key] = rows.map { floats($0) }
                }
            }
            let calibration = floatDict(item["calibration"])
            let blackWhite = item["black_white"] as? Bool ?? false
            let bwMix = floatDict(item["bw_mix"])
            var pointColors: [[String: Float]] = []
            if let raw = item["point_colors"] as? [[String: Any]] {
                pointColors = raw.map { floatDict($0) }
            }
            return UserPreset(
                id: id,
                name: name,
                folderId: folder,
                createdAt: Date(timeIntervalSince1970: created),
                sections: sections,
                values: values,
                hsl: hsl,
                grading: grading,
                curves: curves,
                parametric: parametric,
                calibration: calibration,
                blackWhite: blackWhite,
                bwMix: bwMix,
                pointColors: pointColors
            )
        }
    }

    private static func floatDict(_ raw: Any?) -> [String: Float] {
        guard let dict = raw as? [String: Any] else { return [:] }
        var result: [String: Float] = [:]
        for (key, value) in dict {
            if let number = value as? NSNumber {
                result[key] = number.floatValue
            }
        }
        return result
    }

    private static func floats(_ raw: Any?) -> [Float] {
        guard let list = raw as? [Any] else { return [] }
        return list.compactMap { ($0 as? NSNumber)?.floatValue }
    }
}
