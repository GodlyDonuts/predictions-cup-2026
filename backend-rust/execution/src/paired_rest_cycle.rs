// execution/paired-rest-cycle.mjs — sequential orchestration inside the
// existing collector, not another worker. Reads finish before admission; the
// controller still owns both write fences.
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::instant;
use baal_service_runtime::json_text::js_truthy;
use serde_json::{json, Value as V};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(code.to_string())
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}

/// The paired REST controller surface the cycle drives (JS object with
/// status/prepareNext/dispatchPrepared/recoverExact/cancelExpired/reconcile).
pub trait PairedRestCycleController {
    fn status(&self) -> Result<V, String>;
    fn prepare_next(&self) -> Result<V, String>;
    fn dispatch_prepared(&self) -> Result<V, String>;
    fn recover_exact(&self) -> Result<V, String>;
    fn cancel_expired(&self) -> Result<V, String>;
    fn reconcile(&self, evidence: &V) -> Result<V, String>;
}
/// The paired REST evidence bridge surface the cycle drives.
pub trait PairedRestCycleBridge {
    fn refresh_preparation(&self, leg: &V) -> Result<V, String>;
    fn refresh_reconciliation(&self, order_id: &V, leg: &V) -> Result<V, String>;
    fn refresh_cancel(&self, order_id: &V, leg: &V) -> Result<V, String>;
}

pub struct PairedRestCycle {
    controller: Rc<dyn PairedRestCycleController>,
    bridge: Rc<dyn PairedRestCycleBridge>,
    entry_allowed: Rc<dyn Fn() -> bool>,
    can_operate: Rc<dyn Fn() -> bool>,
    recovery_allowed: Rc<dyn Fn() -> bool>,
    now: Rc<dyn Fn() -> f64>,
    recovery_minimum_ms: f64,
    on_status: Rc<dyn Fn(&V)>,
    busy: Cell<bool>,
    closed: Cell<bool>,
    last_action: RefCell<V>,
}

/// createPairedRestCycle({controller, bridge, entryAllowed, canOperate,
/// recoveryAllowed, now, recoveryMinimumMs, onStatus})
pub fn create_paired_rest_cycle(
    controller: Rc<dyn PairedRestCycleController>,
    bridge: Rc<dyn PairedRestCycleBridge>,
    entry_allowed: Rc<dyn Fn() -> bool>,
    can_operate: Rc<dyn Fn() -> bool>,
    recovery_allowed: Rc<dyn Fn() -> bool>,
    now: Rc<dyn Fn() -> f64>,
    recovery_minimum_ms: f64,
    on_status: Rc<dyn Fn(&V)>,
) -> Result<PairedRestCycle, String> {
    need(
        recovery_minimum_ms.is_finite()
            && recovery_minimum_ms.fract() == 0.0
            && recovery_minimum_ms.abs() <= 9_007_199_254_740_991.0
            && recovery_minimum_ms >= 90_000.0,
        "PAIRED_CYCLE_RECOVERY_INTERVAL",
    )?;
    Ok(PairedRestCycle {
        controller,
        bridge,
        entry_allowed,
        can_operate,
        recovery_allowed,
        now,
        recovery_minimum_ms,
        on_status,
        busy: Cell::new(false),
        closed: Cell::new(false),
        last_action: RefCell::new(V::Null),
    })
}

impl PairedRestCycle {
    // report(action)
    fn report(&self, action: &str) -> Result<V, String> {
        *self.last_action.borrow_mut() = json!(action);
        let state = self.controller.status()?;
        (self.on_status)(&json!({
            "action": action,
            "state": state,
            "observedAt": ms_to_iso((self.now)()),
        }));
        Ok(json!({"action": action, "state": state}))
    }

    /// step()
    pub fn step(&self) -> Result<V, String> {
        need(
            !self.busy.get() && !self.closed.get(),
            "PAIRED_CYCLE_BUSY_OR_CLOSED",
        )?;
        self.busy.set(true);
        let result = self.step_inner();
        self.busy.set(false);
        result
    }
    fn step_inner(&self) -> Result<V, String> {
        if !(self.can_operate)() {
            return self.report("operational_stop");
        }
        let state = self.controller.status()?;
        if js_truthy(state.get("failed")) || js_truthy(state.get("closed")) {
            return self.report("controller_failed_or_closed");
        }
        let orders = get(&state, "orders").as_array().cloned().unwrap_or_default();
        let current = orders.iter().find(|o| get(o, "phase") != "reconciled").cloned();
        match current {
            None => {
                if ["paired", "orphaned", "unfilled"]
                    .contains(&get(&state, "phase").as_str().unwrap_or(""))
                {
                    return self.report("known_terminal_pair_state");
                }
                if !(self.entry_allowed)()
                    || (self.now)() >= instant_v(get(&state, "reviewExpiresAt"))? as f64
                {
                    return self.report(
                        if get(&state, "owned").as_array().is_some_and(|o| !o.is_empty()) {
                            "hedge_window_closed_inventory_retained"
                        } else {
                            "entry_window_closed"
                        },
                    );
                }
                let leg = get(&state, "planLegs")
                    .as_array()
                    .and_then(|legs| legs.get(orders.len()))
                    .cloned()
                    .unwrap_or(V::Null);
                need(!leg.is_null(), "PAIRED_CYCLE_LEG")?;
                self.bridge.refresh_preparation(&leg)?;
                // These calls occur in one cycle; no future timer can submit
                // this intent.
                if !(self.can_operate)()
                    || !(self.entry_allowed)()
                    || (self.now)() >= instant_v(get(&state, "reviewExpiresAt"))? as f64
                {
                    return self.report("entry_stopped_after_reads");
                }
                self.controller.prepare_next()?;
                if !(self.entry_allowed)() {
                    return self.report("prepared_entry_window_closed");
                }
                self.controller.dispatch_prepared()?;
                self.report("placement_attempt_completed")
            }
            Some(current) => {
                let legs = get(&state, "planLegs").as_array().cloned().unwrap_or_default();
                let leg = legs
                    .iter()
                    .find(|l| {
                        get(l, "marketId") == get(&current, "marketId")
                            && get(l, "exchangeId") == get(&current, "exchangeId")
                    })
                    .cloned()
                    .unwrap_or(V::Null);
                need(!leg.is_null(), "PAIRED_CYCLE_ORDER_LEG")?;
                if js_truthy(current.get("semanticMismatch")) {
                    return self.report("semantic_uncertainty_blocks_replacement");
                }
                if get(&current, "phase") == "prepared" {
                    if !(self.entry_allowed)()
                        || (self.now)()
                            >= instant_v(get(get(&current, "body"), "expirationDate"))? as f64
                        || (self.now)() >= instant_v(get(&state, "reviewExpiresAt"))? as f64
                    {
                        return self.report("prepared_expired_reserve_retained");
                    }
                    self.bridge.refresh_preparation(&leg)?;
                    if !(self.can_operate)()
                        || !(self.entry_allowed)()
                        || (self.now)()
                            >= instant_v(get(get(&current, "body"), "expirationDate"))? as f64
                        || (self.now)() >= instant_v(get(&state, "reviewExpiresAt"))? as f64
                    {
                        return self.report("prepared_entry_stopped");
                    }
                    self.controller.dispatch_prepared()?;
                    return self.report("original_prepared_intent_dispatched");
                }
                if !js_truthy(current.get("orderId")) {
                    let started = get(&current, "lastActualFetchStartAt");
                    if !js_truthy(Some(started)) {
                        return self.report("missing_original_fetch_proof_blocks_recovery");
                    }
                    if (self.now)()
                        < (instant_v(get(get(&current, "body"), "expirationDate"))? as f64)
                        || ((self.now)() - instant_v(started)? as f64) < self.recovery_minimum_ms
                    {
                        return self.report("exact_recovery_backoff");
                    }
                    if !(self.recovery_allowed)() {
                        return self.report("shared_api_recovery_cooldown");
                    }
                    self.controller.recover_exact()?;
                    return self.report("identical_expired_key_recovery_attempted");
                }
                let reconciled = self
                    .bridge
                    .refresh_reconciliation(get(&current, "orderId"), &leg)
                    .and_then(|evidence| self.controller.reconcile(&evidence));
                match reconciled {
                    Ok(_) => self.report("official_scoped_reconciliation"),
                    Err(error) => {
                        // An open order is not a missing fill or a terminal
                        // outcome. Expiry alone neither releases funding nor
                        // permits another order or another key.
                        if error != "PAIRED_EVIDENCE_ORDER_SCOPE_OR_PHASE" {
                            return Err(error);
                        }
                        if (self.now)()
                            < instant_v(get(get(&current, "body"), "expirationDate"))? as f64
                        {
                            return self.report("known_order_pending");
                        }
                        if js_truthy(current.get("cancelPending")) {
                            return self.report("cancel_pending_reconciliation");
                        }
                        self.bridge
                            .refresh_cancel(get(&current, "orderId"), &leg)?;
                        self.controller.cancel_expired()?;
                        self.report("owned_expired_order_cancel_attempted")
                    }
                }
            }
        }
    }

    /// status()
    pub fn status(&self) -> V {
        json!({
            "busy": self.busy.get(),
            "closed": self.closed.get(),
            "lastAction": self.last_action.borrow().clone(),
            "additionalPersistentProcesses": 0,
        })
    }
    /// close()
    pub fn close(&self) -> Result<(), String> {
        need(!self.busy.get(), "PAIRED_CYCLE_BUSY")?;
        self.closed.set(true);
        Ok(())
    }
}
