//! Execution of benchmarks for channeled module, with background collection thread running.
//! These benchmarks are very slow compared with the 'nobg' variants.

mod common;

use common::{channeled_bg_map_bench, channeled_bg_u32_bench};
use criterion::{criterion_group, criterion_main, Criterion};

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("channeled_bg_u32", |b| b.iter(channeled_bg_u32_bench));
    c.bench_function("channeled_bg_map", |b| b.iter(channeled_bg_map_bench));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
