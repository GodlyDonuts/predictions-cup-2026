//! Exact ordinary admission completed locally before the SDK network marker.
//! Original queue generations are retained; JSON alone never restores trust.
use crate::feedback::Result;
use baal_transport_service::{admission,RequestSpec,Permit,TransportError,Scope};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{fs,path::{Path,PathBuf},io::{BufRead,BufReader,Write}};
fn hash(v:&Value)->String{format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap()))}
fn context(scope:&Scope,spec:&RequestSpec,p:&Permit,at:u64,e:&TransportError)->Result<(Value,admission::ImmutableBinding)>{
 if spec.method!="POST"||spec.endpoint!="/orders"||e.may_have_executed||e.status.is_some()||e.order_id.is_some()||e.api_code.is_some()||e.retry_after_ms!=0||!matches!(e.code.as_str(),"PERMIT_EXPIRED_OR_INVALID"|"LIVE_CONTRACT_OR_BOOK_STALE"|"LIVE_ACCOUNT_STALE"|"LIVE_VERIFIED_STREAM_REQUIRED"|"RANK_PHYSICAL_FACTS_EXPIRED"|"FUNDAMENTAL_PHYSICAL_TOKEN_EXPIRED_OR_REGRESSED"){return Err("ORDINARY_PRE_NETWORK_ORIGINAL_ERROR".into())}
 let mut body=spec.body.clone().ok_or("ORDINARY_PRE_NETWORK_BODY")?;
 if body["tournamentId"]!=scope.tournament_id||!scope.exchanges.contains_key(body["exchangeId"].as_str().ok_or("ORDINARY_PRE_NETWORK_EXCHANGE")?){return Err("ORDINARY_PRE_NETWORK_SCOPE".into())}
 let raw=body["price"].as_str().map(str::to_owned).unwrap_or_else(||body["price"].to_string());let units=baal_transport_service::decimal_units(&raw)?;
 body["price"]=json!(serde_json::Number::from_string_unchecked(format!("{}.{:06}",units/1_000_000,units%1_000_000)));
 let actual=RequestSpec{method:"POST".into(),endpoint:"/orders".into(),body_json:Some(serde_json::to_string(&body)?),body:Some(body.clone())};
 let binding=admission::ImmutableBinding{intent:format!("{}:{}",p.run_id,p.intent_id),key:body["idempotencyKey"].as_str().ok_or("ORDINARY_PRE_NETWORK_KEY")?.into(),payload_hash:hash(&json!({"scope":scope,"spec":actual}))};
 Ok((json!({"spec":spec,"permit":p,"attemptAtMs":at,"error":e,"scope":scope}),binding))
}
pub struct Proof{context:Value,evidence:Value,_verified:admission::VerifiedLocalNotSentV1}
impl Proof{
 pub fn evidence(&self)->&Value{&self.evidence}
 pub fn validate_binding(&self,spec:&RequestSpec,p:&Permit,at:u64,e:&TransportError)->Result<()>{if self.context["spec"]!=serde_json::to_value(spec)?||self.context["permit"]!=serde_json::to_value(p)?||self.context["attemptAtMs"]!=at||self.context["error"]!=serde_json::to_value(e)?{return Err("ORDINARY_PRE_NETWORK_BINDING".into())}Ok(())}
}
fn local(s:&str)->bool{!s.is_empty()&&!s.contains(['/', '\\'])&&s!="."&&s!=".."}
fn selected(dir:&Path)->Result<Vec<String>>{let m:Value=serde_json::from_slice(&fs::read(dir.join("account-queue-manifest.json"))?)?;let j=m["journal"].as_str().ok_or("ORDINARY_PRE_NETWORK_MANIFEST")?;let c=m["checkpoint"].as_str().ok_or("ORDINARY_PRE_NETWORK_MANIFEST")?;if !local(j)||!local(c)||m["schema_version"]!=1{return Err("ORDINARY_PRE_NETWORK_SELECTION".into())}Ok(vec!["account-queue-manifest.json".into(),"account-queue-generation.guard".into(),j.into(),c.into()])}
fn request(dir:&Path,b:&admission::ImmutableBinding)->Result<String>{let names=selected(dir)?;let mut found=None;
 for line in BufReader::new(fs::File::open(dir.join(&names[2]))?).lines(){let v:Value=serde_json::from_str(&line?)?;let a=&v["state"]["active"];if a["binding"]==serde_json::to_value(b)?&&a["network_started"]==false{let s=a["request"].as_str().ok_or("ORDINARY_PRE_NETWORK_REQUEST")?.to_owned();if found.as_ref().is_some_and(|old|old!=&s){return Err("ORDINARY_PRE_NETWORK_MULTIPLE_REQUESTS".into())}found=Some(s);}}
 found.ok_or("ORDINARY_PRE_NETWORK_POSITIVE_REQUEST_REQUIRED".into())}
/// Caller holds the sole account-owner lease. No SDK commands or HTTP occur.
pub fn capture(queue:&Path,account:&str,scope:&Scope,spec:&RequestSpec,p:&Permit,at:u64,e:&TransportError)->Result<Proof>{
 let(ctx,b)=context(scope,spec,p,at,e)?;let root=queue.join(format!("ordinary-pre-network-{}",hash(&ctx)));
 if root.exists(){return restore(account,scope,spec,p,at,e,&json!({"snapshot":root}))}
 let req=request(queue,&b)?;let original=admission::verify_local_not_sent_v1(queue,account,&b,&req,at)?;
 fs::create_dir(&root)?;
 #[cfg(unix)]{use std::os::unix::fs::PermissionsExt;fs::set_permissions(&root,fs::Permissions::from_mode(0o700))?;}
 for name in selected(queue)?{let src=queue.join(&name);let before=fs::read(&src)?;let mut out=fs::OpenOptions::new().create_new(true).write(true).open(root.join(&name))?;out.write_all(&before)?;out.sync_all()?;if fs::read(&src)?!=before{return Err("ORDINARY_PRE_NETWORK_SOURCE_CHANGED".into())}}
 fs::File::open(&root)?.sync_all()?;
 let proof=restore(account,scope,spec,p,at,e,&json!({"snapshot":root}))?;
 if proof.evidence.get("sdk").map(hash).unwrap_or_else(||proof.evidence["sdkSha256"].as_str().unwrap_or("").into())!=hash(original.evidence()){return Err("ORDINARY_PRE_NETWORK_SOURCE_CHANGED".into())}Ok(proof)
}
pub fn restore(account:&str,scope:&Scope,spec:&RequestSpec,p:&Permit,at:u64,e:&TransportError,expected:&Value)->Result<Proof>{
 let(ctx,b)=context(scope,spec,p,at,e)?;let root=PathBuf::from(expected["snapshot"].as_str().ok_or("ORDINARY_PRE_NETWORK_SNAPSHOT")?);
 if !root.is_absolute()||root.file_name().and_then(|n|n.to_str())!=Some(format!("ordinary-pre-network-{}",hash(&ctx)).as_str()){return Err("ORDINARY_PRE_NETWORK_SNAPSHOT_BINDING".into())}
 let req=request(&root,&b)?;let verified=admission::verify_local_not_sent_v1(&root,account,&b,&req,at)?;
 // Original canonical states stay in the retained SDK generation. Store a
 // digest here, so repeated recoverable expiry cannot exhaust dispatch bounds.
 let evidence=if expected.get("sdk").is_some(){json!({"kind":"ordinary_admitted_pre_network_not_sent_v1","snapshot":root,"contextSha256":hash(&ctx),"sdk":verified.evidence()})}else{json!({"kind":"ordinary_admitted_pre_network_not_sent_v1","snapshot":root,"contextSha256":hash(&ctx),"sdkSha256":hash(verified.evidence()),"sourceJournalSha256":verified.evidence()["journalSha256"]})};
 if (expected.get("sdk").is_some()||expected.get("sdkSha256").is_some())&&*expected!=evidence{return Err("ORDINARY_PRE_NETWORK_RETAINED_CHANGED".into())}
 Ok(Proof{context:ctx,evidence,_verified:verified})
}
