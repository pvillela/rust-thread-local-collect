#![allow(unused)] // needed due to linting bug in Rust Analyzer for benches

mod common;
use common::*;

mod tlm_joined_bench;
pub use tlm_joined_bench::*;

mod tlm_simple_joined_bench;
pub use tlm_simple_joined_bench::*;

mod tlm_probed_bench;
pub use tlm_probed_bench::*;

mod tlm_channeled_bench;
pub use tlm_channeled_bench::*;

mod tlcr_joined_bench;
pub use tlcr_joined_bench::*;

mod tlcr_probed_bench;
pub use tlcr_probed_bench::*;

mod tlmrestr_joined_bench;
pub use tlmrestr_joined_bench::*;

mod tlmrestr_probed_bench;
pub use tlmrestr_probed_bench::*;

mod tlmrestr_simple_joined_bench;
pub use tlmrestr_simple_joined_bench::*;
