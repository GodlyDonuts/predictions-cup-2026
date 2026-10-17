//! Ordinary POST final-local-guard evidence, distinct from never-admitted calls.
//! No network, queue command, unknown clearing or retry is performed here.
use crate::{admission,self as api,Clock,Config,RequestSpec,Permit,TransportError,Result,fail,need,digest};
use serde_json::{Value,json};
use std::{fs::{self,File,OpenOptions},io::{Read,Write},path::{Path,PathBuf}};
const LIMIT:u64=350*1024*1024;
pub const KIND:&str="sdk_ordinary_final_guard_no_http_v1";
const LEGACY_BINARY:&str="5ce153648fe20b6749af71ba4d6a3c88a6ed2b6b92ddc343ec488c0c2042b497";
const LEGACY_SOURCE:&str="022435b64328185b1b8959eca75458fa6dd776b6";
const LEGACY_TRANSPORT:&str="7b07027ac41d6e000170a19fe02673b26a183929b5d13a5830938bbd849fa463";
const LEGACY_AUTHORITY:&str="1e1bde3e7b7b5350fa2b5308fef4251432b5779700e9277b891efe60f87cb64b";
const LEGACY_WORKER:&str="dcb291427ec063940fffec0449e69cad8576ea5f1bbd77459453e44b0aaceed2";
const LEGACY_BUILD:&str="1c61a867fd441cd8309a640a812079168366aa39262eacade9fbd619da46bc80";
fn hash<T:serde::Serialize>(v:&T)->Result<String>{Ok(digest(&serde_json::to_vec(v).map_err(|_|fail("LOCAL_GUARD_JSON"))?))}
fn atom(s:&str)->bool{!s.is_empty()&&s.len()<=160&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-')}
fn read(path:&Path,expected:Option<&str>)->Result<Vec<u8>>{
 for p in path.ancestors(){need(!fs::symlink_metadata(p).map_err(|_|fail("LOCAL_GUARD_SOURCE"))?.file_type().is_symlink(),"LOCAL_GUARD_SYMLINK")?;}
 let mut f=File::open(path).map_err(|_|fail("LOCAL_GUARD_SOURCE"))?;let before=f.metadata().map_err(|_|fail("LOCAL_GUARD_SOURCE"))?;need(before.is_file()&&before.len()<=LIMIT,"LOCAL_GUARD_SOURCE_BOUND")?;
 #[cfg(unix)]{use std::os::unix::fs::MetadataExt;need(before.nlink()==1,"LOCAL_GUARD_SOURCE_LINK")?;}
 let mut raw=Vec::new();Read::by_ref(&mut f).take(LIMIT+1).read_to_end(&mut raw).map_err(|_|fail("LOCAL_GUARD_SOURCE"))?;let after=fs::metadata(path).map_err(|_|fail("LOCAL_GUARD_SOURCE"))?;
 need(raw.len()as u64==before.len()&&before.len()==after.len(),"LOCAL_GUARD_SOURCE_CHANGED")?;
 #[cfg(unix)]{use std::os::unix::fs::MetadataExt;need(before.dev()==after.dev()&&before.ino()==after.ino()&&before.mtime()==after.mtime()&&before.mtime_nsec()==after.mtime_nsec()&&before.ctime()==after.ctime()&&before.ctime_nsec()==after.ctime_nsec(),"LOCAL_GUARD_SOURCE_CHANGED")?;}
 if let Some(expected)=expected{need(digest(&raw)==expected,"LOCAL_GUARD_SOURCE_HASH")?;}Ok(raw)
}
fn write_new(path:&Path,value:&Value)->Result<String>{
 let raw=serde_json::to_vec(value).map_err(|_|fail("LOCAL_GUARD_JSON"))?;need(raw.len()as u64<=LIMIT,"LOCAL_GUARD_SOURCE_BOUND")?;let mut o=OpenOptions::new();o.write(true).create_new(true);
 #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;o.mode(0o600);}
 let mut f=o.open(path).map_err(|_|fail("LOCAL_GUARD_EVIDENCE_DURABILITY"))?;f.write_all(&raw).and_then(|_|f.sync_all()).map_err(|_|fail("LOCAL_GUARD_EVIDENCE_DURABILITY"))?;
 #[cfg(unix)]File::open(path.parent().ok_or_else(||fail("LOCAL_GUARD_PATH"))?).and_then(|f|f.sync_all()).map_err(|_|fail("LOCAL_GUARD_EVIDENCE_DURABILITY"))?;Ok(digest(&raw))
}
fn artifact(config:&Config,name:&str)->Result<PathBuf>{need(config.queue_dir.is_absolute()&&atom(name),"LOCAL_GUARD_ARTIFACT")?;Ok(config.queue_dir.join(format!("ordinary-local-guard-{name}.json")))}
pub(crate) fn binding(config:&Config,spec:&RequestSpec,p:&Permit)->Result<admission::ImmutableBinding>{
 need(spec.method=="POST"&&spec.endpoint=="/orders"&&p.owner=="Codex"&&!p.run_id.is_empty()&&!p.intent_id.is_empty(),"LOCAL_GUARD_ORDINARY_BINDING")?;
 let b=spec.body.as_ref().ok_or_else(||fail("LOCAL_GUARD_BODY"))?;let key=b["idempotencyKey"].as_str().filter(|s|!s.is_empty()&&s.len()<=255&&!s.chars().any(char::is_control)).ok_or_else(||fail("LOCAL_GUARD_KEY"))?;
 need(b["tournamentId"]==config.scope.tournament_id&&b["exchangeId"].as_str().is_some_and(|ex|config.scope.exchanges.contains_key(ex))&&spec.body_json.as_ref().is_some_and(|s|serde_json::from_str::<Value>(s).ok().as_ref()==Some(b)),"LOCAL_GUARD_SCOPE_OR_BODY")?;
 Ok(admission::ImmutableBinding{intent:format!("{}:{}",p.run_id,p.intent_id),key:key.into(),payload_hash:hash(&json!({"scope":config.scope,"spec":spec}))?})
}
pub(crate) struct Pending{pub request:String,pub minimum_clock:u64,path:PathBuf,sha:String}
/// Written BEFORE known-local completion. Failure leaves the original marked
/// request unknown; no proof can be constructed from an uncommitted certificate.
pub(crate) fn record_guard(config:&Config,spec:&RequestSpec,p:&Permit,request:&str,minimum_clock:u64,error:&TransportError)->Result<Pending>{
 need(atom(request)&&!error.may_have_executed&&error.status.is_none()&&error.order_id.is_none()&&error.api_code.is_none(),"LOCAL_GUARD_ORIGINAL_ERROR")?;
 let path=artifact(config,&format!("certificate-{request}"))?;let value=json!({"kind":KIND,"queueScope":config.account_scope,"scope":config.scope,"request":request,"minimumClock":minimum_clock,"originalSpec":spec,"originalPermit":p,"originalError":error,"binding":binding(config,spec,p)?});let sha=write_new(&path,&value)?;Ok(Pending{request:request.into(),minimum_clock,path,sha})
}
/// No Deserialize/public constructor. File pins and typed original SDK replay
/// are rechecked; the capability grants no queue or physical order authority.
pub struct OrdinaryLocalGuardNoHttpProof{context:Value,source:PathBuf,source_sha:String,approval:Option<(PathBuf,String)>,dispatch:Option<(PathBuf,String)>}
impl OrdinaryLocalGuardNoHttpProof{
 pub fn evidence(&self)->Result<Value>{read(&self.source,Some(&self.source_sha))?;for(p,s)in self.approval.iter().chain(self.dispatch.iter()){read(p,Some(s))?;}Ok(self.context.clone())}
 pub fn validate_binding(&self,spec:&RequestSpec,p:&Permit,attempt_at:u64,error:&TransportError)->Result<()>{
  self.evidence()?;need(self.context["originalSpec"]==serde_json::to_value(spec).map_err(|_|fail("LOCAL_GUARD_JSON"))?&&self.context["originalPermit"]==serde_json::to_value(p).map_err(|_|fail("LOCAL_GUARD_JSON"))?&&self.context["attemptAt"]==attempt_at&&self.context["originalError"]==serde_json::to_value(error).map_err(|_|fail("LOCAL_GUARD_JSON"))?,"LOCAL_GUARD_ORIGINAL_BINDING_CHANGED")
 }
}
fn proof(config:&Config,spec:&RequestSpec,p:&Permit,at:u64,error:&TransportError,request:&str,snapshot:Value,mode:&str,approval:Option<(PathBuf,String)>)->Result<OrdinaryLocalGuardNoHttpProof>{
 let b=binding(config,spec,p)?;let legacy=mode=="legacy_owner_forensic";let head=admission::verify_local_guard_snapshot_v1(&config.account_scope,&b,request,at,legacy,&snapshot).map_err(|e|fail(&e))?;
 let source=artifact(config,&format!("source-{request}"))?;let source_value=json!({"kind":KIND,"proofMode":mode,"originalSDKSource":snapshot});let source_sha=if source.exists(){let raw=read(&source,None)?;need(serde_json::from_slice::<Value>(&raw).map_err(|_|fail("LOCAL_GUARD_SOURCE_JSON"))?==source_value,"LOCAL_GUARD_EXISTING_SOURCE_CHANGED")?;digest(&raw)}else{write_new(&source,&source_value)?};
 let mut context=json!({"kind":KIND,"proofMode":mode,"originalSpec":spec,"originalPermit":p,"originalError":error,"attemptAt":at,"binding":b,"queueScope":config.account_scope,"request":request,"sdkSourcePath":source,"sdkSourceSha256":source_sha,"verifiedSDKHead":head,"ownerApprovalPath":approval.as_ref().map(|a|&a.0),"ownerApprovalSha256":approval.as_ref().map(|a|&a.1),"executionAuthority":false,"retryPermitted":false});context["contextSha256"]=json!(hash(&context)?);
 let dispatch=approval.as_ref().map(|(p,s)|->Result<_>{let a:Value=serde_json::from_slice(&read(p,Some(s))?).map_err(|_|fail("LOCAL_GUARD_APPROVAL"))?;Ok((PathBuf::from(a["dispatchPath"].as_str().ok_or_else(||fail("LOCAL_GUARD_DISPATCH_PATH"))?),a["dispatchSha256"].as_str().ok_or_else(||fail("LOCAL_GUARD_DISPATCH_HASH"))?.to_owned()))}).transpose()?;
 Ok(OrdinaryLocalGuardNoHttpProof{context,source,source_sha,approval,dispatch})
}
pub(crate) fn current(config:&Config,queue:&admission::AccountAdmission,pending:Pending,spec:&RequestSpec,p:&Permit,at:u64,error:&TransportError)->Result<OrdinaryLocalGuardNoHttpProof>{
 let raw=read(&pending.path,Some(&pending.sha))?;let cert:Value=serde_json::from_slice(&raw).map_err(|_|fail("LOCAL_GUARD_CERTIFICATE"))?;
 need(at>0&&at<=pending.minimum_clock&&cert["originalSpec"]==serde_json::to_value(spec).map_err(|_|fail("LOCAL_GUARD_JSON"))?&&cert["originalPermit"]==serde_json::to_value(p).map_err(|_|fail("LOCAL_GUARD_JSON"))?&&cert["originalError"]==serde_json::to_value(error).map_err(|_|fail("LOCAL_GUARD_JSON"))?,"LOCAL_GUARD_CERTIFICATE_BINDING")?;
 let mut source=queue.capture_current_local_guard_source(&binding(config,spec,p)?,&pending.request,at).map_err(|e|fail(&e))?;let mut snapshot=source.snapshot().clone();snapshot["localCertificate"]=json!(String::from_utf8(raw).map_err(|_|fail("LOCAL_GUARD_UTF8"))?);source.unchanged().map_err(|e|fail(&e))?;proof(config,spec,p,at,error,&pending.request,snapshot,"durable_local_guard",None)
}
/// Owner forensic approval is externally SHA-pinned, and restricted to the ONE
/// audited installed call graph. This is not a general error-string whitelist.
fn legacy_approval(config:&Config,spec:&RequestSpec,p:&Permit,at:u64,error:&TransportError,path:&Path,sha:&str,snapshot:Option<&Value>)->Result<Value>{
 let raw=read(path,Some(sha))?;need(raw.len()<=65536,"LOCAL_GUARD_APPROVAL_BOUND")?;let a:Value=serde_json::from_slice(&raw).map_err(|_|fail("LOCAL_GUARD_OWNER_APPROVAL"))?;
 need(error.code=="LIVE_CONTRACT_OR_BOOK_STALE"&&error.may_have_executed&&error.status.is_none()&&error.order_id.is_none()&&error.api_code.is_none()
  &&a["kind"]=="owner_approved_legacy_final_guard_no_http_v1"&&a["sourceCommit"]==LEGACY_SOURCE&&a["binarySha256"]==LEGACY_BINARY&&a["buildManifestSha256"]==LEGACY_BUILD&&a["transportSourceSha256"]==LEGACY_TRANSPORT&&a["authoritySourceSha256"]==LEGACY_AUTHORITY&&a["workerSourceSha256"]==LEGACY_WORKER
  &&a["queueScope"]==config.account_scope&&a["originalSpecSha256"]==hash(spec)?&&a["originalPermitSha256"]==hash(p)?&&a["originalErrorSha256"]==hash(error)?&&a["attemptAt"]==at,"LOCAL_GUARD_LEGACY_APPROVAL_BINDING")?;
 let request=a["request"].as_str().filter(|s|atom(s)).ok_or_else(||fail("LOCAL_GUARD_APPROVAL_REQUEST"))?;
 let dp=read(Path::new(a["dispatchPath"].as_str().ok_or_else(||fail("LOCAL_GUARD_DISPATCH_PATH"))?),Some(a["dispatchSha256"].as_str().ok_or_else(||fail("LOCAL_GUARD_DISPATCH_HASH"))?))?;
 need(dp.ends_with(b"\n"),"LOCAL_GUARD_DISPATCH_TORN")?;let mut sequence=0u64;let mut previous="0".repeat(64);let mut events=Vec::new();let qid=p.intent_id.parse::<u64>().map_err(|_|fail("LOCAL_GUARD_DISPATCH_QID"))?;
 for raw in dp.split_inclusive(|b|*b==b'\n'){let mut row:Value=serde_json::from_slice(raw).map_err(|_|fail("LOCAL_GUARD_DISPATCH_JSON"))?;let claimed=row.as_object_mut().ok_or_else(||fail("LOCAL_GUARD_DISPATCH_JSON"))?.remove("hash").ok_or_else(||fail("LOCAL_GUARD_DISPATCH_HASH"))?;
  need(row["sequence"]==sequence+1&&row["previous"]==previous&&claimed==hash(&row)?,"LOCAL_GUARD_DISPATCH_CHAIN")?;sequence+=1;previous=claimed.as_str().ok_or_else(||fail("LOCAL_GUARD_DISPATCH_HASH"))?.into();if row["event"]["qid"]==qid{events.push((sequence,row["event"].clone()));}
 }
 need(events.len()==3&&events[0].0+1==events[1].0&&events[1].0+1==events[2].0&&a["dispatchIntentSequence"].as_u64()==Some(events[0].0)&&a["dispatchAttemptSequence"].as_u64()==Some(events[1].0)&&a["dispatchErrorSequence"].as_u64()==Some(events[2].0),"LOCAL_GUARD_DISPATCH_ORIGINAL_TRIPLE")?;
 need(events[0].1["kind"]=="intent"&&events[0].1["spec"]==serde_json::to_value(spec).map_err(|_|fail("LOCAL_GUARD_JSON"))?&&events[0].1["permit"]==serde_json::to_value(p).map_err(|_|fail("LOCAL_GUARD_JSON"))?&&events[1].1["kind"]=="attempt"&&events[1].1["originalQuoteSha256"]==hash(&events[0].1["quote"])?&&crate::parse_clock(&events[1].1["at"]).is_some_and(|t|t==at)&&events[2].1["kind"]=="error"&&events[2].1["error"]==serde_json::to_value(error).map_err(|_|fail("LOCAL_GUARD_JSON"))?,"LOCAL_GUARD_DISPATCH_ORIGINALS")?;
 if let Some(snapshot)=snapshot{
  for(key,field)in [("sdkManifestSha256","manifest"),("sdkCheckpointSha256","checkpoint"),("sdkJournalSha256","journal")]{need(a[key]==digest(snapshot[field].as_str().ok_or_else(||fail("LOCAL_GUARD_LEGACY_SOURCE_REQUIRED"))?.as_bytes()),"LOCAL_GUARD_LEGACY_SOURCE_HASH")?;}
  let head=admission::verify_local_guard_snapshot_v1(&config.account_scope,&binding(config,spec,p)?,request,at,true,snapshot).map_err(|e|fail(&e))?;let approved=a["approvedAtMs"].as_u64().ok_or_else(||fail("LOCAL_GUARD_APPROVAL_CLOCK"))?;need(approved>=head["lastClock"].as_u64().ok_or_else(||fail("LOCAL_GUARD_APPROVAL_CLOCK"))?&&approved<=api::SystemClock.now_ms()&&crate::parse_clock(&events[2].1["at"]).is_some_and(|t|head["completionAt"].as_u64().is_some_and(|done|done<=t)),"LOCAL_GUARD_APPROVAL_CLOCK")?;
  let next=head["witness"]["completionNext"].as_u64().ok_or_else(||fail("LOCAL_GUARD_COMPLETION_CLOCK"))?;let done=head["completionAt"].as_u64().unwrap();let error_at=crate::parse_clock(&events[2].1["at"]).unwrap();need(error.retry_after_ms<=next.saturating_sub(done)&&error.retry_after_ms>=next.saturating_sub(error_at),"LOCAL_GUARD_ORIGINAL_RETRY_CLOCK")?;
 }Ok(a)
}
pub fn verify_legacy_ordinary_local_guard_no_http(config:&Config,spec:&RequestSpec,p:&Permit,at:u64,error:&TransportError,path:&Path,sha:&str)->Result<OrdinaryLocalGuardNoHttpProof>{
 let a=legacy_approval(config,spec,p,at,error,path,sha,None)?;let request=a["request"].as_str().unwrap();let mut source=admission::capture_local_guard_source(&config.queue_dir,&config.account_scope,&binding(config,spec,p)?,request,at,true).map_err(|e|fail(&e))?;let snapshot=source.snapshot().clone();legacy_approval(config,spec,p,at,error,path,sha,Some(&snapshot))?;source.unchanged().map_err(|e|fail(&e))?;proof(config,spec,p,at,error,request,snapshot,"legacy_owner_forensic",Some((path.into(),sha.into())))
}
pub fn verify_retained_ordinary_local_guard_no_http(config:&Config,spec:&RequestSpec,p:&Permit,at:u64,error:&TransportError,context:&Value)->Result<OrdinaryLocalGuardNoHttpProof>{
 let mut original=context.clone();let sha=original.as_object_mut().ok_or_else(||fail("LOCAL_GUARD_CONTEXT"))?.remove("contextSha256");need(sha.as_ref().and_then(Value::as_str)==Some(hash(&original)?.as_str())&&context["kind"]==KIND&&context["queueScope"]==config.account_scope,"LOCAL_GUARD_CONTEXT_HASH")?;
 let legacy=context["proofMode"]=="legacy_owner_forensic";need(legacy||context["proofMode"]=="durable_local_guard","LOCAL_GUARD_MODE")?;let request=context["request"].as_str().ok_or_else(||fail("LOCAL_GUARD_REQUEST"))?;let path=artifact(config,&format!("source-{request}"))?;need(context["sdkSourcePath"]==path.to_string_lossy().as_ref(),"LOCAL_GUARD_SOURCE_PATH")?;
 let raw=read(&path,Some(context["sdkSourceSha256"].as_str().ok_or_else(||fail("LOCAL_GUARD_SOURCE_HASH"))?))?;let value:Value=serde_json::from_slice(&raw).map_err(|_|fail("LOCAL_GUARD_SOURCE_JSON"))?;let snapshot=&value["originalSDKSource"];
 let approval=if legacy{let approval_path=PathBuf::from(context["ownerApprovalPath"].as_str().ok_or_else(||fail("LOCAL_GUARD_APPROVAL"))?);let approval_sha=context["ownerApprovalSha256"].as_str().ok_or_else(||fail("LOCAL_GUARD_APPROVAL"))?;legacy_approval(config,spec,p,at,error,&approval_path,approval_sha,Some(snapshot))?;Some((approval_path,approval_sha.into()))}else{None};
 if !legacy{let cert:Value=serde_json::from_str(snapshot["localCertificate"].as_str().ok_or_else(||fail("LOCAL_GUARD_CERTIFICATE"))?).map_err(|_|fail("LOCAL_GUARD_CERTIFICATE"))?;need(cert["kind"]==KIND&&cert["queueScope"]==config.account_scope&&cert["originalSpec"]==context["originalSpec"]&&cert["originalPermit"]==context["originalPermit"]&&cert["originalError"]==context["originalError"]&&cert["binding"]==context["binding"]&&cert["request"]==request&&cert["minimumClock"].as_u64().is_some_and(|t|t>=at),"LOCAL_GUARD_CERTIFICATE_BINDING")?;}
 let head=admission::verify_local_guard_snapshot_v1(&config.account_scope,&binding(config,spec,p)?,request,at,legacy,snapshot).map_err(|e|fail(&e))?;need(context["verifiedSDKHead"]==head&&context["binding"]==serde_json::to_value(binding(config,spec,p)?).map_err(|_|fail("LOCAL_GUARD_JSON"))?,"LOCAL_GUARD_CONTEXT_CHANGED")?;
 let dispatch=approval.as_ref().map(|(p,s)|->Result<_>{let a:Value=serde_json::from_slice(&read(p,Some(s))?).map_err(|_|fail("LOCAL_GUARD_APPROVAL"))?;Ok((PathBuf::from(a["dispatchPath"].as_str().ok_or_else(||fail("LOCAL_GUARD_DISPATCH_PATH"))?),a["dispatchSha256"].as_str().ok_or_else(||fail("LOCAL_GUARD_DISPATCH_HASH"))?.to_owned()))}).transpose()?;
 let result=OrdinaryLocalGuardNoHttpProof{context:context.clone(),source:path,source_sha:digest(&raw),approval,dispatch};result.validate_binding(spec,p,at,error)?;Ok(result)
}
