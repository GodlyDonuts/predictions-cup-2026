// baal-ml modules: ml/*.mjs offline book-feature baseline port.
#![allow(clippy::too_many_arguments)]
pub mod cli;
pub mod core;
pub mod experiment;
pub mod generate_fixture;
pub mod test__helpers;
pub mod model;

pub const ML_HASH: &str = env!("BAAL_ML_HASH");
