//! Separate package dispatch journal, not an ordinary-order ACK projection.
//! This stores caller-validated admission/evidence; it creates no account or
//! execution authority. A crash after attempt is unresolved even before HTTP.
//! Hash chains detect damage, not a hostile rewrite/re-hash of the whole file.
use baal_transport_service::{Scope,Permit,RequestSpec,Receipt,TransportError,multi_leg::{self,Basket,ResponseEvidence}};
use moloch_replay::session::ExclusiveLock;
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{collections::BTreeMap,fs::{self,File,OpenOptions},io::{BufRead,BufReader,Write},path::{Path,PathBuf},sync::atomic::{AtomicU64,Ordering}};
pub type Result<T> = std::result::Result<T,Box<dyn std::error::Error>>;
const MAX_ROW:usize=8*1024*1024;
static NONCE:AtomicU64=AtomicU64::new(1);
fn hash<T:Serialize>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn need(ok:bool,why:&'static str)->Result<()>{if ok{Ok(())}else{Err(why.into())}}
fn clock(s:&str)->Result<i64>{Ok(chrono::DateTime::parse_from_rfc3339(s)?.timestamp_millis())}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn sync_directory(path:&Path)->Result<()>{#[cfg(unix)]File::open(path)?.sync_all()?;#[cfg(not(unix))]let _=path;Ok(())}
/// Construct using the private Admission accessors. It is an immutable durable
/// description, not a reconstituted scanner certificate or dispatch permission.
#[derive(Clone,Serialize)]
pub struct DurableIntent{proof:Value,reserve:Value,basket:Basket,scope:Scope,permit:Permit,queue_scope:String,spec:RequestSpec,request_hash:String,queue_binding:Value}
impl DurableIntent{
 pub fn new(proof:Value,reserve:Value,basket:Basket,scope:Scope,permit:Permit,queue_scope:&str)->Result<Self>{
  let digest=hash(&proof)?;let id=format!("pkg-{digest}");
  need(reserve["type"]=="live_package_reserve"&&reserve["source"]=="moloch_live_dispatcher"&&reserve["proofHash"]==digest&&reserve["packageId"]==id&&permit.intent_id==id,"PACKAGE_INTENT_PROOF")?;
  need(proof["runId"]==permit.run_id&&proof["bankManifestHash"]==reserve["bankScope"]["bankManifestHash"]&&proof["tournamentId"]==scope.tournament_id&&reserve["bankScope"]["runId"]==permit.run_id&&reserve["bankScope"]["tournamentId"]==scope.tournament_id&&hex(reserve["bankScope"]["bankManifestHash"].as_str().ok_or("PACKAGE_INTENT_MANIFEST")?),"PACKAGE_INTENT_SCOPE")?;
  need(!queue_scope.is_empty()&&queue_scope.len()<=512&&!queue_scope.chars().any(char::is_control)&&!permit.run_id.is_empty()&&!permit.owner.is_empty(),"PACKAGE_INTENT_OWNER")?;
  need(basket.idempotency_key==format!("{}:{id}",permit.run_id)&&basket.maximum_cost==reserve["economics"]["maximumCost"]&&proof["maximumCost"]==basket.maximum_cost&&proof["usesFutureCollateral"]==false,"PACKAGE_INTENT_ECONOMICS")?;
  let created=clock(reserve["receivedAt"].as_str().ok_or("PACKAGE_INTENT_CLOCK")?)?;let expires=clock(reserve["expiresAt"].as_str().ok_or("PACKAGE_INTENT_CLOCK")?)?;
  need(created>=0&&expires>created&&proof["atMs"].as_i64()==Some(created)&&proof["expiresAtMs"].as_i64()==Some(expires)&&permit.expires_at_ms==expires as u64,"PACKAGE_INTENT_CLOCK")?;
  need(proof["wireLegs"]==reserve["legs"],"PACKAGE_INTENT_LEGS")?;
  let legs=reserve["legs"].as_array().ok_or("PACKAGE_INTENT_LEGS")?;need(legs.len()==basket.legs.len(),"PACKAGE_INTENT_LEGS")?;
  for (l,w) in legs.iter().zip(&basket.legs){need(l["exchangeId"]==w.exchange_id&&l["marketId"].as_str()==scope.exchanges.get(&w.exchange_id).map(String::as_str)&&l["side"]==(if w.side==multi_leg::Side::Yes{"YES"}else{"NO"})&&l["quantity"]==w.quantity&&l["limitPrice"]==w.limit_price&&hex(l["bookHash"].as_str().ok_or("PACKAGE_INTENT_BOOK")?),"PACKAGE_INTENT_LEGS")?;}
  let spec=multi_leg::request_spec(&basket,&scope,&permit)?;let request_hash=hash(&spec)?;
  let queue_binding=json!({"intent":format!("{}:{}",permit.run_id,permit.intent_id),"key":basket.idempotency_key,"payload_hash":hash(&json!({"scope":scope,"spec":spec}))?});
  Ok(Self{proof,reserve,basket,scope,permit,queue_scope:queue_scope.into(),spec,request_hash,queue_binding})
 }
 pub fn package_id(&self)->&str{self.permit.intent_id.as_str()}
 pub fn request_spec(&self)->&RequestSpec{&self.spec}
 pub fn request_hash(&self)->&str{&self.request_hash}
 pub fn queue_binding(&self)->&Value{&self.queue_binding}
 pub fn basket(&self)->&Basket{&self.basket}
 fn restore(v:&Value)->Result<Self>{
  let restored=Self::new(v["proof"].clone(),v["reserve"].clone(),serde_json::from_value(v["basket"].clone())?,serde_json::from_value(v["scope"].clone())?,serde_json::from_value(v["permit"].clone())?,v["queue_scope"].as_str().ok_or("PACKAGE_INTENT_QUEUE_SCOPE")?)?;
  need(serde_json::to_value(&restored)?==*v,"PACKAGE_INTENT_CHANGED")?;Ok(restored)
 }
}
/// Token cannot be deserialized, cloned or minted by a caller. On restart there
/// is no capability to turn an earlier attempt into a local not-sent result.
pub struct AttemptToken{nonce:u64,package_id:String,attempt_head:String}
/// Minted only by a fresh read of this held original journal. There is no
/// Deserialize, Clone, public constructor or evidence-selected source path.
pub(crate) struct VerifiedNeverAttempted{evidence:Value}
impl VerifiedNeverAttempted{pub(crate) fn evidence(&self)->&Value{&self.evidence}}
#[derive(Clone)]
struct Record{intent:DurableIntent,phase:String,attempted_at:Option<String>,outcome:Option<Value>}
#[derive(Clone,Default)]struct Projection{records:BTreeMap<String,Record>,sequence:u64,previous:String,last_clock:Option<i64>}
/// Queue evidence remains a trusted caller attestation of the original local
/// SDK completion. Empty status/lookup alone never proves no physical send.
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueNotSentAttestation{pub queue_scope:String,pub observed_at:String,pub request:String,pub binding:Value,pub state:Value,pub completion:Value}
pub struct PackageDispatchLog{file:File,path:PathBuf,directory:PathBuf,_lock:ExclusiveLock,state:Projection,nonce:u64,poisoned:bool,#[cfg(test)]fail_after_write:bool}
fn response_evidence(v:&Value)->Result<ResponseEvidence>{
 Ok(ResponseEvidence{private_body_path:serde_json::from_value(v["privateBodyPath"].clone())?,private_metadata_path:serde_json::from_value(v["privateMetadataPath"].clone())?,body_sha256:v["bodySha256"].as_str().ok_or("PACKAGE_EVIDENCE_HASH")?.into(),request_sha256:v["requestSha256"].as_str().ok_or("PACKAGE_EVIDENCE_REQUEST")?.into(),status:u16::try_from(v["status"].as_u64().ok_or("PACKAGE_EVIDENCE_STATUS")?)?,request_started_at:v["requestStartedAt"].as_str().ok_or("PACKAGE_EVIDENCE_CLOCK")?.into(),received_at:v["receivedAt"].as_str().ok_or("PACKAGE_EVIDENCE_CLOCK")?.into(),complete_body:v["completeBody"].as_bool().ok_or("PACKAGE_EVIDENCE_COMPLETE")?})
}
fn validate_zero(intent:&DurableIntent,e:&ResponseEvidence)->Result<Value>{
 need(e.request_sha256==intent.request_hash&&e.complete_body,"PACKAGE_ZERO_REQUEST")?;
 need(fs::metadata(&e.private_metadata_path)?.len()<=MAX_ROW as u64,"PACKAGE_ZERO_METADATA_SIZE")?;
 let meta:Value=serde_json::from_slice(&fs::read(&e.private_metadata_path)?)?;
 need(meta["evidence"]==serde_json::to_value(e)?&&meta["request"]==serde_json::to_value(&intent.spec)?,"PACKAGE_ZERO_METADATA")?;
 let response=multi_leg::evidenced_response(e)?;
 need(multi_leg::zero_execution_reason(e.status,&response,intent.basket.legs.len()).is_some(),"PACKAGE_ZERO_UNKNOWN")?;Ok(response)
}
// Replay validates binding/integrity only. Original authenticity is supplied
// at append by the SDK-owned opaque Proof; restart must reverify that source.
pub(crate) fn validate_first_call(admission:&Value,error:&Value,proof:&Value,attempted:i64,at:i64)->Result<()>{
 need(attempted>=0&&at>=attempted&&proof["kind"]=="sdk_before_first_admit_no_http","PACKAGE_FIRST_CALL_KIND_OR_CLOCK")?;
 let t=&proof["ticket"];let k=&proof["keyIndex"];let end=&proof["terminal"];
 for v in [t,k,end]{let mut body=v.clone();let claimed=body.as_object_mut().ok_or("PACKAGE_FIRST_CALL_OBJECT")?.remove("sha256").ok_or("PACKAGE_FIRST_CALL_HASH")?;need(claimed==hash(&body)?,"PACKAGE_FIRST_CALL_HASH")?;}
 need(t["schemaVersion"]==1&&t["kind"]=="sdk_first_logical_invocation"&&t["accountScope"]==admission["queue_scope"]&&t["binding"]==admission["queue_binding"]&&t["scope"]==admission["scope"]&&t["basket"]==admission["basket"]&&t["permit"]==admission["permit"]&&t["request"]==admission["spec"]&&t["requestSha256"]==admission["request_hash"],"PACKAGE_FIRST_CALL_BINDING")?;
 need(k["schemaVersion"]==1&&k["kind"]=="sdk_first_logical_key"&&k["accountScope"]==t["accountScope"]&&k["key"]==t["binding"]["key"]&&k["intent"]==t["binding"]["intent"]&&k["ticketSha256"]==t["sha256"],"PACKAGE_FIRST_CALL_KEY")?;
 need(end["schemaVersion"]==1&&end["kind"]=="sdk_before_first_admit_no_http"&&end["accountScope"]==t["accountScope"]&&end["binding"]==t["binding"]&&end["requestSha256"]==t["requestSha256"]&&end["ticketSha256"]==t["sha256"]&&end["keyIndexSha256"]==k["sha256"]&&end["invocationId"]==t["invocationId"]&&end["startedAtMs"]==t["createdAtMs"]&&end["stage"]=="before_first_admit_invocation"&&end["admitInvocations"]==0&&end["httpInvocations"]==0&&end["queueHealth"]=="not_established"&&end["originalError"]==*error,"PACKAGE_FIRST_CALL_TERMINAL")?;
 need(error["mayHaveExecuted"]==false&&error.get("status")==Some(&Value::Null)&&error.get("orderId")==Some(&Value::Null)&&error.get("apiCode")==Some(&Value::Null)&&error["code"].as_str().is_some_and(|s|!s.is_empty())&&error["retryAfterMs"].as_u64().is_some(),"PACKAGE_FIRST_CALL_ERROR")?;
 let start=t["createdAtMs"].as_i64().ok_or("PACKAGE_FIRST_CALL_CLOCK")?;let complete=end["completedAtMs"].as_i64().ok_or("PACKAGE_FIRST_CALL_CLOCK")?;
 need(start>=attempted&&complete>=start&&complete<=at,"PACKAGE_FIRST_CALL_CLOCK")
}
impl Projection{
 fn initial()->Self{Self{previous:"0".repeat(64),..Self::default()}}
 fn unresolved(&self)->bool{self.records.values().any(|r|matches!(r.phase.as_str(),"attempt"|"unknown"))}
 fn apply(&mut self,e:&Value)->Result<()>{
  let id=e["packageId"].as_str().ok_or("PACKAGE_LOG_ID")?;let kind=e["kind"].as_str().ok_or("PACKAGE_LOG_KIND")?;
  let at=clock(e["at"].as_str().ok_or("PACKAGE_LOG_CLOCK")?)?;
  need(self.last_clock.is_none_or(|last|at>=last),"PACKAGE_LOG_CLOCK_REGRESSION")?;
  self.last_clock=Some(at);
  if kind=="intent"{
   need(!self.records.contains_key(id)&&!self.unresolved(),"PACKAGE_LOG_DUPLICATE_OR_UNRESOLVED")?;
   let i=DurableIntent::restore(&e["admission"])?;need(i.package_id()==id,"PACKAGE_LOG_ID")?;
   need(self.records.values().all(|r|r.intent.basket.idempotency_key!=i.basket.idempotency_key),"PACKAGE_LOG_KEY_REUSE")?;
   need(at>=clock(i.reserve["receivedAt"].as_str().ok_or("PACKAGE_INTENT_CLOCK")?)?,"PACKAGE_LOG_CLOCK")?;
   self.records.insert(id.into(),Record{intent:i,phase:"intent".into(),attempted_at:None,outcome:None});return Ok(())
  }
  let unresolved=self.unresolved();
  let r=self.records.get_mut(id).ok_or("PACKAGE_LOG_UNKNOWN_ID")?;
  need(e["requestHash"]==r.intent.request_hash,"PACKAGE_LOG_REQUEST_CHANGED")?;
  let started=r.attempted_at.as_deref().map(clock).transpose()?;
  if let Some(t)=started{need(at>=t,"PACKAGE_LOG_CLOCK")?;}
  match kind{
   "attempt"=>{need(!unresolved&&r.phase=="intent"&&at<r.intent.permit.expires_at_ms as i64,"PACKAGE_LOG_REPEAT_EXPIRED_OR_UNRESOLVED")?;r.phase="attempt".into();r.attempted_at=Some(e["at"].as_str().unwrap().into());},
   "never_attempted"=>{
    need(r.phase=="intent"&&r.attempted_at.is_none()&&r.outcome.is_none(),"PACKAGE_LOG_NEVER_AFTER_ATTEMPT")?;
    need(e.as_object().is_some_and(|v|v.len()==6)&&e["admissionHash"]==hash(&serde_json::to_value(&r.intent)?)?&&e["priorHead"]==json!({"sequence":self.sequence,"hash":self.previous}),"PACKAGE_LOG_NEVER_BINDING")?;
    r.phase="never_attempted".into();r.outcome=Some(e.clone());
   },
   "unknown"=>{need(matches!(r.phase.as_str(),"attempt"|"unknown"),"PACKAGE_LOG_UNKNOWN_PHASE")?;need(e["error"]["mayHaveExecuted"]==true,"PACKAGE_LOG_UNKNOWN_ERROR")?;r.phase="unknown".into();r.outcome=Some(e.clone());},
   "ack"=>{need(matches!(r.phase.as_str(),"attempt"|"unknown"),"PACKAGE_LOG_ACK_PHASE")?;let receipt:Receipt=serde_json::from_value(e["receipt"].clone())?;multi_leg::reported_fill(&receipt,&r.intent.basket)?;need(clock(&receipt.request_started_at)? >= started.ok_or("PACKAGE_LOG_NO_ATTEMPT")?&&clock(&receipt.received_at)?<=at,"PACKAGE_LOG_ACK_CLOCK")?;r.phase="ack".into();r.outcome=Some(e.clone());},
   "definitive_zero"=>{need(matches!(r.phase.as_str(),"attempt"|"unknown"),"PACKAGE_LOG_ZERO_PHASE")?;let evidence=response_evidence(&e["evidence"])?;let response=validate_zero(&r.intent,&evidence)?;need(e["response"]==response&&clock(&evidence.request_started_at)? >= started.ok_or("PACKAGE_LOG_NO_ATTEMPT")?&&clock(&evidence.request_started_at)?<=clock(&evidence.received_at)?&&clock(&evidence.received_at)?<=at,"PACKAGE_LOG_ZERO_CLOCK_OR_CHANGED")?;r.phase="definitive_zero".into();r.outcome=Some(e.clone());},
   "first_call_not_sent"=>{
    need(r.phase=="attempt","PACKAGE_LOG_FIRST_CALL_PHASE")?;
    validate_first_call(&serde_json::to_value(&r.intent)?,&e["error"],&e["firstCallEvidence"],started.ok_or("PACKAGE_LOG_NO_ATTEMPT")?,at)?;
    r.phase="first_call_not_sent".into();r.outcome=Some(e.clone());
   },
   "not_sent"=>{need(r.phase=="attempt"&&e["error"]["mayHaveExecuted"]==false&&e["error"].get("status")==Some(&Value::Null)&&e["error"].get("orderId")==Some(&Value::Null)&&e["error"]["code"].as_str().is_some_and(|s|!s.is_empty()),"PACKAGE_LOG_NOT_SENT_ERROR")?;
    let q:QueueNotSentAttestation=serde_json::from_value(e["queueEvidence"].clone())?;
    need(q.queue_scope==r.intent.queue_scope&&q.binding==r.intent.queue_binding&&!q.request.is_empty()&&clock(&q.observed_at)? >= started.ok_or("PACKAGE_LOG_NO_ATTEMPT")?&&clock(&q.observed_at)?<=at,"PACKAGE_LOG_QUEUE_BINDING")?;
    need(q.state["scope"]==q.queue_scope&&q.state["unknown"].as_object().is_some_and(|m|m.is_empty())&&q.state.get("active")==Some(&Value::Null)&&q.state["manual_review"]==false&&q.completion["request"]==q.request&&q.completion["binding"]==q.binding&&q.completion["outcome"]=="not_sent"&&q.completion["networkStarted"]==false,"PACKAGE_LOG_NOT_SENT_PROOF")?;
    r.phase="not_sent".into();r.outcome=Some(e.clone());},
   _=>return Err("PACKAGE_LOG_NO_MANUAL_COMPLETION".into())
  }Ok(())
 }
}
// Bounded line reads: an oversized/torn row is never allocated in full.
fn read_line(reader:&mut BufReader<File>)->Result<Option<Vec<u8>>>{let mut out=Vec::new();loop{let buf=reader.fill_buf()?;if buf.is_empty(){return if out.is_empty(){Ok(None)}else{Err("PACKAGE_LOG_TORN_TAIL".into())}}let end=buf.iter().position(|b|*b==b'\n');let n=end.map_or(buf.len(),|p|p+1);need(out.len().saturating_add(n)<=MAX_ROW,"PACKAGE_LOG_ROW_SIZE")?;out.extend_from_slice(&buf[..n]);reader.consume(n);if end.is_some(){return Ok(Some(out))}}}
struct Scan{state:Projection,fence:Option<Value>,bytes:u64,sha256:String}
fn scan(path:&Path,target:Option<&str>)->Result<Scan>{
 let mut state=Projection::initial();let mut reader=BufReader::new(File::open(path)?);let mut digest=Sha256::new();let mut total=0u64;let mut fence=None;
 while let Some(bytes)=read_line(&mut reader)?{
  digest.update(&bytes);total=total.checked_add(bytes.len() as u64).ok_or("PACKAGE_LOG_SIZE")?;
  let mut row:Value=serde_json::from_slice(&bytes)?;let claimed=row.as_object_mut().ok_or("PACKAGE_LOG_ROW")?.remove("hash").ok_or("PACKAGE_LOG_HASH")?;
  need(row["schemaVersion"]==1&&row["sequence"]==state.sequence+1&&row["previous"]==state.previous&&claimed==hash(&row)?,"PACKAGE_LOG_CHAIN")?;
  state.apply(&row["event"])?;state.sequence+=1;state.previous=claimed.as_str().ok_or("PACKAGE_LOG_HASH")?.into();
  if target.is_some_and(|id|row["event"]["packageId"]==id&&row["event"]["kind"]=="never_attempted"){
   need(fence.is_none(),"PACKAGE_LOG_REPEAT_NEVER_FENCE")?;row["hash"]=claimed;fence=Some(row);
  }
 }
 Ok(Scan{state,fence,bytes:total,sha256:format!("{:x}",digest.finalize())})
}
fn load(path:&Path)->Result<Projection>{Ok(scan(path,None)?.state)}
impl PackageDispatchLog{
 pub fn descriptor_identity(&self)->Result<Value>{
  #[cfg(unix)]{use std::os::unix::fs::MetadataExt;let m=self.file.metadata()?;Ok(json!({"schemaVersion":1,"device":m.dev(),"inode":m.ino(),"bytes":m.len(),"sequence":self.state.sequence,"previous":self.state.previous,"poisoned":self.poisoned}))}
  #[cfg(not(unix))]{Err("PACKAGE_LOG_DESCRIPTOR_IDENTITY_UNSUPPORTED".into())}
 }
 pub fn open(directory:&Path)->Result<Self>{
  fs::create_dir_all(directory)?;let lock=ExclusiveLock::acquire(&directory.join("package-dispatch.lock"))?;
  let path=directory.join("package-dispatch.ndjson");let mut options=OpenOptions::new();options.read(true).append(true).create(true);#[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
  let file=options.open(&path)?;file.sync_all()?;sync_directory(directory)?;let state=load(&path)?;
  Ok(Self{file,path,directory:directory.into(),_lock:lock,state,nonce:NONCE.fetch_add(1,Ordering::Relaxed),poisoned:false,#[cfg(test)]fail_after_write:false})
 }
 fn append(&mut self,event:Value)->Result<()>{
  need(!self.poisoned,"PACKAGE_LOG_POISONED")?;
  // Detect external truncation, mutation and appended rows before each write.
  let disk=match load(&self.path){Ok(v)=>v,Err(e)=>{self.poisoned=true;return Err(e)}};
  if disk.sequence!=self.state.sequence||disk.previous!=self.state.previous{self.poisoned=true;return Err("PACKAGE_LOG_CHANGED".into())}
  let mut candidate=self.state.clone();candidate.apply(&event)?;
  let mut row=json!({"schemaVersion":1,"sequence":self.state.sequence+1,"previous":self.state.previous,"event":event});let digest=hash(&row)?;row["hash"]=json!(digest);let mut bytes=serde_json::to_vec(&row)?;bytes.push(b'\n');need(bytes.len()<=MAX_ROW,"PACKAGE_LOG_ROW_SIZE")?;
  if let Err(error)=(||->Result<()>{self.file.write_all(&bytes)?;#[cfg(test)]if self.fail_after_write{return Err("PACKAGE_LOG_INJECTED_SYNC_FAILURE".into())}self.file.sync_all()?;sync_directory(&self.directory)?;let written=load(&self.path)?;need(written.sequence==self.state.sequence+1&&written.previous==digest,"PACKAGE_LOG_WRITE_TARGET_CHANGED")?;Ok(())})(){self.poisoned=true;return Err(error)}
  candidate.sequence+=1;candidate.previous=digest;self.state=candidate;Ok(())
 }
 pub fn intent(&mut self,intent:DurableIntent,at:&str)->Result<()>{self.append(json!({"kind":"intent","packageId":intent.package_id(),"at":at,"admission":intent}))}
 pub fn attempt(&mut self,id:&str,at:&str)->Result<AttemptToken>{let request=self.state.records.get(id).ok_or("PACKAGE_LOG_UNKNOWN_ID")?.intent.request_hash.clone();self.append(json!({"kind":"attempt","packageId":id,"at":at,"requestHash":request}))?;Ok(AttemptToken{nonce:self.nonce,package_id:id.into(),attempt_head:self.state.previous.clone()})}
 /// Irrevocable local boundary BEFORE any physical-attempt capability exists.
 /// Already fenced records reverify without appending, making a crash between
 /// this durable fence and native release safely recoverable on startup.
 pub fn abandon_before_attempt(&mut self,id:&str,at:&str)->Result<()>{
  self.pin_original_descriptor()?;
  let record=self.verified_record(id)?;
  if record["phase"]=="never_attempted"{
   need(clock(at)? >= clock(record["outcome"]["at"].as_str().ok_or("PACKAGE_LOG_CLOCK")?)?,"PACKAGE_LOG_CLOCK_REGRESSION")?;
   self.verified_never_attempted(id)?;return Ok(())
  }
  need(record["phase"]=="intent"&&record.get("attemptedAt")==Some(&Value::Null)&&record.get("outcome")==Some(&Value::Null),"PACKAGE_LOG_NEVER_AFTER_ATTEMPT")?;
  let admission=self.state.records.get(id).ok_or("PACKAGE_LOG_UNKNOWN_ID")?.intent.clone();
  self.append(json!({"kind":"never_attempted","packageId":id,"at":at,"requestHash":admission.request_hash,"admissionHash":hash(&serde_json::to_value(&admission)?)?,"priorHead":{"sequence":self.state.sequence,"hash":self.state.previous}}))?;
  self.verified_never_attempted(id)?;Ok(())
 }
 fn pin_original_descriptor(&self)->Result<()>{
  need(!self.poisoned,"PACKAGE_LOG_POISONED")?;
  #[cfg(unix)]{use std::os::unix::fs::MetadataExt;let held=self.file.metadata()?;let named=fs::metadata(&self.path)?;need(held.dev()==named.dev()&&held.ino()==named.ino(),"PACKAGE_LOG_DESCRIPTOR_CHANGED")?;Ok(())}
  #[cfg(not(unix))]{Err("PACKAGE_LOG_DESCRIPTOR_IDENTITY_UNSUPPORTED".into())}
 }
 pub(crate) fn verified_never_attempted(&self,id:&str)->Result<VerifiedNeverAttempted>{
  self.pin_original_descriptor()?;let original=scan(&self.path,Some(id))?;self.pin_original_descriptor()?;
  need(original.state.sequence==self.state.sequence&&original.state.previous==self.state.previous&&original.bytes==self.file.metadata()?.len(),"PACKAGE_LOG_CHANGED")?;
  let r=original.state.records.get(id).ok_or("PACKAGE_LOG_UNKNOWN_ID")?;
  need(r.phase=="never_attempted"&&r.attempted_at.is_none()&&r.outcome.as_ref().is_some_and(|e|e["kind"]=="never_attempted"),"PACKAGE_LOG_NEVER_FENCE_REQUIRED")?;
  let record=json!({"admission":r.intent,"phase":r.phase,"attemptedAt":r.attempted_at,"outcome":r.outcome});
  Ok(VerifiedNeverAttempted{evidence:json!({"dispatchRecordHash":hash(&record)?,"dispatchRecord":record,"dispatchFenceRow":original.fence.ok_or("PACKAGE_LOG_NEVER_FENCE_REQUIRED")?,"dispatchHead":self.head(),"journalBytes":original.bytes,"journalSha256":original.sha256,"journalLastClockMs":original.state.last_clock.ok_or("PACKAGE_LOG_CLOCK")?})})
 }
 fn outcome(&mut self,id:&str,kind:&str,at:&str,mut extra:Value)->Result<()>{let request=&self.state.records.get(id).ok_or("PACKAGE_LOG_UNKNOWN_ID")?.intent.request_hash;extra["kind"]=json!(kind);extra["packageId"]=json!(id);extra["at"]=json!(at);extra["requestHash"]=json!(request);self.append(extra)}
 pub fn acknowledged(&mut self,id:&str,r:&Receipt,at:&str)->Result<()>{self.outcome(id,"ack",at,json!({"receipt":r}))}
 pub fn definitive_zero(&mut self,id:&str,e:&ResponseEvidence,at:&str)->Result<()>{let intent=&self.state.records.get(id).ok_or("PACKAGE_LOG_UNKNOWN_ID")?.intent;let data=validate_zero(intent,e)?;self.outcome(id,"definitive_zero",at,json!({"evidence":e,"response":data}))}
 pub fn unknown(&mut self,id:&str,error:&TransportError,at:&str)->Result<()>{self.outcome(id,"unknown",at,json!({"error":error}))}
 pub fn not_sent(&mut self,token:AttemptToken,error:&TransportError,queue:&QueueNotSentAttestation,at:&str)->Result<()>{need(token.nonce==self.nonce&&token.attempt_head==self.state.previous,"PACKAGE_LOG_LOCAL_TOKEN")?;self.outcome(&token.package_id,"not_sent",at,json!({"error":error,"queueEvidence":queue}))}
 /// Distinct SDK pre-first-admission outcome. Only the original in-process
 /// single-use attempt token plus genuine SDK opaque proof can append this.
 pub fn first_call_not_sent(&mut self,token:AttemptToken,error:&TransportError,proof:&baal_transport_service::multi_leg_first_call::Proof,at:&str)->Result<()>{
  self.pin_original_descriptor()?;
  need(token.nonce==self.nonce&&token.attempt_head==self.state.previous,"PACKAGE_LOG_LOCAL_TOKEN")?;
  self.outcome(&token.package_id,"first_call_not_sent",at,json!({"error":error,"firstCallEvidence":proof.evidence()}))
 }
 /// Physical acceptance only: an ACK resolves this journal, while the native
 /// package reservation separately blocks until actual fills/terminal/complete.
 pub fn unresolved(&self)->bool{self.poisoned||self.state.unresolved()}
 pub fn record(&self,id:&str)->Option<Value>{self.state.records.get(id).map(|r|json!({"admission":r.intent,"phase":r.phase,"attemptedAt":r.attempted_at,"outcome":r.outcome}))}
 /// Reverify the original retained journal before constructing source-bound
 /// financial feedback. This is read-only and never repairs a damaged prefix.
 pub fn verified_record(&self,id:&str)->Result<Value>{
  need(!self.poisoned,"PACKAGE_LOG_POISONED")?;let disk=load(&self.path)?;
  need(disk.sequence==self.state.sequence&&disk.previous==self.state.previous,"PACKAGE_LOG_CHANGED")?;
  let r=disk.records.get(id).ok_or("PACKAGE_LOG_UNKNOWN_ID")?;
  Ok(json!({"admission":r.intent,"phase":r.phase,"attemptedAt":r.attempted_at,"outcome":r.outcome}))
 }
 pub(crate) fn verified_first_call_record(&self,id:&str)->Result<Value>{
  self.pin_original_descriptor()?;let record=self.verified_record(id)?;self.pin_original_descriptor()?;
  need(record["phase"]=="first_call_not_sent","PACKAGE_LOG_FIRST_CALL_PHASE")?;Ok(record)
 }
 pub fn records(&self)->Vec<Value>{self.state.records.keys().filter_map(|id|self.record(id)).collect()}
 pub fn head(&self)->Value{json!({"sequence":self.state.sequence,"hash":self.state.previous,"poisoned":self.poisoned})}
 /// Test-only durability fault after complete write but before sync/commit.
 #[cfg(test)]pub fn inject_sync_failure(&mut self){self.fail_after_write=true;}
}

