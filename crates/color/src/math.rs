//! 基础矩阵与色彩空间（公开数学：CIE 色度学 + Bradford 色适应）。

/// CIE 1931 xy 色度坐标。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xy {
    pub x: f64,
    pub y: f64,
}

impl Xy {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// 对应 Y = 1 的 XYZ。
    pub fn to_xyz(self) -> [f64; 3] {
        if self.y.abs() < 1e-12 {
            return [0.0, 0.0, 0.0];
        }
        [self.x / self.y, 1.0, (1.0 - self.x - self.y) / self.y]
    }

    /// XYZ → xy（和为零时退回 D65，调用方通常不会遇到）。
    pub fn from_xyz(v: [f64; 3]) -> Xy {
        let s = v[0] + v[1] + v[2];
        if s.abs() < 1e-12 {
            D65
        } else {
            Xy::new(v[0] / s, v[1] / s)
        }
    }
}

pub const D65: Xy = Xy::new(0.3127, 0.3290);
pub const D50: Xy = Xy::new(0.3457, 0.3585);

/// 行主序 3×3 矩阵（f64 保证矩阵求逆与串联的精度，热路径用 [`Mat3::apply_f32`]）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat3(pub [[f64; 3]; 3]);

impl Mat3 {
    pub const IDENTITY: Mat3 = Mat3([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);

    pub fn diag(a: f64, b: f64, c: f64) -> Mat3 {
        Mat3([[a, 0.0, 0.0], [0.0, b, 0.0], [0.0, 0.0, c]])
    }

    #[allow(clippy::needless_range_loop)]
    pub fn mul(&self, o: &Mat3) -> Mat3 {
        let mut r = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                r[i][j] = (0..3).map(|k| self.0[i][k] * o.0[k][j]).sum();
            }
        }
        Mat3(r)
    }

    pub fn apply(&self, v: [f64; 3]) -> [f64; 3] {
        let m = &self.0;
        [
            m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
            m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
            m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
        ]
    }

    pub fn apply_f32(&self, v: [f32; 3]) -> [f32; 3] {
        let r = self.apply([v[0] as f64, v[1] as f64, v[2] as f64]);
        [r[0] as f32, r[1] as f32, r[2] as f32]
    }

    pub fn determinant(&self) -> f64 {
        let m = &self.0;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    pub fn inverse(&self) -> Option<Mat3> {
        let d = self.determinant();
        if !d.is_finite() || d.abs() < 1e-300 {
            return None;
        }
        let m = &self.0;
        let i = 1.0 / d;
        Some(Mat3([
            [
                (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * i,
                (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * i,
                (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * i,
            ],
            [
                (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * i,
                (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * i,
                (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * i,
            ],
            [
                (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * i,
                (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * i,
                (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * i,
            ],
        ]))
    }

    pub fn transpose(&self) -> Mat3 {
        let m = &self.0;
        Mat3([
            [m[0][0], m[1][0], m[2][0]],
            [m[0][1], m[1][1], m[2][1]],
            [m[0][2], m[1][2], m[2][2]],
        ])
    }

    pub fn to_f32(&self) -> [[f32; 3]; 3] {
        self.0.map(|r| r.map(|v| v as f32))
    }
}

/// RGB 色彩空间：基色 + 白点。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RgbSpace {
    pub name: &'static str,
    pub r: Xy,
    pub g: Xy,
    pub b: Xy,
    pub white: Xy,
}

pub const SRGB: RgbSpace = RgbSpace {
    name: "sRGB",
    r: Xy::new(0.640, 0.330),
    g: Xy::new(0.300, 0.600),
    b: Xy::new(0.150, 0.060),
    white: D65,
};

pub const DISPLAY_P3: RgbSpace = RgbSpace {
    name: "Display P3",
    r: Xy::new(0.680, 0.320),
    g: Xy::new(0.265, 0.690),
    b: Xy::new(0.150, 0.060),
    white: D65,
};

pub const REC2020: RgbSpace = RgbSpace {
    name: "Rec.2020",
    r: Xy::new(0.708, 0.292),
    g: Xy::new(0.170, 0.797),
    b: Xy::new(0.131, 0.046),
    white: D65,
};

pub const PROPHOTO: RgbSpace = RgbSpace {
    name: "ProPhoto RGB",
    r: Xy::new(0.734699, 0.265301),
    g: Xy::new(0.159597, 0.840403),
    b: Xy::new(0.036598, 0.000105),
    white: D50,
};

pub const ADOBE_RGB: RgbSpace = RgbSpace {
    name: "Adobe RGB (1998)",
    r: Xy::new(0.640, 0.330),
    g: Xy::new(0.210, 0.710),
    b: Xy::new(0.150, 0.060),
    white: D65,
};

impl RgbSpace {
    /// 本空间线性 RGB → XYZ（白点 Y = 1，相对自身白点）。
    pub fn to_xyz(&self) -> Mat3 {
        let p = [self.r.to_xyz(), self.g.to_xyz(), self.b.to_xyz()];
        let m = Mat3([
            [p[0][0], p[1][0], p[2][0]],
            [p[0][1], p[1][1], p[2][1]],
            [p[0][2], p[1][2], p[2][2]],
        ]);
        let s = m
            .inverse()
            .map_or([1.0; 3], |inv| inv.apply(self.white.to_xyz()));
        m.mul(&Mat3::diag(s[0], s[1], s[2]))
    }

    /// [`RgbSpace::to_xyz`] 的逆（退化基色时退回单位矩阵）。
    pub fn from_xyz(&self) -> Mat3 {
        self.to_xyz().inverse().unwrap_or(Mat3::IDENTITY)
    }

    /// 亮度权重（`to_xyz` 的 Y 行）。
    pub fn luma(&self) -> [f64; 3] {
        self.to_xyz().0[1]
    }

    /// 本空间线性 RGB → `dst` 线性 RGB（Bradford 白点适应）。
    pub fn to_space(&self, dst: &RgbSpace) -> Mat3 {
        let adapt = bradford(self.white, dst.white);
        dst.from_xyz().mul(&adapt).mul(&self.to_xyz())
    }
}

/// Bradford 锥体响应矩阵。
pub const BRADFORD: Mat3 = Mat3([
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
]);

/// Bradford 色适应矩阵（XYZ → XYZ）：把 `src` 白点映射到 `dst` 白点。
pub fn bradford(src: Xy, dst: Xy) -> Mat3 {
    if src == dst {
        return Mat3::IDENTITY;
    }
    let s = BRADFORD.apply(src.to_xyz());
    let d = BRADFORD.apply(dst.to_xyz());
    let Some(inv) = BRADFORD.inverse() else {
        return Mat3::IDENTITY;
    };
    inv.mul(&Mat3::diag(d[0] / s[0], d[1] / s[1], d[2] / s[2]))
        .mul(&BRADFORD)
}

/// 线性 RGB 的亮度（给定权重）。
#[inline]
pub fn luminance(rgb: [f32; 3], weights: [f32; 3]) -> f32 {
    rgb[0] * weights[0] + rgb[1] * weights[1] + rgb[2] * weights[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_matrix_matches_standard() {
        let m = SRGB.to_xyz();
        let want = [
            [0.4124, 0.3576, 0.1805],
            [0.2126, 0.7152, 0.0722],
            [0.0193, 0.1192, 0.9505],
        ];
        for (i, row) in want.iter().enumerate() {
            for (j, expect) in row.iter().enumerate() {
                assert!((m.0[i][j] - expect).abs() < 2e-4, "{i}{j} {}", m.0[i][j]);
            }
        }
    }

    #[test]
    fn rec2020_luma_weights() {
        let l = REC2020.luma();
        assert!((l[0] - 0.2627).abs() < 1e-3);
        assert!((l[1] - 0.6780).abs() < 1e-3);
        assert!((l[2] - 0.0593).abs() < 1e-3);
    }

    #[test]
    fn white_maps_to_white_and_roundtrips() {
        for sp in [SRGB, DISPLAY_P3, REC2020, PROPHOTO, ADOBE_RGB] {
            let w = sp.to_space(&REC2020).apply([1.0, 1.0, 1.0]);
            for c in w {
                assert!((c - 1.0).abs() < 1e-9, "{} {w:?}", sp.name);
            }
        }
        let a = SRGB.to_space(&PROPHOTO);
        let b = PROPHOTO.to_space(&SRGB);
        let v = b.apply(a.apply([0.2, 0.5, 0.9]));
        assert!(
            (v[0] - 0.2).abs() < 1e-9 && (v[1] - 0.5).abs() < 1e-9 && (v[2] - 0.9).abs() < 1e-9
        );
    }
}
