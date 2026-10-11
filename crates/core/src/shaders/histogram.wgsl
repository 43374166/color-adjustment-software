// 直方图：对显示纹理（rgba8unorm，sRGB 编码）统计 R / G / B / 亮度四组 256 桶。
//
// 布局：bins[0..256] = 红、bins[256..512] = 绿、bins[512..768] = 蓝、
// bins[768..1024] = 亮度（Rec.709 整数近似，与 Swift 侧统计同公式）。

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> bins: array<atomic<u32>>;

@compute @workgroup_size(8, 8)
fn cs_histogram(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = textureDimensions(src_tex);
    if (gid.x >= size.x || gid.y >= size.y) {
        return;
    }
    let color = textureLoad(src_tex, vec2<i32>(i32(gid.x), i32(gid.y)), 0);
    let r = u32(clamp(color.r * 255.0 + 0.5, 0.0, 255.0));
    let g = u32(clamp(color.g * 255.0 + 0.5, 0.0, 255.0));
    let b = u32(clamp(color.b * 255.0 + 0.5, 0.0, 255.0));
    let luma = min((r * 2126u + g * 7152u + b * 722u) / 10000u, 255u);
    atomicAdd(&bins[r], 1u);
    atomicAdd(&bins[256u + g], 1u);
    atomicAdd(&bins[512u + b], 1u);
    atomicAdd(&bins[768u + luma], 1u);
}
