//! Execution of benchmarks for different modules with i32 accumulators, using the `group` construct.
//! This excludes [common::tlm_channeled_bg_i32_bench] which is very slow.
//! The results are comparable to those from `bench_i32.sh` which uses `benchmark.rs`.

mod targets;

use criterion::{criterion_group, criterion_main, Criterion};
use targets::{
    tlcr_joined_i32_bench, tlcr_probed_i32_bench, tlm_channeled_nobg_i32_bench,
    tlm_joined_i32_bench, tlm_probed_i32_bench, tlm_simple_joined_i32_bench,
    tlmrestr_joined_i32_bench, tlmrestr_probed_i32_bench, tlmrestr_simple_joined_i32_bench,
};

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("tlc_group");
    group.bench_function("tlcr_joined_i32", |b| b.iter(tlcr_joined_i32_bench));
    group.bench_function("tlcr_probed_i32", |b| b.iter(tlcr_probed_i32_bench));
    group.bench_function("tlm_channeled_nobg_i32", |b| {
        b.iter(tlm_channeled_nobg_i32_bench)
    });
    group.bench_function("tlm_joined_i32", |b| b.iter(tlm_joined_i32_bench));
    group.bench_function("tlm_simple_joined_i32", |b| {
        b.iter(tlm_simple_joined_i32_bench)
    });
    group.bench_function("tlm_probed_i32", |b| b.iter(tlm_probed_i32_bench));
    group.bench_function("tlmrestr_joined_i32", |b| b.iter(tlmrestr_joined_i32_bench));
    group.bench_function("tlmrestr_simple_joined_i32", |b| {
        b.iter(tlmrestr_simple_joined_i32_bench)
    });
    group.bench_function("tlmrestr_probed_i32", |b| b.iter(tlmrestr_probed_i32_bench));

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
