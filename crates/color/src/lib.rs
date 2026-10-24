//! fdxmp 色彩科学核心（纯 Rust、无平台依赖、可在四端与 WASM 上运行）。
//!
//! 分层：
//! - [`math`]：3×3 矩阵、CIE xy、RGB 色彩空间（sRGB / Display P3 / Rec.2020 / ProPhoto / Adobe RGB）
//!   与 Bradford 色适应；
//! - [`transfer`]：sRGB / 纯 gamma / PQ / HLG 传递函数；
//! - [`cct`]：黑体轨迹、色温 ↔ xy、色调（Duv）、mired、白平衡矩阵；
//! - [`perceptual`]：OkLab / OkLCh、HSV / HSL（调色算子的工作空间）；
//! - [`spline`]：单调三次曲线（曲线面板 / 色调曲线的数值基础）；
//! - [`bands`]：8 色带（红橙黄绿青蓝紫洋红）中心色相与单位分解权重；
//! - [`ops`]：自然饱和度 / 饱和度 / HSL 混色 / 点颜色 / 颜色分级 / 相机校准 / 黑白；
//! - [`tone`]：胶片式色调映射（对数域对比度 + 高光肩部 + 阴影趾部）与显示域曲线；
//! - [`gamut`]：色域软裁剪与色域映射。
//!
//! 算法来源与许可：
//! - 结构与常量选自公开标准与公开论文（CIE、IEC 61966-2-1、SMPTE ST 2084、ITU-R BT.2100、
//!   Kim et al. 2002 黑体轨迹近似、Bradford/CAT 色适应、Ottosson 的 OkLab/OkLCh、Fritsch–Carlson 单调插值）；
//! - 调色算子（自然饱和度 / 8 色带混色 / 点颜色 / 三分级 / 校准矩阵）的管线结构参考
//!   **LightCraft**（storytold/lightcraft，MIT OR Apache-2.0）的 `crates/color` 与 `crates/pipeline`，
//!   按 MIT 条款吸收，版权与归属见 `README.md`；
//! - **不含**任何来自 darktable 的代码（GPLv3，仅作为算法思路来源，见 `docs/COLOR_ENGINE_PLAN.md`）。
//!
//! 与 [`crate`] 无关的平台层（wgpu、FFI、文件 IO）一律不在此 crate 内，保证四端共用同一份数学。
#![forbid(unsafe_code)]

pub mod bands;
pub mod cct;
pub mod gamut;
pub mod math;
pub mod ops;
pub mod perceptual;
pub mod spline;
pub mod tone;
pub mod transfer;

pub use cct::{
    kelvin_to_mired, mired_to_kelvin, planckian_xy, temp_tint_to_xy, wb_matrix, xy_to_temp_tint,
};
pub use math::{
    bradford, luminance, Mat3, RgbSpace, Xy, ADOBE_RGB, D50, D65, DISPLAY_P3, PROPHOTO, REC2020,
    SRGB,
};
pub use ops::{
    calibrate, calibration_matrix, Calibration, ColorOps, Grading, Hsl, Mixer, PointColor, Wheel,
};
pub use perceptual::{lab_to_lch, lch_to_lab, oklab_from_rgb, rgb_from_oklab, wrap_angle};
pub use spline::{Lut1, MonotoneCurve};
pub use tone::ToneMap;
