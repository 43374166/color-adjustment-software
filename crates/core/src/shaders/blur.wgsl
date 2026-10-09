// 可分离高斯模糊（清晰度 / 纹理 / 锐化 / 降噪 / 蒙版羽化的局部参考）

struct BlurParams {
    // xy = 方向（1,0 或 0,1），z = 半径（像素），w 保留
    dir: vec4<f32>,
};

@group(0) @binding(0) var<uniform> blur_params: BlurParams;
@group(0) @binding(1) var blur_src: texture_2d<f32>;
@group(0) @binding(2) var blur_dst: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8)
fn cs_blur(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(blur_dst);
    if (gid.x >= size.x || gid.y >= size.y) {
        return;
    }
    let coord = vec2<i32>(i32(gid.x), i32(gid.y));
    // 方向只取单位向量（±1 / 0），半径只决定抽头数量：偏移 = 方向 × i，
    // 抽头间距恒为 1px。旧实现把半径乘进方向后再乘 i（间距 = 半径），
    // 半径 2/3/4 时分别只采样偶数 / 3 的倍数 / 4 的倍数像素，
    // 会产生幽灵边缘与摩尔纹，半径 < 1 时甚至退化为原图。
    let direction = vec2<i32>(clamp(blur_params.dir.xy, vec2<f32>(-1.0), vec2<f32>(1.0)));
    let radius = i32(clamp(blur_params.dir.z, 1.0, 6.0));

    var acc = vec3<f32>(0.0);
    var weight_sum = 0.0;
    for (var i = -6; i <= 6; i = i + 1) {
        if (abs(i) > radius) {
            continue;
        }
        let offset = direction * i;
        let sample_coord = clamp(
            coord + offset,
            vec2<i32>(0, 0),
            vec2<i32>(i32(size.x) - 1, i32(size.y) - 1)
        );
        let weight = 1.0 / (1.0 + f32(i * i));
        acc = acc + textureLoad(blur_src, sample_coord, 0).rgb * weight;
        weight_sum = weight_sum + weight;
    }
    textureStore(blur_dst, coord, vec4<f32>(acc / max(weight_sum, 0.0001), 1.0));
}

// 宽抽头入口：供高分辨率（1:1 / 全分辨率导出）按比例放大的半径使用（最多 49 抽头）。
// 与 cs_blur 逻辑相同，仅循环上限不同（WGSL 循环边界必须是编译期常量）。
@compute @workgroup_size(8, 8)
fn cs_blur_wide(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(blur_dst);
    if (gid.x >= size.x || gid.y >= size.y) {
        return;
    }
    let coord = vec2<i32>(i32(gid.x), i32(gid.y));
    let direction = vec2<i32>(clamp(blur_params.dir.xy, vec2<f32>(-1.0), vec2<f32>(1.0)));
    let radius = i32(clamp(blur_params.dir.z, 1.0, 24.0));

    var acc = vec3<f32>(0.0);
    var weight_sum = 0.0;
    for (var i = -24; i <= 24; i = i + 1) {
        if (abs(i) > radius) {
            continue;
        }
        let offset = direction * i;
        let sample_coord = clamp(
            coord + offset,
            vec2<i32>(0, 0),
            vec2<i32>(i32(size.x) - 1, i32(size.y) - 1)
        );
        let weight = 1.0 / (1.0 + f32(i * i));
        acc = acc + textureLoad(blur_src, sample_coord, 0).rgb * weight;
        weight_sum = weight_sum + weight;
    }
    textureStore(blur_dst, coord, vec4<f32>(acc / max(weight_sum, 0.0001), 1.0));
}
