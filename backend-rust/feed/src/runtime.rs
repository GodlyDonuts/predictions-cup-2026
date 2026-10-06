// feed/runtime.mjs — OfflineRuntime recovery/connect loop, ported synchronous:
// the JS version races each adapter call against timeout/abort promises; here
// adapters are synchronous, so the equivalent checks run before and after each
// call and injected `aborted`/`should_stop` are the only preemption source.
use crate::collector::MarketCollector;
use crate::ms_to_iso;
use baal_service_runtime::budget::AccountReadBudget;
use baal_service_runtime::contract::snapshot_path;
use baal_service_runtime::wait::{backoff, check_stop, interruptible_wait, retry_after_ms, WaitError};
use serde_json::Value as V;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct AdapterError {
    pub status: Option<i64>,
    pub retry_after: Option<String>,
    pub aborted: bool, // JS error?.name === 'AbortError'
    pub message: String,
}
impl AdapterError {
    pub fn aborted() -> AdapterError {
        AdapterError { status: None, retry_after: None, aborted: true, message: "STOPPED".into() }
    }
    pub fn http(status: i64, retry_after: Option<String>) -> AdapterError {
        AdapterError { status: Some(status), retry_after, aborted: false, message: "RECOVERY_UNAVAILABLE".into() }
    }
    pub fn live_disabled() -> AdapterError {
        AdapterError { status: None, retry_after: None, aborted: false, message: "LIVE_TRANSPORT_DISABLED".into() }
    }
}
impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for AdapterError {}
impl From<WaitError> for AdapterError {
    fn from(e: WaitError) -> AdapterError {
        match e {
            WaitError::Stopped => AdapterError::aborted(),
            WaitError::InvalidDelay => {
                AdapterError { status: None, retry_after: None, aborted: false, message: "INVALID_DELAY".into() }
            }
        }
    }
}
impl From<String> for AdapterError {
    fn from(s: String) -> AdapterError {
        AdapterError {
            status: None,
            retry_after: None,
            aborted: s == "STOPPED",
            message: s,
        }
    }
}

pub trait FeedAdapter {
    fn mode(&self) -> &'static str;
    fn subscribe(
        &mut self,
        topic: &str,
        on_event: &mut dyn FnMut(&str, &V, &str),
        aborted: &dyn Fn() -> bool,
    ) -> Result<(), AdapterError>;
    fn read_snapshot(&mut self, path: &str, aborted: &dyn Fn() -> bool) -> Result<V, AdapterError>;
    fn close(&mut self, aborted: &dyn Fn() -> bool) -> Result<(), AdapterError>;
}

pub struct DisabledTransport;
impl FeedAdapter for DisabledTransport {
    fn mode(&self) -> &'static str {
        "disabled"
    }
    fn subscribe(&mut self, _t: &str, _cb: &mut dyn FnMut(&str, &V, &str), _a: &dyn Fn() -> bool) -> Result<(), AdapterError> {
        Err(AdapterError::live_disabled())
    }
    fn read_snapshot(&mut self, _p: &str, _a: &dyn Fn() -> bool) -> Result<V, AdapterError> {
        Err(AdapterError::live_disabled())
    }
    fn close(&mut self, _a: &dyn Fn() -> bool) -> Result<(), AdapterError> {
        Ok(())
    }
}

pub struct OfflineRuntime {
    pub collector: Rc<RefCell<MarketCollector>>,
    pub adapter: RefCell<Box<dyn FeedAdapter>>,
    pub budget: Option<AccountReadBudget>,
    pub enabled: bool,
    pub max_attempts: i64,
    pub max_wait_ms: f64,
    pub request_timeout_ms: f64,
    pub random: Rc<dyn Fn() -> f64>,
    pub sleep: Rc<dyn Fn(f64)>,
    pub should_stop: Rc<dyn Fn() -> bool>,
    controller_aborted: Rc<Cell<bool>>,
}

fn require_that(ok: bool, code: &str) -> Result<(), AdapterError> {
    if ok { Ok(()) } else { Err(code.to_string().into()) }
}

impl OfflineRuntime {
    pub fn new(
        collector: Rc<RefCell<MarketCollector>>,
        adapter: Box<dyn FeedAdapter>,
        budget: Option<AccountReadBudget>,
        enabled: bool,
        max_attempts: i64,
        max_wait_ms: f64,
        request_timeout_ms: f64,
        random: Rc<dyn Fn() -> f64>,
        sleep: Rc<dyn Fn(f64)>,
        should_stop: Rc<dyn Fn() -> bool>,
    ) -> Result<OfflineRuntime, String> {
        if !(max_attempts >= 1
            && max_attempts <= 20
            && max_wait_ms.is_finite()
            && max_wait_ms >= 0.0
            && max_wait_ms <= 86_400_000.0
            && request_timeout_ms.is_finite()
            && request_timeout_ms > 0.0)
        {
            return Err("INVALID_SCHEMA".into());
        }
        Ok(OfflineRuntime {
            collector,
            adapter: RefCell::new(adapter),
            budget,
            enabled,
            max_attempts,
            max_wait_ms,
            request_timeout_ms,
            random,
            sleep,
            should_stop,
            controller_aborted: Rc::new(Cell::new(false)),
        })
    }

    fn combined<'c>(&'c self, aborted: &'c dyn Fn() -> bool) -> impl Fn() -> bool + 'c {
        let ctl = self.controller_aborted.clone();
        move || aborted() || ctl.get()
    }

    fn guard(&self, aborted: &dyn Fn() -> bool) -> Result<(), AdapterError> {
        check_stop(aborted(), &*self.should_stop)?;
        require_that(self.enabled && self.adapter.borrow().mode() == "offline", "LIVE_TRANSPORT_DISABLED")
    }

    // The JS timeout/abort race only prevents an uncooperative adapter from
    // hanging the caller; with synchronous adapters the equivalent guarantee is
    // the stop check before and after the call.
    fn call<T>(&self, f: impl FnOnce(&dyn Fn() -> bool) -> Result<T, AdapterError>, aborted: &dyn Fn() -> bool) -> Result<T, AdapterError> {
        let combined = self.combined(aborted);
        check_stop(combined(), &*self.should_stop)?;
        let out = f(&combined)?;
        check_stop(combined(), &*self.should_stop)?;
        Ok(out)
    }

    pub fn recover(&self, aborted: &dyn Fn() -> bool) -> Result<bool, AdapterError> {
        let combined = self.combined(aborted);
        match self.recover_internal(&combined) {
            Err(e) => {
                if combined() || (self.should_stop)() || e.aborted {
                    self.collector.borrow_mut().stop();
                    self.close_bounded();
                }
                Err(e)
            }
            ok => ok,
        }
    }

    fn recover_internal(&self, aborted: &dyn Fn() -> bool) -> Result<bool, AdapterError> {
        self.guard(aborted)?;
        require_that(self.budget.is_some(), "SHARED_BUDGET_REQUIRED")?;
        let budget = self.budget.as_ref().unwrap();
        for attempt in 0..self.max_attempts {
            self.guard(aborted)?;
            if self.collector.borrow().state == "disconnected" {
                return Ok(false);
            }
            let ticket = self.collector.borrow_mut().begin_recovery().map_err(AdapterError::from)?;
            let path = snapshot_path(&self.collector.borrow().scope);
            let attempt_result = (|| -> Result<bool, AdapterError> {
                self.call(|s| budget.admit(&path, s, &*self.should_stop).map_err(AdapterError::from), aborted)?;
                self.guard(aborted)?;
                let raw = self.call(
                    |s| self.adapter.borrow_mut().read_snapshot(&path, s),
                    aborted,
                )?;
                self.guard(aborted)?;
                let received = ms_to_iso((self.collector.borrow().now)());
                Ok(self.collector.borrow_mut().complete_recovery(&ticket, &raw, &received))
            })();
            match attempt_result {
                Err(e) => {
                    check_stop(aborted(), &*self.should_stop)?;
                    if self.collector.borrow().state == "disconnected" {
                        return Ok(false);
                    }
                    if e.aborted {
                        self.collector.borrow_mut().invalidate("RECOVERY_FAILED")?;
                        return Ok(false);
                    }
                    let status = e.status.unwrap_or(503);
                    self.collector.borrow_mut().invalidate("RECOVERY_FAILED")?;
                    if (400..500).contains(&status) && status != 429 {
                        return Ok(false);
                    }
                    let now = (self.collector.borrow().now)();
                    let delay = (if status == 429 { 60000.0f64 } else { 0.0 })
                        .max(retry_after_ms(e.retry_after.as_deref(), now))
                        .max(backoff(attempt, 250.0, 30000.0, (self.random)())?);
                    self.call(|s| budget.cooldown(delay, s).map_err(AdapterError::from), aborted)?;
                    if delay > self.max_wait_ms {
                        return Ok(false);
                    }
                    if attempt + 1 < self.max_attempts {
                        interruptible_wait(delay, aborted, &*self.should_stop, &*self.sleep)?;
                    }
                    continue;
                }
                Ok(true) => return Ok(true),
                Ok(false) => {}
            }
            if attempt + 1 < self.max_attempts {
                let delay = backoff(attempt, 250.0, 30000.0, (self.random)())?;
                interruptible_wait(delay, aborted, &*self.should_stop, &*self.sleep)?;
            }
        }
        self.collector.borrow_mut().invalidate("RECOVERY_FAILED")?;
        Ok(false)
    }

    pub fn connect(&self, aborted: &dyn Fn() -> bool, reason: &str) -> Result<bool, AdapterError> {
        let combined = self.combined(aborted);
        let run = |aborted: &dyn Fn() -> bool| -> Result<bool, AdapterError> {
            self.guard(aborted)?;
            for attempt in 0..self.max_attempts {
                self.guard(aborted)?;
                let step = (|| -> Result<bool, AdapterError> {
                    let next_epoch = self.collector.borrow().epoch + 1;
                    let pending: Rc<RefCell<Vec<(String, V, String)>>> = Rc::new(RefCell::new(Vec::new()));
                    let subscribed = Rc::new(Cell::new(false));
                    {
                        let collector = self.collector.clone();
                        let pending = pending.clone();
                        let subscribed = subscribed.clone();
                        let mut on_event = move |event: &str, payload: &V, received_at: &str| {
                            if subscribed.get() {
                                collector.borrow_mut().ingest(event, payload, received_at, next_epoch);
                            } else {
                                let mut p = pending.borrow_mut();
                                assert!(p.len() < 1000, "SUBSCRIPTION_BUFFER_FULL");
                                p.push((event.to_string(), payload.clone(), received_at.to_string()));
                            }
                        };
                        let topic = self.collector.borrow().scope.topic.clone();
                        self.call(
                            |s| self.adapter.borrow_mut().subscribe(&topic, &mut on_event, s),
                            aborted,
                        )?;
                    }
                    self.guard(aborted)?;
                    self.collector.borrow_mut().subscribed(reason)?;
                    subscribed.set(true);
                    for (event, payload, received_at) in pending.borrow_mut().drain(..) {
                        self.collector.borrow_mut().ingest(&event, &payload, &received_at, next_epoch);
                    }
                    let recovered = self.recover(aborted)?;
                    if !recovered {
                        self.collector.borrow_mut().disconnected("RECOVERY_FAILED")?;
                        self.close_bounded();
                    }
                    Ok(recovered)
                })();
                match step {
                    Ok(r) => return Ok(r),
                    Err(e) => {
                        check_stop(aborted(), &*self.should_stop)?;
                        self.collector.borrow_mut().disconnected("SOCKET_ERROR")?;
                        self.close_bounded();
                        if attempt + 1 == self.max_attempts || e.aborted {
                            return Ok(false);
                        }
                        let delay = backoff(attempt, 250.0, 30000.0, (self.random)())?;
                        interruptible_wait(delay, aborted, &*self.should_stop, &*self.sleep)?;
                    }
                }
            }
            Ok(false)
        };
        match run(&combined) {
            Err(e) => {
                if combined() || (self.should_stop)() || e.aborted {
                    self.collector.borrow_mut().stop();
                    self.close_bounded();
                }
                Err(e)
            }
            ok => ok,
        }
    }

    pub fn close_bounded(&self) {
        let dead = || false;
        let _ = self.adapter.borrow_mut().close(&dead);
    }

    pub fn stop(&self) {
        self.controller_aborted.set(true);
        self.collector.borrow_mut().stop();
        self.close_bounded();
    }
}
