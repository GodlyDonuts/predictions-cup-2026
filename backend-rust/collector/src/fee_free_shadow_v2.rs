// live-paper/fee-free-shadow-v2.mjs — identical exports to fee-free-shadow.mjs;
// the CLI variant swaps verifySource(full read) for readIncrementalSource and
// adds sourceReadMetrics to the published stats.
use crate::ml_shadow::VerifiedSource;
use serde_json::Value as V;
use std::cell::RefCell;
use std::path::Path;

pub use crate::fee_free_shadow::{advance_fee_free, create_fee_free, fee_free_report};

/// fee-free-shadow-v2.mjs CLI: incremental verified reader + sourceReadMetrics.
pub fn run_fee_free(dir: &Path, workspace_dir: &Path, once: bool) -> Result<(), String> {
    let source_file = dir.join("journal.jsonl");
    let reader = RefCell::new(crate::incremental_journal::VerifiedSourceReader::new(&source_file, 1_048_576, 16_777_216));
    // JS readIncrementalSource lazily creates the reader on first call; until
    // then incrementalSourceMetrics returns null. Mirrors that: metrics is
    // None until the first read attempt (which persists even on read error).
    let has_read = std::cell::Cell::new(false);
    crate::fee_free_shadow::run_fee_free_with_source(dir, workspace_dir, once,
        || {
            has_read.set(true);
            let mut r = reader.borrow_mut();
            let opt = r.read()?;
            let prefix_hashes = r.prefix_hashes.clone();
            Ok(opt.map(|rows| VerifiedSource { records: rows, prefix_hashes }))
        },
        || Some(if has_read.get() { reader.borrow().metrics() } else { V::Null }))
}
