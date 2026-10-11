// 上屏拷贝：把显示纹理（rgba8unorm，sRGB 编码）按 1:1 原样写到表面纹理；
// 目标格式（bgra8unorm 等）与颜色空间转换由 GPU 在写入时完成。

@group(0) @binding(0) var src_tex: texture_2d<f32>;

@vertex
fn vs_present(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // 全屏三角形（比四边形少一次插值边界问题）。
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(positions[index], 0.0, 1.0);
}

@fragment
fn fs_present(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let coord = vec2<i32>(i32(floor(position.x)), i32(floor(position.y)));
    return textureLoad(src_tex, coord, 0);
}
