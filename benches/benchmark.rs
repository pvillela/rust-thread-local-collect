//! Execution of benchmarks for different modules with u32 accumulators.
//! This excludes [common::channeled_bg_u32_bench] which is very slow.

mod common;

use common::{
    channeled_bg_map_bench, channeled_bg_u32_bench, channeled_nobg_map_bench,
    channeled_nobg_u32_bench, joined_map_bench, joined_u32_bench, probed_map_bench,
    probed_u32_bench, simple_joined_map_bench, simple_joined_u32_bench, tlcr_map_bench,
    tlcr_probed_map_bench, tlcr_probed_u32_bench, tlcr_u32_bench,
};
use criterion::{criterion_group, criterion_main, Criterion};
use std::env;

fn target(name: &str) -> fn() {
    match name {
        "joined_u32" => joined_u32_bench,
        "simple_joined_u32" => simple_joined_u32_bench,
        "probed_u32" => probed_u32_bench,
        "tlcr_u32" => tlcr_u32_bench,
        "tlcr_probed_u32" => tlcr_probed_u32_bench,
        "channeled_nobg_u32" => channeled_nobg_u32_bench,
        "channeled_bg_u32" => channeled_bg_u32_bench,

        "joined_map" => joined_map_bench,
        "simple_joined_map" => simple_joined_map_bench,
        "probed_map" => probed_map_bench,
        "tlcr_map" => tlcr_map_bench,
        "tlcr_probed_map" => tlcr_probed_map_bench,
        "channeled_nobg_map" => channeled_nobg_map_bench,
        "channeled_bg_map" => channeled_bg_map_bench,

        _ => panic!("Invalid target name: {name}"),
    }
}

fn criterion_benchmark(c: &mut Criterion) {
    let target_args = env::var("TARGET_ARGS").unwrap();
    let names: Vec<&str> = target_args.split_whitespace().collect();
    println!("target names={names:?}");

    for name in names {
        c.bench_function(name, |b| b.iter(target(name)));
    }
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
