// live-paper/shock-shadow-v2.mjs — identical exports to shock-shadow.mjs;
// the runShock variant swaps verifySource for readIncrementalSource and
// appends sourceReadMetrics to the published stats.
use crate::ml_shadow::VerifiedSource;
use serde_json::Value as V;
use std::cell::RefCell;
use std::path::Path;

pub use crate::shock_shadow::{advance_shock, create_shock, shock_code_identity, shock_report};

/// runShock with v2 journal semantics: persistent VerifiedSourceReader on
/// dir/journal.jsonl, sourceReadMetrics in every published stats object.
pub fn run_shock(dir: &Path, initialize: bool, once: bool, workspace_dir: &Path, now: &dyn Fn() -> i64) -> Result<V, String> {
    let source_file = dir.join("journal.jsonl");
    let reader = RefCell::new(crate::incremental_journal::VerifiedSourceReader::new(&source_file, 1_048_576, 16_777_216));
    // JS readIncrementalSource lazily creates the reader on first call; until
    // then incrementalSourceMetrics returns null. Mirrors that: metrics is
    // None until the first read attempt (which persists even on read error).
    let has_read = std::cell::Cell::new(false);
    crate::shock_shadow::run_shock_with_source(dir, initialize, once, workspace_dir, now,
        || {
            has_read.set(true);
            let mut r = reader.borrow_mut();
            let opt = r.read()?;
            let prefix_hashes = r.prefix_hashes.clone();
            Ok(opt.map(|rows| VerifiedSource { records: rows, prefix_hashes }))
        },
        || Some(if has_read.get() { reader.borrow().metrics() } else { V::Null }))
}
