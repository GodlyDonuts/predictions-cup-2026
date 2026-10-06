// live-paper/ml-shadow-v2.mjs — identical exports to ml-shadow.mjs;
// the CLI variant swaps verifySource for readIncrementalSource and appends
// sourceReadMetrics to the published stats. The library surface is re-exported
// here so downstream modules can depend on the v2 name directly.
pub use crate::ml_shadow::{
    advance_shadow, bind_training_source, combine_reports, require_source_metadata, shadow_report,
    verify_source, VerifiedSource,
};
