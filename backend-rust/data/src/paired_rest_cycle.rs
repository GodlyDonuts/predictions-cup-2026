// execution/paired-rest-cycle.mjs — sequential orchestration inside the
// existing collector, not another worker. Reads finish before admission; the
// controller still owns both write fences.
use baal_service_runtime::fixed::instant;
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::rc::Rc;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.to_string()) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }
fn instant_v(v: &V) -> Result<i64, String> { instant(v.as_str().unwrap_or("")) }

pub trait PairedCycleHost {
    fn status(&self) -> V;
    fn prepare_next(&self) -> Result<V, String>;
    fn dispatch_prepared(&self) -> Result<V, String>;
    fn recover_exact(&self) -> Result<V, String>;
    fn reconcile(&self, evidence: &V) -> Result<V, String>;
    fn cancel_expired(&self) -> Result<V, String>;
}
pub trait PairedCycleBridge {
    fn refresh_preparation(&self, leg: &V) -> Result<V, String>;
    fn refresh_reconciliation(&self, order_id: &V, leg: &V) -> Result<V, String>;
    fn refresh_cancel(&self, order_id: &V, leg: &V) -> Result<V, String>;
}

pub struct PairedRestCycle {
    controller: Rc<dyn PairedCycleHost>,
    bridge: Rc<dyn PairedCycleBridge>,
    entry_allowed: Rc<dyn Fn() -> bool>,
    can_operate: Rc<dyn Fn() -> bool>,
    recovery_allowed: Rc<dyn Fn() -> bool>,
    now: Rc<dyn Fn() -> i64>,
    recovery_minimum_ms: i64,
    on_status: Rc<dyn Fn(&V)>,
    busy: RefCell<bool>,
    closed: RefCell<bool>,
    last_action: RefCell<V>,
}

impl PairedRestCycle {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        controller: Rc<dyn PairedCycleHost>,
        bridge: Rc<dyn PairedCycleBridge>,
        entry_allowed: Rc<dyn Fn() -> bool>,
        can_operate: Rc<dyn Fn() -> bool>,
        recovery_allowed: Rc<dyn Fn() -> bool>,
        now: Rc<dyn Fn() -> i64>,
        recovery_minimum_ms: i64,
        on_status: Rc<dyn Fn(&V)>,
    ) -> Result<Self, String> {
        need(recovery_minimum_ms >= 90_000, "PAIRED_CYCLE_RECOVERY_INTERVAL")?;
        Ok(Self {
            controller, bridge, entry_allowed, can_operate, recovery_allowed, now,
            recovery_minimum_ms, on_status,
            busy: RefCell::new(false),
            closed: RefCell::new(false),
            last_action: RefCell::new(V::Null),
        })
    }
    fn report(&self, action: &str) -> Result<V, String> {
        *self.last_action.borrow_mut() = json!(action);
        let state = self.controller.status();
        (self.on_status)(&json!({
            "action": action,
            "state": state,
            "observedAt": baal_service_runtime::contract::ms_to_iso((self.now)() as f64),
        }));
        Ok(json!({"action": action, "state": state}))
    }
    pub fn step(&self) -> Result<V, String> {
        need(!*self.busy.borrow() && !*self.closed.borrow(), "PAIRED_CYCLE_BUSY_OR_CLOSED")?;
        *self.busy.borrow_mut() = true;
        let result = self.step_inner();
        *self.busy.borrow_mut() = false;
        result
    }
    fn step_inner(&self) -> Result<V, String> {
        if !(self.can_operate)() { return self.report("operational_stop"); }
        let state = self.controller.status();
        if js_truthy(state.get("failed")) || js_truthy(state.get("closed")) {
            return self.report("controller_failed_or_closed");
        }
        let current = get(&state, "orders").as_array()
            .and_then(|o| o.iter().find(|x| get(x, "phase") != &json!("reconciled"))).cloned();
        let current = match current {
            None => {
                if ["paired", "orphaned", "unfilled"].contains(&get(&state, "phase").as_str().unwrap_or("")) {
                    return self.report("known_terminal_pair_state");
                }
                if !(self.entry_allowed)() || (self.now)() >= instant_v(get(&state, "reviewExpiresAt"))? {
                    return self.report(if get(&state, "owned").as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                        "hedge_window_closed_inventory_retained"
                    } else { "entry_window_closed" });
                }
                let legs = get(&state, "planLegs").as_array().cloned().unwrap_or_default();
                let idx = get(&state, "orders").as_array().map(|a| a.len()).unwrap_or(0);
                let leg = legs.get(idx).cloned().ok_or("PAIRED_CYCLE_LEG")?;
                self.bridge.refresh_preparation(&leg)?;
                // These calls occur in one cycle; no future timer can submit
                // this intent.
                if !(self.can_operate)() || !(self.entry_allowed)() || (self.now)() >= instant_v(get(&state, "reviewExpiresAt"))? {
                    return self.report("entry_stopped_after_reads");
                }
                self.controller.prepare_next()?;
                if !(self.entry_allowed)() { return self.report("prepared_entry_window_closed"); }
                self.controller.dispatch_prepared()?;
                return self.report("placement_attempt_completed");
            }
            Some(c) => c,
        };
        let leg = get(&state, "planLegs").as_array().cloned().unwrap_or_default().into_iter()
            .find(|l| get(l, "marketId") == get(&current, "marketId") && get(l, "exchangeId") == get(&current, "exchangeId"))
            .ok_or("PAIRED_CYCLE_ORDER_LEG")?;
        if js_truthy(current.get("semanticMismatch")) {
            return self.report("semantic_uncertainty_blocks_replacement");
        }
        if get(&current, "phase") == &json!("prepared") {
            if !(self.entry_allowed)()
                || (self.now)() >= instant_v(get(get(&current, "body"), "expirationDate"))?
                || (self.now)() >= instant_v(get(&state, "reviewExpiresAt"))?
            {
                return self.report("prepared_expired_reserve_retained");
            }
            self.bridge.refresh_preparation(&leg)?;
            if !(self.can_operate)()
                || !(self.entry_allowed)()
                || (self.now)() >= instant_v(get(get(&current, "body"), "expirationDate"))?
                || (self.now)() >= instant_v(get(&state, "reviewExpiresAt"))?
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
            if (self.now)() < instant_v(get(get(&current, "body"), "expirationDate"))?
                || (self.now)() - instant_v(started)? < self.recovery_minimum_ms
            {
                return self.report("exact_recovery_backoff");
            }
            if !(self.recovery_allowed)() {
                return self.report("shared_api_recovery_cooldown");
            }
            self.controller.recover_exact()?;
            return self.report("identical_expired_key_recovery_attempted");
        }
        match self.bridge.refresh_reconciliation(get(&current, "orderId"), &leg)
            .and_then(|evidence| self.controller.reconcile(&evidence))
        {
            Ok(_) => self.report("official_scoped_reconciliation"),
            Err(e) => {
                // An open order is not a missing fill or a terminal outcome.
                // Expiry alone neither releases funding nor permits another
                // order or another key.
                if e != "PAIRED_EVIDENCE_ORDER_SCOPE_OR_PHASE" { return Err(e); }
                if (self.now)() < instant_v(get(get(&current, "body"), "expirationDate"))? {
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
        json!({
            "busy": *self.busy.borrow(),
            "closed": *self.closed.borrow(),
            "lastAction": self.last_action.borrow().clone(),
            "additionalPersistentProcesses": 0,
        })
    }
    pub fn close(&self) -> Result<(), String> {
        need(!*self.busy.borrow(), "PAIRED_CYCLE_BUSY")?;
        *self.closed.borrow_mut() = true;
        Ok(())
    }
}
