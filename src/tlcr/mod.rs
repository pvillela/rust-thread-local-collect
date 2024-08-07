//! Modules that use the [`thread_local`](https://docs.rs/thread_local/latest/thread_local/) crate. These
//! modules require the **"tlcr"** feature.

pub mod joined;
pub mod probed;
