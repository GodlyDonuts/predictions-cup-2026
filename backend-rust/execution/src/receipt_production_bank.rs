// execution/receipt-production-bank.mjs — produces a reviewable immutable
// strategy candidate; creates no activation, handoff, position, intent, client
// or order. Ported in crate::production (a prior batch combined the production
// modules); re-exported here so the module name matches the JS module.
pub use crate::production::{prepare_receipt_production_bank, RECEIPT_PRODUCTION_POLICY};
