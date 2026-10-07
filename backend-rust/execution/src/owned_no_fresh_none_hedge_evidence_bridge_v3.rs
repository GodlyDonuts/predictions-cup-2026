//! Port of execution/owned-no-fresh-none-hedge-evidence-bridge-v3.mjs.
//! Revision V3 separates mechanical read identity checks (canCheckReadIdentity,
//! used by identity()) from the full financial admission check (canRead,
//! asserted inside guard()). It also records per-read timings (readTimings,
//! capped at 20). The implementation lives alongside V2 in
//! owned_no_fresh_none_hedge_evidence_bridge_v2.rs — the shared body matches
//! the JS v2/v3 delta exactly.

pub use crate::owned_no_fresh_none_hedge_evidence_bridge_v1::{
    EvidenceBridgeApi, OwnedNoFreshNoneBridgeDeps,
};
pub use crate::owned_no_fresh_none_hedge_evidence_bridge_v2::{
    create_owned_no_fresh_none_hedge_evidence_bridge_v3, CombinedNoneEvidenceDeps,
    OwnedNoFreshNoneHedgeEvidenceBridgeV2 as OwnedNoFreshNoneHedgeEvidenceBridgeV3,
};
