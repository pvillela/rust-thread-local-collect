//! Execution of benchmarks for different modules with u32 accumulators.
//! This excludes [common::channeled_bg_u32_bench] which is very slow.

mod common;

use common::{
    channeled_nobg_u32_bench, joined_u32_bench, probed_u32_bench, simple_joined_u32_bench,
    tlcr_probed_u32_bench, tlcr_u32_bench,
};
use criterion::{criterion_group, criterion_main, Criterion};

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("joined_u32", |b| b.iter(joined_u32_bench));
    c.bench_function("simple_joined_u32", |b| b.iter(simple_joined_u32_bench));
    c.bench_function("probed_u32", |b| b.iter(probed_u32_bench));
    c.bench_function("tlcr_u32", |b| b.iter(tlcr_u32_bench));
    c.bench_function("tlcr_probed_u32", |b| b.iter(tlcr_probed_u32_bench));
    c.bench_function("channeled_nobg_u32", |b| b.iter(channeled_nobg_u32_bench));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
