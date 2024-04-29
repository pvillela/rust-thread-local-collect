#![deny(clippy::unwrap_used)]
#![allow(clippy::type_complexity, clippy::new_without_default)]

//! This library supports the **_collection_** and **_aggregation_** of thread-local data across threads.
//!
//! An aggregation operation is applied to the collected thread-local values and the resulting accumulated value is made available to the library's caller. This library contains multiple modules ([`joined`], [`simple_joined`], [`probed`], [`channeled`], and [`tlcr`]), with varying features and constraints. The modules in this library are a study in how to collect and aggregate thread-local values using different techniques.
//!
//! All of the library modules, except for [`tlcr`], use thread-local variables. The [`tlcr`] module, which uses the excellent [`thread_local`](https://docs.rs/thread_local/latest/thread_local/) crate instead of thread-local variables, provides the simplest and most ergonomic implementation.
//!
//! ## Core concepts
//!
//! The three core concepts in this library are the `Control` struct, the `Holder` struct, and the `HolderLocalKey` trait. (The latter two concepts are not applicable to the [`tlcr`] module). The library modules provide specific implementations of these core concepts.
//!
//! `Holder` wraps a thread-local value and ensures that each such variable, when used, is linked with `Control`. In the case of modules [`joined`], [`simple_joined`], and [`probed`], `Holder` notifies `Control` when the `Holder` instance is dropped upon thread termination. In the case of module [`channeled`], `Holder` contains a channel [`Sender`](std::sync::mpsc::Sender) that sends values to be aggregated by `Control`. In the case of module [`tlcr`], there is no`Holder`; instead, `Control` contains a [`ThreadLocal`](https://docs.rs/thread_local/latest/thread_local/) object which is accessible from the threads and is used to collect and aggregate values.
//!
//! `Control` keeps track of the linked thread-local values, contains an accumulation operation `op` and an accumulated value `acc`, and provides methods to access the accumulated value. The accumulated value is updated by applying `op` to each thread-local data value and `acc` when the thread-local value is collected. Depending on the specific module, thread-local values are collected when the `Holder` value is dropped and/or when collection is initiated by a method on the `Control` object, or when the data value is _sent_ to `Control` on a channel or via a shared [`ThreadLocal`](https://docs.rs/thread_local/latest/thread_local/).
//!
//! Implementations of the `HolderLocalKey` trait for [`LocalKey<Holder>`](std::thread::LocalKey) provide methods to conveniently access the thread-local variables. (Recall that [`LocalKey`](std::thread::LocalKey) is the type underlying all thread-local variables.)
//!
//! ## Usage examples
//!
//! See the different modules for usage examples.

pub mod channeled;
pub mod common;
pub mod joined;
pub mod probed;
pub mod simple_joined;
pub mod test_support;

#[cfg(feature = "tlcr")]
pub mod tlcr;

#[cfg(feature = "tlcr")]
pub mod tlcr_probed;
