use crate::core::*;
use crate::decision::{assess_entry_gates, open_entry_decision_audit, EntryDecisionAudit};
use crate::gates::evaluate_entry_gates;
use crate::production::{production_handoff_current, validate_production_activation};
use crate::transport::{OrderTransport, OrderTransportOptions};
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::history::historical_reference;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{digest, js_int};
use baal_service_runtime::ledger::{IntentLedger, LedgerOptions};
use baal_service_runtime::market::normalize_book;
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const TID: &str = crate::TOURNAMENT_ID;
pub const SLUG: &str = crate::TOURNAMENT_SLUG;
pub fn micros(x: &V) -> i128 {
    decimal_micros(&json!(x.as_str().map(str::to_string).unwrap_or_else(|| crate::json_number(x))), "exact")
        .unwrap_or(i128::MIN)
}
fn instant(s: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true) == s)
        .unwrap_or(false)
}
pub fn parse(v: &V) -> i64 {
    chrono::DateTime::parse_from_rfc3339(v.as_str().unwrap_or(""))
        .map(|d| d.timestamp_millis())
        .unwrap_or(i64::MIN)
}
pub fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn empty_control() -> V {
    json!({"schemaVersion": 1, "exits": {}, "cancellations": {}})
}
fn safe_key(k: &str) -> bool {
    !["__proto__", "constructor", "prototype"].contains(&k)
}
fn plain(v: &V) -> bool {
    v.is_object()
}
fn validate_control(state: &V) -> Result<(), String> {
    let c = state.get("control").unwrap_or(&V::Null);
    let keys_ok = c.as_object().is_some_and(|m| {
        m.keys().all(|k| ["schemaVersion", "exits", "cancellations"].contains(&k.as_str()))
    });
    if !(plain(c)
        && c.get("schemaVersion").and_then(V::as_i64) == Some(1)
        && keys_ok
        && plain(c.get("exits").unwrap_or(&V::Null))
        && plain(c.get("cancellations").unwrap_or(&V::Null)))
    {
        return Err("INVALID_EXECUTION_CONTROL".into());
    }
    for (exchange_id, x) in c.get("exits").unwrap().as_object().unwrap() {
        let xok = x.as_object().is_some_and(|m| {
            m.keys().all(|k| ["marketId", "reason", "triggeredAt", "triggerBid", "sourceVersion"].contains(&k.as_str()))
        });
        let owned = state.pointer(&format!("/owned/{exchange_id}"));
        if !(safe_key(exchange_id)
            && xok
            && owned.is_some()
            && x.get("marketId") == owned.unwrap().get("marketId")
            && ["stop_loss", "holding_timeout"].contains(&x.get("reason").and_then(V::as_str).unwrap_or(""))
            && x.get("triggeredAt").and_then(V::as_str).is_some_and(|s| instant(s))
            && x.get("sourceVersion").and_then(V::as_str).is_some_and(|s| s.len() <= 256)
            && micros(x.get("triggerBid").unwrap_or(&V::Null)) > 0
            && micros(x.get("triggerBid").unwrap_or(&V::Null)) < 1_000_000)
        {
            return Err("INVALID_EXIT_OBLIGATION".into());
        }
    }
    for (id, x) in c.get("cancellations").unwrap().as_object().unwrap() {
        let i = state.pointer(&format!("/intents/{id}"));
        let xok = x.as_object().is_some_and(|m| {
            m.keys().all(|k| ["orderId", "attempts", "phase", "lastAttemptAt", "evidenceAt", "reasonCode"].contains(&k.as_str()))
        });
        let order_id = js_int(x.get("orderId").unwrap_or(&V::Null));
        let reason_ok = match x.get("reasonCode") {
            None => true,
            Some(V::Null) => false, // key present must hold a valid value
            Some(v) => v.as_str().is_some_and(|s| {
                !s.is_empty()
                    && s.len() <= 101
                    && s.bytes().next().is_some_and(|b| b.is_ascii_uppercase())
                    && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            }),
        };
        if !(safe_key(id)
            && xok
            && i.is_some()
            && x.get("orderId") == i.unwrap().get("orderId")
            && order_id.is_some_and(|o| o > 0)
            && x.get("attempts").and_then(V::as_i64).is_some_and(|n| n >= 1)
            && ["dispatching", "uncertain", "acknowledged", "reconciled"]
                .contains(&x.get("phase").and_then(V::as_str).unwrap_or(""))
            && x.get("lastAttemptAt").and_then(V::as_str).is_some_and(|s| instant(s))
            && x.get("evidenceAt").and_then(V::as_str).is_some_and(|s| instant(s))
            && (i.unwrap().get("status").and_then(V::as_str) == Some("closed"))
                == (x.get("phase").and_then(V::as_str) == Some("reconciled"))
            && reason_ok)
        {
            return Err("INVALID_CANCEL_CONTROL".into());
        }
    }
    Ok(())
}
pub fn validate_activation(a: &V) -> Result<V, String> {
    if a.get("schemaVersion").and_then(V::as_i64) == Some(2) {
        return validate_production_activation(a);
    }
    let run_ok = a.get("runId").and_then(V::as_str).is_some_and(|s| {
        s.strip_prefix("sig-pilot-").is_some_and(|r| {
            !r.is_empty() && r.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    });
    if !(a.get("schemaVersion").and_then(V::as_i64) == Some(1)
        && a.get("ownerId").and_then(V::as_str) == Some("Codex")
        && a.get("tournamentId").and_then(V::as_str) == Some(TID)
        && run_ok
        && a.get("enabled") == Some(&V::Bool(true))
        && a.get("formerWriterStopCommentId").and_then(V::as_i64) == Some(5_947_535_773)
        && a.get("feeBps").and_then(V::as_i64) == Some(0))
    {
        return Err("INVALID_ACTIVATION".into());
    }
    if !(micros(a.get("maxOrderCost").unwrap_or(&V::Null)) <= 25_000_000
        && micros(a.get("maxOrderCost").unwrap_or(&V::Null)) > 0
        && micros(a.get("maxAggregateCost").unwrap_or(&V::Null)) <= 100_000_000
        && micros(a.get("maxCumulativeBuyCost").unwrap_or(&V::Null)) <= 100_000_000
        && micros(a.get("maxCumulativeBuyCost").unwrap_or(&V::Null))
            >= micros(a.get("maxOrderCost").unwrap_or(&V::Null))
        && a.get("maxManagedMarkets").and_then(V::as_i64) == Some(3)
        && a.get("depthFractionBps").and_then(V::as_i64) == Some(100))
    {
        return Err("INVALID_PILOT_LIMITS".into());
    }
    if !(a.get("startedAt").and_then(V::as_str).is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok())
        && a.get("entryExpiresAt").and_then(V::as_str).is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok())
        && parse(a.get("entryExpiresAt").unwrap()) > parse(a.get("startedAt").unwrap())
        && parse(a.get("entryExpiresAt").unwrap()) - parse(a.get("startedAt").unwrap()) <= 21_600_000)
    {
        return Err("INVALID_ACTIVATION_WINDOW".into());
    }
    Ok(a.clone())
}
fn reference_input_hash(shadow: &V, exchange_id: &str) -> String {
    digest(&json!({
        "exchangeId": exchange_id,
        "history": shadow.pointer(&format!("/histories/{exchange_id}")).cloned().unwrap_or(V::Null),
        "bookHistory": shadow.pointer(&format!("/bookHistory/{exchange_id}")).cloned().unwrap_or(json!([])),
    }))
}

// The JS closures capture the controller; in Rust the shared mutable surface
// lives behind one RefCell so transport fences can re-enter.
pub struct ControllerInner {
    pub state: V,
    pub cash: Option<V>,
    pub account: Option<V>,
    pub markets: BTreeMap<String, V>,
    pub selected: Vec<String>,
    pub current_permit: Option<V>,
    pub bootstrapped: bool,
    pub readiness: V,
    pub status: String,
    pub reason: Option<String>,
    pub halted: bool,
    pub reconciliation_risk_only: bool,
    // Subclass hooks (JS prototype overrides). Set after construction.
    pub entry_reference_gate: Option<Rc<EntryReferenceGate>>,
    pub pre_admit: Option<Rc<dyn Fn(&Rc<RefCell<ControllerInner>>)>>,
    pub bootstrap_market_id: Option<Rc<dyn Fn() -> String>>,
}
impl Default for ControllerInner {
    fn default() -> Self {
        ControllerInner {
            state: V::Null,
            cash: None,
            account: None,
            markets: BTreeMap::new(),
            selected: Vec::new(),
            current_permit: None,
            bootstrapped: false,
            readiness: json!({"complete": false}),
            status: "reconciling".into(),
            reason: None,
            halted: false,
            reconciliation_risk_only: false,
            entry_reference_gate: None,
            pre_admit: None,
            bootstrap_market_id: None,
        }
    }
}
/// entryReferenceAdmitted(i, permit, now) — see controller.mjs.
pub type EntryReferenceGate =
    dyn Fn(&Rc<RefCell<ControllerInner>>, &V, &Path, &V, &V, i64) -> bool;
pub type ApiDispatch = dyn for<'g> Fn(
    &V,
    Option<&'g crate::transport::Guard<'g>>,
    Option<&'g crate::transport::Guard<'g>>,
) -> Result<V, String>;

pub struct CompetitionController {
    pub dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub universe: V,
    pub config: V,
    pub activation: V,
    pub activation_hash: String,
    pub lease: V,
    pub inner: Rc<RefCell<ControllerInner>>,
    pub ledger: Rc<RefCell<IntentLedger>>,
    pub journal: Rc<RefCell<Journal>>,
    pub entry_audit: Rc<RefCell<EntryDecisionAudit>>,
    pub transport: OrderTransport,
    pub now: Rc<dyn Fn() -> f64>,
    pub is_stopped: Rc<dyn Fn() -> bool>,
}
pub struct ControllerOptions {
    pub runtime_dir: PathBuf,
    pub api_dispatch: Rc<ApiDispatch>,
    pub universe: V,
    pub config: V,
    pub is_stopped: Rc<dyn Fn() -> bool>,
    pub activation: V,
    pub now: Rc<dyn Fn() -> f64>,
}
impl CompetitionController {
    pub fn new(options: ControllerOptions) -> Result<Self, String> {
        let activation = validate_activation(&options.activation)?;
        let dir = if activation.get("schemaVersion").and_then(V::as_i64) == Some(2) {
            options
                .runtime_dir
                .join("competition-runs")
                .join(activation.get("runId").and_then(V::as_str).unwrap())
        } else {
            options.runtime_dir.join("competition")
        };
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let inner = Rc::new(RefCell::new(ControllerInner::default()));
        let activation_hash = payload_hash(&activation)?;
        let lease = json!({"ownerId": "Codex", "runId": activation.get("runId").unwrap().clone(), "scope": TID});
        let now_ledger = options.now.clone();
        let is_stopped = options.is_stopped.clone();
        let runtime_dir = options.runtime_dir.clone();
        // The ledger owns the single-writer lock for this run directory.
        let inner_for_stop = inner.clone();
        let can_write_stop = {
            let inner = inner_for_stop.clone();
            let activation = activation.clone();
            let runtime_dir = runtime_dir.clone();
            let activation_hash = activation_hash.clone();
            let is_stopped = is_stopped.clone();
            Rc::new(move || -> bool {
                let inner = inner.borrow();
                if inner.halted || is_stopped() || runtime_dir.join("EXECUTION_STOP").exists() {
                    return false;
                }
                if activation.get("schemaVersion").and_then(V::as_i64) == Some(2)
                    && !production_handoff_current(&runtime_dir, &activation)
                {
                    return false;
                }
                std::fs::read_to_string(runtime_dir.join("competition-activation.json"))
                    .ok()
                    .and_then(|s| serde_json::from_str::<V>(&s).ok())
                    .and_then(|a| payload_hash(&a).ok())
                    == Some(activation_hash.clone())
            })
        };
        let can_write = can_write_stop.clone();
        let ledger = IntentLedger::open(
            &dir,
            LedgerOptions {
                owner_id: "Codex".into(),
                run_id: activation.get("runId").and_then(V::as_str).unwrap().to_string(),
                scope: TID.into(),
                is_stopped: Box::new(move || !can_write()),
                now: Box::new(move || (*now_ledger)()),
            },
        )
        .map_err(|e| e.message.clone())?;
        let ledger = Rc::new(RefCell::new(ledger));
        let journal = Rc::new(RefCell::new(
            Journal::open(&dir.join("core-journal.jsonl")).map_err(|e| e.clone())?,
        ));
        let entry_audit = Rc::new(RefCell::new(
            open_entry_decision_audit(&dir.join("decisions.jsonl"), activation.get("runId").and_then(V::as_str).unwrap())?,
        ));
        // Initial state: journal tail or a fresh state with the activation's
        // policy.
        let state = match &journal.borrow().last {
            Some(last) => {
                let s = last.get("state").cloned().unwrap_or(V::Null);
                validate_control(&s)?;
                validate_execution_state(&s)?
            }
            None => {
                let mut policy = json!({
                    "maxOrderCost": activation.get("maxOrderCost").unwrap().clone(),
                    "maxAggregateCost": activation.get("maxAggregateCost").unwrap().clone(),
                    "maxCumulativeBuyCost": activation.get("maxCumulativeBuyCost").unwrap().clone(),
                    "maxManagedMarkets": 3,
                    "cashReserve": if activation.get("schemaVersion").and_then(V::as_i64) == Some(2) {
                        activation.get("cashReserve").unwrap().clone()
                    } else {
                        json!("90000.000000")
                    },
                });
                if activation.get("schemaVersion").and_then(V::as_i64) == Some(2) {
                    policy.as_object_mut().unwrap().insert("maxAccountAgeMs".into(), json!(20000));
                }
                let mut s = create_execution_state(TID, Some(&policy))?;
                s.as_object_mut().unwrap().insert("control".into(), empty_control());
                validate_execution_state(&s)?
            }
        };
        if !(state.get("tournamentId").and_then(V::as_str) == Some(TID)
            && state.pointer("/policy/maxOrderCost") == activation.get("maxOrderCost").map(|v| v as &V).or(None)
            && state.pointer("/policy/maxAggregateCost") == activation.get("maxAggregateCost").map(|v| v as &V).or(None)
            && state.pointer("/policy/maxCumulativeBuyCost") == activation.get("maxCumulativeBuyCost").map(|v| v as &V).or(None))
        {
            return Err("CORE_POLICY_MISMATCH".into());
        }
        if activation.get("schemaVersion").and_then(V::as_i64) == Some(2)
            && !(state.pointer("/policy/cashReserve") == activation.get("cashReserve").map(|v| v as &V).or(None)
                && state.pointer("/policy/maxAccountAgeMs") == Some(&json!(20000)))
        {
            return Err("CORE_POLICY_MISMATCH".into());
        }
        inner.borrow_mut().state = state;
        let transport = {
            let inner = inner.clone();
            let ledger_for_order = ledger.clone();
            let activation2 = activation.clone();
            let lease_v = lease.clone();
            let now = options.now.clone();
            let dispatch = options.api_dispatch.clone();
            let (inner_a, inner_b, inner_c, inner_d) =
                (inner.clone(), inner.clone(), inner.clone(), inner.clone());
            let (rt_d, rt_e) = (runtime_dir.clone(), runtime_dir.clone());
            let (journal_d, journal_e) = (journal.clone(), journal.clone());
            let _ = journal_e;
            let (ledger_a, ledger_b, ledger_c) =
                (ledger_for_order.clone(), ledger_for_order.clone(), ledger_for_order.clone());
            let (act_a, act_b, act_c, act_d) =
                (activation2.clone(), activation2.clone(), activation2.clone(), activation2.clone());
            let (lease_a, lease_b) = (lease_v.clone(), lease_v.clone());
            let (now_a, now_b, now_c) = (now.clone(), now.clone(), now.clone());
            let (rt_b,) = (runtime_dir.clone(),);
            let (ah_b,) = (activation_hash.clone(),);
            let (st_b,) = (is_stopped.clone(),);
            OrderTransport::new(OrderTransportOptions {
                dispatch: Box::new(move |spec, bd, bn| {
                    dispatch(spec, bd, bn).map_err(|e| crate::transport::OrderTransportError {
                        code: e,
                        outcome: "unknown".into(),
                        may_have_executed: true,
                        retry_same_request_only: false,
                        retry_after_ms: 0.0,
                        status: None,
                        api_code: None,
                        semantic_mismatch: false,
                    })
                }),
                scope: crate::transport::scope_of(&json!({
                    "tournamentId": TID,
                    "tournamentSlug": SLUG,
                    "exchanges": options.universe.get("exchanges").unwrap().clone(),
                }))
                .map_err(|e: crate::transport::OrderTransportError| e.code)?,
                can_write: Box::new({
                    let inner = inner_c;
                    let (act_b, ah_b, rt_b, st_b) = (act_b, ah_b, rt_b, st_b);
                    move || {
                        let inner = inner.borrow();
                        if !(inner.bootstrapped) || inner.halted || st_b() || rt_b.join("EXECUTION_STOP").exists() {
                            return false;
                        }
                        if act_b.get("schemaVersion").and_then(V::as_i64) == Some(2)
                            && !production_handoff_current(&rt_b, &act_b)
                        {
                            return false;
                        }
                        std::fs::read_to_string(rt_b.join("competition-activation.json"))
                            .ok()
                            .and_then(|s| serde_json::from_str::<V>(&s).ok())
                            .and_then(|a| payload_hash(&a).ok())
                            == Some(ah_b.clone())
                    }
                }),
                owns_order: Box::new({
                    let inner = inner_a;
                    move |id: &str| {
                        inner
                            .borrow()
                            .state
                            .get("intents")
                            .and_then(V::as_object)
                            .map(|m| {
                                m.values().any(|i| {
                                    i.get("orderId").map(|v| crate::js_key(v)) == Some(id.to_string())
                                })
                            })
                            .unwrap_or(false)
                    }
                }),
                validate_lease: Box::new({
                    let ledger = ledger_a;
                    let act = act_a;
                    move |lease: &V, _spec: &V| {
                        let _ = ledger.borrow().snapshot();
                        Ok(lease.get("runId") == act.get("runId")
                            && lease.get("ownerId").and_then(V::as_str) == Some("Codex")
                            && lease.get("scope").and_then(V::as_str) == Some(TID))
                    }
                }),
                validate_permit: Box::new({
                    move |permit: &V, spec: &V| Ok(controller_admit(&inner_b, &ledger_b, &act_d, &lease_a, &now_b, &rt_d, Some(&journal_d), permit, spec))
                }),
                validate_network_permit: Box::new({
                    let inner = inner_d;
                    let ledger = ledger_c;
                    let act = act_c;
                    let lease = lease_b;
                    let now = now_c;
                    move |permit: &V, spec: &V| {
                        Ok(controller_network_admit(&inner, &ledger, &act, &lease, &now, &rt_e, permit, spec))
                    }
                }),
                now: Box::new(move || (now_a)() as i64),
                max_pages: 100,
            })
            .map_err(|e: crate::transport::OrderTransportError| e.code)?
        };
        let controller = CompetitionController {
            dir,
            runtime_dir,
            universe: options.universe,
            config: options.config,
            activation,
            activation_hash,
            lease,
            inner,
            ledger,
            journal,
            entry_audit,
            transport,
            now: options.now,
            is_stopped: options.is_stopped,
        };
        controller.assert_ledgers()?;
        controller.publish();
        Ok(controller)
    }
    pub fn now_ms(&self) -> i64 {
        (self.now)() as i64
    }
    pub fn can_operate(&self) -> bool {
        let inner = self.inner.borrow();
        if inner.halted || (self.is_stopped)() || self.runtime_dir.join("EXECUTION_STOP").exists() {
            return false;
        }
        if self.activation.get("schemaVersion").and_then(V::as_i64) == Some(2)
            && !production_handoff_current(&self.runtime_dir, &self.activation)
        {
            return false;
        }
        std::fs::read_to_string(self.runtime_dir.join("competition-activation.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<V>(&s).ok())
            .and_then(|a| payload_hash(&a).ok())
            .as_deref()
            == Some(self.activation_hash.as_str())
    }
    pub fn can_write(&self) -> bool {
        self.inner.borrow().bootstrapped && self.can_operate()
    }
    pub fn entry_window_admitted(&self, now: i64) -> bool {
        let start = parse(self.activation.get("startedAt").unwrap());
        let end = parse(self.activation.get("entryExpiresAt").unwrap());
        let mut inner = self.inner.borrow_mut();
        if start == i64::MIN || end == i64::MIN {
            inner.reason = Some("INVALID_ACTIVATION_CLOCK".into());
            return false;
        }
        if now < start {
            inner.reason = Some("ENTRY_WINDOW_NOT_STARTED".into());
            return false;
        }
        if now >= end {
            inner.reason = Some("ENTRY_WINDOW_CLOSED".into());
            return false;
        }
        true
    }
    fn responsive_reference(&self, expected_hash: Option<&str>) -> Result<V, String> {
        let deny = |code: &str| Err(code.to_string());
        let read = |name: &str| -> Result<V, String> {
            std::fs::read_to_string(self.runtime_dir.join(name))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .ok_or_else(|| "REFERENCE_WORKER_UNAVAILABLE".to_string())
        };
        let bank = read("dip-responsive-bank.json")?;
        let shadow = read("dip-responsive-state.json")?;
        let stats = read("dip-responsive-stats.json")?;
        let hash = digest(&bank);
        let now = self.now_ms();
        if !(bank.pointer("/policy/executionPolicy").and_then(V::as_str) == Some("responsive_v2")
            && shadow.get("bankHash").and_then(V::as_str) == Some(hash.as_str())
            && stats.get("bankHash").and_then(V::as_str) == Some(hash.as_str())
            && expected_hash.map(|e| e == hash).unwrap_or(true))
        {
            return deny("REFERENCE_BANK_MISMATCH");
        }
        if stats.get("status").and_then(V::as_str) != Some("running") {
            return deny("REFERENCE_WORKER_NOT_RUNNING");
        }
        if !(stats.get("updatedAt").and_then(V::as_str).is_some_and(|s| instant(s))
            && parse(stats.get("updatedAt").unwrap()) <= now
            && now - parse(stats.get("updatedAt").unwrap()) <= 60000)
        {
            return deny("REFERENCE_WORKER_STALE");
        }
        if !(js_int(bank.get("sourceRecords").unwrap_or(&V::Null)).is_some_and(|n| n >= 1)
            && js_int(shadow.get("sourceSequence").unwrap_or(&V::Null))
                .is_some_and(|s| s >= js_int(bank.get("sourceRecords").unwrap()).unwrap())
            && shadow.get("lastRecordHash").and_then(V::as_str).is_some_and(|h| {
                h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            })
            && stats.get("sourceSequence") == shadow.get("sourceSequence")
            && stats.get("sourceHash") == shadow.get("lastRecordHash"))
        {
            return deny("REFERENCE_SOURCE_MISMATCH");
        }
        Ok(json!({
            "ok": true,
            "bank": bank,
            "shadow": shadow.clone(),
            "proof": {
                "bankHash": hash,
                "sourceSequence": shadow.get("sourceSequence").unwrap().clone(),
                "sourceHash": shadow.get("lastRecordHash").unwrap().clone(),
            },
        }))
    }
    pub fn entry_reference_admitted(&self, intent: &V, permit: &V, now: i64) -> bool {
        // An identical expired replay can only retrieve an earlier acceptance.
        if intent.get("action").and_then(V::as_str) != Some("buy") {
            return true;
        }
        let inner = self.inner.borrow();
        let current = inner.current_permit.clone().unwrap_or(V::Null);
        drop(inner);
        if permit.get("retry") == Some(&V::Bool(true))
            && current.get("retry") == Some(&V::Bool(true))
            && intent.get("attempts").and_then(V::as_i64).unwrap_or(0) > 0
            && now >= parse(intent.get("expirationDate").unwrap())
        {
            return true;
        }
        if !self.entry_window_admitted(now) {
            return false;
        }
        let ref_result = self.responsive_reference(intent.pointer("/attribution/modelHash").and_then(V::as_str));
        let proof = current.get("referenceProof").cloned().unwrap_or(V::Null);
        let Ok(ref_ok) = ref_result else {
            self.inner.borrow_mut().reason = ref_result.err();
            return false;
        };
        let ref_proof = ref_ok.get("proof").unwrap();
        let exchange_id = intent.get("exchangeId").and_then(V::as_str).unwrap_or("");
        let ok = !proof.is_null()
            && proof.get("bankHash") == ref_proof.get("bankHash")
            && js_int(ref_proof.get("sourceSequence").unwrap_or(&V::Null))
                .zip(js_int(proof.get("sourceSequence").unwrap_or(&V::Null)))
                .is_some_and(|(r, p)| r > p || (r == p && ref_proof.get("sourceHash") == proof.get("sourceHash")))
            && proof.get("referenceHash").and_then(V::as_str)
                == Some(reference_input_hash(ref_ok.get("shadow").unwrap(), exchange_id).as_str());
        if !ok {
            self.inner.borrow_mut().reason = Some("REFERENCE_SOURCE_CHANGED".into());
        }
        ok
    }
    pub fn checked_book(&self, response: &V, market_id: &str) -> Result<V, String> {
        let b = normalize_book(
            response.get("data").unwrap_or(&V::Null),
            market_id,
            TID,
            response.get("receivedAt").and_then(V::as_str).unwrap_or(""),
            response.get("requestStartedAt").and_then(V::as_str).unwrap_or(""),
            20000.0,
            &std::collections::BTreeSet::new(),
            "authoritative_receipt_v1",
        )?;
        let expected: Vec<&V> = self
            .universe
            .get("exchanges")
            .and_then(V::as_array)
            .map(|xs| xs.iter().filter(|x| x.get("marketId").and_then(V::as_str) == Some(market_id)).collect())
            .unwrap_or_default();
        if expected.len() != 1
            || expected[0].get("exchangeId").and_then(V::as_str) != b.get("exchangeId").and_then(V::as_str)
        {
            return Err("BOOK_EXCHANGE_IDENTITY_MISMATCH".into());
        }
        Ok(b)
    }
    pub fn assert_ledgers(&self) -> Result<(), String> {
        let durable = self.ledger.borrow().snapshot().map_err(|e| e.message.clone())?;
        let durable_intents = durable.get("intents").and_then(V::as_object).cloned().unwrap_or_default();
        let state_intents = self.inner.borrow().state.get("intents").and_then(V::as_object).cloned().unwrap_or_default();
        if durable_intents.len() != state_intents.len() {
            return Err("EXECUTION_JOURNAL_DIVERGENCE".into());
        }
        for (id, i) in &state_intents {
            let d = durable_intents.get(id);
            if !(d.is_some()
                && d.unwrap().get("payloadHash") == i.get("payloadHash")
                && d.unwrap().get("status") == i.get("status")
                && d.unwrap().get("orderId").map(|v| crate::js_key(v)).unwrap_or_default()
                    == i.get("orderId").map(|v| crate::js_key(v)).unwrap_or_default())
            {
                return Err("EXECUTION_JOURNAL_DIVERGENCE".into());
            }
        }
        Ok(())
    }
    pub fn commit(&mut self, result: &V, event: &V) -> Result<(), String> {
        let state = result.get("state").cloned().unwrap_or(V::Null);
        validate_control(&state)?;
        let state = validate_execution_state(&state)?;
        self.journal.borrow_mut().commit(&state, event)?;
        self.inner.borrow_mut().state = state;
        Ok(())
    }
    pub fn update_control(&mut self, mutator: impl FnOnce(&mut V), event: &V) -> Result<(), String> {
        let mut state = self.inner.borrow().state.clone();
        mutator(state.pointer_mut("/control").unwrap_or(&mut V::Null));
        self.commit(&json!({"state": state}), event)
    }
    pub fn record(&mut self, result: &V, id: &str, event: &str) -> Result<(), String> {
        let mut result = result.clone();
        let i = result.pointer(&format!("/state/intents/{id}")).cloned().unwrap_or(V::Null);
        // Only authoritative order/fill/account reconciliation may discharge
        // an exit.
        if event == "official_reconciliation" {
            let mut state = result.get("state").cloned().unwrap_or(V::Null);
            let exchange_id = i.get("exchangeId").and_then(V::as_str).unwrap_or("");
            if state.pointer(&format!("/owned/{exchange_id}")).is_none() {
                if let Some(m) = state.pointer_mut(&format!("/control/exits/{exchange_id}")) {
                    *m = V::Null;
                    state.pointer_mut("/control/exits").unwrap().as_object_mut().unwrap().remove(exchange_id);
                }
            }
            if i.get("status").and_then(V::as_str) == Some("closed")
                && state.pointer(&format!("/control/cancellations/{id}")).is_some()
            {
                state
                    .pointer_mut(&format!("/control/cancellations/{id}/phase"))
                    .map(|p| *p = json!("reconciled"));
            }
            result.as_object_mut().unwrap().insert("state".into(), state);
        }
        let mut outcome = json!({
            "status": i.get("status").unwrap().clone(),
            "reasonCode": result.get("code").unwrap().clone(),
        });
        if i.get("orderId").is_some_and(|v| !v.is_null()) {
            outcome.as_object_mut().unwrap().insert(
                "orderId".into(),
                json!(crate::js_key(i.get("orderId").unwrap())),
            );
        }
        if i.get("status").and_then(V::as_str) == Some("uncertain") {
            outcome.as_object_mut().unwrap().insert(
                "retryAllowed".into(),
                json!(i.get("retryAllowed") == Some(&V::Bool(true))),
            );
            outcome.as_object_mut().unwrap().insert(
                "earliestRetryAt".into(),
                i.get("retryAfterAt").cloned().unwrap_or(V::Null),
            );
        }
        self.ledger.borrow_mut().record_outcome(id, &outcome).map_err(|e| e.message.clone())?;
        self.commit(&result, &json!({"type": event, "intentId": id, "code": result.get("code").unwrap().clone()}))?;
        self.assert_ledgers()
    }
    pub fn managed_market_ids(&self) -> Vec<String> {
        let inner = self.inner.borrow();
        let mut ids: Vec<String> = Vec::new();
        for x in inner.state.get("owned").and_then(V::as_object).map(|m| m.values()).into_iter().flatten() {
            let m = x.get("marketId").and_then(V::as_str).unwrap_or("").to_string();
            if !ids.contains(&m) {
                ids.push(m);
            }
        }
        for x in inner.state.get("intents").and_then(V::as_object).map(|m| m.values()).into_iter().flatten() {
            if x.get("status").and_then(V::as_str) != Some("closed") {
                let m = x.get("marketId").and_then(V::as_str).unwrap_or("").to_string();
                if !ids.contains(&m) {
                    ids.push(m);
                }
            }
        }
        ids
    }
    pub fn active(&self) -> Option<V> {
        self.inner
            .borrow()
            .state
            .get("intents")
            .and_then(V::as_object)
            .and_then(|m| m.values().find(|i| i.get("status").and_then(V::as_str) != Some("closed")).cloned())
    }
    pub fn snapshot(&self, book: Option<&V>) -> V {
        let inner = self.inner.borrow();
        let mut s = json!({
            "tournamentId": TID,
            "cash": inner.cash.clone().unwrap_or(V::Null),
            "positions": inner.account.as_ref().and_then(|a| a.get("positions").cloned()).unwrap_or(V::Null),
            "orders": inner.account.as_ref().and_then(|a| a.get("orders").cloned()).unwrap_or(V::Null),
        });
        if let Some(b) = book {
            s.as_object_mut().unwrap().insert("book".into(), b.clone());
        }
        s
    }
    pub fn cash_refresh(&mut self) -> Result<(), String> {
        let r = self.transport.tournament().map_err(|e| e.code)?;
        if !(r.pointer("/data/status").and_then(V::as_str) == Some("active")
            && r.pointer("/data/isPendingEnrolment") != Some(&V::Bool(true)))
        {
            return Err("TOURNAMENT_NOT_ACTIVE".into());
        }
        self.inner.borrow_mut().cash = Some(json!({
            "tournamentId": TID,
            "at": r.get("receivedAt").unwrap().clone(),
            "amount": r.pointer("/data/myBalance").unwrap().clone(),
        }));
        Ok(())
    }
    pub fn set_account(&mut self, positions: &V, orders: &V, orders_at: &V) -> Result<(), String> {
        let rows = positions.pointer("/data/positions").and_then(V::as_array);
        let order_rows = orders.as_array();
        if rows.is_none() || order_rows.is_none() {
            return Err("ACCOUNT_INCOMPLETE".into());
        }
        self.inner.borrow_mut().account = Some(json!({
            "positions": {
                "tournamentId": TID,
                "at": positions.get("receivedAt").unwrap().clone(),
                "complete": true,
                "rows": rows.unwrap().iter().map(|x| json!({
                    "exchangeId": x.get("exchangeId").unwrap().clone(),
                    "marketId": x.get("marketId").unwrap().clone(),
                    "quantity": x.get("quantity").unwrap().clone(),
                })).collect::<Vec<_>>(),
            },
            "orders": {
                "tournamentId": TID,
                "at": orders_at.clone(),
                "complete": true,
                "rows": order_rows.unwrap().clone(),
            },
        }));
        Ok(())
    }
    pub fn refresh_account(&mut self) -> Result<(), String> {
        let p = self.transport.positions().map_err(|e| e.code)?;
        let o = self.transport.orders("open").map_err(|e| e.code)?;
        self.set_account(&p, o.pointer("/data/data").unwrap_or(&V::Null), o.get("receivedAt").unwrap_or(&V::Null))
    }
    pub fn bootstrap(&mut self) -> V {
        {
            let mut inner = self.inner.borrow_mut();
            inner.bootstrapped = false;
            inner.readiness = json!({"complete": false, "at": iso(self.now_ms())});
        }
        let result = (|| -> Result<V, String> {
            if !self.can_operate() {
                return Err("EXECUTION_STOPPED".into());
            }
            self.cash_refresh()?;
            self.refresh_account()?;
            let market_id = if self.activation.get("schemaVersion").and_then(V::as_i64) == Some(2) {
                self.bootstrap_market_id()
            } else {
                "377".to_string()
            };
            let market = self.transport.market(&market_id).map_err(|e| e.code)?;
            let ctx = market.pointer("/data/contexts/0").cloned().unwrap_or(V::Null);
            if !(ctx.get("status").and_then(V::as_str) == Some("open")
                && ctx.get("settledWith").is_some_and(V::is_null)
                && market.pointer("/data/status").and_then(V::as_str) == Some("open")
                && !market.pointer("/data/isComposite").and_then(V::as_bool).unwrap_or(false)
                && !market.pointer("/data/isMultiOutcome").and_then(V::as_bool).unwrap_or(false))
            {
                return Err("BOOTSTRAP_MARKET_NOT_OPEN".into());
            }
            let response = self.transport.orderbook(&market_id).map_err(|e| e.code)?;
            let book = self.checked_book(&response, &market_id)?;
            let now = self.now_ms();
            {
                let inner = self.inner.borrow();
                for p in [inner.cash.clone(), inner.account.as_ref().and_then(|a| a.get("positions").cloned()), inner.account.as_ref().and_then(|a| a.get("orders").cloned())] {
                    let at = p.as_ref().and_then(|x| x.get("at")).and_then(V::as_str).map(|s| parse(&json!(s)));
                    let max_age = self.inner.borrow().state.pointer("/policy/maxAccountAgeMs").and_then(V::as_i64).unwrap_or(180000);
                    if !(at.is_some_and(|a| a != i64::MIN && a <= now && now - a <= max_age)) {
                        return Err("STALE_BOOTSTRAP_ACCOUNT".into());
                    }
                }
            }
            let max_book_age = self.inner.borrow().state.pointer("/policy/maxBookAgeMs").and_then(V::as_i64).unwrap_or(20000);
            if !(now - parse(book.get("receivedAt").unwrap()) <= max_book_age && self.can_operate()) {
                return Err("BOOTSTRAP_NOT_READY".into());
            }
            let mut inner = self.inner.borrow_mut();
            inner.bootstrapped = true;
            inner.readiness = json!({
                "complete": true,
                "at": iso(now),
                "marketId": market_id,
                "exchangeId": book.get("exchangeId").unwrap().clone(),
                "bookReceivedAt": book.get("receivedAt").unwrap().clone(),
            });
            inner.status = "monitoring".into();
            inner.reason = None;
            Ok(inner.readiness.clone())
        })();
        match result {
            Ok(r) => {
                self.publish();
                r
            }
            Err(e) => {
                let mut inner = self.inner.borrow_mut();
                inner.status = "bootstrap_blocked".into();
                inner.reason = Some(e);
                let reason = inner.reason.clone();
                let mut readiness = inner.readiness.clone();
                readiness.as_object_mut().unwrap().insert("reason".into(), json!(reason));
                inner.readiness = readiness;
                drop(inner);
                self.publish();
                self.inner.borrow().readiness.clone()
            }
        }
    }
    pub fn bootstrap_market_id(&self) -> String {
        if let Some(f) = self.inner.borrow().bootstrap_market_id.clone() {
            return f();
        }
        "377".into()
    }
    pub fn reconcile(&mut self, i: &V) -> Result<(), String> {
        let order_id = i.get("orderId").cloned().unwrap_or(V::Null);
        let order = self.transport.order(&order_id).map_err(|e| e.code)?;
        let fills = self.transport.fills(&order_id).map_err(|e| e.code)?;
        self.cash_refresh()?;
        self.refresh_account()?;
        let evidence_at = iso(parse(order.get("requestStartedAt").unwrap()).min(parse(fills.get("requestStartedAt").unwrap())));
        let fills_data = {
            let mut d = fills.get("data").cloned().unwrap_or(V::Null);
            let complete = d.pointer("/pagination/hasMore") == Some(&V::Bool(false));
            let rows = d.get("data").cloned();
            if let Some(m) = d.as_object_mut() {
                m.insert("complete".into(), json!(complete));
                if let Some(rows) = rows {
                    m.insert("rows".into(), rows);
                }
            }
            d
        };
        let snapshot = self.snapshot(None);
        let r = reconcile_intent(
            &self.inner.borrow().state,
            i.get("intentId").and_then(V::as_str).unwrap_or(""),
            &json!({
                "at": evidence_at,
                "fillsReceivedAt": fills.get("receivedAt").unwrap().clone(),
                "snapshot": snapshot,
                "order": order.get("data").unwrap().clone(),
                "fills": fills_data,
            }),
            &json!(self.now_ms()),
        );
        if r.get("ok") != Some(&V::Bool(true)) {
            let mut inner = self.inner.borrow_mut();
            inner.status = "reconciliation_blocked".into();
            inner.reason = Some(r.get("code").and_then(V::as_str).unwrap_or("").to_string());
            let code = inner.reason.clone().unwrap();
            drop(inner);
            if code == "OPEN_ORDER_RECONCILIATION_MISMATCH"
                && order.pointer("/data/open") == Some(&V::Bool(true))
                && self.now_ms() >= parse(i.get("expirationDate").unwrap())
            {
                self.cancel_expired(i, &json!({
                    "at": evidence_at,
                    "snapshot": self.snapshot(None),
                    "order": order.get("data").unwrap().clone(),
                }))?;
            }
            return Ok(());
        }
        self.record(&r, i.get("intentId").and_then(V::as_str).unwrap_or(""), "official_reconciliation")?;
        let status = r.pointer("/state/intents/status").and_then(V::as_str);
        {
            let mut inner = self.inner.borrow_mut();
            inner.status = if r.pointer(&format!("/intent/status")).and_then(V::as_str) == Some("closed") || status == Some("closed") {
                "monitoring".into()
            } else {
                "order_pending".into()
            };
            inner.reason = Some(r.get("code").and_then(V::as_str).unwrap_or("").to_string());
        }
        let intent = r.get("intent").cloned().unwrap_or(V::Null);
        if intent.get("status").and_then(V::as_str) == Some("reconciled")
            && self.now_ms() >= parse(intent.get("expirationDate").unwrap())
        {
            self.cancel_expired(&intent, &json!({
                "at": evidence_at,
                "snapshot": self.snapshot(None),
                "order": order.get("data").unwrap().clone(),
            }))?;
        }
        Ok(())
    }
    fn cancel_expired(&mut self, i: &V, evidence: &V) -> Result<(), String> {
        self.inner.borrow_mut().current_permit = Some(json!({
            "kind": "cancel",
            "intentId": i.get("intentId").unwrap().clone(),
            "evidence": evidence.clone(),
        }));
        let intent_id = i.get("intentId").and_then(V::as_str).unwrap_or("").to_string();
        let result = self.transport.cancel(
            i.get("orderId").unwrap(),
            &json!({
                "permit": {"kind": "cancel", "intentId": intent_id, "orderId": i.get("orderId").unwrap().clone(), "runId": self.activation.get("runId").unwrap().clone()},
                "lease": self.lease.clone(),
            }),
        );
        match result {
            Ok(_) => {
                self.update_control(
                    |c| {
                        let m = c.pointer_mut(&format!("/cancellations/{intent_id}"));
                        if let Some(m) = m {
                            m.as_object_mut().unwrap().insert("phase".into(), json!("acknowledged"));
                            m.as_object_mut().unwrap().insert("reasonCode".into(), json!("CANCEL_ACKNOWLEDGED"));
                        }
                    },
                    &json!({"type": "cancel_response", "intentId": intent_id, "orderId": i.get("orderId").unwrap().clone()}),
                )?;
                let mut inner = self.inner.borrow_mut();
                inner.status = "cancel_pending_reconciliation".into();
                inner.reason = Some("CANCEL_ACKNOWLEDGED".into());
            }
            Err(_) => {
                let phase = self
                    .inner
                    .borrow()
                    .state
                    .pointer(&format!("/control/cancellations/{intent_id}/phase"))
                    .and_then(V::as_str)
                    .map(str::to_string);
                if phase.as_deref() == Some("dispatching") {
                    self.update_control(
                        |c| {
                            let m = c.pointer_mut(&format!("/cancellations/{intent_id}"));
                            if let Some(m) = m {
                                m.as_object_mut().unwrap().insert("phase".into(), json!("uncertain"));
                                m.as_object_mut().unwrap().insert("reasonCode".into(), json!("CANCEL_UNCONFIRMED"));
                            }
                        },
                        &json!({"type": "cancel_uncertain", "intentId": intent_id, "orderId": i.get("orderId").unwrap().clone()}),
                    )?;
                }
                let mut inner = self.inner.borrow_mut();
                inner.status = "cancel_pending_reconciliation".into();
                inner.reason = Some("CANCEL_UNCONFIRMED".into());
            }
        }
        self.inner.borrow_mut().current_permit = None;
        self.publish();
        Ok(())
    }
    pub fn submit(&mut self, i: &V, snapshot: Option<&V>, retry: bool, reference_proof: Option<&V>) -> Result<(), String> {
        let intent_id = i.get("intentId").and_then(V::as_str).unwrap_or("").to_string();
        self.inner.borrow_mut().current_permit = Some(json!({
            "intentId": intent_id,
            "snapshot": snapshot.cloned().unwrap_or(V::Null),
            "retry": retry,
            "referenceProof": reference_proof.cloned().unwrap_or(V::Null),
        }));
        let result = self.transport.place(
            i.get("body").unwrap_or(&V::Null),
            &json!({
                "permit": {"intentId": intent_id, "runId": self.activation.get("runId").unwrap().clone(), "retry": retry},
                "lease": self.lease.clone(),
            }),
        );
        let outcome = match result {
            Ok(r) => Ok(json!({"status": r.get("status").unwrap().clone(), "response": r.get("data").unwrap().clone()})),
            Err(e) => Err(e),
        };
        let record_result = match outcome {
            Ok(outcome) => {
                let r = record_placement(&self.inner.borrow().state, &intent_id, &outcome, &json!(self.now_ms()));
                self.record(&r, &intent_id, "placement_response")
            }
            Err(e) => {
                let status = self
                    .inner
                    .borrow()
                    .state
                    .pointer(&format!("/intents/{intent_id}/status"))
                    .and_then(V::as_str)
                    .map(str::to_string);
                match status.as_deref() {
                    Some("dispatching") => {
                        let r = record_placement(
                            &self.inner.borrow().state,
                            &intent_id,
                            &json!({
                                "status": e.status.map(|s| json!(s)).unwrap_or(V::Null),
                                "code": e.api_code.clone().unwrap_or_else(|| "TRANSPORT_UNCONFIRMED".into()),
                                "retryAfterMs": e.retry_after_ms,
                            }),
                            &json!(self.now_ms()),
                        );
                        self.record(&r, &intent_id, "placement_uncertain")
                    }
                    Some("prepared") => {
                        let r = abandon_prepared_intent(
                            &self.inner.borrow().state,
                            &intent_id,
                            "ADMISSION_BLOCKED",
                            &json!(self.now_ms()),
                        );
                        self.record(&r, &intent_id, "not_submitted")
                    }
                    _ => {
                        self.inner.borrow_mut().current_permit = None;
                        self.publish();
                        return Err(e.code);
                    }
                }
            }
        };
        self.inner.borrow_mut().current_permit = None;
        self.publish();
        record_result
    }
    pub fn entry_decision(&mut self, book: &V, code: &str, details: &V) {
        let mut rec = json!({
            "book": book.clone(),
            "clockAt": iso(self.now_ms()),
            "code": code,
        });
        if let Some(d) = details.as_object() {
            for (k, v) in d {
                rec.as_object_mut().unwrap().insert(k.clone(), v.clone());
            }
        }
        let _ = self.entry_audit.borrow_mut().record(&rec);
        if code != "INTENT_READY" {
            let mut inner = self.inner.borrow_mut();
            inner.status = match code {
                "REFERENCE_BLOCKED" => "reference_blocked".into(),
                "WINDOW_BLOCKED" => {
                    if details.get("detailCode").and_then(V::as_str) == Some("ENTRY_WINDOW_NOT_STARTED") {
                        "entry_window_not_started".into()
                    } else {
                        "entry_window_closed".into()
                    }
                }
                _ => "entry_rejected".into(),
            };
            inner.reason = Some(if ["REFERENCE_BLOCKED", "WINDOW_BLOCKED", "PREPARE_REJECTED"].contains(&code) {
                details.get("detailCode").and_then(V::as_str).unwrap_or("").to_string()
            } else {
                code.to_string()
            });
            drop(inner);
            self.publish();
        }
    }
    pub fn before_books(&mut self, args: &V) -> Result<(), String> {
        {
            let mut inner = self.inner.borrow_mut();
            inner.selected = Vec::new();
            inner.reconciliation_risk_only = false;
            inner.current_permit = None;
        }
        self.assert_ledgers()?;
        if !self.can_operate() {
            self.inner.borrow_mut().status = "stopped".into();
            self.publish();
            return Ok(());
        }
        let needs_bootstrap = !self.inner.borrow().bootstrapped;
        if needs_bootstrap && self.bootstrap().get("complete") != Some(&V::Bool(true)) {
            return Ok(());
        }
        if !needs_bootstrap {
            self.set_account(
                args.get("positions").unwrap_or(&V::Null),
                args.get("orders").unwrap_or(&V::Null),
                args.get("ordersAt").unwrap_or(&V::Null),
            )?;
        }
        let mut active = self.active();
        if active.as_ref().and_then(|a| a.get("status")).and_then(V::as_str) == Some("prepared") {
            let id = active.as_ref().unwrap().get("intentId").and_then(V::as_str).unwrap_or("").to_string();
            let r = abandon_prepared_intent(&self.inner.borrow().state, &id, "RESTARTED_BEFORE_DISPATCH", &json!(self.now_ms()));
            self.record(&r, &id, "abandoned")?;
            active = None;
        }
        if active.as_ref().and_then(|a| a.get("status")).and_then(V::as_str) == Some("dispatching") {
            let id = active.as_ref().unwrap().get("intentId").and_then(V::as_str).unwrap_or("").to_string();
            let r = record_placement(
                &self.inner.borrow().state,
                &id,
                &json!({"status": V::Null, "code": "RESTART_UNCONFIRMED"}),
                &json!(self.now_ms()),
            );
            self.record(&r, &id, "interrupted_submission")?;
            active = self.active();
        }
        if active.as_ref().is_some_and(|a| a.get("orderId").is_some_and(|o| !o.is_null())) {
            self.reconcile(active.as_ref().unwrap())?;
            let residual = self
                .inner
                .borrow()
                .state
                .pointer(&format!("/owned/{}", active.as_ref().unwrap().get("exchangeId").and_then(V::as_str).unwrap_or("")))
                .cloned();
            // Keep this path reduce-only; uncertain/open orders and new entries
            // still wait.
            let a = active.as_ref().unwrap();
            if self
                .inner
                .borrow()
                .state
                .pointer(&format!("/intents/{}/status", a.get("intentId").and_then(V::as_str).unwrap_or("")))
                .and_then(V::as_str)
                == Some("closed")
                && self.active().is_none()
                && residual.as_ref().and_then(|r| r.get("marketId")).and_then(V::as_str)
                    == a.get("marketId").and_then(V::as_str)
                && (a.get("action").and_then(V::as_str) == Some("buy")
                    || (a.get("action").and_then(V::as_str) == Some("sell")
                        && self
                            .inner
                            .borrow()
                            .state
                            .pointer(&format!("/control/exits/{}", a.get("exchangeId").and_then(V::as_str).unwrap_or("")))
                            .is_some()))
            {
                let id = residual.as_ref().unwrap().get("marketId").and_then(V::as_str).unwrap().to_string();
                self.inner.borrow_mut().markets.remove(&id);
                let r = self.transport.market(&id).map_err(|e| e.code)?;
                let ctx = r.pointer("/data/contexts/0").cloned().unwrap_or(V::Null);
                if ctx.get("status").and_then(V::as_str) == Some("open")
                    && ctx.get("settledWith").is_some_and(V::is_null)
                    && r.pointer("/data/status").and_then(V::as_str) == Some("open")
                    && !r.pointer("/data/isComposite").and_then(V::as_bool).unwrap_or(false)
                    && !r.pointer("/data/isMultiOutcome").and_then(V::as_bool).unwrap_or(false)
                {
                    let mut inner = self.inner.borrow_mut();
                    inner.markets.insert(id.clone(), json!({
                        "at": r.get("receivedAt").unwrap().clone(),
                        "title": r.pointer("/data/title").unwrap().clone(),
                        "status": "open",
                    }));
                    inner.selected = vec![id];
                    inner.reconciliation_risk_only = true;
                }
            }
            self.publish();
            return Ok(());
        }
        if let Some(active) = &active {
            {
                let mut inner = self.inner.borrow_mut();
                inner.status = "submission_uncertain".into();
                inner.reason = active.get("reasonCode").and_then(V::as_str).map(str::to_string);
            }
            // Recovery is deliberate: do not turn an ambiguous request into a
            // new entry.
            if active.get("retryAllowed") == Some(&V::Bool(true))
                && self.now_ms() >= parse(active.get("retryAfterAt").unwrap())
                && self.now_ms() > parse(active.get("expirationDate").unwrap())
            {
                self.submit(active, None, true, None)?;
            }
            self.publish();
            return Ok(());
        }
        let (held, scope): (Vec<String>, std::collections::HashSet<String>) = {
            let inner = self.inner.borrow();
            (
                inner
                    .state
                    .get("owned")
                    .and_then(V::as_object)
                    .map(|m| m.values().filter_map(|x| x.get("marketId").and_then(V::as_str).map(str::to_string)).collect())
                    .unwrap_or_default(),
                self.universe
                    .get("exchanges")
                    .and_then(V::as_array)
                    .map(|xs| xs.iter().filter_map(|x| x.get("marketId").and_then(V::as_str).map(str::to_string)).collect())
                    .unwrap_or_default(),
            )
        };
        let candidates: Vec<String> = args
            .get("candidateMarketIds")
            .and_then(V::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).filter(|id| scope.contains(id)).collect())
            .unwrap_or_default();
        let mut selected: Vec<String> = Vec::new();
        for id in held.into_iter().chain(candidates) {
            if !selected.contains(&id) {
                selected.push(id);
            }
        }
        selected.truncate(3);
        if selected.is_empty() {
            self.inner.borrow_mut().status = "waiting_for_signal".into();
            self.publish();
            return Ok(());
        }
        self.cash_refresh()?;
        for id in &selected {
            self.inner.borrow_mut().markets.remove(id);
            let r = self.transport.market(id).map_err(|e| e.code)?;
            let ctx = r.pointer("/data/contexts/0").cloned().unwrap_or(V::Null);
            if ctx.get("status").and_then(V::as_str) == Some("open")
                && ctx.get("settledWith").is_some_and(V::is_null)
                && r.pointer("/data/status").and_then(V::as_str) == Some("open")
                && !r.pointer("/data/isComposite").and_then(V::as_bool).unwrap_or(false)
                && !r.pointer("/data/isMultiOutcome").and_then(V::as_bool).unwrap_or(false)
            {
                self.inner.borrow_mut().markets.insert(id.clone(), json!({
                    "at": r.get("receivedAt").unwrap().clone(),
                    "title": r.pointer("/data/title").unwrap().clone(),
                    "status": "open",
                }));
            }
        }
        self.inner.borrow_mut().selected = selected;
        {
            let mut inner = self.inner.borrow_mut();
            inner.status = "monitoring".into();
            inner.reason = None;
        }
        self.publish();
        Ok(())
    }
    pub fn on_book(&mut self, response: &V) -> Result<(), String> {
        let market_id = response
            .get("endpoint")
            .and_then(V::as_str)
            .and_then(|e| e.strip_prefix("/markets/"))
            .and_then(|r| r.split('/').next())
            .filter(|m| m.bytes().all(|b| b.is_ascii_digit()))
            .map(str::to_string);
        let Some(market_id) = market_id else { return Ok(()) };
        {
            let inner = self.inner.borrow();
            let market_fresh = inner
                .markets
                .get(&market_id)
                .and_then(|m| m.get("at"))
                .and_then(V::as_str)
                .map(|s| self.now_ms() - parse(&json!(s)) <= 180000)
                .unwrap_or(false);
            if !(self.can_write_gate(&inner) && self.active().is_none() && inner.selected.contains(&market_id) && inner.markets.contains_key(&market_id) && market_fresh) {
                return Ok(());
            }
        }
        let b = self.checked_book(response, &market_id)?;
        let owned = self
            .inner
            .borrow()
            .state
            .pointer(&format!("/owned/{}", b.get("exchangeId").and_then(V::as_str).unwrap_or("")))
            .cloned();
        if self.inner.borrow().reconciliation_risk_only && owned.is_none() {
            return Ok(());
        }
        let (bank, shadow, reference_proof);
        if let Some(owned) = &owned {
            bank = std::fs::read_to_string(self.runtime_dir.join("dip-responsive-bank.json"))
                .ok()
                .and_then(|s| serde_json::from_str::<V>(&s).ok())
                .ok_or("PILOT_REFERENCE_IDENTITY_CHANGED")?;
            let entries: Vec<V> = self
                .inner
                .borrow()
                .state
                .get("intents")
                .and_then(V::as_object)
                .map(|m| {
                    m.values()
                        .filter(|i| {
                            i.get("exchangeId") == b.get("exchangeId")
                                && i.get("action").and_then(V::as_str) == Some("buy")
                                && i.get("filledQuantity").and_then(V::as_i64).unwrap_or(0) > 0
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            let hash = digest(&bank);
            if !(bank.pointer("/policy/executionPolicy").and_then(V::as_str) == Some("responsive_v2")
                && !entries.is_empty()
                && entries.iter().all(|i| i.pointer("/attribution/modelHash").and_then(V::as_str) == Some(hash.as_str())))
            {
                return Err("PILOT_REFERENCE_IDENTITY_CHANGED".into());
            }
            let _ = owned;
            shadow = V::Null;
            reference_proof = V::Null;
        } else {
            let ref_ok = match self.responsive_reference(None) {
                Ok(r) => r,
                Err(code) => {
                    self.entry_decision(&b, "REFERENCE_BLOCKED", &json!({"detailCode": code}));
                    return Ok(());
                }
            };
            bank = ref_ok.get("bank").unwrap().clone();
            shadow = ref_ok.get("shadow").unwrap().clone();
            let exchange_id = b.get("exchangeId").and_then(V::as_str).unwrap_or("");
            let mut proof = ref_ok.get("proof").unwrap().clone();
            proof.as_object_mut().unwrap().insert(
                "referenceHash".into(),
                json!(reference_input_hash(&shadow, exchange_id)),
            );
            reference_proof = proof;
        }
        let bank_hash = digest(&bank);
        let ask = micros(b.pointer("/asks/0/price").unwrap_or(&V::Null));
        let bid = micros(b.pointer("/bids/0/price").unwrap_or(&V::Null));
        let (action, limit, mut quantity, reason);
        if let Some(owned) = &owned {
            let entries: Vec<V> = self
                .inner
                .borrow()
                .state
                .get("intents")
                .and_then(V::as_object)
                .map(|m| {
                    m.values()
                        .filter(|i| {
                            i.get("exchangeId") == b.get("exchangeId")
                                && i.get("action").and_then(V::as_str) == Some("buy")
                                && i.get("filledQuantity").and_then(V::as_i64).unwrap_or(0) > 0
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            let entry = entries.last().cloned().unwrap_or(V::Null);
            let first_fill = entry.pointer("/reconciledFills/0/filledAt").and_then(V::as_str).map(str::to_string);
            let prepared = entry.get("preparedAt").and_then(V::as_str).map(str::to_string);
            let entry_at = first_fill.or(prepared).unwrap_or_else(|| iso(self.now_ms()));
            let age = self.now_ms() - parse(&json!(entry_at));
            let avg = micros(owned.get("cost").unwrap_or(&V::Null)) / owned.get("quantity").and_then(V::as_i64).map(|q| q as i128).unwrap_or(1);
            let stop_loss = micros(bank.pointer("/policy/stopLoss").unwrap_or(&V::Null));
            let risk = bid <= avg - stop_loss
                || age >= bank.pointer("/policy/maximumHoldMs").and_then(V::as_i64).unwrap_or(0);
            if risk
                && self
                    .inner
                    .borrow()
                    .state
                    .pointer(&format!("/control/exits/{}", b.get("exchangeId").and_then(V::as_str).unwrap_or("")))
                    .is_none()
            {
                let triggered = iso(self.now_ms());
                let exchange = b.get("exchangeId").and_then(V::as_str).unwrap_or("").to_string();
                let obligation = json!({
                    "marketId": market_id,
                    "reason": if bid <= avg - stop_loss { "stop_loss" } else { "holding_timeout" },
                    "triggeredAt": triggered,
                    "triggerBid": format_micros(bid),
                    "sourceVersion": b.get("versionKey").unwrap().clone(),
                });
                let exit_insert = obligation.clone();
                self.update_control(
                    move |c| {
                        c.pointer_mut("/exits").unwrap().as_object_mut().unwrap().insert(exchange.clone(), exit_insert.clone());
                    },
                    &json!({"type": "exit_obligation_latched", "exchangeId": b.get("exchangeId").unwrap().clone(), "marketId": market_id}),
                )?;
            }
            let obligation = self
                .inner
                .borrow()
                .state
                .pointer(&format!("/control/exits/{}", b.get("exchangeId").and_then(V::as_str).unwrap_or("")))
                .cloned();
            if obligation.is_none() && bid < avg + micros(bank.pointer("/policy/takeProfit").unwrap_or(&V::Null)) {
                return Ok(());
            }
            action = "sell";
            limit = bid;
            quantity = owned.get("quantity").and_then(V::as_i64).unwrap_or(0) as f64;
            reason = obligation
                .as_ref()
                .and_then(|o| o.get("reason"))
                .and_then(V::as_str)
                .map(|r| format!("latched_{r}"))
                .unwrap_or_else(|| "take_profit".into());
        } else {
            if !self.entry_window_admitted(self.now_ms()) {
                let detail = self.inner.borrow().reason.clone().unwrap_or_default();
                self.entry_decision(&b, "WINDOW_BLOCKED", &json!({"modelHash": bank_hash, "referenceProof": reference_proof, "detailCode": detail}));
                return Ok(());
            }
            let identity = bank.pointer(&format!("/identities/{market_id}")).cloned().unwrap_or(V::Null);
            if !(identity.get("state").is_some() && identity.get("office").is_some()) {
                self.entry_decision(&b, "UNKNOWN_IDENTITY", &json!({"modelHash": bank_hash, "referenceProof": reference_proof}));
                return Ok(());
            }
            let concentration = self
                .inner
                .borrow()
                .state
                .get("owned")
                .and_then(V::as_object)
                .map(|m| {
                    m.values().any(|p| {
                        bank.pointer(&format!(
                            "/identities/{}/state",
                            p.get("marketId").and_then(V::as_str).unwrap_or("")
                        )) == identity.get("state").map(|v| v as &V).or(None)
                    })
                })
                .unwrap_or(false);
            if concentration {
                self.entry_decision(&b, "STATE_CONCENTRATION", &json!({"modelHash": bank_hash, "referenceProof": reference_proof}));
                return Ok(());
            }
            let external = {
                let inner = self.inner.borrow();
                let account = inner.account.clone().unwrap_or(V::Null);
                account.pointer("/positions/rows").and_then(V::as_array).is_some_and(|rows| {
                    rows.iter().any(|p| {
                        p.get("exchangeId") == b.get("exchangeId")
                            && p.get("quantity").and_then(V::as_i64).unwrap_or(0) != 0
                    })
                }) || account.pointer("/orders/rows").and_then(V::as_array).is_some_and(|rows| {
                    rows.iter().any(|o| o.get("exchangeId") == b.get("exchangeId"))
                })
            };
            if external {
                self.entry_decision(&b, "EXTERNAL_EXPOSURE", &json!({"modelHash": bank_hash, "referenceProof": reference_proof}));
                return Ok(());
            }
            let last_closed = self
                .inner
                .borrow()
                .state
                .get("intents")
                .and_then(V::as_object)
                .and_then(|m| {
                    m.values()
                        .filter(|i| {
                            i.get("exchangeId") == b.get("exchangeId")
                                && i.get("status").and_then(V::as_str) == Some("closed")
                        })
                        .last()
                        .cloned()
                });
            if let Some(last) = last_closed {
                if self.now_ms() - parse(last.get("closedAt").unwrap())
                    < bank.pointer("/policy/cooldownMs").and_then(V::as_i64).unwrap_or(0)
                {
                    self.entry_decision(&b, "COOLDOWN", &json!({"modelHash": bank_hash, "referenceProof": reference_proof}));
                    return Ok(());
                }
            }
            let exchange_id = b.get("exchangeId").and_then(V::as_str).unwrap_or("");
            let ref_input = historical_reference(
                shadow.pointer(&format!("/histories/{exchange_id}")),
                &shadow
                    .pointer(&format!("/bookHistory/{exchange_id}"))
                    .and_then(V::as_array)
                    .cloned()
                    .unwrap_or_default(),
                b.get("requestStartedAt").and_then(V::as_str).unwrap_or(""),
                bank.get("policy").unwrap_or(&V::Null),
            )
            .unwrap_or(V::Null);
            let effective_ask = format_micros(ask + 5000);
            let effective_bid = if bid >= 5000 {
                format_micros(bid - 5000)
            } else {
                format!("-{}", format_micros(5000 - bid))
            };
            let signal = evaluate_entry_gates(
                &ref_input,
                &json!(effective_ask),
                &json!(effective_bid),
                bank.get("policy").unwrap_or(&V::Null),
            )?;
            if signal.get("admitted") != Some(&V::Bool(true)) {
                let assessment = assess_entry_gates(
                    &ref_input,
                    &json!(effective_ask),
                    &json!(effective_bid),
                    bank.get("policy").unwrap_or(&V::Null),
                )
                .unwrap_or(V::Null);
                self.entry_decision(&b, "SIGNAL_BLOCKED", &json!({
                    "modelHash": bank_hash,
                    "referenceProof": reference_proof,
                    "assessment": assessment,
                }));
                return Ok(());
            }
            let cumulative: i128 = self
                .inner
                .borrow()
                .state
                .get("intents")
                .and_then(V::as_object)
                .map(|m| {
                    m.values()
                        .filter(|i| {
                            i.get("action").and_then(V::as_str) == Some("buy")
                                && i.get("attempts").and_then(V::as_i64).unwrap_or(0) > 0
                        })
                        .map(|i| micros(i.get("price").unwrap_or(&V::Null)) * i.get("quantity").and_then(V::as_i64).map(|q| q as i128).unwrap_or(0))
                        .sum()
                })
                .unwrap_or(0);
            action = "buy";
            limit = ask;
            quantity = (micros(self.activation.get("maxCumulativeBuyCost").unwrap_or(&V::Null)) - cumulative) as f64 / limit as f64;
            reason = "historical_discount".into();
        }
        let depth: Vec<&V> = if action == "buy" {
            b.get("asks").and_then(V::as_array).map(|a| a.iter().collect()).unwrap_or_default()
        } else {
            b.get("bids").and_then(V::as_array).map(|a| a.iter().collect()).unwrap_or_default()
        };
        let available: f64 = depth
            .iter()
            .filter(|x| micros(x.get("price").unwrap_or(&V::Null)) == limit)
            .map(|x| x.get("quantity").and_then(V::as_f64).unwrap_or(0.0))
            .sum::<f64>();
        let max_order = micros(self.activation.get("maxOrderCost").unwrap_or(&V::Null));
        let depth_fraction = self.activation.get("depthFractionBps").and_then(V::as_i64).unwrap_or(100) as f64;
        quantity = quantity.min(max_order as f64 / limit as f64).min((available * depth_fraction / 10000.0).floor());
        if quantity.fract() != 0.0 || quantity < 1.0 {
            if action == "buy" {
                self.entry_decision(&b, "ZERO_EXECUTABLE_SIZE", &json!({"modelHash": bank_hash, "referenceProof": reference_proof}));
            }
            return Ok(());
        }
        let quantity = quantity as i64;
        let spec = json!({
            "intentId": format!("intent-{}", uuid::Uuid::new_v4()),
            "idempotencyKey": uuid::Uuid::new_v4().to_string(),
            "marketId": market_id,
            "exchangeId": b.get("exchangeId").unwrap().clone(),
            "side": "yes",
            "action": action,
            "quantity": quantity,
            "price": format_micros(limit),
            "expirationDate": iso(self.now_ms() + 30000),
            "attribution": {
                "runId": self.activation.get("runId").unwrap().clone(),
                "strategyId": "historical-dip-responsive-v2",
                "modelHash": bank_hash,
                "signalId": b.get("versionKey").unwrap().clone(),
                "reason": reason,
            },
        });
        let snapshot = self.snapshot(Some(&json!({
            "tournamentId": TID,
            "marketId": market_id,
            "exchangeId": b.get("exchangeId").unwrap().clone(),
            "at": b.get("receivedAt").unwrap().clone(),
            "requestStartedAt": b.get("requestStartedAt").unwrap().clone(),
            "bids": b.get("bids").unwrap().clone(),
            "asks": b.get("asks").unwrap().clone(),
        })));
        let r = prepare_intent(&self.inner.borrow().state, &spec, &snapshot, &json!(self.now_ms()));
        if r.get("ok") != Some(&V::Bool(true)) {
            if action == "buy" {
                self.entry_decision(&b, "PREPARE_REJECTED", &json!({
                    "modelHash": bank_hash,
                    "referenceProof": reference_proof,
                    "detailCode": r.get("code").unwrap().clone(),
                }));
            } else {
                self.inner.borrow_mut().reason = r.get("code").and_then(V::as_str).map(str::to_string);
                self.publish();
            }
            return Ok(());
        }
        if action == "buy" {
            self.entry_decision(&b, "INTENT_READY", &json!({"modelHash": bank_hash, "referenceProof": reference_proof}));
        }
        let intent = r.get("intent").cloned().unwrap_or(V::Null);
        self.ledger.borrow_mut().prepare_intent(&json!({
            "method": "POST",
            "path": "/orders",
            "intentId": spec.get("intentId").unwrap().clone(),
            "idempotencyKey": spec.get("idempotencyKey").unwrap().clone(),
            "body": intent.get("body").unwrap().clone(),
            "bodyJson": intent.get("bodyJson").unwrap().clone(),
            "payloadHash": intent.get("payloadHash").unwrap().clone(),
            "attribution": {
                "ownerId": "Codex",
                "runId": self.activation.get("runId").unwrap().clone(),
                "strategyId": spec.pointer("/attribution/strategyId").unwrap().clone(),
                "modelHash": bank_hash,
                "marketId": market_id,
                "exchangeId": b.get("exchangeId").unwrap().clone(),
            },
        })).map_err(|e| e.message.clone())?;
        self.commit(&r, &json!({"type": "intent_prepared", "intentId": spec.get("intentId").unwrap().clone(), "reason": reason, "referenceProof": reference_proof}))?;
        self.submit(&intent, Some(&snapshot), false, Some(&reference_proof))
    }
    fn can_write_gate(&self, inner: &ControllerInner) -> bool {
        if !inner.bootstrapped || inner.halted || (self.is_stopped)() || self.runtime_dir.join("EXECUTION_STOP").exists() {
            return false;
        }
        if self.activation.get("schemaVersion").and_then(V::as_i64) == Some(2)
            && !production_handoff_current(&self.runtime_dir, &self.activation)
        {
            return false;
        }
        std::fs::read_to_string(self.runtime_dir.join("competition-activation.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<V>(&s).ok())
            .and_then(|a| payload_hash(&a).ok())
            .as_deref()
            == Some(self.activation_hash.as_str())
    }
    pub fn publish(&self) {
        let inner = self.inner.borrow();
        let intents: Vec<V> = inner
            .state
            .get("intents")
            .and_then(V::as_object)
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default();
        let mut fills: Vec<V> = Vec::new();
        for i in &intents {
            for f in i.get("reconciledFills").and_then(V::as_array).cloned().unwrap_or_default() {
                let mut f = f.clone();
                let m = f.as_object_mut().unwrap();
                m.insert("action".into(), i.get("action").unwrap().clone());
                m.insert("marketId".into(), i.get("marketId").unwrap().clone());
                m.insert("exchangeId".into(), i.get("exchangeId").unwrap().clone());
                m.insert("orderId".into(), i.get("orderId").cloned().unwrap_or(V::Null));
                fills.push(f);
            }
        }
        let mut cash_flow: i128 = 0;
        for f in &fills {
            let sign: i128 = if f.get("action").and_then(V::as_str) == Some("sell") { 1 } else { -1 };
            cash_flow += sign * micros(f.get("price").unwrap_or(&V::Null)) * f.get("quantity").and_then(V::as_i64).map(|q| q as i128).unwrap_or(0);
        }
        let cost: i128 = inner
            .state
            .get("owned")
            .and_then(V::as_object)
            .map(|m| m.values().map(|p| micros(p.get("cost").unwrap_or(&V::Null))).sum())
            .unwrap_or(0);
        let realized = cash_flow + cost;
        let active = inner
            .state
            .get("intents")
            .and_then(V::as_object)
            .and_then(|m| m.values().find(|i| i.get("status").and_then(V::as_str) != Some("closed")));
        let stats = json!({
            "mode": "official_quant_execution",
            "owner": "Codex",
            "runId": self.activation.get("runId").unwrap().clone(),
            "updatedAt": iso(self.now_ms()),
            "status": inner.status,
            "reason": inner.reason,
            "readiness": inner.readiness,
            "limits": {
                "maxOrderCost": self.activation.get("maxOrderCost").unwrap().clone(),
                "maxAggregateCost": self.activation.get("maxAggregateCost").unwrap().clone(),
                "maxCumulativeBuyCost": self.activation.get("maxCumulativeBuyCost").unwrap().clone(),
                "maxManagedMarkets": 3,
                "limitExpiryMs": 30000,
            },
            "ordersSubmitted": intents.iter().filter(|i| i.get("orderId").is_some_and(|o| !o.is_null())).count(),
            "dispatchAttempts": intents.iter().map(|i| i.get("attempts").and_then(V::as_i64).unwrap_or(0)).sum::<i64>(),
            "unconfirmedIntents": intents.iter().filter(|i| {
                i.get("attempts").and_then(V::as_i64).unwrap_or(0) > 0
                    && i.get("orderId").map(|o| o.is_null()).unwrap_or(true)
                    && i.get("status").and_then(V::as_str) != Some("closed")
            }).count(),
            "openIntent": active.map(|a| json!({
                "intentId": a.get("intentId").unwrap().clone(),
                "status": a.get("status").unwrap().clone(),
                "orderId": a.get("orderId").cloned().unwrap_or(V::Null),
            })).unwrap_or(V::Null),
            "positions": inner.state.get("owned").unwrap().clone(),
            "exitObligations": inner.state.pointer("/control/exits").unwrap().clone(),
            "cancellations": inner.state.pointer("/control/cancellations").unwrap().clone(),
            "fills": fills,
            "realizedPnl": format!("{}{}", if realized < 0 { "-" } else { "" }, format_micros(realized.abs())),
            "addedFees": "0.000000",
            "entryDecisions": self.entry_audit.borrow().status(),
            "profitabilityEstablished": false,
        });
        drop(inner);
        let _ = atomic_json(&self.runtime_dir.join("competition-stats.json"), &stats);
    }
    pub fn failure(&mut self, error: &V) {
        let transient = [429, 500, 502, 503, 504]
            .contains(&error.get("status").and_then(V::as_i64).unwrap_or(0))
            || error.get("code").and_then(V::as_str) == Some("REQUEST_UNCONFIRMED");
        let mut inner = self.inner.borrow_mut();
        inner.halted = !transient;
        inner.status = if transient { "reconciliation_retry".into() } else { "blocked".into() };
        inner.reason = Some(
            error
                .get("code")
                .and_then(V::as_str)
                .unwrap_or("EXECUTION_RECONCILIATION_UNAVAILABLE")
                .to_string(),
        );
        drop(inner);
        self.publish();
    }
    pub fn close(&mut self) -> Result<(), String> {
        self.ledger.borrow_mut().close().map_err(|e| e.message.clone())
    }
}

// admit() and networkAdmit() are free functions so the transport guard closures
// can re-enter the shared state without a live &mut borrow on the controller.
fn controller_admit(
    inner: &Rc<RefCell<ControllerInner>>,
    ledger: &Rc<RefCell<IntentLedger>>,
    activation: &V,
    _lease: &V,
    now: &Rc<dyn Fn() -> f64>,
    runtime_dir: &Path,
    journal: Option<&Rc<RefCell<Journal>>>,
    permit: &V,
    spec: &V,
) -> bool {
    if let Some(hook) = inner.borrow().pre_admit.clone() {
        hook(inner);
    }
    let current = inner.borrow().current_permit.clone().unwrap_or(V::Null);
    if permit.get("intentId") != current.get("intentId")
        || permit.get("runId") != activation.get("runId")
    {
        return false;
    }
    let now_ms = now() as i64;
    if spec.get("method").and_then(V::as_str) == Some("DELETE") {
        let active = inner
            .borrow()
            .state
            .get("intents")
            .and_then(V::as_object)
            .and_then(|m| m.values().find(|i| i.get("status").and_then(V::as_str) != Some("closed")).cloned());
        let i = active.unwrap_or(V::Null);
        let e = current.get("evidence").cloned().unwrap_or(V::Null);
        let durable = ledger.borrow().snapshot().ok().and_then(|s| {
            s.pointer(&format!("/intents/{}", permit.get("intentId").and_then(V::as_str).unwrap_or("")))
                .cloned()
        });
        let max_age = inner
            .borrow()
            .state
            .pointer("/policy/maxAccountAgeMs")
            .and_then(V::as_i64)
            .unwrap_or(180000);
        let order = e.get("order").cloned().unwrap_or(V::Null);
        let ok = permit.get("kind").and_then(V::as_str) == Some("cancel")
            && current.get("kind").and_then(V::as_str) == Some("cancel")
            && !i.is_null()
            && i.get("intentId") == permit.get("intentId")
            && i.get("orderId").is_some_and(|o| !o.is_null())
            && permit.get("orderId") == i.get("orderId")
            && durable.as_ref().map(|d| crate::js_key(d.get("orderId").unwrap_or(&V::Null)))
                == Some(crate::js_key(i.get("orderId").unwrap()))
            && spec.get("endpoint").and_then(V::as_str)
                == Some(format!("/orders/{}", crate::js_key(i.get("orderId").unwrap())).as_str())
            && now_ms >= parse(i.get("expirationDate").unwrap())
            && order.get("id") == i.get("orderId")
            && order.get("open") == Some(&V::Bool(true))
            && order.get("tournamentId").and_then(V::as_str) == Some(TID)
            && order.get("exchangeId") == i.get("exchangeId")
            && order.get("side").and_then(V::as_str) == Some("yes")
            && order.get("action") == i.get("action")
            && micros(order.get("priceLimit").unwrap_or(&V::Null)) == micros(i.get("price").unwrap_or(&V::Null))
            && parse(order.get("expirationDate").unwrap()) == parse(i.get("expirationDate").unwrap())
            && parse(e.get("at").unwrap()) != i64::MIN
            && parse(e.get("at").unwrap()) <= now_ms
            && now_ms - parse(e.get("at").unwrap()) <= max_age;
        if !ok {
            return false;
        }
        for k in ["cash", "positions", "orders"] {
            let p = e.pointer(&format!("/snapshot/{k}")).cloned().unwrap_or(V::Null);
            let part_ok = p.get("tournamentId").and_then(V::as_str) == Some(TID)
                && parse(p.get("at").unwrap()) != i64::MIN
                && parse(p.get("at").unwrap()) <= now_ms
                && now_ms - parse(p.get("at").unwrap()) <= max_age
                && (k == "cash" || (p.get("complete") == Some(&V::Bool(true)) && p.get("rows").is_some_and(V::is_array)));
            if !part_ok {
                return false;
            }
        }
        let intent_id = i.get("intentId").and_then(V::as_str).unwrap_or("").to_string();
        let mut state = inner.borrow().state.clone();
        let attempts = state
            .pointer(&format!("/control/cancellations/{intent_id}/attempts"))
            .and_then(V::as_i64)
            .unwrap_or(0)
            + 1;
        state
            .pointer_mut("/control/cancellations")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(
                intent_id.clone(),
                json!({
                    "orderId": i.get("orderId").unwrap().clone(),
                    "attempts": attempts,
                    "phase": "dispatching",
                    "lastAttemptAt": iso(now_ms),
                    "evidenceAt": e.get("at").unwrap().clone(),
                }),
            );
        // commit equivalent: validate + journal append + swap state
        if validate_control(&state).is_err() || validate_execution_state(&state).is_err() {
            return false;
        }
        if let Some(j) = journal {
            if j.borrow_mut()
                .commit(&state, &json!({"type": "cancel_dispatch_admitted", "intentId": intent_id, "orderId": i.get("orderId").unwrap().clone()}))
                .is_err()
            {
                return false;
            }
        }
        inner.borrow_mut().state = state;
        return true;
    }
    if spec.get("method").and_then(V::as_str) != Some("POST")
        || permit.get("kind").is_some()
        || current.get("kind").is_some()
    {
        return false;
    }
    let intent_id = permit.get("intentId").and_then(V::as_str).unwrap_or("");
    let i = inner
        .borrow()
        .state
        .pointer(&format!("/intents/{intent_id}"))
        .cloned()
        .unwrap_or(V::Null);
    if i.is_null()
        || i.get("payloadHash").and_then(V::as_str) != spec.get("body").map(|b| payload_hash(b).unwrap_or_default()).as_deref()
        || spec.get("bodyJson") != i.get("bodyJson")
    {
        return false;
    }
    // entryReferenceAdmitted — pluggable so subclass controllers override the
    // reference gate exactly as the JS prototype chain does.
    if !entry_reference_gate(inner, activation, runtime_dir, &i, permit, now_ms) {
        return false;
    }
    let r = begin_dispatch(
        &inner.borrow().state,
        intent_id,
        current.get("snapshot").unwrap_or(&V::Null),
        &json!(now_ms),
        permit.get("retry") == Some(&V::Bool(true)),
    );
    if r.get("ok") != Some(&V::Bool(true)) {
        inner.borrow_mut().reason = r.get("code").and_then(V::as_str).map(str::to_string);
        return false;
    }
    if ledger
        .borrow_mut()
        .mark_dispatching(intent_id, permit.get("retry") == Some(&V::Bool(true)))
        .is_err()
    {
        return false;
    }
    // journal commit
    let state = validate_execution_state(r.get("state").unwrap()).ok().filter(|s| validate_control(s).is_ok());
    let Some(state) = state else { return false };
    if let Some(j) = journal {
        if j.borrow_mut()
            .commit(&state, &json!({"type": "dispatch_admitted", "intentId": intent_id}))
            .is_err()
        {
            return false;
        }
    }
    inner.borrow_mut().state = state;
    true
}
// Free-standing responsiveReference + entryReferenceAdmitted for guard use:
// the transport closure holds runtime_dir/activation, not the controller.
fn responsive_reference_at(
    runtime_dir: &Path,
    now_ms: i64,
    expected_hash: Option<&str>,
) -> Result<V, String> {
    let read = |name: &str| -> Result<V, String> {
        std::fs::read_to_string(runtime_dir.join(name))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .ok_or_else(|| "REFERENCE_WORKER_UNAVAILABLE".to_string())
    };
    let bank = read("dip-responsive-bank.json")?;
    let shadow = read("dip-responsive-state.json")?;
    let stats = read("dip-responsive-stats.json")?;
    let hash = digest(&bank);
    if !(bank.pointer("/policy/executionPolicy").and_then(V::as_str) == Some("responsive_v2")
        && shadow.get("bankHash").and_then(V::as_str) == Some(hash.as_str())
        && stats.get("bankHash").and_then(V::as_str) == Some(hash.as_str())
        && expected_hash.map(|e| e == hash).unwrap_or(true))
    {
        return Err("REFERENCE_BANK_MISMATCH".into());
    }
    if stats.get("status").and_then(V::as_str) != Some("running") {
        return Err("REFERENCE_WORKER_NOT_RUNNING".into());
    }
    if !(stats.get("updatedAt").and_then(V::as_str).is_some_and(|s| instant(s))
        && parse(stats.get("updatedAt").unwrap()) <= now_ms
        && now_ms - parse(stats.get("updatedAt").unwrap()) <= 60000)
    {
        return Err("REFERENCE_WORKER_STALE".into());
    }
    if !(js_int(bank.get("sourceRecords").unwrap_or(&V::Null)).is_some_and(|n| n >= 1)
        && js_int(shadow.get("sourceSequence").unwrap_or(&V::Null))
            .is_some_and(|s| s >= js_int(bank.get("sourceRecords").unwrap()).unwrap())
        && shadow.get("lastRecordHash").and_then(V::as_str).is_some_and(|h| {
            h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        })
        && stats.get("sourceSequence") == shadow.get("sourceSequence")
        && stats.get("sourceHash") == shadow.get("lastRecordHash"))
    {
        return Err("REFERENCE_SOURCE_MISMATCH".into());
    }
    Ok(json!({
        "ok": true,
        "bank": bank,
        "shadow": shadow.clone(),
        "proof": {
            "bankHash": hash,
            "sourceSequence": shadow.get("sourceSequence").unwrap().clone(),
            "sourceHash": shadow.get("lastRecordHash").unwrap().clone(),
        },
    }))
}
pub fn entry_reference_gate(
    inner: &Rc<RefCell<ControllerInner>>,
    activation: &V,
    runtime_dir: &Path,
    i: &V,
    permit: &V,
    now_ms: i64,
) -> bool {
    let gate = inner.borrow().entry_reference_gate.clone();
    if let Some(g) = gate {
        return g(inner, activation, runtime_dir, i, permit, now_ms);
    }
    base_reference_gate(inner, activation, runtime_dir, i, permit, now_ms)
}
pub fn entry_window_gate(inner: &Rc<RefCell<ControllerInner>>, activation: &V, now_ms: i64) -> bool {
    let start = parse(activation.get("startedAt").unwrap());
    let end = parse(activation.get("entryExpiresAt").unwrap());
    if start == i64::MIN || end == i64::MIN {
        inner.borrow_mut().reason = Some("INVALID_ACTIVATION_CLOCK".into());
        return false;
    }
    if now_ms < start {
        inner.borrow_mut().reason = Some("ENTRY_WINDOW_NOT_STARTED".into());
        return false;
    }
    if now_ms >= end {
        inner.borrow_mut().reason = Some("ENTRY_WINDOW_CLOSED".into());
        return false;
    }
    true
}
fn base_reference_gate(
    inner: &Rc<RefCell<ControllerInner>>,
    activation: &V,
    runtime_dir: &Path,
    i: &V,
    permit: &V,
    now_ms: i64,
) -> bool {
    // An identical expired replay can only retrieve an earlier acceptance; it
    // cannot create a live entry and remains available for reconciliation.
    if i.get("action").and_then(V::as_str) != Some("buy") {
        return true;
    }
    let current = inner.borrow().current_permit.clone().unwrap_or(V::Null);
    if permit.get("retry") == Some(&V::Bool(true))
        && current.get("retry") == Some(&V::Bool(true))
        && i.get("attempts").and_then(V::as_i64).unwrap_or(0) > 0
        && now_ms >= parse(i.get("expirationDate").unwrap())
    {
        return true;
    }
    // A queued purchase must remain inside the authorization window at both
    // dispatch fences. An unexpired retry could still be its first acceptance.
    if !entry_window_gate(inner, activation, now_ms) {
        return false;
    }
    let ref_result = responsive_reference_at(runtime_dir, now_ms, i.pointer("/attribution/modelHash").and_then(V::as_str));
    let proof = current.get("referenceProof").cloned().unwrap_or(V::Null);
    let Ok(ref_ok) = ref_result else {
        inner.borrow_mut().reason = ref_result.err();
        return false;
    };
    let ref_proof = ref_ok.get("proof").unwrap();
    let ok = !proof.is_null()
        && proof.get("bankHash") == ref_proof.get("bankHash")
        && js_int(ref_proof.get("sourceSequence").unwrap_or(&V::Null))
            .zip(js_int(proof.get("sourceSequence").unwrap_or(&V::Null)))
            .is_some_and(|(r, p)| r > p || (r == p && ref_proof.get("sourceHash") == proof.get("sourceHash")))
        && proof.get("referenceHash").and_then(V::as_str)
            == Some(reference_input_hash(ref_ok.get("shadow").unwrap(), i.get("exchangeId").and_then(V::as_str).unwrap_or("")).as_str());
    if !ok {
        inner.borrow_mut().reason = Some("REFERENCE_SOURCE_CHANGED".into());
    }
    ok
}
fn controller_network_admit(
    inner: &Rc<RefCell<ControllerInner>>,
    ledger: &Rc<RefCell<IntentLedger>>,
    activation: &V,
    _lease: &V,
    now: &Rc<dyn Fn() -> f64>,
    _runtime_dir: &Path,
    permit: &V,
    spec: &V,
) -> bool {
    let current = inner.borrow().current_permit.clone().unwrap_or(V::Null);
    if permit.get("intentId") != current.get("intentId") || permit.get("runId") != activation.get("runId") {
        return false;
    }
    let intent_id = permit.get("intentId").and_then(V::as_str).unwrap_or("");
    let i = inner
        .borrow()
        .state
        .pointer(&format!("/intents/{intent_id}"))
        .cloned()
        .unwrap_or(V::Null);
    let now_ms = now() as i64;
    if i.is_null() || i.get("status").and_then(V::as_str) == Some("closed") {
        return false;
    }
    if spec.get("method").and_then(V::as_str) == Some("DELETE") {
        let e = current.get("evidence").cloned().unwrap_or(V::Null);
        let phase = inner
            .borrow()
            .state
            .pointer(&format!("/control/cancellations/{intent_id}/phase"))
            .and_then(V::as_str)
            .map(str::to_string);
        let durable = ledger.borrow().snapshot().ok().and_then(|s| {
            s.pointer(&format!("/intents/{intent_id}")).cloned()
        });
        let order = e.get("order").cloned().unwrap_or(V::Null);
        let max_age = inner
            .borrow()
            .state
            .pointer("/policy/maxAccountAgeMs")
            .and_then(V::as_i64)
            .unwrap_or(180000);
        let ok = permit.get("kind").and_then(V::as_str) == Some("cancel")
            && current.get("kind").and_then(V::as_str) == Some("cancel")
            && permit.get("orderId") == i.get("orderId")
            && durable.as_ref().map(|d| crate::js_key(d.get("orderId").unwrap_or(&V::Null)))
                == Some(crate::js_key(i.get("orderId").unwrap()))
            && spec.get("endpoint").and_then(V::as_str)
                == Some(format!("/orders/{}", crate::js_key(i.get("orderId").unwrap())).as_str())
            && phase.as_deref() == Some("dispatching")
            && order.get("id") == i.get("orderId")
            && now_ms >= parse(i.get("expirationDate").unwrap())
            && parse(e.get("at").unwrap()) != i64::MIN
            && parse(e.get("at").unwrap()) <= now_ms
            && now_ms - parse(e.get("at").unwrap()) <= max_age;
        if !ok {
            return false;
        }
        return ["cash", "positions", "orders"].iter().all(|k| {
            let p = e.pointer(&format!("/snapshot/{k}")).cloned().unwrap_or(V::Null);
            p.get("tournamentId").and_then(V::as_str) == Some(TID)
                && parse(p.get("at").unwrap()) != i64::MIN
                && parse(p.get("at").unwrap()) <= now_ms
                && now_ms - parse(p.get("at").unwrap()) <= max_age
                && (*k == "cash" || (p.get("complete") == Some(&V::Bool(true)) && p.get("rows").is_some_and(V::is_array)))
        });
    }
    if spec.get("method").and_then(V::as_str) != Some("POST")
        || i.get("status").and_then(V::as_str) != Some("dispatching")
        || i.get("payloadHash").and_then(V::as_str) != spec.get("body").map(|b| payload_hash(b).unwrap_or_default()).as_deref()
        || i.get("bodyJson") != spec.get("bodyJson")
    {
        return false;
    }
    if !entry_reference_gate(inner, activation, _runtime_dir, &i, permit, now_ms) {
        return false;
    }
    if permit.get("retry") == Some(&V::Bool(true))
        && current.get("retry") == Some(&V::Bool(true))
        && now_ms >= parse(i.get("expirationDate").unwrap())
    {
        return true;
    }
    let r = revalidate_intent_evidence(
        &inner.borrow().state,
        intent_id,
        current.get("snapshot").unwrap_or(&V::Null),
        &json!(now_ms),
    );
    if r.get("ok") != Some(&V::Bool(true)) {
        inner.borrow_mut().reason = r.get("code").and_then(V::as_str).map(str::to_string);
        return false;
    }
    true
}
