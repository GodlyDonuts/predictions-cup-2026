// execution/production-activation.mjs — production activation + handoff
// validation. Ported in crate::production (a prior batch combined the
// production modules); re-exported here so the module name matches the JS
// module.
pub use crate::production::{
    production_authority_fields, production_handoff_current, validate_production_activation,
    validate_production_handoff,
};
