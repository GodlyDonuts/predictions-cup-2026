// live-paper/collector-status-writer.mjs — derived UI/health projection writer.
// These are derived UI/health projections. Preserve their existing JSON shape
// (including omission of optional undefined fields) before the durable writer.
// Authoritative journals, accounting and model bindings do not use this adapter.
use baal_service_runtime::atomic::atomic_json_canonical;
use serde_json::Value as V;
use std::path::Path;

/// writeCollectorStatus(file, value) — atomicJson is the execution/ledger.mjs
/// canonical atomic write. JSON.parse(JSON.stringify(value)) only drops
/// undefined fields; a serde Value is already plain JSON.
pub fn write_collector_status(file: &Path, value: &V) -> Result<(), String> {
    atomic_json_canonical(file, &value.clone())
}
