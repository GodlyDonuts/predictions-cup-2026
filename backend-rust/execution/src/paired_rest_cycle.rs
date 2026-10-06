// execution/paired-rest-cycle.mjs — sequential orchestration inside the
// existing collector, not another worker. Reads finish before admission; the
// controller still owns both write fences.
use crate::paired_rest_evidence_bridge::PairedRestEvidenceBridge;
use baal_service_runtime::fixed::instant;
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::rc::Rc;

// need here throws the full code, not a prefixed fragment.
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_string()) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn iso(n: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(n)
}

// The controller surface the cycle drives — implemented by
// paired_rest_controller and paired_rest_controller_v3.
pub trait PairedCycleDriver {
    fn status(&self) -> V;
    fn prepare_next(&self) -> Result<V, String>;
    fn dispatch_prepared(&self) -> Result<V, String>;
    fn reconcile(&self, evidence: &V) -> Result<V, String>;
    fn recover_exact(&self) -> Result<V, String>;
    fn cancel_expired(&self) -> Result<V, String>;
}

struct Inner {
    busy: bool,
    closed: bool,
    last_action: Option<String>,
}

pub struct PairedRestCycle {
    controller: Rc<dyn PairedCycleDriver>,
    bridge: Rc<PairedRestEvidenceBridge>,
    entry_allowed: Rc<dyn Fn() -> bool>,
    can_operate: Rc<dyn Fn() -> bool>,
    recovery_allowed: Rc<dyn Fn() -> bool>,
    now: Rc<dyn Fn() -> f64>,
    recovery_minimum_ms: i64,
    on_status: Rc<dyn Fn(&V)>,
    inner: RefCell<Inner>,
}

pub struct PairedRestCycleDeps {
    pub entry_allowed: Rc<dyn Fn() -> bool>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub recovery_allowed: Rc<dyn Fn() -> bool>,
    pub now: Rc<dyn Fn() -> f64>,
    pub on_status: Rc<dyn Fn(&V)>,
}

// createPairedRestCycle({controller,bridge,entryAllowed,canOperate,recoveryAllowed,now,recoveryMinimumMs,onStatus})
pub fn create_paired_rest_cycle(
    controller: Rc<dyn PairedCycleDriver>,
    bridge: Rc<PairedRestEvidenceBridge>,
    deps: PairedRestCycleDeps,
    recovery_minimum_ms: Option<i64>,
) -> Result<PairedRestCycle, String> {
    let minimum = recovery_minimum_ms.unwrap_or(90_000);
    need(minimum >= 90_000, "PAIRED_CYCLE_RECOVERY_INTERVAL")?;
    Ok(PairedRestCycle {
        controller,
        bridge,
        entry_allowed: deps.entry_allowed,
        can_operate: deps.can_operate,
        recovery_allowed: deps.recovery_allowed,
        now: deps.now,
        recovery_minimum_ms: minimum,
        on_status: deps.on_status,
        inner: RefCell::new(Inner { busy: false, closed: false, last_action: None }),
    })
}

impl PairedRestCycle {
    fn report(&self, action: &str) -> Result<V, String> {
        self.inner.borrow_mut().last_action = Some(action.to_string());
        let state = self.controller.status();
        (self.on_status)(&json!({
            "action": action,
            "state": state,
            "observedAt": iso((self.now)()),
        }));
        Ok(json!({"action": action, "state": state}))
    }

    // step() — async in JS; synchronous here (reads never overlap a write).
    pub fn step(&self) -> Result<V, String> {
        {
            let mut g = self.inner.borrow_mut();
            need(!g.busy && !g.closed, "PAIRED_CYCLE_BUSY_OR_CLOSED")?;
            g.busy = true;
        }
        let result = self.step_inner();
        self.inner.borrow_mut().busy = false;
        result
    }

    fn step_inner(&self) -> Result<V, String> {
        if !(self.can_operate)() {
            return self.report("operational_stop");
        }
        let state = self.controller.status();
        if js_truthy(state.get("failed")) || js_truthy(state.get("closed")) {
            return self.report("controller_failed_or_closed");
        }
        let current = get(&state, "orders").as_array().cloned().unwrap_or_default().into_iter().find(|o| {
            get(o, "phase").as_str() != Some("reconciled")
        });
        let now = (self.now)() as i64;
        if current.is_none() {
            if matches!(get(&state, "phase").as_str(), Some("paired" | "orphaned" | "unfilled")) {
                return self.report("known_terminal_pair_state");
            }
            if !(self.entry_allowed)() || now >= instant_v(get(&state, "reviewExpiresAt"))? {
                return self.report(if get(&state, "owned").as_array().is_some_and(|a| !a.is_empty()) {
                    "hedge_window_closed_inventory_retained"
                } else {
                    "entry_window_closed"
                });
            }
            let leg = get(&state, "planLegs")
                .as_array()
                .and_then(|a| a.get(get(&state, "orders").as_array().map(|x| x.len()).unwrap_or(0)))
                .cloned()
                .unwrap_or(V::Null);
            need(!leg.is_null(), "PAIRED_CYCLE_LEG")?;
            self.bridge.refresh_preparation(&leg)?;
            // These calls occur in one cycle; no future timer can submit this
            // intent.
            if !(self.can_operate)()
                || !(self.entry_allowed)()
                || (self.now)() as i64 >= instant_v(get(&state, "reviewExpiresAt"))?
            {
                return self.report("entry_stopped_after_reads");
            }
            self.controller.prepare_next()?;
            if !(self.entry_allowed)() {
                return self.report("prepared_entry_window_closed");
            }
            self.controller.dispatch_prepared()?;
            return self.report("placement_attempt_completed");
        }
        let current = current.unwrap();
        let leg = get(&state, "planLegs").as_array().cloned().unwrap_or_default().into_iter().find(|l| {
            get(l, "marketId") == get(&current, "marketId") && get(l, "exchangeId") == get(&current, "exchangeId")
        });
        let leg = leg.unwrap_or(V::Null);
        need(!leg.is_null(), "PAIRED_CYCLE_ORDER_LEG")?;
        if js_truthy(current.get("semanticMismatch")) {
            return self.report("semantic_uncertainty_blocks_replacement");
        }
        if get(&current, "phase").as_str() == Some("prepared") {
            if !(self.entry_allowed)()
                || now >= instant_v(get(&get(&current, "body"), "expirationDate"))?
                || now >= instant_v(get(&state, "reviewExpiresAt"))?
            {
                return self.report("prepared_expired_reserve_retained");
            }
            self.bridge.refresh_preparation(&leg)?;
            if !(self.can_operate)()
                || !(self.entry_allowed)()
                || (self.now)() as i64 >= instant_v(get(&get(&current, "body"), "expirationDate"))?
                || (self.now)() as i64 >= instant_v(get(&state, "reviewExpiresAt"))?
            {
                return self.report("prepared_entry_stopped");
            }
            self.controller.dispatch_prepared()?;
            return self.report("original_prepared_intent_dispatched");
        }
        if get(&current, "orderId").is_null() {
            let started = get(&current, "lastActualFetchStartAt");
            if started.is_null() {
                return self.report("missing_original_fetch_proof_blocks_recovery");
            }
            if now < instant_v(get(&get(&current, "body"), "expirationDate"))?
                || now - instant_v(started)? < self.recovery_minimum_ms
            {
                return self.report("exact_recovery_backoff");
            }
            if !(self.recovery_allowed)() {
                return self.report("shared_api_recovery_cooldown");
            }
            self.controller.recover_exact()?;
            return self.report("identical_expired_key_recovery_attempted");
        }
        match self.bridge.refresh_reconciliation(get(&current, "orderId"), &leg) {
            Ok(evidence) => {
                self.controller.reconcile(&evidence)?;
                self.report("official_scoped_reconciliation")
            }
            Err(error) => {
                // An open order is not a missing fill or a terminal outcome.
                // Expiry alone neither releases funding nor permits another
                // order or another key.
                if error != "PAIRED_EVIDENCE_ORDER_SCOPE_OR_PHASE" {
                    return Err(error);
                }
                if now < instant_v(get(&get(&current, "body"), "expirationDate"))? {
                    return self.report("known_order_pending");
                }
                if js_truthy(current.get("cancelPending")) {
                    return self.report("cancel_pending_reconciliation");
                }
                self.bridge.refresh_cancel(get(&current, "orderId"), &leg)?;
                self.controller.cancel_expired()?;
                self.report("owned_expired_order_cancel_attempted")
            }
        }
    }

    pub fn status(&self) -> V {
        let g = self.inner.borrow();
        json!({
            "busy": g.busy,
            "closed": g.closed,
            "lastAction": g.last_action,
            "additionalPersistentProcesses": 0,
        })
    }

    pub fn close(&self) -> Result<(), String> {
        let mut g = self.inner.borrow_mut();
        need(!g.busy, "PAIRED_CYCLE_BUSY")?;
        g.closed = true;
        Ok(())
    }
}
