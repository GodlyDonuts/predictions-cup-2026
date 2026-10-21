use baal_transport_service::TransportError;
use moloch_live_runtime::{stream_token_retry::{TokenFailure,after_token_failure},stream_connect_retry::{self,ConnectionFailure},stream_recovery::{RecoveryBudget,MAXIMUM_CLOCK_MS}};
fn error(code:&str,status:Option<u16>)->TransportError{TransportError{code:code.into(),status,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None}}
fn classified(e:TransportError)->Box<dyn std::error::Error>{TokenFailure::from_token_error(e)}

#[test]fn exact_sdk_transient_token_errors_have_a_separate_type(){
 for (code,status) in [("CONNECTION_UNCONFIRMED",None),("BODY_UNCONFIRMED",Some(200)),("BODY_UNCONFIRMED",Some(503)),("OFFICIAL_REQUEST_REJECTED",Some(408)),("OFFICIAL_REQUEST_REJECTED",Some(429)),("OFFICIAL_REQUEST_REJECTED",Some(500)),("OFFICIAL_REQUEST_REJECTED",Some(502)),("OFFICIAL_REQUEST_REJECTED",Some(503)),("OFFICIAL_REQUEST_REJECTED",Some(504))]{
  let e=classified(error(code,status));let f=e.downcast_ref::<TokenFailure>().unwrap();assert_eq!(f.reason(),code);assert_eq!(f.status(),status);assert!(e.downcast_ref::<ConnectionFailure>().is_none());
 }
}
#[test]fn auth_schema_durability_lease_and_uncertainty_errors_remain_original_fatal_errors(){
 for(code,status)in [("REALTIME_AUTH_FORMAT",Some(200)),("REALTIME_AUTH_EXPIRY",Some(200)),("INVALID_RESPONSE_JSON",Some(200)),("RESPONSE_TOO_LARGE",Some(200)),("COMPLETION_DURABILITY_UNCERTAIN",None),("LOCAL_NOT_SENT_DURABILITY_UNCERTAIN",None),("STOPPED_OR_OWNER_LOST",None),("EXCLUSIVE_QUEUE_LOCK_REQUIRED",None),("UNKNOWN_ACCEPTANCE_RECONCILE_FIRST",None),("OFFICIAL_REQUEST_REJECTED",Some(401)),("OFFICIAL_REQUEST_REJECTED",Some(403)),("BODY_UNCONFIRMED",Some(401)),("BODY_UNCONFIRMED",None),("CONNECTION_UNCONFIRMED",Some(200)),("OFFICIAL_REQUEST_REJECTED",Some(409)),("STREAM_DNS",None),("LIVE_STREAM_JOIN_FAILED",None)]{
  let original=error(code,status);let got=classified(original.clone());assert!(got.downcast_ref::<TokenFailure>().is_none(),"{code} {status:?}");let retained=got.downcast_ref::<TransportError>().unwrap();assert_eq!(serde_json::to_value(retained).unwrap(),serde_json::to_value(original).unwrap());
 }
}
#[test]fn possibly_executed_order_identity_and_unknown_api_codes_never_enter_token_lane(){
 for field in 0..3{let mut e=error("OFFICIAL_REQUEST_REJECTED",Some(503));match field{0=>e.may_have_executed=true,1=>e.order_id=Some("123".into()),_=>e.api_code=Some("ORDER_STATUS_UNKNOWN".into())}assert!(classified(e).downcast_ref::<TokenFailure>().is_none());}
 let text:Box<dyn std::error::Error>=Box::new(std::io::Error::other("CONNECTION_UNCONFIRMED"));assert!(text.downcast_ref::<TokenFailure>().is_none());
}
#[test]fn structured_transient_token_codes_retain_original_error_and_cooldown(){
 for(status,code)in[(500,"INTERNAL_ERROR"),(502,"INTERNAL_ERROR"),(503,"INTERNAL_ERROR"),(504,"INTERNAL_ERROR"),(503,"SERVICE_UNAVAILABLE"),(429,"RATE_LIMITED"),(429,"RATE_LIMIT_EXCEEDED"),(429,"TOO_MANY_REQUESTS")]{
  let mut e=error("OFFICIAL_REQUEST_REJECTED",Some(status));e.api_code=Some(code.into());e.retry_after_ms=10000;
  let got=classified(e);let f=got.downcast_ref::<TokenFailure>().expect("token-only structured transient");assert_eq!(f.api_code(),Some(code));assert_eq!(f.status(),Some(status));assert_eq!(f.retry_after_ms(),10000);
  let b=stream_connect_retry::prepare_attempt(&RecoveryBudget::new(1000).unwrap(),1000,None).unwrap();let original=serde_json::to_vec(&b).unwrap();assert_eq!(after_token_failure(&b,2000,Some(1002),f).unwrap(),61002);assert_eq!(serde_json::to_vec(&b).unwrap(),original);
 }
}
#[test]fn mismatched_structured_codes_auth_and_possible_execution_remain_fatal(){
 for(status,code)in[(400,"INTERNAL_ERROR"),(401,"INTERNAL_ERROR"),(403,"RATE_LIMITED"),(500,"RATE_LIMITED"),(429,"INTERNAL_ERROR"),(503,"ORDER_STATUS_UNKNOWN"),(429,"AUTHENTICATION_REQUIRED"),(503,"UNKNOWN_NEW_CODE"),(401,"SERVICE_UNAVAILABLE"),(500,"SERVICE_UNAVAILABLE")]{let mut e=error("OFFICIAL_REQUEST_REJECTED",Some(status));e.api_code=Some(code.into());assert!(classified(e).downcast_ref::<TokenFailure>().is_none());}
 for(field,status,code)in[(0,500,"INTERNAL_ERROR"),(1,429,"RATE_LIMITED")]{let mut e=error("OFFICIAL_REQUEST_REJECTED",Some(status));e.api_code=Some(code.into());if field==0{e.may_have_executed=true}else{e.order_id=Some("123".into())};assert!(classified(e).downcast_ref::<TokenFailure>().is_none());}
}
#[test]fn mixed_token_and_native_failures_consume_one_shared_persisted_budget(){
 let mut b=stream_connect_retry::prepare_attempt(&RecoveryBudget::new(1000).unwrap(),1000,None).unwrap();
 let token=classified(error("CONNECTION_UNCONFIRMED",None));let token=token.downcast_ref::<TokenFailure>().unwrap();
 assert_eq!(after_token_failure(&b,2000,Some(1002),token).unwrap(),61002);assert_eq!(b.attempts(),&[1000]);
 b=stream_connect_retry::prepare_attempt(&b,61002,Some(1002)).unwrap();
 assert_eq!(stream_connect_retry::after_connect_failure(&b,62000,Some(61004),ConnectionFailure::from_connect_error("STREAM_DNS")).unwrap(),121004);
 b=stream_connect_retry::prepare_attempt(&b,121004,Some(61004)).unwrap();
 // A real restart reloads the SAME original attempts, without refunding failures.
 let original=serde_json::to_vec(&b).unwrap();let reloaded:RecoveryBudget=serde_json::from_slice(&original).unwrap();
 assert_eq!(after_token_failure(&reloaded,122000,Some(121006),token).unwrap(),601001);assert_eq!(serde_json::to_vec(&reloaded).unwrap(),original);assert_eq!(reloaded.attempts(),&[1000,61002,121004]);
}
#[test]fn retry_after_exceeds_mint_spacing_without_mutating_or_double_charging_budget(){
 let b=stream_connect_retry::prepare_attempt(&RecoveryBudget::new(0).unwrap(),0,None).unwrap();let saved=serde_json::to_vec(&b).unwrap();let mut e=error("OFFICIAL_REQUEST_REJECTED",Some(429));e.retry_after_ms=180000;
 let e=classified(e);assert_eq!(after_token_failure(&b,1000,Some(2),e.downcast_ref::<TokenFailure>().unwrap()).unwrap(),181000);assert_eq!(serde_json::to_vec(&b).unwrap(),saved);
 assert!(stream_connect_retry::prepare_attempt(&b,60001,Some(2)).is_err());
}
#[test]fn missing_charge_clock_regression_future_marker_and_cooldown_overflow_fail_closed(){
 let e=classified(error("CONNECTION_UNCONFIRMED",None));let f=e.downcast_ref::<TokenFailure>().unwrap();let b=RecoveryBudget::new(1000).unwrap();assert!(after_token_failure(&b,1000,Some(1000),f).is_err());
 let b=stream_connect_retry::prepare_attempt(&b,1000,None).unwrap();for(now,mint)in [(999,Some(1000)),(1000,Some(1001)),(1000,None),(1000,Some(999))]{assert!(after_token_failure(&b,now,mint,f).is_err());}
 let mut e=error("CONNECTION_UNCONFIRMED",None);e.retry_after_ms=u64::MAX;let e=classified(e);assert!(after_token_failure(&b,1000,Some(1000),e.downcast_ref::<TokenFailure>().unwrap()).is_err());
 assert!(after_token_failure(&b,MAXIMUM_CLOCK_MS+1,Some(1000),f).is_err());
}
