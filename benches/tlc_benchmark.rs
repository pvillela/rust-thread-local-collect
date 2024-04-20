mod common;

use common::{simple_joined_bench, tlcr_bench};
use criterion::{criterion_group, criterion_main, Criterion};

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("tlcr", |b| b.iter(|| tlcr_bench()));
    c.bench_function("simple_joined", |b| b.iter(|| simple_joined_bench()));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
