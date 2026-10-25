//! Camera profile: linear camera-space correction + optional per-channel tone curve.
//!
//! This is deliberately a small, format-agnostic core. A DCP/Adobe profile reader
//! can lower into [`CameraProfile`] later; the color math stays testable without
//! file I/O or third-party crates.

/// A provisional camera profile, applied to linear RGB before the editing pipeline.
///
/// The matrix maps input linear RGB to the working linear RGB. `curve` is an
/// optional per-channel cubic polynomial `c0 + c1*x + c2*x^2 + c3*x^3`, useful
/// for a camera's tone rendering. Values are clamped to `[0, 1]` at the end.
#[derive(Debug, Clone, PartialEq)]
pub struct CameraProfile {
    /// Row-major 3x3 matrix.
    pub matrix: [[f32; 3]; 3],
    /// Additive bias, in linear light.
    pub bias: [f32; 3],
    /// Per-channel cubic coefficients, constant term first.
    pub curve: [[f32; 4]; 3],
}

impl Default for CameraProfile {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl CameraProfile {
    /// Identity profile (no correction).
    pub const IDENTITY: Self = Self {
        matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        bias: [0.0; 3],
        curve: [[0.0, 1.0, 0.0, 0.0]; 3],
    };

    /// Apply the profile to one linear RGB triplet.
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let mut out = [0.0f32; 3];
        for (channel, value) in out.iter_mut().enumerate() {
            let mut v = self.bias[channel];
            for (input, coefficient) in self.matrix[channel].iter().zip(rgb) {
                v += input * coefficient;
            }
            let c = self.curve[channel];
            v = ((c[3] * v + c[2]) * v + c[1]) * v + c[0];
            *value = if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
        out
    }

    /// Apply to an RGBA8 buffer in linear-ish sRGB space is intentionally not
    /// provided: callers should keep the pipeline linear and convert once.
    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_leaves_linear_rgb_alone() {
        let p = CameraProfile::IDENTITY;
        assert_eq!(p.apply([0.1, 0.4, 0.9]), [0.1, 0.4, 0.9]);
        assert!(p.is_identity());
    }

    #[test]
    fn matrix_and_curve_are_applied() {
        let p = CameraProfile {
            matrix: [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            bias: [0.0; 3],
            curve: [[0.0, 1.0, 0.0, 0.0]; 3],
        };
        assert_eq!(p.apply([0.2, 0.3, 0.4]), [0.3, 0.2, 0.4]);
    }

    #[test]
    fn non_finite_input_is_clamped_to_black() {
        let p = CameraProfile::IDENTITY;
        assert_eq!(p.apply([f32::NAN, 0.5, f32::INFINITY]), [0.0, 0.0, 0.0]);
    }
}
