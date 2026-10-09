//! criterion 基准：参数 JSON 序列化与预设打包（阶段 0 基线）。

use criterion::{criterion_group, criterion_main, Criterion};
use fdxmp_core::params::EditParams;
use fdxmp_core::pipeline::apply_reference;
use fdxmp_core::preset::PresetFile;

fn bench_params_json(c: &mut Criterion) {
    let params = EditParams::default();
    c.bench_function("params_to_json", |b| {
        b.iter(|| serde_json::to_vec(&params).unwrap())
    });
}

fn bench_preset_pack(c: &mut Criterion) {
    let preset = PresetFile::new("bench", "criterion", EditParams::default());
    c.bench_function("preset_to_bytes", |b| b.iter(|| preset.to_bytes().unwrap()));
}

fn bench_reference_pipeline(c: &mut Criterion) {
    let params = EditParams::default();
    c.bench_function("reference_pixel", |b| {
        b.iter(|| apply_reference([0.18, 0.36, 0.72], &params))
    });
}

criterion_group!(
    benches,
    bench_params_json,
    bench_preset_pack,
    bench_reference_pipeline
);
criterion_main!(benches);
