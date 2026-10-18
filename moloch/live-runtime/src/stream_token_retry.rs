//! A separate retry lane for the SDK's actual read-budget realtime-token call.
//! Construct ONLY at `Transport::realtime_token`'s error boundary. This is not a
//! native connection failure, an order outcome proof, or a generic HTTP retry.
use baal_transport_service::TransportError;
use crate::{stream_connect_retry::{self,RetryError},stream_recovery::{RecoveryBudget,MAXIMUM_CLOCK_MS}};
use std::{error::Error,fmt};

/// Opaque, non-deserializable classification retaining the original typed error.
#[derive(Debug)]
pub struct TokenFailure { original:TransportError }
impl TokenFailure {
 /// Return fatal errors unchanged; no display-text or JSON classification.
 /// The caller must use this ONLY on the concrete token endpoint SDK result.
 pub fn from_token_error(error:TransportError)->Box<dyn Error>{
  let transient_status=|s:u16|matches!(s,408|429|500|502|503|504);
  // The official API can attach its structured transient code. Rejecting every
  // Some(code) discards the existing charged retry lane even for a token-only
  // rate limit or internal error. Unknown/financial codes remain fatal.
  let token_api_code=match error.api_code.as_deref(){
   None=>true,
   Some("INTERNAL_ERROR")=>matches!(error.status,Some(500|502|503|504)),
   Some("SERVICE_UNAVAILABLE")=>error.status==Some(503),
   Some("RATE_LIMITED"|"RATE_LIMIT_EXCEEDED"|"TOO_MANY_REQUESTS")=>error.status==Some(429),
   _=>false,
  };
  let eligible=!error.may_have_executed&&error.order_id.is_none()&&token_api_code
   &&match error.code.as_str(){
    "CONNECTION_UNCONFIRMED"=>error.status.is_none(),
    "BODY_UNCONFIRMED"=>error.status.is_some_and(|s|(200..300).contains(&s)||transient_status(s)),
    "OFFICIAL_REQUEST_REJECTED"=>error.status.is_some_and(transient_status),
    _=>false,
   };
  if eligible{Box::new(Self{original:error})}else{Box::new(error)}
 }
 pub fn reason(&self)->&str{&self.original.code}
 pub fn status(&self)->Option<u16>{self.original.status}
 pub fn retry_after_ms(&self)->u64{self.original.retry_after_ms}
 pub fn api_code(&self)->Option<&str>{self.original.api_code.as_deref()}
}
impl fmt::Display for TokenFailure{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{write!(f,"STREAM_TOKEN_{}",self.reason())}}
impl Error for TokenFailure{fn source(&self)->Option<&(dyn Error+'static)>{Some(&self.original)}}

#[derive(Debug)]
pub enum TokenRetryError{Timing(RetryError),UnchargedMint,DelayRange}
impl fmt::Display for TokenRetryError{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{match self{
 Self::Timing(e)=>e.fmt(f),Self::UnchargedMint=>f.write_str("STREAM_TOKEN_UNCHARGED_MINT"),Self::DelayRange=>f.write_str("STREAM_TOKEN_RETRY_CLOCK_RANGE"),
}}}
impl Error for TokenRetryError{}

/// Query only. The same persisted budget was charged BEFORE this failed mint;
/// never refund, reseed, charge again, or start waiting out an exhausted window.
/// SDK Retry-After is remaining persisted account cooldown, not a new limiter.
pub fn after_token_failure(budget:&RecoveryBudget,now_ms:u64,last_auth_mint_ms:Option<u64>,failure:&TokenFailure)->Result<u64,TokenRetryError>{
 let next=stream_connect_retry::next_attempt(budget,now_ms,last_auth_mint_ms).map_err(TokenRetryError::Timing)?;
 let mint=last_auth_mint_ms.ok_or(TokenRetryError::UnchargedMint)?;
 if budget.attempts().last().is_none_or(|attempt|mint<*attempt){return Err(TokenRetryError::UnchargedMint)}
 let cooldown=now_ms.checked_add(failure.retry_after_ms()).filter(|at|*at<=MAXIMUM_CLOCK_MS).ok_or(TokenRetryError::DelayRange)?;
 Ok(next.max(cooldown))
}
