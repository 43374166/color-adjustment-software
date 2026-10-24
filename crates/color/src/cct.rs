//! 色温 / 色调（白平衡的数值基础）。
//!
//! - 黑体轨迹用 Kim et al. (2002) 的公开三次近似；
//! - 色调 `tint` 以 CIE 1960 uv 中相对轨迹法线的偏移表示（±1 对应 ±0.032 Duv，
//!   正值 = 洋红方向，与 Lightroom 的 Tint 方向一致）；
//! - 色温 → 色调的逆解用轨迹上的粗扫 + 黄金分割细化，全部是公开数值方法。
//!
//! 与 fdxmp 现有滑杆的关系：`white_balance.rs` 的滑杆是"相对刻度"（5500K 为 0），
//! 本模块给出标准开尔文 / Duv 模型，两者可以用 [`kelvin_to_mired`] 与色调换算互相桥接，
//! 见 `docs/COLOR_ENGINE_PLAN.md` 的 M4。

use crate::math::{bradford, Mat3, RgbSpace, Xy};

/// `tint` 满刻度对应的 Duv 偏移（Lightroom 的色调滑杆量级）。
pub const TINT_DUV: f64 = 0.032;

/// 黑体轨迹近似（Kim et al. 2002）：色温 K → CIE 1931 xy。
pub fn planckian_xy(kelvin: f64) -> Xy {
    let t = kelvin.clamp(1000.0, 40_000.0);
    let t2 = t * t;
    let t3 = t2 * t;
    let x = if t <= 4000.0 {
        -0.266_123_9e9 / t3 - 0.234_358_9e6 / t2 + 0.877_695_6e3 / t + 0.179_910
    } else {
        -3.025_846_9e9 / t3 + 2.107_037_9e6 / t2 + 0.222_634_7e3 / t + 0.240_390
    };
    let y = if t <= 2222.0 {
        -1.106_381_38 * x * x * x - 1.348_110_27 * x * x + 2.185_558_33 * x - 0.202_196_83
    } else if t <= 4000.0 {
        -0.954_947_61 * x * x * x - 1.374_185_93 * x * x + 2.091_370_20 * x - 0.167_488_64
    } else {
        3.081_758_19 * x * x * x - 5.873_386_74 * x * x + 3.751_129_40 * x - 0.370_014_83
    };
    Xy::new(x, y)
}

/// CIE 1931 xy → CIE 1960 uv（色温距离在该空间近似均匀）。
#[inline]
pub fn xy_to_uv(xy: Xy) -> (f64, f64) {
    let d = -2.0 * xy.x + 12.0 * xy.y + 3.0;
    if d.abs() < 1e-12 {
        return (0.0, 0.0);
    }
    (4.0 * xy.x / d, 6.0 * xy.y / d)
}

/// CIE 1960 uv → CIE 1931 xy。
#[inline]
pub fn uv_to_xy(u: f64, v: f64) -> Xy {
    let d = 2.0 * u - 8.0 * v + 4.0;
    if d.abs() < 1e-12 {
        return Xy::new(0.3127, 0.3290);
    }
    Xy::new(3.0 * u / d, 2.0 * v / d)
}

/// 轨迹在 `kelvin` 处的单位法线（uv 空间，指向洋红一侧）。
fn locus_normal(kelvin: f64) -> (f64, f64) {
    let h = 1.0;
    let a = xy_to_uv(planckian_xy(kelvin - h));
    let b = xy_to_uv(planckian_xy(kelvin + h));
    let (du, dv) = (b.0 - a.0, b.1 - a.1);
    let len = (du * du + dv * dv).sqrt();
    if len < 1e-12 {
        return (0.0, -1.0);
    }
    let (tu, tv) = (du / len, dv / len);
    // 法线 (-tv, tu)：轨迹随 K 增大向左下移动，该法线指向 uv 图下方，即洋红一侧。
    (-tv, tu)
}

/// 色温 + 色调 → xy。`tint` ∈ [-1, 1]，正值朝洋红。
pub fn temp_tint_to_xy(kelvin: f64, tint: f64) -> Xy {
    let base = xy_to_uv(planckian_xy(kelvin));
    let (nu, nv) = locus_normal(kelvin);
    let duv = tint.clamp(-2.0, 2.0) * TINT_DUV;
    uv_to_xy(base.0 + nu * duv, base.1 + nv * duv)
}

/// xy → 色温 + 色调。返回 `(kelvin, tint)`，`tint` ∈ 约 [-2, 2]，正值朝洋红。
pub fn xy_to_temp_tint(xy: Xy) -> (f64, f64) {
    let (u, v) = xy_to_uv(xy);
    let dist2 = |t: f64| {
        let (lu, lv) = xy_to_uv(planckian_xy(t));
        (lu - u) * (lu - u) + (lv - v) * (lv - v)
    };
    // 粗扫（对 1/T 均匀，即 mired 均匀：25 mired ≈ 40000K，1000 mired = 1000K）
    let (mut best_t, mut best_d) = (6500.0_f64, f64::MAX);
    for i in 0..=240 {
        let t = mired_to_kelvin(25.0 + (1000.0 - 25.0) * i as f64 / 240.0);
        let d = dist2(t);
        if d < best_d {
            best_d = d;
            best_t = t;
        }
    }
    // 黄金分割细化
    let mut lo = (best_t / 1.08).max(1000.0);
    let mut hi = (best_t * 1.08).min(40_000.0);
    for _ in 0..60 {
        let m1 = lo + (hi - lo) * 0.382;
        let m2 = lo + (hi - lo) * 0.618;
        if dist2(m1) < dist2(m2) {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    let kelvin = (lo + hi) * 0.5;
    let base = xy_to_uv(planckian_xy(kelvin));
    let (nu, nv) = locus_normal(kelvin);
    let tint = ((u - base.0) * nu + (v - base.1) * nv) / TINT_DUV;
    (kelvin, tint)
}

/// 色温 → mired（`10^6 / K`）。
pub fn kelvin_to_mired(kelvin: f64) -> f64 {
    if kelvin <= 0.0 {
        0.0
    } else {
        1.0e6 / kelvin
    }
}

/// mired → 色温。
pub fn mired_to_kelvin(mired: f64) -> f64 {
    if mired <= 1.0 {
        1.0e6
    } else {
        1.0e6 / mired
    }
}

/// 白平衡矩阵：在 `space` 的线性 RGB 中把 `white` 渲染为中性（Bradford 适应到空间白点）。
pub fn wb_matrix(space: &RgbSpace, white: Xy) -> Mat3 {
    let adapt = bradford(white, space.white);
    space.from_xyz().mul(&adapt).mul(&space.to_xyz())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::D65;

    #[test]
    fn planckian_matches_known_points() {
        let d65 = planckian_xy(6504.0);
        assert!(
            (d65.x - D65.x).abs() < 0.01 && (d65.y - D65.y).abs() < 0.01,
            "{d65:?}"
        );
        let a = planckian_xy(2856.0); // 标准 A 光源
        assert!(
            (a.x - 0.4476).abs() < 0.02 && (a.y - 0.4074).abs() < 0.02,
            "{a:?}"
        );
    }

    #[test]
    fn temp_tint_roundtrip() {
        for &k in &[2800.0, 4000.0, 5500.0, 6500.0, 9000.0] {
            for &t in &[-0.6, -0.2, 0.0, 0.3, 0.8] {
                let xy = temp_tint_to_xy(k, t);
                let (k2, t2) = xy_to_temp_tint(xy);
                assert!((k2 - k).abs() / k < 0.02, "{k} -> {k2}");
                assert!((t2 - t).abs() < 0.05, "{t} -> {t2}");
            }
        }
    }

    #[test]
    fn tint_sign_is_magenta_positive() {
        // 正色调应在 uv 图中位于轨迹下方（v 更小），即偏洋红。
        let base = xy_to_uv(planckian_xy(5000.0));
        let plus = xy_to_uv(temp_tint_to_xy(5000.0, 1.0));
        assert!(plus.1 < base.1, "{base:?} {plus:?}");
    }

    #[test]
    fn wb_matrix_neutralizes_the_chosen_white() {
        use crate::math::SRGB;
        // 空间自身白点：矩阵应为恒等。
        let identity = wb_matrix(&SRGB, D65);
        let v = identity.apply([1.0, 1.0, 1.0]);
        assert!(
            (v[0] - 1.0).abs() < 1e-9 && (v[1] - 1.0).abs() < 1e-9,
            "{v:?}"
        );
        // 任意白点：一个"理想相机"在该光下记录到的 RGB（即该白点的 sRGB 坐标），
        // 乘以白平衡矩阵后必须变成等能灰。
        for (k, tint) in [(4000.0, 0.0), (6500.0, 0.3), (3000.0, -0.4)] {
            let white = temp_tint_to_xy(k, tint);
            let m = wb_matrix(&SRGB, white);
            let rgb = SRGB.from_xyz().apply(white.to_xyz());
            let v = m.apply(rgb);
            assert!(
                (v[0] - v[1]).abs() < 1e-6 && (v[1] - v[2]).abs() < 1e-6,
                "{k} {tint}: {v:?}"
            );
        }
    }

    #[test]
    fn mired_roundtrip() {
        for k in [2000.0, 5500.0, 6504.0] {
            assert!((mired_to_kelvin(kelvin_to_mired(k)) - k).abs() < 1e-6);
        }
    }
}
