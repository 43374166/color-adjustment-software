//! 几何变换：裁剪 / 旋转 / 翻转 / 透视（CPU 参考实现 + 逆映射矩阵）。
//!
//! 约定：
//! - 像素坐标系：像素中心位于 `(x + 0.5, y + 0.5)`，y 轴向下；
//! - “视图空间”与源图同尺寸（旋转 / 透视后的画面），裁剪矩形定义在视图空间；
//! - GPU / CPU 都使用**逆映射**（输出像素 → 源图像素），避免空洞。
//!
//! 组合顺序（源图 → 视图，正向）：
//! `翻转 ∘ 透视 ∘ 旋转`（各自围绕图像中心），裁剪再取视图空间的轴对齐矩形。

use serde::{Deserialize, Serialize};

use crate::params::{AspectRatio, CropParams, EditParams};

/// 3×3 行主序矩阵。
pub type Mat3 = [[f32; 3]; 3];

/// 单位矩阵。
pub const IDENTITY: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// 矩阵乘法 `a · b`。
pub fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut out = [[0.0f32; 3]; 3];
    for (row, out_row) in out.iter_mut().enumerate() {
        for (col, value) in out_row.iter_mut().enumerate() {
            *value = a[row][0] * b[0][col] + a[row][1] * b[1][col] + a[row][2] * b[2][col];
        }
    }
    out
}

/// 平移矩阵。
pub fn translate(tx: f32, ty: f32) -> Mat3 {
    [[1.0, 0.0, tx], [0.0, 1.0, ty], [0.0, 0.0, 1.0]]
}

/// 缩放矩阵。
pub fn scale(sx: f32, sy: f32) -> Mat3 {
    [[sx, 0.0, 0.0], [0.0, sy, 0.0], [0.0, 0.0, 1.0]]
}

/// 旋转矩阵（y 轴向下，正角度 = 屏幕上顺时针）。
pub fn rotate(radians: f32) -> Mat3 {
    let (sin, cos) = radians.sin_cos();
    [[cos, -sin, 0.0], [sin, cos, 0.0], [0.0, 0.0, 1.0]]
}

/// 透视（梯形校正）矩阵：`u, v` 为归一化到半宽/半高的坐标。
///
/// `w = 1 + vertical · v + horizontal · u`，`u' = u / w`、`v' = v / w`。
pub fn perspective(vertical: f32, horizontal: f32) -> Mat3 {
    [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [horizontal, vertical, 1.0],
    ]
}

/// 应用矩阵（含透视除法）。
pub fn apply(m: &Mat3, point: [f32; 2]) -> [f32; 2] {
    let x = m[0][0] * point[0] + m[0][1] * point[1] + m[0][2];
    let y = m[1][0] * point[0] + m[1][1] * point[1] + m[1][2];
    let w = m[2][0] * point[0] + m[2][1] * point[1] + m[2][2];
    if w.abs() < 1e-6 {
        return [x, y];
    }
    [x / w, y / w]
}

/// 3×3 逆矩阵（奇异时返回 `None`）。
pub fn inverse(m: &Mat3) -> Option<Mat3> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv_det = 1.0 / det;
    Some([
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
        ],
    ])
}

/// 几何渲染模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeometryMode {
    /// 全幅：只应用旋转 / 翻转 / 透视，不做裁剪（裁剪编辑视图使用）。
    Full,
    /// 裁剪：在全幅基础上裁剪到 `crop.rect`（正常显示与导出使用）。
    Crop,
}

/// 镜头校正模型（畸变 / 色差 / 暗角），CPU 与 GPU 使用同一套系数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LensModel {
    /// 畸变，-100..=100。
    pub distortion: f32,
    /// 色差，-100..=100。
    pub chromatic_aberration: f32,
    /// 暗角校正，-100..=100。
    pub vignette: f32,
}

impl LensModel {
    /// 畸变系数（与着色器一致）。
    pub fn distortion_k(&self) -> f32 {
        self.distortion / 100.0 * 0.3
    }

    /// 色差系数（与着色器一致）。
    pub fn chromatic_scale(&self) -> f32 {
        self.chromatic_aberration / 100.0 * 0.01
    }

    /// 暗角校正强度（与着色器一致）。
    pub fn vignette_strength(&self) -> f32 {
        self.vignette / 100.0 * 0.5
    }

    /// 是否无任何校正。
    pub fn is_identity(&self) -> bool {
        self.distortion.abs() < 1e-4
            && self.chromatic_aberration.abs() < 1e-4
            && self.vignette.abs() < 1e-4
    }
}

/// 从编辑参数中提炼出的几何参数（便于矩阵构建与测试）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Geometry {
    /// 合计旋转角度（度，顺时针为正）。
    pub rotation_deg: f32,
    /// 水平翻转。
    pub flip_horizontal: bool,
    /// 垂直翻转。
    pub flip_vertical: bool,
    /// 垂直透视，-100..=100。
    pub perspective_vertical: f32,
    /// 水平透视，-100..=100。
    pub perspective_horizontal: f32,
    /// 镜头畸变校正，-100..=100（正值收缩边缘，负值扩张）。
    pub lens_distortion: f32,
    /// 色差校正，-100..=100（按通道径向缩放）。
    pub lens_chromatic_aberration: f32,
    /// 镜头暗角校正，-100..=100（正值提亮四角）。
    pub lens_vignette: f32,
    /// 镜头校正缩放，0 = 100%（>100 放大以裁掉畸变边缘）。
    pub lens_scale: f32,
}

impl Geometry {
    /// 从完整编辑参数提取（裁剪旋转 + 变换旋转叠加，翻转按异或合并）。
    pub fn from_params(params: &EditParams) -> Self {
        Self {
            rotation_deg: params.crop.rotation_deg + params.transform.rotate,
            flip_horizontal: params.crop.flip_horizontal ^ params.transform.flip_horizontal,
            flip_vertical: params.crop.flip_vertical ^ params.transform.flip_vertical,
            perspective_vertical: params.transform.perspective_vertical,
            perspective_horizontal: params.transform.perspective_horizontal,
            lens_distortion: params.lens.geometry.distortion,
            lens_chromatic_aberration: params.lens.chromatic_aberration,
            lens_vignette: params.lens.vignette,
            lens_scale: params.lens.geometry.scale,
        }
    }

    /// 是否为恒等（无任何几何变换，可走快速路径）。
    pub fn is_identity(&self) -> bool {
        self.rotation_deg.abs() < 1e-4
            && !self.flip_horizontal
            && !self.flip_vertical
            && self.perspective_vertical.abs() < 1e-4
            && self.perspective_horizontal.abs() < 1e-4
            && self.lens_distortion.abs() < 1e-4
            && self.lens_chromatic_aberration.abs() < 1e-4
            && self.lens_vignette.abs() < 1e-4
            && (self.lens_scale.abs() < 1e-4 || (self.lens_scale - 100.0).abs() < 1e-4)
    }

    /// 镜头校正参数（全部为默认值时返回 `None`）。
    pub fn lens_model(&self) -> Option<LensModel> {
        if self.lens_distortion.abs() < 1e-4
            && self.lens_chromatic_aberration.abs() < 1e-4
            && self.lens_vignette.abs() < 1e-4
        {
            return None;
        }
        Some(LensModel {
            distortion: self.lens_distortion,
            chromatic_aberration: self.lens_chromatic_aberration,
            vignette: self.lens_vignette,
        })
    }

    /// 源图 → 视图（正向矩阵，像素坐标）。
    pub fn forward_matrix(&self, width: f32, height: f32) -> Mat3 {
        let cx = width * 0.5;
        let cy = height * 0.5;
        let center_out = translate(-cx, -cy);
        let center_back = translate(cx, cy);

        let rotation = rotate(self.rotation_deg.to_radians());
        let perspective =
            if self.perspective_vertical.abs() < 1e-4 && self.perspective_horizontal.abs() < 1e-4 {
                IDENTITY
            } else {
                let to_norm = scale(1.0 / cx.max(1e-3), 1.0 / cy.max(1e-3));
                let from_norm = scale(cx, cy);
                let keystone = perspective(
                    self.perspective_vertical / 100.0 * 0.6,
                    self.perspective_horizontal / 100.0 * 0.6,
                );
                mul(&mul(&from_norm, &keystone), &to_norm)
            };
        let flips = if self.flip_horizontal || self.flip_vertical {
            scale(
                if self.flip_horizontal { -1.0 } else { 1.0 },
                if self.flip_vertical { -1.0 } else { 1.0 },
            )
        } else {
            IDENTITY
        };
        // 镜头校正缩放（>100% 放大画面以裁掉畸变边缘）。
        let zoom = if self.lens_scale <= 1e-4 {
            1.0
        } else {
            (self.lens_scale / 100.0).max(0.05)
        };
        let lens_zoom = if (zoom - 1.0).abs() < 1e-6 {
            IDENTITY
        } else {
            scale(zoom, zoom)
        };

        let local = mul(&mul(&mul(&lens_zoom, &flips), &perspective), &rotation);
        mul(&mul(&center_back, &local), &center_out)
    }

    /// 视图 → 源图（逆矩阵，像素坐标）。
    pub fn view_to_source_matrix(&self, width: f32, height: f32) -> Mat3 {
        inverse(&self.forward_matrix(width, height)).unwrap_or(IDENTITY)
    }
}

/// 输出像素 → 源图像素 的完整矩阵（含裁剪映射）。
pub fn output_to_source_matrix(
    geometry: &Geometry,
    crop_rect: [f32; 4],
    source_width: u32,
    source_height: u32,
    output_width: u32,
    output_height: u32,
) -> Mat3 {
    let width = source_width.max(1) as f32;
    let height = source_height.max(1) as f32;
    let view_to_source = geometry.view_to_source_matrix(width, height);
    let rect = crop_rect;
    let scale_x = rect[2] * width / output_width.max(1) as f32;
    let scale_y = rect[3] * height / output_height.max(1) as f32;
    let offset_x = rect[0] * width;
    let offset_y = rect[1] * height;
    let output_to_view = mul(&translate(offset_x, offset_y), &scale(scale_x, scale_y));
    mul(&view_to_source, &output_to_view)
}

/// 分块矩阵：把“整图输出 → 整图源”的矩阵转换为
/// “分块输出（以 `output_origin` 为原点）→ 分块源（以 `source_origin` 为原点）”。
pub fn tile_matrix(full: &Mat3, output_origin: [f32; 2], source_origin: [f32; 2]) -> Mat3 {
    let to_full_output = translate(output_origin[0], output_origin[1]);
    let from_full_source = translate(-source_origin[0], -source_origin[1]);
    mul(&mul(&from_full_source, full), &to_full_output)
}

/// 一个渲染分块：输出矩形 + 该分块需要的源图裁剪矩形（含细节模糊边距）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TilePlan {
    /// 输出分块起点 / 尺寸（整图输出坐标）。
    pub x: u32,
    /// 见 [`TilePlan::x`]。
    pub y: u32,
    /// 分块宽度。
    pub width: u32,
    /// 分块高度。
    pub height: u32,
    /// 源图裁剪矩形 `[x, y, w, h]`（已含 `margin`，并夹取在源图范围内）。
    pub source: [u32; 4],
}

/// 把整图输出切分成 `tile_edge` 大小的分块，并按逆矩阵反算每块需要的源图矩形。
///
/// 源图矩形 = 分块输出矩形四角逆映射后的包围盒，四周外扩 `margin` 像素
/// （覆盖细节模糊抽头，避免分块边缘出现接缝），最后夹取到源图范围内。
pub fn plan_tiles(
    output_to_source: &Mat3,
    output_width: u32,
    output_height: u32,
    source_width: u32,
    source_height: u32,
    tile_edge: u32,
    margin: f32,
) -> Vec<TilePlan> {
    let tile_edge = tile_edge.max(1);
    let mut plans = Vec::new();
    let mut y = 0u32;
    while y < output_height {
        let height = tile_edge.min(output_height - y);
        let mut x = 0u32;
        while x < output_width {
            let width = tile_edge.min(output_width - x);
            let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
            let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
            for corner in [
                [x as f32, y as f32],
                [(x + width) as f32, y as f32],
                [x as f32, (y + height) as f32],
                [(x + width) as f32, (y + height) as f32],
            ] {
                let mapped = apply(output_to_source, corner);
                min_x = min_x.min(mapped[0]);
                min_y = min_y.min(mapped[1]);
                max_x = max_x.max(mapped[0]);
                max_y = max_y.max(mapped[1]);
            }
            let sx0 = (min_x - margin).max(0.0) as u32;
            let sy0 = (min_y - margin).max(0.0) as u32;
            let sx1 = ((max_x + margin).min(source_width as f32)).max(sx0 as f32 + 1.0) as u32;
            let sy1 = ((max_y + margin).min(source_height as f32)).max(sy0 as f32 + 1.0) as u32;
            plans.push(TilePlan {
                x,
                y,
                width,
                height,
                source: [sx0, sy0, sx1 - sx0, sy1 - sy0],
            });
            x += width;
        }
        y += height;
    }
    plans
}

/// 裁剪输出尺寸（源图像素尺度，至少 1×1）。
pub fn crop_output_size(crop: &CropParams, source_width: u32, source_height: u32) -> (u32, u32) {
    let width = (crop.rect[2] * source_width.max(1) as f32).round().max(1.0) as u32;
    let height = (crop.rect[3] * source_height.max(1) as f32)
        .round()
        .max(1.0) as u32;
    (width, height)
}

/// 目标输出尺寸：全幅模式 = 源图尺寸；裁剪模式 = 裁剪矩形尺寸。
pub fn output_size(
    params: &EditParams,
    source_width: u32,
    source_height: u32,
    mode: GeometryMode,
) -> (u32, u32) {
    match mode {
        GeometryMode::Full => (source_width.max(1), source_height.max(1)),
        GeometryMode::Crop => crop_output_size(&params.crop, source_width, source_height),
    }
}

/// 比例值（宽高比）；`Free` 返回 `None`。
pub fn aspect_value(aspect: AspectRatio, source_width: u32, source_height: u32) -> Option<f32> {
    let width = source_width.max(1) as f32;
    let height = source_height.max(1) as f32;
    match aspect {
        AspectRatio::Free => None,
        AspectRatio::Square => Some(1.0),
        AspectRatio::FourThree => Some(4.0 / 3.0),
        AspectRatio::ThreeTwo => Some(3.0 / 2.0),
        AspectRatio::SixteenNine => Some(16.0 / 9.0),
        AspectRatio::Original => Some(width / height),
    }
}

/// 把裁剪矩形调整为目标比例（保持中心与宽度）。
pub fn rect_with_aspect(rect: [f32; 4], aspect: Option<f32>, image_aspect: f32) -> [f32; 4] {
    let Some(aspect) = aspect else {
        return rect;
    };
    if aspect <= 0.0 || image_aspect <= 0.0 {
        return rect;
    }
    // 显示宽高比 = (w·W)/(h·H) = (w/h)·image_aspect
    let height = rect[2] * image_aspect / aspect;
    let center_y = rect[1] + rect[3] * 0.5;
    [rect[0], center_y - height * 0.5, rect[2], height]
}

/// 约束裁剪矩形：缩放并平移，使其四角在旋转 / 透视后仍落在源图内。
///
/// 采用二分缩放 + 中心夹取，兼容任意组合的几何变换。
pub fn constrain_rect(
    geometry: &Geometry,
    rect: [f32; 4],
    source_width: u32,
    source_height: u32,
) -> [f32; 4] {
    let width = source_width.max(1) as f32;
    let height = source_height.max(1) as f32;
    let view_to_source = geometry.view_to_source_matrix(width, height);

    let fits = |candidate: [f32; 4]| -> bool {
        let corners = [
            [candidate[0] * width, candidate[1] * height],
            [(candidate[0] + candidate[2]) * width, candidate[1] * height],
            [candidate[0] * width, (candidate[1] + candidate[3]) * height],
            [
                (candidate[0] + candidate[2]) * width,
                (candidate[1] + candidate[3]) * height,
            ],
        ];
        corners.iter().all(|corner| {
            let mapped = apply(&view_to_source, *corner);
            mapped[0] >= -0.5
                && mapped[1] >= -0.5
                && mapped[0] <= width + 0.5
                && mapped[1] <= height + 0.5
        })
    };

    // 已满足则只做中心夹取。
    if fits(rect) {
        return clamp_center(rect);
    }

    let center = [rect[0] + rect[2] * 0.5, rect[1] + rect[3] * 0.5];
    let mut low = 0.0f32;
    let mut high = 1.0f32;
    let candidate_at = |scale_factor: f32| -> [f32; 4] {
        let w = rect[2] * scale_factor;
        let h = rect[3] * scale_factor;
        [center[0] - w * 0.5, center[1] - h * 0.5, w, h]
    };
    for _ in 0..32 {
        let mid = (low + high) * 0.5;
        if fits(candidate_at(mid)) {
            low = mid;
        } else {
            high = mid;
        }
    }
    clamp_center(candidate_at(low.max(1e-3)))
}

/// 平移裁剪矩形到画面内（保持尺寸）。
pub fn clamp_center(rect: [f32; 4]) -> [f32; 4] {
    let width = rect[2].clamp(0.01, 1.0);
    let height = rect[3].clamp(0.01, 1.0);
    let x = rect[0].clamp(0.0, (1.0 - width).max(0.0));
    let y = rect[1].clamp(0.0, (1.0 - height).max(0.0));
    [x, y, width, height]
}

/// sRGB u8 → 线性 f32（与 GPU 的 `textureLoad` 解码一致）。
fn srgb_u8_to_linear(value: u8) -> f32 {
    let x = value as f32 / 255.0;
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// 线性 f32 → sRGB u8（与 GPU 的存储编码一致）。
fn linear_to_srgb_u8(value: f32) -> u8 {
    let x = value.clamp(0.0, 1.0);
    let encoded = if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

/// 双线性采样（线性光空间插值，边界外返回透明黑）。
///
/// 与 GPU 几何趟语义一致：源纹理按 sRGB 解码为线性值后插值。
fn sample_bilinear(source: &image::RgbaImage, x: f32, y: f32) -> [f32; 4] {
    let width = source.width() as i64;
    let height = source.height() as i64;
    let fx = x - 0.5;
    let fy = y - 0.5;
    let x0 = fx.floor() as i64;
    let y0 = fy.floor() as i64;
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;
    let mut result = [0.0f32; 4];
    for (dx, dy, weight) in [
        (0i64, 0i64, (1.0 - tx) * (1.0 - ty)),
        (1, 0, tx * (1.0 - ty)),
        (0, 1, (1.0 - tx) * ty),
        (1, 1, tx * ty),
    ] {
        if weight <= 0.0 {
            continue;
        }
        let sx = x0 + dx;
        let sy = y0 + dy;
        if sx < 0 || sy < 0 || sx >= width || sy >= height {
            continue;
        }
        let pixel = source.get_pixel(sx as u32, sy as u32).0;
        result[0] += srgb_u8_to_linear(pixel[0]) * weight;
        result[1] += srgb_u8_to_linear(pixel[1]) * weight;
        result[2] += srgb_u8_to_linear(pixel[2]) * weight;
        // alpha 在 sRGB 纹理中不做解码。
        result[3] += (pixel[3] as f32 / 255.0) * weight;
    }
    result
}

/// CPU 参考实现：按几何矩阵重采样一张 RGBA8 图（测试 / 无 GPU 回退）。
pub fn warp_rgba(
    source: &image::RgbaImage,
    output_to_source: &Mat3,
    output_width: u32,
    output_height: u32,
) -> image::RgbaImage {
    warp_rgba_lens(source, output_to_source, output_width, output_height, None)
}

/// 单通道双线性采样（线性光；alpha 不做 sRGB 解码）。
fn sample_channel_linear(source: &image::RgbaImage, x: f32, y: f32, channel: usize) -> f32 {
    let width = source.width() as i64;
    let height = source.height() as i64;
    let fx = x - 0.5;
    let fy = y - 0.5;
    let x0 = fx.floor() as i64;
    let y0 = fy.floor() as i64;
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;
    let mut result = 0.0f32;
    for (dx, dy, weight) in [
        (0i64, 0i64, (1.0 - tx) * (1.0 - ty)),
        (1, 0, tx * (1.0 - ty)),
        (0, 1, (1.0 - tx) * ty),
        (1, 1, tx * ty),
    ] {
        if weight <= 0.0 {
            continue;
        }
        let sx = x0 + dx;
        let sy = y0 + dy;
        if sx < 0 || sy < 0 || sx >= width || sy >= height {
            continue;
        }
        let value = source.get_pixel(sx as u32, sy as u32).0[channel];
        result += if channel == 3 {
            value as f32 / 255.0
        } else {
            srgb_u8_to_linear(value)
        } * weight;
    }
    result
}

/// smoothstep（与着色器一致）。
fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// CPU 参考实现：几何矩阵 + 可选镜头校正模型。
pub fn warp_rgba_lens(
    source: &image::RgbaImage,
    output_to_source: &Mat3,
    output_width: u32,
    output_height: u32,
    lens: Option<LensModel>,
) -> image::RgbaImage {
    let mut output = image::RgbaImage::new(output_width.max(1), output_height.max(1));
    let center = [source.width() as f32 * 0.5, source.height() as f32 * 0.5];
    let half = [
        (source.width() as f32 * 0.5).max(1.0),
        (source.height() as f32 * 0.5).max(1.0),
    ];
    for y in 0..output.height() {
        for x in 0..output.width() {
            let mapped = apply(output_to_source, [x as f32 + 0.5, y as f32 + 0.5]);
            let sampled = match lens.filter(|model| !model.is_identity()) {
                Some(model) => {
                    let nx = (mapped[0] - center[0]) / half[0];
                    let ny = (mapped[1] - center[1]) / half[1];
                    let radius_sq = nx * nx + ny * ny;
                    let radial = 1.0 + model.distortion_k() * radius_sq;
                    let ca = model.chromatic_scale();
                    let coordinate = |factor: f32| {
                        [
                            center[0] + nx * radial * factor * half[0],
                            center[1] + ny * radial * factor * half[1],
                        ]
                    };
                    let red = coordinate(1.0 + ca);
                    let base = coordinate(1.0);
                    let blue = coordinate(1.0 - ca);
                    let falloff = smoothstep(0.55, 1.0, radius_sq.sqrt());
                    let gain = 1.0 + model.vignette_strength() * falloff;
                    [
                        sample_channel_linear(source, red[0], red[1], 0) * gain,
                        sample_channel_linear(source, base[0], base[1], 1) * gain,
                        sample_channel_linear(source, blue[0], blue[1], 2) * gain,
                        sample_channel_linear(source, base[0], base[1], 3),
                    ]
                }
                None => sample_bilinear(source, mapped[0], mapped[1]),
            };
            let pixel = [
                linear_to_srgb_u8(sampled[0]),
                linear_to_srgb_u8(sampled[1]),
                linear_to_srgb_u8(sampled[2]),
                (sampled[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
            ];
            output.put_pixel(x, y, image::Rgba(pixel));
        }
    }
    output
}

/// CPU 参考实现：按编辑参数渲染几何结果（与 GPU `cs_geometry` 语义一致）。
pub fn render_geometry(
    source: &image::RgbaImage,
    params: &EditParams,
    mode: GeometryMode,
) -> Result<image::RgbaImage, String> {
    let geometry = Geometry::from_params(params);
    let (width, height) = (source.width().max(1), source.height().max(1));
    let (out_width, out_height) = output_size(params, width, height, mode);
    if mode == GeometryMode::Crop
        && out_width == width
        && out_height == height
        && geometry.is_identity()
    {
        return Ok(source.clone());
    }
    let crop_rect = if mode == GeometryMode::Full {
        [0.0, 0.0, 1.0, 1.0]
    } else {
        params.crop.rect
    };
    let matrix =
        output_to_source_matrix(&geometry, crop_rect, width, height, out_width, out_height);
    Ok(warp_rgba_lens(
        source,
        &matrix,
        out_width,
        out_height,
        geometry.lens_model(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::EditParams;

    fn gradient(width: u32, height: u32) -> image::RgbaImage {
        let mut image = image::RgbaImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let r = (x * 255 / width.max(1)) as u8;
                let g = (y * 255 / height.max(1)) as u8;
                image.put_pixel(x, y, image::Rgba([r, g, 128, 255]));
            }
        }
        image
    }

    #[test]
    fn default_geometry_is_identity() {
        let geometry = Geometry::from_params(&EditParams::default());
        assert!(geometry.is_identity());
        let matrix = geometry.forward_matrix(100.0, 50.0);
        assert_eq!(apply(&matrix, [10.0, 20.0]), [10.0, 20.0]);
        let inverse = geometry.view_to_source_matrix(100.0, 50.0);
        assert_eq!(apply(&inverse, [10.0, 20.0]), [10.0, 20.0]);
    }

    #[test]
    fn tile_plan_covers_output_and_margins_sources() {
        let geometry = Geometry::from_params(&EditParams::default());
        let matrix =
            output_to_source_matrix(&geometry, [0.0, 0.0, 1.0, 1.0], 6000, 4000, 5000, 3000);
        let tiles = plan_tiles(&matrix, 5000, 3000, 6000, 4000, 2048, 12.0);
        assert_eq!(
            tiles.len(),
            6,
            "5000×3000 按 2048 分块应为 3×2（2048/2048/904 × 2048/952）"
        );

        let mut covered = 0u64;
        for tile in &tiles {
            covered += tile.width as u64 * tile.height as u64;
            let [sx, sy, sw, sh] = tile.source;
            assert!(sw >= 1 && sh >= 1);
            assert!(sx + sw <= 6000 && sy + sh <= 4000, "源矩形越界：{tile:?}");
            // 分块映射后的源包围盒必须被裁进来的源矩形覆盖，
            // 且严格位于源图内部时四周至少外扩 margin - 1（允许取整误差）。
            let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
            let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
            for corner in [
                [tile.x as f32, tile.y as f32],
                [(tile.x + tile.width) as f32, tile.y as f32],
                [tile.x as f32, (tile.y + tile.height) as f32],
                [(tile.x + tile.width) as f32, (tile.y + tile.height) as f32],
            ] {
                let mapped = apply(&matrix, corner);
                min_x = min_x.min(mapped[0]);
                min_y = min_y.min(mapped[1]);
                max_x = max_x.max(mapped[0]);
                max_y = max_y.max(mapped[1]);
            }
            assert!(
                sx as f32 <= min_x && sy as f32 <= min_y,
                "源矩形未覆盖分块：{tile:?}"
            );
            assert!(
                (sx + sw) as f32 >= max_x && (sy + sh) as f32 >= max_y,
                "源矩形未覆盖分块：{tile:?}"
            );
            let margin = 11.0;
            if min_x >= margin {
                assert!(sx as f32 <= min_x - margin, "左侧边距不足：{tile:?}");
            }
            if min_y >= margin {
                assert!(sy as f32 <= min_y - margin, "上侧边距不足：{tile:?}");
            }
            if max_x + margin <= 6000.0 {
                assert!((sx + sw) as f32 >= max_x + margin, "右侧边距不足：{tile:?}");
            }
            if max_y + margin <= 4000.0 {
                assert!((sy + sh) as f32 >= max_y + margin, "下侧边距不足：{tile:?}");
            }
        }
        assert_eq!(covered, 5000 * 3000, "分块应恰好覆盖整个输出");
    }

    #[test]
    fn rotation_90_degrees_maps_right_to_down() {
        let geometry = Geometry {
            rotation_deg: 90.0,
            ..Geometry::default()
        };
        let matrix = geometry.forward_matrix(100.0, 100.0);
        // 中心右侧 10px 的点应转到中心下方 10px（顺时针）。
        let mapped = apply(&matrix, [60.0, 50.0]);
        assert!((mapped[0] - 50.0).abs() < 1e-3, "x = {}", mapped[0]);
        assert!((mapped[1] - 60.0).abs() < 1e-3, "y = {}", mapped[1]);
    }

    #[test]
    fn rotation_90_pixel_roundtrip_moves_corner() {
        let mut source = image::RgbaImage::new(4, 4);
        source.put_pixel(3, 0, image::Rgba([255, 0, 0, 255]));
        let geometry = Geometry {
            rotation_deg: 90.0,
            ..Geometry::default()
        };
        let matrix = geometry.view_to_source_matrix(4.0, 4.0);
        let warped = warp_rgba(&source, &matrix, 4, 4);
        // 顺时针 90°：右上角 (3,0) → 右下角 (3,3)。
        assert_eq!(warped.get_pixel(3, 3).0[0], 255);
        assert_eq!(warped.get_pixel(0, 0).0[0], 0);
    }

    #[test]
    fn rotation_180_equals_double_flip() {
        let source = gradient(8, 6);
        let rotate180 = Geometry {
            rotation_deg: 180.0,
            ..Geometry::default()
        };
        let flipped = Geometry {
            flip_horizontal: true,
            flip_vertical: true,
            ..Geometry::default()
        };
        let a = render_geometry(&source, &params_with(rotate180), GeometryMode::Full).unwrap();
        let b = render_geometry(&source, &params_with(flipped), GeometryMode::Full).unwrap();
        assert_eq!(a.as_raw(), b.as_raw());
    }

    fn params_with(geometry: Geometry) -> EditParams {
        let mut params = EditParams::default();
        params.crop.rotation_deg = geometry.rotation_deg;
        params.crop.flip_horizontal = geometry.flip_horizontal;
        params.crop.flip_vertical = geometry.flip_vertical;
        params
    }

    #[test]
    fn horizontal_flip_mirrors_pixels() {
        let mut source = image::RgbaImage::new(4, 2);
        source.put_pixel(0, 0, image::Rgba([255, 255, 255, 255]));
        let geometry = Geometry {
            flip_horizontal: true,
            ..Geometry::default()
        };
        let matrix = geometry.view_to_source_matrix(4.0, 2.0);
        let warped = warp_rgba(&source, &matrix, 4, 2);
        assert_eq!(warped.get_pixel(3, 0).0[0], 255);
        assert_eq!(warped.get_pixel(0, 0).0[0], 0);
    }

    #[test]
    fn crop_rect_maps_to_requested_region() {
        let source = gradient(100, 80);
        let mut params = EditParams::default();
        params.crop.rect = [0.25, 0.5, 0.5, 0.25];
        let rendered = render_geometry(&source, &params, GeometryMode::Crop).unwrap();
        assert_eq!((rendered.width(), rendered.height()), (50, 20));
        // 裁剪后的左上角对应源图 (25, 40) 附近。
        let expected = *source.get_pixel(25, 40);
        let actual = *rendered.get_pixel(0, 0);
        assert!((actual.0[0] as i32 - expected.0[0] as i32).abs() <= 2);
        assert!((actual.0[1] as i32 - expected.0[1] as i32).abs() <= 2);
    }

    #[test]
    fn output_size_follows_geometry_mode() {
        let mut params = EditParams::default();
        params.crop.rect = [0.1, 0.2, 0.5, 0.4];
        assert_eq!(
            output_size(&params, 1000, 800, GeometryMode::Full),
            (1000, 800)
        );
        assert_eq!(
            output_size(&params, 1000, 800, GeometryMode::Crop),
            (500, 320)
        );
    }

    #[test]
    fn aspect_value_and_rect_adjustment() {
        assert_eq!(aspect_value(AspectRatio::Square, 100, 50), Some(1.0));
        assert_eq!(aspect_value(AspectRatio::Free, 100, 50), None);
        let rect = rect_with_aspect([0.0, 0.0, 0.5, 0.5], Some(1.0), 2.0);
        // 图像宽高比 2：1，方形裁剪 ⇒ h = w · 2
        assert!((rect[3] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn constrain_keeps_corners_inside_when_rotated() {
        let geometry = Geometry {
            rotation_deg: 30.0,
            ..Geometry::default()
        };
        let rect = constrain_rect(&geometry, [0.0, 0.0, 1.0, 1.0], 1000, 800);
        let view_to_source = geometry.view_to_source_matrix(1000.0, 800.0);
        for corner in [
            [rect[0] * 1000.0, rect[1] * 800.0],
            [(rect[0] + rect[2]) * 1000.0, rect[1] * 800.0],
            [rect[0] * 1000.0, (rect[1] + rect[3]) * 800.0],
            [(rect[0] + rect[2]) * 1000.0, (rect[1] + rect[3]) * 800.0],
        ] {
            let mapped = apply(&view_to_source, corner);
            assert!(mapped[0] >= -1.0 && mapped[0] <= 1001.0, "{mapped:?}");
            assert!(mapped[1] >= -1.0 && mapped[1] <= 801.0, "{mapped:?}");
        }
        assert!(rect[2] > 0.3 && rect[2] < 1.0);
    }

    #[test]
    fn perspective_matrix_is_invertible() {
        let geometry = Geometry {
            perspective_vertical: 50.0,
            perspective_horizontal: -30.0,
            ..Geometry::default()
        };
        let forward = geometry.forward_matrix(800.0, 600.0);
        let inverse = geometry.view_to_source_matrix(800.0, 600.0);
        let roundtrip = mul(&inverse, &forward);
        for point in [[100.0, 100.0], [400.0, 300.0], [700.0, 500.0]] {
            let mapped = apply(&roundtrip, point);
            assert!((mapped[0] - point[0]).abs() < 1e-2, "{mapped:?}");
            assert!((mapped[1] - point[1]).abs() < 1e-2, "{mapped:?}");
        }
    }

    #[test]
    fn tile_matrix_offsets_both_spaces() {
        let full = IDENTITY;
        let tile = tile_matrix(&full, [10.0, 20.0], [100.0, 200.0]);
        let mapped = apply(&tile, [0.5, 0.5]);
        assert!((mapped[0] + 89.5).abs() < 1e-4, "{mapped:?}");
        assert!((mapped[1] + 179.5).abs() < 1e-4, "{mapped:?}");
        let mapped = apply(&tile, [30.0, 40.0]);
        assert!((mapped[0] + 60.0).abs() < 1e-4, "{mapped:?}");
        assert!((mapped[1] + 140.0).abs() < 1e-4, "{mapped:?}");
    }

    #[test]
    fn lens_params_activate_geometry() {
        let mut params = EditParams::default();
        params.lens.geometry.distortion = 40.0;
        let geometry = Geometry::from_params(&params);
        assert!(!geometry.is_identity());
        assert!(geometry.lens_model().is_some());
        let model = geometry.lens_model().unwrap();
        assert!((model.distortion_k() - 0.12).abs() < 1e-5);
    }

    #[test]
    fn distortion_changes_corner_sampling() {
        let source = gradient(100, 80);
        let plain = EditParams::default();
        let mut distorted = EditParams::default();
        distorted.lens.geometry.distortion = 80.0;
        let base = render_geometry(&source, &plain, GeometryMode::Crop).unwrap();
        let corrected = render_geometry(&source, &distorted, GeometryMode::Crop).unwrap();
        let mut changed = 0usize;
        for (left, right) in base.pixels().zip(corrected.pixels()) {
            if (left.0[0] as i32 - right.0[0] as i32).abs() > 4 {
                changed += 1;
            }
        }
        assert!(
            changed > 200,
            "畸变校正应显著改变边缘采样（{changed} 像素）"
        );
    }

    #[test]
    fn chromatic_aberration_separates_channels() {
        let source = gradient(120, 90);
        let mut params = EditParams::default();
        params.lens.chromatic_aberration = 100.0;
        let corrected = render_geometry(&source, &params, GeometryMode::Crop).unwrap();
        // 灰度渐变本应 R=G=B；色差校正后红蓝通道采样位置不同。
        let mut channel_gap = 0i32;
        for pixel in corrected.pixels() {
            channel_gap = channel_gap.max((pixel.0[0] as i32 - pixel.0[2] as i32).abs());
        }
        assert!(
            channel_gap > 5,
            "色差校正应使红蓝通道分离（差值 {channel_gap}）"
        );
    }

    #[test]
    fn vignette_correction_brightens_corners() {
        let mut source = image::RgbaImage::new(80, 60);
        for pixel in source.pixels_mut() {
            *pixel = image::Rgba([120, 120, 120, 255]);
        }
        let mut params = EditParams::default();
        params.lens.vignette = 100.0;
        let corrected = render_geometry(&source, &params, GeometryMode::Crop).unwrap();
        let corner = corrected.get_pixel(1, 1).0[0];
        let center = corrected.get_pixel(40, 30).0[0];
        assert!(
            corner > center + 10,
            "暗角校正应提亮四角：角 {corner} 中心 {center}"
        );
    }

    #[test]
    fn lens_scale_zooms_in() {
        let source = gradient(100, 80);
        let plain = EditParams::default();
        let mut scaled = EditParams::default();
        scaled.lens.geometry.scale = 200.0;
        let base = render_geometry(&source, &plain, GeometryMode::Crop).unwrap();
        let zoomed = render_geometry(&source, &scaled, GeometryMode::Crop).unwrap();
        // 水平渐变：放大后左边缘采样点更靠中心（更亮）。
        assert!(
            zoomed.get_pixel(1, 40).0[0] > base.get_pixel(1, 40).0[0] + 15,
            "缩放 200% 应放大画面：{} vs {}",
            zoomed.get_pixel(1, 40).0[0],
            base.get_pixel(1, 40).0[0]
        );
    }

    #[test]
    fn out_of_bounds_samples_are_transparent() {
        let source = gradient(64, 32);
        let geometry = Geometry {
            rotation_deg: 90.0,
            ..Geometry::default()
        };
        let params = params_with(geometry);
        let rendered = render_geometry(&source, &params, GeometryMode::Full).unwrap();
        // 90° 旋转后四角必然落在源图外，中心保持不透明。
        assert_eq!(rendered.get_pixel(0, 0).0[3], 0);
        assert_eq!(rendered.get_pixel(63, 31).0[3], 0);
        assert_eq!(rendered.get_pixel(32, 16).0[3], 255);
    }
}
