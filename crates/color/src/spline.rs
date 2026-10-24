//! 单调三次插值（Fritsch–Carlson）与一维 LUT：曲线面板 / 色调曲线的数值基础。
//!
//! 单调性是硬性要求：用户把曲线点拖过头也不会出现色调反转（Lightroom 同样保证这一点）。

/// 单调三次曲线（点必须按 x 严格递增）。
#[derive(Clone, Debug, PartialEq)]
pub struct MonotoneCurve {
    xs: Vec<f64>,
    ys: Vec<f64>,
    ms: Vec<f64>,
}

impl MonotoneCurve {
    /// 至少两个点、x 严格递增、数值有限时返回曲线，否则 `None`。
    pub fn new(points: &[(f64, f64)]) -> Option<Self> {
        if points.len() < 2 {
            return None;
        }
        let mut xs = Vec::with_capacity(points.len());
        let mut ys = Vec::with_capacity(points.len());
        for (i, (x, y)) in points.iter().enumerate() {
            if !x.is_finite() || !y.is_finite() {
                return None;
            }
            if i > 0 && *x <= xs[i - 1] {
                return None;
            }
            xs.push(*x);
            ys.push(*y);
        }
        let n = xs.len();
        let d: Vec<f64> = (0..n - 1)
            .map(|i| (ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i]))
            .collect();
        let mut ms = vec![0.0; n];
        ms[0] = d[0];
        ms[n - 1] = d[n - 2];
        for i in 1..n - 1 {
            ms[i] = if d[i - 1] * d[i] <= 0.0 {
                0.0
            } else {
                (d[i - 1] + d[i]) * 0.5
            };
        }
        // Fritsch–Carlson 限制：控制单调性。
        for i in 0..n - 1 {
            if d[i] == 0.0 {
                ms[i] = 0.0;
                ms[i + 1] = 0.0;
                continue;
            }
            let a = ms[i] / d[i];
            let b = ms[i + 1] / d[i];
            let s = a * a + b * b;
            if s > 9.0 {
                let t = 3.0 / s.sqrt();
                ms[i] = t * a * d[i];
                ms[i + 1] = t * b * d[i];
            }
        }
        Some(Self { xs, ys, ms })
    }

    /// 曲线取值；区间外按端点切线线性外推（曲线面板默认 0..1，超出部分由调用方夹取）。
    pub fn eval(&self, x: f64) -> f64 {
        let n = self.xs.len();
        if x <= self.xs[0] {
            return self.ys[0] + self.ms[0] * (x - self.xs[0]);
        }
        if x >= self.xs[n - 1] {
            return self.ys[n - 1] + self.ms[n - 1] * (x - self.xs[n - 1]);
        }
        let mut lo = 0usize;
        let mut hi = n - 1;
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if self.xs[mid] <= x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let (x0, x1) = (self.xs[lo], self.xs[lo + 1]);
        let (y0, y1) = (self.ys[lo], self.ys[lo + 1]);
        let (m0, m1) = (self.ms[lo], self.ms[lo + 1]);
        let h = x1 - x0;
        let t = (x - x0) / h;
        let t2 = t * t;
        let t3 = t2 * t;
        (2.0 * t3 - 3.0 * t2 + 1.0) * y0
            + (t3 - 2.0 * t2 + t) * h * m0
            + (-2.0 * t3 + 3.0 * t2) * y1
            + (t3 - t2) * h * m1
    }

    /// 采样成 `n` 点 LUT（供 GPU uniform / 查表使用）。
    pub fn to_lut(&self, n: usize) -> Lut1 {
        let n = n.max(2);
        let (x0, x1) = (self.xs[0], self.xs[self.xs.len() - 1]);
        Lut1 {
            v: (0..n)
                .map(|i| {
                    let x = x0 + (x1 - x0) * i as f64 / (n - 1) as f64;
                    self.eval(x) as f32
                })
                .collect(),
        }
    }
}

/// 定义域 0..1 的一维查找表。
#[derive(Clone, Debug, PartialEq)]
pub struct Lut1 {
    pub v: Vec<f32>,
}

impl Lut1 {
    pub fn from_fn(n: usize, f: impl Fn(f32) -> f32) -> Self {
        let n = n.max(2);
        Lut1 {
            v: (0..n).map(|i| f(i as f32 / (n - 1) as f32)).collect(),
        }
    }

    /// 线性插值取值（`x` 夹在 0..1）。
    #[inline]
    pub fn eval(&self, x: f32) -> f32 {
        let n = self.v.len();
        let f = x.clamp(0.0, 1.0) * (n - 1) as f32;
        let i = (f as usize).min(n - 2);
        let t = f - i as f32;
        self.v[i] + (self.v[i + 1] - self.v[i]) * t
    }

    /// `self ∘ other`：先 `other` 再 `self`（两者定义域都按 0..1 处理）。
    pub fn compose(&self, other: &Lut1) -> Lut1 {
        let n = self.v.len().max(other.v.len());
        Lut1::from_fn(n, |x| self.eval(other.eval(x)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_curve_is_identity() {
        let c = MonotoneCurve::new(&[(0.0, 0.0), (1.0, 1.0)]).unwrap();
        for i in 0..=100 {
            let x = i as f64 / 100.0;
            assert!((c.eval(x) - x).abs() < 1e-9, "{x}");
        }
        let lut = c.to_lut(256);
        for i in 0..=100 {
            let x = i as f32 / 100.0;
            assert!((lut.eval(x) - x).abs() < 1e-4, "{x}");
        }
    }

    #[test]
    fn monotone_even_with_wild_points() {
        let c = MonotoneCurve::new(&[(0.0, 0.0), (0.3, 0.9), (0.31, 0.92), (1.0, 1.0)]).unwrap();
        let mut prev = c.eval(0.0);
        for i in 1..=2000 {
            let x = i as f64 / 2000.0;
            let v = c.eval(x);
            assert!(v >= prev - 1e-9, "{x}: {v} < {prev}");
            assert!((-1e-6..=1.0 + 1e-6).contains(&v), "{x}: {v}");
            prev = v;
        }
    }

    #[test]
    fn invalid_points_are_rejected() {
        assert!(MonotoneCurve::new(&[(0.0, 0.0)]).is_none());
        assert!(MonotoneCurve::new(&[(0.0, 0.0), (0.0, 1.0)]).is_none());
        assert!(MonotoneCurve::new(&[(1.0, 0.0), (0.5, 1.0)]).is_none());
        assert!(MonotoneCurve::new(&[(0.0, f64::NAN), (1.0, 1.0)]).is_none());
    }

    #[test]
    fn compose_orders_correctly() {
        let double = Lut1::from_fn(64, |x| (x * 2.0).min(1.0));
        let half = Lut1::from_fn(64, |x| x * 0.5);
        let c = half.compose(&double); // 先 ×2 再 ×0.5 = 恒等
        assert!((c.eval(0.25) - 0.25).abs() < 0.02);
    }
}
