//! Cooperative scheduling only: no account authority, clocks, or financial state.
use std::{collections::{BTreeSet, VecDeque}, sync::mpsc::{Receiver, TryRecvError}, time::Instant};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy, Debug)]
pub struct DrainBudget { pub frames: usize, pub events: usize, pub elapsed_ms: u64 }
impl DrainBudget {
    pub fn validate(self) -> Result<()> {
        if self.frames == 0 || self.frames > 1024 || self.events == 0 || self.events > 4096 || self.elapsed_ms == 0 || self.elapsed_ms > 1000 {
            return Err("SCHEDULING_DRAIN_BUDGET".into());
        }
        Ok(())
    }
}
impl Default for DrainBudget {
    fn default() -> Self { Self { frames: 32, events: 64, elapsed_ms: 50 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YieldReason { Empty, Frames, Events, Time }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrainProgress {
    pub reason: YieldReason, pub frames: usize, pub events: usize, pub pending_events: usize,
}

/// Retain a parsed frame's ordered events across cooperative yields. Never parse
/// another frame before this queue empties. Never discard this object on a yield.
pub struct OrderedDrain<E> { pending: VecDeque<E> }
impl<E> Default for OrderedDrain<E> {
    fn default() -> Self { Self { pending: VecDeque::new() } }
}
impl<E> OrderedDrain<E> {
    pub fn pending_events(&self) -> usize { self.pending.len() }
    /// `guard` must check current stream health and STOP before any work and on
    /// return. `apply` must finish durable admission before returning success.
    /// A parser or apply error is fatal to this pass, never automatic retry.
    pub fn drain<F>(
        &mut self, receiver: &Receiver<F>, budget: DrainBudget,
        guard: impl FnMut() -> Result<()>, parse: impl FnMut(F) -> Result<Vec<E>>,
        apply: impl FnMut(&E) -> Result<()>,
    ) -> Result<DrainProgress> {
        let start = Instant::now();
        self.drain_with_clock(receiver, budget, guard, parse, apply, || start.elapsed().as_millis().min(u64::MAX as u128) as u64)
    }
    fn drain_with_clock<F>(
        &mut self, receiver: &Receiver<F>, budget: DrainBudget,
        mut guard: impl FnMut() -> Result<()>, mut parse: impl FnMut(F) -> Result<Vec<E>>,
        mut apply: impl FnMut(&E) -> Result<()>, mut elapsed: impl FnMut() -> u64,
    ) -> Result<DrainProgress> {
        budget.validate()?;
        let (mut frames, mut events, mut previous_elapsed) = (0, 0, 0);
        loop {
            guard()?;
            let current = elapsed();
            if current < previous_elapsed { return Err("SCHEDULING_MONOTONIC_CLOCK".into()); }
            previous_elapsed = current;
            // Check before try_recv: even one speculative receive would lose a frame.
            let reason = if current >= budget.elapsed_ms { Some(YieldReason::Time) }
                else if events >= budget.events { Some(YieldReason::Events) }
                else if self.pending.is_empty() && frames >= budget.frames { Some(YieldReason::Frames) }
                else { None };
            if let Some(reason) = reason {
                return Ok(DrainProgress { reason, frames, events, pending_events: self.pending.len() });
            }
            if let Some(event) = self.pending.front() {
                apply(event)?;
                self.pending.pop_front(); // Never remove an event before durable apply succeeds.
                events += 1;
                continue;
            }
            match receiver.try_recv() {
                Ok(frame) => { self.pending.extend(parse(frame)?); frames += 1; }
                Err(TryRecvError::Empty) => {
                    guard()?;
                    return Ok(DrainProgress { reason: YieldReason::Empty, frames, events, pending_events: 0 });
                }
                Err(TryRecvError::Disconnected) => return Err("LIVE_STREAM_DISCONNECTED".into()),
            }
        }
    }
}

/// Counts from the durable dispatch log, not native quote counts. Both components
/// change when an old pending order closes and a new order is acknowledged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingVersion { pub acknowledged: u64, pub terminal: u64 }
impl PendingVersion {
    fn validate(self) -> Result<()> {
        if self.terminal > self.acknowledged { return Err("SCHEDULING_PENDING_VERSION".into()); }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconcileDue { Startup, PendingChanged, Periodic }
#[derive(Clone, Copy)]
struct Completed { at: u64, version: PendingVersion, blocked: bool }
/// Completion anchored so a slow full-tape cohort does not immediately force
/// another identical cohort. Monotonic milliseconds; never source timestamps.
pub struct ReconciliationSchedule { interval_ms: u64, completed: Option<Completed> }
impl ReconciliationSchedule {
    pub fn new(interval_ms: u64) -> Result<Self> {
        if interval_ms == 0 || interval_ms > 20_000 { return Err("SCHEDULING_RECONCILE_INTERVAL".into()); }
        Ok(Self { interval_ms, completed: None })
    }
    fn check(&self, now_ms: u64, version: PendingVersion) -> Result<()> {
        version.validate()?;
        if let Some(c) = self.completed {
            if now_ms < c.at || version.acknowledged < c.version.acknowledged || version.terminal < c.version.terminal {
                return Err("SCHEDULING_RECONCILE_REGRESSION".into());
            }
        }
        Ok(())
    }
    pub fn due(&self, now_ms: u64, version: PendingVersion) -> Result<Option<ReconcileDue>> {
        self.check(now_ms, version)?;
        let Some(c) = self.completed else { return Ok(Some(ReconcileDue::Startup)); };
        Ok(if c.version != version { Some(ReconcileDue::PendingChanged) }
            else if now_ms - c.at >= self.interval_ms { Some(ReconcileDue::Periodic) } else { None })
    }
    /// Call ONLY after a real successful reconciliation, using its CURRENT block
    /// result and the durable log version AFTER its terminal updates.
    pub fn completed(&mut self, now_ms: u64, version: PendingVersion, blocks_new_submissions: bool) -> Result<()> {
        self.check(now_ms, version)?;
        self.completed = Some(Completed { at: now_ms, version, blocked: blocks_new_submissions });
        Ok(())
    }
    pub fn permits_submission(&self, now_ms: u64, version: PendingVersion) -> Result<bool> {
        Ok(self.due(now_ms, version)?.is_none() && self.completed.is_some_and(|c| !c.blocked))
    }
}

/// Stable partition ONLY an existing Universe selection. Never add a scope or
/// rerun selection on each yield: preserve its existing fair rotation/cursor.
pub fn prioritize_selected(selected: &[String], owned: &BTreeSet<String>, urgent: &BTreeSet<String>) -> Result<Vec<String>> {
    if !urgent.is_subset(owned) { return Err("SCHEDULING_URGENT_NOT_OWNED".into()); }
    let mut seen = BTreeSet::new();
    if selected.iter().any(|s| !seen.insert(s)) { return Err("SCHEDULING_DUPLICATE_SCOPE".into()); }
    let mut out = Vec::with_capacity(selected.len());
    for class in 0..3 {
        out.extend(selected.iter().filter(|s| {
            let actual = if urgent.contains(*s) { 0 } else if owned.contains(*s) { 1 } else { 2 };
            actual == class
        }).cloned());
    }
    Ok(out)
}

/// Consume BEFORE expensive new-order snapshots, even if they later fail guards.
/// Reconciliation/cancellation are independent and must not consume this permit.
#[derive(Default)]
pub struct PreflightBudget { consumed: bool }
impl PreflightBudget {
    pub fn try_start(&mut self) -> bool {
        if self.consumed { false } else { self.consumed = true; true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::{Cell, RefCell}, sync::mpsc};
    fn budget(frames: usize, events: usize, elapsed_ms: u64) -> DrainBudget { DrainBudget { frames, events, elapsed_ms } }
    fn v(a: u64, t: u64) -> PendingVersion { PendingVersion { acknowledged: a, terminal: t } }
    #[test]
    fn continuous_arrivals_yield_without_prefetch_or_loss() {
        let (tx, rx) = mpsc::channel(); tx.send(0).unwrap();
        let mut drain = OrderedDrain::default(); let got = RefCell::new(Vec::new());
        let p = drain.drain_with_clock(&rx, budget(4, 99, 50), || Ok(()), |n| Ok(vec![n]), |n| {
            got.borrow_mut().push(*n); tx.send(n+1)?; Ok(())
        }, || 0).unwrap();
        assert_eq!(*got.borrow(), vec![0,1,2,3]); assert_eq!(p.reason, YieldReason::Frames);
        assert_eq!(rx.try_recv().unwrap(), 4);
    }
    #[test]
    fn large_frame_retained_and_later_frame_not_parsed_ahead() {
        let (tx, rx) = mpsc::channel(); tx.send(237).unwrap(); tx.send(1).unwrap();
        let mut drain = OrderedDrain::default(); let parsed = Cell::new(0); let got = RefCell::new(Vec::new());
        for _ in 0..14 {
            let p = drain.drain_with_clock(&rx, budget(32, 16, 50), || Ok(()), |n| { parsed.set(parsed.get()+1); Ok((0..n).collect()) }, |n| {got.borrow_mut().push(*n); Ok(())}, || 0).unwrap();
            assert_eq!(p.events, 16); assert_eq!(parsed.get(), 1);
        }
        assert_eq!(drain.pending_events(), 13);
        let p = drain.drain_with_clock(&rx, budget(32, 13, 50), || Ok(()), |_| panic!("prefetched"), |n| {got.borrow_mut().push(*n); Ok(())}, || 0).unwrap();
        assert_eq!(p.pending_events, 0); assert_eq!(*got.borrow(), (0..237).collect::<Vec<_>>());
        assert_eq!(rx.try_recv().unwrap(), 1);
    }
    #[test]
    fn elapsed_budget_preserves_next_event() {
        let (tx, rx) = mpsc::channel(); tx.send(()).unwrap();
        let mut drain = OrderedDrain::default(); let clock = Cell::new(0);
        let p = drain.drain_with_clock(&rx, budget(32, 64, 10), || Ok(()), |_| Ok(vec![1,2,3]), |_| {clock.set(clock.get()+7); Ok(())}, || clock.get()).unwrap();
        assert_eq!(p.reason, YieldReason::Time); assert_eq!(p.events, 2); assert_eq!(drain.pending.front(), Some(&3));
    }
    #[test]
    fn failed_apply_keeps_front_and_does_not_apply_later() {
        let (tx, rx) = mpsc::channel(); tx.send(()).unwrap(); tx.send(()).unwrap();
        let mut drain = OrderedDrain::default(); let calls = Cell::new(0);
        assert!(drain.drain_with_clock(&rx, budget(32,64,50), || Ok(()), |_| Ok(vec![1,2]), |_| {calls.set(calls.get()+1); Err("DURABLE_FAILURE".into())}, || 0).is_err());
        assert_eq!(calls.get(), 1); assert_eq!(drain.pending.front(), Some(&1)); assert!(rx.try_recv().is_ok());
    }
    #[test]
    fn stop_or_stream_failure_checked_after_last_budgeted_event() {
        let (tx, rx) = mpsc::channel(); tx.send(()).unwrap();
        let mut drain = OrderedDrain::default(); let healthy = Cell::new(true);
        assert!(drain.drain_with_clock(&rx, budget(1,1,50), || if healthy.get() {Ok(())} else {Err("STOP_OR_STREAM".into())}, |_| Ok(vec![1,2]), |_| {healthy.set(false); Ok(())}, || 0).is_err());
        assert_eq!(drain.pending.front(), Some(&2));
    }
    #[test]
    fn disconnect_is_not_empty_and_empty_requires_health() {
        let (tx, rx) = mpsc::channel::<()>(); let mut d = OrderedDrain::<()>::default();
        assert_eq!(d.drain(&rx, DrainBudget::default(), || Ok(()), |_| Ok(vec![]), |_| Ok(())).unwrap().reason, YieldReason::Empty);
        assert!(d.drain(&rx, DrainBudget::default(), || Err("UNHEALTHY".into()), |_| Ok(vec![]), |_| Ok(())).is_err());
        drop(tx); assert!(d.drain(&rx, DrainBudget::default(), || Ok(()), |_| Ok(vec![]), |_| Ok(())).is_err());
    }
    #[test]
    fn invalid_budget_does_not_receive() {
        let (tx, rx) = mpsc::channel(); tx.send(7).unwrap(); let mut d = OrderedDrain::<u8>::default();
        assert!(d.drain(&rx, budget(0,1,50), || Ok(()), |n| Ok(vec![n]), |_| Ok(())).is_err());
        assert_eq!(rx.try_recv().unwrap(), 7);
    }
    #[test]
    fn empty_frames_are_still_bounded() {
        let (tx, rx) = mpsc::channel(); for _ in 0..3 {tx.send(()).unwrap();}
        let p = OrderedDrain::<()>::default().drain_with_clock(&rx, budget(2,10,50), || Ok(()), |_| Ok(vec![]), |_| Ok(()), || 0).unwrap();
        assert_eq!(p.frames,2); assert_eq!(p.events,0); assert_eq!(p.reason,YieldReason::Frames); assert!(rx.try_recv().is_ok());
    }
    #[test]
    fn parse_time_overrun_does_not_start_first_event() {
        let (tx, rx) = mpsc::channel(); tx.send(()).unwrap(); let clock = Cell::new(0);
        let p = OrderedDrain::default().drain_with_clock(&rx, budget(2,10,50), || Ok(()), |_| {clock.set(100); Ok(vec![1])}, |_| panic!("over budget"), || clock.get()).unwrap();
        assert_eq!(p.events,0); assert_eq!(p.pending_events,1); assert_eq!(p.reason,YieldReason::Time);
    }
    #[test]
    fn scheduling_startup_periodic_and_completion_anchor() {
        let mut s = ReconciliationSchedule::new(10_000).unwrap();
        assert_eq!(s.due(0,v(20,0)).unwrap(),Some(ReconcileDue::Startup)); assert!(!s.permits_submission(0,v(20,0)).unwrap());
        s.completed(60_000,v(20,0),false).unwrap(); // 60s previous loop gap is harmless.
        assert!(s.permits_submission(69_999,v(20,0)).unwrap()); assert_eq!(s.due(70_000,v(20,0)).unwrap(),Some(ReconcileDue::Periodic));
        // A failed attempt does not call completed and cannot move this deadline.
        assert!(!s.permits_submission(75_000,v(20,0)).unwrap());
        s.completed(95_000,v(20,0),false).unwrap(); assert!(s.permits_submission(95_001,v(20,0)).unwrap());
    }
    #[test]
    fn changed_members_with_same_pending_count_force_reconcile() {
        let mut s = ReconciliationSchedule::new(1000).unwrap(); s.completed(0,v(20,10),false).unwrap();
        assert_eq!(s.due(1,v(21,11)).unwrap(),Some(ReconcileDue::PendingChanged)); assert!(!s.permits_submission(1,v(21,11)).unwrap());
    }
    #[test]
    fn latest_block_result_replaces_previous_permission() {
        let mut s = ReconciliationSchedule::new(1000).unwrap(); s.completed(0,v(1,0),false).unwrap();
        s.completed(1,v(1,0),true).unwrap(); assert!(!s.permits_submission(2,v(1,0)).unwrap());
        s.completed(3,v(1,1),false).unwrap(); assert!(s.permits_submission(4,v(1,1)).unwrap());
    }
    #[test]
    fn schedule_rejects_clock_and_log_regression_and_bad_interval() {
        assert!(ReconciliationSchedule::new(0).is_err()); assert!(ReconciliationSchedule::new(20_001).is_err());
        let mut s = ReconciliationSchedule::new(1000).unwrap(); s.completed(10,v(3,1),false).unwrap();
        assert!(s.due(9,v(3,1)).is_err()); assert!(s.due(11,v(2,1)).is_err()); assert!(s.due(11,v(3,0)).is_err()); assert!(s.due(11,v(3,4)).is_err());
    }
    fn strings(xs: &[&str]) -> Vec<String> {xs.iter().map(|s|s.to_string()).collect()}
    #[test]
    fn stable_priority_keeps_fair_owned_rotation_and_three_discovery_slots() {
        let selected = strings(&["d1","o2","d2","o1","urgent","d3","o3"]);
        let owned = strings(&["o1","o2","o3","urgent"]).into_iter().collect();
        let urgent = strings(&["urgent"]).into_iter().collect();
        assert_eq!(prioritize_selected(&selected,&owned,&urgent).unwrap(),strings(&["urgent","o2","o1","o3","d1","d2","d3"]));
    }
    #[test]
    fn priority_never_inserts_unselected_owned_and_rejects_duplicates_or_unowned_urgent() {
        let owned = strings(&["o1","o2"]).into_iter().collect();
        assert_eq!(prioritize_selected(&strings(&["d1","o1"]),&owned,&strings(&["o2"]).into_iter().collect()).unwrap(),strings(&["o1","d1"]));
        assert!(prioritize_selected(&strings(&["o1","o1"]),&owned,&BTreeSet::new()).is_err());
        assert!(prioritize_selected(&strings(&["d1"]),&owned,&strings(&["d1"]).into_iter().collect()).is_err());
    }
    #[test]
    fn failed_preflight_consumes_only_attempt_and_rotation_can_progress() {
        // Worker preserves this candidate cursor across passes, never reuses snapshots.
        let candidates = ["expired", "good"]; let mut cursor = 0; let mut expensive = 0;
        for _ in 0..2 {
            let mut pass = PreflightBudget::default();
            while cursor < candidates.len() && pass.try_start() {
                let q = candidates[cursor]; cursor += 1; expensive += 1;
                if q == "expired" {continue;} // Still cannot start a second six-GET packet.
            }
            assert_eq!(expensive,cursor);
        }
        assert_eq!(cursor,2); assert_eq!(expensive,2);
    }
}

/// Stable market/side/action keys retain fairness when quote IDs are refreshed.
pub fn preflight_rotation(keys:&[String],last:Option<&str>)->Vec<usize>{
 let mut order:Vec<usize>=(0..keys.len()).collect();order.sort_by_key(|i|(last.is_some_and(|at|keys[*i].as_str()<=at),keys[*i].clone(),*i));order
}
#[cfg(test)]mod rotation_tests{use super::*;
 #[test]fn changing_quote_ids_does_not_reset_market_rotation(){let keys=vec!["B:YES:sell".into(),"A:YES:buy".into(),"C:NO:sell".into()];assert_eq!(preflight_rotation(&keys,None),vec![1,0,2]);assert_eq!(preflight_rotation(&keys,Some("B:YES:sell")),vec![2,1,0]);assert_eq!(preflight_rotation(&keys,Some("C:NO:sell")),vec![1,0,2]);}
 #[test]fn absent_cursor_scope_and_multiple_quotes_keep_stable_order(){let keys=vec!["A".into(),"C".into(),"C".into(),"D".into()];assert_eq!(preflight_rotation(&keys,Some("B")),vec![1,2,3,0]);assert!(preflight_rotation(&[],Some("B")).is_empty());}
}
