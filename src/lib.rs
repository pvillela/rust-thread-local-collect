#![deny(clippy::unwrap_used, clippy::expect_used)]
#![allow(clippy::type_complexity)]

//! This library supports the ***collection*** and ***aggregation*** of the values of a designated thread-local variable across threads.
//!
//! An aggregation operation is applied to the values collected from the the thread-local variables and the resulting accumulated value is made available to the library's caller.
//!
//! This library contains multiple modules ([`joined`], [`simple_joined`], [`probed`], [`channeled`], and [`tlcr`]), with varying features and constraints, that support thread-local variable *collection* and *aggregation*.
//!
//! ## Core concepts
//!
//! The core concepts in this library are the `Control` struct, the `Holder` struct, and the `HolderLocalKey` trait.
//! The library modules provide specific implementations of these core concepts.
//!
//! `Holder` wraps a thread-local value and ensures that each such variable, when used, is linked with `Control`. In the case of modules [`joined`], [`simple_joined`], and [`probed`], `Holder` notifies `Control` when the `Holder` instance is dropped upon thread termination. In the case of module [`channeled`], `Holder` contains a channel [`Sender`](std::sync::mpsc::Sender) that sends values to be aggregated by `Control`.
//!
//! `Control` keeps track of the linked thread-local variables, contains an accumulation operation `op` and an accumulated value `acc`, and provides methods to access the accumulated value. The accumulated value is updated by applying `op` to each thread-local data value and `acc` when the thread-local value is collected. Depending on the specific module, thread-local values are collected when the `Holder` value is dropped and/or when collection is initiated by a  method on the `Control` object, or when the data value is sent by `Holder` to `Control` on a channel.
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

#[doc(hidden)]
#[cfg(feature = "old")]
pub mod joined_old;

#[cfg(feature = "tlcr")]
pub mod tlcr;
