//! 白平衡：滑杆（相对值）↔ 开尔文映射 + Bradford 色适应矩阵。
//!
//! 滑杆刻度与 Lightroom XMP 导入保持同一映射：`0` 对应 5500K；
//! 暖端（> 0）每单位 90K（+100 → 14500K），冷端（< 0）每单位 23K（-100 → 3200K）——
//! 旧的对称 55K/单位在冷端会一路压到 ~1000K，画面蓝得"像反相"。
//! （由 [`kelvin_to_relative`] / [`relative_to_kelvin`] 定义，两处必须一致）。
//! 实际校正在线性 sRGB 域用 Bradford 色适应完成：把"假定照明"白点
//! `W(T, tint)` 适应到参考白点 `W(5500K, 0)`——滑杆为 0 时矩阵恰为恒等。
//!
//! 下面的系数直接照抄公开公式（Kim et al. / Bradford / sRGB 矩阵），
//! 保留原始位数便于对照来源，允许 f32 精度告警。
#![allow(clippy::excessive_precision)]

/// 参考色温（滑杆 0 点，K）。
pub const REFERENCE_KELVIN: f32 = 5500.0;

/// 暖端（滑杆 > 0）每单位开尔文数：+100 → 14500K。
pub const KELVIN_PER_UNIT_WARM: f32 = 90.0;

/// 冷端（滑杆 < 0）每单位开尔文数：-100 → 3200K。
pub const KELVIN_PER_UNIT_COOL: f32 = 23.0;

/// 色调滑杆 ±100 对应的 Duv 偏移（垂直黑体轨迹，正 = 洋红方向）。
const TINT_DUV: f32 = 0.032;

/// Lightroom 色温（开尔文）→ 相对滑杆值（5500K 为 0±100）。
pub fn kelvin_to_relative(kelvin: f32) -> f32 {
    let delta = kelvin - REFERENCE_KELVIN;
    if delta >= 0.0 {
        (delta / KELVIN_PER_UNIT_WARM).clamp(0.0, 100.0)
    } else {
        (delta / KELVIN_PER_UNIT_COOL).clamp(-100.0, 0.0)
    }
}

/// 相对滑杆值 → 开尔文（[`kelvin_to_relative`] 的逆映射）。
pub fn relative_to_kelvin(relative: f32) -> f32 {
    if relative >= 0.0 {
        REFERENCE_KELVIN + relative * KELVIN_PER_UNIT_WARM
    } else {
        REFERENCE_KELVIN + relative * KELVIN_PER_UNIT_COOL
    }
}

/// Planckian locus 近似（Kim et al. 2002）：色温 → CIE xy。
fn kelvin_to_xy(kelvin: f32) -> [f32; 2] {
    let t = kelvin.clamp(1000.0, 40000.0);
    let t2 = t * t;
    let t3 = t2 * t;
    let x = if t <= 4000.0 {
        -0.266_123_9e9 / t3 - 0.234_358_9e6 / t2 + 0.877_695_6e3 / t + 0.179_910
    } else {
        -3.025_846_9e9 / t3 + 2.107_037_9e6 / t2 + 0.222_634_7e3 / t + 0.240_390
    };
    let y = if t <= 2222.0 {
        -1.106_381_4 * x * x * x - 1.348_110_2 * x * x + 2.185_558_32 * x - 0.202_196_83
    } else if t <= 4000.0 {
        -0.954_947_6 * x * x * x - 1.374_185_9 * x * x + 2.091_370_15 * x - 0.167_488_67
    } else {
        3.081_758_0 * x * x * x - 5.873_386_7 * x * x + 3.751_129_97 * x - 0.370_014_83
    };
    [x, y]
}

fn xy_to_uv(xy: [f32; 2]) -> [f32; 2] {
    let denom = -2.0 * xy[0] + 12.0 * xy[1] + 3.0;
    [4.0 * xy[0] / denom, 6.0 * xy[1] / denom]
}

fn uv_to_xy(uv: [f32; 2]) -> [f32; 2] {
    let denom = 2.0 * uv[0] - 8.0 * uv[1] + 4.0;
    [3.0 * uv[0] / denom, 2.0 * uv[1] / denom]
}

/// 色温 + 色调 → "假定照明"的 CIE xy 白点。
///
/// 色调滑杆与 Lightroom 一致：**+ 表示假定照明偏绿**，色适应校正后画面偏洋红
/// （与温度同理：+ 表示假定照明偏冷，校正后画面偏暖）。
fn white_point(temperature: f32, tint: f32) -> [f32; 2] {
    let kelvin = relative_to_kelvin(temperature);
    let base = kelvin_to_uv(kelvin);
    if tint.abs() < 1e-4 {
        return uv_to_xy(base);
    }
    // 轨迹切向 → 法向（取 +v 方向为"绿"）；正的 tint 把假定照明往绿侧推。
    let ahead = kelvin_to_uv(kelvin + 200.0);
    let behind = kelvin_to_uv(kelvin - 200.0);
    let tangent = normalize([ahead[0] - behind[0], ahead[1] - behind[1]]);
    let normal = [-tangent[1], tangent[0]];
    let normal = if normal[1] < 0.0 {
        [-normal[0], -normal[1]]
    } else {
        normal
    };
    let duv = (tint / 100.0) * TINT_DUV;
    uv_to_xy([base[0] + normal[0] * duv, base[1] + normal[1] * duv])
}

fn kelvin_to_uv(kelvin: f32) -> [f32; 2] {
    xy_to_uv(kelvin_to_xy(kelvin))
}

fn normalize(v: [f32; 2]) -> [f32; 2] {
    let len = (v[0] * v[0] + v[1] * v[1]).sqrt().max(1e-9);
    [v[0] / len, v[1] / len]
}

fn xy_to_xyz(xy: [f32; 2]) -> [f32; 3] {
    let y = xy[1].max(1e-6);
    [xy[0] / y, 1.0, (1.0 - xy[0] - xy[1]) / y]
}

fn mat3_mul(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0f32; 3]; 3];
    for (row, out_row) in out.iter_mut().enumerate() {
        for (col, value) in out_row.iter_mut().enumerate() {
            *value = (0..3).map(|k| a[row][k] * b[k][col]).sum();
        }
    }
    out
}

fn mat3_apply(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn mat3_inverse(m: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let det = if det.abs() < 1e-9 { 1e-9 } else { det };
    [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) / det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) / det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) / det,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) / det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) / det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) / det,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) / det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) / det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) / det,
        ],
    ]
}

/// Bradford 锥体响应矩阵。
const BRADFORD: [[f32; 3]; 3] = [
    [0.895_100_0, 0.266_400_0, -0.161_400_0],
    [-0.750_200_0, 1.713_500_0, 0.036_700_0],
    [0.038_900_0, -0.068_500_0, 1.029_600_0],
];

/// 线性 sRGB ↔ CIE XYZ（D65）。
const SRGB_TO_XYZ: [[f32; 3]; 3] = [
    [0.412_456_4, 0.357_576_1, 0.180_437_5],
    [0.212_672_9, 0.715_152_2, 0.072_175_0],
    [0.019_333_9, 0.119_192_0, 0.950_304_1],
];

const XYZ_TO_SRGB: [[f32; 3]; 3] = [
    [3.240_454_2, -1.537_138_5, -0.498_531_4],
    [-0.969_266_0, 1.876_010_8, 0.041_556_0],
    [0.055_643_4, -0.204_025_9, 1.057_225_2],
];

/// 由"照明白点"到"参考白点"的线性 sRGB 色适应矩阵（行主序）。
///
/// `temperature` / `tint` 为滑杆值（-100..=100）：
/// - 0 / 0 → 单位矩阵（不改动画面）；
/// - 温度 > 0（假定照明更冷）→ 画面变暖（R 升、B 降），与 Lightroom 方向一致；
/// - 色调 > 0 → 洋红方向。
#[must_use]
pub fn adaptation_matrix(temperature: f32, tint: f32) -> [[f32; 3]; 3] {
    let from = xy_to_xyz(white_point(
        temperature.clamp(-100.0, 100.0),
        tint.clamp(-100.0, 100.0),
    ));
    let to = xy_to_xyz(white_point(0.0, 0.0));
    let cone_from = mat3_apply(&BRADFORD, from);
    let cone_to = mat3_apply(&BRADFORD, to);
    let mut scale = [[0.0f32; 3]; 3];
    for i in 0..3 {
        scale[i][i] = cone_to[i] / cone_from[i].max(1e-6);
    }
    let adapt_xyz = mat3_mul(&mat3_inverse(&BRADFORD), &mat3_mul(&scale, &BRADFORD));
    mat3_mul(&XYZ_TO_SRGB, &mat3_mul(&adapt_xyz, &SRGB_TO_XYZ))
}

/// 用色适应矩阵变换线性 RGB（测试 / CPU 参考用）。
#[must_use]
pub fn apply_matrix(matrix: &[[f32; 3]; 3], rgb: [f32; 3]) -> [f32; 3] {
    mat3_apply(matrix, rgb)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn max_identity_error(matrix: &[[f32; 3]; 3]) -> f32 {
        let identity = [[1.0f32, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let mut worst = 0.0f32;
        for row in 0..3 {
            for col in 0..3 {
                worst = worst.max((matrix[row][col] - identity[row][col]).abs());
            }
        }
        worst
    }

    #[test]
    fn slider_zero_is_identity() {
        let matrix = adaptation_matrix(0.0, 0.0);
        assert!(
            max_identity_error(&matrix) < 1e-3,
            "0/0 应为单位矩阵：{matrix:?}"
        );
    }

    #[test]
    fn positive_temperature_warms() {
        let warm = apply_matrix(&adaptation_matrix(100.0, 0.0), [0.18, 0.18, 0.18]);
        assert!(warm[0] > 0.18, "R 应上升：{warm:?}");
        assert!(warm[2] < 0.18, "B 应下降：{warm:?}");
        let cool = apply_matrix(&adaptation_matrix(-100.0, 0.0), [0.18, 0.18, 0.18]);
        assert!(cool[2] > cool[0], "负温度应偏冷：{cool:?}");
    }

    #[test]
    fn positive_tint_goes_magenta() {
        let base = apply_matrix(&adaptation_matrix(0.0, 0.0), [0.2, 0.2, 0.2]);
        let magenta = apply_matrix(&adaptation_matrix(0.0, 100.0), [0.2, 0.2, 0.2]);
        assert!(
            magenta[1] < base[1],
            "色调 +100 应压低绿色（洋红方向）：{base:?} → {magenta:?}"
        );
        let green = apply_matrix(&adaptation_matrix(0.0, -100.0), [0.2, 0.2, 0.2]);
        assert!(green[1] > base[1], "色调 -100 应偏绿：{green:?}");
    }

    #[test]
    fn kelvin_roundtrips_through_slider() {
        // 范围外的开尔文会被滑杆夹住（-100 → 3200K / +100 → 14500K），只测范围内的往返。
        for kelvin in [3200.0f32, 4200.0, 5500.0, 6500.0, 9000.0, 14000.0] {
            let back = relative_to_kelvin(kelvin_to_relative(kelvin));
            assert!(
                (back - kelvin).abs() < 1.0,
                "{kelvin}K → 滑杆 → {back}K 往返偏差过大"
            );
        }
    }
}
