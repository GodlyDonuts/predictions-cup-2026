//! Optional proposal-only Paper 5 adapter for the existing sole worker. Source
//! files are read/hash-checked at setup; evaluation uses the actual Session,
//! original dispatch barrier and already-retained official node receipts. No
//! network client, fsync, journal replay, posterior fitting or physical permit.
use crate::{dispatch::DispatchLog,election_curve::{self,FitPolicy,HedgeLimits},execution_router::{self,Policy as RoutePolicy},feedback::Result,fundamental_policy::{ReviewAuthority,ReviewFile,ReviewInput,SourceBytes,SourceFile},source_innovation::Signal,value_surface::{BoundSurface,MappingReview,PreparedReview,Surface,sha256}};
use baal_transport_service::{Receipt,Scope};
use moloch_replay::session::Session;
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::{BTreeMap,BTreeSet},fs::{self,File},io::Read,path::{Path,PathBuf}};
fn need(b:bool,e:&'static str)->Result<()>{if b{Ok(())}else{Err(e.into())}}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct BindingFiles{pub review:ReviewFile,pub mapping:SourceFile}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct RouterSettings{pub strategy_sha256:String,pub order_lifetime_ms:u64,pub model_uncertainty_per_share:i128,pub terminal_tail_charge_per_share:i128,pub write_charge_per_order:i128,pub capital_charge_per_million_unit_ms:i128}
/// Must come from the parent's independently pinned Control. Artifact/mapping
/// allowlists are external approvals, never values copied from the artifact.
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Settings{
 pub version:u32,pub artifact:SourceFile,pub approved_artifact_sha256:BTreeSet<String>,
 pub authority_path:PathBuf,pub authority_sha256:String,pub approved_mapping_sha256:BTreeSet<String>,
 pub bindings:Vec<BindingFiles>,pub router:RouterSettings,pub curve:Option<FitPolicy>,pub hedges:Option<HedgeLimits>,
 pub allow_research_diagnostics:bool,pub maximum_proposals:usize,pub maximum_semantic_receipt_age_ms:u64,
 /// Explicit diagnostic work bounds, not live trading limits. Exceeding them
 /// defers this optional module; it never changes account/native funding caps.
 pub maximum_dispatch_records:usize,pub maximum_bank_sequence:u64,
}
fn read_original(path:&Path,expected:&str,maximum:usize,total:&mut usize)->Result<Vec<u8>>{
 need(path.is_absolute(),"P5_ABSOLUTE_SOURCE_PATH")?;let path_meta=fs::symlink_metadata(path)?;need(path_meta.is_file()&&!path_meta.file_type().is_symlink(),"P5_REGULAR_SOURCE")?;let mut file=File::open(path)?;let meta=file.metadata()?;need(meta.len()<=maximum as u64,"P5_SOURCE_SIZE")?;
 #[cfg(unix)]{use std::os::unix::fs::MetadataExt;need(path_meta.dev()==meta.dev()&&path_meta.ino()==meta.ino(),"P5_SOURCE_REPLACED")?;}
 let mut bytes=Vec::new();(&mut file).take(maximum as u64+1).read_to_end(&mut bytes)?;need(!bytes.is_empty()&&bytes.len()<=maximum&&sha256(&bytes)==expected,"P5_ORIGINAL_SOURCE_HASH")?;
 let after=fs::symlink_metadata(path)?;need(after.is_file()&&!after.file_type().is_symlink()&&after.len()==meta.len()&&after.modified().ok()==meta.modified().ok(),"P5_SOURCE_CHANGED")?;
 #[cfg(unix)]{use std::os::unix::fs::MetadataExt;need(after.dev()==meta.dev()&&after.ino()==meta.ino(),"P5_SOURCE_REPLACED")?;}
 *total=total.checked_add(bytes.len()).ok_or("P5_SOURCE_SIZE")?;need(*total<=32*1024*1024,"P5_TOTAL_SOURCE_SIZE")?;Ok(bytes)
}
pub struct Runtime{settings:Settings,surface:Surface,reviews:Vec<PreparedReview>,source_hashes:BTreeSet<String>}
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Diagnostic{
 pub state:&'static str,pub at_ms:u64,pub source_sha256:String,pub model_sha256:String,
 pub qualification:Value,pub bank_head:Value,pub deferred:Vec<Value>,pub routing:Vec<Value>,pub curve:Option<Value>,pub hedge_ideas:Vec<Value>,pub source_freshness:Vec<Value>,
 pub execution_authority:bool,pub financial_state_changed:bool,pub source_files_read_during_evaluation:u32,
}
impl Runtime{
 /// Loading an old artifact validates it at its ORIGINAL as-of for provenance.
 /// This does not renew it: evaluate checks original observed/expiry clocks.
 pub fn load(settings:&Settings)->Result<Self>{
  need(settings.version==1&&(1..=16).contains(&settings.bindings.len())&&(1..=32).contains(&settings.maximum_proposals)&&(1..=86400000).contains(&settings.maximum_semantic_receipt_age_ms)&&(1..=20000).contains(&settings.maximum_dispatch_records)&&(1..=1_000_000).contains(&settings.maximum_bank_sequence)&&settings.router.order_lifetime_ms>0&&settings.router.order_lifetime_ms<=600000,"P5_SETTINGS")?;
  let mut total=0;let artifact=read_original(&settings.artifact.path,&settings.artifact.sha256,16*1024*1024,&mut total)?;let raw:Value=serde_json::from_slice(&artifact)?;let original_asof=raw["observedAtMs"].as_u64().ok_or("P5_ORIGINAL_ASOF")?;
  let surface=Surface::load(&artifact,&settings.artifact.sha256,&settings.approved_artifact_sha256,original_asof)?;
  let authority=ReviewAuthority::load(&settings.authority_path,&settings.authority_sha256)?;let mut reviews=Vec::new();let mut hashes=BTreeSet::from([settings.artifact.sha256.clone(),settings.authority_sha256.clone()]);let mut exchanges=BTreeSet::new();
  for b in &settings.bindings{
   let bytes=read_original(&b.review.path,&b.review.file_sha256,64*1024,&mut total)?;let input:ReviewInput=serde_json::from_slice(&bytes)?;need(exchanges.insert(input.exchange_id.clone())&&(3..=8).contains(&b.review.source_files.len())&&b.review.source_files.len()==input.sources.len(),"P5_DUPLICATE_BINDING_OR_SOURCES")?;
   let mapping_bytes=read_original(&b.mapping.path,&b.mapping.sha256,64*1024,&mut total)?;let mapping:MappingReview=serde_json::from_slice(&mapping_bytes)?;
   let mut originals=Vec::new();for s in &b.review.source_files{originals.push(read_original(&s.path,&s.sha256,8*1024*1024,&mut total)?);hashes.insert(s.sha256.clone());}
   let refs=b.review.source_files.iter().zip(&originals).map(|(s,bytes)|SourceBytes{sha256:&s.sha256,bytes}).collect::<Vec<_>>();let reviewed_at=input.reviewed_at_ms;
   reviews.push(PreparedReview::verify(&surface,input,&refs,mapping,authority.trusted(),&settings.approved_mapping_sha256,reviewed_at)?);hashes.insert(b.review.file_sha256.clone());hashes.insert(b.mapping.sha256.clone());
  }authority.validate()?;Ok(Self{settings:settings.clone(),surface,reviews,source_hashes:hashes})
 }
 pub fn source_sha256(&self)->&str{self.surface.source_sha256()}
 pub fn original_source_hashes(&self)->&BTreeSet<String>{&self.source_hashes}
 /// Original official current-node receipts come from the EXISTING worker's
 /// preflight/cache. Missing/stale records do not trigger any extra API reads.
 /// This must run BEFORE reservation/attempt/preflight permit-critical work.
 pub fn evaluate(&self,bank:&Session,scope:&Scope,dispatch:&DispatchLog,nodes:&BTreeMap<String,Receipt>,signals:&BTreeMap<String,Signal>,trusted_innovation_models:&BTreeSet<String>,now:u64)->Diagnostic{
  let mut out=Diagnostic{state:"unavailable",at_ms:now,source_sha256:self.surface.source_sha256().into(),model_sha256:self.surface.model_sha256().into(),qualification:self.surface.qualification().clone(),bank_head:bank.durable_head(),deferred:Vec::new(),routing:Vec::new(),curve:None,hedge_ideas:Vec::new(),source_freshness:Vec::new(),execution_authority:false,financial_state_changed:false,source_files_read_during_evaluation:0};
  let defer=|out:&mut Diagnostic,reason:&str|out.deferred.push(json!({"reason":reason}));
  if now<self.surface.observed_at_ms(){defer(&mut out,"posterior_not_yet_observed");return out}if now>=self.surface.valid_until_ms(){defer(&mut out,"posterior_expired_original_clock");return out}
  if self.surface.is_research()&&!self.settings.allow_research_diagnostics{defer(&mut out,"research_model_not_promoted");return out}
  if scope.exchanges.len()>1024||dispatch.intents.len()>self.settings.maximum_dispatch_records||dispatch.attempted.len()>self.settings.maximum_dispatch_records||out.bank_head["sequence"].as_u64().is_none_or(|s|s>self.settings.maximum_bank_sequence){defer(&mut out,"diagnostic_work_bound");return out}
  if let Err(e)=dispatch.require_resolved_attempts(){out.state="deferred";defer(&mut out,&e.to_string());return out}
  let mut selected=Vec::new();for r in &self.reviews{if !r.is_current(now){out.deferred.push(json!({"exchangeId":r.exchange_id(),"reason":"review_or_source_not_current"}));continue}let Some(node)=nodes.get(r.exchange_id())else{out.deferred.push(json!({"exchangeId":r.exchange_id(),"reason":"original_current_node_not_retained"}));continue};selected.push((r,node));}
  if selected.is_empty(){return out}
  let panel=match BoundSurface::bind_prepared_with_semantic_age(self.surface.clone(),bank,scope,&selected,self.settings.maximum_semantic_receipt_age_ms,now){Ok(s)=>s,Err(e)=>{defer(&mut out,&e.to_string());return out}};
  for(c,(_,receipt))in panel.contracts().iter().zip(&selected){let b=&panel.books()[&c.exchange_id];out.source_freshness.push(json!({"exchangeId":c.exchange_id,"originalSemanticReceiptSha256":crate::feedback::digest(receipt).ok(),"semanticReceiptRequestAgeMs":crate::daily_windows::clock(&receipt.request_started_at).ok().and_then(|at|now.checked_sub(at)),"maximumSemanticReceiptAgeMs":self.settings.maximum_semantic_receipt_age_ms,"bookSourceAgeMs":now-b.observed_ms,"bookReceiptAgeMs":now-b.source.received_ms,"maximumBookAgeMs":bank.engine().policy()["bookAgeMs"].as_u64().map(|a|a.min(10000)),"physicalPacketStillRequired":true}));}
  let native_lifetime=bank.engine().policy()["lifetimeMs"].as_u64().unwrap_or(0);let Some(expiry)=now.checked_add(self.settings.router.order_lifetime_ms.min(native_lifetime))else{defer(&mut out,"proposal_clock_overflow");return out};
  let policy=RoutePolicy{strategy_sha256:self.settings.router.strategy_sha256.clone(),order_expiry_ms:expiry,model_uncertainty_per_share:self.settings.router.model_uncertainty_per_share,terminal_tail_charge_per_share:self.settings.router.terminal_tail_charge_per_share,write_charge_per_order:self.settings.router.write_charge_per_order,capital_charge_per_million_unit_ms:self.settings.router.capital_charge_per_million_unit_ms};
  let mut remaining=self.settings.maximum_proposals;for(i,c)in panel.contracts().iter().enumerate(){if remaining==0{defer(&mut out,"proposal_count_bound");break}
   let result=if let Some(signal)=signals.get(&c.exchange_id){execution_router::with_innovation(bank,scope,&panel,i,&policy,signal,trusted_innovation_models,now)}else{execution_router::terminal(bank,scope,&panel,i,&policy,now)};
   match result{Ok(mut route)=>{route.candidates.truncate(remaining);remaining-=route.candidates.len();match serde_json::to_value(route){Ok(v)=>out.routing.push(v),Err(e)=>out.deferred.push(json!({"exchangeId":c.exchange_id,"reason":e.to_string()}))}},Err(e)=>out.deferred.push(json!({"exchangeId":c.exchange_id,"reason":e.to_string()}))}
  }
  if let Some(p)=&self.settings.curve{match election_curve::fit(&panel,p){Ok(fit)=>{if let Some(limits)=&self.settings.hedges{match election_curve::hedge_candidates(&panel,&fit,bank,scope,limits,now){Ok(ideas)=>{out.hedge_ideas=ideas.into_iter().take(remaining).filter_map(|a|serde_json::to_value(a).ok()).collect();},Err(e)=>defer(&mut out,&e.to_string())}}out.curve=serde_json::to_value(fit).ok();},Err(e)=>defer(&mut out,&e.to_string())}}
  out.state="proposal_only";out
 }
}
