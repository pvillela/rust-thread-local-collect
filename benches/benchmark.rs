//! Execution of benchmarks for different modules.
//! The modules are selected based on the environment variable `TARGET_ARGS`, which is set by script `bench.sh`.

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
    let target_args = env::var("TARGET_ARGS").expect("`TARGET_ARGS` is set by script `bench.sh`");
    let names: Vec<&str> = target_args.split_whitespace().collect();
    println!("target names={names:?}");
    // force early validation of all target names
    let targets: Vec<fn()> = names.iter().map(|name| target(name)).collect();

    for (name, target) in names.into_iter().zip(targets.into_iter()) {
        c.bench_function(name, |b| b.iter(target));
    }
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
