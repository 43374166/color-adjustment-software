//! darktable UCS 2022 色彩空间（下称 dt UCS）。
//!
//! 四层坐标：
//! - **L\\***：感知明度（0..≈2.0989，白点 `L_white = L*(Y=1)`）；
//! - **UV\\***：色度坐标（`U'_star`、`V'_star`，可由 CIE xy 得到）；
//! - **JCH**：明度 `J = L*/L_white`、彩度 `C`、色相 `H`（弧度，`atan2`）；
//! - **HSB**：色相 `H`、饱和度 `S = C/B`、亮度 `B = J·(C^1.3365 + 1)`。
//!
//! 也提供色域边界表 [`Gamut`]：在 xy 平面按角度步进求 RGB 色域的边界，
//! 存成 `M² = U'²+V'²` 的 512 点循环表，用于 [`Gamut::map_hsb`] 的软裁剪。
//!
//! 公式与常数取自公开的 UCS 2022 定义（Aurélien Pierre, 2022），
//! 本文件是独立 Rust 实现，不含 darktable 源码。数值保持 `f32`，
//! 与参考实现在 1e-6 量级内一致。
//!
//! 常数按公开定义里的小数位原样书写（多数超出 f32 精度），保持可核对性。
#![allow(clippy::excessive_precision)]

use crate::math::{bradford, Mat3, RgbSpace, D65};

/// 色域边界 LUT 的采样数（2 的幂，索引可用掩码取模）。
pub const LUT_ELEM: usize = 512;

/// `L*` 曲线的渐近上限（`Y → ∞`）。
const L_STAR_RANGE: f32 = 2.098_883_8;
/// 反解 `L* → Y` 时的裁剪上限，避免除零。
const L_STAR_UPPER: f32 = 2.098_85;
/// 彩度系数。
const CHROMA_SCALE: f32 = 15.932_993_652_962_535;
/// `C = CHROMA_SCALE · L*^CHROMA_L · M²^CHROMA_M / L_white`
const CHROMA_L: f32 = 0.652_399_752_473_801_8;
const CHROMA_M: f32 = 0.600_755_701_750_849_1;
/// `M²` 的反正幂。
const CHROMA_M_INV: f32 = 0.832_285_067_861_685_5;
/// HSB 中亮度对彩度的非线性耦合指数。
const HSB_COUPLING: f32 = 1.336_542_210_293_86;

/// D65 白点（CIE 1931 2°）。
pub const D65_XY: [f32; 2] = [D65.x as f32, D65.y as f32];

/// dt UCS 的 `L*` 上限（供外部裁剪）。
pub const L_STAR_MAX: f32 = L_STAR_UPPER;

/// Y（相对漫反射白 = 1）→ `L*`。
#[inline]
pub fn l_star_from_y(y: f32) -> f32 {
    if y <= 0.0 {
        return 0.0;
    }
    let y_hat = y.powf(0.631_651_345_306_265);
    L_STAR_RANGE * y_hat / (y_hat + 1.124_267_737_493_57)
}

/// `L*` → Y。
#[inline]
pub fn y_from_l_star(l_star: f32) -> f32 {
    let l = l_star.clamp(0.0, L_STAR_UPPER);
    (1.124_267_737_493_57 * l / (L_STAR_RANGE - l)).powf(1.583_151_856_527_964_8)
}

/// 白点对应的 `L_white`（`white_level = 1` 时即漫反射白）。
#[inline]
pub fn l_white_of(white_y: f32) -> f32 {
    l_star_from_y(white_y)
}

/// XYZ（D65）→ xyY。负值先裁剪到 0，全黑退回白点色度。
#[inline]
pub fn xyz_to_xyy(xyz: [f32; 3]) -> [f32; 3] {
    let x = xyz[0].max(0.0);
    let y = xyz[1].max(0.0);
    let z = xyz[2].max(0.0);
    let sum = x + y + z;
    if sum > 0.0 {
        [x / sum, y / sum, y]
    } else {
        [D65_XY[0], D65_XY[1], y]
    }
}

/// xyY → XYZ。`y == 0` 时返回全零。
#[inline]
pub fn xyy_to_xyz(xyy: [f32; 3]) -> [f32; 3] {
    let x = xyy[0];
    let y = xyy[1];
    let big_y = xyy[2];
    if y == 0.0 {
        return [0.0; 3];
    }
    [big_y * x / y, big_y, big_y * (1.0 - x - y) / y]
}

/// 带符号的极小值裁剪（与 `MAX(FLT_MIN, v)` / `MIN(-FLT_MIN, v)` 等价）。
#[inline]
fn signed_min(v: f32) -> f32 {
    if v >= 0.0 {
        v.max(f32::MIN_POSITIVE)
    } else {
        v.min(-f32::MIN_POSITIVE)
    }
}

/// CIE xy（D65 观察者）→ dt UCS 的 `U'V'`。
#[inline]
pub fn xy_to_uv(xy: [f32; 2]) -> [f32; 2] {
    let x = xy[0];
    let y = xy[1];

    // 三个“分母通道”，第三个用于归一化。
    let uvd = [
        -0.783_941_002_840_055 * x + 0.277_512_987_809_202 * y + 0.153_836_578_598_858,
        0.745_273_540_913_283 * x - 0.205_375_866_083_878 * y - 0.165_478_376_301_988,
        0.318_707_282_433_486 * x + 2.167_436_927_321_58 * y + 0.291_320_554_395_942,
    ];
    let div = signed_min(uvd[2]);
    let u = uvd[0] / div;
    let v = uvd[1] / div;

    // 双曲压缩，把 ±∞ 映射到 ±因子。
    const FACTORS: [f32; 2] = [1.396_562_256_67, 1.451_395_428_7];
    const HALF: [f32; 2] = [1.492_173_529_29, 1.524_886_379_14];
    let u_star = FACTORS[0] * u / (u.abs() + HALF[0]);
    let v_star = FACTORS[1] * v / (v.abs() + HALF[1]);

    // 旋转/缩放，使色相分布更均匀。
    [
        -1.124_983_854_323_892 * u_star - 0.980_483_721_769_325 * v_star,
        1.863_233_150_986_72 * u_star + 1.971_853_092_390_862 * v_star,
    ]
}

/// dt UCS 的 `U'V'` → CIE xy。
#[inline]
pub fn uv_to_xy(uv: [f32; 2]) -> [f32; 2] {
    const FACTORS: [f32; 2] = [1.396_562_256_67, 1.451_395_428_7];
    const HALF: [f32; 2] = [1.492_173_529_29, 1.524_886_379_14];

    let u_star = -5.037_522_385_190_711 * uv[0] - 2.504_856_328_185_843 * uv[1];
    let v_star = 4.760_029_407_436_461 * uv[0] + 2.874_012_963_239_247 * uv[1];

    let u = -HALF[0] * u_star / (u_star.abs() - FACTORS[0]);
    let v = -HALF[1] * v_star / (v_star.abs() - FACTORS[1]);

    let xd = [
        0.167_171_472_114_775 * u + 0.141_299_802_443_708 * v - 0.008_015_313_008_505_82,
        -0.150_959_086_409_163 * u - 0.155_185_060_382_272 * v - 0.008_433_124_335_780_07,
        0.940_254_742_367_256 * u + 1.0 * v - 0.025_632_596_765_288_9,
    ];
    let div = signed_min(xd[2]);
    [xd[0] / div, xd[1] / div]
}

/// `L*` + `U'V'` → JCH。`L_white` 为白点明度。
#[inline]
pub fn luv_to_jch(l_star: f32, l_white: f32, uv: [f32; 2]) -> [f32; 3] {
    let m2 = uv[0] * uv[0] + uv[1] * uv[1];
    let j = l_star / l_white;
    let c = if m2 > 0.0 {
        CHROMA_SCALE * l_star.powf(CHROMA_L) * m2.powf(CHROMA_M) / l_white
    } else {
        0.0
    };
    [j, c, uv[1].atan2(uv[0])]
}

/// JCH → xyY。
pub fn jch_to_xyy(jch: [f32; 3], l_white: f32) -> [f32; 3] {
    let l_star = (jch[0] * l_white).clamp(0.0, L_STAR_UPPER);
    let c = jch[1].max(0.0);
    let m = if l_star > 0.0 {
        (c * l_white / (CHROMA_SCALE * l_star.powf(CHROMA_L))).powf(CHROMA_M_INV)
    } else {
        0.0
    };
    let uv = [m * jch[2].cos(), m * jch[2].sin()];
    let xy = uv_to_xy(uv);
    [xy[0], xy[1], y_from_l_star(l_star)]
}

/// JCH → HSB（`B = J·(C^1.3365 + 1)`，`S = C/B`）。
#[inline]
pub fn jch_to_hsb(jch: [f32; 3]) -> [f32; 3] {
    let c = jch[1].max(0.0);
    let b = jch[0] * (c.powf(HSB_COUPLING) + 1.0);
    let s = if b > 0.0 { c / b } else { 0.0 };
    [jch[2], s, b]
}

/// HSB → JCH。
#[inline]
pub fn hsb_to_jch(hsb: [f32; 3]) -> [f32; 3] {
    let c = hsb[1] * hsb[2];
    let j = if c > 0.0 {
        hsb[2] / (c.powf(HSB_COUPLING) + 1.0)
    } else {
        hsb[2]
    };
    [j, c, hsb[0]]
}

/// xyY → JCH（`L_white` 为白点明度）。
#[inline]
pub fn xyy_to_jch(xyy: [f32; 3], l_white: f32) -> [f32; 3] {
    luv_to_jch(l_star_from_y(xyy[2]), l_white, xy_to_uv([xyy[0], xyy[1]]))
}

/// XYZ（D65）→ JCH。
#[inline]
pub fn xyz_to_jch(xyz: [f32; 3], l_white: f32) -> [f32; 3] {
    xyy_to_jch(xyz_to_xyy(xyz), l_white)
}

/// JCH → XYZ（D65）。
#[inline]
pub fn jch_to_xyz(jch: [f32; 3], l_white: f32) -> [f32; 3] {
    xyy_to_xyz(jch_to_xyy(jch, l_white))
}

/// HSB → XYZ（D65）。
#[inline]
pub fn hsb_to_xyz(hsb: [f32; 3], l_white: f32) -> [f32; 3] {
    jch_to_xyz(hsb_to_jch(hsb), l_white)
}

/// 软裁剪：`soft` 以上用指数压缩到 `hard`。
#[inline]
pub fn soft_clip(x: f32, soft: f32, hard: f32) -> f32 {
    let norm = (hard - soft).max(f32::MIN_POSITIVE);
    if x > soft {
        soft + (1.0 - (-(x - soft) / norm).exp()) * norm
    } else {
        x
    }
}

/// RGB 色域在 dt UCS HSB 下的边界表。
///
/// 构造时按 0.02° 步进扫描 CIE xy 平面上的色域边界三角形（白点 + 三基色），
/// 记录每个色相角对应的 `M² = U'²+V'²` 平均值；`map_hsb` 用它把饱和度
/// 软裁剪进目标 RGB 色域。
#[derive(Clone)]
pub struct Gamut {
    lut: [f32; LUT_ELEM],
}

impl core::fmt::Debug for Gamut {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Gamut").finish_non_exhaustive()
    }
}

impl Gamut {
    /// 由「线性 RGB → XYZ D65」矩阵构建。
    pub fn new(rgb_to_xyz: &Mat3) -> Self {
        let m = rgb_to_xyz.0;
        the_gamut_from_matrix(&[
            [m[0][0] as f32, m[0][1] as f32, m[0][2] as f32],
            [m[1][0] as f32, m[1][1] as f32, m[1][2] as f32],
            [m[2][0] as f32, m[2][1] as f32, m[2][2] as f32],
        ])
    }

    /// 由「线性 RGB → XYZ D65」的 f32 矩阵构建。
    pub fn from_rgb_to_xyz(m: &[[f32; 3]; 3]) -> Self {
        the_gamut_from_matrix(m)
    }

    /// 给定色相（弧度）的色域边界 `M²`，线性插值。
    #[inline]
    pub fn max_colorfulness(&self, hue: f32) -> f32 {
        let x = LUT_ELEM as f32 * (hue + core::f32::consts::PI) / (2.0 * core::f32::consts::PI);
        let x_prev = x.floor();
        let x_next = x.ceil();
        let xi = (x_prev as i32) & (LUT_ELEM as i32 - 1);
        let xii = (x_next as i32) & (LUT_ELEM as i32 - 1);
        let y_prev = self.lut[xi as usize];
        if xi == xii {
            y_prev
        } else {
            y_prev + (x - x_prev) * (self.lut[xii as usize] - y_prev)
        }
    }

    /// 保持色相与亮度，把饱和度软裁剪进色域。
    pub fn map_hsb(&self, hsb: &mut [f32; 3], l_white: f32) {
        let jch = hsb_to_jch(*hsb);
        let max_colorfulness = self.max_colorfulness(jch[2]);
        let j = jch[0].max(0.0);
        let max_chroma =
            CHROMA_SCALE * (j * l_white).powf(CHROMA_L) * max_colorfulness.powf(CHROMA_M) / l_white;

        let boundary = jch_to_hsb([j, max_chroma, jch[2]]);
        hsb[1] = soft_clip(hsb[1], 0.8 * boundary[1], boundary[1]);
    }

    /// 512 点边界表（`M²`），供 GPU 上传。
    pub fn values(&self) -> &[f32; LUT_ELEM] {
        &self.lut
    }
}

/// 两角之差，折回 `[-π, π]`。
#[inline]
fn delta_h(a: f32, b: f32) -> f32 {
    let mut diff = a - b;
    if diff < -core::f32::consts::PI {
        diff += 2.0 * core::f32::consts::PI;
    } else if diff > core::f32::consts::PI {
        diff -= 2.0 * core::f32::consts::PI;
    }
    diff
}

fn the_gamut_from_matrix(m: &[[f32; 3]; 3]) -> Gamut {
    let mut accum = [0.0f32; LUT_ELEM];
    let mut count = [0.0f32; LUT_ELEM];

    // 三基色的 xyY：矩阵的列。
    let primaries = [
        [m[0][0], m[1][0], m[2][0]],
        [m[0][1], m[1][1], m[2][1]],
        [m[0][2], m[1][2], m[2][2]],
    ]
    .map(xyz_to_xyy);

    let white = D65_XY;
    let h_red = (primaries[0][1] - white[1]).atan2(primaries[0][0] - white[0]);
    let h_green = (primaries[1][1] - white[1]).atan2(primaries[1][0] - white[0]);
    let h_blue = (primaries[2][1] - white[1]).atan2(primaries[2][0] - white[0]);

    const STEPS: usize = 50 * LUT_ELEM;
    for i in 0..STEPS {
        let angle =
            -core::f32::consts::PI + (i as f32) / (STEPS as f32) * (2.0 * core::f32::consts::PI);
        let tan_angle = angle.tan();

        let t_1 = delta_h(angle, h_blue) / delta_h(h_red, h_blue);
        let t_2 = delta_h(angle, h_red) / delta_h(h_green, h_red);
        let t_3 = delta_h(angle, h_green) / delta_h(h_blue, h_green);

        let mut x_t = 0.0f32;
        let mut y_t = 0.0f32;

        if (0.0..=1.0).contains(&t_1) {
            let t = (white[1] - primaries[2][1] + tan_angle * (primaries[2][0] - white[0]))
                / (primaries[0][1] - primaries[2][1]
                    + tan_angle * (primaries[2][0] - primaries[0][0]));
            x_t = primaries[2][0] + t * (primaries[0][0] - primaries[2][0]);
            y_t = primaries[2][1] + t * (primaries[0][1] - primaries[2][1]);
        } else if (0.0..=1.0).contains(&t_2) {
            let t = (white[1] - primaries[0][1] + tan_angle * (primaries[0][0] - white[0]))
                / (primaries[1][1] - primaries[0][1]
                    + tan_angle * (primaries[0][0] - primaries[1][0]));
            x_t = primaries[0][0] + t * (primaries[1][0] - primaries[0][0]);
            y_t = primaries[0][1] + t * (primaries[1][1] - primaries[0][1]);
        } else if (0.0..=1.0).contains(&t_3) {
            let t = (white[1] - primaries[1][1] + tan_angle * (primaries[1][0] - white[0]))
                / (primaries[2][1] - primaries[1][1]
                    + tan_angle * (primaries[1][0] - primaries[2][0]));
            x_t = primaries[1][0] + t * (primaries[2][0] - primaries[1][0]);
            y_t = primaries[1][1] + t * (primaries[2][1] - primaries[1][1]);
        }

        let uv = xy_to_uv([x_t, y_t]);
        let hue = uv[1].atan2(uv[0]);
        let mut index = ((LUT_ELEM - 1) as f32 * (hue + core::f32::consts::PI)
            / (2.0 * core::f32::consts::PI))
            .round() as i32;
        index = index.rem_euclid(LUT_ELEM as i32);

        accum[index as usize] += uv[0] * uv[0] + uv[1] * uv[1];
        count[index as usize] += 1.0;
    }

    for k in 0..LUT_ELEM {
        accum[k] /= count[k].max(1.0);
    }
    Gamut { lut: accum }
}

/// 线性 RGB → XYZ D65 矩阵（非 D65 空间先做 Bradford 适配）。
pub fn rgb_to_xyz_d65(space: &RgbSpace) -> Mat3 {
    let m = space.to_xyz();
    if space.white == D65 {
        m
    } else {
        bradford(space.white, D65).mul(&m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::SRGB;

    fn srgb_to_xyz() -> Mat3 {
        rgb_to_xyz_d65(&SRGB)
    }

    #[test]
    fn l_star_roundtrips() {
        for y in [0.0, 0.002, 0.18, 0.5, 1.0, 4.0] {
            let l = l_star_from_y(y);
            let back = y_from_l_star(l);
            assert!((back - y).abs() < 1e-4 * y.max(1.0), "{y} -> {l} -> {back}");
        }
        assert!((l_star_from_y(1.0) - 0.988_04).abs() < 1e-3);
    }

    #[test]
    fn uv_roundtrips() {
        let white_xyz = srgb_to_xyz().apply([1.0, 1.0, 1.0]);
        let white = xyz_to_xyy([
            white_xyz[0] as f32,
            white_xyz[1] as f32,
            white_xyz[2] as f32,
        ]);
        let uv = xy_to_uv([white[0], white[1]]);
        assert!(uv[0].abs() < 1e-4 && uv[1].abs() < 1e-4, "{uv:?}");

        for xy in [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06], [0.4, 0.4]] {
            let uv = xy_to_uv(xy);
            let back = uv_to_xy(uv);
            assert!((back[0] - xy[0]).abs() < 1e-5, "{xy:?} -> {back:?}");
            assert!((back[1] - xy[1]).abs() < 1e-5, "{xy:?} -> {back:?}");
        }
    }

    #[test]
    fn jch_roundtrips() {
        let l_white = l_white_of(1.0);
        for rgb in [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.5, 0.5, 0.5],
            [0.2, 0.4, 0.8],
        ] {
            let xyz = srgb_to_xyz().apply(rgb.map(f64::from));
            let xyz = [xyz[0] as f32, xyz[1] as f32, xyz[2] as f32];
            let jch = xyz_to_jch(xyz, l_white);
            let back = jch_to_xyz(jch, l_white);
            for c in 0..3 {
                assert!(
                    (back[c] - xyz[c]).abs() < 2e-4,
                    "{rgb:?} {xyz:?} -> {jch:?} -> {back:?}"
                );
            }
        }
    }

    #[test]
    fn hsb_roundtrips() {
        for hsb in [[0.3f32, 0.2, 0.8], [-2.9, 0.9, 0.1], [1.5, 0.0, 0.5]] {
            let back = jch_to_hsb(hsb_to_jch(hsb));
            for c in 0..3 {
                assert!((back[c] - hsb[c]).abs() < 1e-5, "{hsb:?} -> {back:?}");
            }
        }
    }

    /// sRGB 红在 dt UCS 中的色相约为 20°，这是颜色均衡器 `ANGLE_SHIFT` 的依据。
    #[test]
    fn srgb_red_hue_is_about_20_degrees() {
        let l_white = l_white_of(1.0);
        let xyz = srgb_to_xyz().apply([1.0, 0.0, 0.0]);
        let jch = xyz_to_jch([xyz[0] as f32, xyz[1] as f32, xyz[2] as f32], l_white);
        let deg = jch[2].to_degrees();
        assert!((deg - 20.0).abs() < 1.5, "red hue = {deg}°");
    }

    #[test]
    fn gamut_boundary_is_finite_and_positive() {
        let gamut = Gamut::new(&srgb_to_xyz());
        for deg in (0..360).step_by(7) {
            let m2 = gamut.max_colorfulness((deg as f32).to_radians());
            assert!(m2.is_finite() && m2 > 0.0, "{deg}° -> {m2}");
        }
    }

    #[test]
    fn gamut_map_keeps_neutral_untouched() {
        let gamut = Gamut::new(&srgb_to_xyz());
        let l_white = l_white_of(1.0);
        let mut hsb = [0.0, 0.0, 0.5];
        gamut.map_hsb(&mut hsb, l_white);
        assert_eq!(hsb, [0.0, 0.0, 0.5]);
    }
}
