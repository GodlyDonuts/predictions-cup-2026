// live-paper/latency-clock.mjs — ported at baal_service_runtime::timing
// (a sampler thread measuring timer-scheduling delay stands in for Node's
// monitorEventLoopDelay histogram).
pub use baal_service_runtime::timing::LatencyClock;
