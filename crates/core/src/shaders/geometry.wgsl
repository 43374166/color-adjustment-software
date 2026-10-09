// fdxmp 几何管线（阶段 6：裁剪 / 旋转 / 翻转 / 透视）
//
// 入口：
//   cs_geometry —— 逆映射重采样：输出像素 →（逆矩阵）→ 源图像素，双线性采样。
//                  源纹理 rgba8unorm-srgb（textureLoad 自动解码为线性），
//                  输出 rgba16float（线性，供后续模糊 / 调色管线使用）。
//
// 参数布局（vec4 对齐）：
//   m0 = [a00, a01, a02, 0]
//   m1 = [a10, a11, a12, 0]
//   m2 = [a20, a21, a22, 0]   （输出像素齐次坐标 → 源图像素，含透视除法）
//   sizes = [源宽, 源高, 输出宽, 输出高]
//   lens  = [畸变系数 k1, 色差系数 ca, 暗角强度 v, 是否启用]

struct GeoParams {
    m0: vec4<f32>,
    m1: vec4<f32>,
    m2: vec4<f32>,
    sizes: vec4<f32>,
    lens: vec4<f32>,
};

@group(0) @binding(0) var<uniform> geo: GeoParams;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var dst_tex: texture_storage_2d<rgba16float, write>;

fn sample_bilinear(position: vec2<f32>) -> vec4<f32> {
    let size = vec2<i32>(i32(geo.sizes.x), i32(geo.sizes.y));
    let center = position - vec2<f32>(0.5);
    let base = vec2<i32>(i32(floor(center.x)), i32(floor(center.y)));
    let fraction = center - vec2<f32>(f32(base.x), f32(base.y));
    var result = vec4<f32>(0.0);
    for (var dy = 0; dy < 2; dy = dy + 1) {
        for (var dx = 0; dx < 2; dx = dx + 1) {
            let coord = base + vec2<i32>(dx, dy);
            if (coord.x < 0 || coord.y < 0 || coord.x >= size.x || coord.y >= size.y) {
                continue;
            }
            let weight = select(1.0 - fraction.x, fraction.x, dx == 1)
                * select(1.0 - fraction.y, fraction.y, dy == 1);
            result = result + textureLoad(src_tex, coord, 0) * weight;
        }
    }
    return result;
}

/// 单通道双线性采样（镜头色差校正用，坐标各自独立）。
fn sample_channel(position: vec2<f32>, channel: u32) -> f32 {
    let size = vec2<i32>(i32(geo.sizes.x), i32(geo.sizes.y));
    let center = position - vec2<f32>(0.5);
    let base = vec2<i32>(i32(floor(center.x)), i32(floor(center.y)));
    let fraction = center - vec2<f32>(f32(base.x), f32(base.y));
    var result = 0.0;
    for (var dy = 0; dy < 2; dy = dy + 1) {
        for (var dx = 0; dx < 2; dx = dx + 1) {
            let coord = base + vec2<i32>(dx, dy);
            if (coord.x < 0 || coord.y < 0 || coord.x >= size.x || coord.y >= size.y) {
                continue;
            }
            let weight = select(1.0 - fraction.x, fraction.x, dx == 1)
                * select(1.0 - fraction.y, fraction.y, dy == 1);
            result = result + textureLoad(src_tex, coord, 0)[channel] * weight;
        }
    }
    return result;
}

@compute @workgroup_size(8, 8)
fn cs_geometry(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(dst_tex);
    if (gid.x >= size.x || gid.y >= size.y) {
        return;
    }
    let pixel = vec2<f32>(f32(gid.x) + 0.5, f32(gid.y) + 0.5);
    let w = geo.m2.x * pixel.x + geo.m2.y * pixel.y + geo.m2.z;
    let safe_w = select(w, 1.0, abs(w) < 1e-6);
    let source = vec2<f32>(
        (geo.m0.x * pixel.x + geo.m0.y * pixel.y + geo.m0.z) / safe_w,
        (geo.m1.x * pixel.x + geo.m1.y * pixel.y + geo.m1.z) / safe_w
    );
    var sampled: vec4<f32>;
    if (geo.lens.w > 0.5) {
        // 镜头校正：以源图中心为原点的归一化半径上做畸变 / 色差 / 暗角。
        let center = vec2<f32>(geo.sizes.x * 0.5, geo.sizes.y * 0.5);
        let half_size = vec2<f32>(max(geo.sizes.x * 0.5, 1.0), max(geo.sizes.y * 0.5, 1.0));
        let normalized = (source - center) / half_size;
        let radius_sq = dot(normalized, normalized);
        let radial = 1.0 + geo.lens.x * radius_sq;
        let ca = geo.lens.y;
        let red = center + normalized * radial * (1.0 + ca) * half_size;
        let green = center + normalized * radial * half_size;
        let blue = center + normalized * radial * (1.0 - ca) * half_size;
        let falloff = smoothstep(0.55, 1.0, sqrt(radius_sq));
        let gain = 1.0 + geo.lens.z * falloff;
        sampled = vec4<f32>(
            sample_channel(red, 0u) * gain,
            sample_channel(green, 1u) * gain,
            sample_channel(blue, 2u) * gain,
            sample_channel(green, 3u)
        );
    } else {
        sampled = sample_bilinear(source);
    }
    textureStore(dst_tex, vec2<i32>(i32(gid.x), i32(gid.y)), sampled);
}
