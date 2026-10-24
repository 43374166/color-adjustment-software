// fdxmp 色彩管线（阶段 2/3/8）
//
// 入口：
//   cs_base  —— 基础调色（白平衡 → 饱和度），写入线性 f16 中间纹理
//   cs_main  —— 细节链路（纹理 / 清晰度 / 去雾 / 锐化 / 降噪 / 暗角 / 颗粒，
//               模糊参考取自基础调色结果）；当 p(68) > 0.5 时按蒙版权重混入当前图像
//   cs_encode—— 线性 f16 → sRGB 8bit（唯一的量化 / 裁剪点，供 egui 与回读）
//   cs_overlay —— 蒙版叠加显示（sRGB 8bit 域）
//   cs_copy  —— 图像拷贝（蒙版趟数为奇数时把结果搬回 A）
//
// 颜色空间约定：
//   源纹理：rgba8unorm-srgb（读取即线性）
//   中间结果 / 模糊 / 输出：rgba16float（线性、不裁剪，保留 >1 的高光余量）
//   显示纹理：rgba8unorm（sRGB 编码，egui 用户纹理约定）
//   曲线 LUT：在 gamma 空间应用后转回线性
//
// 参数布局（float 下标，与 Rust 侧 gpu_pipeline::slot 对齐）：
//   0 曝光 1 对比度 2 高光 3 阴影 4 白色 5 黑色 6 色温 7 色调
//   8 自然饱和度 9 饱和度 10 纹理 11 清晰度 12 去雾 13 暗角 14 颗粒
//   15 锐化数量 16 锐化半径 17 锐化细节 18 锐化蒙版
//   19 降噪亮度 20 降噪亮度细节 21 降噪亮度对比度 22 降噪颜色 23 降噪颜色细节
//   24 降噪颜色平滑度 25 分级混合 26 分级平衡
//   27 参数曲线高光 28 参数曲线中间调 29 参数曲线阴影 30 高光分界 31 中间调分界
//   32..55 HSL 8 段 × (色相, 饱和度, 明亮度)
//   56..64 颜色分级 阴影/中间调/高光 × (色相, 饱和度, 明亮度)
//   65 颗粒种子 66 宽度 67 高度 68 蒙版模式（0=全局，1=局部混合）
//   69..77 几何逆矩阵（行主序：输出像素 → 源图像素）
//   78 几何是否启用（>0.5 时蒙版权重按逆映射采样到源图坐标）
//   79 细节模糊是否有效（>0.5 时锐化 / 降噪使用小半径模糊；见 cs_main 绑定 6）
//   80 蒙版叠加不透明度（0 = 关闭） 81..83 叠加颜色（线性 RGB）
//   84..85 蒙版纹理相对源图的比例（1:1 分块渲染时源图全分辨率、蒙版按代理分辨率）
//   86..87 分块原点（整图输出坐标） 97..98 整图输出尺寸（分块时暗角 / 颗粒用整图坐标）
//   88..96 白平衡色适应矩阵（行主序；CPU 侧由色温 / 色调算好）
//   100..101 蒙版参考图相对整图源像素的原点（分块 / 裁剪时把源坐标加回整图）
//   104 颗粒大小 105 颗粒粗糙度 106 暗角羽化 107 暗角中点

struct Params {
    // 49 个 vec4（196 floats）：0..107 基础 / 细节 / 曲线 / HSL / 分级，
    // 108..195 调色段（色带中心 / 黑白 / 校准 / 点颜色 / 分级向量），见 Rust `slot`。
    v: array<vec4<f32>, 49>,
    curve: array<vec4<f32>, 256>,
    // 电影曲线 LUT：4096 项，覆盖中灰 -14..+10 EV（与 `fdxmp_color::tone::LUT_N` 一致）。
    tone: array<vec4<f32>, 1024>,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var blur_tex: texture_2d<f32>;
@group(0) @binding(3) var dst_tex: texture_storage_2d<rgba16float, write>;
@group(0) @binding(4) var mask_tex: texture_2d<f32>;
@group(0) @binding(5) var current_tex: texture_2d<f32>;
// cs_main：小半径细节模糊结果；cs_copy：拷贝源（两个入口复用同一绑定）。
@group(0) @binding(6) var aux_tex: texture_2d<f32>;
// cs_overlay / cs_encode 专用：8bit sRGB 显示纹理（egui 用户纹理 / 回读来源）。
@group(0) @binding(7) var display_tex: texture_storage_2d<rgba8unorm, write>;

fn p(index: u32) -> f32 {
    return params.v[index / 4u][index % 4u];
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn linear_to_gamma(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return pow(x, vec3<f32>(1.0 / 2.2));
}

fn gamma_to_linear(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return pow(x, vec3<f32>(2.2));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    let lower = x * 12.92;
    let higher = 1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(higher, lower, x <= vec3<f32>(0.0031308));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    let lower = x / 12.92;
    let higher = pow((x + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4));
    return select(higher, lower, x <= vec3<f32>(0.04045));
}

fn curve_lookup(t: f32, channel: u32) -> f32 {
    // 线性插值查表（旧实现取最近邻，256 级台阶在渐变处可见）
    let scaled = clamp(t, 0.0, 1.0) * 255.0;
    let index = u32(floor(scaled));
    let next = min(index + 1u, 255u);
    let fraction = scaled - f32(index);
    return mix(params.curve[index][channel], params.curve[next][channel], fraction);
}

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
    let max_c = max(max(c.r, c.g), c.b);
    let min_c = min(min(c.r, c.g), c.b);
    let delta = max_c - min_c;
    var hue = 0.0;
    if (delta > 0.00001) {
        if (max_c == c.r) {
            hue = (c.g - c.b) / delta;
        } else if (max_c == c.g) {
            hue = 2.0 + (c.b - c.r) / delta;
        } else {
            hue = 4.0 + (c.r - c.g) / delta;
        }
        hue = fract(hue / 6.0);
    }
    let sat = select(0.0, delta / max(max_c, 0.00001), max_c > 0.0);
    return vec3<f32>(hue, sat, max_c);
}

fn hsv_to_rgb(hsv: vec3<f32>) -> vec3<f32> {
    let h = fract(hsv.x);
    let s = clamp(hsv.y, 0.0, 1.0);
    let v = max(hsv.z, 0.0);
    let i = floor(h * 6.0);
    let f = h * 6.0 - i;
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let p1 = v * (1.0 - s);
    let sector = i32(i) % 6;
    if (sector == 0) { return vec3<f32>(v, t, p1); }
    if (sector == 1) { return vec3<f32>(q, v, p1); }
    if (sector == 2) { return vec3<f32>(p1, v, t); }
    if (sector == 3) { return vec3<f32>(p1, q, v); }
    if (sector == 4) { return vec3<f32>(t, p1, v); }
    return vec3<f32>(v, p1, q);
}

// ---- OkLab / OkLCh：与 `fdxmp-color::perceptual` 同一组公开矩阵 ----

fn oklab_from_linear(c: vec3<f32>) -> vec3<f32> {
    let lms = vec3<f32>(
        dot(c, vec3<f32>(0.4122214708, 0.5363325363, 0.0514459929)),
        dot(c, vec3<f32>(0.2119034982, 0.6806995451, 0.1073969566)),
        dot(c, vec3<f32>(0.0883024619, 0.2817188376, 0.6299787005))
    );
    let root = sign(lms) * pow(abs(lms), vec3<f32>(1.0 / 3.0));
    return vec3<f32>(
        dot(root, vec3<f32>(0.2104542553, 0.7936177850, -0.0040720468)),
        dot(root, vec3<f32>(1.9779984951, -2.4285922050, 0.4505937099)),
        dot(root, vec3<f32>(0.0259040371, 0.7827717662, -0.8086757660))
    );
}

fn linear_from_oklab(lab: vec3<f32>) -> vec3<f32> {
    let l = vec3<f32>(
        dot(lab, vec3<f32>(1.0, 0.3963377774, 0.2158037573)),
        dot(lab, vec3<f32>(1.0, -0.1055613458, -0.0638541728)),
        dot(lab, vec3<f32>(1.0, -0.0894841775, -1.2914855480))
    );
    let cube = l * l * l;
    return vec3<f32>(
        dot(cube, vec3<f32>(4.0767416621, -3.3077115913, 0.2309699292)),
        dot(cube, vec3<f32>(-1.2684380046, 2.6097574011, -0.3413193965)),
        dot(cube, vec3<f32>(-0.0041960863, -0.7034186147, 1.7076147010))
    );
}

/// OkLab → (L, C, h)：色相弧度，与 CPU `lab_to_lch` 一致。
fn oklab_to_lch(lab: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(lab.x, length(lab.yz), atan2(lab.z, lab.y));
}

fn lch_to_oklab(lch: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(lch.x, lch.y * cos(lch.z), lch.y * sin(lch.z));
}

/// 归一化到 (-π, π]。
fn wrap_pi(a: f32) -> f32 {
    return a - 6.283185307179586 * floor(a / 6.283185307179586 + 0.5);
}

/// 归一化到 [0, 2π)。
fn wrap_tau(a: f32) -> f32 {
    return a - 6.283185307179586 * floor(a / 6.283185307179586);
}

/// 调色段：8 色带混色 → 点颜色 → 自然饱和度 / 饱和度 → 黑白 → 颜色分级。
///
/// 与 CPU `fdxmp-color::ops::ColorOps::apply` / `color_engine::ColorStage` 同序同公式；
/// 色带中心与分级色轮向量在 CPU 侧换算成 OkLCh 后经 uniform 传入（108..195）。
fn pipeline_colour_ops(input_color: vec3<f32>) -> vec3<f32> {
    let lch = oklab_to_lch(oklab_from_linear(input_color));
    var l = lch.x;
    var chroma = lch.y;
    var hue = lch.z;

    // 8 色带：相邻两带的升余弦分割（权重和恒为 1），同时累加黑白混色权重。
    var hue_shift = 0.0;
    var sat_scale = 0.0;
    var lum_shift = 0.0;
    var bw_mix = 0.0;
    for (var i = 0u; i < 8u; i = i + 1u) {
        let next = (i + 1u) % 8u;
        let a = p(108u + i);
        let b = p(108u + next);
        let span = wrap_tau(b - a);
        let d = wrap_tau(hue - a);
        if (d <= span) {
            let t = select(0.0, d / max(span, 1e-6), span > 1e-6);
            let s = 0.5 - 0.5 * cos(t * 3.14159265358979);
            let w0 = 1.0 - s;
            let w1 = s;
            hue_shift = hue_shift
                + (w0 * p(32u + i * 3u) + w1 * p(32u + next * 3u)) / 100.0 * 0.5;
            sat_scale = sat_scale
                + (w0 * p(33u + i * 3u) + w1 * p(33u + next * 3u)) / 100.0;
            lum_shift = lum_shift
                + (w0 * p(34u + i * 3u) + w1 * p(34u + next * 3u)) / 100.0 * 0.18;
            bw_mix = bw_mix + (w0 * p(117u + i) + w1 * p(117u + next)) / 100.0;
            break;
        }
    }
    if (abs(hue_shift) + abs(sat_scale) + abs(lum_shift) > 0.0001) {
        let chroma_weight = min(chroma / 0.12, 1.0);
        hue = hue + hue_shift * chroma_weight;
        chroma = chroma * max(1.0 + sat_scale, 0.0);
        // 明亮度正向提升时轻微去饱和（与 CPU `ColorOps` 同公式）。
        let dl_pos = max(lum_shift, 0.0);
        if (dl_pos > 0.0) {
            chroma = chroma * max(1.0 - 1.2 * dl_pos, 0.0);
        }
        l = l + lum_shift * chroma_weight * sqrt(max(l, 0.05));
    }

    // 点颜色：软范围（色相 / 彩度 / 明度）内的加权调整，最多 4 个样本。
    let point_count = u32(clamp(p(136), 0.0, 4.0) + 0.5);
    for (var i = 0u; i < point_count; i = i + 1u) {
        let base = 137u + i * 10u;
        let pl = p(base);
        let pc = p(base + 1u);
        let ph = p(base + 2u);
        let pdh = p(base + 3u);
        let psat = p(base + 4u);
        let pdl = p(base + 5u);
        let pvar = p(base + 6u);
        let pwh = p(base + 7u);
        let pwc = p(base + 8u);
        let pwl = p(base + 9u);
        var weight = 1.0;
        if (pc >= 0.02) {
            weight = (1.0 - smoothstep(0.5 * pwh, pwh, abs(wrap_pi(hue - ph))))
                * smoothstep(0.005, 0.025, chroma);
        }
        if (weight > 0.0) {
            weight = weight * (1.0 - smoothstep(0.5 * pwc, pwc, abs(chroma - pc)));
            weight = weight * (1.0 - smoothstep(0.5 * pwl, pwl, abs(l - pl)));
        }
        if (weight > 0.0) {
            hue = hue + weight * (pvar * wrap_pi(hue - ph) + pdh);
            chroma = chroma + weight * pvar * (chroma - pc);
            chroma = max(chroma * (1.0 + weight * psat), 0.0);
            l = l + weight * (pvar * (l - pl) + pdl);
        }
    }

    // 自然饱和度（低饱和加权 + 肤色保护）/ 饱和度。
    let vibrance = p(8) / 100.0;
    if (vibrance != 0.0) {
        let low = 1.0 - clamp(chroma / 0.22, 0.0, 1.0);
        var skin = 1.0;
        if (vibrance > 0.0) {
            let delta = wrap_pi(hue - p(191)) / 0.35;
            skin = 1.0 - 0.6 * exp(-delta * delta);
        }
        chroma = chroma * max(1.0 + vibrance * low * low * skin * 1.2, 0.0);
    }
    let saturation = p(9) / 100.0;
    if (saturation != 0.0) {
        chroma = chroma * max(1.0 + saturation, 0.0);
    }

    // 黑白：按 8 色带权重把彩度转成明度，输出单色（之后的分级即为"分离色调"）。
    if (p(116) > 0.5) {
        l = max(l + bw_mix * min(chroma / 0.2, 1.0) * 0.25, 0.0);
        chroma = 0.0;
    }

    // 颜色分级：OkLab 三分区（阴影 / 中间调 / 高光）+ 全局，色轮是 a / b 向量。
    var lab = lch_to_oklab(vec3<f32>(l, chroma, hue));
    let gm = p(189);
    let gw = p(190);
    let w_shadow = 1.0 - smoothstep(gm - gw, gm + gw * 0.25, lab.x);
    let w_high = smoothstep(gm - gw * 0.25, gm + gw, lab.x);
    let w_mid = max(1.0 - w_shadow - w_high, 0.0);
    let weights = vec4<f32>(w_shadow, w_mid, w_high, 1.0);
    for (var zone = 0u; zone < 4u; zone = zone + 1u) {
        let wt = weights[zone];
        lab.y = lab.y + p(177u + zone * 3u) * wt;
        lab.z = lab.z + p(178u + zone * 3u) * wt;
        lab.x = lab.x + p(179u + zone * 3u) * wt;
    }
    return linear_from_oklab(lab);
}

fn hash_noise(seed: vec2<f32>, salt: f32) -> f32 {
    let value = sin(dot(seed, vec2<f32>(12.9898, 78.233)) + salt) * 43758.5453;
    return fract(value);
}

/// 值噪声：格子哈希 + 平滑双线性插值（用于可调大小的颗粒）。
fn value_noise(coord: vec2<f32>, salt: f32) -> f32 {
    let base = floor(coord);
    let frac = coord - base;
    let a = hash_noise(base, salt);
    let b = hash_noise(base + vec2<f32>(1.0, 0.0), salt);
    let d = hash_noise(base + vec2<f32>(0.0, 1.0), salt);
    let e = hash_noise(base + vec2<f32>(1.0, 1.0), salt);
    let u = frac * frac * (3.0 - 2.0 * frac);
    return mix(mix(a, b, u.x), mix(d, e, u.x), u.y);
}

/// 基础调色链路：白平衡 → 饱和度（线性域，不裁剪，保留 >1 高光余量）。
fn pipeline_base(input_color: vec3<f32>) -> vec3<f32> {
    var c = input_color;

    // 1) 白平衡：Bradford 色适应矩阵（行主序写在 88..96，CPU 侧按开尔文 / 色调算好）。
    //    0 / 0 时为恒等；温度 > 0 变暖、色调 > 0 偏洋红，与 Lightroom 方向一致。
    c = vec3<f32>(
        p(88) * c.r + p(89) * c.g + p(90) * c.b,
        p(91) * c.r + p(92) * c.g + p(93) * c.b,
        p(94) * c.r + p(95) * c.g + p(96) * c.b
    );

    // 1b) 相机校准（场景线性，白平衡之后、曝光之前）：基色矩阵 + 阴影色调。
    //     与 CPU `fdxmp-color::calibrate` 同公式（矩阵在 CPU 侧算好，126..134）。
    if (p(125) > 0.5) {
        c = vec3<f32>(
            p(126) * c.r + p(127) * c.g + p(128) * c.b,
            p(129) * c.r + p(130) * c.g + p(131) * c.b,
            p(132) * c.r + p(133) * c.g + p(134) * c.b
        );
        c = max(c, vec3<f32>(0.0));
        let calib_tint = p(135);
        if (abs(calib_tint) > 0.0001) {
            let y0 = luminance(c);
            let shadow_weight = 1.0 - smoothstep(-5.0, -0.5, log2(max(y0, 1e-7) / 0.18));
            c.g = max(c.g * (1.0 - 0.3 * calib_tint * shadow_weight), 0.0);
            let y1 = max(luminance(c), 1e-9);
            c = c * (y0 / y1);
        }
    }

    // 2) 曝光
    c = c * pow(2.0, p(0));

    return c;
}

// ---- 色调段：局部基底层 → 电影曲线 → 色域映射（与 CPU `tone::ToneStage` 同公式）----

/// 电影曲线 LUT 的定义域（相对中灰的场景 EV），与 `fdxmp_color::tone` 一致。
const TONE_MIN_EV: f32 = -14.0;
const TONE_MAX_EV: f32 = 10.0;
const TONE_N: u32 = 4096u;

/// 场景亮度 → 显示亮度（LUT 线性插值；低于下限时线性外推，避免黑场断层）。
fn tone_lookup(y: f32) -> f32 {
    if (y <= 1e-6) {
        return 0.0;
    }
    let ev = log2(y / 0.18);
    let f = clamp((ev - TONE_MIN_EV) / (TONE_MAX_EV - TONE_MIN_EV), 0.0, 1.0)
        * f32(TONE_N - 1u);
    let i = u32(floor(f));
    let next = min(i + 1u, TONE_N - 1u);
    let t = f - f32(i);
    let v = mix(params.tone[i / 4u][i % 4u], params.tone[next / 4u][next % 4u], t);
    if (ev < TONE_MIN_EV) {
        return v * (y / (0.18 * exp2(TONE_MIN_EV)));
    }
    return v;
}

/// 色域软裁剪：`v <= knee` 原样，超过后单调趋于 1（C¹ 连续）。
fn gamut_soft_clip(v: f32, knee: f32) -> f32 {
    let k = clamp(knee, 0.0, 0.999);
    if (v <= k) {
        return v;
    }
    let r = 1.0 - k;
    return k + r * (1.0 - exp(-(v - k) / r));
}

/// 色域映射：软裁剪 + 按需向亮度去饱和（保持色相），与 CPU `gamut::map_to_gamut` 一致。
fn gamut_map(rgb: vec3<f32>) -> vec3<f32> {
    let clipped = vec3<f32>(
        gamut_soft_clip(rgb.r, 0.9),
        gamut_soft_clip(rgb.g, 0.9),
        gamut_soft_clip(rgb.b, 0.9)
    );
    let lo = min(min(clipped.r, clipped.g), clipped.b);
    let hi = max(max(clipped.r, clipped.g), clipped.b);
    if (lo >= 0.0 && hi <= 1.0) {
        return clipped;
    }
    let y = clamp(luminance(clipped), 0.0, 1.0);
    var lo_k = 0.0;
    var hi_k = 1.0;
    for (var i = 0; i < 24; i = i + 1) {
        let k = (lo_k + hi_k) * 0.5;
        let candidate = vec3<f32>(y) + (clipped - vec3<f32>(y)) * k;
        if (all(candidate >= vec3<f32>(0.0)) && all(candidate <= vec3<f32>(1.0))) {
            lo_k = k;
        } else {
            hi_k = k;
        }
    }
    let y0 = luminance(clipped);
    let mapped = vec3<f32>(y) + (clipped - vec3<f32>(y)) * lo_k;
    let y1 = max(luminance(mapped), 1e-9);
    return clamp(mapped * (y0 / y1), vec3<f32>(0.0), vec3<f32>(1.0));
}

/// 色调段：局部色调（高光 / 阴影，权重取自基底层）→ 电影曲线 → 色域映射。
///
/// `base_ev` 是基底层（大模糊参考）在像素处相对中灰的场景 EV——
/// 小面积高光的基底层仍是中灰，因此不会被"顺手"压暗（无光晕）。
fn pipeline_tone(input_color: vec3<f32>, base_ev: f32) -> vec3<f32> {
    var c = input_color;
    if (p(193) <= 0.5) {
        return c;
    }
    // 1) 局部色调：乘性 EV 位移（纯黑保持纯黑、通道比例不变）。
    let h = clamp(p(2) / 100.0, -1.0, 1.0);
    let sh = clamp(p(3) / 100.0, -1.0, 1.0);
    if (h != 0.0 || sh != 0.0) {
        var delta = 0.0;
        if (sh != 0.0) {
            let w_sh = 1.0 - smoothstep(-4.8, 0.3, base_ev);
            delta = delta + sh * 1.7 * w_sh * sqrt(w_sh);
        }
        if (h != 0.0) {
            let w_hi = smoothstep(-1.0, 2.8, base_ev);
            delta = delta + h * 1.7 * w_hi;
        }
        c = c * exp2(delta);
    }
    // 2) 电影曲线：按亮度比缩放（保持色相与通道比例）。
    let y = max(luminance(c), 0.0);
    if (y > 1e-6) {
        c = c * (tone_lookup(y) / y);
    } else if (p(5) != 0.0) {
        // 纯黑：黑点被抬起 / 压死时按曲线的黑场值输出（与 CPU `ToneStage` 一致）。
        c = vec3<f32>(max(clamp(p(5), 0.0, 100.0) / 100.0 * 0.03, 0.0));
    }
    // 3) 色域映射。
    return gamut_map(max(c, vec3<f32>(0.0)));
}

/// 显示段（色调段之后）：参数曲线 → 曲线 LUT → 调色算子（OkLab / OkLCh）。
///
/// 原先在 `pipeline_base` 的第 5–7 步；色调段移到 `cs_main` 后，
/// 这三步跟着到色调映射之后（与 LightCraft 的"显示段"顺序一致）。
fn pipeline_display_ops(input_color: vec3<f32>) -> vec3<f32> {
    var c = input_color;

    // 5) 参数曲线（分界同样是显示参考空间：默认 0.65 / 0.35 即 sRGB 亮度分界）
    let param_lum = luminance(linear_to_srgb(c));
    let hi_split = clamp(p(30), 0.05, 0.95);
    let mid_split = clamp(p(31), 0.05, 0.95);
    let param_hi_zone = smoothstep(hi_split, 1.0, param_lum);
    let param_lo_zone = 1.0 - smoothstep(0.0, mid_split, param_lum);
    let param_mid_zone = max(1.0 - param_hi_zone - param_lo_zone, 0.0);
    let param_gain = 1.0
        + p(27) / 100.0 * param_hi_zone * 0.6
        + p(28) / 100.0 * param_mid_zone * 0.6
        + p(29) / 100.0 * param_lo_zone * 0.6;
    c = max(c * param_gain, vec3<f32>(0.0));

    // 6) 曲线 LUT（gamma 空间；曲线定义域为 0..1，>1 的高光余量原样保留）
    let curve_domain = min(c, vec3<f32>(1.0));
    var gamma = linear_to_gamma(curve_domain);
    gamma = vec3<f32>(
        curve_lookup(gamma.r, 0u),
        curve_lookup(gamma.g, 1u),
        curve_lookup(gamma.b, 2u)
    );
    c = gamma_to_linear(gamma) + max(c - vec3<f32>(1.0), vec3<f32>(0.0));

    // 7) 调色段（OkLab / OkLCh）：8 色带混色 → 点颜色 → 自然饱和度 / 饱和度
    //    → 黑白 → 颜色分级。与 CPU `fdxmp-color::ops::ColorOps` 同序同公式；
    //    p(192) 由 CPU 侧判断（全部为默认值时整段跳过）。
    if (p(192) > 0.5) {
        c = pipeline_colour_ops(c);
    }

    return c;
}

/// 细节链路：纹理 / 清晰度 / 去雾 / 锐化 / 降噪 / 暗角 / 颗粒。
///
/// `blurred`（binding 2）与小半径 `aux_tex`（binding 6）都取自**基础调色结果**
/// 的模糊，与 `c` 处于同一曝光 / 色彩域；旧实现用未调色的原图做参考，
/// 一旦有曝光 / 白平衡调整就会产生直流偏移与色偏。
fn pipeline_detail(
    input_color: vec3<f32>,
    blurred: vec3<f32>,
    uv: vec2<f32>,
    size: vec2<f32>,
    gid: vec2<u32>,
    coord: vec2<i32>
) -> vec3<f32> {
    var c = input_color;

    // 10) 纹理（细半径微反差）/ 清晰度（大半径中间调局部对比）/ 去雾
    let fine = select(blurred, textureLoad(aux_tex, coord, 0).rgb, p(79) > 0.5);
    let texture_weight = p(10) / 100.0;
    let clarity_weight = p(11) / 100.0;
    if (abs(texture_weight) > 0.0001) {
        c = c + (c - fine) * texture_weight * 1.5;
    }
    if (abs(clarity_weight) > 0.0001) {
        let midtone_weight = max(1.0 - abs(luminance(c) - 0.5) * 1.6, 0.0);
        c = c + (c - blurred) * clarity_weight * midtone_weight * 1.6;
    }
    let dehaze = p(12) / 100.0;
    if (dehaze > 0.0001) {
        // 去雾：拉黑场 + 提对比 + 提饱和（正方向）；负方向加"雾"（提黑场、降对比）。
        // +100 强度按实测再上一档（黑场 0.14 / 对比 ×1.85 / 饱和 ×1.35），对齐 LR 的去雾力度。
        c = max((c - vec3<f32>(0.14 * dehaze)) * (1.0 + dehaze * 0.85), vec3<f32>(0.0));
        let gray = vec3<f32>(luminance(c));
        c = gray + (c - gray) * (1.0 + dehaze * 0.35);
    } else if (dehaze < -0.0001) {
        let veil = -dehaze;
        c = max(c * (1.0 - veil * 0.3) + vec3<f32>(veil * 0.10), vec3<f32>(0.0));
    }

    // 11) 锐化：数量 / 半径 / 细节 / 蒙版（参考 = 细半径模糊）
    let sharpen_amount = p(15) / 100.0;
    if (sharpen_amount > 0.0001) {
        let detail_mix = clamp(p(17) / 100.0, 0.0, 1.0);
        var signal = c - mix(fine, blurred, 0.4 * (1.0 - detail_mix));
        // 蒙版：按局部梯度只在边缘处锐化（0 = 全图锐化）
        let mask_amount = clamp(p(18) / 100.0, 0.0, 1.0);
        if (mask_amount > 0.0001) {
            let max_coord = vec2<i32>(i32(size.x) - 1, i32(size.y) - 1);
            let left = textureLoad(src_tex, clamp(coord + vec2<i32>(-1, 0), vec2<i32>(0), max_coord), 0).rgb;
            let right = textureLoad(src_tex, clamp(coord + vec2<i32>(1, 0), vec2<i32>(0), max_coord), 0).rgb;
            let up = textureLoad(src_tex, clamp(coord + vec2<i32>(0, -1), vec2<i32>(0), max_coord), 0).rgb;
            let down = textureLoad(src_tex, clamp(coord + vec2<i32>(0, 1), vec2<i32>(0), max_coord), 0).rgb;
            let gradient = length(c - left) + length(c - right) + length(c - up) + length(c - down);
            signal = signal * smoothstep(0.0, 0.02 + mask_amount * 0.3, gradient);
        }
        // 半径越大，锐化过渡越柔和（半径 0.5..3.0 → 系数 0.78..1.2）
        let radius_scale = 0.7 + 0.5 * clamp(p(16), 0.5, 3.0) / 3.0;
        c = c + signal * sharpen_amount * radius_scale * 1.8;
    }

    // 12) 降噪：亮度（细节 / 对比度）
    let luminance_amount = clamp(p(19) / 100.0, 0.0, 1.0) * 0.9;
    if (luminance_amount > 0.0001) {
        let luma_detail = clamp(p(20) / 100.0, 0.0, 1.0);
        let denoise_source = mix(blurred, fine, luma_detail);
        let before_denoise = c;
        c = mix(c, denoise_source, luminance_amount);
        // 亮度对比度：把残差按比例加回，保留质感
        let luma_contrast = clamp(p(21) / 100.0, 0.0, 1.0) * 0.6;
        c = c + (before_denoise - denoise_source) * luma_contrast;
    }

    // 13) 降噪：颜色（细节 / 平滑度）
    let color_amount = clamp(p(22) / 100.0, 0.0, 1.0) * 0.9;
    if (color_amount > 0.0001) {
        let color_detail = clamp(p(23) / 100.0, 0.0, 1.0);
        let color_smoothness = clamp(p(24) / 100.0, 0.0, 1.0);
        let effective = clamp(
            color_amount * (1.0 - color_detail * 0.5) + color_smoothness * 0.3,
            0.0,
            1.0
        );
        let base_lum = vec3<f32>(luminance(c));
        let blur_lum = vec3<f32>(luminance(blurred));
        let chroma_original = c - base_lum;
        let chroma_blurred = blurred - blur_lum;
        c = base_lum + mix(chroma_original, chroma_blurred, effective);
    }

    // 分块渲染（1:1 视图）时用整图坐标：暗角 / 颗粒与整图渲染保持一致。
    // p(97..98) = 整图输出尺寸（>0.5 表示当前是分块），p(86..87) = 分块原点。
    var frame_uv = uv;
    var frame_coord = vec2<f32>(gid);
    var frame_size = size;
    if (p(97) > 0.5) {
        frame_coord = vec2<f32>(gid) + vec2<f32>(p(86), p(87));
        frame_size = vec2<f32>(p(97), p(98));
        frame_uv = (frame_coord + vec2<f32>(0.5)) / max(frame_size, vec2<f32>(1.0));
    }

    // 14) 暗角（中点控制作用范围、羽化控制过渡宽度；正负数量分别压暗 / 提亮四角）
    let vignette = p(13) / 100.0;
    if (abs(vignette) > 0.0001) {
        let midpoint = clamp(p(107) / 100.0, 0.0, 1.0);
        let feather = clamp(p(106) / 100.0, 0.0, 1.0);
        let aspect = frame_size.x / max(frame_size.y, 1.0);
        let centered = (frame_uv - vec2<f32>(0.5)) * vec2<f32>(aspect, 1.0);
        let radius = length(centered) / length(vec2<f32>(aspect, 1.0) * 0.5);
        let start = mix(0.10, 0.90, midpoint);
        let width = mix(0.10, 0.90, feather);
        let falloff = smoothstep(start, start + max(width, 0.02), radius);
        c = c * (1.0 - vignette * falloff * 0.8);
    }

    // 15) 颗粒（大小 = 噪声格子尺寸，粗糙度 = 细噪声占比；中间调最明显）
    let grain = p(14) / 100.0;
    if (grain > 0.0001) {
        let grain_size = clamp(p(104) / 100.0, 0.0, 1.0);
        let roughness = clamp(p(105) / 100.0, 0.0, 1.0);
        // 颗粒尺度随输出分辨率缩放：预览 2048 与全分辨率导出观感一致。
        // 基础格子 1.5px 起（1px 格子在缩放视图里会被降采样抹掉，像"没有颗粒"）。
        let grain_scale = clamp(max(frame_size.x, frame_size.y) / 2048.0, 1.0, 4.0);
        let scale = mix(1.5, 11.5, grain_size) * grain_scale;
        let smooth_noise = value_noise(frame_coord / scale, p(65));
        let fine_noise = value_noise(frame_coord / max(scale * 0.4, 1.0), p(65) + 3.0);
        let rough_noise = hash_noise(frame_coord, p(65) + 7.0);
        let noise = mix(mix(smooth_noise, fine_noise, 0.5), rough_noise, roughness);
        let grain_luma = clamp(
            luminance(linear_to_srgb(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)))),
            0.0,
            1.0
        );
        let grain_weight = max(1.0 - abs(grain_luma - 0.5) * 1.2, 0.0);
        c = c + (noise - 0.5) * grain * grain_weight * 0.25;
    }

    return max(c, vec3<f32>(0.0));
}

/// 基础调色：源图（几何输出 stage）→ 线性 f16 中间纹理。
@compute @workgroup_size(8, 8)
fn cs_base(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(dst_tex);
    if (gid.x >= size.x || gid.y >= size.y) {
        return;
    }
    let coord = vec2<i32>(i32(gid.x), i32(gid.y));
    let source = textureLoad(src_tex, coord, 0).rgb;
    textureStore(dst_tex, coord, vec4<f32>(pipeline_base(source), 1.0));
}

/// 细节链路 + 可选蒙版混合：基础调色结果 → 线性 f16 结果。
@compute @workgroup_size(8, 8)
fn cs_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(dst_tex);
    if (gid.x >= size.x || gid.y >= size.y) {
        return;
    }
    let coord = vec2<i32>(i32(gid.x), i32(gid.y));
    let uv = (vec2<f32>(gid.xy) + vec2<f32>(0.5)) / vec2<f32>(size);

    let base = textureLoad(src_tex, coord, 0).rgb;
    let blurred = textureLoad(blur_tex, coord, 0).rgb;
    // 色调段：局部基底层（大模糊参考的亮度）→ 局部色调 / 电影曲线 / 色域映射，
    // 然后才是显示段（曲线 + 调色算子）与细节链路。与 CPU `tone::ToneStage` /
    // `preview::render_preview_image_at` 同序同公式。
    let toned = pipeline_tone(base, log2(max(luminance(blurred), 1e-6) / 0.18));
    let displayed = pipeline_display_ops(toned);
    let adjusted = pipeline_detail(displayed, blurred, uv, vec2<f32>(size), gid.xy, coord);

    var output_linear = adjusted;
    if (p(68) > 0.5) {
        let mask_coord = mask_coord_for(gid.xy, coord);
        let weight = clamp(textureLoad(mask_tex, mask_coord, 0).r, 0.0, 1.0);
        let current = textureLoad(current_tex, coord, 0).rgb; // 线性 f16，无需解码
        output_linear = mix(current, adjusted, weight);
    }
    textureStore(dst_tex, coord, vec4<f32>(max(output_linear, vec3<f32>(0.0)), 1.0));
}

/// 编码：线性 f16 → sRGB 8bit（整条管线唯一的量化 / 裁剪点）。
@compute @workgroup_size(8, 8)
fn cs_encode(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(display_tex);
    if (gid.x >= size.x || gid.y >= size.y) {
        return;
    }
    let coord = vec2<i32>(i32(gid.x), i32(gid.y));
    let linear = textureLoad(src_tex, coord, 0).rgb;
    textureStore(display_tex, coord, vec4<f32>(linear_to_srgb(linear), 1.0));
}

/// 输出像素 → 蒙版采样坐标（几何激活时映射回源图坐标，越界时夹取）。
fn mask_coord_for(gid: vec2<u32>, coord: vec2<i32>) -> vec2<i32> {
    if (p(78) <= 0.5) {
        return coord;
    }
    let pixel = vec2<f32>(f32(gid.x) + 0.5, f32(gid.y) + 0.5);
    let w = p(75) * pixel.x + p(76) * pixel.y + p(77);
    let safe_w = select(w, 1.0, abs(w) < 1e-6);
    let source = vec2<f32>(
        (p(69) * pixel.x + p(70) * pixel.y + p(71)) / safe_w,
        (p(72) * pixel.x + p(73) * pixel.y + p(74)) / safe_w
    );
    let dims = vec2<i32>(textureDimensions(mask_tex));
    let scale = vec2<f32>(max(p(84), 1e-6), max(p(85), 1e-6));
    // 分块 / 裁剪渲染时源坐标是相对当前裁剪图的，先加回整图原点再换算蒙版坐标
    let scaled = (source + vec2<f32>(p(100), p(101))) / scale;
    return clamp(
        vec2<i32>(i32(floor(scaled.x)), i32(floor(scaled.y))),
        vec2<i32>(0),
        max(dims - vec2<i32>(1), vec2<i32>(0))
    );
}

/// 蒙版叠加层：在当前图像上按蒙版权重叠加半透明颜色（裁剪 / 旋转后仍与画面内容对齐）。
@compute @workgroup_size(8, 8)
fn cs_overlay(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(display_tex);
    if (gid.x >= size.x || gid.y >= size.y) {
        return;
    }
    let coord = vec2<i32>(i32(gid.x), i32(gid.y));
    let alpha = clamp(p(80), 0.0, 1.0);
    if (alpha <= 0.0001) {
        textureStore(display_tex, coord, textureLoad(current_tex, coord, 0));
        return;
    }
    let mask_coord = mask_coord_for(gid.xy, coord);
    let weight = clamp(textureLoad(mask_tex, mask_coord, 0).r, 0.0, 1.0);
    let base = textureLoad(current_tex, coord, 0).rgb;
    let tint = vec3<f32>(p(81), p(82), p(83));
    textureStore(display_tex, coord, vec4<f32>(mix(base, tint, weight * alpha), 1.0));
}

@compute @workgroup_size(8, 8)
fn cs_copy(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(dst_tex);
    if (gid.x >= size.x || gid.y >= size.y) {
        return;
    }
    let coord = vec2<i32>(i32(gid.x), i32(gid.y));
    textureStore(dst_tex, coord, textureLoad(aux_tex, coord, 0));
}
