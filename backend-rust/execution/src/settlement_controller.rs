// execution/settlement-controller.mjs — settlement-bank production controller.
// Ported in crate::settlement (a prior batch combined it with
// live-paper/native-publication.mjs and live-paper/settlement-value-native.mjs);
// re-exported here so the one-file-per-module mapping resolves to a single
// implementation rather than a divergent copy.
pub use crate::settlement::{validate_settlement_bank, SettlementCompetitionController, SettlementState};
