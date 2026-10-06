use crate::atomic::{need, read_json};
use serde_json::Value as V;
use std::path::Path;

// live-paper/transport-network-budget.mjs schema 1-4 validators plus the
// schema-1-only live-paper/network-budget.mjs variant. Every field is checked
// exactly; unknown keys reject.
fn keys_of(v: &V) -> Vec<String> {
    v.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default()
}
fn exact_keys(v: &V, keys: &[&str]) -> bool {
    let got = keys_of(v);
    got.len() == keys.len() && got.iter().all(|k| keys.contains(&k.as_str()))
}
fn pos_safe_int(v: &V) -> bool {
    v.as_i64().is_some_and(|n| n > 0)
}
fn handoff_matches(v: &V, human: bool) -> bool {
    let Some(id) = v.get("handoffId").and_then(V::as_str) else { return false };
    let prefix = if human { "human-to-baal-" } else { "daenerys-to-baal-" };
    let Some(rest) = id.strip_prefix(prefix) else { return false };
    !rest.is_empty() && rest.len() <= 70 && rest.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
pub fn validate_network_budget(v: &V) -> Result<V, String> {
    let sv = v.get("schemaVersion").and_then(V::as_i64).unwrap_or(-1);
    match sv {
        4 => {
            let human = v.get("authorityKind").and_then(V::as_str) == Some("direct_human_assignment");
            let mut base = [
                "schemaVersion", "ownerId", "enabled", "intervalMs", "writeIntervalMs",
                "soleAccountClient", "executionOwner", "handoffId", "ackCommentId",
                "writesEnabled",
            ]
            .to_vec();
            if human {
                base.extend(["authorityKind", "assignmentCommentId"]);
            } else {
                base.push("releaseCommentId");
            }
            let comment = if human { "assignmentCommentId" } else { "releaseCommentId" };
            need(
                exact_keys(v, &base)
                    && v.get("ownerId").and_then(V::as_str) == Some("Codex")
                    && v.get("enabled") == Some(&V::Bool(true))
                    && v.get("intervalMs").and_then(V::as_i64) == Some(1000)
                    && v.get("writeIntervalMs").and_then(V::as_i64) == Some(2500)
                    && v.get("soleAccountClient") == Some(&V::Bool(true))
                    && v.get("executionOwner").and_then(V::as_str) == Some("Codex")
                    && v.get("writesEnabled") == Some(&V::Bool(true))
                    && handoff_matches(v, human)
                    && pos_safe_int(v.get(comment).unwrap_or(&V::Null))
                    && pos_safe_int(v.get("ackCommentId").unwrap_or(&V::Null)),
                "Invalid production network budget",
            )?;
            Ok(v.clone())
        }
        3 => {
            let keys = [
                "schemaVersion", "ownerId", "enabled", "intervalMs", "soleAccountClient",
                "executionOwner", "ackCommentId", "maxReadsPerMinute", "writesEnabled",
            ];
            need(
                exact_keys(v, &keys)
                    && v.get("ownerId").and_then(V::as_str) == Some("Codex")
                    && v.get("enabled") == Some(&V::Bool(true))
                    && v.get("intervalMs").and_then(V::as_i64) == Some(3000)
                    && v.get("soleAccountClient") == Some(&V::Bool(false))
                    && v.get("executionOwner").and_then(V::as_str) == Some("Daenerys")
                    && v.get("ackCommentId").and_then(V::as_i64) == Some(5966761902)
                    && v.get("maxReadsPerMinute").and_then(V::as_i64) == Some(20)
                    && v.get("writesEnabled") == Some(&V::Bool(false)),
                "Invalid shared read-only network budget",
            )?;
            Ok(v.clone())
        }
        1 | 2 => {
            let mut keys = vec![
                "schemaVersion", "ownerId", "enabled", "intervalMs", "soleAccountClient",
                "formerWriterStopCommentId",
            ];
            if sv == 2 {
                keys.push("writeIntervalMs");
            }
            let interval_ok = if sv == 1 {
                v.get("intervalMs").and_then(V::as_i64) == Some(3000)
            } else {
                v.get("intervalMs").and_then(V::as_i64) == Some(1000)
                    && v.get("writeIntervalMs").and_then(V::as_i64) == Some(2500)
            };
            need(
                v.is_object()
                    && exact_keys(v, &keys)
                    && v.get("ownerId").and_then(V::as_str) == Some("Codex")
                    && v.get("enabled") == Some(&V::Bool(true))
                    && v.get("soleAccountClient") == Some(&V::Bool(true))
                    && v.get("formerWriterStopCommentId").and_then(V::as_i64) == Some(5947535773)
                    && interval_ok,
                "Invalid coordinated network budget",
            )?;
            Ok(v.clone())
        }
        _ => Err("Invalid coordinated network budget".into()),
    }
}

// live-paper/network-budget.mjs: schema-1-only validator used by the legacy
// paper path.
pub fn validate_network_budget_v1(v: &V) -> Result<V, String> {
    let keys = [
        "schemaVersion", "ownerId", "enabled", "intervalMs", "soleAccountClient",
        "formerWriterStopCommentId",
    ];
    need(
        v.is_object()
            && exact_keys(v, &keys)
            && v.get("schemaVersion").and_then(V::as_i64) == Some(1)
            && v.get("ownerId").and_then(V::as_str) == Some("Codex")
            && v.get("enabled") == Some(&V::Bool(true))
            && v.get("soleAccountClient") == Some(&V::Bool(true))
            && v.get("formerWriterStopCommentId").and_then(V::as_i64) == Some(5947535773)
            && v.get("intervalMs").and_then(V::as_i64) == Some(3000),
        "Invalid coordinated network budget",
    )?;
    Ok(v.clone())
}

pub fn load_network_budget(file: &Path) -> Result<Option<V>, String> {
    match read_json(file)? {
        None => Ok(None),
        Some(v) => Ok(Some(validate_network_budget(&v)?)),
    }
}

fn safe_integer(v: f64) -> bool {
    v.fract() == 0.0 && v.abs() <= 9_007_199_254_740_991.0
}
pub fn network_interval(interval_ms: i64, budget: Option<&V>) -> Result<u64, String> {
    need(
        safe_integer(interval_ms as f64) && interval_ms >= 5000,
        "Read budget requires >=5 second pacing unless explicitly coordinated",
    )?;
    match budget {
        None => Ok(interval_ms as u64),
        Some(b) => Ok(validate_network_budget(b)?
            .get("intervalMs")
            .and_then(V::as_i64)
            .unwrap_or(0) as u64),
    }
}
pub fn network_write_interval(interval_ms: i64, budget: Option<&V>) -> Result<u64, String> {
    let read = network_interval(interval_ms, budget)?;
    match budget.and_then(|b| b.get("schemaVersion")).and_then(V::as_i64) {
        Some(2) | Some(4) => Ok(validate_network_budget(budget.unwrap())?
            .get("writeIntervalMs")
            .and_then(V::as_i64)
            .unwrap_or(0) as u64),
        _ => Ok(read),
    }
}

// feed/runtime.mjs AccountReadBudget: the same service-backed admission
// callback is injected across all account clients; this is not itself a
// distributed limiter.
pub struct AccountReadBudget {
    pub account_scope: String,
    pub admit_callback: Box<dyn Fn(&V) -> Result<(), String> + Send + Sync>,
    pub cooldown_callback: Box<dyn Fn(&V) -> Result<(), String> + Send + Sync>,
}
impl AccountReadBudget {
    pub fn new(
        account_scope: &str,
        admit: Box<dyn Fn(&V) -> Result<(), String> + Send + Sync>,
        cooldown: Box<dyn Fn(&V) -> Result<(), String> + Send + Sync>,
    ) -> Result<AccountReadBudget, String> {
        need(!account_scope.is_empty(), "SHARED_BUDGET_REQUIRED")?;
        Ok(AccountReadBudget {
            account_scope: account_scope.to_string(),
            admit_callback: admit,
            cooldown_callback: cooldown,
        })
    }
    pub fn admit(&self, path: &str, aborted: &dyn Fn() -> bool, should_stop: &dyn Fn() -> bool) -> Result<(), String> {
        crate::wait::check_stop(aborted(), should_stop).map_err(|e| e.to_string())?;
        (self.admit_callback)(&serde_json::json!({
            "accountScope": self.account_scope,
            "method": "GET",
            "path": path,
            "cost": 1,
            "priority": "recovery",
        }))?;
        crate::wait::check_stop(aborted(), should_stop).map_err(|e| e.to_string())
    }
    pub fn cooldown(&self, ms: f64, aborted: &dyn Fn() -> bool) -> Result<(), String> {
        need(ms.is_finite() && ms >= 0.0, "INVALID_DELAY")?;
        (self.cooldown_callback)(&serde_json::json!({
            "accountScope": self.account_scope,
            "retryAfterMs": ms,
            "requiresManualReview": ms == crate::wait::MAX_SAFE_INTEGER as f64,
            "aborted": aborted(),
        }))
    }
}
