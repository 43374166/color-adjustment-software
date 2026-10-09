//! 镜头配置文件校正：纯 Rust 版 LensFun（`lensfun` crate，数据库内置）。
//!
//! 与手动镜头校正（`geometry::LensModel`）互补：
//! - 手动：UI 滑杆即时调畸变 / 色差 / 暗角；
//! - 配置文件：按「相机 + 镜头 + 焦距 / 光圈 / 距离」从 LensFun 数据库
//!   插值出标定参数，做畸变、横向色差（TCA）与暗角校正。
//!
//! 校正以 CPU 参考实现完成（复用上游经验证的数学），供预览与导出共用。
//!
//! 许可提示：`lensfun` crate 为 LGPL-3.0-or-later（源自 LensFun 项目）；
//! 本仓库以源码形式分发，满足其重链接要求。

use lensfun::Modifier;

use crate::error::{FdxmpError, Result};

/// 相机机身与镜头类型（UI 侧选择配置时使用）。
pub use lensfun::{Camera, Lens};

/// LensFun 数据库句柄（内置约 5 MB XML，首次加载后复用）。
pub struct LensDatabase {
    inner: lensfun::Database,
}

impl LensDatabase {
    /// 载入内置数据库。
    pub fn load() -> Result<Self> {
        lensfun::Database::load_bundled()
            .map(|inner| Self { inner })
            .map_err(|err| FdxmpError::Unsupported(format!("镜头数据库加载失败：{err}")))
    }

    /// 相机数量。
    pub fn camera_count(&self) -> usize {
        self.inner.cameras.len()
    }

    /// 镜头数量。
    pub fn lens_count(&self) -> usize {
        self.inner.lenses.len()
    }

    /// 按厂商 / 机型模糊查找相机（返回克隆，便于跨线程持有）。
    pub fn find_cameras(&self, maker: Option<&str>, model: &str) -> Vec<Camera> {
        self.inner
            .find_cameras(maker, model)
            .into_iter()
            .cloned()
            .collect()
    }

    /// 按机型模糊查找镜头（`camera` 用于匹配卡口）。
    pub fn find_lenses(&self, camera: Option<&Camera>, model: &str) -> Vec<Lens> {
        self.inner
            .find_lenses(camera, model)
            .into_iter()
            .cloned()
            .collect()
    }
}

/// 一次配置文件校正请求。
#[derive(Debug, Clone, PartialEq)]
pub struct LensCorrection {
    /// 相机机身（提供画幅系数与卡口）。
    pub camera: Camera,
    /// 镜头（携带标定数据）。
    pub lens: Lens,
    /// 拍摄焦距（mm）。
    pub focal: f32,
    /// 光圈（f 值）。
    pub aperture: f32,
    /// 被摄距离（米）。
    pub distance: f32,
    /// 畸变校正。
    pub distortion: bool,
    /// 横向色差校正。
    pub tca: bool,
    /// 暗角校正。
    pub vignetting: bool,
}

impl LensCorrection {
    /// 是否有任何一项校正启用。
    pub fn is_active(&self) -> bool {
        self.distortion || self.tca || self.vignetting
    }

    /// 对 RGBA8 图像应用配置文件校正（边长不变，边缘钳制采样）。
    pub fn apply(&self, rgba: &image::RgbaImage) -> image::RgbaImage {
        if !self.is_active() || rgba.width() == 0 || rgba.height() == 0 {
            return rgba.clone();
        }
        let width = rgba.width() as usize;
        let height = rgba.height() as usize;
        let mut modifier = Modifier::new(
            &self.lens,
            self.focal.max(1.0),
            self.camera.crop_factor.max(0.1),
            rgba.width(),
            rgba.height(),
            true,
        );
        let mut active = false;
        if self.distortion {
            active |= modifier.enable_distortion_correction(&self.lens);
        }
        if self.tca {
            active |= modifier.enable_tca_correction(&self.lens);
        }
        if self.vignetting {
            active |= modifier.enable_vignetting_correction(
                &self.lens,
                self.aperture.max(1.0),
                self.distance.max(0.1),
            );
        }
        if !active {
            return rgba.clone();
        }

        let source = rgba.as_raw();
        let mut output = rgba.clone();
        let mut geometry = vec![0.0f32; width * 2];
        let mut subpixel = vec![0.0f32; width * 6];
        let mut row = vec![0u8; width * 4];
        for y in 0..height {
            let has_geometry =
                modifier.apply_geometry_distortion(0.0, y as f32, width, 1, &mut geometry);
            let has_tca =
                modifier.apply_subpixel_distortion(0.0, y as f32, width, 1, &mut subpixel);
            for x in 0..width {
                let (sx, sy) = if has_geometry {
                    (geometry[x * 2], geometry[x * 2 + 1])
                } else {
                    (x as f32, y as f32)
                };
                let (red, green, blue) = if has_tca {
                    let base = x * 6;
                    (
                        sample(source, width, height, subpixel[base], subpixel[base + 1], 0),
                        sample(
                            source,
                            width,
                            height,
                            subpixel[base + 2],
                            subpixel[base + 3],
                            1,
                        ),
                        sample(
                            source,
                            width,
                            height,
                            subpixel[base + 4],
                            subpixel[base + 5],
                            2,
                        ),
                    )
                } else {
                    (
                        sample(source, width, height, sx, sy, 0),
                        sample(source, width, height, sx, sy, 1),
                        sample(source, width, height, sx, sy, 2),
                    )
                };
                let alpha = sample(source, width, height, sx, sy, 3);
                row[x * 4] = red;
                row[x * 4 + 1] = green;
                row[x * 4 + 2] = blue;
                row[x * 4 + 3] = alpha;
            }
            if self.vignetting {
                modifier.apply_color_modification_u8(&mut row, 0.0, y as f32, width, 1, 4);
            }
            let start = y * width * 4;
            output.as_mut()[start..start + width * 4].copy_from_slice(&row);
        }
        output
    }

    /// 对 RGBA32F（线性光）图像应用配置文件校正。
    ///
    /// 与 [`LensCorrection::apply`] 同一畸变 / 色差 / 暗角模型，但在 f32 线性域
    /// 重采样与增益：分块导出与 1:1 视图使用，避免 8bit 往返量化
    /// （此前这两条路径要么丢掉镜头校正、要么退回 8bit 源）。
    pub fn apply_f32(&self, rgba: &image::Rgba32FImage) -> image::Rgba32FImage {
        if !self.is_active() || rgba.width() == 0 || rgba.height() == 0 {
            return rgba.clone();
        }
        let width = rgba.width() as usize;
        let height = rgba.height() as usize;
        let mut modifier = Modifier::new(
            &self.lens,
            self.focal.max(1.0),
            self.camera.crop_factor.max(0.1),
            rgba.width(),
            rgba.height(),
            true,
        );
        let mut active = false;
        if self.distortion {
            active |= modifier.enable_distortion_correction(&self.lens);
        }
        if self.tca {
            active |= modifier.enable_tca_correction(&self.lens);
        }
        if self.vignetting {
            active |= modifier.enable_vignetting_correction(
                &self.lens,
                self.aperture.max(1.0),
                self.distance.max(0.1),
            );
        }
        if !active {
            return rgba.clone();
        }

        let source = rgba.as_raw();
        let mut output = rgba.clone();
        let mut geometry = vec![0.0f32; width * 2];
        let mut subpixel = vec![0.0f32; width * 6];
        let mut row = vec![0.0f32; width * 4];
        for y in 0..height {
            let has_geometry =
                modifier.apply_geometry_distortion(0.0, y as f32, width, 1, &mut geometry);
            let has_tca =
                modifier.apply_subpixel_distortion(0.0, y as f32, width, 1, &mut subpixel);
            for x in 0..width {
                let (sx, sy) = if has_geometry {
                    (geometry[x * 2], geometry[x * 2 + 1])
                } else {
                    (x as f32, y as f32)
                };
                let (red, green, blue) = if has_tca {
                    let base = x * 6;
                    (
                        sample_f32(source, width, height, subpixel[base], subpixel[base + 1], 0),
                        sample_f32(
                            source,
                            width,
                            height,
                            subpixel[base + 2],
                            subpixel[base + 3],
                            1,
                        ),
                        sample_f32(
                            source,
                            width,
                            height,
                            subpixel[base + 4],
                            subpixel[base + 5],
                            2,
                        ),
                    )
                } else {
                    (
                        sample_f32(source, width, height, sx, sy, 0),
                        sample_f32(source, width, height, sx, sy, 1),
                        sample_f32(source, width, height, sx, sy, 2),
                    )
                };
                let alpha = sample_f32(source, width, height, sx, sy, 3);
                row[x * 4] = red;
                row[x * 4 + 1] = green;
                row[x * 4 + 2] = blue;
                row[x * 4 + 3] = alpha;
            }
            if self.vignetting {
                modifier.apply_color_modification_f32(&mut row, 0.0, y as f32, width, 1, 4);
            }
            let start = y * width * 4;
            output.as_mut()[start..start + width * 4].copy_from_slice(&row);
        }
        output
    }
}

/// 双线性采样（边缘钳制，坐标为像素中心系）。
fn sample(source: &[u8], width: usize, height: usize, x: f32, y: f32, channel: usize) -> u8 {
    let max_x = width.saturating_sub(1) as f32;
    let max_y = height.saturating_sub(1) as f32;
    let x = x.clamp(0.0, max_x);
    let y = y.clamp(0.0, max_y);
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let value = |px: usize, py: usize| source[(py * width + px) * 4 + channel] as f32;
    let top = value(x0, y0) * (1.0 - tx) + value(x1, y0) * tx;
    let bottom = value(x0, y1) * (1.0 - tx) + value(x1, y1) * tx;
    (top * (1.0 - ty) + bottom * ty + 0.5).clamp(0.0, 255.0) as u8
}

/// 双线性采样（f32 线性数据，边缘钳制）。
fn sample_f32(source: &[f32], width: usize, height: usize, x: f32, y: f32, channel: usize) -> f32 {
    let max_x = width.saturating_sub(1) as f32;
    let max_y = height.saturating_sub(1) as f32;
    let x = x.clamp(0.0, max_x);
    let y = y.clamp(0.0, max_y);
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let value = |px: usize, py: usize| source[(py * width + px) * 4 + channel];
    let top = value(x0, y0) * (1.0 - tx) + value(x1, y0) * tx;
    let bottom = value(x0, y1) * (1.0 - tx) + value(x1, y1) * tx;
    top * (1.0 - ty) + bottom * ty
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_image(width: u32, height: u32) -> image::RgbaImage {
        let mut image = image::RgbaImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let checker = if (x / 8 + y / 8) % 2 == 0 { 220 } else { 40 };
            *pixel = image::Rgba([checker, checker, checker, 255]);
        }
        image
    }

    #[test]
    fn bundled_database_loads_and_finds_lenses() {
        let db = LensDatabase::load().unwrap();
        assert!(db.camera_count() > 200, "相机数量 {}", db.camera_count());
        assert!(db.lens_count() > 300, "镜头数量 {}", db.lens_count());
        let cameras = db.find_cameras(Some("Canon"), "EOS 5D");
        assert!(!cameras.is_empty(), "应能匹配 Canon EOS 5D 系列");
    }

    #[test]
    fn profile_correction_changes_geometry_but_keeps_size() {
        let db = LensDatabase::load().unwrap();
        let cameras = db.find_cameras(Some("Canon"), "EOS 5D Mark II");
        let camera = cameras.first().cloned().expect("机身");
        // 找一个带畸变标定的镜头。
        let lenses = db.find_lenses(Some(&camera), "");
        let lens = lenses
            .iter()
            .find(|lens| !lens.calib_distortion.is_empty())
            .cloned()
            .expect("带畸变标定的镜头");
        let correction = LensCorrection {
            camera,
            lens,
            focal: 24.0,
            aperture: 8.0,
            distance: 5.0,
            distortion: true,
            tca: false,
            vignetting: false,
        };
        let source = grid_image(160, 120);
        let corrected = correction.apply(&source);
        assert_eq!(corrected.dimensions(), source.dimensions());
        let diff: i32 = source
            .pixels()
            .zip(corrected.pixels())
            .map(|(a, b)| (a.0[0] as i32 - b.0[0] as i32).abs())
            .max()
            .unwrap_or(0);
        assert!(diff > 10, "畸变校正应改变画面（最大差 {diff}）");
        // 边缘不应出现透明（钳制采样）。
        assert!(corrected.get_pixel(0, 0).0[3] > 0);
    }

    #[test]
    fn f32_profile_correction_matches_8bit() {
        let db = LensDatabase::load().unwrap();
        let camera = db
            .find_cameras(Some("Canon"), "EOS 5D Mark II")
            .first()
            .cloned()
            .expect("机身");
        let lens = db
            .find_lenses(Some(&camera), "")
            .iter()
            .find(|lens| !lens.calib_distortion.is_empty())
            .cloned()
            .expect("带畸变标定的镜头");
        let correction = LensCorrection {
            camera,
            lens,
            focal: 24.0,
            aperture: 8.0,
            distance: 5.0,
            distortion: true,
            tca: true,
            vignetting: true,
        };
        let source = grid_image(96, 72);
        let eight_bit = correction.apply(&source);
        let mut linear = image::Rgba32FImage::new(96, 72);
        for (target, pixel) in linear.pixels_mut().zip(source.pixels()) {
            target.0 = [
                pixel.0[0] as f32 / 255.0,
                pixel.0[1] as f32 / 255.0,
                pixel.0[2] as f32 / 255.0,
                1.0,
            ];
        }
        let corrected = correction.apply_f32(&linear);
        assert_eq!(corrected.dimensions(), (96, 72));
        let mut max_diff = 0i32;
        for (expected, actual) in eight_bit.pixels().zip(corrected.pixels()) {
            for channel in 0..3 {
                let encoded = (actual.0[channel].clamp(0.0, 1.0) * 255.0 + 0.5) as i32;
                max_diff = max_diff.max((expected.0[channel] as i32 - encoded).abs());
            }
        }
        assert!(
            max_diff <= 4,
            "f32 线性通路应与 8bit 通路一致（最大差 {max_diff}）"
        );
    }

    #[test]
    fn profile_vignetting_brightens_corners() {
        let db = LensDatabase::load().unwrap();
        let cameras = db.find_cameras(Some("Canon"), "EOS 5D Mark II");
        let camera = cameras.first().cloned().expect("机身");
        let lenses = db.find_lenses(Some(&camera), "");
        // 找一个在当前焦距 / 光圈下确实能启用暗角校正的镜头。
        let mut chosen = None;
        for lens in &lenses {
            if lens.calib_vignetting.is_empty() {
                continue;
            }
            let focal = lens.focal_min.max(1.0);
            let aperture = if lens.aperture_min.is_finite() && lens.aperture_min > 0.0 {
                lens.aperture_min.max(1.0)
            } else {
                4.0
            };
            let mut probe = Modifier::new(lens, focal, camera.crop_factor.max(0.1), 96, 72, true);
            if probe.enable_vignetting_correction(lens, aperture, 3.0) {
                chosen = Some((lens.clone(), focal, aperture));
                break;
            }
        }
        let Some((lens, focal, aperture)) = chosen else {
            return; // 数据库无可用暗角标定时跳过
        };
        let correction = LensCorrection {
            camera,
            lens,
            focal,
            aperture,
            distance: 3.0,
            distortion: false,
            tca: false,
            vignetting: true,
        };
        let mut source = image::RgbaImage::new(96, 72);
        for pixel in source.pixels_mut() {
            *pixel = image::Rgba([120, 120, 120, 255]);
        }
        let corrected = correction.apply(&source);
        let corner = corrected.get_pixel(1, 1).0[0] as i32;
        let center = corrected.get_pixel(48, 36).0[0] as i32;
        assert!(
            corner > center + 5,
            "暗角校正应提亮四角：角 {corner} 中心 {center}"
        );
    }
}
