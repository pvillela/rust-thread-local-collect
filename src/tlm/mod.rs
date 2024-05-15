//! Modules that use the [`std::thread_local`] macro.

pub mod common;
pub mod send;
pub mod tmap_d;

pub mod channeled;
pub mod joined;
pub mod probed;
pub mod simple_joined;
