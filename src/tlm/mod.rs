//! Modules that use the [`std::thread_local`] macro.

pub(crate) mod common;
pub mod send;

#[doc(hidden)]
pub(crate) mod tmap_d;

pub mod channeled;
pub mod joined;
pub mod probed;
pub mod simple_joined;
