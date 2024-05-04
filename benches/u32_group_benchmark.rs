//! Execution of benchmarks for different modules with u32 accumulators, using the `group` construct.
//! This excludes [common::tlm_channeled_bg_u32_bench] which is very slow.
//! The results are comparable to those from `bench_u32.sh` which uses `benchmark.rs`.

mod common;

use common::{
    tlcr_joined_u32_bench, tlcr_probed_u32_bench, tlm_channeled_nobg_u32_bench,
    tlm_joined_u32_bench, tlm_probed_u32_bench, tlm_simple_joined_u32_bench,
};
use criterion::{criterion_group, criterion_main, Criterion};

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("tlc_group");
    group.bench_function("tlm_joined_u32", |b| b.iter(tlm_joined_u32_bench));
    group.bench_function("tlm_simple_joined_u32", |b| {
        b.iter(tlm_simple_joined_u32_bench)
    });
    group.bench_function("tlm_probed_u32", |b| b.iter(tlm_probed_u32_bench));
    group.bench_function("tlcr_joined_u32", |b| b.iter(tlcr_joined_u32_bench));
    group.bench_function("tlcr_probed_u32", |b| b.iter(tlcr_probed_u32_bench));
    group.bench_function("tlm_channeled_nobg_u32", |b| {
        b.iter(tlm_channeled_nobg_u32_bench)
    });
    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
