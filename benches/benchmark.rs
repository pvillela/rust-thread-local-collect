//! Execution of benchmarks for different modules

mod common;

use common::{channeled_bench, joined_bench, probed_bench, simple_joined_bench, tlcr_bench};
use criterion::{criterion_group, criterion_main, Criterion};

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("joined", |b| b.iter(|| joined_bench()));
    c.bench_function("simple_joined", |b| b.iter(|| simple_joined_bench()));
    c.bench_function("probed", |b| b.iter(|| probed_bench()));
    c.bench_function("tlcr", |b| b.iter(|| tlcr_bench()));

    // Below placed last because it may use significantly more resources and impact benches following it.
    c.bench_function("channeled", |b| b.iter(|| channeled_bench()));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
