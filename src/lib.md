# thread_local_collect

This library supports the **_collection_** and **_aggregation_** of thread-local data across threads. It provides several modules, each of which accomplishes the aforementioned task in a different way. See [below](#comparative-overview-of-modules) for a comparison of the various modules. For most use cases, either the [`tlm::probed`] module or the [`tlcr::probed`] would be a good choice -- the former is a little more flexible, the latter has a little better ergonomics .

An aggregation operation is applied to the collected thread-local values and the resulting accumulated value is made available to the library's caller. This library contains multiple modules, with varying capabilities and constraints, grouped as sub-modules of two top-level modules: [`tlm`] and [`tlcr`].

The sub-modules of [`tlm`] use the [`std::thread_local`] macro and [`tlcr`] sub-modules use the excellent [`thread_local`](https://docs.rs/thread_local/latest/thread_local/) crate. The [`tlcr`] sub-modules have a simpler implementation but are also somewhat more restrictive as the thread-local values and aggregated value must be of the same type.

## Core concepts

The primary core concept in this library is the **`Control`** struct, which has a specific implementation for each sub-module. `Control` keeps track of the linked thread-local values, contains an accumulated value `acc`, and provides methods to access the accumulated value. The accumulated value is updated by applying an aggregation operation to each thread-local data value and `acc` when the thread-local value is collected.

### `Holder` struct for [`tlm`] sub-modules

In addition to **`Control`**, [`tlm`] sub-modules rely on the **`Holder`** struct. The sub-modules provide specific implementations of these core concepts.

A thread-local variable of type `Holder` wraps a thread-local value and ensures that each such variable, when used, is linked with `Control`. In the case of modules [`tlm::joined`], [`tlm::simple_joined`], and [`tlm::probed`], `Holder` notifies `Control` when the `Holder` instance is dropped upon thread termination. In the case of module [`tlm::channeled`], `Holder` contains a channel [`Sender`](std::sync::mpsc::Sender) that sends values to be aggregated by `Control`.

Depending on the specific module, thread-local values are collected when the `Holder` value is dropped and/or when collection is initiated by a method on the `Control` object, or when the data value is _sent_ to `Control` on a channel.

## Rust version requirements

This version of this library can be compiled with rustc 1.79.0 or higher. It may work with earlier rustc versions but that is not guaranteed.

## Usage examples

The documentation of the modules [listed below](#comparative-overview-of-modules) includes simple usage examples.

The [latency_trace](https://crates.io/crates/latency_trace) library is a substantial use case for this package (it uses the [`tlm::probed`] module).

## Default cargo feature

To include this library as a dependency without optional features in your Cargo.toml:

```toml
[dependencies]
thread_local_collect = "1"
```

For the default feature, this library only depends on the `std` library and the [`log`](https://crates.io/crates/log) crate.

## Optional cargo features

The optional feature flag "tlcr" enables module [`tlcr`] and its sub-modules.

```toml
[dependencies]
thread_local_collect = { version = "1", features = ["tlcr"] }
```

To run the `tlcr_*` executables from the `examples` directory of the cloned/downloaded source [repo](https://github.com/pvillela/rust-thread-local-collect/tree/main), specify `--features tlcr` or `--all-features` when invoking `cargo run`. For example, to run `tlcr_joined_map_accumulator.rs`, do as follows:

```bash
cargo run --features tlcr --example tlcr_joined_map_accumulator
```

or

```bash
cargo run --all-features --example tlcr_joined_map_accumulator
```

Likewise, specify `--features tlcr` or `--all-features` when executing benchmarks involving the `tlcr` sub-modules.

## Comparative overview of modules

### [`tlm`] direct sub-modules

These modules use thread-local static variables defined with the [`std::thread_local`] macro.

- [`tlm::joined`] -- The values of linked thread-local variables are collected and aggregated into the control object’s accumulated value when the thread-local variables are dropped following thread termination. After all participating threads other than the thread responsible for collection/aggregation have terminated and EXPLICITLY joined, directly or indirectly, into the thread responsible for collection, the accumulated value may be retrieved.
- [`tlm::probed`] -- Similar to [`tlm::joined`] but the final accumulated value may be retrieved after all threads other than the one responsible for collection/aggregation have terminated (joins are not necessary), and this module also allows a partial accumulation of thread-local values to be inspected before the various threads have terminated. Given its flexibility and benchmarking results, this module is a good choice for most use cases.
- [`tlm::simple_joined`] -- This is a simplified implementation of [`tlm::joined`] that does not aggregate the value from the thread-local variable for the thread responsible for collection/aggregation.
- [`tlm::channeled`] -- Unlike the above modules, values in thread-local variables are not collected when the threads terminate and join. Instead, threads use a thread-local channel [`Sender`](std::sync::mpsc::Sender) to send values for aggregation by the control object. Partial aggregations may be inspected before the various threads have terminated.

### [`tlcr`] sub-modules

These-modules use the [`ThreadLocal`](https://docs.rs/thread_local/latest/thread_local/struct.ThreadLocal.html) object instead of thread-local static variables and require the **"tlcr"** feature.

- [`tlcr::joined`] -- The participating threads update thread-local data via the control object which contains a [`ThreadLocal`](https://docs.rs/thread_local/latest/thread_local/) instance and aggregates the values. After all participating threads other than the thread responsible for collection/aggregation have terminated (joins are not necessary), the accumulated value may be retrieved.
- [`tlcr::probed`] -- Similar to [`tlcr::joined`], but this module also allows a partial accumulation of thread-local values to be inspected before the threads have terminated. Given its relative flexibility and benchmarking results, this module is a good choice for many use cases.

### [`tlm::restr`] sub-modules

These modules use a wrapper around the corresponding [`tlm`] sub-modules to provide an API similar to that of the [`tlcr`] sub-modules.

- [`tlm::restr::joined`] -- Wrapper of [`tlm::joined`] providing an API and capabilities similar to those of [`tlcr::joined`].
- [`tlm::restr::probed`] -- Wrapper of [`tlm::probed`] providing an API and capabilities similar to those of [`tlcr::probed`].
- [`tlm::restr::simple_joined`] -- Wrapper of [`tlm::simple_joined`] providing an API and capabilities similar to those of [`tlcr::joined`], but without the ability to aggregate values from the thread responsible for collection/aggregation.

## Benchmarks

Running the benchmarks defined in the [benches](https://github.com/pvillela/rust-thread-local-collect/tree/main/benches) directory of the repo on my laptop, all the different modules have essentially indistinguishable performance, except for the [`tlm::channeled`] module which performs worse than all the others.

When a background receiver thread is not used, the `channeled` module performs slightly worse than the others, but its performance is orders of magnitude worse when a background receiver thread is used. The only reason to use a background receiver thread would be to keep the channel buffer from expanding without bounds during execution of the participating threads. This is not necessary unless there is a risk of memory overflow arising from the amount of data sent from the participating threads.

It is worth observing that the `probed` modules do not exhibit performance materially different from that of `joined` or `simple_joined`. Although the `probed` modules use mutexes, the mutexes are uncontended except in rare situations.
