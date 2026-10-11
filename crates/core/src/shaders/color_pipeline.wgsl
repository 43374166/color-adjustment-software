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
    v: array<vec4<f32>, 27>,
    curve: array<vec4<f32>, 256>,
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

/// 颜色分级单区累加：目标色按亮度归一后按权重 / 饱和度累积，同时累积本区亮度增益。
fn add_grade_zone(
    hue_degrees: f32,
    saturation: f32,
    luminance_shift: f32,
    weight: f32,
    grade_color: ptr<function, vec3<f32>>,
    grade_strength: ptr<function, f32>,
    grade_lum_shift: ptr<function, f32>
) {
    if (weight <= 0.0001) {
        return;
    }
    let amount = clamp(saturation / 100.0, 0.0, 1.0);
    let hue_color = hsv_to_rgb(vec3<f32>(fract(hue_degrees / 360.0), 1.0, 1.0));
    let normalized = hue_color / max(luminance(hue_color), 1e-4);
    *grade_color = *grade_color + normalized * weight * amount;
    *grade_strength = *grade_strength + weight * amount;
    *grade_lum_shift =
        *grade_lum_shift + weight * clamp(luminance_shift / 100.0, -1.0, 1.0) * 0.5;
}

/// HSL 8 色带权重：色相落在相邻两个色带中心之间时按距离线性分配，
/// **权重和恒为 1**（分区完备）。
///
/// 旧实现用固定 30° 衰减半径，而色带中心间距最大 60°（黄 60°→绿 120°→青 180°→蓝 240°），
/// 导致 90° / 150° / 210° 附近权重和为 0——滑杆完全无响应。
fn hsl_band_weights(hue: f32) -> array<f32, 8> {
    let centers = array<f32, 8>(
        0.0 / 360.0, 30.0 / 360.0, 60.0 / 360.0, 120.0 / 360.0,
        180.0 / 360.0, 240.0 / 360.0, 280.0 / 360.0, 320.0 / 360.0
    );
    var weights = array<f32, 8>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let h = fract(hue);
    for (var band = 0u; band < 8u; band = band + 1u) {
        let start = centers[band];
        // 末段（320° → 360°/0°）跨 0°，fract 给出正向跨度。
        let span = fract(centers[(band + 1u) % 8u] - start + 1.0);
        // 只有"覆盖当前色相的那一段"会命中：其余段即使 +1 也落在区间之外。
        let x = select(h + 1.0, h, h >= start);
        if (x < start + span) {
            let t = clamp((x - start) / max(span, 1e-6), 0.0, 1.0);
            weights[band] = 1.0 - t;
            weights[(band + 1u) % 8u] = t;
        }
    }
    return weights;
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

    // 2) 曝光
    c = c * pow(2.0, p(0));

    // 3) 对比度：Lightroom 式显示空间 S 曲线——端点锚定（0 → 0、1 → 1，
    //    不压死暗部、不裁白），中段斜率随强度增加（+100 → 中段 ≈ 1.4×）；
    //    超出 1 的高光余量原样保留。与 CPU `pipeline::apply_reference` 同公式。
    let contrast_amount = clamp(p(1) / 100.0, -1.0, 1.0) * 0.8;
    if (abs(contrast_amount) > 0.0001) {
        let contrast_headroom = max(c - vec3<f32>(1.0), vec3<f32>(0.0));
        let contrast_s = linear_to_srgb(c);
        let contrast_smooth = contrast_s * contrast_s * (vec3<f32>(3.0) - 2.0 * contrast_s);
        let contrast_curved = contrast_s + vec3<f32>(contrast_amount) * (contrast_smooth - contrast_s);
        c = srgb_to_linear(contrast_curved) + contrast_headroom;
    }

    // 4) 高光 / 阴影 / 白色 / 黑色：显示空间分区色调（Lightroom 式）——
    //    分区权重用显示亮度；高光 / 阴影是「向白 / 黑渐进」的有界位移（端点锚定），
    //    白色 / 黑色是白点 / 黑点位移（只动端点，中灰不受影响）。
    //    与 CPU `preview::apply_tone_zones` 同公式。
    let tone_amount = abs(p(2)) + abs(p(3)) + abs(p(4)) + abs(p(5));
    if (tone_amount > 0.0001) {
        let tone_headroom = max(c - vec3<f32>(1.0), vec3<f32>(0.0));
        let tone_s = linear_to_srgb(c);
        let tone_lum = luminance(tone_s);
        let h = clamp(p(2) / 100.0, -1.0, 1.0);
        let sh = clamp(p(3) / 100.0, -1.0, 1.0);
        let wv = clamp(p(4) / 100.0, -1.0, 1.0);
        let bl = clamp(p(5) / 100.0, -1.0, 1.0);
        let w_hi = smoothstep(0.45, 0.85, tone_lum);
        let w_sh = 1.0 - smoothstep(0.15, 0.55, tone_lum);
        let w_wh = smoothstep(0.60, 0.95, tone_lum);
        let w_bl = 1.0 - smoothstep(0.05, 0.40, tone_lum);
        let h_pos = max(h, 0.0);
        let h_neg = max(-h, 0.0);
        let sh_pos = max(sh, 0.0);
        let sh_neg = max(-sh, 0.0);
        let hi_shape_pos = max(vec3<f32>(1.0) - tone_s, vec3<f32>(0.0));
        let hi_shape_neg = max(tone_s - vec3<f32>(0.5), vec3<f32>(0.0))
            * (vec3<f32>(1.0) - 0.55 * smoothstep(vec3<f32>(0.85), vec3<f32>(1.0), tone_s));
        let sh_shape_pos = max(vec3<f32>(0.55) - tone_s, vec3<f32>(0.0));
        let tone_delta = vec3<f32>(h_pos * w_hi * 0.45) * hi_shape_pos
            - vec3<f32>(h_neg * w_hi * 0.45) * hi_shape_neg
            + vec3<f32>(sh_pos * w_sh * 0.40) * sh_shape_pos
            - vec3<f32>(sh_neg * w_sh * 0.35) * tone_s
            + vec3<f32>((wv * w_wh + bl * w_bl) * 0.10);
        let tone_shifted = clamp(tone_s + tone_delta, vec3<f32>(0.0), vec3<f32>(1.0));
        c = srgb_to_linear(tone_shifted) + tone_headroom;
    }

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

    // 7) HSL 8 色带（色相判定在显示参考空间；权重和恒为 1、无空洞）
    let hsl_scale = max(max(c.r, c.g), max(c.b, 1.0));
    let hsl_reference = linear_to_srgb(clamp(c / hsl_scale, vec3<f32>(0.0), vec3<f32>(1.0)));
    var hsv = rgb_to_hsv(hsl_reference);
    let band_weights = hsl_band_weights(hsv.x);
    var hue_shift = 0.0;
    var sat_scale = 0.0;
    var lum_shift = 0.0;
    for (var band = 0u; band < 8u; band = band + 1u) {
        let weight = band_weights[band];
        hue_shift = hue_shift + weight * p(32u + band * 3u) / 100.0 * (1.0 / 12.0);
        sat_scale = sat_scale + weight * p(33u + band * 3u) / 100.0;
        lum_shift = lum_shift + weight * p(34u + band * 3u) / 100.0;
    }
    if (abs(hue_shift) + abs(sat_scale) + abs(lum_shift) > 0.0001) {
        hsv.x = fract(hsv.x + hue_shift);
        hsv.y = clamp(hsv.y * (1.0 + sat_scale), 0.0, 1.0);
        hsv.z = clamp(hsv.z * (1.0 + lum_shift), 0.0, 4.0);
        let adjusted = hsv_to_rgb(hsv);
        // 显示域参考 → 线性；>1 的余量单独保留后统一还原峰值缩放
        let base = min(adjusted, vec3<f32>(1.0));
        let overflow = max(adjusted - vec3<f32>(1.0), vec3<f32>(0.0));
        c = (srgb_to_linear(base) + overflow) * hsl_scale;
    }

    // 8) 颜色分级：三区各自"色相 + 饱和度（强度）+ 明亮度（增益）"，
    //    混合 = 过渡宽度，平衡 = 分界整体平移；
    //    上色用亮度保持混合（目标色按像素亮度归一），不再做加法溢色。
    let grade_lum = luminance(linear_to_srgb(c));
    let balance = clamp(p(26) / 100.0, -1.0, 1.0) * 0.25;
    let blend = clamp(p(25) / 100.0, 0.0, 1.0);
    let soft = 0.05 + blend * 0.25;
    let grade_lo = clamp(0.33 + balance, soft, 1.0 - soft);
    let grade_hi = clamp(0.66 + balance, soft, 1.0 - soft);
    let shadow_zone = 1.0 - smoothstep(grade_lo - soft, grade_lo + soft, grade_lum);
    let highlight_zone = smoothstep(grade_hi - soft, grade_hi + soft, grade_lum);
    let mid_zone = max(1.0 - shadow_zone - highlight_zone, 0.0);

    var grade_color = vec3<f32>(0.0);
    var grade_strength = 0.0;
    var grade_lum_shift = 0.0;
    add_grade_zone(
        p(56), p(57), p(58), shadow_zone,
        &grade_color, &grade_strength, &grade_lum_shift
    );
    add_grade_zone(
        p(59), p(60), p(61), mid_zone,
        &grade_color, &grade_strength, &grade_lum_shift
    );
    add_grade_zone(
        p(62), p(63), p(64), highlight_zone,
        &grade_color, &grade_strength, &grade_lum_shift
    );
    if (grade_strength > 0.0001) {
        let grade_target = grade_color / max(grade_strength, 1e-4);
        let tinted = grade_target * luminance(c);
        c = mix(c, tinted, clamp(grade_strength * 0.5, 0.0, 1.0));
    }
    if (abs(grade_lum_shift) > 0.0001) {
        c = max(c * (1.0 + grade_lum_shift), vec3<f32>(0.0));
    }

    // 9) 自然饱和度 / 饱和度
    let gray = vec3<f32>(luminance(c));
    let max_channel = max(max(c.r, c.g), c.b);
    let min_channel = min(min(c.r, c.g), c.b);
    let chroma = max_channel - min_channel;
    let vibrance_factor = 1.0 + p(8) / 100.0 * (1.0 - clamp(chroma, 0.0, 1.0));
    c = gray + (c - gray) * vibrance_factor;
    c = gray + (c - gray) * (1.0 + p(9) / 100.0);

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

    // 10) 纹理 / 清晰度（共用大半径模糊参考）与去雾
    let texture_weight = p(10) / 100.0;
    let clarity_weight = p(11) / 100.0;
    if (abs(texture_weight) + abs(clarity_weight) > 0.0001) {
        let detail = c - blurred;
        let midtone_weight = 1.0 - abs(luminance(c) - 0.5) * 1.6;
        c = c + detail * texture_weight * 0.7;
        c = c + detail * clarity_weight * max(midtone_weight, 0.0) * 0.9;
    }
    let dehaze = p(12) / 100.0;
    c = max((c - vec3<f32>(0.02 * dehaze)) * (1.0 + dehaze * 0.35), vec3<f32>(0.0));

    // 11) 锐化：数量 / 半径 / 细节 / 蒙版
    let fine = select(blurred, textureLoad(aux_tex, coord, 0).rgb, p(79) > 0.5);
    let sharpen_amount = p(15) / 100.0;
    if (sharpen_amount > 0.0001) {
        let detail_mix = clamp(p(17) / 100.0, 0.0, 1.0);
        var signal = mix(c - blurred, c - fine, detail_mix);
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
        c = c + signal * sharpen_amount * radius_scale;
    }

    // 12) 降噪：亮度（细节 / 对比度）
    let luminance_amount = clamp(p(19) / 100.0, 0.0, 1.0) * 0.8;
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
    let color_amount = clamp(p(22) / 100.0, 0.0, 1.0) * 0.8;
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

    // 15) 颗粒（大小 = 噪声格子尺寸，粗糙度 = 细噪声占比）
    let grain = p(14) / 100.0;
    if (grain > 0.0001) {
        let grain_size = clamp(p(104) / 100.0, 0.0, 1.0);
        let roughness = clamp(p(105) / 100.0, 0.0, 1.0);
        let scale = 1.0 + grain_size * 7.0;
        let smooth_noise = value_noise(frame_coord / scale, p(65));
        let rough_noise = hash_noise(frame_coord, p(65) + 7.0);
        let noise = mix(smooth_noise, rough_noise, roughness);
        c = c + (noise - 0.5) * grain * 0.12;
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
    let adjusted = pipeline_detail(base, blurred, uv, vec2<f32>(size), gid.xy, coord);

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
