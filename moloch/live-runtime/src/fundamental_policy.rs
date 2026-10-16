//! Runtime policy for explicitly reviewed, risky terminal-value cohorts.
//!
//! Integration contract (sole-worker owner, no new client): open this ORIGINAL
//! sidecar before any dispatch; install reviewed policies; call
//! `exit_refresh_needed` before expired-exit hints, and `assess` before ordinary
//! priority/preflight and after fresh preflight.
//! Call `bind` before appending the ordinary physical-attempt event, and call
//! `check_bound` in physical authority with the exact request. Exclude `manages`
//! Add `durable_tag(qid)` as `fundamental` on that ORIGINAL ordinary intent row;
//! replay requires it and detects a lost/truncated sidecar binding.
//! exchanges from package acquisition/netting. Do not gate cancels or official
//! owned-order/fill reconciliation. Missing/corrupt sidecars must block fresh
//! writes, not restore ordinary age exits for previously managed inventory.
//! Service `entry_cancellations` through the existing cancellation authority
//! whenever a review changes/expires; it never resolves an unknown attempt.
//!
//! Native live age exits are proposals, not fills: this module vetoes their
//! physical submission while preserving all real native ACK/fill accounting.
//! It never edits a price, quantity, expiry, cash balance or financial policy.
//! An entry must fit the WHOLE existing native quote. Stale/revoked forecast
//! review stops entry; stale value retains the cohort only until its immutable
//! deadline, while explicit reviewed revocation/adverse value permits a covered
//! native exit. There is no settlement-floor/arbitrage certificate here.
use crate::{daily_windows, dispatch::DispatchLog, feedback::Result, package_admission};
use baal_transport_service::{RequestSpec, Scope, decimal_units};
use moloch_replay::{Side, money, session::{ExclusiveLock, Session}};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write}, path::{Path, PathBuf}, sync::Arc};

const UNIT:i128=1_000_000;
const MAX_ROW:usize=64*1024;
const MAX_LOG:u64=16*1024*1024;
fn need(ok:bool, why:&'static str)->Result<()> {if ok {Ok(())} else {Err(why.into())}}
fn digest<T:Serialize+?Sized>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn bytes_sha(b:&[u8])->String{format!("{:x}",Sha256::digest(b))}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn atom(s:&str)->bool{!s.is_empty()&&s.len()<=128&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b"-_:".contains(&b))}
fn amount(s:&str)->Result<i128>{let n=decimal_units(s)?;need(n>=0,"FUNDAMENTAL_NEGATIVE_AMOUNT")?;Ok(n)}
fn mul(a:i128,b:i128)->Result<i128>{a.checked_mul(b).ok_or("FUNDAMENTAL_RANGE".into())}
fn add(a:i128,b:i128)->Result<i128>{a.checked_add(b).ok_or("FUNDAMENTAL_RANGE".into())}

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct SourceRef {
    pub kind:String, pub url:String, pub sha256:String,
    pub published_at_ms:u64, pub observed_at_ms:u64, pub maximum_age_ms:u64,
}
pub struct SourceBytes<'a>{pub sha256:&'a str,pub bytes:&'a [u8]}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ReviewInput {
    pub version:u32, pub reviewer_id:String, pub assumption_label:String,
    pub run_id:String, pub bank_manifest_sha256:String, pub tournament_id:String,
    pub market_id:String, pub exchange_id:String, pub contract_id:String, pub side:String,
    /// Conditional probability of ordinary $1 payout, explicitly a model assumption.
    pub ordinary_probability:String, pub exceptional_probability_allowance:String,
    pub minimum_exceptional_payout:String, pub margin_per_share:String,
    pub maximum_principal:String, pub maximum_quantity:u64,
    pub reviewed_at_ms:u64, pub expires_at_ms:u64, pub hold_until_ms:u64,
    /// V1 deliberately supports assumptions, not a claim of guaranteed mapping.
    pub payout_mapping_is_operator_assumption:bool,
    pub revoke_existing:bool, pub sources:Vec<SourceRef>,
}
pub fn review_sha256(input:&ReviewInput)->Result<String>{digest(input)}
fn structure(r:&ReviewInput)->Result<i128>{
    need(r.version==1&&atom(&r.reviewer_id)&&atom(&r.run_id)&&hex(&r.bank_manifest_sha256)
        &&atom(&r.tournament_id)&&atom(&r.market_id)&&atom(&r.exchange_id)&&atom(&r.contract_id)
        &&matches!(r.side.as_str(),"YES"|"NO")&&r.payout_mapping_is_operator_assumption
        &&!r.assumption_label.trim().is_empty()&&r.assumption_label.len()<=4096,"FUNDAMENTAL_REVIEW_SCOPE_OR_ASSUMPTION")?;
    let p=amount(&r.ordinary_probability)?;let e=amount(&r.exceptional_probability_allowance)?;
    let exceptional=amount(&r.minimum_exceptional_payout)?;let margin=amount(&r.margin_per_share)?;
    need(p<=UNIT&&e<=UNIT&&exceptional<=UNIT&&margin<UNIT&&amount(&r.maximum_principal)?>0
        &&r.maximum_quantity>0&&r.maximum_quantity<=10_000,"FUNDAMENTAL_REVIEW_ECONOMICS")?;
    need(r.reviewed_at_ms<r.expires_at_ms&&r.expires_at_ms<=r.hold_until_ms
        &&r.hold_until_ms-r.reviewed_at_ms<=366*86_400_000,"FUNDAMENTAL_REVIEW_HORIZON")?;
    need((3..=8).contains(&r.sources.len()),"FUNDAMENTAL_SOURCES")?;
    let mut kinds=BTreeSet::new();let mut hashes=BTreeSet::new();
    for s in &r.sources{
        need(matches!(s.kind.as_str(),"forecast"|"official_ballot"|"official_contract_node"|"resolver")
            &&s.url.starts_with("https://")&&s.url.len()<=2048&&!s.url.contains('@')
            &&hex(&s.sha256)&&hashes.insert(&s.sha256)&&kinds.insert(s.kind.as_str())
            &&s.published_at_ms<=s.observed_at_ms&&s.observed_at_ms<=r.reviewed_at_ms
            &&s.maximum_age_ms>0&&s.maximum_age_ms<=366*86_400_000,"FUNDAMENTAL_SOURCE_BINDING")?;
    }
    need(["forecast","official_ballot","official_contract_node"].iter().all(|k|kinds.contains(k)),"FUNDAMENTAL_REQUIRED_SOURCE")?;
    // Floor once, after combining numerators: do not round a risky payout up.
    Ok(add(mul(UNIT-e,p)?,mul(e,exceptional)?)?/UNIT-margin)
}
fn current(r:&ReviewInput,now:u64)->bool{
    now>=r.reviewed_at_ms&&now<r.expires_at_ms&&r.sources.iter().all(|s|
        now>=s.observed_at_ms&&now-s.observed_at_ms<=s.maximum_age_ms)
}
pub struct Reviewed{input:ReviewInput,sha:String}
impl Reviewed{
    /// The allowlist is from the separately pinned execution-owner control.
    /// Supplying an input/hash does not approve itself or authenticate a publisher.
    pub fn verify(input:ReviewInput,originals:&[SourceBytes<'_>],trusted:&BTreeSet<String>,now:u64)->Result<Self>{
        structure(&input)?;let sha=review_sha256(&input)?;
        need(trusted.contains(&sha)&&current(&input,now),"FUNDAMENTAL_REVIEW_NOT_CURRENTLY_APPROVED")?;
        need(originals.len()==input.sources.len(),"FUNDAMENTAL_ORIGINAL_SOURCES")?;
        for s in &input.sources{
            let found=originals.iter().filter(|v|v.sha256==s.sha256).collect::<Vec<_>>();
            need(found.len()==1&&!found[0].bytes.is_empty()&&found[0].bytes.len()<=8*1024*1024
                &&bytes_sha(found[0].bytes)==s.sha256,"FUNDAMENTAL_ORIGINAL_SOURCE_HASH")?;
        }Ok(Self{input,sha})
    }
    pub fn sha256(&self)->&str{&self.sha}
    pub fn entry_ceiling(&self)->Result<String>{Ok(money(structure(&self.input)?))}
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Binding{qid:u64,spec:RequestSpec,spec_sha256:String,quote:Value,review_sha256:String,bound_at_ms:u64}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Cohort{entry:Binding,first_fill_index:usize,prior_fills_sha256:String,bank_head_at_entry:Value,hold_until_ms:u64,exits:BTreeMap<u64,Binding>}
#[derive(Clone,Debug,Serialize,PartialEq,Eq)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum Decision{
    Unmanaged,
    Hold{reason:String},
    Entry{review_sha256:String,maximum_principal:String,ceiling:String},
    Exit{reason:String,owned_quantity:u64},
}
fn hold(reason:&str)->Decision{Decision::Hold{reason:reason.into()}}

/// The held owner lease and the original root-derived ordinary log remain the
/// trust boundary. Hash chaining detects corruption, not malicious fabrication.
pub struct Cohorts{
    file:File,path:PathBuf,ordinary_path:PathBuf,ordinary_identity:Value,
    _owner:Arc<ExclusiveLock>,manifest:String,run:String,
    sequence:u64,previous:String,clock:u64,poisoned:bool,
    policies:BTreeMap<String,ReviewInput>,cohorts:BTreeMap<String,Cohort>,
}
#[cfg(unix)]fn identity(file:&File)->Result<Value>{use std::os::unix::fs::MetadataExt;let m=file.metadata()?;Ok(json!({"device":m.dev(),"inode":m.ino()}))}
#[cfg(not(unix))]fn identity(_file:&File)->Result<Value>{Err("FUNDAMENTAL_DESCRIPTOR_UNSUPPORTED".into())}
fn path_pin(path:&Path,want:&Value)->Result<()>{
    need(!fs::symlink_metadata(path)?.file_type().is_symlink(),"FUNDAMENTAL_SYMLINK")?;
    need(identity(&File::open(path)?)?==*want,"FUNDAMENTAL_PATH_REPLACED")
}
impl Cohorts{
    /// Explicit one-time owner operation; never invoked as missing-file recovery.
    pub fn initialize(root:&Path,bank:&Session,ordinary:&DispatchLog,owner:Arc<ExclusiveLock>)->Result<Self>{
        need(root.is_absolute()&&!fs::symlink_metadata(root)?.file_type().is_symlink(),"FUNDAMENTAL_ROOT")?;
        owner.descriptor_identity()?;let d=ordinary.descriptor_identity()?;
        path_pin(&root.join("dispatch.ndjson"),&json!({"device":d["device"],"inode":d["inode"]}))?;
        let path=root.join("fundamental.ndjson");
        let mut options=OpenOptions::new();options.write(true).create_new(true);
        #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
        let f=options.open(path)?;f.sync_all()?;File::open(root)?.sync_all()?;
        Self::open(root,bank,ordinary,owner)
    }
    /// `root` MUST be the sole worker's configured namespace, never an evidence
    /// path. A new empty sidecar is safe only after the owner establishes that no
    /// earlier fundamental tags exist. Never recreate a missing deployed sidecar.
    pub fn open(root:&Path,bank:&Session,ordinary:&DispatchLog,owner:Arc<ExclusiveLock>)->Result<Self>{
        need(root.is_absolute()&&!fs::symlink_metadata(root)?.file_type().is_symlink(),"FUNDAMENTAL_ROOT")?;
        owner.descriptor_identity()?;
        let ordinary_path=root.join("dispatch.ndjson");let d=ordinary.descriptor_identity()?;
        let ordinary_identity=json!({"device":d["device"],"inode":d["inode"]});path_pin(&ordinary_path,&ordinary_identity)?;
        let path=root.join("fundamental.ndjson");if path.exists(){need(!fs::symlink_metadata(&path)?.file_type().is_symlink(),"FUNDAMENTAL_SYMLINK")?;}
        let mut options=OpenOptions::new();options.read(true).append(true);
        #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
        let file=options.open(&path)?;file.try_lock().map_err(|_|"FUNDAMENTAL_WRITER_LOCKED")?;file.sync_all()?;File::open(root)?.sync_all()?;
        need(file.metadata()?.len()<=MAX_LOG,"FUNDAMENTAL_LOG_BOUND")?;
        let mut out=Self{file,path,ordinary_path,ordinary_identity,_owner:owner,
            manifest:bank.manifest_hash().as_str().ok_or("FUNDAMENTAL_MANIFEST")?.into(),
            run:bank.live_account_seed().ok_or("FUNDAMENTAL_LIVE_BANK")?["runId"].as_str().ok_or("FUNDAMENTAL_RUN")?.into(),
            sequence:0,previous:"0".repeat(64),clock:0,poisoned:false,policies:BTreeMap::new(),cohorts:BTreeMap::new()};
        let mut reader=BufReader::new(out.file.try_clone()?);let mut line=Vec::new();
        loop{line.clear();let n=reader.by_ref().take((MAX_ROW+1) as u64).read_until(b'\n',&mut line)?;if n==0{break}
            need(line.len()<=MAX_ROW&&line.last()==Some(&b'\n'),"FUNDAMENTAL_TORN_OR_OVERSIZED_ROW")?;
            let mut row:Value=serde_json::from_slice(&line)?;let sha=row.as_object_mut().ok_or("FUNDAMENTAL_ROW")?.remove("hash").ok_or("FUNDAMENTAL_ROW_HASH")?;
            need(row["sequence"]==out.sequence+1&&row["previous"]==out.previous&&sha==digest(&row)?
                &&row["manifestHash"]==out.manifest&&row["runId"]==out.run,"FUNDAMENTAL_CHAIN_OR_BANK")?;
            let at=row["atMs"].as_u64().filter(|at|*at>=out.clock).ok_or("FUNDAMENTAL_CLOCK")?;
            out.apply(&row["event"])?;out.sequence+=1;out.previous=sha.as_str().ok_or("FUNDAMENTAL_HASH")?.into();out.clock=at;
        }
        out.check(bank,ordinary)?;Ok(out)
    }
    fn apply(&mut self,event:&Value)->Result<()>{
        match event["kind"].as_str(){
            Some("review")=>{let r:ReviewInput=serde_json::from_value(event["review"].clone())?;structure(&r)?;
                need(event["sha256"]==review_sha256(&r)?&&r.run_id==self.run&&r.bank_manifest_sha256==self.manifest,"FUNDAMENTAL_REVIEW_BANK")?;
                if let Some(old)=self.policies.get(&r.exchange_id){need(old.market_id==r.market_id&&old.contract_id==r.contract_id&&old.tournament_id==r.tournament_id&&old.side==r.side&&r.reviewed_at_ms>old.reviewed_at_ms,"FUNDAMENTAL_REVIEW_REVISION")?;}
                need(self.policies.contains_key(&r.exchange_id)||self.policies.len()<32,"FUNDAMENTAL_POLICY_COUNT")?;
                if let Some(c)=self.cohorts.get(&r.exchange_id){need(r.hold_until_ms<=c.hold_until_ms,"FUNDAMENTAL_CANNOT_EXTEND_COHORT")?;}
                self.policies.insert(r.exchange_id.clone(),r);
            },
            Some("entry")=>{let ex=event["exchangeId"].as_str().ok_or("FUNDAMENTAL_EXCHANGE")?;let c:Cohort=serde_json::from_value(event["cohort"].clone())?;
                need(self.policies.contains_key(ex)&&!self.cohorts.contains_key(ex)&&c.exits.is_empty(),"FUNDAMENTAL_EXCLUSIVE_COHORT")?;self.cohorts.insert(ex.into(),c);},
            Some("exit")=>{let ex=event["exchangeId"].as_str().ok_or("FUNDAMENTAL_EXCHANGE")?;let b:Binding=serde_json::from_value(event["binding"].clone())?;let c=self.cohorts.get_mut(ex).ok_or("FUNDAMENTAL_NO_COHORT")?;
                need(c.exits.len()<128&&!c.exits.contains_key(&b.qid),"FUNDAMENTAL_EXIT_BINDING_LIMIT_OR_DUPLICATE")?;c.exits.insert(b.qid,b);},
            _=>return Err("FUNDAMENTAL_EVENT".into()),
        }Ok(())
    }
    fn append(&mut self,event:Value,at:u64)->Result<()>{
        need(!self.poisoned&&at>=self.clock,"FUNDAMENTAL_POISONED_OR_CLOCK")?;
        path_pin(&self.path,&identity(&self.file)?)?;
        // Apply to small metadata maps before writing; no financial state copy.
        let policies=self.policies.clone();let cohorts=self.cohorts.clone();
        let result=self.apply(&event);self.policies=policies;self.cohorts=cohorts;result?;
        let mut row=json!({"sequence":self.sequence+1,"previous":self.previous,"manifestHash":self.manifest,"runId":self.run,"atMs":at,"event":event});
        let sha=digest(&row)?;row["hash"]=json!(sha);let mut bytes=serde_json::to_vec(&row)?;bytes.push(b'\n');
        need(bytes.len()<=MAX_ROW&&self.file.metadata()?.len().checked_add(bytes.len() as u64).is_some_and(|n|n<=MAX_LOG),"FUNDAMENTAL_LOG_BOUND")?;
        self.poisoned=true;self.file.write_all(&bytes)?;self.file.sync_all()?;
        path_pin(&self.path,&identity(&self.file)?)?;self.apply(&event)?;
        self.sequence+=1;self.previous=sha;self.clock=at;self.poisoned=false;Ok(())
    }
    fn check(&self,bank:&Session,log:&DispatchLog)->Result<()>{
        need(!self.poisoned&&bank.manifest_hash().as_str()==Some(self.manifest.as_str())&&bank.live_account_seed().is_some_and(|s|s["runId"]==self.run),"FUNDAMENTAL_BANK_OR_POISON")?;
        path_pin(&self.path,&identity(&self.file)?)?;path_pin(&self.ordinary_path,&self.ordinary_identity)?;
        let d=log.descriptor_identity()?;need(d["device"]==self.ordinary_identity["device"]&&d["inode"]==self.ordinary_identity["inode"],"FUNDAMENTAL_ORIGINAL_DISPATCH_REQUIRED")?;
        for(id,row)in &log.intents{if row.get("fundamental").is_some_and(|v|!v.is_null()){
            need(self.durable_tag(*id).as_ref()==row.get("fundamental"),"FUNDAMENTAL_REQUIRED_SIDECAR_TAG_MISSING")?;
        }}
        for ex in self.policies.keys().filter(|ex|!self.cohorts.contains_key(*ex)){
            need(bank.engine().wallet.quantity(ex,Side::Yes)==0&&bank.engine().wallet.quantity(ex,Side::No)==0,
                "FUNDAMENTAL_UNTAGGED_POSITION_AFTER_POLICY_INSTALL")?;
        }
        for(ex,c)in &self.cohorts{
            for b in std::iter::once(&c.entry).chain(c.exits.values()){
                need(digest(&b.spec)?==b.spec_sha256,"FUNDAMENTAL_BINDING_HASH")?;
                if let Some(original)=log.intents.get(&b.qid){let s:RequestSpec=serde_json::from_value(original["spec"].clone())?;need(digest(&s)?==b.spec_sha256&&original.get("fundamental")==self.durable_tag(b.qid).as_ref(),"FUNDAMENTAL_DISPATCH_REQUEST_OR_TAG_CHANGED")?;}
                else{need(log.never_attempted(b.qid),"FUNDAMENTAL_MISSING_ORIGINAL_INTENT")?;}
                let q=bank.engine().order_records().into_iter().find(|q|q["id"]==b.qid).ok_or("FUNDAMENTAL_NATIVE_QUOTE_MISSING")?;
                need(q["exchangeId"]==*ex&&q["side"]==b.quote["side"]&&q["marketId"]==b.quote["marketId"]&&q["action"]==b.quote["action"]&&q["price"]==b.quote["price"]&&q["quantity"].as_u64().is_some_and(|n|n<=b.quote["quantity"].as_u64().unwrap_or(0)),"FUNDAMENTAL_NATIVE_BINDING_CHANGED")?;
            }
            self.owned(bank,ex,c)?;
        }Ok(())
    }
    fn owned(&self,bank:&Session,ex:&str,c:&Cohort)->Result<u64>{
        let r=self.policies.get(ex).ok_or("FUNDAMENTAL_REVIEW_MISSING")?;let mut held=0u64;
        need(c.first_fill_index<=bank.engine().fills().len(),"FUNDAMENTAL_BANK_PREFIX_MISSING")?;
        need(digest(&bank.engine().fills()[..c.first_fill_index])?==c.prior_fills_sha256
            &&bank.durable_head()["sequence"].as_u64()>=c.bank_head_at_entry["sequence"].as_u64(),"FUNDAMENTAL_FILL_PREFIX_CHANGED")?;
        for f in &bank.engine().fills()[c.first_fill_index..]{if f["exchangeId"]!=ex{continue}
            let id=f["orderId"].as_u64().ok_or("FUNDAMENTAL_FILL_ID")?;let n=f["quantity"].as_u64().ok_or("FUNDAMENTAL_FILL_QUANTITY")?;
            need(f["side"]==r.side&&f["counterfactual"]==false,"FUNDAMENTAL_FOREIGN_FILL")?;
            if id==c.entry.qid&&f["action"]=="buy"{held=held.checked_add(n).ok_or("FUNDAMENTAL_RANGE")?;}
            else if c.exits.contains_key(&id)&&f["action"]=="sell"{held=held.checked_sub(n).ok_or("FUNDAMENTAL_EXIT_EXCEEDS_COHORT")?;}
            else{return Err("FUNDAMENTAL_FOREIGN_FILL".into())}
        }
        let side=if r.side=="YES"{Side::Yes}else{Side::No};let opposite=if r.side=="YES"{Side::No}else{Side::Yes};
        need(bank.engine().wallet.quantity(ex,side)==held&&bank.engine().wallet.quantity(ex,opposite)==0,"FUNDAMENTAL_UNATTRIBUTED_HOLDING")?;Ok(held)
    }
    pub fn manages(&self,exchange:&str)->bool{self.policies.contains_key(exchange)}
    /// An expired native quote is not a submit candidate, so `assess` cannot be
    /// used for the expired-quote read hint. This separate gate permits bounded
    /// refresh at the fixed deadline, reviewed revocation/revision, or an already
    /// observed executable value exit; final `assess` must still pass afterward.
    pub fn exit_refresh_needed(&self,bank:&Session,log:&DispatchLog,scope:&Scope,exchange:&str,trusted:&BTreeSet<String>,now:u64)->Result<bool>{
        self.check(bank,log)?;let Some(r)=self.policies.get(exchange)else{return Ok(true)};
        let Some(c)=self.cohorts.get(exchange)else{return Ok(false)};
        if self.owned(bank,exchange,c)?==0||!log.closed.contains(&c.entry.qid)
            ||c.exits.keys().any(|id|log.attempted.contains(id)&&!log.closed.contains(id)){return Ok(false)}
        need(scope.tournament_id==r.tournament_id&&scope.exchanges.get(exchange)==Some(&r.market_id),"FUNDAMENTAL_SCOPE_CHANGED")?;
        if now>=c.hold_until_ms.min(r.hold_until_ms){return Ok(true)}
        if !trusted.contains(&digest(r)?)||!current(r,now){return Ok(false)}
        if r.revoke_existing||digest(r)?!=c.entry.review_sha256{return Ok(true)}
        let(books,_)=package_admission::native_books(bank,scope,now,bank.engine().policy()["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?)?;
        let value=add(structure(r)?,amount(&r.margin_per_share)?)?;
        Ok(books.get(exchange).and_then(|b|if r.side=="YES"{b.yes_bids.first()}else{b.no_bids.first()}).is_some_and(|l|l.price>=value))
    }
    pub fn durable_tag(&self,qid:u64)->Option<Value>{
        for c in self.cohorts.values(){if let Some(b)=if c.entry.qid==qid{Some(&c.entry)}else{c.exits.get(&qid)}{
            return Some(json!({"version":1,"bankManifestHash":self.manifest,"runId":self.run,"entryQid":c.entry.qid,"qid":qid,"bindingSha256":digest(b).ok()?}));
        }}None
    }
    pub fn install(&mut self,r:Reviewed,bank:&Session,log:&DispatchLog,now:u64)->Result<()>{
        self.check(bank,log)?;need(r.input.run_id==self.run&&r.input.bank_manifest_sha256==self.manifest,"FUNDAMENTAL_REVIEW_BANK")?;
        if let Some(old)=self.policies.get(&r.input.exchange_id){if digest(old)?==r.sha{return Ok(())}}
        else{
            let ex=&r.input.exchange_id;
            need(bank.engine().wallet.quantity(ex,Side::Yes)==0&&bank.engine().wallet.quantity(ex,Side::No)==0
                &&!bank.engine().order_records().iter().any(|q|q["exchangeId"]==*ex
                    &&q["id"].as_u64().is_some_and(|id|log.attempted.contains(&id)&&!log.closed.contains(&id)))
                &&!bank.engine().package_records().as_array().ok_or("FUNDAMENTAL_PACKAGE_RECORDS")?.iter().any(|p|
                    p["phase"]!="no_execution"&&p.get("protectionReleased")!=Some(&json!(true))
                    &&p["legs"].as_array().is_some_and(|legs|legs.iter().any(|l|l["exchangeId"]==*ex))),
                "FUNDAMENTAL_CANNOT_ADOPT_OR_CLAIM_EXISTING_EXPOSURE")?;
        }
        need(current(&r.input,now),"FUNDAMENTAL_INSTALL_REVIEW_EXPIRED")?;
        self.append(json!({"kind":"review","review":r.input,"sha256":r.sha}),now)
    }
    /// No sidecar closing/reset shortcut: a completed flat cohort stays excluded
    /// until a separately reviewed future lifecycle is deliberately implemented.
    pub fn assess(&self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,trusted:&BTreeSet<String>,now:u64)->Result<Decision>{
        self.check(bank,log)?;
        let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("FUNDAMENTAL_QUOTE")?;
        let ex=q["exchangeId"].as_str().ok_or("FUNDAMENTAL_EXCHANGE")?;let Some(r)=self.policies.get(ex)else{return Ok(Decision::Unmanaged)};
        need(scope.tournament_id==r.tournament_id&&scope.exchanges.get(ex)==Some(&r.market_id)&&q["marketId"]==r.market_id,"FUNDAMENTAL_SCOPE_CHANGED")?;
        if bank.engine().is_package_order(qid){return Ok(hold("package_cannot_consume_fundamental_inventory"))}
        if q["retired"]!=false||q["expiresAt"].as_u64().is_none_or(|t|t<=now)||q["cancelEffectiveAt"].is_u64(){return Ok(hold("inactive_native_quote"))}
        let approved=trusted.contains(&digest(r)?)&&current(r,now);
        let ceiling=structure(r)?;
        let (books,_)=package_admission::native_books(bank,scope,now,bank.engine().policy()["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?)?;
        let Some(book)=books.get(ex)else{return Ok(hold("fresh_official_book_required"))};
        let price=amount(q["price"].as_str().ok_or("FUNDAMENTAL_PRICE")?)?;let n=q["quantity"].as_u64().filter(|n|*n>0).ok_or("FUNDAMENTAL_QUANTITY")?;
        if let Some(c)=self.cohorts.get(ex){
            let owned=self.owned(bank,ex,c)?;
            if q["action"]!="sell"||q["side"]!=r.side||owned==0{return Ok(hold("exclusive_cohort_no_additional_or_opposite_entry"))}
            if !log.closed.contains(&c.entry.qid)||c.exits.keys().any(|id|*id!=qid&&log.attempted.contains(id)&&!log.closed.contains(id)){return Ok(hold("prior_owned_order_reconciliation_required"))}
            need(n<=owned,"FUNDAMENTAL_UNCOVERED_EXIT")?;
            // Verify actual depth, not the last print or nominal quote price.
            let levels=if r.side=="YES"{&book.yes_bids}else{&book.no_bids};
            let mut left=n;let mut proceeds=0i128;
            for l in levels{if l.price<price{break}let take=left.min(l.quantity);proceeds=add(proceeds,mul(l.price,take as i128)?)?;left-=take;if left==0{break}}
            if left!=0{return Ok(hold("covered_exit_not_executable_at_native_limit"))}
            let reason=if now>=c.hold_until_ms.min(r.hold_until_ms){Some("reviewed_holding_deadline")}
                else if approved&&r.revoke_existing{Some("reviewed_source_revocation")}
                else if approved&&proceeds>=mul(add(ceiling,amount(&r.margin_per_share)?)?.max(0),n as i128)?{Some("refreshed_value_exit")}
                else{None};
            return Ok(reason.map_or_else(||hold(if approved{"terminal_value_hold_not_native_age_exit"}else{"stale_review_hold_until_deadline"}),|s|Decision::Exit{reason:s.into(),owned_quantity:owned}))
        }
        if !approved||r.revoke_existing||now>=r.hold_until_ms{return Ok(hold("current_review_required_for_entry"))}
        if q["action"]!="buy"||q["side"]!=r.side{return Ok(hold("reviewed_entry_side_only"))}
        if bank.engine().wallet.quantity(ex,Side::Yes)!=0||bank.engine().wallet.quantity(ex,Side::No)!=0{return Ok(hold("existing_inventory_cannot_be_adopted"))}
        if log.attempted.iter().any(|id|!log.acknowledgments.contains_key(id)){return Ok(hold("unknown_physical_acceptance"))}
        if bank.engine().order_records().iter().any(|other|other["exchangeId"]==ex
            &&other["id"].as_u64().is_some_and(|id|log.attempted.contains(&id)&&!log.closed.contains(&id))){return Ok(hold("same_exchange_physical_order_open"))}
        if bank.engine().package_blocks_submissions(){return Ok(hold("unresolved_package"))}
        if now>=daily_windows::clock(bank.engine().policy()["entryStopAt"].as_str().ok_or("FUNDAMENTAL_ENTRY_DEADLINE")?)?{return Ok(hold("original_entry_window_expired"))}
        let source_until=r.sources.iter().try_fold(r.expires_at_ms,|end,s|Ok::<_,Box<dyn std::error::Error>>(end.min(s.observed_at_ms.checked_add(s.maximum_age_ms).ok_or("FUNDAMENTAL_SOURCE_CLOCK_RANGE")?)))?;
        if q["expiresAt"].as_u64().is_none_or(|t|t>source_until){return Ok(hold("native_order_outlives_reviewed_source_window"))}
        let asks=if r.side=="YES"{&book.yes_asks}else{&book.no_asks};
        if asks.first().is_none_or(|l|price>=l.price){return Ok(hold("ordinary_entry_must_remain_passive"))}
        if price>ceiling||n>r.maximum_quantity||mul(price,n as i128)?>amount(&r.maximum_principal)?{return Ok(hold("whole_native_quote_exceeds_reviewed_value_or_risk_cap"))}
        Ok(Decision::Entry{review_sha256:digest(r)?,maximum_principal:r.maximum_principal.clone(),ceiling:money(ceiling)})
    }
    /// Qids with known actual ACKs only. Queue/original-log unknowns remain on
    /// their original recovery path; never invent an actual ID to cancel.
    pub fn entry_cancellations(&self,bank:&Session,log:&DispatchLog,trusted:&BTreeSet<String>,now:u64)->Result<Vec<u64>>{
        self.check(bank,log)?;let mut out=Vec::new();
        for(ex,c)in &self.cohorts{let r=&self.policies[ex];
            if !log.closed.contains(&c.entry.qid)&&log.acknowledgments.contains_key(&c.entry.qid)
                &&(!current(r,now)||r.revoke_existing||!trusted.contains(&digest(r)?)||digest(r)?!=c.entry.review_sha256||now>=c.hold_until_ms.min(r.hold_until_ms)){
                out.push(c.entry.qid);
            }
        }Ok(out)
    }
    /// Persist exact original request BEFORE any physical attempt, never after
    /// observing a fill. No partial-size substitution or guessed ACK is allowed.
    pub fn bind(&mut self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,spec:&RequestSpec,trusted:&BTreeSet<String>,now:u64)->Result<Decision>{
        let decision=self.assess(bank,log,scope,qid,trusted,now)?;
        need(matches!(decision,Decision::Entry{..}|Decision::Exit{..}),"FUNDAMENTAL_NOT_ELIGIBLE")?;
        need(log.never_attempted(qid)&&!log.intents.contains_key(&qid),"FUNDAMENTAL_BIND_MUST_PRECEDE_DISPATCH")?;
        let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("FUNDAMENTAL_QUOTE")?;let ex=q["exchangeId"].as_str().unwrap();let r=&self.policies[ex];
        request_matches(spec,&q,&self.run,&r.tournament_id)?;
        let b=Binding{qid,spec:spec.clone(),spec_sha256:digest(spec)?,quote:q.clone(),review_sha256:digest(r)?,bound_at_ms:now};
        if matches!(decision,Decision::Entry{..}){let c=Cohort{entry:b,first_fill_index:bank.engine().fills().len(),prior_fills_sha256:digest(&bank.engine().fills())?,bank_head_at_entry:bank.durable_head(),hold_until_ms:r.hold_until_ms,exits:BTreeMap::new()};self.append(json!({"kind":"entry","exchangeId":ex,"cohort":c}),now)?;}
        else{self.append(json!({"kind":"exit","exchangeId":ex,"binding":b}),now)?;}
        Ok(decision)
    }
    /// Policy-only physical gate. An existing attempt grants NO retry authority.
    /// The SDK/original dispatch retain unknown-acceptance and rate controls.
    pub fn check_bound(&self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,spec:&RequestSpec,trusted:&BTreeSet<String>,now:u64)->Result<()>{
        self.check(bank,log)?;
        let(ex,c)=self.cohorts.iter().find(|(_,c)|c.entry.qid==qid||c.exits.contains_key(&qid)).ok_or("FUNDAMENTAL_DURABLE_BINDING_REQUIRED")?;
        let b=if c.entry.qid==qid{&c.entry}else{&c.exits[&qid]};need(digest(spec)?==b.spec_sha256,"FUNDAMENTAL_PHYSICAL_REQUEST_CHANGED")?;
        if c.entry.qid==qid{
            let r=&self.policies[ex];need(trusted.contains(&digest(r)?)&&digest(r)?==b.review_sha256&&current(r,now)&&!r.revoke_existing&&now<c.hold_until_ms,"FUNDAMENTAL_PHYSICAL_REVIEW_CHANGED")?;
            need(self.owned(bank,ex,c)?==0&&log.attempted.iter().all(|id|*id==qid||log.acknowledgments.contains_key(id)),"FUNDAMENTAL_PHYSICAL_ENTRY_OWNERSHIP")?;
            need(scope.tournament_id==r.tournament_id&&scope.exchanges.get(ex)==Some(&r.market_id)
                &&now<daily_windows::clock(bank.engine().policy()["entryStopAt"].as_str().ok_or("FUNDAMENTAL_ENTRY_DEADLINE")?)?,"FUNDAMENTAL_PHYSICAL_SCOPE_OR_DEADLINE")?;
            let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("FUNDAMENTAL_QUOTE")?;
            need(q["retired"]==false&&q["quantity"]==b.quote["quantity"]&&q["activationAt"].as_u64().is_some_and(|t|t<=now)
                &&q["expiresAt"].as_u64().is_some_and(|t|t>now)&&!q["cancelEffectiveAt"].is_u64(),"FUNDAMENTAL_PHYSICAL_NATIVE_INACTIVE")?;
            request_matches(spec,&b.quote,&self.run,&r.tournament_id)?;
            let(books,_)=package_admission::native_books(bank,scope,now,bank.engine().policy()["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?)?;
            let book=books.get(ex).ok_or("FUNDAMENTAL_PHYSICAL_BOOK_STALE")?;let asks=if r.side=="YES"{&book.yes_asks}else{&book.no_asks};let price=amount(b.quote["price"].as_str().ok_or("FUNDAMENTAL_PRICE")?)?;
            need(price<=structure(r)?&&asks.first().is_some_and(|l|price<l.price),"FUNDAMENTAL_PHYSICAL_ENTRY_EDGE")?;
        }else{need(matches!(self.assess(bank,log,scope,qid,trusted,now)?,Decision::Exit{..}),"FUNDAMENTAL_PHYSICAL_EXIT_NOT_ELIGIBLE")?;}
        Ok(())
    }
}
fn request_matches(s:&RequestSpec,q:&Value,run:&str,tournament:&str)->Result<()>{
    let b=s.body.as_ref().ok_or("FUNDAMENTAL_BODY")?;
    let expected_side=if q["side"]=="YES"{"yes"}else{"no"};
    need(s.method=="POST"&&s.endpoint=="/orders"&&b["idempotencyKey"]==format!("{run}:{}",q["id"].as_u64().ok_or("FUNDAMENTAL_QID")?)&&b["tournamentId"]==tournament
        &&b["exchangeId"]==q["exchangeId"]&&b["side"]==expected_side&&b["action"]==q["action"]&&b["quantity"]==q["quantity"]
        &&decimal_units(&b["price"].to_string())?==amount(q["price"].as_str().ok_or("FUNDAMENTAL_PRICE")?)?
        &&daily_windows::clock(b["expirationDate"].as_str().ok_or("FUNDAMENTAL_EXPIRATION")?)?==q["expiresAt"].as_u64().ok_or("FUNDAMENTAL_EXPIRATION")?,"FUNDAMENTAL_EXACT_NATIVE_REQUEST")?;
    if let Some(raw)=&s.body_json{need(serde_json::from_str::<Value>(raw)?==*b,"FUNDAMENTAL_RAW_BODY_CHANGED")?;}Ok(())
}
