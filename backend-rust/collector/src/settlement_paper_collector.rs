// live-paper/settlement-paper-collector.mjs — collector-side settlement paper binding.
use baal_execution::core::payload_hash;
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::settlement_paper_host::PaperHost;

fn digest(v: &V) -> String { crate::model::digest(v) }
fn parse_at(v: &V) -> f64 { baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN) }

/// openCollectorSettlementPaper({runtimeDir, universe, now, workspaceDir})
/// `live_paper_dir` stands in for import.meta.url. Returns Ok(None) when the
/// pointer file is absent or OBSERVER_STOP is set (JS null).
pub fn open_collector_settlement_paper(runtime_dir: &Path, universe: &V,
    now: Arc<dyn Fn() -> i64 + Send + Sync>, workspace_dir: &Path,
    live_paper_dir: &Path) -> Result<Option<SettlementCollectorHost>, String> {
    let file = runtime_dir.join("settlement-paper-active.json");
    if !file.exists() { return Ok(None); }
    let pointer: V = serde_json::from_slice(&std::fs::read(&file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let cohort = pointer["cohort"].as_str().unwrap_or("");
    let cb = cohort.as_bytes();
    if !(cb.len() >= 2 && cb.len() <= 64 && cb[0].is_ascii_lowercase()
        && cb[1..].iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-'))
    {
        return Err("SETTLEMENT_COLLECTOR_SCOPE".into());
    }
    let dir = runtime_dir.join("settlement-paper-cohorts").join(cohort);
    let bank: V = serde_json::from_slice(&std::fs::read(dir.join("bank.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if dir.join("OBSERVER_STOP").exists() { return Ok(None); }
    let mut bank_raw = bank["universe"].clone();
    let bank_declared_hash = bank_raw.as_object_mut().and_then(|m| m.shift_remove("hash"));
    let mut live_raw = universe.clone();
    let live_declared_hash = live_raw.as_object_mut().and_then(|m| m.shift_remove("hash"));
    let checked = crate::breadth::validate_universe(&bank_raw, universe["tournamentId"].as_str().unwrap_or(""))?;
    let live_universe = crate::breadth::validate_universe(&live_raw, universe["tournamentId"].as_str().unwrap_or(""))?;
    if (bank_declared_hash.as_ref().is_some_and(|h| js_truthy(Some(h)))
            && bank_declared_hash.as_ref() != Some(&checked["hash"]))
        || (live_declared_hash.as_ref().is_some_and(|h| js_truthy(Some(h)))
            && live_declared_hash.as_ref() != Some(&live_universe["hash"]))
    {
        return Err("SETTLEMENT_COLLECTOR_UNIVERSE_HASH".into());
    }
    if digest(&bank) != pointer["bankHash"].as_str().unwrap_or("")
        || checked["hash"] != live_universe["hash"]
        || payload_hash(&checked["exchanges"]).ok() != payload_hash(&live_universe["exchanges"]).ok()
    {
        return Err("SETTLEMENT_COLLECTOR_BINDING".into());
    }
    // Frozen-module check: the Rust port has no dynamic module load — the frozen
    // snapshot is verified, then the crate's own host implementation runs.
    let _frozen = crate::frozen_paper_runtime::frozen_paper_module(runtime_dir, &bank,
        "live-paper/settlement-paper-host.mjs")?;
    let host = crate::settlement_paper_host::SettlementPaperHost::open(runtime_dir, cohort, false, None,
        workspace_dir, now.clone(), live_paper_dir)?;
    let original = crate::fundamental_entry_drain::FundamentalEntryDrainHost::wrap(
        Box::new(host) as Box<dyn PaperHost>, runtime_dir, now.clone())?;
    let original_lock = original.dir().join("worker.lock");
    let original_token = std::fs::read_to_string(&original_lock).map_err(|e| e.to_string())?;
    let can_operate = {
        let workspace_dir = workspace_dir.to_path_buf();
        let runtime_dir = runtime_dir.to_path_buf();
        let original_dir = original.dir().to_path_buf();
        let original_lock = original_lock.clone();
        let original_token = original_token.clone();
        Arc::new(move || -> bool {
            ![workspace_dir.join("STOP"), runtime_dir.join("STOP"), runtime_dir.join("EXECUTION_STOP"),
                runtime_dir.join("COLLECTOR_STOP"), original_dir.join("STOP"), original_dir.join("OBSERVER_STOP")]
                .iter().any(|f| f.exists())
                && original_lock.exists()
                && std::fs::read_to_string(&original_lock).map(|t| t == original_token).unwrap_or(false)
        }) as Arc<dyn Fn() -> bool + Send + Sync>
    };
    // wrap() closes the original host itself when the operator open fails.
    let host = crate::fundamental_value_revision_v1::FundamentalValueRevisionWrapper::wrap(
        Box::new(original) as Box<dyn PaperHost>, runtime_dir, now.clone(), can_operate, live_paper_dir)?;
    Ok(Some(SettlementCollectorHost {
        revision_active: host.operator_revision_active(),
        host, bank, bank_hash: pointer["bankHash"].as_str().unwrap_or("").to_string(),
        runtime_dir: runtime_dir.to_path_buf(), workspace_dir: workspace_dir.to_path_buf(), now,
    }))
}
/// `{...host, focus: host.operatorRevisionActive ? host.focus : focus}` — the
/// outer object delegates everything except the focus override chosen at open.
pub struct SettlementCollectorHost {
    host: Box<dyn PaperHost>,
    revision_active: bool,
    bank: V,
    bank_hash: String,
    #[allow(dead_code)]
    runtime_dir: PathBuf,
    #[allow(dead_code)]
    workspace_dir: PathBuf,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
}
impl SettlementCollectorHost {
    /// The collector-side focus (used only when no operator revision is active).
    fn collector_focus(&mut self) -> Result<V, String> {
        let stats = self.host.status()?;
        let mut markets: indexmap::IndexMap<String, V> = Default::default();
        for (_key, w) in stats["wallets"].as_object().cloned().unwrap_or_default() {
            for p in w["positions"].as_array().cloned().unwrap_or_default() {
                markets.insert(p["marketId"].as_str().unwrap_or("").to_string(),
                    json!({"marketId": p["marketId"], "phase": "held"}));
            }
            for p in w["pending"].as_array().cloned().unwrap_or_default() {
                let id = p["marketId"].as_str().unwrap_or("").to_string();
                if !markets.contains_key(&id) {
                    markets.insert(id, json!({"marketId": p["marketId"],
                        "phase": if p["action"].as_str() == Some("sell") { "pending_sell" } else { "pending_buy" }}));
                }
            }
        }
        let draining = self.host.entry_draining()?;
        for r in self.bank["research"].as_array().cloned().unwrap_or_default() {
            let flags_ok = match &r["flags"] {
                V::Null => return Err("SETTLEMENT_COLLECTOR_FLAGS".into()),
                V::Array(a) => a.is_empty(),
                V::String(s) => s.is_empty(),
                _ => true,
            };
            if !draining && js_truthy(r.get("settlementReviewed")) && flags_ok
                && (self.now)() as f64 <= parse_at(&r["expiresAt"])
                && (self.now)() as f64 - parse_at(&r["sourceAt"]) <= self.bank["policy"]["maximumResearchAgeMs"].as_f64().unwrap_or(0.0)
                && !markets.contains_key(r["marketId"].as_str().unwrap_or(""))
            {
                markets.insert(r["marketId"].as_str().unwrap_or("").to_string(),
                    json!({"marketId": r["marketId"], "phase": "depth_required"}));
            }
        }
        Ok(json!({"bankHash": self.bank_hash, "markets": markets.values().cloned().collect::<Vec<_>>(),
            "ownedRiskReads": true, "executionAuthority": false}))
    }
}
impl PaperHost for SettlementCollectorHost {
    fn dir(&self) -> &Path { self.host.dir() }
    fn bank(&self) -> &V { self.host.bank() }
    fn consume(&mut self) -> Result<usize, String> { self.host.consume() }
    fn status(&mut self) -> Result<V, String> { self.host.status() }
    fn publish(&mut self) -> Result<(), String> { self.host.publish() }
    fn focus(&mut self) -> Result<V, String> {
        if self.revision_active { self.host.focus() } else { self.collector_focus() }
    }
    fn boundary(&mut self, reason: &str) -> Result<(), String> { self.host.boundary(reason) }
    fn on_book(&mut self, publication: &V, market_id: &str, account: &V) -> Result<(), String> {
        self.host.on_book(publication, market_id, account)
    }
    fn entry_draining(&self) -> Result<bool, String> { self.host.entry_draining() }
    fn close(&mut self) -> Result<(), String> { self.host.close() }
    fn operator_revision_active(&self) -> bool { self.revision_active }
}
