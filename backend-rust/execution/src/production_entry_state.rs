// execution/production-entry-state.mjs — derived entry-window status only;
// dispatch authority still lives in the controller fences. Ported in
// crate::gates (which also covers api-tick-price and entry gates); re-exported
// here so the module name matches the JS module.
pub use crate::gates::production_entry_state;
