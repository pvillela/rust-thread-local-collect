//! Modules that use the [`std::thread_local`] macro.

pub mod channeled;
pub mod common;
pub mod joined;
pub mod probed;
pub mod simple_joined;

pub mod control_send;
