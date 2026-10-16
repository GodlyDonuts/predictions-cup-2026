//! Cooperatively service the original Session while one existing SDK GET blocks.
//!
//! The Transport is exclusively borrowed by one scoped I/O thread; the caller's
//! service closure stays on the original thread and must not use a Transport,
//! dispatch an order, or mint financial authority. No client/limiter is created.
//! Only preflight endpoints are allowed. Do not wrap owned-order/fill reads or a
//! write, or place this between a returned fill tape and its financial commit.
//!
//! A service error stops servicing, but the original logical GET is joined before
//! returning that SAME error. Existing SDK pagination/backoff cannot be cancelled
//! here; no caller may reuse the Transport or issue another request before join.
//! Receipt recording and thread-local caches belong to the caller after return.
use baal_transport_service::{self as sdk,Endpoint,Receipt,Transport};
use std::{error::Error,fmt,sync::mpsc::{self,Receiver,RecvTimeoutError},thread,time::Duration};

pub type Result<T> = std::result::Result<T,Box<dyn Error>>;
const SERVICE_INTERVAL:Duration=Duration::from_millis(25);

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum TaskFailureKind { Panicked, CompletionDisconnected, UnsupportedEndpoint }
/// Sanitized failure: no panic payload, response, authorization or credentials.
#[derive(Debug)]
pub struct TaskFailure { pub kind:TaskFailureKind, earlier_service_error:Option<Box<dyn Error>> }
impl fmt::Display for TaskFailure {
 fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result {f.write_str(match self.kind{TaskFailureKind::Panicked=>"SERVICED_READ_TASK_PANICKED",TaskFailureKind::CompletionDisconnected=>"SERVICED_READ_COMPLETION_DISCONNECTED",TaskFailureKind::UnsupportedEndpoint=>"SERVICED_READ_PREFLIGHT_ENDPOINT_ONLY"})}
}
impl Error for TaskFailure {fn source(&self)->Option<&(dyn Error+'static)>{self.earlier_service_error.as_deref()}}

fn wait<T>(received:&Receiver<T>,service:&mut impl FnMut()->Result<()>)->(Option<T>,Option<Box<dyn Error>>){
 let mut first_error=None;
 loop{match received.recv_timeout(SERVICE_INTERVAL){
  Ok(result)=>return(Some(result),first_error),
  Err(RecvTimeoutError::Disconnected)=>return(None,first_error),
  Err(RecvTimeoutError::Timeout)=>{
   // Check once more before doing work. Completion never invokes a trailing
   // callback; there is no callback after the caller receives this result.
   match received.try_recv(){Ok(result)=>return(Some(result),first_error),Err(mpsc::TryRecvError::Disconnected)=>return(None,first_error),Err(mpsc::TryRecvError::Empty)=>{}}
   if first_error.is_none(){if let Err(error)=service(){first_error=Some(error)}}
  }
 }}
}
fn scoped<T:Send>(task:impl FnOnce()->T+Send,mut service:impl FnMut()->Result<()>)->Result<T>{
 thread::scope(|scope|{
  let(send,receive)=mpsc::sync_channel(1);
  let task=scope.spawn(move||{let result=task();let _=send.send(result);});
  let(result,service_error)=wait(&receive,&mut service);
  // Always join, including service failure. A panic is fatal even if a prior
  // recoverable stream error exists; keep that error as diagnostic source.
  if task.join().is_err(){return Err(Box::new(TaskFailure{kind:TaskFailureKind::Panicked,earlier_service_error:service_error}) as Box<dyn Error>)}
  if let Some(error)=service_error{return Err(error)}
  result.ok_or_else(||Box::new(TaskFailure{kind:TaskFailureKind::CompletionDisconnected,earlier_service_error:None}) as Box<dyn Error>)
 })
}
fn preflight_endpoint(endpoint:&Endpoint)->bool{
 matches!(endpoint,Endpoint::Tournament|Endpoint::Positions|Endpoint::Account|Endpoint::Market{..}|Endpoint::Nodes{..}|Endpoint::OrderBook{..}|Endpoint::Prices{..})
 ||matches!(endpoint,Endpoint::Orders{status}if status=="open")
}
/// The inner SDK result is returned unchanged. The outer error is a service or
/// scoped-thread failure, which is fatal to this read path and is NOT an SDK
/// retry classification. Keep recording/validation/application on the caller.
pub fn request(transport:&mut Transport,endpoint:Endpoint,service:impl FnMut()->Result<()>)->Result<sdk::Result<Receipt>>{
 if !preflight_endpoint(&endpoint){return Err(Box::new(TaskFailure{kind:TaskFailureKind::UnsupportedEndpoint,earlier_service_error:None}))}
 scoped(move||transport.request(endpoint),service)
}

#[cfg(test)]mod tests{
 use super::*;
 use serde_json::json;
 use std::sync::{Arc,atomic::{AtomicBool,AtomicUsize,Ordering}};
 #[derive(Debug)]struct ServiceFailed;impl fmt::Display for ServiceFailed{fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{f.write_str("fixture service failure")}}impl Error for ServiceFailed{}
 fn receipt()->Receipt{Receipt{endpoint:"/fixture".into(),request_started_at:"2026-10-16T12:00:00.0000001Z".into(),received_at:"2026-10-16T12:00:00.1000002Z".into(),status:200,data:json!({"original":[1,2,3],"price":0.005})}}
 #[test]fn services_multiple_times_on_original_thread_until_exact_completion(){let original=thread::current().id();let(send,receive)=mpsc::sync_channel(1);let expected=serde_json::to_vec(&receipt()).unwrap();let mut calls=0;let result=scoped(move||{receive.recv().unwrap();receipt()},||{assert_eq!(thread::current().id(),original);calls+=1;if calls==3{send.send(()).unwrap()}Ok(())}).unwrap();assert!(calls>=3);assert_eq!(serde_json::to_vec(&result).unwrap(),expected);}
 #[test]fn queued_completion_has_no_extra_pump(){let(send,receive)=mpsc::sync_channel(1);send.send(77).unwrap();let mut calls=0;let(result,error)=wait(&receive,&mut||{calls+=1;Ok(())});assert_eq!(result,Some(77));assert!(error.is_none());assert_eq!(calls,0);}
 #[test]fn service_failure_joins_task_and_returns_same_typed_error_without_more_service(){let(send,receive)=mpsc::sync_channel(1);let finished=Arc::new(AtomicBool::new(false));let task_finished=finished.clone();let mut calls=0;let error=scoped(move||{receive.recv().unwrap();task_finished.store(true,Ordering::SeqCst);42},||{calls+=1;send.send(()).unwrap();Err(Box::new(ServiceFailed))}).unwrap_err();assert!(finished.load(Ordering::SeqCst));assert!(error.downcast_ref::<ServiceFailed>().is_some());assert_eq!(calls,1);}
 #[test]fn raw_sdk_rejection_is_not_rewritten_or_retried(){let count=Arc::new(AtomicUsize::new(0));let task_count=count.clone();let original=sdk::TransportError{code:"OFFICIAL_REQUEST_REJECTED".into(),status:Some(429),may_have_executed:false,retry_after_ms:1234,order_id:None,api_code:Some("RATE_LIMITED".into())};let expected=serde_json::to_vec(&original).unwrap();let result=scoped(move||{task_count.fetch_add(1,Ordering::SeqCst);std::result::Result::<Receipt,_>::Err(original)},||Ok(())).unwrap().unwrap_err();assert_eq!(serde_json::to_vec(&result).unwrap(),expected);assert_eq!(count.load(Ordering::SeqCst),1);}
 #[test]fn task_panic_is_sanitized_fatal_and_joined(){let error=scoped(||->u64{panic!("fixture payload must not become returned error")},||Ok(())).unwrap_err();assert_eq!(error.downcast_ref::<TaskFailure>().unwrap().kind,TaskFailureKind::Panicked);assert_eq!(error.to_string(),"SERVICED_READ_TASK_PANICKED");}
 #[test]fn endpoint_allowlist_excludes_fills_known_orders_trades_and_token_paths(){for endpoint in [Endpoint::OwnedFills{order_id:"991".into()},Endpoint::OwnedOrder{order_id:"991".into()},Endpoint::Trades{exchange_id:"101".into(),from:"a".into(),to:"b".into()},Endpoint::Orders{status:"closed".into()},Endpoint::Settlements,Endpoint::Collateral]{assert!(!preflight_endpoint(&endpoint))}for endpoint in [Endpoint::Tournament,Endpoint::Account,Endpoint::Positions,Endpoint::Orders{status:"open".into()},Endpoint::Market{market_id:"201".into()},Endpoint::Nodes{market_id:"201".into()},Endpoint::OrderBook{market_id:"201".into(),depth:20},Endpoint::Prices{exchange_ids:vec!["101".into()]}]{assert!(preflight_endpoint(&endpoint))}}
 #[cfg(unix)]#[test]fn actual_single_sdk_get_completes_queue_and_preserves_original_receipt(){
  use std::{fs,io::{Read,Write},net::TcpListener,os::unix::fs::PermissionsExt,time::{SystemTime,UNIX_EPOCH,Instant}};
  const T:&str="11111111-1111-4111-8111-111111111111";
  let root=std::env::temp_dir().join(format!("serviced-read-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir_all(&root).unwrap();fs::set_permissions(&root,fs::Permissions::from_mode(0o700)).unwrap();let key=root.join("dummy");fs::write(&key,b"ace_fixture_only_never_real").unwrap();fs::set_permissions(&key,fs::Permissions::from_mode(0o600)).unwrap();
  let listener=TcpListener::bind("127.0.0.1:0").unwrap();listener.set_nonblocking(true).unwrap();let base=format!("http://{}/api/v1",listener.local_addr().unwrap());let(send,receive)=mpsc::sync_channel(1);let gets=Arc::new(AtomicUsize::new(0));let observed_gets=gets.clone();let data=json!({"id":T,"slug":"cup","isPendingEnrolment":false,"status":"active","myBalance":1000});let expected=data.clone();
  let server=thread::spawn(move||{let deadline=Instant::now()+Duration::from_secs(5);let mut stream=loop{match listener.accept(){Ok((s,_))=>break s,Err(e)if e.kind()==std::io::ErrorKind::WouldBlock&&Instant::now()<deadline=>thread::sleep(Duration::from_millis(1)),Err(e)=>panic!("loopback accept: {e}")}};stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();let mut header=Vec::new();while !header.ends_with(b"\r\n\r\n"){let mut byte=[0];stream.read_exact(&mut byte).unwrap();header.push(byte[0]);assert!(header.len()<8192)}assert!(header.starts_with(b"GET /api/v1/tournaments/cup HTTP/1.1\r\n"));observed_gets.fetch_add(1,Ordering::SeqCst);receive.recv_timeout(Duration::from_secs(5)).unwrap();let bytes=serde_json::to_vec(&data).unwrap();write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len()).unwrap();stream.write_all(&bytes).unwrap();});
  let config=sdk::Config{queue_dir:root.join("queue"),account_scope:"offline-serviced-read".into(),scope:sdk::Scope{tournament_id:T.into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"201".into())].into()},maximum_pages:2,writes_enabled:false};let credentials=sdk::credentials::UnixFileCredentials::approved_file(key).unwrap();let mut transport=Transport::new_for_mock(config,Box::new(credentials),Box::new(sdk::SystemClock),Box::new(sdk::ReadOnlyAuthority),base).unwrap();let mut calls=0;let r=request(&mut transport,Endpoint::Tournament,||{calls+=1;if calls==3{send.send(()).unwrap()}Ok(())}).unwrap().unwrap();server.join().unwrap();assert!(calls>=3);assert_eq!(gets.load(Ordering::SeqCst),1);assert_eq!(r.endpoint,"/tournaments/cup");assert_eq!(r.status,200);assert_eq!(r.data,expected);assert!(chrono::DateTime::parse_from_rfc3339(&r.request_started_at).unwrap()<=chrono::DateTime::parse_from_rfc3339(&r.received_at).unwrap());let summary=transport.admission_summary().unwrap();assert!(!summary.has_unknown);assert!(summary.state.active.is_none());drop(transport);fs::remove_dir_all(root).unwrap();
 }
}
