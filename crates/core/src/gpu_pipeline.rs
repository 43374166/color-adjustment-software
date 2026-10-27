//! 阶段 2/3/6：色彩管线、蒙版与几何变换（wgpu compute）。
//!
//! 每帧 dispatch 顺序（全链路线性 f16，末段才编码为 sRGB 8bit）：
//!
//! ```text
//! 0. cs_geometry        src（sRGB u8）→ stage（线性 f16）
//!                       裁剪 / 旋转 / 翻转 / 透视；恒等时直接用 src 作为 stage
//! 1. cs_base            stage → base（基础调色：白平衡 → 饱和度）
//! 2. cs_blur 水平/垂直   base → blur_big（纹理 / 清晰度 / 锐化 / 降噪参考）
//!                       需要时再跑一趟小半径 base → blur_small
//! 3. cs_main（全局）     base + 模糊 → work_a（细节链路，线性 f16）
//! 4. 每个蒙版：compose_local（全局 + 增量）→ cs_base → cs_blur → cs_main，
//!              按蒙版权重混入当前图像（A → B → A → B … ping-pong）
//! 5. 蒙版趟数为奇数时 cs_copy 把结果搬回 work_a
//! 6. cs_encode          work_a → display（rgba8unorm sRGB，唯一量化点）
//! 7. cs_overlay         蒙版叠加显示（写 8bit 暂存后拷回 display）
//! ```
//!
//! 参数通过 196 floats + 256×4 曲线 LUT + 4096 色调 LUT 的 uniform 传递，布局与
//! `shaders/color_pipeline.wgsl` 注释一一对应；曲线 LUT 的 alpha 通道存参数曲线。
//! 渲染目标（尺寸随裁剪变化）缓存在 [`ColorPipeline`] 内：同一张源图 + 同一
//! 输出尺寸 + 同一几何开关时复用纹理与绑定组。

use crate::error::{FdxmpError, Result};
use crate::geometry::{output_size, output_to_source_matrix, Geometry, GeometryMode, Mat3};
use crate::masks::{generate_mask, mask_key};
use crate::params::{eval_curve, EditParams, MaskParams, ToneCurve};

/// 源图编号（用于判断渲染目标是否需要重建）。
static SOURCE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// 标量参数数量（与 WGSL `Params.v` 对齐，49 个 vec4；见文件末尾的 [`slot`] 表）。
pub const PARAM_FLOATS: usize = 196;

/// 曲线 LUT 条目数。
pub const CURVE_ENTRIES: usize = 256;

/// 电影曲线 LUT 的 `vec4` 数（4 × 1024 = 4096 项，与 `fdxmp_color::tone::LUT_N` 一致）。
pub const TONE_ENTRIES: usize = 1024;

/// 电影曲线 LUT 的 float 数。
pub const TONE_LUT_FLOATS: usize = TONE_ENTRIES * 4;

/// 颜色均衡器的 uniform 区：8 个全局量 + 4 张 512 点表
///（色相偏移 / 饱和度增益 / 亮度增益 / 色域边界 `M²`）。
pub const EQ_FLOATS: usize = 8 + 4 * 512;

/// HSL 面板的 uniform 区：4 个全局量（开关）+ 3 张 512 点表。
pub const HSL_FLOATS: usize = 4 + 3 * 512;

/// 模糊半径（像素，9~13 抽头）。
pub const BLUR_RADIUS: f32 = 4.0;

/// 模糊半径的参考长边（像素）：代理预览尺寸。
///
/// 细节（纹理 / 清晰度 / 锐化 / 降噪）的半径按 `渲染长边 / 2048` 缩放，
/// 让代理预览、1:1 分块与全分辨率导出的强度一致。
pub const BLUR_REFERENCE_EDGE: f32 = 2048.0;

/// 模糊半径缩放上限（全分辨率导出最多 4× ≈ 12288px 长边的图；再大需要 mip 金字塔）。
///
/// 缩放只放大不缩小（下限 1.0）：小图保持既有观感，且半径缩到 1px 以下会因
/// 抽头取整而失去区分度。
pub const BLUR_SCALE_MAX: f32 = 4.0;

/// 全分辨率导出的分块边长（像素）。
///
/// 导出按分块渲染，GPU 只保留一个分块的工作集：45MP 全图的 f16 工作集约 2.9GB，
/// 分块后降到约 0.3GB，避免低显存机器导出失败。
pub const EXPORT_TILE_EDGE: u32 = 2048;

/// 参数下标（与 shader 注释一致）。
pub mod slot {
    /// 曝光。
    pub const EXPOSURE: usize = 0;
    /// 对比度。
    pub const CONTRAST: usize = 1;
    /// 高光。
    pub const HIGHLIGHTS: usize = 2;
    /// 阴影。
    pub const SHADOWS: usize = 3;
    /// 白色。
    pub const WHITES: usize = 4;
    /// 黑色。
    pub const BLACKS: usize = 5;
    /// 色温。
    pub const TEMPERATURE: usize = 6;
    /// 色调。
    pub const TINT: usize = 7;
    /// 自然饱和度。
    pub const VIBRANCE: usize = 8;
    /// 饱和度。
    pub const SATURATION: usize = 9;
    /// 纹理。
    pub const TEXTURE: usize = 10;
    /// 清晰度。
    pub const CLARITY: usize = 11;
    /// 去雾。
    pub const DEHAZE: usize = 12;
    /// 暗角。
    pub const VIGNETTE: usize = 13;
    /// 颗粒。
    pub const GRAIN: usize = 14;
    /// 锐化数量。
    pub const SHARPEN_AMOUNT: usize = 15;
    /// 降噪亮度。
    pub const NR_LUMINANCE: usize = 19;
    /// 降噪颜色。
    pub const NR_COLOR: usize = 22;
    /// 分级混合。
    pub const GRADING_BLENDING: usize = 25;
    /// 分级平衡。
    pub const GRADING_BALANCE: usize = 26;
    /// HSL 是否使用 Color Zones 连续插值（0 = LR 8 段）。
    pub const HSL_CONTINUOUS: usize = 27;
    /// HSL 起始下标（8 段 × 3）。
    pub const HSL_START: usize = 32;
    /// 颜色分级起始下标（3 区 × 3）。
    pub const GRADING_START: usize = 56;
    /// 颗粒种子。
    pub const GRAIN_SEED: usize = 65;
    /// 宽度。
    pub const WIDTH: usize = 66;
    /// 高度。
    pub const HEIGHT: usize = 67;
    /// 蒙版模式（0 = 全局，1 = 局部混合）。
    pub const MASK_MODE: usize = 68;
    /// 几何逆矩阵起始下标（9 个浮点，行主序：输出像素 → 源图像素）。
    pub const GEOMETRY_MATRIX: usize = 69;
    /// 几何是否启用（> 0.5 时蒙版按逆映射采样到源图坐标）。
    pub const GEOMETRY_ACTIVE: usize = 78;
    /// 小半径细节模糊是否有效（> 0.5 时锐化 / 降噪使用第二套模糊）。
    pub const DETAIL_BLUR: usize = 79;
    /// 蒙版叠加不透明度（0 = 不叠加）。
    pub const OVERLAY_ALPHA: usize = 80;
    /// 蒙版叠加颜色（线性 RGB，81..83）。
    pub const OVERLAY_RGB: usize = 81;
    /// 蒙版纹理相对源图坐标的比例（84 = x，85 = y）。
    ///
    /// 1:1 分块渲染时源图是全分辨率、蒙版仍按代理分辨率生成，
    /// 着色器用 `源像素 / 比例` 读取蒙版权重。
    pub const MASK_SCALE_X: usize = 84;
    /// 见 [`slot::MASK_SCALE_X`]。
    pub const MASK_SCALE_Y: usize = 85;
    /// 分块原点 x（整图输出坐标；非分块渲染为 0）。
    pub const TILE_ORIGIN_X: usize = 86;
    /// 分块原点 y。
    pub const TILE_ORIGIN_Y: usize = 87;
    /// 白平衡色适应矩阵（行主序，9 个 float；由色温 / 色调在 CPU 侧计算）。
    pub const WB_MATRIX: usize = 88;
    /// 整图输出宽度（分块渲染时 > 0，暗角 / 颗粒按整图坐标计算）。
    pub const FULL_SIZE_X: usize = 97;
    /// 整图输出高度。
    pub const FULL_SIZE_Y: usize = 98;
    /// 蒙版参考图相对整图源像素的原点 x（分块 / 裁剪渲染时蒙版坐标需要加回）。
    pub const MASK_ORIGIN_X: usize = 100;
    /// 蒙版参考图相对整图源像素的原点 y。
    pub const MASK_ORIGIN_Y: usize = 101;
    /// 颗粒大小 0..=100（0 = 逐像素细颗粒，100 = 约 8px）。
    pub const GRAIN_SIZE: usize = 104;
    /// 颗粒粗糙度 0..=100（0 = 平滑值噪声，100 = 逐像素哈希）。
    pub const GRAIN_ROUGHNESS: usize = 105;
    /// 暗角羽化 0..=100。
    pub const VIGNETTE_FEATHER: usize = 106;
    /// 暗角中点 0..=100（越小压暗范围越向中心扩展）。
    pub const VIGNETTE_MIDPOINT: usize = 107;

    /// 8 色带中心（OkLCh 色相，弧度；108..115，CPU 侧换算好，GPU 不做 HSV→OkLCh）。
    pub const BAND_HUES: usize = 108;
    /// 黑白模式开关（> 0.5 = 黑白）。
    pub const BW_ENABLE: usize = 116;
    /// 黑白混色（8 带，-100..=100；117..124）。
    pub const BW_MIX: usize = 117;
    /// 相机校准启用开关（> 0.5）。
    pub const CALIB_ENABLE: usize = 125;
    /// 校准基色矩阵（行主序 3×3；126..134）。
    pub const CALIB_MATRIX: usize = 126;
    /// 校准阴影色调（-1..=1）。
    pub const CALIB_TINT: usize = 135;
    /// 点颜色样本数（0..=4）。
    pub const POINT_COUNT: usize = 136;
    /// 点颜色样本起始下标（每个 [`fdxmp_color::ops::POINT_WORDS`] 个 float；137..176）。
    pub const POINT_START: usize = 137;
    /// 分级色轮向量（阴影 / 中间调 / 高光 / 全局 各 `(a, b, lum)`；177..188）。
    pub const GRADE_VEC: usize = 177;
    /// 分级分区中心（OkLab 明度）。
    pub const GRADE_M: usize = 189;
    /// 分级过渡宽度。
    pub const GRADE_WIDTH: usize = 190;
    /// 肤色色相（OkLCh 弧度；自然饱和度的保护中心）。
    pub const SKIN_HUE: usize = 191;
    /// 调色段是否有内容（0 = 整段跳过；由 CPU 侧的统一实现判断）。
    pub const COLOR_ACTIVE: usize = 192;

    /// 色调段是否有内容（0 = 整段跳过；对比度 / 高光 / 阴影 / 白色 / 黑色全为 0 时）。
    pub const TONE_ACTIVE: usize = 193;
    /// 参数曲线是否有内容（0 = 跳过；三个区域滑杆全为 0 时）。
    pub const PARAM_CURVE_ACTIVE: usize = 194;
    /// 曝光参考 LUT 是否启用（|EV| ≤ 1 时）。
    pub const EXPOSURE_CURVE_ACTIVE: usize = 195;
}

/// 把 `EditParams` 打成 uniform 数组（[`PARAM_FLOATS`] + 1024 floats）。
pub fn pack_params(params: &EditParams, width: u32, height: u32) -> Vec<f32> {
    let mut data = vec![
        0.0f32;
        PARAM_FLOATS + CURVE_ENTRIES * 4 + TONE_LUT_FLOATS + EQ_FLOATS + HSL_FLOATS
    ];
    let light = &params.light;
    let color = &params.color;
    let effects = &params.effects;

    data[slot::EXPOSURE] = light.exposure;
    data[slot::CONTRAST] = light.contrast;
    data[slot::HIGHLIGHTS] = light.highlights;
    data[slot::SHADOWS] = light.shadows;
    data[slot::WHITES] = light.whites;
    data[slot::BLACKS] = light.blacks;
    data[slot::TEMPERATURE] = color.temperature;
    data[slot::TINT] = color.tint;
    // 白平衡：由色温 / 色调算 Bradford 色适应矩阵（着色器第 1 段直接应用）。
    let white_balance = crate::white_balance::adaptation_matrix_for(
        color.temperature,
        color.tint,
        color.wb_reference_kelvin,
    );
    for (index, value) in white_balance.iter().flatten().enumerate() {
        data[slot::WB_MATRIX + index] = *value;
    }
    data[slot::VIBRANCE] = color.vibrance;
    data[slot::SATURATION] = color.saturation;
    data[slot::TEXTURE] = effects.texture;
    data[slot::CLARITY] = effects.clarity;
    data[slot::DEHAZE] = effects.dehaze;
    data[slot::VIGNETTE] = effects.vignette;
    data[slot::GRAIN] = effects.grain;
    data[slot::VIGNETTE_MIDPOINT] = effects.vignette_midpoint;
    data[slot::VIGNETTE_FEATHER] = effects.vignette_feather;
    data[slot::GRAIN_SIZE] = effects.grain_size;
    data[slot::GRAIN_ROUGHNESS] = effects.grain_roughness;

    data[slot::SHARPEN_AMOUNT] = params.detail.sharpen.amount;
    data[16] = params.detail.sharpen.radius;
    data[17] = params.detail.sharpen.detail;
    data[18] = params.detail.sharpen.masking;
    data[slot::NR_LUMINANCE] = params.detail.noise.luminance;
    data[20] = params.detail.noise.luminance_detail;
    data[21] = params.detail.noise.luminance_contrast;
    data[slot::NR_COLOR] = params.detail.noise.color;
    data[23] = params.detail.noise.color_detail;
    data[24] = params.detail.noise.color_smoothness;

    data[slot::GRADING_BLENDING] = params.grading.blending;
    data[slot::GRADING_BALANCE] = params.grading.balance;

    data[slot::HSL_CONTINUOUS] = 0.0;
    for (band_index, band) in crate::params::ColorBand::ALL.iter().enumerate() {
        let hsl = params.hsl.band(*band);
        let base = slot::HSL_START + band_index * 3;
        data[base] = hsl.hue;
        data[base + 1] = hsl.saturation;
        data[base + 2] = hsl.luminance;
    }

    for (zone_index, zone) in [
        &params.grading.shadows,
        &params.grading.midtones,
        &params.grading.highlights,
    ]
    .iter()
    .enumerate()
    {
        let base = slot::GRADING_START + zone_index * 3;
        data[base] = zone.hue;
        data[base + 1] = zone.saturation;
        data[base + 2] = zone.luminance;
    }

    data[slot::GRAIN_SEED] = 17.0;
    data[slot::WIDTH] = width as f32;
    data[slot::HEIGHT] = height as f32;
    data[slot::MASK_MODE] = 0.0;
    // 调色段（8 色带中心 / 黑白 / 校准 / 点颜色 / 分级向量）：与 CPU `ColorStage` 同一份参数。
    crate::color_engine::pack_color_slots(&mut data, params);
    pack_geometry(&mut data, &crate::geometry::IDENTITY, false);
    data[slot::DETAIL_BLUR] = 0.0;
    data[slot::MASK_SCALE_X] = 1.0;
    data[slot::MASK_SCALE_Y] = 1.0;

    // 曲线 LUT：主曲线 → 各通道曲线，逐项合成。
    // alpha 通道不被曲线查找使用，复用为参数曲线 LUT（x = index / (N-1)），
    // 这样 GPU 不需要额外 uniform 区，也不会和 CPU 的参数曲线语义分叉。
    let master = build_curve_lut(&params.curves.rgb);
    let red = build_curve_lut(&params.curves.red);
    let green = build_curve_lut(&params.curves.green);
    let blue = build_curve_lut(&params.curves.blue);
    let parametric = crate::params::parametric_curve_lut(&params.curves.parametric);
    let curve_base = PARAM_FLOATS;
    for index in 0..CURVE_ENTRIES {
        let master_value = master[index];
        let sample_index = ((master_value.clamp(0.0, 1.0) * 255.0) + 0.5) as usize;
        let sample_index = sample_index.min(CURVE_ENTRIES - 1);
        data[curve_base + index * 4] = red[sample_index];
        data[curve_base + index * 4 + 1] = green[sample_index];
        data[curve_base + index * 4 + 2] = blue[sample_index];
        data[curve_base + index * 4 + 3] = parametric[index];
    }
    data[slot::PARAM_CURVE_ACTIVE] = if params.curves.parametric.is_identity() {
        0.0
    } else {
        1.0
    };

    // 色调段：4 张 256 项参考曲线 LUT（高光 / 阴影 / 白色 / 黑色）。
    // 复用原先电影曲线占用的 uniform 区，前 1024 项有效，其余保持 0。
    let tone_lut_base = curve_base + CURVE_ENTRIES * 4;
    let tone = crate::tone::ToneStage::from_params(params);
    for (slot_index, lut) in tone.luts().iter().enumerate() {
        let base = tone_lut_base + slot_index * crate::tone::TONE_LUT_SIZE;
        for (index, value) in lut.iter().enumerate() {
            data[base + index] = *value;
        }
    }
    data[slot::TONE_ACTIVE] = if tone.is_identity() { 0.0 } else { 1.0 };
    data[slot::EXPOSURE_CURVE_ACTIVE] =
        if params.light.exposure != 0.0 && params.light.exposure.abs() <= 1.0 {
            1.0
        } else {
            0.0
        };

    pack_equalizer(&mut data, params);
    pack_hsl_curves(&mut data, params);

    data
}

/// 把 HSL 面板的三条曲线（Lightroom 8 段 → dt UCS 周期 RBF）写进 uniform。
///
/// 与 CPU `HslStage` 用的是同一份烘焙结果；全平（未调整）时只写 0 标志。
fn pack_hsl_curves(data: &mut [f32], params: &EditParams) {
    let base = PARAM_FLOATS + CURVE_ENTRIES * 4 + TONE_LUT_FLOATS + EQ_FLOATS;
    let bands = crate::params::ColorBand::ALL;
    let hue: [f32; 8] = std::array::from_fn(|index| params.hsl.band(bands[index]).hue);
    let saturation: [f32; 8] =
        std::array::from_fn(|index| params.hsl.band(bands[index]).saturation);
    let luminance: [f32; 8] =
        std::array::from_fn(|index| params.hsl.band(bands[index]).luminance);

    let neutral = hue.iter().all(|v| v.abs() < 1e-6)
        && saturation.iter().all(|v| v.abs() < 1e-6)
        && luminance.iter().all(|v| v.abs() < 1e-6);
    data[base] = if neutral { 0.0 } else { 1.0 };
    if neutral {
        return;
    }

    let curves = fdxmp_color::equalizer::HslCurves::from_bands(hue, saturation, luminance);
    data[base + 1] = fdxmp_color::ucs::l_white_of(1.0);
    for index in 0..512 {
        data[base + 4 + index] = curves.hue[index];
        data[base + 4 + 512 + index] = curves.saturation[index];
        data[base + 4 + 1024 + index] = curves.brightness[index];
    }
}

/// 把颜色均衡器的全局参数与 3 张色相 LUT、色域边界表写进 uniform。
///
/// LUT 在 CPU 侧由 [`fdxmp_color::ColorEqualizer`] 烘焙（8 节点周期 RBF），
/// 与 CPU 预览 / 导出用的是同一份数据；关闭时只写一个 0 标志。
fn pack_equalizer(data: &mut [f32], params: &EditParams) {
    let eq = &params.equalizer;
    let base = PARAM_FLOATS + CURVE_ENTRIES * 4 + TONE_LUT_FLOATS;
    data[base] = if eq.is_active() { 1.0 } else { 0.0 };
    data[base + 1] = -0.015 + 0.3 * (5.0 * eq.threshold).powi(2);
    data[base + 2] = -60.0 - 40.0 * eq.contrast;
    data[base + 3] = if eq.use_filter { 1.0 } else { 0.0 };
    data[base + 4] = fdxmp_color::ucs::l_white_of(eq.white_level.exp2());
    data[base + 5] = 0.5 * eq.chroma_size;
    data[base + 6] = 0.5 * eq.param_size;
    let max_brightness = eq.brightness.iter().fold(1.0f32, |acc, v| acc.max(*v));
    data[base + 7] =
        data[base + 1] + 0.01 * max_brightness * 5.0f32.min(eq.param_size.max(0.0).sqrt());

    // 始终烘焙：HSL 面板要用同一张色域边界表（同一工作空间），
    // 而且两张表的色域映射必须一致。关闭均衡器时只是把开关写 0。
    let equalizer =
        fdxmp_color::ColorEqualizer::new(eq.to_engine(), &crate::color_engine::WORKSPACE);
    let (hue, saturation, brightness, gamut) = equalizer.luts();
    for index in 0..512 {
        data[base + 8 + index] = hue[index];
        data[base + 8 + 512 + index] = saturation[index];
        data[base + 8 + 1024 + index] = brightness[index];
        data[base + 8 + 1536 + index] = gamut[index];
    }
}

/// 把几何逆矩阵与启用开关写入参数包（下标见 [`slot::GEOMETRY_MATRIX`]）。
pub fn pack_geometry(data: &mut [f32], matrix: &Mat3, active: bool) {
    let base = slot::GEOMETRY_MATRIX;
    for row in 0..3 {
        for col in 0..3 {
            data[base + row * 3 + col] = matrix[row][col];
        }
    }
    data[slot::GEOMETRY_ACTIVE] = if active { 1.0 } else { 0.0 };
}

/// 单条曲线的 256 项 LUT（单调三次插值，严格过控制点）。
///
/// 插值由 `fdxmp-color` 的 [`fdxmp_color::MonotoneCurve`] 提供（与 CPU 路径、预设导入同一实现）；
/// 控制点退化（少于两个点 / x 不递增）时回退到 [`crate::params::eval_curve`]。
pub fn build_curve_lut(curve: &ToneCurve) -> [f32; CURVE_ENTRIES] {
    let mut lut = [0.0f32; CURVE_ENTRIES];
    let points: Vec<(f64, f64)> = curve
        .points
        .iter()
        .map(|point| (point.x as f64, point.y as f64))
        .collect();
    match fdxmp_color::MonotoneCurve::new(&points) {
        Some(spline) => {
            let (first, last) = (points[0].0, points[points.len() - 1].0);
            for (index, value) in lut.iter_mut().enumerate() {
                let x = index as f64 / (CURVE_ENTRIES - 1) as f64;
                *value = spline.eval(x.clamp(first, last)).clamp(0.0, 1.0) as f32;
            }
        }
        None => {
            for (index, value) in lut.iter_mut().enumerate() {
                let x = index as f32 / (CURVE_ENTRIES - 1) as f32;
                *value = eval_curve(&curve.points, x);
            }
        }
    }
    lut
}

/// 蒙版权重纹理缓存项。
struct MaskTexture {
    hash: u64,
    // 纹理必须保持存活：`view` 依赖它，drop 后视图会失效。
    #[allow(dead_code)]
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// GPU 上的源图（上传一次；渲染目标由 [`ColorPipeline`] 按裁剪尺寸维护）。
pub struct PipelineImage {
    /// 源图宽度。
    pub width: u32,
    /// 源图高度。
    pub height: u32,
    id: u64,
    source_pixels: image::RgbaImage,
    source_view: wgpu::TextureView,
    /// 蒙版参考图（1:1 分块渲染时指向代理图）、相对源图的缩放与源图原点。
    mask_reference: Option<MaskReference>,
    #[allow(dead_code)]
    source_texture: wgpu::Texture,
}

impl PipelineImage {
    /// 设置蒙版参考图：蒙版权重按该图（通常为代理图）生成，
    /// 着色器用 `(源像素 + origin) / scale` 换算到蒙版纹理坐标
    /// （1:1 分块 / 分块导出时源坐标是裁剪图坐标，需要加回原点）。
    pub fn set_mask_reference(
        &mut self,
        pixels: std::sync::Arc<image::RgbaImage>,
        scale: [f32; 2],
        origin: [f32; 2],
    ) {
        self.mask_reference = Some(MaskReference {
            pixels,
            scale,
            origin,
        });
    }

    /// 清除蒙版参考图（恢复按源图生成蒙版）。
    pub fn clear_mask_reference(&mut self) {
        self.mask_reference = None;
    }

    /// 当前蒙版缩放（无参考图时为 1:1）。
    pub fn mask_scale(&self) -> [f32; 2] {
        self.mask_reference
            .as_ref()
            .map(|reference| reference.scale)
            .unwrap_or([1.0, 1.0])
    }

    /// 当前蒙版参考图的源图原点（无参考图时为 0）。
    pub fn mask_origin(&self) -> [f32; 2] {
        self.mask_reference
            .as_ref()
            .map(|reference| reference.origin)
            .unwrap_or([0.0, 0.0])
    }
}

/// 蒙版参考图：代理像素 + 相对源图的缩放 + 源图原点（裁剪 / 分块坐标）。
struct MaskReference {
    pixels: std::sync::Arc<image::RgbaImage>,
    scale: [f32; 2],
    origin: [f32; 2],
}

/// 蒙版叠加样式（颜色为显示空间 RGB，0..1）。
#[derive(Debug, Clone, Copy)]
pub struct OverlayStyle {
    /// 叠加颜色。
    pub color: [f32; 3],
    /// 不透明度，0..1。
    pub alpha: f32,
}

/// 渲染计划：输出尺寸 + 输出→源矩阵 + 是否启用几何趟。
#[derive(Debug, Clone, Copy)]
pub struct RenderPlan {
    /// 输出像素 → 源像素 的矩阵（含透视除法）。
    pub matrix: Mat3,
    /// 输出（渲染目标）宽度。
    pub width: u32,
    /// 输出高度。
    pub height: u32,
    /// 是否启用几何趟（恒等裁剪可关闭以省一趟）。
    pub geometry_active: bool,
    /// 分块渲染时该分块在**整图输出**中的原点（非分块为 `[0.0, 0.0]`）。
    pub tile_origin: [f32; 2],
    /// 分块渲染时的整图输出尺寸；`[0.0, 0.0]` 表示非分块。
    /// 暗角 / 颗粒按整图坐标计算，保证分块与整图渲染一致。
    pub full_size: [f32; 2],
}

/// 渲染目标：尺寸随裁剪变化，几何开关决定 stage 是否走 `cs_geometry`。
struct RenderTarget {
    source_id: u64,
    width: u32,
    height: u32,
    geometry_active: bool,
    generation: u64,
    #[allow(dead_code)]
    stage_texture: Option<wgpu::Texture>,
    stage_view: wgpu::TextureView,
    /// 基础调色结果（线性 f16，未编码）。
    #[allow(dead_code)]
    base_texture: wgpu::Texture,
    base_view: wgpu::TextureView,
    #[allow(dead_code)]
    blur_a_texture: wgpu::Texture,
    #[allow(dead_code)]
    blur_a_view: wgpu::TextureView,
    #[allow(dead_code)]
    blur_b_texture: wgpu::Texture,
    blur_b_view: wgpu::TextureView,
    #[allow(dead_code)]
    blur_a2_texture: wgpu::Texture,
    #[allow(dead_code)]
    blur_a2_view: wgpu::TextureView,
    #[allow(dead_code)]
    blur_small_texture: wgpu::Texture,
    blur_small_view: wgpu::TextureView,
    /// 细节结果 ping-pong（线性 f16，蒙版叠加用）。
    #[allow(dead_code)]
    work_a_texture: wgpu::Texture,
    work_a_view: wgpu::TextureView,
    #[allow(dead_code)]
    work_b_texture: wgpu::Texture,
    work_b_view: wgpu::TextureView,
    /// 显示纹理（sRGB 8bit，管线唯一编码输出；egui 与回读使用）。
    #[allow(dead_code)]
    display_texture: wgpu::Texture,
    display_view: wgpu::TextureView,
    /// 叠加显示用的 8bit 暂存（避免同一纹理同时读写）。
    #[allow(dead_code)]
    display_scratch_texture: wgpu::Texture,
    display_scratch_view: wgpu::TextureView,
    #[allow(dead_code)]
    dummy_texture: wgpu::Texture,
    dummy_view: wgpu::TextureView,
    #[allow(dead_code)]
    dummy_float_texture: wgpu::Texture,
    dummy_float_view: wgpu::TextureView,
    geometry_bind_group: Option<wgpu::BindGroup>,
    base_bind_group: wgpu::BindGroup,
    blur_h_bind_group: wgpu::BindGroup,
    blur_v_bind_group: wgpu::BindGroup,
    blur_h2_bind_group: wgpu::BindGroup,
    blur_v2_bind_group: wgpu::BindGroup,
    main_bind_group: wgpu::BindGroup,
}

/// 色彩管线（可复用；蒙版纹理按内容哈希缓存）。
pub struct ColorPipeline {
    geometry_pipeline: wgpu::ComputePipeline,
    blur_h_pipeline: wgpu::ComputePipeline,
    blur_v_pipeline: wgpu::ComputePipeline,
    blur_wide_h_pipeline: wgpu::ComputePipeline,
    blur_wide_v_pipeline: wgpu::ComputePipeline,
    base_pipeline: wgpu::ComputePipeline,
    main_pipeline: wgpu::ComputePipeline,
    encode_pipeline: wgpu::ComputePipeline,
    overlay_pipeline: wgpu::ComputePipeline,
    copy_pipeline: wgpu::ComputePipeline,
    geometry_layout: wgpu::BindGroupLayout,
    blur_layout: wgpu::BindGroupLayout,
    main_layout: wgpu::BindGroupLayout,
    overlay_layout: wgpu::BindGroupLayout,
    params_buffer: wgpu::Buffer,
    geometry_params_buffer: wgpu::Buffer,
    blur_params_buffer: wgpu::Buffer,
    mask_cache: Vec<MaskTexture>,
    /// 蒙版趟 bind group 缓存：键 = 目标代数 + 蒙版序号 + 乒乓相位 + 蒙版栅格哈希。
    mask_bind_cache: std::collections::HashMap<u64, wgpu::BindGroup>,
    target: Option<RenderTarget>,
    target_counter: u64,
}

impl ColorPipeline {
    /// 创建管线资源。
    pub fn new(device: &wgpu::Device) -> Self {
        let geometry_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fdxmp-geometry"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/geometry.wgsl").into()),
        });
        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fdxmp-blur"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/blur.wgsl").into()),
        });
        let color_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fdxmp-color-pipeline"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/color_pipeline.wgsl").into()),
        });

        let geometry_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fdxmp-geometry-layout"),
            entries: &[
                uniform_entry(0),
                texture_entry(1),
                storage_entry(2, wgpu::TextureFormat::Rgba16Float),
            ],
        });

        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fdxmp-blur-layout"),
            entries: &[
                uniform_entry(0),
                texture_entry(1),
                storage_entry(2, wgpu::TextureFormat::Rgba16Float),
            ],
        });

        let main_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fdxmp-main-layout"),
            entries: &[
                uniform_entry(0),
                texture_entry(1),
                texture_entry(2),
                storage_entry(3, wgpu::TextureFormat::Rgba16Float),
                // 蒙版权重是 R32Float，不可过滤（用 textureLoad 读取）
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                texture_entry(5),
                texture_entry(6),
            ],
        });

        // 叠加 / 编码入口：输出 8bit sRGB（binding 7），不使用 f16 存储写。
        let overlay_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fdxmp-overlay-layout"),
            entries: &[
                uniform_entry(0),
                texture_entry(1),
                texture_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                texture_entry(5),
                texture_entry(6),
                storage_entry(7, wgpu::TextureFormat::Rgba8Unorm),
            ],
        });

        let geometry_pipeline =
            create_compute_pipeline(device, &geometry_shader, &geometry_layout, "cs_geometry");
        let blur_h_pipeline =
            create_compute_pipeline(device, &blur_shader, &blur_layout, "cs_blur");
        let blur_v_pipeline =
            create_compute_pipeline(device, &blur_shader, &blur_layout, "cs_blur");
        let blur_wide_h_pipeline =
            create_compute_pipeline(device, &blur_shader, &blur_layout, "cs_blur_wide");
        let blur_wide_v_pipeline =
            create_compute_pipeline(device, &blur_shader, &blur_layout, "cs_blur_wide");
        let base_pipeline = create_compute_pipeline(device, &color_shader, &main_layout, "cs_base");
        let main_pipeline = create_compute_pipeline(device, &color_shader, &main_layout, "cs_main");
        let encode_pipeline =
            create_compute_pipeline(device, &color_shader, &overlay_layout, "cs_encode");
        let overlay_pipeline =
            create_compute_pipeline(device, &color_shader, &overlay_layout, "cs_overlay");
        let copy_pipeline = create_compute_pipeline(device, &color_shader, &main_layout, "cs_copy");

        let params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fdxmp-params"),
            size: ((PARAM_FLOATS + CURVE_ENTRIES * 4 + TONE_LUT_FLOATS + EQ_FLOATS + HSL_FLOATS)
                * std::mem::size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let geometry_params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fdxmp-geometry-params"),
            size: 20 * std::mem::size_of::<f32>() as u64,
            usage: wgpu::BufferUsages::UNIFORM
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let blur_params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fdxmp-blur-params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            geometry_pipeline,
            blur_h_pipeline,
            blur_v_pipeline,
            blur_wide_h_pipeline,
            blur_wide_v_pipeline,
            base_pipeline,
            main_pipeline,
            encode_pipeline,
            overlay_pipeline,
            copy_pipeline,
            geometry_layout,
            blur_layout,
            main_layout,
            overlay_layout,
            params_buffer,
            geometry_params_buffer,
            blur_params_buffer,
            mask_cache: Vec::new(),
            mask_bind_cache: std::collections::HashMap::new(),
            target: None,
            target_counter: 0,
        }
    }

    /// 上传 RGBA8（sRGB 编码）像素，建立各趟绑定。
    pub fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<PipelineImage> {
        if width == 0 || height == 0 || rgba.len() < (width * height * 4) as usize {
            return Err(FdxmpError::Gpu("上传尺寸与像素数据不匹配".to_string()));
        }
        let extent = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let source = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fdxmp-source"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &source,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            extent,
        );

        let source_pixels = image::RgbaImage::from_raw(
            width,
            height,
            rgba[..(width * height * 4) as usize].to_vec(),
        )
        .ok_or_else(|| FdxmpError::Gpu("源像素尺寸不匹配".to_string()))?;

        Ok(PipelineImage {
            width,
            height,
            id: SOURCE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            source_pixels,
            source_view,
            mask_reference: None,
            source_texture: source,
        })
    }

    /// 上传线性 f32 RGBA（0..1，sRGB 解码后的线性值），纹理格式为 `rgba16float`。
    ///
    /// 与 [`ColorPipeline::upload`] 的区别：源数据不经过 8bit 量化，
    /// 16bit RAW / TIFF 的渐变与暗部精度得以保留（着色器读取即线性，无需 sRGB 解码）。
    pub fn upload_linear_f32(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[f32],
        width: u32,
        height: u32,
    ) -> Result<PipelineImage> {
        let pixel_count = (width as usize) * (height as usize);
        if width == 0 || height == 0 || rgba.len() < pixel_count * 4 {
            return Err(FdxmpError::Gpu("上传尺寸与像素数据不匹配".to_string()));
        }
        let extent = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let source = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fdxmp-source-linear"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let source_view = source.create_view(&wgpu::TextureViewDescriptor::default());

        let mut half: Vec<u8> = Vec::with_capacity(pixel_count * 8);
        let mut eight: Vec<u8> = vec![0u8; pixel_count * 4];
        for (index, pixel) in rgba[..pixel_count * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
        {
            for channel in 0..4 {
                let value = pixel[channel];
                half.extend_from_slice(&f32_to_f16(value).to_le_bytes());
                if channel < 3 {
                    eight[index * 4 + channel] = linear_to_srgb_u8(value);
                } else {
                    eight[index * 4 + 3] = (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
            }
        }

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &source,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &half,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 8),
                rows_per_image: Some(height),
            },
            extent,
        );

        let source_pixels = image::RgbaImage::from_raw(width, height, eight)
            .ok_or_else(|| FdxmpError::Gpu("源像素尺寸不匹配".to_string()))?;

        Ok(PipelineImage {
            width,
            height,
            id: SOURCE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            source_pixels,
            source_view,
            mask_reference: None,
            source_texture: source,
        })
    }

    /// 按当前参数渲染一帧（默认按裁剪出图；恒等裁剪时与旧行为一致）。
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &PipelineImage,
        params: &EditParams,
    ) {
        self.render_with(device, queue, image, params, GeometryMode::Crop);
    }

    /// 按指定几何模式渲染一帧（全局 + 各蒙版局部调整）。
    ///
    /// - [`GeometryMode::Crop`]：正常显示 / 导出（应用裁剪矩形）；
    /// - [`GeometryMode::Full`]：裁剪编辑视图（只做旋转 / 翻转 / 透视，不裁剪）。
    pub fn render_with(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &PipelineImage,
        params: &EditParams,
        mode: GeometryMode,
    ) {
        let geometry = Geometry::from_params(params);
        let (width, height) = output_size(params, image.width, image.height, mode);
        let crop_rect = if mode == GeometryMode::Full {
            [0.0, 0.0, 1.0, 1.0]
        } else {
            params.crop.rect
        };
        let matrix = output_to_source_matrix(
            &geometry,
            crop_rect,
            image.width,
            image.height,
            width,
            height,
        );
        // 裁剪矩形变化也需要几何趟；全幅模式忽略裁剪矩形。
        let crop_active = mode == GeometryMode::Crop && params.crop.rect != [0.0, 0.0, 1.0, 1.0];
        let plan = RenderPlan {
            matrix,
            width,
            height,
            geometry_active: !geometry.is_identity() || crop_active,
            tile_origin: [0.0, 0.0],
            full_size: [0.0, 0.0],
        };
        self.render_internal(device, queue, image, params, &plan);
    }

    /// 以自定义“输出像素 → 源像素”矩阵渲染（1:1 分块渲染用）。
    ///
    /// 调用方通过 [`crate::geometry::tile_matrix`] 把整图矩阵换算成
    /// 分块矩阵；蒙版按 `image` 的蒙版参考图（通常为代理图）采样。
    pub fn render_tile(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &PipelineImage,
        params: &EditParams,
        plan: &RenderPlan,
    ) {
        self.render_internal(device, queue, image, params, plan);
    }

    /// 渲染主体：几何 → 模糊 → 调色 → 蒙版（所有入口共用）。
    fn render_internal(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &PipelineImage,
        params: &EditParams,
        plan: &RenderPlan,
    ) {
        let matrix = &plan.matrix;
        let (width, height) = (plan.width, plan.height);
        self.ensure_target(device, image, width, height, plan.geometry_active);
        let Some(target) = self.target.as_ref() else {
            return;
        };
        let geometry_active = target.geometry_active;
        let target_generation = target.generation;
        let geometry_bind_group = target.geometry_bind_group.clone();
        let base_bind_group = target.base_bind_group.clone();
        let blur_h_bind_group = target.blur_h_bind_group.clone();
        let blur_v_bind_group = target.blur_v_bind_group.clone();
        let blur_h2_bind_group = target.blur_h2_bind_group.clone();
        let blur_v2_bind_group = target.blur_v2_bind_group.clone();
        let main_bind_group = target.main_bind_group.clone();
        let stage_view = target.stage_view.clone();
        let base_view = target.base_view.clone();
        let blur_b_view = target.blur_b_view.clone();
        let blur_small_view = target.blur_small_view.clone();
        let work_a_view = target.work_a_view.clone();
        let work_b_view = target.work_b_view.clone();
        let display_view = target.display_view.clone();
        let dummy_view = target.dummy_view.clone();
        let dummy_float_view = target.dummy_float_view.clone();
        let groups = div_ceil(width, 8);
        let rows = div_ceil(height, 8);
        let blur_scale = detail_blur_scale(plan);

        // 全局参数包（几何矩阵同时供蒙版逆映射采样使用）
        let mut packed = pack_params(params, width, height);
        pack_geometry(&mut packed, matrix, geometry_active);
        let mask_scale = image.mask_scale();
        packed[slot::MASK_SCALE_X] = mask_scale[0];
        packed[slot::MASK_SCALE_Y] = mask_scale[1];
        let mask_origin = image.mask_origin();
        packed[slot::MASK_ORIGIN_X] = mask_origin[0];
        packed[slot::MASK_ORIGIN_Y] = mask_origin[1];
        packed[slot::TILE_ORIGIN_X] = plan.tile_origin[0];
        packed[slot::TILE_ORIGIN_Y] = plan.tile_origin[1];
        packed[slot::FULL_SIZE_X] = plan.full_size[0];
        packed[slot::FULL_SIZE_Y] = plan.full_size[1];
        packed[slot::DETAIL_BLUR] = if needs_small_blur(params) { 1.0 } else { 0.0 };
        queue.write_buffer(&self.params_buffer, 0, &as_bytes(&packed));

        // 几何趟（可选）+ 基础调色：合并进同一 encoder / 一次提交
        // （几何输出 stage 是 base 的输入；同一 encoder 内 compute pass 按提交顺序执行）。
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fdxmp-base-encoder"),
        });
        if let Some(geometry_bind_group) = &geometry_bind_group {
            let mut data = [0.0f32; 20];
            for row in 0..3 {
                for col in 0..3 {
                    // uniform 布局为 3 个 vec4 行（第 4 个分量补 0）。
                    data[row * 4 + col] = matrix[row][col];
                }
            }
            data[12] = image.width as f32;
            data[13] = image.height as f32;
            data[14] = width as f32;
            data[15] = height as f32;
            // 镜头校正系数（与 CPU 参考实现共用同一套公式）。
            let lens = Geometry::from_params(params).lens_model();
            data[16] = lens.map(|model| model.distortion_k()).unwrap_or(0.0);
            data[17] = lens.map(|model| model.chromatic_scale()).unwrap_or(0.0);
            data[18] = lens.map(|model| model.vignette_strength()).unwrap_or(0.0);
            data[19] = if lens.is_some() { 1.0 } else { 0.0 };
            queue.write_buffer(&self.geometry_params_buffer, 0, &as_bytes(&data));
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fdxmp-geometry"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.geometry_pipeline);
            pass.set_bind_group(0, geometry_bind_group, &[]);
            pass.dispatch_workgroups(groups, rows, 1);
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fdxmp-base"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.base_pipeline);
            pass.set_bind_group(0, &base_bind_group, &[]);
            pass.dispatch_workgroups(groups, rows, 1);
        }
        queue.submit(Some(encoder.finish()));

        // 模糊参考一律取自"基础调色结果"，与细节链路同域
        // （旧实现取未调色的原图，曝光 / 白平衡一开就产生直流偏移与色偏）。
        self.render_blurs(
            device,
            queue,
            groups,
            rows,
            needs_big_blur(params),
            needs_small_blur(params),
            BLUR_RADIUS * blur_scale,
            params.detail.sharpen.radius * blur_scale,
            &blur_h_bind_group,
            &blur_v_bind_group,
            &blur_h2_bind_group,
            &blur_v2_bind_group,
        );

        // 全局细节链路 → work_a（线性 f16）
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fdxmp-main-encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fdxmp-main"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.main_pipeline);
            pass.set_bind_group(0, &main_bind_group, &[]);
            pass.dispatch_workgroups(groups, rows, 1);
        }
        // 无蒙版：编码趟可直接并入同一 encoder（省一次提交）
        if params.masks.is_empty() {
            let encode_bind_group = self.encode_bind_group(
                device,
                &work_a_view,
                &display_view,
                &dummy_view,
                &dummy_float_view,
            );
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fdxmp-encode"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.encode_pipeline);
                pass.set_bind_group(0, &encode_bind_group, &[]);
                pass.dispatch_workgroups(groups, rows, 1);
            }
            queue.submit(Some(encoder.finish()));
            return;
        }
        queue.submit(Some(encoder.finish()));

        // 逐蒙版：用该蒙版的参数重跑管线，按权重混入当前图像（A/B ping-pong）。
        let mut current_is_a = true;
        for (index, mask) in params.masks.iter().enumerate() {
            // 隐藏的蒙版不参与合成。
            if !mask.visible {
                continue;
            }
            let (mask_view, mask_hash) =
                match self.ensure_mask_texture(device, queue, index, mask, image) {
                    Ok(value) => value,
                    Err(_) => continue,
                };

            // 蒙版参数是相对全局的增量：先合成，再按
            // "基础调色 → 模糊 → 细节"完整重跑，模糊同样取自蒙版自己的基础结果。
            let composed = params.compose_local(&mask.adjustments);
            let mut mask_packed = pack_params(&composed, width, height);
            mask_packed[slot::MASK_MODE] = 1.0;
            pack_geometry(&mut mask_packed, matrix, geometry_active);
            mask_packed[slot::DETAIL_BLUR] = if needs_small_blur(&composed) {
                1.0
            } else {
                0.0
            };
            mask_packed[slot::MASK_SCALE_X] = mask_scale[0];
            mask_packed[slot::MASK_SCALE_Y] = mask_scale[1];
            mask_packed[slot::MASK_ORIGIN_X] = mask_origin[0];
            mask_packed[slot::MASK_ORIGIN_Y] = mask_origin[1];
            mask_packed[slot::TILE_ORIGIN_X] = plan.tile_origin[0];
            mask_packed[slot::TILE_ORIGIN_Y] = plan.tile_origin[1];
            mask_packed[slot::FULL_SIZE_X] = plan.full_size[0];
            mask_packed[slot::FULL_SIZE_Y] = plan.full_size[1];
            queue.write_buffer(&self.params_buffer, 0, &as_bytes(&mask_packed));

            let mask_needs_blur = needs_big_blur(&composed) || needs_small_blur(&composed);
            // 蒙版基础调色 → base（覆盖上一趟内容；全局细节已完成，可安全复用）
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fdxmp-mask-base-encoder"),
            });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fdxmp-mask-base"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.base_pipeline);
                pass.set_bind_group(0, &base_bind_group, &[]);
                pass.dispatch_workgroups(groups, rows, 1);
            }
            // 需要细节模糊时先提交 base（模糊要读它），随后再开一个 encoder 跑细节混合；
            // 不需要模糊（只调曝光 / 颜色等）时 base 与细节混合合并成一次提交。
            if mask_needs_blur {
                queue.submit(Some(encoder.finish()));
                self.render_blurs(
                    device,
                    queue,
                    groups,
                    rows,
                    needs_big_blur(&composed),
                    needs_small_blur(&composed),
                    BLUR_RADIUS * blur_scale,
                    composed.detail.sharpen.radius * blur_scale,
                    &blur_h_bind_group,
                    &blur_v_bind_group,
                    &blur_h2_bind_group,
                    &blur_v2_bind_group,
                );
                encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("fdxmp-mask-encoder"),
                });
            }

            let (current_view, destination_view) = if current_is_a {
                (work_a_view.clone(), work_b_view.clone())
            } else {
                (work_b_view.clone(), work_a_view.clone())
            };
            // bind group 只依赖（目标代数, 蒙版序号, 乒乓相位, 蒙版栅格），缓存复用；
            // 拖动局部调色滑块不改变这些量，因此不再每帧重建。
            let bind_key = mask_bind_key(target_generation, index, current_is_a, mask_hash);
            let bind_group = if let Some(cached) = self.mask_bind_cache.get(&bind_key) {
                cached.clone()
            } else {
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("fdxmp-mask-pass"),
                    layout: &self.main_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Buffer(
                                self.params_buffer.as_entire_buffer_binding(),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&base_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&blur_b_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(&destination_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(&mask_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::TextureView(&current_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: wgpu::BindingResource::TextureView(&blur_small_view),
                        },
                    ],
                });
                if self.mask_bind_cache.len() >= 64 {
                    self.mask_bind_cache.clear();
                }
                self.mask_bind_cache.insert(bind_key, bind_group.clone());
                bind_group
            };

            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fdxmp-mask"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.main_pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(div_ceil(width, 8), div_ceil(height, 8), 1);
            }
            queue.submit(Some(encoder.finish()));
            current_is_a = !current_is_a;
        }

        // 蒙版趟数为奇数：结果在 work_b，搬回 work_a
        if !current_is_a {
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fdxmp-copy"),
                layout: &self.main_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(
                            self.params_buffer.as_entire_buffer_binding(),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&stage_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&blur_b_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&work_a_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&dummy_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(&dummy_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::TextureView(&work_b_view),
                    },
                ],
            });
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fdxmp-copy-encoder"),
            });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fdxmp-copy"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.copy_pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(div_ceil(width, 8), div_ceil(height, 8), 1);
            }
            queue.submit(Some(encoder.finish()));
        }

        // 编码：线性 f16 → sRGB 8bit（整条管线唯一的量化 / 裁剪点）
        let encode_bind_group = self.encode_bind_group(
            device,
            &work_a_view,
            &display_view,
            &dummy_view,
            &dummy_float_view,
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fdxmp-encode-encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fdxmp-encode"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.encode_pipeline);
            pass.set_bind_group(0, &encode_bind_group, &[]);
            pass.dispatch_workgroups(groups, rows, 1);
        }
        queue.submit(Some(encoder.finish()));
    }

    /// 编码趟 bind group（线性 f16 → sRGB 8bit 显示纹理）。
    fn encode_bind_group(
        &self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
        display: &wgpu::TextureView,
        dummy: &wgpu::TextureView,
        dummy_float: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fdxmp-encode"),
            layout: &self.overlay_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(
                        self.params_buffer.as_entire_buffer_binding(),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(dummy_float),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(dummy),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(dummy_float),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(dummy_float),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(display),
                },
            ],
        })
    }

    /// 从 `base` 纹理生成大 / 小半径模糊（两步都可按需省略）。
    ///
    /// 模糊参考必须与细节链路同域，因此统一从基础调色结果（base）读取，
    /// 蒙版趟也同样从"蒙版自己的 base"读取。
    #[allow(clippy::too_many_arguments)]
    fn render_blurs(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        groups: u32,
        rows: u32,
        big: bool,
        small: bool,
        big_radius: f32,
        small_radius: f32,
        blur_h: &wgpu::BindGroup,
        blur_v: &wgpu::BindGroup,
        blur_h2: &wgpu::BindGroup,
        blur_v2: &wgpu::BindGroup,
    ) {
        // 半径 > 6 时需要宽抽头入口（1:1 / 全分辨率导出按分辨率放大半径）。
        let (big_h_pipeline, big_v_pipeline) = if big_radius > 6.0 {
            (&self.blur_wide_h_pipeline, &self.blur_wide_v_pipeline)
        } else {
            (&self.blur_h_pipeline, &self.blur_v_pipeline)
        };
        let (small_h_pipeline, small_v_pipeline) = if small_radius > 6.0 {
            (&self.blur_wide_h_pipeline, &self.blur_wide_v_pipeline)
        } else {
            (&self.blur_h_pipeline, &self.blur_v_pipeline)
        };
        if big {
            let radius = big_radius.clamp(1.0, 24.0);
            queue.write_buffer(
                &self.blur_params_buffer,
                0,
                &as_bytes(&[1.0, 0.0, radius, 0.0]),
            );
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fdxmp-blur-h-encoder"),
            });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fdxmp-blur-h"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(big_h_pipeline);
                pass.set_bind_group(0, blur_h, &[]);
                pass.dispatch_workgroups(groups, rows, 1);
            }
            queue.submit(Some(encoder.finish()));

            queue.write_buffer(
                &self.blur_params_buffer,
                0,
                &as_bytes(&[0.0, 1.0, radius, 0.0]),
            );
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fdxmp-blur-v-encoder"),
            });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fdxmp-blur-v"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(big_v_pipeline);
                pass.set_bind_group(0, blur_v, &[]);
                pass.dispatch_workgroups(groups, rows, 1);
            }
            queue.submit(Some(encoder.finish()));
        }

        if small {
            let radius = small_radius.clamp(0.5, 24.0);
            queue.write_buffer(
                &self.blur_params_buffer,
                0,
                &as_bytes(&[1.0, 0.0, radius, 0.0]),
            );
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fdxmp-detail-blur-h-encoder"),
            });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fdxmp-detail-blur-h"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(small_h_pipeline);
                pass.set_bind_group(0, blur_h2, &[]);
                pass.dispatch_workgroups(groups, rows, 1);
            }
            queue.submit(Some(encoder.finish()));

            queue.write_buffer(
                &self.blur_params_buffer,
                0,
                &as_bytes(&[0.0, 1.0, radius, 0.0]),
            );
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fdxmp-detail-blur-v-encoder"),
            });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("fdxmp-detail-blur-v"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(small_v_pipeline);
                pass.set_bind_group(0, blur_v2, &[]);
                pass.dispatch_workgroups(groups, rows, 1);
            }
            queue.submit(Some(encoder.finish()));
        }
    }

    /// 蒙版叠加：把选中蒙版按权重叠加到当前输出（颜色为显示空间 RGB，alpha 0..1）。
    ///
    /// 在 [`ColorPipeline::render_with`]（裁剪模式）之后调用；叠加层与几何变换一致，
    /// 裁剪 / 旋转后仍贴合画面内容。
    pub fn render_overlay(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &PipelineImage,
        params: &EditParams,
        mask_index: usize,
        style: OverlayStyle,
    ) {
        let Some(target) = self.target.as_ref() else {
            return;
        };
        let (width, height) = (target.width, target.height);
        let display_view = target.display_view.clone();
        let display_scratch_view = target.display_scratch_view.clone();
        let display_texture = target.display_texture.clone();
        let display_scratch_texture = target.display_scratch_texture.clone();
        let dummy_float_view = target.dummy_float_view.clone();

        let Some(mask) = params.masks.get(mask_index) else {
            return;
        };
        let mask_view = match self.ensure_mask_texture(device, queue, mask_index, mask, image) {
            Ok((view, _)) => view,
            Err(_) => return,
        };

        // 叠加层与当前渲染目标使用同一套几何映射（此时目标对应裁剪输出）。
        let geometry = Geometry::from_params(params);
        let crop_rect = params.crop.rect;
        let matrix = output_to_source_matrix(
            &geometry,
            crop_rect,
            image.width,
            image.height,
            width,
            height,
        );
        let mut packed = pack_params(params, width, height);
        let geometry_active = !geometry.is_identity() || crop_rect != [0.0, 0.0, 1.0, 1.0];
        pack_geometry(&mut packed, &matrix, geometry_active);
        packed[slot::DETAIL_BLUR] = if params_needs_detail_blur(params) {
            1.0
        } else {
            0.0
        };
        let mask_scale = image.mask_scale();
        packed[slot::MASK_SCALE_X] = mask_scale[0];
        packed[slot::MASK_SCALE_Y] = mask_scale[1];
        let mask_origin = image.mask_origin();
        packed[slot::MASK_ORIGIN_X] = mask_origin[0];
        packed[slot::MASK_ORIGIN_Y] = mask_origin[1];
        packed[slot::OVERLAY_ALPHA] = style.alpha.clamp(0.0, 1.0);
        packed[slot::OVERLAY_RGB] = style.color[0].clamp(0.0, 1.0);
        packed[slot::OVERLAY_RGB + 1] = style.color[1].clamp(0.0, 1.0);
        packed[slot::OVERLAY_RGB + 2] = style.color[2].clamp(0.0, 1.0);
        queue.write_buffer(&self.params_buffer, 0, &as_bytes(&packed));

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fdxmp-overlay"),
            layout: &self.overlay_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(
                        self.params_buffer.as_entire_buffer_binding(),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&dummy_float_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&dummy_float_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&mask_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&display_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&dummy_float_view),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&display_scratch_view),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fdxmp-overlay-encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fdxmp-overlay"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.overlay_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(div_ceil(width, 8), div_ceil(height, 8), 1);
        }
        // 叠加结果写在 8bit 暂存，再拷回显示纹理（同一趟不能同时读写一张纹理）。
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &display_scratch_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &display_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));
    }

    /// 确保渲染目标与当前源图 / 输出尺寸 / 几何矩阵一致（不一致时重建纹理与绑定组）。
    fn ensure_target(
        &mut self,
        device: &wgpu::Device,
        image: &PipelineImage,
        width: u32,
        height: u32,
        geometry_active: bool,
    ) {
        if let Some(target) = self.target.as_ref() {
            if target.source_id == image.id
                && target.width == width
                && target.height == height
                && target.geometry_active == geometry_active
            {
                return;
            }
        }
        let target = self.build_target(device, image, width, height, geometry_active);
        // 目标重建后旧的 bind group 引用的是已失效的纹理视图，必须清空。
        self.mask_bind_cache.clear();
        self.target = Some(target);
    }

    /// 重建渲染目标（纹理 + 绑定组）。
    fn build_target(
        &mut self,
        device: &wgpu::Device,
        image: &PipelineImage,
        width: u32,
        height: u32,
        geometry_active: bool,
    ) -> RenderTarget {
        let extent = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let make_texture =
            |label: &str, format: wgpu::TextureFormat, usage: wgpu::TextureUsages| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
        let float_usage =
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING;
        let u8_usage = float_usage | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST;

        let stage_texture = geometry_active
            .then(|| make_texture("fdxmp-stage", wgpu::TextureFormat::Rgba16Float, float_usage));
        let stage_view = match &stage_texture {
            Some(texture) => texture.create_view(&wgpu::TextureViewDescriptor::default()),
            None => image.source_view.clone(),
        };
        let base_texture =
            make_texture("fdxmp-base", wgpu::TextureFormat::Rgba16Float, float_usage);
        let blur_a_texture = make_texture(
            "fdxmp-blur-a",
            wgpu::TextureFormat::Rgba16Float,
            float_usage,
        );
        let blur_b_texture = make_texture(
            "fdxmp-blur-b",
            wgpu::TextureFormat::Rgba16Float,
            float_usage,
        );
        let blur_a2_texture = make_texture(
            "fdxmp-blur-a2",
            wgpu::TextureFormat::Rgba16Float,
            float_usage,
        );
        let blur_small_texture = make_texture(
            "fdxmp-blur-small",
            wgpu::TextureFormat::Rgba16Float,
            float_usage,
        );
        // 细节结果 ping-pong：线性 f16，保留 >1 高光余量。
        let work_a_texture = make_texture(
            "fdxmp-work-a",
            wgpu::TextureFormat::Rgba16Float,
            float_usage,
        );
        let work_b_texture = make_texture(
            "fdxmp-work-b",
            wgpu::TextureFormat::Rgba16Float,
            float_usage,
        );
        // 显示纹理：整条管线唯一的 8bit sRGB 量化（egui / 回读 / 叠加显示）。
        let display_texture =
            make_texture("fdxmp-display", wgpu::TextureFormat::Rgba8Unorm, u8_usage);
        let display_scratch_texture = make_texture(
            "fdxmp-display-scratch",
            wgpu::TextureFormat::Rgba8Unorm,
            u8_usage,
        );
        let dummy_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fdxmp-dummy"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let dummy_float_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fdxmp-dummy-float"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: float_usage,
            view_formats: &[],
        });
        let base_view = base_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let blur_a_view = blur_a_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let blur_b_view = blur_b_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let blur_a2_view = blur_a2_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let blur_small_view =
            blur_small_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let work_a_view = work_a_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let work_b_view = work_b_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let display_view = display_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let display_scratch_view =
            display_scratch_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let dummy_view = dummy_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let dummy_float_view =
            dummy_float_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let geometry_bind_group = geometry_active.then(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fdxmp-geometry"),
                layout: &self.geometry_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(
                            self.geometry_params_buffer.as_entire_buffer_binding(),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&image.source_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&stage_view),
                    },
                ],
            })
        });
        let blur_h_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fdxmp-blur-h"),
            layout: &self.blur_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(
                        self.blur_params_buffer.as_entire_buffer_binding(),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&base_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&blur_a_view),
                },
            ],
        });
        let blur_v_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fdxmp-blur-v"),
            layout: &self.blur_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(
                        self.blur_params_buffer.as_entire_buffer_binding(),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&blur_a_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&blur_b_view),
                },
            ],
        });
        let blur_h2_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fdxmp-blur-h2"),
            layout: &self.blur_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(
                        self.blur_params_buffer.as_entire_buffer_binding(),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&base_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&blur_a2_view),
                },
            ],
        });
        let blur_v2_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fdxmp-blur-v2"),
            layout: &self.blur_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(
                        self.blur_params_buffer.as_entire_buffer_binding(),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&blur_a2_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&blur_small_view),
                },
            ],
        });
        // 基础调色趟：stage → base（f16 线性）。
        let base_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fdxmp-base"),
            layout: &self.main_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(
                        self.params_buffer.as_entire_buffer_binding(),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&stage_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&dummy_float_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&base_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&dummy_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&dummy_float_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&dummy_float_view),
                },
            ],
        });
        let main_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fdxmp-main"),
            layout: &self.main_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(
                        self.params_buffer.as_entire_buffer_binding(),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&base_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&blur_b_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&work_a_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&dummy_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&dummy_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&blur_small_view),
                },
            ],
        });

        self.target_counter += 1;
        RenderTarget {
            source_id: image.id,
            width,
            height,
            geometry_active,
            generation: self.target_counter,
            stage_texture,
            stage_view,
            base_texture,
            base_view,
            blur_a_texture,
            blur_a_view,
            blur_b_texture,
            blur_b_view,
            blur_a2_texture,
            blur_a2_view,
            blur_small_texture,
            blur_small_view,
            work_a_texture,
            work_a_view,
            work_b_texture,
            work_b_view,
            display_texture,
            display_view,
            display_scratch_texture,
            display_scratch_view,
            dummy_texture,
            dummy_view,
            dummy_float_texture,
            dummy_float_view,
            geometry_bind_group,
            base_bind_group,
            blur_h_bind_group,
            blur_v_bind_group,
            blur_h2_bind_group,
            blur_v2_bind_group,
            main_bind_group,
        }
    }

    /// 当前渲染目标尺寸（`render` / `render_with` 之后有效）。
    pub fn target_size(&self) -> Option<(u32, u32)> {
        self.target
            .as_ref()
            .map(|target| (target.width, target.height))
    }

    /// 当前渲染目标的显示纹理视图（注册给 egui 用）。
    pub fn output_view(&self) -> Option<&wgpu::TextureView> {
        self.target.as_ref().map(|target| &target.display_view)
    }

    /// 当前渲染目标的显示纹理（零拷贝上屏 / 直方图用；未渲染时为 None）。
    pub fn display_texture(&self) -> Option<&wgpu::Texture> {
        self.target.as_ref().map(|target| &target.display_texture)
    }

    /// 渲染目标代数：源图 / 尺寸 / 几何状态变化时递增（用于刷新 egui 纹理注册）。
    pub fn target_generation(&self) -> u64 {
        self.target
            .as_ref()
            .map(|target| target.generation)
            .unwrap_or(0)
    }

    /// 调试：回读最近一次写入的几何 uniform（矩阵 + 尺寸）。
    pub fn debug_geometry_params(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Option<Vec<f32>> {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fdxmp-geometry-debug"),
            size: 80,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fdxmp-geometry-debug-encoder"),
        });
        encoder.copy_buffer_to_buffer(&self.geometry_params_buffer, 0, &buffer, 0, 80);
        queue.submit(Some(encoder.finish()));
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        let _ = device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        receiver.recv().ok()?.ok()?;
        let mapped = slice.get_mapped_range().ok()?;
        let mut values = Vec::with_capacity(20);
        for chunk in mapped.as_chunks::<4>().0 {
            values.push(f32::from_le_bytes(*chunk));
        }
        drop(mapped);
        buffer.unmap();
        Some(values)
    }

    /// 蒙版权重纹理（内容哈希缓存：参数没变就不重新生成）。
    fn ensure_mask_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        index: usize,
        mask: &MaskParams,
        image: &PipelineImage,
    ) -> Result<(wgpu::TextureView, u64)> {
        // 1:1 分块渲染用代理图生成蒙版（着色器按 mask_scale 换算坐标）。
        let reference = image
            .mask_reference
            .as_ref()
            .map(|reference| reference.pixels.as_ref())
            .unwrap_or(&image.source_pixels);
        let (width, height) = (reference.width().max(1), reference.height().max(1));
        let hash = mask_key(mask, width, height);
        let cached = self
            .mask_cache
            .get(index)
            .map(|entry| entry.hash == hash)
            .unwrap_or(false);
        if cached {
            return Ok((self.mask_cache[index].view.clone(), hash));
        }

        let raster = generate_mask(mask, reference, width, height)?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fdxmp-mask-weights"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut bytes = Vec::with_capacity(raster.weights.len() * 4);
        for weight in &raster.weights {
            bytes.extend_from_slice(&weight.to_le_bytes());
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let entry = MaskTexture {
            hash,
            texture,
            view,
        };
        if index < self.mask_cache.len() {
            self.mask_cache[index] = entry;
        } else {
            self.mask_cache.push(entry);
        }
        Ok((self.mask_cache[index].view.clone(), hash))
    }

    /// 回读输出纹理为 sRGB 编码的 RGBA8（测试 / CLI / 直方图用）。
    pub fn readback(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _image: &PipelineImage,
    ) -> Result<Vec<u8>> {
        let target = self
            .target
            .as_ref()
            .ok_or_else(|| FdxmpError::Gpu("尚未渲染，无法回读".to_string()))?;
        let (width, height) = (target.width, target.height);
        let bytes_per_row = width * 4; // rgba8unorm（sRGB 编码）
        let padded = bytes_per_row.div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fdxmp-readback"),
            size: (padded as u64) * (height as u64),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fdxmp-readback-encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.display_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        let _ = device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        receiver
            .recv()
            .map_err(|err| FdxmpError::Gpu(format!("回读等待失败：{err}")))?
            .map_err(|err| FdxmpError::Gpu(format!("回读映射失败：{err}")))?;

        let mapped = slice
            .get_mapped_range()
            .map_err(|err| FdxmpError::Gpu(format!("回读映射失败：{err}")))?;
        let mut rgba = Vec::with_capacity((bytes_per_row * height) as usize);
        for row in 0..height as usize {
            let row_start = row * padded as usize;
            rgba.extend_from_slice(&mapped[row_start..row_start + bytes_per_row as usize]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(rgba)
    }
}

/// 直方图桶数（R / G / B / 亮度各 256）。
pub const HISTOGRAM_BINS: usize = 1024;

/// 上屏拷贝通道：把显示纹理按 1:1 写进表面纹理（零拷贝上屏的最后一跳）。
pub struct PresentPass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

impl PresentPass {
    /// 创建拷贝通道（目标格式 = 显示表面格式，转换由 GPU 完成）。
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fdxmp-present"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/present.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fdxmp-present-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fdxmp-present-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fdxmp-present-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_present"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_present"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline, layout }
    }

    /// 把显示纹理按 1:1 拷贝到目标视图（同一命令编码器内）。
    pub fn draw(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        target: &wgpu::TextureView,
    ) {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fdxmp-present-bind"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fdxmp-present-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// 显示纹理直方图：compute pass 统计 + 回读 4KB（4×256 小端 u32）。
pub struct HistogramPass {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    bins: wgpu::Buffer,
    staging: wgpu::Buffer,
}

impl HistogramPass {
    /// 创建直方图通道（管线 + 计数缓冲 + 回读缓冲）。
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fdxmp-histogram"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/histogram.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fdxmp-histogram-layout"),
            entries: &[
                texture_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fdxmp-histogram-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("fdxmp-histogram-pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("cs_histogram"),
            compilation_options: Default::default(),
            cache: None,
        });
        let size = (HISTOGRAM_BINS * 4) as u64;
        let bins = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fdxmp-histogram-bins"),
            size,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fdxmp-histogram-staging"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            bins,
            staging,
        }
    }

    /// 统计一帧显示纹理的直方图（需先渲染出显示纹理）。
    pub fn compute(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<[u32; HISTOGRAM_BINS]> {
        if width == 0 || height == 0 {
            return Err(FdxmpError::Gpu("直方图尺寸为 0".to_string()));
        }
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fdxmp-histogram-bind"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.bins.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fdxmp-histogram-encoder"),
        });
        encoder.clear_buffer(&self.bins, 0, None);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fdxmp-histogram-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
        encoder.copy_buffer_to_buffer(&self.bins, 0, &self.staging, 0, (HISTOGRAM_BINS * 4) as u64);
        queue.submit(Some(encoder.finish()));

        let slice = self.staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        let _ = device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        receiver
            .recv()
            .map_err(|err| FdxmpError::Gpu(format!("直方图等待失败：{err}")))?
            .map_err(|err| FdxmpError::Gpu(format!("直方图映射失败：{err}")))?;
        let mapped = slice
            .get_mapped_range()
            .map_err(|err| FdxmpError::Gpu(format!("直方图映射失败：{err}")))?;
        let mut out = [0u32; HISTOGRAM_BINS];
        for (index, slot) in out.iter_mut().enumerate() {
            let offset = index * 4;
            *slot = u32::from_le_bytes([
                mapped[offset],
                mapped[offset + 1],
                mapped[offset + 2],
                mapped[offset + 3],
            ]);
        }
        drop(mapped);
        self.staging.unmap();
        Ok(out)
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn storage_entry(binding: u32, format: wgpu::TextureFormat) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        count: None,
    }
}

fn create_compute_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::BindGroupLayout,
    entry_point: &str,
) -> wgpu::ComputePipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fdxmp-pipeline-layout"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry_point),
        layout: Some(&pipeline_layout),
        module: shader,
        entry_point: Some(entry_point),
        compilation_options: Default::default(),
        cache: None,
    })
}

fn div_ceil(value: u32, divisor: u32) -> u32 {
    value.div_ceil(divisor)
}

/// 是否需要小半径细节模糊（锐化或亮度降噪启用；蒙版独立参数一并考虑）。
/// 是否需要小半径模糊参考（锐化 / 亮度降噪的"细节"参数用）。
fn needs_small_blur(params: &EditParams) -> bool {
    // 纹理也依赖细半径参考（微反差）；锐化 / 亮度降噪同样。
    params.effects.texture.abs() > 0.0
        || params.detail.sharpen.amount > 0.0
        || params.detail.noise.luminance > 0.0
}

/// 是否需要大半径模糊参考（纹理 / 清晰度 / 锐化 / 亮度降噪 / 颜色降噪）。
fn needs_big_blur(params: &EditParams) -> bool {
    params.effects.texture.abs() > 0.0
        || params.effects.clarity.abs() > 0.0
        || params.detail.sharpen.amount > 0.0
        || params.detail.noise.luminance > 0.0
        || params.detail.noise.color > 0.0
        // 高光 / 阴影的权重取自大模糊基底层（色调段的一部分）。
        || params.light.highlights.abs() > 0.0
        || params.light.shadows.abs() > 0.0
}

/// 全局 + 任一蒙版是否需要小半径模糊（叠加显示打包 `DETAIL_BLUR` 用）。
fn params_needs_detail_blur(params: &EditParams) -> bool {
    needs_small_blur(params)
        || params
            .masks
            .iter()
            .any(|mask| needs_small_blur(&mask.adjustments))
}

/// 蒙版趟 bind group 的缓存键（FNV-1a 混合：目标代数 / 蒙版序号 / 乒乓相位 / 栅格哈希）。
fn mask_bind_key(generation: u64, index: usize, current_is_a: bool, mask_hash: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for value in [generation, index as u64, u64::from(current_is_a), mask_hash] {
        for byte in value.to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// 细节模糊的分辨率缩放系数：以代理长边（[`BLUR_REFERENCE_EDGE`]）为参考。
///
/// 1:1 分块渲染时按整图输出尺寸（`plan.full_size`）计算，而不是分块自身尺寸，
/// 这样 1:1 视图与全分辨率导出的细节强度一致；上限 [`BLUR_SCALE_MAX`]
/// 由宽抽头模糊窗口（±24）决定。
fn detail_blur_scale(plan: &RenderPlan) -> f32 {
    let reference = if plan.full_size[0] > 0.5 {
        plan.full_size[0].max(plan.full_size[1])
    } else {
        plan.width.max(plan.height) as f32
    };
    (reference / BLUR_REFERENCE_EDGE).clamp(1.0, BLUR_SCALE_MAX)
}

/// f32 切片 → little-endian 字节（uniform 上传用，避免 unsafe）。
fn as_bytes(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// 线性 0..1 → sRGB 8bit。
pub fn linear_to_srgb_u8(linear: f32) -> u8 {
    let srgb = if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (srgb.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// 半精度浮点 → f32（回读 rgba16float 用）。
pub fn f16_to_f32(bits: u16) -> f32 {
    let sign = (bits >> 15) & 1;
    let exponent = (bits >> 10) & 0x1f;
    let mantissa = bits & 0x3ff;
    let value = if exponent == 0 {
        (mantissa as f32) * 2f32.powi(-24)
    } else if exponent == 0x1f {
        if mantissa == 0 {
            f32::INFINITY
        } else {
            f32::NAN
        }
    } else {
        (1.0 + mantissa as f32 / 1024.0) * 2f32.powi(exponent as i32 - 15)
    };
    if sign == 1 {
        -value
    } else {
        value
    }
}

/// f32 → 半精度浮点（上传线性源纹理用；就近舍入，支持 0..1 与常见溢出）。
pub fn f32_to_f16(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x007f_ffff;
    if value.is_nan() {
        return sign | 0x7e00;
    }
    if exponent >= 0x1f {
        // 溢出为无穷（f16 上限 65504）
        return sign | 0x7c00;
    }
    if exponent <= 0 {
        if exponent < -10 {
            return sign;
        }
        // 次正规数：隐含位补上后右移
        let mantissa = mantissa | 0x0080_0000;
        let shift = (14 - exponent) as u32;
        let rounded = (mantissa + (1 << (shift - 1))) >> shift;
        return sign | rounded as u16;
    }
    let rounded = mantissa + 0x0000_1000; // 13 位尾数就近舍入
    if rounded & 0x0080_0000 != 0 {
        return sign | (((exponent + 1) as u16) << 10);
    }
    sign | (((exponent as u16) << 10) | ((rounded >> 13) as u16))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{ColorBand, CurvePoint, GradingZone};

    /// 用真实设备编译整条管线：WGSL 语法 / uniform 布局 / 绑定组不匹配都会在这里炸。
    ///
    /// 没有可用适配器时直接跳过（CI 无 GPU 也能绿）。
    #[test]
    #[cfg(feature = "gpu")]
    fn gpu_pipeline_builds_with_equalizer_uniforms() {        let Ok(context) = crate::gpu::GpuContext::new_blocking() else {
            return;
        };
        let _pipeline = ColorPipeline::new(&context.device);

        let mut params = EditParams::default();
        params.equalizer.enabled = true;
        params.equalizer.hue[2] = -30.0;
        let packed = pack_params(&params, 64, 64);
        assert_eq!(
            packed.len(),
            PARAM_FLOATS + CURVE_ENTRIES * 4 + TONE_LUT_FLOATS + EQ_FLOATS + HSL_FLOATS
        );
        // 均衡器开关 + 第一张 LUT 的第一个点。
        let eq_base = PARAM_FLOATS + CURVE_ENTRIES * 4 + TONE_LUT_FLOATS;
        assert_eq!(packed[eq_base], 1.0);
        assert!(packed[eq_base + 8].is_finite());
    }

    #[test]
    fn identity_curve_is_identity() {
        let curve = ToneCurve::default();
        let lut = build_curve_lut(&curve);
        assert!((lut[0] - 0.0).abs() < 1e-4);
        assert!((lut[255] - 1.0).abs() < 1e-4);
        assert!((lut[128] - 128.0 / 255.0).abs() < 0.01);
    }

    #[test]
    fn curve_control_points_are_respected() {
        let curve = ToneCurve {
            points: vec![
                CurvePoint { x: 0.0, y: 0.0 },
                CurvePoint { x: 0.5, y: 0.75 },
                CurvePoint { x: 1.0, y: 1.0 },
            ],
        };
        let lut = build_curve_lut(&curve);
        assert!((lut[128] - 0.75).abs() < 0.02, "中点应被抬到 ~0.75");
    }

    #[test]
    fn pack_params_maps_sliders_and_lut() {
        let mut params = EditParams::default();
        params.light.exposure = 1.25;
        params.color.saturation = -30.0;
        params.hsl.band_mut(ColorBand::Blue).hue = 40.0;
        params.grading.shadows = GradingZone {
            hue: 200.0,
            saturation: 30.0,
            luminance: 0.0,
        };

        let packed = pack_params(&params, 800, 600);
        assert_eq!(
            packed.len(),
            PARAM_FLOATS + CURVE_ENTRIES * 4 + TONE_LUT_FLOATS + EQ_FLOATS + HSL_FLOATS
        );
        assert_eq!(packed[slot::EXPOSURE], 1.25);
        assert_eq!(packed[slot::SATURATION], -30.0);
        assert_eq!(packed[slot::HSL_START + 5 * 3], 40.0);
        assert_eq!(packed[slot::GRADING_START], 200.0);
        assert_eq!(packed[slot::WIDTH], 800.0);
        assert_eq!(packed[slot::MASK_MODE], 0.0);
        // 色调段：中性参数下整段关闭；4 张参考 LUT 仍是恒等曲线（0 → 0，1 → 1）。
        assert_eq!(packed[slot::TONE_ACTIVE], 0.0);
        let tone_base = PARAM_FLOATS + CURVE_ENTRIES * 4;
        assert!(packed[tone_base] < 0.001);
        assert!((packed[tone_base + 255] - 1.0).abs() < 0.01);
    }

    #[test]
    fn f16_conversion_matches_known_values() {
        assert!((f16_to_f32(0x3c00) - 1.0).abs() < 1e-6);
        assert!((f16_to_f32(0x0000) - 0.0).abs() < 1e-6);
        assert!((f16_to_f32(0x3800) - 0.5).abs() < 1e-6);
        assert!((f16_to_f32(0xc000) + 2.0).abs() < 1e-6);
    }

    #[test]
    fn f32_to_f16_roundtrips_typical_values() {
        for value in [0.0f32, 0.001, 0.04, 0.25, 0.5, 0.9, 1.0, 2.0, 100.0] {
            let back = f16_to_f32(f32_to_f16(value));
            let tolerance = value.abs() * 1e-3 + 1e-6;
            assert!(
                (back - value).abs() <= tolerance,
                "{value} → {back}（超出 f16 半步精度）"
            );
        }
    }

    #[test]
    fn srgb_encoding_endpoints() {
        assert_eq!(linear_to_srgb_u8(0.0), 0);
        assert_eq!(linear_to_srgb_u8(1.0), 255);
        let mid = linear_to_srgb_u8(0.2140);
        assert!((mid as i32 - 128).abs() <= 2);
    }

    #[test]
    fn mask_hash_changes_with_params() {
        let mask = MaskParams {
            id: "m".to_string(),
            ..MaskParams::default()
        };
        let first = mask_key(&mask, 100, 100);
        assert_eq!(first, mask_key(&mask, 100, 100));
        assert_ne!(first, mask_key(&mask, 200, 100));
    }

    #[test]
    fn detail_blur_scale_grows_with_output_and_caps() {
        let plan = |width: u32, height: u32, full: [f32; 2]| RenderPlan {
            matrix: crate::geometry::IDENTITY,
            width,
            height,
            geometry_active: false,
            tile_origin: [0.0, 0.0],
            full_size: full,
        };
        // 代理尺寸：保持 1×
        assert!((detail_blur_scale(&plan(2048, 1365, [0.0, 0.0])) - 1.0).abs() < 1e-6);
        // 小图不缩小（下限 1）
        assert!((detail_blur_scale(&plan(500, 300, [0.0, 0.0])) - 1.0).abs() < 1e-6);
        // 1:1 分块按整图尺寸算：8192 长边 → 4×
        assert!((detail_blur_scale(&plan(2048, 1365, [8192.0, 5461.0])) - 4.0).abs() < 1e-6);
        // 超过上限则封顶
        assert!(
            (detail_blur_scale(&plan(256, 256, [16384.0, 16384.0])) - BLUR_SCALE_MAX).abs() < 1e-6
        );
    }

    /// GPU 颜色均衡器 vs CPU 同参数：逐像素比对（8bit sRGB），用于守住 WGSL 与 Rust 的一致性。
    ///
    /// `use_filter = false` 时两侧算的是同一套逐像素公式，差异只来自 f16 存储 + 8bit 量化；
    /// 开滤波时 GPU 用 5x5 单遍近似，容差放宽到 3 级。
    #[test]
    #[cfg(feature = "gpu")]
    #[ignore = "需要真实 GPU；手动 cargo test --features gpu -- --ignored"]
    fn gpu_equalizer_matches_cpu() {
        let Ok(context) = crate::gpu::GpuContext::new_blocking() else {
            return;
        };
        let (w, h) = (24u32, 24u32);
        let mut rgba = vec![0.0f32; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let hue = x as f32 / w as f32;
                let rgb = fdxmp_color::perceptual::hsv_to_rgb([hue, 0.55, 0.62]);
                let k = ((y * w + x) * 4) as usize;
                for c in 0..3 {
                    rgba[k + c] = fdxmp_color::transfer::srgb_to_linear(rgb[c]);
                }
                rgba[k + 3] = 1.0;
            }
        }

        for use_filter in [false, true] {
            let mut params = EditParams::default();
            params.equalizer.enabled = true;
            params.equalizer.use_filter = use_filter;
            params.equalizer.hue[2] = -30.0;
            params.equalizer.saturation[0] = 1.35;
            params.equalizer.brightness[6] = 1.08;

            let mut pipeline = ColorPipeline::new(&context.device);
            let image = pipeline
                .upload_linear_f32(&context.device, &context.queue, &rgba, w, h)
                .expect("upload");
            pipeline.render(&context.device, &context.queue, &image, &params);
            let bytes = pipeline
                .readback(&context.device, &context.queue, &image)
                .expect("readback");

            let stage = crate::color_engine::EqualizerStage::from_params(&params);
            let mut flat: Vec<f32> = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect();
            stage.apply_linear(&mut flat, w as usize, h as usize);

            let mut max_diff = 0i32;
            let mut sum = 0i64;
            for index in 0..(w * h) as usize {
                for c in 0..3 {
                    let cpu = linear_to_srgb_u8(flat[index * 3 + c]) as i32;
                    let gpu = bytes[index * 4 + c] as i32;
                    let diff = (cpu - gpu).abs();
                    max_diff = max_diff.max(diff);
                    sum += diff as i64;
                    if !use_filter {
                        assert!(
                            diff <= 2,
                            "pixel {index} ch {c}: cpu={cpu} gpu={gpu}"
                        );
                    }
                }
            }
            eprintln!(
                "GPU vs CPU (use_filter={use_filter}): mean {:.3} max {max_diff}",
                sum as f64 / (w * h * 3) as f64
            );
        }

        // HSL 面板：蓝色带 -100 饱和度，GPU 与 CPU 必须一致（否则就是 HSL 段没生效）。
        {
            let mut params = EditParams::default();
            params.hsl.blue.saturation = -100.0;
            let mut pipeline = ColorPipeline::new(&context.device);
            let image = pipeline
                .upload_linear_f32(&context.device, &context.queue, &rgba, w, h)
                .expect("upload");
            pipeline.render(&context.device, &context.queue, &image, &params);
            let bytes = pipeline
                .readback(&context.device, &context.queue, &image)
                .expect("readback");

            let stage = crate::color_engine::ColorStage::from_params(&params);
            let mut max_diff = 0i32;
            let mut sum = 0i64;
            for index in 0..(w * h) as usize {
                let src = &rgba[index * 4..index * 4 + 3];
                let cpu_rgb = stage.apply([src[0], src[1], src[2]]);
                for c in 0..3 {
                    let cpu = linear_to_srgb_u8(cpu_rgb[c]) as i32;
                    let gpu = bytes[index * 4 + c] as i32;
                    let diff = (cpu - gpu).abs();
                    max_diff = max_diff.max(diff);
                    sum += diff as i64;
                }
            }
            eprintln!(
                "HSL GPU vs CPU: mean {:.3} max {max_diff}",
                sum as f64 / (w * h * 3) as f64
            );
            assert!(max_diff <= 3, "HSL GPU 与 CPU 不一致：max {max_diff}");
        }
    }
}
