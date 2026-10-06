// execution/baal-scaled-live-host-v1.mjs — scaled live production host.
// Only baalScaledLiveIdentity is ported here: the data batch only needs the
// frozen code identity. openBaalScaledLiveHost additionally depends on the
// still-unported scaled controller, shared budget and evidence bridge modules
// (baal-scaled-no-controller-v1, baal-scaled-shared-budget-v1,
// baal-scaled-no-evidence-bridge-v1); porting it is a separate follow-up.
use baal_collector::frozen_paper_graph::collect_frozen_paper_files;
use serde_json::{json, Value as V};
use std::path::Path;

// baalScaledLiveIdentity(workspace): frozen source hashes for this host plus
// the scoped receipt decoder it loads.
pub fn baal_scaled_live_identity(workspace: &Path) -> Result<V, String> {
    Ok(json!(collect_frozen_paper_files(
        workspace,
        &[
            "execution/baal-scaled-live-host-v1.mjs".to_string(),
            "execution/scoped-rest-receipts-v2.mjs".to_string(),
        ],
    )?))
}
