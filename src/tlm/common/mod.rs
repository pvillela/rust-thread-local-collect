//! Modules that use the [`std::thread_local`] macro.

mod common_traits;
pub use common_traits::*;

mod control_g;
pub use control_g::*;

mod holder_g;
pub use holder_g::*;
