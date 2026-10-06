extern crate self as baal_median_dip_replay;
include!("frozen_kernel.rs");
mod frozen_observe;
pub mod frontiers;
pub use frozen_observe::{ReadOnlyConfig,read_only_replay,ContinuationConfig,run_continuation};
