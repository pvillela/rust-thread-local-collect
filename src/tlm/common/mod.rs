//! Common traits and structs that support the [`crate::tlm`] sub-modules, except [`crate::tlm::channeled`].

mod common_traits;
pub use common_traits::*;

mod control_g;
pub use control_g::*;

mod holder_g;
pub use holder_g::*;
