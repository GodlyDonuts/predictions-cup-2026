// live-paper/canonical-capacity-guard.mjs — external high-water guard wrapping
// the canonical capacity monitor. The frozen evaluator, bank and original
// journal stay intact; the guard only enforces the durable tail.
use baal_execution::core::payload_hash;
use baal_service_runtime::json_text::{js_int, sha256};
use serde_json::{json, Value as V};
use std::fs;
use std::path::{Path, PathBuf};

use crate::canonical_capacity_monitor::CanonicalCapacityMonitor;

fn check(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("CAPACITY_GUARD_{}", code)); } Ok(()) }
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(_), V::Number(_)) => a.as_f64() == b.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
// JS `<=`/`>=` on possibly-absent values: undefined -> NaN.
fn js_num(v: Option<&V>) -> f64 {
    v.and_then(|v| v.as_f64()).unwrap_or(f64::NAN)
}

pub struct CanonicalCapacityGuard {
    monitor: CanonicalCapacityMonitor,
    file: PathBuf,
    anchor_file: PathBuf,
    stats_file: PathBuf,
    anchor: Option<V>,
}

/// guardCanonicalCapacity(monitor)
pub fn guard_canonical_capacity(monitor: Option<CanonicalCapacityMonitor>) -> Result<Option<CanonicalCapacityGuard>, String> {
    let Some(monitor) = monitor else { return Ok(None); };
    let file = monitor.dir.join("journal.jsonl");
    let anchor_file = monitor.dir.join("collector-high-water.json");
    let stats_file = monitor.dir.join("stats.json");
    let mut guard = CanonicalCapacityGuard { monitor, file, anchor_file, stats_file, anchor: None };
    if guard.anchor_file.exists() {
        let row: V = serde_json::from_str(&fs::read_to_string(&guard.anchor_file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let mut body = row.as_object().cloned().unwrap_or_default();
        let hash = body.remove("hash").unwrap_or(V::Null);
        let body = V::Object(body);
        check(js_int(&body["schemaVersion"]) == Some(1)
            && js_int(&body["bytes"]).is_some_and(|b| b >= 0)
            && js_int(&body["sequence"]).is_some_and(|s| s >= 0)
            && strict_eq(&json!(payload_hash(&body)?), &hash), "ANCHOR_INVALID")?;
        guard.anchor = Some(body);
    }
    // Bootstrap from the previously published tail too: a surviving stats
    // projection can expose truncation that occurred before this external
    // guard was introduced.
    if guard.stats_file.exists() {
        let prior: V = serde_json::from_str(&fs::read_to_string(&guard.stats_file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let current = guard.monitor.status()?;
        check(strict_eq(&prior["cohort"], &current["cohort"])
            && strict_eq(&prior["bankHash"], &current["bankHash"])
            && js_int(&prior["journalSequence"]).is_some_and(|s| s <= current["journalSequence"].as_i64().unwrap_or(i64::MIN)),
            "PUBLISHED_TAIL_ROLLBACK")?;
        if strict_eq(&prior["journalSequence"], &current["journalSequence"]) {
            check(strict_eq(&prior["journalHash"], &current["journalHash"]), "PUBLISHED_TAIL_CHANGED")?;
        }
    }
    guard.checkpoint()?;
    Ok(Some(guard))
}

impl CanonicalCapacityGuard {
    pub fn bank(&self) -> &V { &self.monitor.bank }
    pub fn bank_hash(&self) -> &str { &self.monitor.bank_hash }
    pub fn dir(&self) -> &Path { &self.monitor.dir }
    pub fn monitor(&self) -> &CanonicalCapacityMonitor { &self.monitor }

    fn verify(&self) -> Result<(V, Vec<u8>), String> {
        let status = self.monitor.status()?;
        let bytes = fs::read(&self.file).map_err(|e| e.to_string())?;
        if let Some(anchor) = &self.anchor {
            check(strict_eq(&status["cohort"], &anchor["cohort"])
                && strict_eq(&status["bankHash"], &anchor["bankHash"]), "IDENTITY")?;
            let abytes = anchor["bytes"].as_u64().unwrap_or(0) as usize;
            check(bytes.len() >= abytes
                && sha256(&bytes[..abytes.min(bytes.len())]) == anchor["prefixHash"].as_str().unwrap_or("")
                && js_num(status.get("journalSequence")) >= js_num(anchor.get("sequence")), "JOURNAL_ROLLBACK")?;
            if strict_eq(&status["journalSequence"], &anchor["sequence"]) {
                check(strict_eq(&status["journalHash"], &anchor["tailHash"]), "TAIL_CHANGED")?;
            }
        }
        Ok((status, bytes))
    }
    fn checkpoint(&mut self) -> Result<(), String> {
        let (status, bytes) = self.verify()?;
        let body = json!({"schemaVersion": 1, "cohort": status["cohort"].clone(), "bankHash": status["bankHash"].clone(),
            "bytes": bytes.len(), "prefixHash": sha256(&bytes),
            "sequence": status["journalSequence"].clone(), "tailHash": status["journalHash"].clone()});
        let write = payload_hash(&body).map(|h| {
            let mut row = body.clone();
            row.as_object_mut().unwrap().insert("hash".into(), json!(h));
            row
        }).and_then(|row| baal_service_runtime::atomic::atomic_json_canonical(&self.anchor_file, &row));
        if write.is_err() {
            return Err("CAPACITY_GUARD_DURABILITY".into());
        }
        self.anchor = Some(body);
        Ok(())
    }
    /// status() — monitor projection plus the external high-water flag.
    pub fn status(&self) -> Result<V, String> {
        let mut s = self.monitor.status()?;
        s.as_object_mut().unwrap().insert("externalHighWater".into(), json!(true));
        Ok(s)
    }
    /// inspect(publication) — verify the tail, delegate to the monitor, then
    /// re-checkpoint when the journal changed.
    pub fn inspect(&mut self, publication: &V) -> Result<bool, String> {
        self.verify()?;
        let changed = self.monitor.inspect(publication)?;
        if changed {
            self.checkpoint()?;
        }
        Ok(changed)
    }
    /// plan(base, context) — the monitor's substitution is allowed only when
    /// it never moves a held-risk read and lands an adjacent pair.
    pub fn plan(&self, base: &V, context: &V) -> Result<V, String> {
        self.verify()?;
        let next = self.monitor.plan(base, context)?;
        let ids = next["canonicalCapacityFocus"]["marketIds"].as_array().cloned().unwrap_or_default();
        if !ids.is_empty() {
            const OPTIONAL: [&str; 3] = ["discovery", "comparison", "edge_observation"];
            if !base["reads"].is_array() {
                return Err("base.reads.filter is not a function".into());
            }
            let protected_ids: Vec<V> = base["reads"].as_array().cloned().unwrap_or_default().iter()
                .filter(|x| !OPTIONAL.contains(&x["reason"].as_str().unwrap_or("")))
                .map(|x| x["marketId"].clone()).collect();
            let next_reads = next["reads"].as_array().cloned().unwrap_or_default();
            let indexes: Vec<i64> = ids.iter().map(|id| {
                next_reads.iter().position(|x| strict_eq(&x["marketId"], id)).map(|i| i as i64).unwrap_or(-1)
            }).collect();
            let gap = indexes.get(1).map(|i| (indexes[0] - i) as f64).unwrap_or(f64::NAN);
            // Never move a held-risk read to manufacture an adjacent research pair.
            if ids.iter().any(|id| protected_ids.iter().any(|p| strict_eq(p, id)))
                || indexes.iter().any(|i| *i < 0)
                || gap.abs() != 1.0
            {
                let mut out = base.clone();
                out["canonicalCapacityFocus"] = json!({"marketIds": [],
                    "reason": "protected_leg_or_nonadjacent_pair",
                    "requestsAdded": 0, "executionAuthority": false});
                return Ok(out);
            }
        }
        Ok(next)
    }
}
