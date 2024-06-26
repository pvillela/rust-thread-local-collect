#![deny(clippy::unwrap_used)]
#![allow(clippy::type_complexity, clippy::new_without_default)]
#![doc = include_str!("lib.md")]

pub mod tlm;

#[cfg(feature = "tlcr")]
pub mod tlcr;

#[doc(hidden)]
pub mod dev_support;
