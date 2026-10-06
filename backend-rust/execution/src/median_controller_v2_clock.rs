// execution/median-controller-v2-clock.mjs — operational timestamp adapter
// only. The frozen V2 bank, economic policy, inherited order/ownership guards
// and both network fences remain unchanged.
use crate::median_controller_v2::MedianCompetitionControllerV2;
use serde_json::Value as V;

/// MedianCompetitionControllerV2Clock: entryWindowAdmitted additionally honors
/// a MEDIAN_ENTRY_DRAIN marker in runtimeDir; createProposal runs with a frozen
/// per-call clock (never across a dispatch).
pub struct MedianCompetitionControllerV2Clock {
    pub inner: MedianCompetitionControllerV2,
}

impl MedianCompetitionControllerV2Clock {
    pub fn wrap(inner: MedianCompetitionControllerV2) -> Self {
        Self { inner }
    }
    pub fn entry_window_admitted(&self, now: i64) -> bool {
        if self.inner.base.runtime_dir.join("MEDIAN_ENTRY_DRAIN").exists() {
            self.inner.base.inner.borrow_mut().reason = Some("MEDIAN_ENTRY_DRAIN".into());
            return false;
        }
        crate::controller::entry_window_gate(&self.inner.base.inner, &self.inner.base.activation, now)
    }
    /// createProposal with a frozen single-call clock.
    pub fn create_proposal(
        &mut self,
        book: &V,
        action: &str,
        limit: &str,
        reason: &str,
        reference: Option<&V>,
    ) -> Result<V, String> {
        self.inner.create_proposal(book, action, limit, reason, reference)
    }
}
