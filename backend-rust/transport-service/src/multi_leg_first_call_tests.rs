use super::*;
use std::{fs,path::PathBuf,net::TcpListener,sync::{Arc,atomic::{AtomicBool,AtomicU64,Ordering}},thread,io::{Read,Write},time::{Duration,SystemTime,UNIX_EPOCH}};
use zeroize::Zeroizing;
fn temp()->PathBuf{let p=std::env::temp_dir().join(format!("transport-first-call-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&p).unwrap();p}
fn scope()->Scope{Scope{tournament_id:"11111111-1111-4111-8111-111111111111".into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"101".into()),("102".into(),"102".into())].into()}}
fn basket()->multi_leg::Basket{multi_leg::Basket{idempotency_key:"original-stable-key".into(),tournament_id:scope().tournament_id,maximum_cost:"0.900000".into(),relationship_constraint:None,legs:vec![multi_leg::Leg{exchange_id:"101".into(),side:multi_leg::Side::Yes,quantity:1,limit_price:"0.450000".into()},multi_leg::Leg{exchange_id:"102".into(),side:multi_leg::Side::No,quantity:1,limit_price:"0.450000".into()}]}}
fn permit()->Permit{Permit{run_id:"fixture-run".into(),intent_id:"fixture-package".into(),owner:"Codex".into(),expires_at_ms:1_000_000_000,maximum_quantity:2,maximum_cost:"0.900000".into()}}
struct FakeClock(Arc<AtomicU64>);impl Clock for FakeClock{fn now_ms(&self)->u64{self.0.load(Ordering::SeqCst)}fn sleep_ms(&mut self,n:u64)->Result<()>{self.0.fetch_add(n,Ordering::SeqCst);Ok(())}}
struct Auth{owner:Arc<AtomicBool>,deny_network:bool,deny_all:bool}
impl Authority for Auth{fn can_read(&self,_:&Scope)->bool{self.owner.load(Ordering::SeqCst)}fn authorize(&self,_:&Scope,_:&RequestSpec,_:&Permit,network:bool,_:u64)->Result<()>{need(!self.deny_all&&!(network&&self.deny_network),"FIXTURE_AUTHORITY_DENIED")}fn owns_order(&self,_:&str)->bool{false}}
struct Credentials{mode:u8,clock:Arc<AtomicU64>}
impl CredentialProvider for Credentials{fn load(&self)->Result<Zeroizing<String>>{match self.mode{1=>Err(fail("FIXTURE_CREDENTIAL_UNAVAILABLE")),2=>Ok(Zeroizing::new("invalid\nheader".into())),3=>{self.clock.store(999,Ordering::SeqCst);Err(fail("FIXTURE_CREDENTIAL_UNAVAILABLE"))},_=>Ok(Zeroizing::new("OFFLINE_FIXTURE_ONLY".into()))}}}
struct Server{base:String,count:Arc<AtomicU64>,alive:Arc<AtomicBool>,join:Option<thread::JoinHandle<()>>}
impl Server{fn new(status:u16,body:Value)->Self{let socket=TcpListener::bind("127.0.0.1:0").unwrap();socket.set_nonblocking(true).unwrap();let base=format!("http://{}/api/v1",socket.local_addr().unwrap());let count=Arc::new(AtomicU64::new(0));let counter=count.clone();let alive=Arc::new(AtomicBool::new(true));let run=alive.clone();let bytes=body.to_string();let join=thread::spawn(move||{while run.load(Ordering::SeqCst){let(mut stream,_)=match socket.accept(){Ok(v)=>v,Err(e)if e.kind()==std::io::ErrorKind::WouldBlock=>{thread::sleep(Duration::from_millis(2));continue},Err(_)=>break};stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();let mut raw=Vec::new();let mut part=[0u8;1024];let header_end=loop{let n=stream.read(&mut part).unwrap_or(0);if n==0{break None}raw.extend_from_slice(&part[..n]);if let Some(i)=raw.windows(4).position(|w|w==b"\r\n\r\n"){break Some(i+4)}};if let Some(end)=header_end{let head=String::from_utf8_lossy(&raw[..end]);let length=head.lines().find_map(|s|s.to_ascii_lowercase().strip_prefix("content-length:").and_then(|n|n.trim().parse::<usize>().ok())).unwrap_or(0);while raw.len()<end+length{let n=stream.read(&mut part).unwrap_or(0);if n==0{break}raw.extend_from_slice(&part[..n]);}counter.fetch_add(1,Ordering::SeqCst);let reply=format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{bytes}",bytes.len());let _=stream.write_all(reply.as_bytes());}}});Self{base,count,alive,join:Some(join)}}}
impl Drop for Server{fn drop(&mut self){self.alive.store(false,Ordering::SeqCst);if let Some(j)=self.join.take(){j.join().unwrap();}}}
fn transport(dir:&std::path::Path,server:&Server,network_deny:bool,credential_mode:u8)->(Transport,Arc<AtomicBool>){let owner=Arc::new(AtomicBool::new(true));let clock=Arc::new(AtomicU64::new(1000));let t=Transport::new_for_mock(Config{queue_dir:dir.into(),account_scope:"actual-fixture-account".into(),scope:scope(),maximum_pages:5,writes_enabled:true},Box::new(Credentials{mode:credential_mode,clock:clock.clone()}),Box::new(FakeClock(clock)),Box::new(Auth{owner:owner.clone(),deny_network:network_deny,deny_all:false}),server.base.clone()).unwrap();(t,owner)}

fn front_failure(t:&mut Transport)->TransportError{
 t.authority=Box::new(Auth{owner:Arc::new(AtomicBool::new(true)),deny_network:false,deny_all:true});
 t.place_all_or_none(basket(),permit()).err().expect("original guard failure")
}
fn binding_and_request()->(admission::ImmutableBinding,String){let spec=multi_leg::request_spec(&basket(),&scope(),&permit()).unwrap();(multi_leg_first_call::binding(&scope(),&spec,&basket(),&permit()).unwrap(),digest(&serde_json::to_vec(&spec).unwrap()))}
fn sources(p:&std::path::Path,part:&str)->Vec<PathBuf>{fs::read_dir(p).unwrap().map(|e|e.unwrap().path()).filter(|p|p.file_name().unwrap().to_string_lossy().starts_with(part)).collect()}
#[test]fn first_original_guard_error_is_durable_single_use_local_evidence_and_zero_http(){
 let p=temp();let server=Server::new(200,json!({}));{
  let(mut t,_)=transport(&p,&server,false,0);let error=front_failure(&mut t);let proof=t.take_first_call_no_http_proof().unwrap();let e=proof.evidence();
  assert_eq!(e["kind"],"sdk_before_first_admit_no_http");assert_eq!(e["terminal"]["originalError"],serde_json::to_value(error).unwrap());
  assert_eq!(e["terminal"]["admitInvocations"],0);assert_eq!(e["terminal"]["httpInvocations"],0);assert_eq!(e["terminal"]["queueHealth"],"not_established");assert!(e["terminal"].get("status").is_none());
  assert_eq!(e["ticket"]["accountScope"],"actual-fixture-account");assert_eq!(e["ticket"]["binding"],serde_json::to_value(binding_and_request().0).unwrap());
  assert!(t.take_first_call_no_http_proof().is_err());assert!(t.take_local_not_sent_proof().is_err());assert_eq!(server.count.load(Ordering::SeqCst),0);assert_eq!(t.admission_summary().unwrap().immutable_bindings,0);
 }fs::remove_dir_all(p).unwrap();
}
#[test]fn retained_original_recovery_requires_exact_scope_intent_payload_request_and_sources(){
 let p=temp();let server=Server::new(200,json!({}));let original;{
  let(mut t,_)=transport(&p,&server,false,0);front_failure(&mut t);original=t.take_first_call_no_http_proof().unwrap().evidence().clone();
 }{
  let(t,_)=transport(&p,&server,false,0);let(binding,request)=binding_and_request();assert_eq!(t.verify_retained_first_call_no_http(&binding,&request).unwrap().evidence(),&original);
  for change in 0..4{let mut b=binding.clone();let mut r=request.clone();match change{0=>b.intent.push('x'),1=>b.key.push('x'),2=>b.payload_hash="b".repeat(64),_=>r="b".repeat(64)}assert!(t.verify_retained_first_call_no_http(&b,&r).is_err());}
  let path=sources(&p,"first-call-terminal-").pop().unwrap();let bytes=fs::read(&path).unwrap();fs::write(&path,b"{").unwrap();assert!(t.verify_retained_first_call_no_http(&binding,&request).is_err());fs::write(&path,&bytes).unwrap();
  let path=sources(&p,"first-call-key-").pop().unwrap();fs::write(&path,b"{}").unwrap();assert!(t.verify_retained_first_call_no_http(&binding,&request).is_err());assert_eq!(server.count.load(Ordering::SeqCst),0);
 }fs::remove_dir_all(p).unwrap();
}
#[test]fn permanent_intent_and_key_indices_block_identical_and_rebound_invocations(){
 for change in 0..4{let p=temp();let server=Server::new(200,json!({}));{
  let(mut t,_)=transport(&p,&server,false,0);front_failure(&mut t);let mut b=basket();let mut permit=permit();match change{0=>{},1=>b.legs[0].limit_price="0.440000".into(),2=>permit.intent_id="different-intent".into(),_=>b.idempotency_key="different-key".into()}
  assert!(t.place_all_or_none(b,permit).is_err());assert!(t.poisoned);assert!(t.take_first_call_no_http_proof().is_err());assert_eq!(server.count.load(Ordering::SeqCst),0);
 }fs::remove_dir_all(p).unwrap();}
}
#[test]fn any_actual_admit_invocation_disqualifies_even_when_it_never_sends_http(){
 let p=temp();let server=Server::new(200,json!({}));{
  let(mut t,_)=transport(&p,&server,true,0);assert!(t.place_all_or_none(basket(),permit()).is_err());assert!(t.take_first_call_no_http_proof().is_err());assert!(t.take_local_not_sent_proof().is_ok());assert_eq!(server.count.load(Ordering::SeqCst),0);
 }fs::remove_dir_all(p).unwrap();
}
#[test]fn actual_http_outcomes_and_their_repeat_never_mint_first_no_http_proof(){
 for(status,body)in [(400,json!({"error":{"code":"VALIDATION_ERROR","message":"fixture","details":{"rejectionReasonCode":"AllOrNoneNotFilled","leg":0}}})),(500,json!({"error":{"code":"INTERNAL_ERROR","message":"fixture"}})),(200,json!({"results":[]}))]{let p=temp();let server=Server::new(status,body);{
  let(mut t,_)=transport(&p,&server,false,0);assert!(t.place_all_or_none(basket(),permit()).is_ok());assert!(t.take_first_call_no_http_proof().is_err());assert!(t.place_all_or_none(basket(),permit()).is_err());assert!(t.take_first_call_no_http_proof().is_err());assert_eq!(server.count.load(Ordering::SeqCst),1);
 }fs::remove_dir_all(p).unwrap();}
}
#[test]fn old_core_key_with_different_intent_is_positive_disqualification_not_absence(){
 let p=temp();let server=Server::new(200,json!({}));{
  let(mut t,_)=transport(&p,&server,false,0);
  let mut old=binding_and_request().0;old.intent="legacy:other-intent".into();
  let admitted=t.command(json!({"op":"admit","request":"old-call","write":true,"binding":old,"authorized":true,"lease_valid":true,"permit_valid":true})).unwrap();assert_eq!(admitted["admitted"],true);
  t.command(json!({"op":"complete","request":"old-call","outcome":"not_sent"})).unwrap();assert_eq!(t.queue.lookup_key(&old.key).unwrap(),Some(old));
  let error=t.place_all_or_none(basket(),permit()).err().unwrap();assert_eq!(error.code,"FIRST_CALL_PRIOR_BINDING");assert!(t.take_first_call_no_http_proof().is_err());assert_eq!(server.count.load(Ordering::SeqCst),0);
 }fs::remove_dir_all(p).unwrap();
}
#[test]fn legacy_identical_request_artifact_is_not_first_call_evidence(){
 let p=temp();let server=Server::new(200,json!({}));{
  let(mut t,_)=transport(&p,&server,false,0);let spec=multi_leg::request_spec(&basket(),&scope(),&permit()).unwrap();multi_leg::record_request(&p,&spec,&basket(),&permit()).unwrap();let error=t.place_all_or_none(basket(),permit()).err().unwrap();assert_eq!(error.code,"FIRST_CALL_PRIOR_REQUEST_ARTIFACT");assert!(t.take_first_call_no_http_proof().is_err());assert_eq!(server.count.load(Ordering::SeqCst),0);
 }fs::remove_dir_all(p).unwrap();
}
#[test]fn intervening_operation_invalidates_immediate_capability_but_not_original_recovery(){
 let p=temp();let server=Server::new(200,json!({}));{
  let(mut t,_)=transport(&p,&server,false,0);front_failure(&mut t);t.admission_state().unwrap();assert!(t.take_first_call_no_http_proof().is_err());let(b,r)=binding_and_request();assert!(t.verify_retained_first_call_no_http(&b,&r).is_ok());
 }fs::remove_dir_all(p).unwrap();
}
#[test]fn poisoned_lost_owner_or_nonquiet_controller_never_releases_original_proof(){
 for mode in 0..3{let p=temp();let server=Server::new(200,json!({}));{
  let(mut t,_)=transport(&p,&server,false,0);front_failure(&mut t);match mode{0=>t.poisoned=true,1=>t.authority=Box::new(Auth{owner:Arc::new(AtomicBool::new(false)),deny_network:false,deny_all:false}),_=>{fs::write(p.join("offline-account-queue.lock"),b"other-worker").unwrap();}}assert!(t.take_first_call_no_http_proof().is_err());let(b,r)=binding_and_request();assert!(t.verify_retained_first_call_no_http(&b,&r).is_err());assert_eq!(server.count.load(Ordering::SeqCst),0);
 }fs::remove_dir_all(p).unwrap();}
}
#[test]fn incomplete_ticket_pair_or_terminal_write_failure_poison_and_never_publish_capability(){
 for phase in 0..3{let p=temp();let server=Server::new(200,json!({}));{
  let(mut t,_)=transport(&p,&server,false,0);let spec=multi_leg::request_spec(&basket(),&scope(),&permit()).unwrap();let b=binding_and_request().0;
  if phase==0{let key_path=p.join(format!("first-call-key-{}.json",digest(&serde_json::to_vec(&json!(["actual-fixture-account",b.key])).unwrap())));fs::create_dir(key_path).unwrap();}
  if phase==1{let terminal_path=p.join(format!("first-call-terminal-{}.json",digest(&serde_json::to_vec(&json!(["actual-fixture-account",b.intent])).unwrap())));fs::create_dir(terminal_path).unwrap();}
  if phase==2{let path=p.join(format!("first-call-intent-{}.json",digest(&serde_json::to_vec(&json!(["actual-fixture-account",b.intent])).unwrap())));fs::write(path,b"{").unwrap();}
  let _=spec;assert!(!front_failure(&mut t).may_have_executed);assert!(t.poisoned);assert!(t.take_first_call_no_http_proof().is_err());assert_eq!(server.count.load(Ordering::SeqCst),0);
 }fs::remove_dir_all(p).unwrap();}
}
#[cfg(unix)]#[test]fn source_symlink_and_scope_substitution_are_rejected(){use std::os::unix::fs::symlink;
 let p=temp();let server=Server::new(200,json!({}));{
  let(mut t,_)=transport(&p,&server,false,0);front_failure(&mut t);let path=sources(&p,"first-call-terminal-").pop().unwrap();let other=p.join("retained-original");fs::rename(&path,&other).unwrap();symlink(&other,&path).unwrap();let(b,r)=binding_and_request();assert!(t.verify_retained_first_call_no_http(&b,&r).is_err());assert_eq!(server.count.load(Ordering::SeqCst),0);
 }fs::remove_dir_all(p).unwrap();
}

struct ArtifactFailure{path:PathBuf}
impl Authority for ArtifactFailure{fn can_read(&self,_:&Scope)->bool{true}fn owns_order(&self,_:&str)->bool{false}fn authorize(&self,_:&Scope,_:&RequestSpec,_:&Permit,_:bool,_:u64)->Result<()>{if !self.path.exists(){fs::create_dir(&self.path).unwrap();}Ok(())}}
#[test]fn original_request_record_io_error_before_admission_is_proven_without_faking_venue_receipt(){
 let p=temp();let server=Server::new(200,json!({}));{
  let(mut t,_)=transport(&p,&server,false,0);let spec=multi_leg::request_spec(&basket(),&scope(),&permit()).unwrap();let path=multi_leg_first_call::legacy_request_path(&p,&spec,&basket(),&permit()).unwrap();t.authority=Box::new(ArtifactFailure{path});
  let e=t.place_all_or_none(basket(),permit()).err().unwrap();assert_eq!(e.code,"BASKET_REQUEST_EVIDENCE_DURABILITY");let proof=t.take_first_call_no_http_proof().unwrap();assert_eq!(proof.evidence()["terminal"]["originalError"],serde_json::to_value(e).unwrap());assert_eq!(t.admission_summary().unwrap().sequence,0);assert_eq!(server.count.load(Ordering::SeqCst),0);
 }fs::remove_dir_all(p).unwrap();
}
#[test]fn actual_admit_error_is_disqualified_before_core_returns_error(){
 let p=temp();let server=Server::new(200,json!({}));{
  let(mut t,_)=transport(&p,&server,false,0);
  // The private stage is invalidated at the invocation boundary, including a
  // malformed core command rejected before any durable admission or HTTP.
  let spec=multi_leg::request_spec(&basket(),&scope(),&permit()).unwrap();let mut ticket=multi_leg_first_call::Ticket::begin(&p,"actual-fixture-account",&scope(),&basket(),&permit(),&spec,1000).unwrap();ticket.qualify();t.first_call_active=Some(ticket);
  assert!(t.command(json!({"op":"admit"})).is_err());assert!(!t.first_call_active.as_ref().unwrap().can_seal());assert_eq!(server.count.load(Ordering::SeqCst),0);assert!(sources(&p,"first-call-terminal-").is_empty());
 }fs::remove_dir_all(p).unwrap();
}
#[test]fn direct_private_queue_mutation_stales_immediate_ticket_capability(){
 let p=temp();let server=Server::new(200,json!({}));{
  let(mut t,_)=transport(&p,&server,false,0);front_failure(&mut t);t.queue.command(json!({"op":"status","scope":"actual-fixture-account","now":1000})).unwrap();assert!(t.take_first_call_no_http_proof().is_err());
 }fs::remove_dir_all(p).unwrap();
}

#[test]fn injected_original_fsync_errors_poison_without_publishing_any_capability(){
 for phase in 1..=3{let p=temp();let server=Server::new(200,json!({}));{
  let(mut t,_)=transport(&p,&server,false,0);multi_leg_first_call::fail_next_sync(phase);let e=front_failure(&mut t);assert!(!e.may_have_executed);assert!(t.poisoned);assert!(t.take_first_call_no_http_proof().is_err());assert!(t.package_queue_quiet().is_err());assert_eq!(server.count.load(Ordering::SeqCst),0);
 }fs::remove_dir_all(p).unwrap();}
}
#[test]fn genuine_success_ack_and_repeat_cannot_be_relabelled_local_no_http(){
 let body=json!({"results":[{"index":0,"data":{"orderId":"901","exchangeId":"101","side":"yes","action":"buy","open":false,"quantity":1,"quantityTraded":1,"remainingQuantity":0,"price":0.45,"fillPrice":0.45,"totalCost":0.45,"all":null}},{"index":1,"data":{"orderId":"902","exchangeId":"102","side":"no","action":"buy","open":false,"quantity":1,"quantityTraded":-1,"remainingQuantity":0,"price":0.45,"fillPrice":0.45,"totalCost":0.45,"all":null}}]});
 let p=temp();let server=Server::new(200,body);{
  let(mut t,_)=transport(&p,&server,false,0);assert!(matches!(t.place_all_or_none(basket(),permit()).unwrap(),multi_leg::Outcome::FullFillReported{..}));assert!(t.take_first_call_no_http_proof().is_err());assert!(t.place_all_or_none(basket(),permit()).is_err());assert!(t.take_first_call_no_http_proof().is_err());assert_eq!(server.count.load(Ordering::SeqCst),1);
 }fs::remove_dir_all(p).unwrap();
}
