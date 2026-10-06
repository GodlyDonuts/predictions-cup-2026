// data/prepare-nh-publication-20261004.mjs
use crate::publication_pack::{elements, write_pack};
use serde_json::Value as V;

const FILES: &[&str] = &[
    "live-paper/no-maker-paper-v3.mjs", "live-paper/no-maker-paper-v3.test.mjs",
    "live-paper/no-maker-host-v5.mjs", "live-paper/no-maker-host-v5.test.mjs",
    "live-paper/quote-projection-recovery.mjs", "live-paper/quote-projection-recovery.test.mjs",
    "live-paper/canonical-no-capacity-v2.mjs", "live-paper/canonical-no-capacity-v2.test.mjs",
    "live-paper/baal-pair-host-v3.mjs", "live-paper/baal-pair-host-v3.test.mjs",
    "live-paper/paper-book-continuation-frontier.mjs", "live-paper/paper-book-continuation-frontier.test.mjs",
    "live-paper/runner.mjs", "live-paper/trade-tape.mjs",
    "data/prepare-nh-paper-v5-20261004.mjs", "data/deploy-nh-paper-v5-20261004.ps1",
    "data/verify-nh-paper-v5-20261004.mjs", "data/record-nh-milestone-20261004.mjs",
    "data/prepare-nh-publication-20261004.mjs", "data/nh-source-capture-20261004.json",
    "data/nh-conditional-refund-review-20261004.json", "data/no-maker-bank-20261004-v5.json",
    "data/baal-pair-bank-20261004-v3.json", "data/nh-paper-v5-preservation-20261004.json",
    "data/nh-paper-v5-deployment-20261004.json", "data/nh-paper-v5-live-20261004.json",
    "data/nh-paper-v5-milestone-20261004.json", "docs/BAAL.md",
];
pub fn run() -> Result<V, String> {
    let els = elements(FILES, crate::secret_scan::ace_jwt_check, |f| format!("Secret-like content: {f}"))?;
    write_pack("data/nh-publication-elements.json", &els, false)
}
