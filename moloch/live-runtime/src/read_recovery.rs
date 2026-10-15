use baal_transport_service::TransportError;
/// A rejected cancellation is never retried. Only a later exact terminal order
/// and complete fill tape may satisfy its desired state without stopping.
pub fn reconcilable_cancel_rejection(error:&TransportError)->bool{!error.may_have_executed&&matches!(error.status,Some(400|404|409))}
/// Classification is restricted to callers issuing typed GET observations.
pub fn retryable_read(error:&TransportError)->bool{
 if error.may_have_executed{return false}
 let status=error.status.unwrap_or(0);
 match error.code.as_str(){
  "CONNECTION_UNCONFIRMED"|"BODY_UNCONFIRMED"|"INVALID_RESPONSE_JSON"=>status==0||(200..300).contains(&status)||status==429||(500..600).contains(&status),
  "OFFICIAL_REQUEST_REJECTED"=>status==429||(500..600).contains(&status)||status==409&&error.api_code.as_deref()==Some("REQUEST_IN_FLIGHT"),
  _=>false
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn e(code:&str,status:Option<u16>,unknown:bool)->TransportError{TransportError{code:code.into(),status,may_have_executed:unknown,retry_after_ms:0,order_id:None,api_code:None}}
 #[test]fn rejected_cancel_only_qualifies_for_later_terminal_proof_never_write_retry(){for status in [400,404,409]{assert!(reconcilable_cancel_rejection(&e("OFFICIAL_REQUEST_REJECTED",Some(status),false)));assert!(!reconcilable_cancel_rejection(&e("OFFICIAL_REQUEST_REJECTED",Some(status),true)));}for status in [None,Some(200),Some(401),Some(403),Some(429),Some(500)]{assert!(!reconcilable_cancel_rejection(&e("OFFICIAL_REQUEST_REJECTED",status,false)));}}
 #[test]fn malformed_success_and_temporary_read_failures_can_recover(){for code in ["CONNECTION_UNCONFIRMED","BODY_UNCONFIRMED","INVALID_RESPONSE_JSON"]{for status in [None,Some(200),Some(429),Some(503)]{assert!(retryable_read(&e(code,status,false)))}}assert!(retryable_read(&e("OFFICIAL_REQUEST_REJECTED",Some(502),false)));}
 #[test]fn unknown_writes_client_errors_and_local_fences_are_never_retried(){for code in ["CONNECTION_UNCONFIRMED","INVALID_RESPONSE_JSON","OFFICIAL_REQUEST_REJECTED"]{assert!(!retryable_read(&e(code,Some(503),true)));for status in [301,400,401,403,404,409]{assert!(!retryable_read(&e(code,Some(status),false)))}}for code in ["COMPLETION_DURABILITY_UNCERTAIN","DURABILITY_FAILURE","EXPLICIT_COMPACTION_REQUIRED","STOPPED_OR_OWNER_LOST","SCOPED_LEASE_LOST","INVALID_FILL"]{assert!(!retryable_read(&e(code,None,false)))}}
}
