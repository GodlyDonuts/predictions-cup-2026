//! Pure recovery classification and persisted attempt budget. No reconnect/I/O.
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

/// Only a deliberately typed stream failure may request worker recovery.
/// Never construct this from an uncertain order/write failure or arbitrary text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamFailure { reason: &'static str }
impl StreamFailure {
    pub fn new(reason: &'static str) -> Self {
        Self { reason: match reason {
            "STREAM_FRAME" | "STREAM_JOIN_REJECTED" | "STREAM_HEARTBEAT_REJECTED"
            | "STREAM_REPLY_REF" | "STREAM_TOPIC" | "STREAM_BEFORE_JOIN"
            | "STREAM_BROADCAST" | "STREAM_SYSTEM_REJECTED" | "STREAM_CHANNEL_CLOSED"
            | "STREAM_EVENT" | "STREAM_HEARTBEAT_PENDING" | "STREAM_JOIN_TIMEOUT"
            | "STREAM_HEARTBEAT_TIMEOUT" | "STREAM_HEARTBEAT_SEND" | "STREAM_JSON"
            | "STREAM_QUEUE_FULL" | "STREAM_CONSUMER_DISCONNECTED" | "STREAM_SOCKET_CLOSED"
            | "STREAM_NON_TEXT" | "STREAM_READ_FAILED" | "STREAM_FLUSH"
            | "STREAM_DEADLINE_REACHED" | "STREAM_STOP_REQUESTED"
            | "STREAM_WORKER_PANICKED" | "STREAM_WORKER_EXIT_UNEXPLAINED" => reason,
            _ => "STREAM_WORKER_EXIT_UNEXPLAINED",
        } }
    }
    pub fn reason(self) -> &'static str { self.reason }
    pub fn is_recoverable(self) -> bool {
        matches!(self.reason,"STREAM_DEADLINE_REACHED" | "STREAM_SOCKET_CLOSED"
            | "STREAM_READ_FAILED" | "STREAM_FLUSH" | "STREAM_HEARTBEAT_SEND"
            | "STREAM_HEARTBEAT_TIMEOUT" | "STREAM_QUEUE_FULL")
    }
}
impl fmt::Display for StreamFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {f.write_str(self.reason)}
}
impl Error for StreamFailure {}

pub const RECOVERY_WINDOW_MS: u64 = 600_000;
pub const RECOVERY_MINIMUM_GAP_MS: u64 = 60_000;
pub const RECOVERY_MAXIMUM_ATTEMPTS: usize = 3;
pub const MAXIMUM_CLOCK_MS: u64 = 8_640_000_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetError { Schema, ClockRange, ClockRegression, FutureAttempt, Overfull, Unordered, Spacing, TooSoon, ClockOverflow }
impl fmt::Display for BudgetError {
    fn fmt(&self, f:&mut fmt::Formatter<'_>)->fmt::Result {f.write_str(match self {
        Self::Schema=>"STREAM_RECOVERY_BUDGET_SCHEMA",Self::ClockRange=>"STREAM_RECOVERY_CLOCK_RANGE",
        Self::ClockRegression=>"STREAM_RECOVERY_CLOCK_REGRESSION",Self::FutureAttempt=>"STREAM_RECOVERY_FUTURE_ATTEMPT",
        Self::Overfull=>"STREAM_RECOVERY_BUDGET_OVERFULL",Self::Unordered=>"STREAM_RECOVERY_ATTEMPTS_UNORDERED",
        Self::Spacing=>"STREAM_RECOVERY_ATTEMPTS_SPACING",Self::TooSoon=>"STREAM_RECOVERY_ATTEMPT_TOO_SOON",
        Self::ClockOverflow=>"STREAM_RECOVERY_CLOCK_OVERFLOW",
    })}
}
impl Error for BudgetError {}

/// Loading malformed state is fatal; never replace an existing budget with new().
/// Field presence is mandatory, including an explicitly empty attempts array.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct RecoveryBudget { schema_version:u32, last_clock_ms:u64, attempts:Vec<u64> }
impl RecoveryBudget {
    /// For a genuinely new, absent budget only. Caller decides/persists absence.
    pub fn new(now_ms:u64)->Result<Self,BudgetError> {
        let budget=Self{schema_version:1,last_clock_ms:now_ms,attempts:vec![]};budget.validate(now_ms)?;Ok(budget)
    }
    pub fn validate(&self,now_ms:u64)->Result<(),BudgetError> {
        if self.schema_version!=1{return Err(BudgetError::Schema)}
        if now_ms>MAXIMUM_CLOCK_MS || self.last_clock_ms>MAXIMUM_CLOCK_MS{return Err(BudgetError::ClockRange)}
        if now_ms<self.last_clock_ms{return Err(BudgetError::ClockRegression)}
        if self.attempts.len()>RECOVERY_MAXIMUM_ATTEMPTS{return Err(BudgetError::Overfull)}
        for (i,at) in self.attempts.iter().copied().enumerate(){
            if at>self.last_clock_ms{return Err(BudgetError::FutureAttempt)}
            if i>0 {
                let previous=self.attempts[i-1];
                if at<=previous{return Err(BudgetError::Unordered)}
                if at-previous<RECOVERY_MINIMUM_GAP_MS{return Err(BudgetError::Spacing)}
            }
        }
        Ok(())
    }
    /// Earliest permitted time, or now when immediately eligible. Pure query:
    /// never advances the durable clock or discards retained attempt evidence.
    /// Rolling window includes its boundary (attempt age <=600_000 counts).
    pub fn next_allowed(&self,now_ms:u64)->Result<u64,BudgetError> {
        self.validate(now_ms)?;
        let mut next=now_ms;
        if let Some(last)=self.attempts.last(){
            next=next.max(last.checked_add(RECOVERY_MINIMUM_GAP_MS).filter(|n|*n<=MAXIMUM_CLOCK_MS).ok_or(BudgetError::ClockOverflow)?);
        }
        let retained:Vec<_>=self.attempts.iter().copied().filter(|at|now_ms-*at<=RECOVERY_WINDOW_MS).collect();
        if retained.len()>=RECOVERY_MAXIMUM_ATTEMPTS {
            next=next.max(retained[0].checked_add(RECOVERY_WINDOW_MS+1).filter(|n|*n<=MAXIMUM_CLOCK_MS).ok_or(BudgetError::ClockOverflow)?);
        }
        Ok(next)
    }
    /// Mutates ONLY after full validation/admission. Caller MUST atomically
    /// persist this resulting state BEFORE token mint/connect; failure consumes
    /// the local operation and forbids proceeding. No token is minted here.
    pub fn record_attempt(&mut self,now_ms:u64)->Result<(),BudgetError> {
        if self.next_allowed(now_ms)?>now_ms{return Err(BudgetError::TooSoon)}
        let mut next=self.clone();
        next.attempts.retain(|at|now_ms-*at<=RECOVERY_WINDOW_MS);
        next.attempts.push(now_ms);next.last_clock_ms=now_ms;next.validate(now_ms)?;
        *self=next;Ok(())
    }
    pub fn last_clock_ms(&self)->u64{self.last_clock_ms}
    pub fn attempts(&self)->&[u64]{&self.attempts}
}

#[cfg(test)]mod tests {
    use super::*;use serde_json::{Value,json};
    #[test]fn only_explicit_transient_stream_reasons_recover(){
        for code in ["STREAM_DEADLINE_REACHED","STREAM_SOCKET_CLOSED","STREAM_READ_FAILED","STREAM_FLUSH","STREAM_HEARTBEAT_SEND","STREAM_HEARTBEAT_TIMEOUT","STREAM_QUEUE_FULL"]{assert!(StreamFailure::new(code).is_recoverable(),"{code}");}
        for code in ["STREAM_JSON","STREAM_FRAME","STREAM_JOIN_REJECTED","STREAM_JOIN_TIMEOUT","STREAM_HEARTBEAT_REJECTED","STREAM_TOPIC","STREAM_CHANNEL_CLOSED","STREAM_CONSUMER_DISCONNECTED","STREAM_STOP_REQUESTED","STREAM_WORKER_PANICKED","STREAM_WORKER_EXIT_UNEXPLAINED","LIVE_WRITE_NOT_CONFIRMED_NO_RETRY"]{assert!(!StreamFailure::new(code).is_recoverable(),"{code}");}
    }
    #[test]fn sanitized_type_downcast_is_required_not_matching_error_text(){
        let typed:Box<dyn Error>=Box::new(StreamFailure::new("STREAM_SOCKET_CLOSED"));assert!(typed.downcast_ref::<StreamFailure>().unwrap().is_recoverable());
        let unrelated:Box<dyn Error>=Box::new(std::io::Error::other("STREAM_SOCKET_CLOSED"));assert!(unrelated.downcast_ref::<StreamFailure>().is_none());
        let secret=StreamFailure::new("https://fixture.invalid/?token=DO_NOT_STORE");assert_eq!(secret.to_string(),"STREAM_WORKER_EXIT_UNEXPLAINED");assert!(!secret.is_recoverable());
    }
    #[test]fn three_attempts_and_minimum_gap_are_strict(){
        let mut b=RecoveryBudget::new(0).unwrap();assert_eq!(b.next_allowed(0).unwrap(),0);b.record_attempt(0).unwrap();
        assert_eq!(b.next_allowed(59_999).unwrap(),60_000);let previous=b.clone();assert_eq!(b.record_attempt(59_999),Err(BudgetError::TooSoon));assert_eq!(b,previous);
        b.record_attempt(60_000).unwrap();b.record_attempt(120_000).unwrap();assert_eq!(b.attempts(),&[0,60_000,120_000]);assert_eq!(b.next_allowed(180_000).unwrap(),600_001);assert_eq!(b.record_attempt(180_000),Err(BudgetError::TooSoon));
    }
    #[test]fn restart_preserves_failed_mint_attempts_without_a_reset(){
        let mut b=RecoveryBudget::new(1000).unwrap();for now in [1000,61_000,121_000]{b.record_attempt(now).unwrap();}
        // Mint/connect success is irrelevant: all admitted attempts are retained.
        let bytes=serde_json::to_vec(&b).unwrap();let mut loaded:RecoveryBudget=serde_json::from_slice(&bytes).unwrap();assert_eq!(loaded,b);assert_eq!(loaded.record_attempt(181_000),Err(BudgetError::TooSoon));assert_eq!(loaded.next_allowed(181_000).unwrap(),601_001);
    }
    #[test]fn rolling_boundary_expires_only_after_exact_window(){
        let mut b=RecoveryBudget::new(0).unwrap();for now in [0,60_000,120_000]{b.record_attempt(now).unwrap();}
        assert_eq!(b.next_allowed(600_000).unwrap(),600_001);assert_eq!(b.record_attempt(600_000),Err(BudgetError::TooSoon));b.record_attempt(600_001).unwrap();assert_eq!(b.attempts(),&[60_000,120_000,600_001]);
        assert_eq!(b.next_allowed(660_001).unwrap(),660_001);b.record_attempt(660_001).unwrap();assert_eq!(b.attempts(),&[120_000,600_001,660_001]);
    }
    #[test]fn regressed_clock_is_error_not_pruning_or_new_budget(){
        let mut b=RecoveryBudget::new(1000).unwrap();b.record_attempt(1000).unwrap();let previous=b.clone();assert_eq!(b.next_allowed(999),Err(BudgetError::ClockRegression));assert_eq!(b.record_attempt(999),Err(BudgetError::ClockRegression));assert_eq!(b,previous);
    }
    fn loaded(value:Value)->RecoveryBudget{serde_json::from_value(value).unwrap()}
    #[test]fn bad_persisted_schema_future_unordered_spacing_and_overfull_fail(){
        let cases=[(json!({"schemaVersion":2,"lastClockMs":200_000,"attempts":[]}),BudgetError::Schema),
            (json!({"schemaVersion":1,"lastClockMs":200_001,"attempts":[]}),BudgetError::ClockRegression),
            (json!({"schemaVersion":1,"lastClockMs":200_000,"attempts":[200_001]}),BudgetError::FutureAttempt),
            (json!({"schemaVersion":1,"lastClockMs":200_000,"attempts":[60_000,0]}),BudgetError::Unordered),
            (json!({"schemaVersion":1,"lastClockMs":200_000,"attempts":[0,0]}),BudgetError::Unordered),
            (json!({"schemaVersion":1,"lastClockMs":200_000,"attempts":[0,59_999]}),BudgetError::Spacing),
            (json!({"schemaVersion":1,"lastClockMs":200_000,"attempts":[0,60_000,120_000,180_000]}),BudgetError::Overfull)];
        for(value,error)in cases{let mut b=loaded(value);let previous=b.clone();assert_eq!(b.next_allowed(200_000),Err(error));assert_eq!(b.record_attempt(200_000),Err(error));assert_eq!(b,previous);}
    }
    #[test]fn missing_unknown_negative_and_wrong_type_fields_are_not_defaulted(){
        for value in [json!({}),json!({"schemaVersion":1,"lastClockMs":0}),json!({"schemaVersion":1,"attempts":[]}),json!({"lastClockMs":0,"attempts":[]}),json!({"schemaVersion":1,"lastClockMs":0,"attempts":[],"reset":true}),json!({"schemaVersion":1,"lastClockMs":-1,"attempts":[]}),json!({"schemaVersion":1,"lastClockMs":0,"attempts":null})]{assert!(serde_json::from_value::<RecoveryBudget>(value).is_err());}
    }
    #[test]fn queries_do_not_mutate_clock_or_retained_history(){let mut b=RecoveryBudget::new(0).unwrap();b.record_attempt(0).unwrap();let previous=b.clone();assert_eq!(b.next_allowed(1_000_000).unwrap(),1_000_000);assert_eq!(b,previous);assert_eq!(b.last_clock_ms(),0);}
    #[test]fn clock_range_and_overflow_fail_closed_without_mutation(){
        assert_eq!(RecoveryBudget::new(MAXIMUM_CLOCK_MS+1),Err(BudgetError::ClockRange));
        let mut b=RecoveryBudget::new(MAXIMUM_CLOCK_MS).unwrap();b.record_attempt(MAXIMUM_CLOCK_MS).unwrap();let previous=b.clone();assert_eq!(b.next_allowed(MAXIMUM_CLOCK_MS),Err(BudgetError::ClockOverflow));assert_eq!(b.record_attempt(MAXIMUM_CLOCK_MS),Err(BudgetError::ClockOverflow));assert_eq!(b,previous);
    }
}
