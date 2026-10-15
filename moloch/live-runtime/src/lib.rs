//! Live account feedback is derived only from scoped, owned official receipts.
pub mod feedback;
pub mod authority;
pub mod daily_windows;
pub mod dispatch;
pub mod breadth;
pub mod read_recovery;
pub mod universe;
pub mod handoff;
pub mod inventory;
pub mod relationship_census;
pub mod relationship_strategy;
pub mod reconciliation_plan;
pub mod reconciliation;
pub mod dispatch_window;

pub mod scheduling;
pub mod exit_refresh;
pub mod stream_recovery;
