// execution/entry-gates.mjs — exact rational comparison of decimal lexemes.
// Ported in crate::gates (which also covers api-tick-price and production
// entry state); re-exported here so the module name matches the JS module.
pub use crate::gates::evaluate_entry_gates;
