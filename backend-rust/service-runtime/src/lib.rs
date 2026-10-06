// baal-service-runtime: Rust port of the shared JS runtime layer that every
// live-paper/feed/execution service built on — journals, locks, atomic files,
// canonical JSON, rate budgets, read client, timing and history validation.
// Behaviors are byte/semantic ports of the referenced .mjs modules.
pub mod atomic;
pub mod budget;
pub mod client;
pub mod contract;
pub mod fixed;
pub mod exclusions;
pub mod history;
pub mod journal;
pub mod json_text;
pub mod market;
pub mod ledger;
pub mod timing;
pub mod wait;

pub const SERVICE_RUNTIME_HASH: &str = env!("BAAL_SERVICE_RUNTIME_HASH");

#[cfg(test)]
mod tests;
