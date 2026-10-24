//! `.fdxmp` 预设格式：JSON + 可选内嵌 LUT 的 ZIP 包。
//!
//! 归档布局：
//!
//! ```text
//! preset.json          # PresetFile 序列化结果（UTF-8）
//! luts/<name>.cube     # 可选 LUT 资源
//! ```

use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::{FdxmpError, Result};
use crate::params::{ColorBand, EditParams};

/// 预设格式版本，写入 `schema_version` 字段。
pub const PRESET_SCHEMA_VERSION: u32 = 1;

/// 预设文件扩展名。
pub const PRESET_EXTENSION: &str = "fdxmp";

/// 预设主 JSON 在归档中的路径。
pub const PRESET_JSON_ENTRY: &str = "preset.json";

/// 内嵌 LUT 的归档目录前缀。
pub const PRESET_LUT_DIR: &str = "luts/";

/// 随预设一起打包的 LUT 资源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LutAsset {
    /// 归档内路径，必须以 `luts/` 开头。
    pub entry: String,
    /// 原始字节。
    #[serde(skip)]
    pub data: Vec<u8>,
}

/// `.fdxmp` 预设。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresetFile {
    /// 格式版本，必须为 [`PRESET_SCHEMA_VERSION`]。
    pub schema_version: u32,
    /// 预设名称。
    pub name: String,
    /// 作者。
    pub author: String,
    /// 创建时间（RFC 3339 / UTC）。
    pub created_at: String,
    /// 可选描述。
    #[serde(default)]
    pub description: String,
    /// 全部调色参数。
    pub params: EditParams,
    /// 内嵌 LUT 清单。
    #[serde(default)]
    pub luts: Vec<LutAsset>,
}

impl PresetFile {
    /// 用当前时间创建预设。
    pub fn new(name: impl Into<String>, author: impl Into<String>, params: EditParams) -> Self {
        Self {
            schema_version: PRESET_SCHEMA_VERSION,
            name: name.into(),
            author: author.into(),
            created_at: now_utc_rfc3339(),
            description: String::new(),
            params,
            luts: Vec::new(),
        }
    }

    /// 校验版本号是否受支持。
    pub fn validate(&self) -> Result<()> {
        if self.schema_version > PRESET_SCHEMA_VERSION {
            return Err(FdxmpError::Preset(format!(
                "schema_version {} 高于当前支持的 {}",
                self.schema_version, PRESET_SCHEMA_VERSION
            )));
        }
        for lut in &self.luts {
            if !lut.entry.starts_with(PRESET_LUT_DIR) {
                return Err(FdxmpError::Preset(format!(
                    "LUT 路径必须以 {PRESET_LUT_DIR} 开头：{}",
                    lut.entry
                )));
            }
        }
        Ok(())
    }

    /// 序列化为 `.fdxmp` 字节流（ZIP）。
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(Vec::new());
        self.write_to(&mut cursor)?;
        Ok(cursor.into_inner())
    }

    /// 从 `.fdxmp` 字节流反序列化。
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Self::read_from(Cursor::new(bytes))
    }

    /// 写入任意可写可定位目标。
    pub fn write_to<W: Write + Seek>(&self, writer: W) -> Result<()> {
        self.validate()?;
        let mut zip = ZipWriter::new(writer);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);

        zip.start_file(PRESET_JSON_ENTRY, options)?;
        let json = serde_json::to_vec_pretty(self)?;
        zip.write_all(&json)?;

        for lut in &self.luts {
            zip.start_file(lut.entry.as_str(), options)?;
            zip.write_all(&lut.data)?;
        }

        zip.finish()?;
        Ok(())
    }

    /// 从任意可读可定位来源读取。
    pub fn read_from<R: Read + Seek>(reader: R) -> Result<Self> {
        let mut archive = ZipArchive::new(reader)?;
        let mut preset: PresetFile = {
            let mut entry = archive.by_name(PRESET_JSON_ENTRY)?;
            let mut json = String::new();
            entry.read_to_string(&mut json)?;
            serde_json::from_str(&json)?
        };

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            if !name.starts_with(PRESET_LUT_DIR) || name.ends_with('/') {
                continue;
            }
            let mut data = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut data)?;
            if let Some(lut) = preset.luts.iter_mut().find(|lut| lut.entry == name) {
                lut.data = data;
            } else {
                preset.luts.push(LutAsset { entry: name, data });
            }
        }

        preset.validate()?;
        Ok(preset)
    }

    /// 写入文件（自动补扩展名由调用方负责）。
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let file = std::fs::File::create(path)?;
        self.write_to(std::io::BufWriter::new(file))?;
        Ok(())
    }

    /// 读取文件。
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        Self::read_from(std::io::BufReader::new(file))
    }

    /// 导入 Lightroom XMP 预设。
    ///
    /// 解析 `crs:` 属性（Lightroom 经典版 / Camera Raw 的 XMP 预设），
    /// 映射到本项目的参数模型：主滑块、细节、HSL 八色带、颜色分级 / 分离色调、
    /// 相机校准与黑白混色；点颜色与曲线（`ToneCurvePV2012` 的序列化格式复杂）暂不导入。
    pub fn from_lightroom_xmp(xmp: &str) -> Result<Self> {
        let mut params = EditParams::default();
        let mut imported = 0usize;

        for (key, value) in xmp_attributes(xmp) {
            let Some(target) = key.strip_prefix("crs:") else {
                continue;
            };
            let Some(number) = parse_xmp_number(&value) else {
                continue;
            };
            let applied = match target {
                "Exposure2012" => set(&mut params.light.exposure, number),
                "Contrast2012" => set(&mut params.light.contrast, number),
                "Highlights2012" => set(&mut params.light.highlights, number),
                "Shadows2012" => set(&mut params.light.shadows, number),
                "Whites2012" => set(&mut params.light.whites, number),
                "Blacks2012" => set(&mut params.light.blacks, number),
                "Temperature" => set(&mut params.color.temperature, kelvin_to_relative(number)),
                "Tint" => set(&mut params.color.tint, number),
                "Vibrance" => set(&mut params.color.vibrance, number),
                "Saturation" => set(&mut params.color.saturation, number),
                "Texture" => set(&mut params.effects.texture, number),
                "Clarity2012" => set(&mut params.effects.clarity, number),
                "Dehaze" => set(&mut params.effects.dehaze, number),
                "PostCropVignetteAmount" => set(&mut params.effects.vignette, number),
                "GrainAmount" => set(&mut params.effects.grain, number),
                "Sharpness" => set(&mut params.detail.sharpen.amount, number),
                "SharpenRadius" => set(&mut params.detail.sharpen.radius, number),
                "SharpenDetail" => set(&mut params.detail.sharpen.detail, number),
                "SharpenEdgeMasking" => set(&mut params.detail.sharpen.masking, number),
                "LuminanceSmoothing" => set(&mut params.detail.noise.luminance, number),
                "LuminanceNoiseReductionDetail" => {
                    set(&mut params.detail.noise.luminance_detail, number)
                }
                "LuminanceNoiseReductionContrast" => {
                    set(&mut params.detail.noise.luminance_contrast, number)
                }
                "ColorNoiseReduction" => set(&mut params.detail.noise.color, number),
                "ColorNoiseReductionDetail" => set(&mut params.detail.noise.color_detail, number),
                "ColorNoiseReductionSmoothness" => {
                    set(&mut params.detail.noise.color_smoothness, number)
                }
                _ => apply_xmp_extended(&mut params, target, number),
            };
            if applied {
                imported += 1;
            }
        }

        if imported == 0 {
            return Err(FdxmpError::Preset(
                "XMP 中未找到可导入的 crs: 参数（仅支持 Lightroom 经典版预设）".to_string(),
            ));
        }

        let name = xmp_attribute(xmp, "xmp:PresetName")
            .map(|value| value.to_string())
            .unwrap_or_else(|| "Imported XMP".to_string());
        let mut preset = PresetFile::new(name, "Lightroom XMP", params);
        preset.description = format!("从 Lightroom XMP 导入，识别 {imported} 个参数");
        Ok(preset)
    }
}

fn set(target: &mut f32, value: f32) -> bool {
    *target = value;
    true
}

/// Lightroom 的 8 个色带名 → 本项目的 [`ColorBand`]。
const XMP_BANDS: [(&str, ColorBand); 8] = [
    ("Red", ColorBand::Red),
    ("Orange", ColorBand::Orange),
    ("Yellow", ColorBand::Yellow),
    ("Green", ColorBand::Green),
    ("Aqua", ColorBand::Aqua),
    ("Blue", ColorBand::Blue),
    ("Purple", ColorBand::Purple),
    ("Magenta", ColorBand::Magenta),
];

/// 扩展字段：HSL 八色带、颜色分级 / 分离色调、相机校准、黑白。
///
/// 命名与 Lightroom 的 `crs:` 字段一致（`HueAdjustment<Band>`、`SaturationAdjustment<Band>`、
/// `LuminanceAdjustment<Band>`、`ColorGrade*`、`SplitToning*`、
/// `RedHue` / `RedSaturation` 等、`GrayMixer<Band>`、`ConvertToGrayscale`）。
fn apply_xmp_extended(params: &mut EditParams, target: &str, number: f32) -> bool {
    // HSL：前缀 + 色带名。
    for (prefix, field) in [
        ("HueAdjustment", 0u8),
        ("SaturationAdjustment", 1),
        ("LuminanceAdjustment", 2),
    ] {
        if let Some(rest) = target.strip_prefix(prefix) {
            if let Some((_, band)) = XMP_BANDS.iter().find(|(name, _)| *name == rest) {
                let hsl = params.hsl.band_mut(*band);
                match field {
                    0 => hsl.hue = number,
                    1 => hsl.saturation = number,
                    _ => hsl.luminance = number,
                }
                return true;
            }
        }
    }

    // 颜色分级（2018 之后的分裂色调字段）与旧的分离色调字段。
    let grading = match target {
        "ColorGradeShadowHue" | "SplitToningShadowHue" => Some((0u8, 0u8)),
        "ColorGradeShadowSat" | "SplitToningShadowSaturation" => Some((0, 1)),
        "ColorGradeShadowLum" => Some((0, 2)),
        "ColorGradeMidtoneHue" => Some((1, 0)),
        "ColorGradeMidtoneSat" => Some((1, 1)),
        "ColorGradeMidtoneLum" => Some((1, 2)),
        "ColorGradeHighlightHue" | "SplitToningHighlightHue" => Some((2, 0)),
        "ColorGradeHighlightSat" | "SplitToningHighlightSaturation" => Some((2, 1)),
        "ColorGradeHighlightLum" => Some((2, 2)),
        _ => None,
    };
    if let Some((zone, field)) = grading {
        let wheel = match zone {
            0 => &mut params.grading.shadows,
            1 => &mut params.grading.midtones,
            _ => &mut params.grading.highlights,
        };
        match field {
            0 => wheel.hue = number.rem_euclid(360.0),
            1 => wheel.saturation = number.clamp(0.0, 100.0),
            _ => wheel.luminance = number,
        }
        return true;
    }
    match target {
        "ColorGradeBlending" | "SplitToningBalance" => {
            params.grading.blending = number;
            return true;
        }
        "ColorGradeBalance" => {
            params.grading.balance = number;
            return true;
        }
        _ => {}
    }

    // 相机校准。
    match target {
        "RedHue" => return set(&mut params.calibration.red_hue, number),
        "RedSaturation" => return set(&mut params.calibration.red_sat, number),
        "GreenHue" => return set(&mut params.calibration.green_hue, number),
        "GreenSaturation" => return set(&mut params.calibration.green_sat, number),
        "BlueHue" => return set(&mut params.calibration.blue_hue, number),
        "BlueSaturation" => return set(&mut params.calibration.blue_sat, number),
        "ShadowTint" => return set(&mut params.calibration.shadows_tint, number),
        _ => {}
    }

    // 黑白：转换开关与 8 色带混色。
    if target == "ConvertToGrayscale" {
        params.color.black_white = number > 0.5;
        return true;
    }
    if let Some(rest) = target.strip_prefix("GrayMixer") {
        if let Some((_, band)) = XMP_BANDS.iter().find(|(name, _)| *name == rest) {
            let slot = match band {
                ColorBand::Red => &mut params.bw_mix.red,
                ColorBand::Orange => &mut params.bw_mix.orange,
                ColorBand::Yellow => &mut params.bw_mix.yellow,
                ColorBand::Green => &mut params.bw_mix.green,
                ColorBand::Aqua => &mut params.bw_mix.aqua,
                ColorBand::Blue => &mut params.bw_mix.blue,
                ColorBand::Purple => &mut params.bw_mix.purple,
                ColorBand::Magenta => &mut params.bw_mix.magenta,
            };
            *slot = number;
            return true;
        }
    }
    false
}

/// Lightroom 色温（开尔文）→ 相对值（5500K 为 0）。
///
/// 与渲染侧的 [`crate::white_balance::relative_to_kelvin`] 共用同一映射，
/// 保证"导入 XMP 的色温"与"显示 / 渲染的开尔文"一致。
fn kelvin_to_relative(kelvin: f32) -> f32 {
    crate::white_balance::kelvin_to_relative(kelvin)
}

/// 解析 `key="value"` 形式的扁平 XMP 属性。
fn xmp_attributes(xmp: &str) -> Vec<(String, String)> {
    let bytes = xmp.as_bytes();
    let mut attributes = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        // 找 key
        let key_start = index;
        while index < bytes.len()
            && (bytes[index].is_ascii_alphanumeric()
                || bytes[index] == b':'
                || bytes[index] == b'_')
        {
            index += 1;
        }
        let key = &xmp[key_start..index];
        // 跳空白，要求等号
        let mut cursor = index;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if key.is_empty() || cursor >= bytes.len() || bytes[cursor] != b'=' {
            index += 1;
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] != b'"' {
            index = cursor;
            continue;
        }
        cursor += 1;
        let value_start = cursor;
        while cursor < bytes.len() && bytes[cursor] != b'"' {
            cursor += 1;
        }
        if cursor <= bytes.len() {
            attributes.push((key.to_string(), xmp[value_start..cursor].to_string()));
        }
        index = cursor + 1;
    }
    attributes
}

fn xmp_attribute<'a>(xmp: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let start = xmp.find(&needle)? + needle.len();
    let rest = &xmp[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn parse_xmp_number(value: &str) -> Option<f32> {
    let trimmed = value.trim();
    // 处理 "+1.25" / "1,25" / "1.25"
    trimmed
        .trim_start_matches('+')
        .replace(',', ".")
        .parse::<f32>()
        .ok()
}

/// 内置预设（规格规定的 7 个分类，各一套基础参数）。
///
/// 桌面端与移动端的「内置预设」入口共用这里，保证四端一致。
pub fn builtin_presets() -> Vec<PresetFile> {
    use crate::params::{ColorBand, ColorGradingParams, CurvePoint, GradingZone, ToneCurve};

    let mut presets = Vec::new();

    let mut base = EditParams::default();
    base.light.contrast = 12.0;
    base.light.highlights = -15.0;
    base.light.shadows = 15.0;
    base.effects.clarity = 12.0;
    base.detail.sharpen.amount = 25.0;
    presets.push(PresetFile::new("基础校正", "fdxmp", base));

    let mut portrait = EditParams::default();
    portrait.light.exposure = 0.15;
    portrait.light.contrast = -5.0;
    portrait.light.shadows = 25.0;
    portrait.light.highlights = -10.0;
    portrait.color.temperature = 8.0;
    portrait.color.vibrance = 18.0;
    portrait.hsl.band_mut(ColorBand::Orange).saturation = 8.0;
    portrait.hsl.band_mut(ColorBand::Orange).luminance = 6.0;
    portrait.detail.noise.luminance = 15.0;
    portrait.detail.sharpen.masking = 40.0;
    presets.push(PresetFile::new("人像", "fdxmp", portrait));

    let mut landscape = EditParams::default();
    landscape.light.contrast = 22.0;
    landscape.light.highlights = -35.0;
    landscape.light.shadows = 30.0;
    landscape.light.whites = 12.0;
    landscape.effects.clarity = 22.0;
    landscape.effects.dehaze = 15.0;
    landscape.color.vibrance = 20.0;
    landscape.hsl.band_mut(ColorBand::Blue).saturation = 15.0;
    landscape.hsl.band_mut(ColorBand::Green).hue = -8.0;
    landscape.detail.sharpen.amount = 40.0;
    presets.push(PresetFile::new("风景", "fdxmp", landscape));

    let mut monochrome = EditParams::default();
    monochrome.color.saturation = -100.0;
    monochrome.light.contrast = 25.0;
    monochrome.light.blacks = -10.0;
    monochrome.effects.clarity = 18.0;
    monochrome.curves.rgb = ToneCurve {
        points: vec![
            CurvePoint { x: 0.0, y: 0.02 },
            CurvePoint { x: 0.25, y: 0.2 },
            CurvePoint { x: 0.75, y: 0.82 },
            CurvePoint { x: 1.0, y: 0.98 },
        ],
    };
    presets.push(PresetFile::new("黑白", "fdxmp", monochrome));

    let mut cinemagraph = EditParams::default();
    cinemagraph.light.contrast = 15.0;
    cinemagraph.light.highlights = -20.0;
    cinemagraph.light.shadows = 10.0;
    cinemagraph.grading = ColorGradingParams {
        shadows: GradingZone {
            hue: 210.0,
            saturation: 25.0,
            luminance: -5.0,
        },
        midtones: GradingZone::default(),
        highlights: GradingZone {
            hue: 35.0,
            saturation: 22.0,
            luminance: 4.0,
        },
        blending: 60.0,
        balance: 10.0,
    };
    cinemagraph.effects.grain = 15.0;
    cinemagraph.effects.vignette = 15.0;
    presets.push(PresetFile::new("电影", "fdxmp", cinemagraph));

    let mut vintage = EditParams::default();
    vintage.color.temperature = 20.0;
    vintage.color.tint = 6.0;
    vintage.light.contrast = -8.0;
    vintage.light.blacks = 18.0;
    vintage.effects.grain = 30.0;
    vintage.effects.vignette = 25.0;
    vintage.grading.shadows = GradingZone {
        hue: 40.0,
        saturation: 28.0,
        luminance: 6.0,
    };
    vintage.grading.highlights = GradingZone {
        hue: 55.0,
        saturation: 18.0,
        luminance: 0.0,
    };
    vintage.grading.blending = 55.0;
    vintage.curves.rgb = ToneCurve {
        points: vec![
            CurvePoint { x: 0.0, y: 0.08 },
            CurvePoint { x: 0.5, y: 0.5 },
            CurvePoint { x: 1.0, y: 0.94 },
        ],
    };
    presets.push(PresetFile::new("复古", "fdxmp", vintage));

    let mut hdr = EditParams::default();
    hdr.light.contrast = 18.0;
    hdr.light.highlights = -60.0;
    hdr.light.shadows = 60.0;
    hdr.light.whites = 15.0;
    hdr.light.blacks = -15.0;
    hdr.effects.clarity = 40.0;
    hdr.effects.dehaze = 20.0;
    hdr.detail.sharpen.amount = 45.0;
    hdr.detail.noise.luminance = 20.0;
    presets.push(PresetFile::new("HDR", "fdxmp", hdr));

    presets
}

/// 当前 UTC 时间的 RFC 3339 表示（无第三方时间库依赖）。
pub fn now_utc_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    unix_to_rfc3339(secs)
}

/// UNIX 秒 → `YYYY-MM-DDTHH:MM:SSZ`。
pub fn unix_to_rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant 的 civil_from_days 算法。
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_bytes_roundtrip() {
        let mut params = EditParams::default();
        params.light.exposure = 0.75;
        let mut preset = PresetFile::new("测试预设", "tester", params);
        preset.description = "阶段 0 往返测试".to_string();
        preset.luts.push(LutAsset {
            entry: format!("{PRESET_LUT_DIR}film.cube"),
            data: b"LUT_3D_SIZE 2\n".to_vec(),
        });

        let bytes = preset.to_bytes().unwrap();
        assert_eq!(&bytes[0..2], b"PK");

        let back = PresetFile::from_bytes(&bytes).unwrap();
        assert_eq!(back.name, "测试预设");
        assert_eq!(back.params.light.exposure, 0.75);
        assert_eq!(back.luts.len(), 1);
        assert_eq!(back.luts[0].data, b"LUT_3D_SIZE 2\n");
    }

    #[test]
    fn future_schema_is_rejected() {
        let mut preset = PresetFile::new("future", "tester", EditParams::default());
        preset.schema_version = PRESET_SCHEMA_VERSION + 1;
        let err = preset.to_bytes().unwrap_err();
        assert!(matches!(err, FdxmpError::Preset(_)));
    }

    #[test]
    fn timestamp_format_is_rfc3339() {
        assert_eq!(unix_to_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(unix_to_rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
        let now = now_utc_rfc3339();
        assert!(now.ends_with('Z') && now.len() == 20);
    }

    #[test]
    fn imports_lightroom_xmp_sliders() {
        let xmp = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
      xmp:PresetName="Punchy"
      crs:Exposure2012="+1.25"
      crs:Contrast2012="25"
      crs:Highlights2012="-40"
      crs:Shadows2012="30"
      crs:Temperature="7000"
      crs:Tint="12"
      crs:Vibrance="20"
      crs:Saturation="-5"
      crs:Clarity2012="15"
      crs:Dehaze="8"
      crs:PostCropVignetteAmount="-20"
      crs:GrainAmount="10"
      crs:Sharpness="60"
      crs:ColorNoiseReduction="35"
      crs:UnknownThing="1" />
</x:xmpmeta>"#;

        let preset = PresetFile::from_lightroom_xmp(xmp).unwrap();
        assert_eq!(preset.name, "Punchy");
        assert!((preset.params.light.exposure - 1.25).abs() < 1e-5);
        assert!((preset.params.light.contrast - 25.0).abs() < 1e-5);
        assert!((preset.params.light.highlights + 40.0).abs() < 1e-5);
        assert!((preset.params.color.tint - 12.0).abs() < 1e-5);
        assert!((preset.params.effects.dehaze - 8.0).abs() < 1e-5);
        assert!((preset.params.detail.noise.color - 35.0).abs() < 1e-5);
        // 7000K → (7000-5500)/90 ≈ 16.7（暖端每单位 90K）
        assert!((preset.params.color.temperature - 16.67).abs() < 0.5);
        assert!(preset.description.contains("识别"));
    }

    #[test]
    fn rejects_xmp_without_crs_parameters() {
        let err = PresetFile::from_lightroom_xmp("<xmpmeta></xmpmeta>").unwrap_err();
        assert!(matches!(err, FdxmpError::Preset(_)));
    }

    #[test]
    fn kelvin_mapping_is_clamped() {
        assert!((kelvin_to_relative(5500.0) - 0.0).abs() < 1e-6);
        // 冷端每单位 23K：3200K → -100（再冷就夹住）。
        assert!((kelvin_to_relative(3200.0) + 100.0).abs() < 1e-4);
        assert!((kelvin_to_relative(2000.0) + 100.0).abs() < 1e-4);
        // 暖端每单位 90K：14500K → +100；50000K 夹住。
        assert!((kelvin_to_relative(14500.0) - 100.0).abs() < 1e-4);
        assert!((kelvin_to_relative(50000.0) - 100.0).abs() < 1e-6);
    }

    #[test]
    fn builtin_presets_cover_seven_categories() {
        let presets = builtin_presets();
        assert_eq!(presets.len(), 7, "规格要求 7 个内置分类");
        let names: Vec<&str> = presets.iter().map(|preset| preset.name.as_str()).collect();
        for expected in ["基础校正", "人像", "风景", "黑白", "电影", "复古", "HDR"] {
            assert!(names.contains(&expected), "缺少内置预设：{expected}");
        }
        let monochrome = presets.iter().find(|preset| preset.name == "黑白").unwrap();
        assert!((monochrome.params.color.saturation + 100.0).abs() < 1e-6);
        let hdr = presets.iter().find(|preset| preset.name == "HDR").unwrap();
        assert!((hdr.params.light.shadows - 60.0).abs() < 1e-6);
        // 所有内置预设都能正常打包（schema 校验通过）
        for preset in &presets {
            assert!(preset.to_bytes().is_ok(), "{} 无法打包", preset.name);
        }
    }
}
