//! Minimal DCP (DNG Camera Profile) reader.
//!
//! DCP is a TIFF container. This module reads the tags needed by fdxmp:
//! profile name, ColorMatrix1/2, ForwardMatrix1/2, CalibrationIlluminant1/2
//! and ToneCurve. It deliberately does not interpret HueSatMap yet.
//!
//! Nothing here is copied from Adobe or darktable; tags are from the public
//! DNG specification and the parser is written from scratch.

use std::fs;
use std::path::Path;

use crate::error::{FdxmpError, Result};

/// Parsed DCP fields.
#[derive(Debug, Clone, PartialEq)]
pub struct DcpProfile {
    /// Profile name (`ProfileName`, tag 50936).
    pub name: String,
    /// Unique camera model (tag 50708).
    pub camera_model: String,
    /// ColorMatrix1 (tag 50721), maps XYZ to camera.
    pub color_matrix1: Option<[[f32; 3]; 3]>,
    /// ColorMatrix2 (tag 50722).
    pub color_matrix2: Option<[[f32; 3]; 3]>,
    /// ForwardMatrix1 (tag 50964), maps XYZ D50 to camera.
    pub forward_matrix1: Option<[[f32; 3]; 3]>,
    /// ForwardMatrix2 (tag 50965).
    pub forward_matrix2: Option<[[f32; 3]; 3]>,
    /// CalibrationIlluminant1 (tag 50778).
    pub illuminant1: Option<u16>,
    /// CalibrationIlluminant2 (tag 50779).
    pub illuminant2: Option<u16>,
    /// ToneCurve (tag 50940): input/output pairs.
    pub tone_curve: Vec<[f32; 2]>,
}

impl DcpProfile {
    /// Parse a DCP file from disk.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = fs::read(path)?;
        Self::parse(&bytes)
    }

    /// Parse a DCP from a TIFF byte buffer.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 8 {
            return Err(FdxmpError::Raw("DCP file too short".to_string()));
        }
        let little = match &bytes[0..2] {
            b"II" => true,
            b"MM" => false,
            _ => return Err(FdxmpError::Raw("DCP is not a TIFF container".to_string())),
        };
        let magic = read_u16(bytes, 2, little).ok_or_else(|| bad("TIFF magic"))?;
        // Standard TIFF uses 42; Adobe DCP uses the "RC" magic (0x4352).
        if magic != 42 && magic != 0x4352 {
            return Err(FdxmpError::Raw(format!(
                "unsupported DCP/TIFF magic: {magic}"
            )));
        }
        let ifd = read_u32(bytes, 4, little).ok_or_else(|| bad("IFD offset"))? as usize;
        let count = read_u16(bytes, ifd, little).ok_or_else(|| bad("IFD entry count"))? as usize;
        let mut profile = DcpProfile {
            name: String::new(),
            camera_model: String::new(),
            color_matrix1: None,
            color_matrix2: None,
            forward_matrix1: None,
            forward_matrix2: None,
            illuminant1: None,
            illuminant2: None,
            tone_curve: Vec::new(),
        };
        for index in 0..count {
            let entry = ifd + 2 + index * 12;
            let Some(tag) = read_u16(bytes, entry, little) else {
                continue;
            };
            let Some(kind) = read_u16(bytes, entry + 2, little) else {
                continue;
            };
            let Some(items) = read_u32(bytes, entry + 4, little) else {
                continue;
            };
            let Some(value_offset) = read_u32(bytes, entry + 8, little) else {
                continue;
            };
            let inline = type_size(kind).is_some_and(|size| size * items as usize <= 4);
            let offset = if inline {
                entry + 8
            } else {
                value_offset as usize
            };
            match tag {
                50708 => {
                    if let Some(raw) = read_ascii(bytes, offset, items as usize) {
                        profile.camera_model = raw;
                    }
                }
                50936 => {
                    if let Some(raw) = read_ascii(bytes, offset, items as usize) {
                        profile.name = raw;
                    }
                }
                50721 => profile.color_matrix1 = read_matrix(bytes, offset, items, kind, little),
                50722 => profile.color_matrix2 = read_matrix(bytes, offset, items, kind, little),
                50964 => profile.forward_matrix1 = read_matrix(bytes, offset, items, kind, little),
                50965 => profile.forward_matrix2 = read_matrix(bytes, offset, items, kind, little),
                50778 => profile.illuminant1 = read_u16(bytes, offset, little),
                50779 => profile.illuminant2 = read_u16(bytes, offset, little),
                50940 => profile.tone_curve = read_pairs(bytes, offset, items, kind, little),
                _ => {}
            }
        }
        Ok(profile)
    }

    /// Build a linear sRGB `CameraProfile` from ForwardMatrix1.
    ///
    /// DNG ForwardMatrix maps XYZ D50 to camera; this inverts it, adapts
    /// D50 -> D65 with Bradford, then converts XYZ D65 -> linear sRGB.
    /// This is only valid when the input RGB really is camera-native RGB.
    pub fn to_camera_profile(&self) -> Option<fdxmp_color::CameraProfile> {
        use fdxmp_color::{bradford, Mat3, D50, D65, SRGB};

        let forward = self.forward_matrix1?;
        let forward = Mat3(forward.map(|row| row.map(f64::from)));
        let inverse = forward.inverse()?;
        let adapt = bradford(D50, D65);
        let matrix = SRGB.from_xyz().mul(&adapt).mul(&inverse);
        Some(fdxmp_color::CameraProfile {
            matrix: matrix.0.map(|row| row.map(|value| value as f32)),
            bias: [0.0; 3],
            curve: [[0.0, 1.0, 0.0, 0.0]; 3],
        })
    }

    /// A short one-line summary.
    pub fn summary(&self) -> String {
        format!(
            "{} | matrix1={} | forward1={} | tone={}",
            if self.name.is_empty() {
                "DCP"
            } else {
                &self.name
            },
            self.color_matrix1.is_some(),
            self.forward_matrix1.is_some(),
            self.tone_curve.len()
        )
    }
}

fn bad(what: &str) -> FdxmpError {
    FdxmpError::Raw(format!("DCP parse error: {what}"))
}

fn type_size(kind: u16) -> Option<usize> {
    match kind {
        1 | 2 | 6 | 7 => Some(1),
        3 | 8 => Some(2),
        4 | 9 | 11 => Some(4),
        5 | 10 | 12 => Some(8),
        _ => None,
    }
}

fn read_u16(bytes: &[u8], offset: usize, little: bool) -> Option<u16> {
    let slice = bytes.get(offset..offset + 2)?;
    Some(if little {
        u16::from_le_bytes([slice[0], slice[1]])
    } else {
        u16::from_be_bytes([slice[0], slice[1]])
    })
}

fn read_u32(bytes: &[u8], offset: usize, little: bool) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(if little {
        u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]])
    } else {
        u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]])
    })
}

fn read_i32(bytes: &[u8], offset: usize, little: bool) -> Option<i32> {
    read_u32(bytes, offset, little).map(|value| value as i32)
}

fn read_ascii(bytes: &[u8], offset: usize, count: usize) -> Option<String> {
    let slice = bytes.get(offset..offset + count)?;
    let end = slice.iter().position(|b| *b == 0).unwrap_or(slice.len());
    Some(String::from_utf8_lossy(&slice[..end]).to_string())
}

fn read_rational(bytes: &[u8], offset: usize, little: bool, signed: bool) -> Option<f32> {
    if signed {
        let num = read_i32(bytes, offset, little)? as f32;
        let den = read_i32(bytes, offset + 4, little)? as f32;
        (den != 0.0).then_some(num / den)
    } else {
        let num = read_u32(bytes, offset, little)? as f32;
        let den = read_u32(bytes, offset + 4, little)? as f32;
        (den != 0.0).then_some(num / den)
    }
}

fn read_matrix(
    bytes: &[u8],
    offset: usize,
    items: u32,
    kind: u16,
    little: bool,
) -> Option<[[f32; 3]; 3]> {
    if items != 9 || !matches!(kind, 5 | 10) {
        return None;
    }
    let mut values = [0.0f32; 9];
    for (index, value) in values.iter_mut().enumerate() {
        *value = read_rational(bytes, offset + index * 8, little, kind == 10)?;
    }
    Some([
        [values[0], values[1], values[2]],
        [values[3], values[4], values[5]],
        [values[6], values[7], values[8]],
    ])
}

fn read_pairs(bytes: &[u8], offset: usize, items: u32, kind: u16, little: bool) -> Vec<[f32; 2]> {
    if !matches!(kind, 5 | 10) {
        return Vec::new();
    }
    let mut out = Vec::with_capacity((items / 2) as usize);
    for index in 0..(items / 2) as usize {
        let input = read_rational(bytes, offset + index * 16, little, kind == 10);
        let output = read_rational(bytes, offset + index * 16 + 8, little, kind == 10);
        if let (Some(input), Some(output)) = (input, output) {
            out.push([input, output]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le_u16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }
    fn le_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }
    fn le_i32(out: &mut Vec<u8>, value: i32) {
        out.extend_from_slice(&value.to_le_bytes());
    }
    fn rational(out: &mut Vec<u8>, num: i32, den: i32) {
        le_i32(out, num);
        le_i32(out, den);
    }

    fn synthetic() -> Vec<u8> {
        // Header + 4 IFD entries + data. All offsets are little-endian.
        let count = 4u16;
        let ifd_offset = 8u32;
        let data_start = ifd_offset as usize + 2 + count as usize * 12;
        let name = b"Test DCP\0";
        let name_offset = data_start;
        let matrix_offset = name_offset + name.len();
        let curve_offset = matrix_offset + 9 * 8;
        let mut out = Vec::new();
        out.extend_from_slice(b"II");
        le_u16(&mut out, 42);
        le_u32(&mut out, ifd_offset);
        le_u16(&mut out, count);
        // ProfileName: ASCII, 9 bytes.
        le_u16(&mut out, 50936);
        le_u16(&mut out, 2);
        le_u32(&mut out, name.len() as u32);
        le_u32(&mut out, name_offset as u32);
        // ColorMatrix1: SRATIONAL[9].
        le_u16(&mut out, 50721);
        le_u16(&mut out, 10);
        le_u32(&mut out, 9);
        le_u32(&mut out, matrix_offset as u32);
        // CalibrationIlluminant1: SHORT, inline.
        le_u16(&mut out, 50778);
        le_u16(&mut out, 3);
        le_u32(&mut out, 1);
        le_u32(&mut out, 21);
        // ToneCurve: SRATIONAL[4] = 2 points.
        le_u16(&mut out, 50940);
        le_u16(&mut out, 10);
        le_u32(&mut out, 4);
        le_u32(&mut out, curve_offset as u32);
        out.extend_from_slice(name);
        for value in [1, 0, 0, 0, 1, 0, 0, 0, 1] {
            rational(&mut out, value, 1);
        }
        for (num, den) in [(0, 1), (0, 1), (1, 1), (1, 1)] {
            rational(&mut out, num, den);
        }
        out
    }

    #[test]
    fn parses_profile_name_matrix_curve() {
        let dcp = DcpProfile::parse(&synthetic()).unwrap();
        assert_eq!(dcp.name, "Test DCP");
        assert_eq!(dcp.illuminant1, Some(21));
        assert_eq!(
            dcp.color_matrix1,
            Some([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
        );
        assert_eq!(dcp.tone_curve, vec![[0.0, 0.0], [1.0, 1.0]]);
        assert!(dcp.summary().contains("Test DCP"));
    }

    #[test]
    fn rejects_non_tiff() {
        assert!(DcpProfile::parse(b"not a dcp").is_err());
    }

    #[test]
    fn forward_matrix_builds_camera_profile() {
        let dcp = DcpProfile {
            name: "test".to_string(),
            camera_model: "test".to_string(),
            color_matrix1: None,
            color_matrix2: None,
            forward_matrix1: Some([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            forward_matrix2: None,
            illuminant1: None,
            illuminant2: None,
            tone_curve: Vec::new(),
        };
        let profile = dcp.to_camera_profile().expect("forward matrix");
        assert!(profile
            .matrix
            .iter()
            .flatten()
            .all(|value| value.is_finite()));
        assert!(profile.matrix[0][0] > 0.5);
    }
}
