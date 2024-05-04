#![deny(clippy::unwrap_used)]
#![allow(clippy::type_complexity, clippy::new_without_default)]

//! This library supports the **_collection_** and **_aggregation_** of thread-local data across threads.
//!
//! An aggregation operation is applied to the collected thread-local values and the resulting accumulated value is made available to the library's caller. This library contains multiple modules, with varying features and constraints, grouped as sub-modules of two top-level modules: [`tlm`] and [`tlcr`].
//!
//! The sub-modules of [`tlm`] use the [`std::thread_local`] macro and [`tlcr`] sub-modules use the excellent [`thread_local`](https://docs.rs/thread_local/latest/thread_local/) crate. The [`tlcr`] sub-modules provide the simplest and most ergonomic implementation.
//!
//! ## Core concepts
//!
//! The primary core concept in this library is the **`Control`** struct, which has a specific implementation for each sub-module. `Control` keeps track of the linked thread-local values, contains an accumulation operation `op` and an accumulated value `acc`, and provides methods to access the accumulated value. The accumulated value is updated by applying `op` to each thread-local data value and `acc` when the thread-local value is collected.
//!
//! ### `Holder` struct for [`tlm`] sub-modules
//!
//! In addition to **`Control`**, [`tlm`] sub-modules rely on the **`Holder`** struct. The sub-modules provide specific implementations of these core concepts.
//!
//! `Holder` wraps a thread-local value and ensures that each such variable, when used, is linked with `Control`. In the case of modules [`tlm::joined`], [`tlm::simple_joined`], and [`tlm::probed`], `Holder` notifies `Control` when the `Holder` instance is dropped upon thread termination. In the case of module [`tlm::channeled`], `Holder` contains a channel [`Sender`](std::sync::mpsc::Sender) that sends values to be aggregated by `Control`.
//!
//! Depending on the specific module, thread-local values are collected when the `Holder` value is dropped and/or when collection is initiated by a method on the `Control` object, or when the data value is _sent_ to `Control` on a channel.
//!
//! ## Usage examples
//!
//! See the different modules for usage examples.

pub mod test_support;
pub mod tlcr;
pub mod tlm;
