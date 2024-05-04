#![allow(unused)] // needed due to linting bug in Rust Analyzer for benches

mod common_bench;
pub use common_bench::*;

mod tlm_joined_bench;
pub use tlm_joined_bench::*;

mod tlm_simple_joined_bench;
pub use tlm_simple_joined_bench::*;

mod tlm_probed_bench;
pub use tlm_probed_bench::*;

mod tlm_channeled_bench;
pub use tlm_channeled_bench::*;

mod tlcr_bench;
pub use tlcr_bench::*;

mod tlcr_probed_bench;
pub use tlcr_probed_bench::*;
