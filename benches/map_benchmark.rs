//! Execution of benchmarks for different modules with map accumulators.
//! This excludes [common::channeled_bg_map_bench] which is very slow.

mod common;

use common::{
    channeled_nobg_map_bench, joined_map_bench, probed_map_bench, simple_joined_map_bench,
    tlcr_map_bench,
};
use criterion::{criterion_group, criterion_main, Criterion};

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("joined_map", |b| b.iter(joined_map_bench));
    c.bench_function("simple_joined_map", |b| b.iter(simple_joined_map_bench));
    c.bench_function("probed_map", |b| b.iter(probed_map_bench));
    c.bench_function("tlcr_map", |b| b.iter(tlcr_map_bench));
    c.bench_function("channeled_nobg_map", |b| b.iter(channeled_nobg_map_bench));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
