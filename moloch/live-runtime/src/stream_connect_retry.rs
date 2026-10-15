//! Bounded, pure planning for connection setup retries. No clients/files/sleeps.
//!
//! Integrate around **Realtime::connect only**, whose errors are static native
//! codes. Never classify a realtime-token HTTP error, Auth parse error, JOIN
//! failure, source admission failure, arbitrary BoxError display text or order
//! error through this type. DNS/TCP failure precedes every contest order; it says
//! nothing about the separate token-mint HTTP operation or its durability.
//!
//! Startup and stream replacement use the SAME persisted RecoveryBudget. Before
//! EVERY connect_stream invocation (including first startup), obtain not_before,
//! wait cooperatively with STOP/control/clock checks, then recheck resolved
//! dispatch attempts, global unknown/active/manual-review state and all controls.
//! Recompute/prepare the budget and durably persist it BEFORE mint/connect.
//! Existing recover_stream already consumes once: replace that admission; do not
//! add a second charge. Never reset budget after success, failure or restart.
//!
//! Only a typed failure returned directly by Realtime::connect can request the
//! next attempt. Failed mint/Auth/JOIN/protocol/durability remains fatal. Drop/join
//! the old stream once before this loop, clear Ready, and never bind a new stream
//! healthy before confirmed JOIN plus actual durable subscribed-gap events.
//! Preserve the old per-scope history boundaries and ordinary fill liabilities.
//!
//! Max three admitted attempts per inclusive rolling ten minutes; >=60 seconds
//! between all attempts, also honoring the durable token-mint marker. Exhaustion
//! stops the current startup/recovery operation, rather than waiting ten minutes
//! to start an unbounded loop. A retry never retries an order or renews authority.
use crate::stream_recovery::{BudgetError,RecoveryBudget,MAXIMUM_CLOCK_MS,RECOVERY_MAXIMUM_ATTEMPTS,RECOVERY_MINIMUM_GAP_MS,RECOVERY_WINDOW_MS};
use std::{error::Error,fmt};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ConnectionFailure { reason:&'static str }
impl ConnectionFailure {
 /// Call ONLY on the static error branch of native Realtime::connect.
 pub fn from_connect_error(code:&'static str)->Self{Self{reason:match code{
  "STREAM_DNS"|"STREAM_CONNECT"|"STREAM_AUTH_URL"|"STREAM_HOST"|"STREAM_TIMEOUT"|"STREAM_HANDSHAKE"|"STREAM_JOIN_SEND"=>code,
  _=>"STREAM_CONNECT_UNCLASSIFIED",
 }}}
 pub fn reason(self)->&'static str{self.reason}
 pub fn is_retryable(self)->bool{matches!(self.reason,"STREAM_DNS"|"STREAM_CONNECT")}
}
impl fmt::Display for ConnectionFailure{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{f.write_str(self.reason)}}
impl Error for ConnectionFailure{}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum RetryError {Budget(BudgetError),Exhausted,FatalConnection(ConnectionFailure),AuthClockRegression,AuthClockRange,TooSoon}
impl fmt::Display for RetryError{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{match self{
 Self::Budget(e)=>e.fmt(f),Self::Exhausted=>f.write_str("STREAM_CONNECT_ATTEMPTS_EXHAUSTED"),
 Self::FatalConnection(failure)=>fmt::Display::fmt(failure,f),Self::AuthClockRegression=>f.write_str("STREAM_CONNECT_AUTH_CLOCK_REGRESSION"),
 Self::AuthClockRange=>f.write_str("STREAM_CONNECT_AUTH_CLOCK_RANGE"),Self::TooSoon=>f.write_str("STREAM_CONNECT_ATTEMPT_TOO_SOON"),
 }}}
impl Error for RetryError{}
impl From<BudgetError> for RetryError{fn from(e:BudgetError)->Self{Self::Budget(e)}}

/// Earliest first startup/replacement attempt. Query only; never prunes/advances
/// durable state. Caller still validates the reason for replacing a live stream.
pub fn next_attempt(budget:&RecoveryBudget,now_ms:u64,last_auth_mint_ms:Option<u64>)->Result<u64,RetryError>{
 budget.validate(now_ms)?;
 if budget.attempts().iter().filter(|at|now_ms-**at<=RECOVERY_WINDOW_MS).count()>=RECOVERY_MAXIMUM_ATTEMPTS{return Err(RetryError::Exhausted)}
 let mut next=budget.next_allowed(now_ms)?;
 if let Some(auth)=last_auth_mint_ms{
  if auth>MAXIMUM_CLOCK_MS{return Err(RetryError::AuthClockRange)}
  if auth>now_ms{return Err(RetryError::AuthClockRegression)}
  next=next.max(auth.checked_add(RECOVERY_MINIMUM_GAP_MS).filter(|n|*n<=MAXIMUM_CLOCK_MS).ok_or(RetryError::AuthClockRange)?);
 }
 Ok(next)
}
/// Allows retries only for deliberate, bounded native connection failures.
/// The caller must not pass mint/authority/JOIN/source/write errors here.
pub fn after_connect_failure(budget:&RecoveryBudget,now_ms:u64,last_auth_mint_ms:Option<u64>,failure:ConnectionFailure)->Result<u64,RetryError>{
 if !failure.is_retryable(){return Err(RetryError::FatalConnection(failure))}
 next_attempt(budget,now_ms,last_auth_mint_ms)
}
/// Clone the exact resulting budget to persist atomically BEFORE token mint.
/// A successful return is not proof of persistence and grants no execution.
/// Recomputes admission using current time/state, so an old wait result cannot
/// bypass newer attempts, an updated auth marker or clock regression.
pub fn prepare_attempt(budget:&RecoveryBudget,now_ms:u64,last_auth_mint_ms:Option<u64>)->Result<RecoveryBudget,RetryError>{
 if next_attempt(budget,now_ms,last_auth_mint_ms)?>now_ms{return Err(RetryError::TooSoon)}
 let mut next=budget.clone();next.record_attempt(now_ms)?;Ok(next)
}
