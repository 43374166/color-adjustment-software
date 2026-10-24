//! 传递函数（OETF/EOTF）：sRGB、纯 gamma、SMPTE ST 2084 (PQ)、ITU-R BT.2100 (HLG)。

/// sRGB 编码值 → 线性（IEC 61966-2-1）。
#[inline]
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// 线性 → sRGB 编码值。
#[inline]
pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// 纯 gamma：编码值 → 线性（`v^g`）。
#[inline]
pub fn gamma_to_linear(v: f32, g: f32) -> f32 {
    v.max(0.0).powf(g)
}

/// 纯 gamma：线性 → 编码值（`v^(1/g)`）。
#[inline]
pub fn linear_to_gamma(v: f32, g: f32) -> f32 {
    v.max(0.0).powf(1.0 / g)
}

const PQ_M1: f64 = 2610.0 / 16384.0;
const PQ_M2: f64 = 2523.0 / 4096.0 * 128.0;
const PQ_C1: f64 = 3424.0 / 4096.0;
const PQ_C2: f64 = 2413.0 / 4096.0 * 32.0;
const PQ_C3: f64 = 2392.0 / 4096.0 * 32.0;

/// ST 2084 逆 EOTF：绝对亮度/10000 → 码值 0..1。
pub fn pq_encode(y: f64) -> f64 {
    let y = y.clamp(0.0, 1.0).powf(PQ_M1);
    ((PQ_C1 + PQ_C2 * y) / (1.0 + PQ_C3 * y)).powf(PQ_M2)
}

/// ST 2084 EOTF：码值 → 绝对亮度/10000。
pub fn pq_decode(e: f64) -> f64 {
    let p = e.clamp(0.0, 1.0).powf(1.0 / PQ_M2);
    ((p - PQ_C1).max(0.0) / (PQ_C2 - PQ_C3 * p)).powf(1.0 / PQ_M1)
}

const HLG_A: f64 = 0.178_832_77;
const HLG_B: f64 = 0.284_668_92;
const HLG_C: f64 = 0.559_910_73;

/// BT.2100 HLG OETF（场景线性 0..1 → 信号）。
pub fn hlg_encode(e: f64) -> f64 {
    let e = e.max(0.0);
    if e <= 1.0 / 12.0 {
        (3.0 * e).sqrt()
    } else {
        HLG_A * (12.0 * e - HLG_B).ln() + HLG_C
    }
}

/// [`hlg_encode`] 的逆。
pub fn hlg_decode(s: f64) -> f64 {
    if s <= 0.5 {
        s * s / 3.0
    } else {
        (((s - HLG_C) / HLG_A).exp() + HLG_B) / 12.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_roundtrip() {
        for i in 0..=100 {
            let v = i as f32 / 100.0;
            assert!((linear_to_srgb(srgb_to_linear(v)) - v).abs() < 1e-5);
        }
        assert!((srgb_to_linear(0.0)).abs() < 1e-9);
        assert!((linear_to_srgb(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn pq_hlg_roundtrip() {
        for i in 0..=50 {
            let y = i as f64 / 50.0;
            assert!((pq_decode(pq_encode(y)) - y).abs() < 1e-9);
            assert!((hlg_decode(hlg_encode(y)) - y).abs() < 1e-9);
        }
        // 100 nit ≈ PQ 0.508
        assert!((pq_encode(0.01) - 0.508).abs() < 0.002);
    }
}
