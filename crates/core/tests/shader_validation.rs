//! 着色器语法自检：所有 WGSL 必须能被 naga 解析并通过校验。
//!
//! 与 wgpu 30 同一份 naga，本地 `cargo test` 就能发现着色器错误，
//! 不必等到 GPU 环境 / 真机才暴露。

#[test]
fn wgsl_shaders_parse_and_validate() {
    let shaders: [(&str, &str); 5] = [
        (
            "color_pipeline.wgsl",
            include_str!("../src/shaders/color_pipeline.wgsl"),
        ),
        (
            "geometry.wgsl",
            include_str!("../src/shaders/geometry.wgsl"),
        ),
        ("blur.wgsl", include_str!("../src/shaders/blur.wgsl")),
        (
            "histogram.wgsl",
            include_str!("../src/shaders/histogram.wgsl"),
        ),
        ("present.wgsl", include_str!("../src/shaders/present.wgsl")),
    ];
    for (name, source) in shaders {
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|err| panic!("{name} 解析失败：{err:?}"));
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .unwrap_or_else(|err| panic!("{name} 校验失败：{err:?}"));
    }
}
