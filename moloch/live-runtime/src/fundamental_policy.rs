//! Runtime policy for explicitly reviewed, risky terminal-value cohorts.
//!
//! Integration contract (sole-worker owner, no new client): open this ORIGINAL
//! sidecar before any dispatch; install reviewed policies; call
//! `exit_refresh_needed` before expired-exit hints, and `assess` before ordinary
//! priority/preflight and after fresh preflight.
//! Sequence: `bind` -> `physical_decision` -> prepare frozen authority and its
//! preliminary authorization -> ORIGINAL tagged intent+attempt -> `check_bound`
//! against the actual Session/log -> SDK, whose physical callback validates the
//! token's frozen data, original files and time. Compare `native_head()` with the
//! actual Session head while preparing authority. Add `durable_tag(qid)` as
//! `fundamental` on that original intent; replay detects a lost sidecar binding.
//! Exclude `manages` exchanges from package acquisition/netting. Do not gate cancels or official
//! owned-order/fill reconciliation. Missing/corrupt sidecars must block fresh
//! writes, not restore ordinary age exits for previously managed inventory.
//! Service `entry_cancellations` through the existing cancellation authority
//! whenever a review changes/expires; it never resolves an unknown attempt.
//!
//! Native live age exits are proposals, not fills: this module vetoes their
//! physical submission while preserving all real native ACK/fill accounting.
//! It never edits a price, quantity, expiry, cash balance or financial policy.
//! Default entry APIs require the WHOLE native quote; explicit quantity APIs
//! bind one fixed review-sized physical slice before any attempt. Native price,
//! original quote/reservation and actual ACK/fill semantics remain unchanged.
//! Stale/revoked forecast
//! review stops entry; stale value retains the cohort only until its immutable
//! deadline, while explicit reviewed revocation/adverse value permits a covered
//! native exit. There is no settlement-floor/arbitrage certificate here.
use crate::{daily_windows, dispatch::DispatchLog, feedback::Result, package_admission};
use baal_transport_service::{RequestSpec, Scope, Permit, Receipt, Transport, decimal_units};
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
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ExistingApproval{pub adoption_receipt_sha256:String,pub adopted_position_sha256:String}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ReviewInput {
    pub version:u32, pub reviewer_id:String, pub assumption_label:String,
    pub run_id:String, pub bank_manifest_sha256:String, pub tournament_id:String,
    pub market_id:String, pub exchange_id:String, pub contract_id:String, pub side:String,
    /// Canonical semantic SHA of the ORIGINAL official node root, verified from
    /// the approved SourceBytes and rechecked against current physical facts.
    pub official_contract_root_sha256:String,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub existing_origin:Option<ExistingApproval>,
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
    if let Some(a)=&r.existing_origin{need(hex(&a.adoption_receipt_sha256)&&hex(&a.adopted_position_sha256),"FUNDAMENTAL_EXISTING_APPROVAL")?;}
    need(r.version==1&&atom(&r.reviewer_id)&&atom(&r.run_id)&&hex(&r.bank_manifest_sha256)
        &&atom(&r.tournament_id)&&atom(&r.market_id)&&atom(&r.exchange_id)&&atom(&r.contract_id)&&hex(&r.official_contract_root_sha256)
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
/// Canonical root hashing ignores object-key ordering but preserves every root
/// field and value. It is a change detector, not resolver-authentication proof.
pub fn official_contract_root_sha256(data:&Value)->Result<String>{
    need(data["root"].is_object()&&data["root"]["node_type"]=="contract","FUNDAMENTAL_OFFICIAL_CONTRACT_ROOT")?;
    digest(&data["root"])
}
fn validate_contract_data(input:&ReviewInput,data:&Value)->Result<()>{
    need(data["market_id"]==input.market_id&&data["root"]["node_type"]=="contract"
        &&data["root"]["node_id"]==input.contract_id&&data["root"]["contract_id"]==input.contract_id
        &&official_contract_root_sha256(data)?==input.official_contract_root_sha256,"FUNDAMENTAL_REVIEWED_CONTRACT_CHANGED")
}
fn verify_snapshot(input:ReviewInput,originals:&[SourceBytes<'_>],trusted:&BTreeSet<String>)->Result<Reviewed>{
    structure(&input)?;let sha=review_sha256(&input)?;
    need(trusted.contains(&sha),"FUNDAMENTAL_REVIEW_NOT_CURRENTLY_APPROVED")?;
    need(originals.len()==input.sources.len(),"FUNDAMENTAL_ORIGINAL_SOURCES")?;
    for s in &input.sources{
        let found=originals.iter().filter(|v|v.sha256==s.sha256).collect::<Vec<_>>();
        need(found.len()==1&&!found[0].bytes.is_empty()&&found[0].bytes.len()<=8*1024*1024
            &&bytes_sha(found[0].bytes)==s.sha256,"FUNDAMENTAL_ORIGINAL_SOURCE_HASH")?;
        if s.kind=="official_contract_node"{
            let original:Value=serde_json::from_slice(found[0].bytes)?;
            let data=if original.get("root").is_some(){&original}else{
                need(original["status"]==200&&original["endpoint"]==format!("/markets/{}/nodes?tournamentId={}",input.market_id,input.tournament_id),"FUNDAMENTAL_ORIGINAL_CONTRACT_RECEIPT")?;
                &original["data"]
            };
            validate_contract_data(&input,data)?;
        }
    }Ok(Reviewed{input,sha})
}
impl Reviewed{
    /// The allowlist is from the separately pinned execution-owner control.
    /// Supplying an input/hash does not approve itself or authenticate a publisher.
    pub fn verify(input:ReviewInput,originals:&[SourceBytes<'_>],trusted:&BTreeSet<String>,now:u64)->Result<Self>{
        need(current(&input,now),"FUNDAMENTAL_REVIEW_NOT_CURRENTLY_APPROVED")?;
        verify_snapshot(input,originals,trusted)
    }
    pub fn sha256(&self)->&str{&self.sha}
    pub fn entry_ceiling(&self)->Result<String>{Ok(money(structure(&self.input)?))}
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Binding{qid:u64,spec:RequestSpec,spec_sha256:String,quote:Value,review_sha256:String,bound_at_ms:u64}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Cohort{#[serde(default,skip_serializing_if="Option::is_none")]entry:Option<Binding>,#[serde(default,skip_serializing_if="Option::is_none")]existing:Option<ExistingOrigin>,first_fill_index:usize,prior_fills_sha256:String,bank_head_at_entry:Value,hold_until_ms:u64,exits:BTreeMap<u64,Binding>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct ExistingOrigin{approval:ExistingApproval,adoption_ref:Value,adopted_position:Value,lots:Value,quantity:u64,principal:String,review_sha256:String}
impl Cohort{
    fn entry_id(&self)->Option<u64>{self.entry.as_ref().map(|b|b.qid)}
    fn binding(&self,qid:u64)->Option<&Binding>{self.entry.as_ref().filter(|b|b.qid==qid).or_else(||self.exits.get(&qid))}
    fn pending_entry(&self,log:&DispatchLog)->bool{self.entry_id().is_some_and(|id|!log.closed.contains(&id))}
    fn origin_sha(&self)->Result<String>{digest(&json!({"existing":self.existing,"firstFillIndex":self.first_fill_index,"fillPrefixSha256":self.prior_fills_sha256,"nativeHead":self.bank_head_at_entry}))}
}
/// Captured from the actual loaded bank and sole existing Transport, never from
/// a caller-provided holdings projection. It changes no native money or lots.
pub struct ExistingSnapshot{origin:ExistingOrigin,head:Value,report_sha:String,fill_count:usize,fill_sha:String,dispatch:Value,queue:Value,scope_sha:String,account_sha:String}
fn fifo_remaining(lots:&Value,mut closed:u64)->Result<Value>{
    let mut remaining=Vec::new();for lot in lots.as_array().ok_or("FUNDAMENTAL_FIFO_LOTS")?{let n=lot["quantity"].as_u64().filter(|n|*n>0).ok_or("FUNDAMENTAL_FIFO_QUANTITY")?;
        need(lot.get("executedAt").is_none()&&amount(lot["price"].as_str().ok_or("FUNDAMENTAL_FIFO_PRICE")?)?<=UNIT,"FUNDAMENTAL_PURE_INHERITED_ONLY")?;
        let take=closed.min(n);closed-=take;if take<n{let mut row=lot.clone();row["quantity"]=json!(n-take);remaining.push(row);}}
    need(closed==0,"FUNDAMENTAL_FIFO_OVERSELL")?;Ok(json!(remaining))
}
fn no_existing_liability(bank:&Session,log:&DispatchLog,ex:&str)->Result<()>{
    log.require_resolved_attempts()?;
    let orders=bank.engine().order_records();for id in log.acknowledgments.keys().filter(|id|!log.closed.contains(id)){
        let q=orders.iter().find(|q|q["id"]==*id).ok_or("FUNDAMENTAL_UNMAPPED_OPEN_ACK")?;need(q["exchangeId"]!=ex,"FUNDAMENTAL_EXISTING_LIABILITY")?;
    }
    need(!bank.engine().order_records().iter().any(|q|q["exchangeId"]==ex&&q["retired"]==false)
        &&!log.acknowledgments.keys().any(|id|!log.closed.contains(id)&&bank.engine().order_records().iter().any(|q|q["id"]==*id&&q["exchangeId"]==ex))
        &&!bank.engine().package_records().as_array().ok_or("FUNDAMENTAL_PACKAGE_RECORDS")?.iter().any(|p|p["phase"]!="no_execution"&&p["protectionReleased"]!=true
            &&(p["legs"].as_array().is_some_and(|a|a.iter().any(|l|l["exchangeId"]==ex))||p["portfolio"]["componentExchanges"].as_array().is_some_and(|a|a.iter().any(|x|x==ex)))),"FUNDAMENTAL_EXISTING_LIABILITY")
}
fn queue_view(transport:&Transport)->Result<Value>{let q=transport.package_queue_quiet()?;Ok(json!({"sequence":q.sequence,"previous":q.previous}))}
impl ExistingSnapshot{
    pub fn approval(&self)->&ExistingApproval{&self.origin.approval}
    pub fn capture(bank:&Session,log:&DispatchLog,transport:&Transport,source_id:&str,sequence:u64,adoption_event:&Value,exchange:&str)->Result<Self>{
        let queue=queue_view(transport)?;no_existing_liability(bank,log,exchange)?;
        let mut event=adoption_event.clone();let source_ref=event.as_object_mut().ok_or("FUNDAMENTAL_ADOPTION_EVENT")?.remove("sourceRef");let event_sha=digest(&event)?;
        need(bank.committed_source_hash(source_id,sequence)==Some(event_sha.as_str())&&source_ref.is_none_or(|v|v==json!({"id":source_id,"sequence":sequence,"hash":event_sha}))
            &&event["type"]=="actual_position_adoption"&&event["authority"]=="human_moloch_inherited_management_20261015","FUNDAMENTAL_COMMITTED_ADOPTION_REQUIRED")?;
        let report=bank.engine().report()?;let inherited=&report["inheritedInventory"];
        need(event["receiptHash"]==inherited["receiptHash"]&&event["positions"]==inherited["sourcePositions"]
            &&inherited["adoptedExchangeIds"].as_array().is_some_and(|xs|xs.iter().any(|x|x==exchange))&&event["tournamentId"]==transport.scope().tournament_id,"FUNDAMENTAL_ADOPTION_REPORT")?;
        let rows=event["positions"].as_array().ok_or("FUNDAMENTAL_ADOPTION_POSITIONS")?;let matches=rows.iter().filter(|p|p["exchangeId"]==exchange).collect::<Vec<_>>();need(matches.len()==1,"FUNDAMENTAL_ADOPTED_POSITION")?;let row=matches[0];
        let signed=row["quantity"].as_i64().filter(|n|*n!=0&&*n!=i64::MIN).ok_or("FUNDAMENTAL_ADOPTION_QUANTITY")?;let side=if signed>0{"YES"}else{"NO"};let opposite=if signed>0{"NO"}else{"YES"};let native_side=if signed>0{Side::Yes}else{Side::No};
        need(transport.scope().exchanges.get(exchange).is_some_and(|m|row["marketId"]==*m)&&bank.engine().wallet.quantity(exchange,native_side.other())==0,"FUNDAMENTAL_EXISTING_SCOPE")?;
        let quantity=bank.engine().wallet.quantity(exchange,native_side);need(quantity>0&&quantity<=signed.unsigned_abs(),"FUNDAMENTAL_EXISTING_QUANTITY")?;
        let mut original=Vec::new();for lot in row["lots"].as_array().ok_or("FUNDAMENTAL_ADOPTION_LOTS")?{let price=lot["entryPrice"].as_str().map(str::to_owned).unwrap_or_else(||lot["entryPrice"].to_string());original.push(json!({"quantity":lot["quantity"],"price":money(amount(&price)?)}));}let original=json!(original);
        let lots=fifo_remaining(&original,signed.unsigned_abs()-quantity)?;let positions=bank.engine().wallet.positions();need(positions[exchange][side]==lots&&positions[exchange][opposite].as_array().is_none_or(|a|a.is_empty()),"FUNDAMENTAL_EXISTING_FIFO_MISMATCH")?;
        let principal=money(bank.engine().wallet.market_cost(exchange));let approval=ExistingApproval{adoption_receipt_sha256:event["receiptHash"].as_str().ok_or("FUNDAMENTAL_ADOPTION_RECEIPT")?.into(),adopted_position_sha256:digest(row)?};
        let origin=ExistingOrigin{approval,adoption_ref:json!({"id":source_id,"sequence":sequence,"sha256":event_sha}),adopted_position:row.clone(),lots,quantity,principal,review_sha256:String::new()};
        need(queue_view(transport)?==queue,"FUNDAMENTAL_EXISTING_QUEUE_CHANGED")?;
        Ok(Self{origin,head:bank.durable_head(),report_sha:digest(&report)?,fill_count:bank.engine().fills().len(),fill_sha:digest(bank.engine().fills())?,dispatch:log.descriptor_identity()?,queue,scope_sha:digest(transport.scope())?,account_sha:bytes_sha(transport.account_scope().as_bytes())})
    }
}
pub struct ExistingExit{event:Value}
impl ExistingExit{pub fn event(&self)->&Value{&self.event}}
/// Read/preflight hint only. It grants no native reservation or HTTP authority.
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct ExistingExitHint{pub exchange_id:String,pub market_id:String,pub contract_id:String,pub side:String,pub reason:String}
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
    marker:FilePin,
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
fn read_file(file:&File,maximum:u64)->Result<Vec<u8>>{
    let n=file.metadata()?.len();need(n<=maximum,"FUNDAMENTAL_FILE_BOUND")?;
    let mut bytes=vec![0;usize::try_from(n)?];
    #[cfg(unix)]{use std::os::unix::fs::FileExt;file.read_exact_at(&mut bytes,0)?;}
    #[cfg(not(unix))]{return Err("FUNDAMENTAL_DESCRIPTOR_UNSUPPORTED".into())}
    need(file.metadata()?.len()==n,"FUNDAMENTAL_FILE_CHANGED")?;Ok(bytes)
}
struct FilePin{file:File,path:PathBuf,identity:Value,bytes:u64,sha:String,maximum:u64}
impl FilePin{
    fn from_held(file:File,path:PathBuf,maximum:u64,expected:Option<&str>)->Result<Self>{
        let identity=identity(&file)?;path_pin(&path,&identity)?;let bytes=read_file(&file,maximum)?;let sha=bytes_sha(&bytes);
        need(expected.is_none_or(|e|hex(e)&&e==sha),"FUNDAMENTAL_PINNED_FILE_HASH")?;
        Ok(Self{file,path,identity,bytes:bytes.len() as u64,sha,maximum})
    }
    fn open(path:&Path,expected:&str,maximum:u64)->Result<Self>{
        need(path.is_absolute(),"FUNDAMENTAL_ABSOLUTE_SOURCE_PATH_REQUIRED")?;
        Self::from_held(File::open(path)?,path.into(),maximum,Some(expected))
    }
    fn validate(&self)->Result<()>{path_pin(&self.path,&self.identity)?;
        need(self.file.metadata()?.len()==self.bytes&&bytes_sha(&read_file(&self.file,self.maximum)?)==self.sha,"FUNDAMENTAL_PINNED_FILE_CHANGED")
    }
    fn duplicate(&self)->Result<Self>{Self::from_held(self.file.try_clone()?,self.path.clone(),self.maximum,Some(&self.sha))}
}
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct AuthorityInput{version:u32,trusted_review_sha256:BTreeSet<String>}
/// Expected absolute path+SHA come from the separately pinned production Control.
/// This small immutable file approves canonical review hashes, not itself.
pub struct ReviewAuthority{pin:FilePin,trusted:BTreeSet<String>}
impl ReviewAuthority{
    pub fn load(path:&Path,expected_sha256:&str)->Result<Self>{
        let pin=FilePin::open(path,expected_sha256,64*1024)?;
        let config:AuthorityInput=serde_json::from_slice(&read_file(&pin.file,pin.maximum)?)?;
        need(config.version==1&&config.trusted_review_sha256.len()<=256&&config.trusted_review_sha256.iter().all(|s|hex(s)),"FUNDAMENTAL_TRUST_FILE")?;
        pin.validate()?;Ok(Self{pin,trusted:config.trusted_review_sha256})
    }
    pub fn trusted(&self)->&BTreeSet<String>{&self.trusted}
    pub fn validate(&self)->Result<()>{self.pin.validate()}
}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct SourceFile{pub path:PathBuf,pub sha256:String}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ReviewFile{pub path:PathBuf,pub file_sha256:String,pub source_files:Vec<SourceFile>}
/// Optional field in the separately pinned Control. All paths absolute; file
/// hashes pin immutable versioned files. Initialization is consumed by a durable
/// marker; leaving the flag true cannot recreate a lost deployed sidecar.
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Settings{pub version:u32,pub authority_path:PathBuf,pub authority_sha256:String,pub reviews:Vec<ReviewFile>,pub allow_initialization:bool}
pub struct LoadedSettings{authority:ReviewAuthority,reviews:Vec<Reviewed>,initialize:bool}
impl Settings{
    pub fn load(&self)->Result<LoadedSettings>{
        need(self.version==1&&(1..=32).contains(&self.reviews.len()),"FUNDAMENTAL_SETTINGS")?;
        let authority=ReviewAuthority::load(&self.authority_path,&self.authority_sha256)?;
        let mut reviews=Vec::new();let mut exchanges=BTreeSet::new();
        for r in &self.reviews{
            let pin=FilePin::open(&r.path,&r.file_sha256,64*1024)?;
            let input:ReviewInput=serde_json::from_slice(&read_file(&pin.file,pin.maximum)?)?;
            need(exchanges.insert(input.exchange_id.clone())&&r.source_files.len()==input.sources.len(),"FUNDAMENTAL_DUPLICATE_SETTING_OR_SOURCES")?;
            let mut data=Vec::new();let mut total=0usize;
            for s in &r.source_files{let p=FilePin::open(&s.path,&s.sha256,8*1024*1024)?;let bytes=read_file(&p.file,p.maximum)?;
                total=total.checked_add(bytes.len()).ok_or("FUNDAMENTAL_SOURCE_BOUND")?;need(total<=16*1024*1024,"FUNDAMENTAL_SOURCE_BOUND")?;p.validate()?;data.push(bytes);}
            let sources=r.source_files.iter().zip(&data).map(|(s,b)|SourceBytes{sha256:&s.sha256,bytes:b}).collect::<Vec<_>>();
            reviews.push(verify_snapshot(input,&sources,authority.trusted())?);pin.validate()?;
        }authority.validate()?;Ok(LoadedSettings{authority,reviews,initialize:self.allow_initialization})
    }
}
impl LoadedSettings{
    /// An expired snapshot may recover an already managed cohort; it cannot
    /// initialize a new policy or silently extend an old cohort's horizon.
    pub fn open(self,root:&Path,bank:&Session,log:&DispatchLog,owner:Arc<ExclusiveLock>,now:u64)->Result<(Cohorts,ReviewAuthority,Vec<String>)>{
        self.open_inner(root,bank,log,owner,now,None)
    }
    pub fn open_with_existing(self,root:&Path,bank:&Session,log:&DispatchLog,owner:Arc<ExclusiveLock>,now:u64,transport:&Transport,adoption_source_id:&str,adoption_sequence:u64,adoption_event:&Value)->Result<(Cohorts,ReviewAuthority,Vec<String>)>{
        self.open_inner(root,bank,log,owner,now,Some((transport,adoption_source_id,adoption_sequence,adoption_event)))
    }
    fn open_inner(self,root:&Path,bank:&Session,log:&DispatchLog,owner:Arc<ExclusiveLock>,now:u64,existing:Option<(&Transport,&str,u64,&Value)>)->Result<(Cohorts,ReviewAuthority,Vec<String>)>{
        let mut store=if root.join("fundamental.ndjson").exists(){Cohorts::open(root,bank,log,owner)?}
            else{need(self.initialize&&!root.join("fundamental.initialized.json").exists(),"FUNDAMENTAL_INITIALIZATION_NOT_APPROVED_OR_PREVIOUSLY_USED")?;Cohorts::initialize(root,bank,log,owner)?};
        for r in &self.reviews{need(current(&r.input,now)||store.manages(&r.input.exchange_id),"FUNDAMENTAL_FIRST_POLICY_STALE")?;}
        let mut stale=Vec::new();for r in self.reviews{if current(&r.input,now){
            if r.input.existing_origin.is_some()&&!store.manages(&r.input.exchange_id){let(t,id,n,event)=existing.ok_or("FUNDAMENTAL_EXISTING_ACCOUNT_BOUND_INIT_REQUIRED")?;let snapshot=ExistingSnapshot::capture(bank,log,t,id,n,event,&r.input.exchange_id)?;store.install_existing(r,snapshot,bank,log,t,now)?;}
            else{store.install(r,bank,log,now)?;}
        }else{stale.push(r.sha);}}
        self.authority.validate()?;Ok((store,self.authority,stale))
    }
}
/// Opaque supplemental policy capability. It is neither an intent, physical
/// attempt nor retry authority. No public constructor, Clone or Deserialize.
/// ProductionAuthority must call BOTH methods on its own frozen prepared data.
pub struct PhysicalDecision{
    qid:u64,planned_quantity:u64,native_head:Value,quote_sha:String,ledger_sha:String,scope_sha:String,
    spec_sha:String,permit_sha:String,review_sha:String,trust_sha:String,
    contract_market:String,contract_id:String,contract_tournament:String,contract_root_sha:String,
    minted:u64,expires:u64,sidecar:FilePin,marker:FilePin,sidecar_sequence:u64,sidecar_tail:String,
    authority:FilePin,ordinary_file:File,ordinary_path:PathBuf,ordinary_identity:Value,
    _owner:Arc<ExclusiveLock>,
}
impl PhysicalDecision{
    pub fn native_head(&self)->&Value{&self.native_head}
    pub fn planned_quantity(&self)->u64{self.planned_quantity}
    pub fn expires_at_ms(&self)->u64{self.expires}
    pub fn review_sha256(&self)->&str{&self.review_sha}
    pub fn validate_contract(&self,receipt:&Receipt)->Result<()>{
        need(receipt.status==200&&receipt.endpoint==format!("/markets/{}/nodes?tournamentId={}",self.contract_market,self.contract_tournament)
            &&receipt.data["market_id"]==self.contract_market&&receipt.data["root"]["node_type"]=="contract"
            &&receipt.data["root"]["node_id"]==self.contract_id&&receipt.data["root"]["contract_id"]==self.contract_id
            &&official_contract_root_sha256(&receipt.data)?==self.contract_root_sha,"FUNDAMENTAL_PHYSICAL_CONTRACT_CHANGED")
    }
    fn time(&self,now:u64)->Result<()>{need(now>=self.minted&&now<self.expires,"FUNDAMENTAL_PHYSICAL_TOKEN_EXPIRED_OR_REGRESSED")}
    pub fn validate_frozen(&self,quote:&Value,ledger:&Value,qid:u64,now:u64)->Result<()>{
        self.time(now)?;need(qid==self.qid&&quote["id"]==qid&&digest(quote)?==self.quote_sha&&digest(ledger)?==self.ledger_sha,"FUNDAMENTAL_PHYSICAL_FROZEN_STATE_CHANGED")
    }
    pub fn authorize(&self,scope:&Scope,spec:&RequestSpec,permit:&Permit,now:u64)->Result<()>{
        self.time(now)?;need(digest(scope)?==self.scope_sha&&digest(spec)?==self.spec_sha&&digest(permit)?==self.permit_sha,"FUNDAMENTAL_PHYSICAL_FROZEN_REQUEST_CHANGED")?;
        self.sidecar.validate()?;self.marker.validate()?;self.authority.validate()?;
        // The same original dispatch inode may append OUR intent/attempt after
        // mint. Its growing head is checked by actual Cohorts.check_bound after
        // those appends, not falsely frozen here before they exist.
        need(identity(&self.ordinary_file)?==self.ordinary_identity,"FUNDAMENTAL_ORIGINAL_DISPATCH_REQUIRED")?;
        path_pin(&self.ordinary_path,&self.ordinary_identity)?;
        let data:AuthorityInput=serde_json::from_slice(&read_file(&self.authority.file,self.authority.maximum)?)?;
        need(digest(&data.trusted_review_sha256)?==self.trust_sha,"FUNDAMENTAL_TRUST_CHANGED")?;
        let bytes=read_file(&self.sidecar.file,self.sidecar.maximum)?;let line=bytes.strip_suffix(b"\n").ok_or("FUNDAMENTAL_TORN_TAIL")?.rsplit(|b|*b==b'\n').next().ok_or("FUNDAMENTAL_TAIL")?;
        let mut row:Value=serde_json::from_slice(line)?;let claimed=row.as_object_mut().ok_or("FUNDAMENTAL_ROW")?.remove("hash").ok_or("FUNDAMENTAL_HASH")?;
        need(row["sequence"]==self.sidecar_sequence&&claimed==self.sidecar_tail&&claimed==digest(&row)?,"FUNDAMENTAL_PHYSICAL_TAIL_CHANGED")
    }
}
impl Cohorts{
    pub fn existing_exit_hint(&self,bank:&Session,log:&DispatchLog,scope:&Scope,exchange:&str,trusted:&BTreeSet<String>,now:u64)->Result<Option<ExistingExitHint>>{
        self.check(bank,log)?;log.require_resolved_attempts()?;
        let Some(c)=self.cohorts.get(exchange).filter(|c|c.existing.is_some())else{return Ok(None)};
        if self.owned(bank,exchange,c)?==0||c.exits.keys().any(|id|log.attempted.contains(id)&&!log.closed.contains(id)){return Ok(None)}
        let r=&self.policies[exchange];need(scope.tournament_id==r.tournament_id&&scope.exchanges.get(exchange)==Some(&r.market_id),"FUNDAMENTAL_EXISTING_SCOPE")?;
        let reason=if now>=c.hold_until_ms.min(r.hold_until_ms){"reviewed_holding_deadline"}
            else if !trusted.contains(&digest(r)?)||!current(r,now){return Ok(None)}
            else if r.revoke_existing{"reviewed_source_revocation"}
            else{let(books,_)=package_admission::native_books(bank,scope,now,bank.engine().policy()["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?)?;
                if let Some(book)=books.get(exchange){let levels=if r.side=="YES"{&book.yes_bids}else{&book.no_bids};let payout=add(structure(r)?,amount(&r.margin_per_share)?)?.max(0);
                    if levels.first().is_none_or(|l|l.price<payout){return Ok(None)}"refreshed_value_exit"
                }else{"reviewed_inventory_book_refresh"}};
        Ok(Some(ExistingExitHint{exchange_id:exchange.into(),market_id:r.market_id.clone(),contract_id:r.contract_id.clone(),side:r.side.clone(),reason:reason.into()}))
    }
    pub fn install_existing(&mut self,r:Reviewed,mut snapshot:ExistingSnapshot,bank:&Session,log:&DispatchLog,transport:&Transport,now:u64)->Result<()>{
        self.check(bank,log)?;let ex=&r.input.exchange_id;no_existing_liability(bank,log,ex)?;
        need(!self.policies.contains_key(ex)&&!self.cohorts.contains_key(ex)&&r.input.existing_origin.as_ref()==Some(&snapshot.origin.approval)
            &&r.input.run_id==self.run&&r.input.bank_manifest_sha256==self.manifest&&current(&r.input,now)
            &&snapshot.head==bank.durable_head()&&snapshot.report_sha==digest(&bank.engine().report()?)?&&snapshot.fill_count==bank.engine().fills().len()&&snapshot.fill_sha==digest(bank.engine().fills())?
            &&snapshot.dispatch==log.descriptor_identity()?&&snapshot.queue==queue_view(transport)?&&snapshot.scope_sha==digest(transport.scope())?&&snapshot.account_sha==bytes_sha(transport.account_scope().as_bytes())
            &&snapshot.origin.adopted_position["exchangeId"]==*ex&&snapshot.origin.adopted_position["marketId"]==r.input.market_id&&transport.scope().tournament_id==r.input.tournament_id
            &&snapshot.origin.adopted_position["quantity"].as_i64().is_some_and(|n|if n<0{r.input.side=="NO"}else{r.input.side=="YES"})
            &&snapshot.origin.quantity<=r.input.maximum_quantity&&amount(&snapshot.origin.principal)?<=amount(&r.input.maximum_principal)?,"FUNDAMENTAL_EXISTING_SNAPSHOT_OR_REVIEW_CHANGED")?;
        snapshot.origin.review_sha256=r.sha.clone();let c=Cohort{entry:None,existing:Some(snapshot.origin),first_fill_index:snapshot.fill_count,prior_fills_sha256:snapshot.fill_sha,bank_head_at_entry:snapshot.head,hold_until_ms:r.input.hold_until_ms,exits:BTreeMap::new()};
        self.append(json!({"kind":"existing_origin","review":r.input,"sha256":r.sha,"cohort":c}),now)
    }
    /// Proposes a genuine covered native SELL; this is not an API request or an
    /// execution. Retire proved never-sent drafts first, accept this event once,
    /// then bind its actual native qid/spec and mint the normal opaque token.
    pub fn prepare_existing_exit(&self,bank:&Session,log:&DispatchLog,transport:&Transport,scope:&Scope,exchange:&str,trusted:&BTreeSet<String>,now:u64)->Result<Option<ExistingExit>>{
        self.check(bank,log)?;let queue=queue_view(transport)?;let Some(c)=self.cohorts.get(exchange)else{return Ok(None)};let Some(origin)=&c.existing else{return Ok(None)};
        let r=&self.policies[exchange];let owned=self.owned(bank,exchange,c)?;if owned==0{return Ok(None)}
        need(digest(scope)?==digest(transport.scope())?&&scope.tournament_id==r.tournament_id&&scope.exchanges.get(exchange)==Some(&r.market_id),"FUNDAMENTAL_EXISTING_SCOPE")?;
        no_existing_liability(bank,log,exchange)?;
        let deadline=c.hold_until_ms.min(r.hold_until_ms);let approved=trusted.contains(&digest(r)?)&&current(r,now);let payout=add(structure(r)?,amount(&r.margin_per_share)?)?.max(0);
        let reason=if now>=deadline{"reviewed_holding_deadline"}else if approved&&r.revoke_existing{"reviewed_source_revocation"}else if approved{"refreshed_value_exit"}else{return Ok(None)};
        let policy=bank.engine().policy();let (books,hashes)=package_admission::native_books(bank,scope,now,policy["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?)?;let Some(book)=books.get(exchange)else{return Ok(None)};
        let cap=owned.min(r.maximum_quantity).min(policy["quantity"].as_u64().ok_or("FUNDAMENTAL_QUANTITY")?);let levels=if r.side=="YES"{&book.yes_bids}else{&book.no_bids};let mut n=0u64;let mut limit=None;
        for l in levels{if reason=="refreshed_value_exit"&&l.price<payout{break}let take=(cap-n).min(l.quantity);if take>0{n+=take;limit=Some(l.price)}if n==cap{break}}
        let Some(limit)=limit else{return Ok(None)};need(n>0,"FUNDAMENTAL_EXISTING_DEPTH")?;
        let until=r.sources.iter().try_fold(r.expires_at_ms,|end,s|Ok::<_,Box<dyn std::error::Error>>(end.min(s.observed_at_ms.checked_add(s.maximum_age_ms).ok_or("FUNDAMENTAL_SOURCE_CLOCK_RANGE")?)))?;
        let mut expires=now.checked_add(policy["lifetimeMs"].as_u64().ok_or("FUNDAMENTAL_LIFETIME")?).ok_or("FUNDAMENTAL_CLOCK_RANGE")?;
        if reason!="reviewed_holding_deadline"{expires=expires.min(until).min(deadline);}
        if expires<=now.checked_add(policy["latencyMs"].as_u64().ok_or("FUNDAMENTAL_LATENCY")?).ok_or("FUNDAMENTAL_CLOCK_RANGE")?{return Ok(None)}
        let event=json!({"type":"reviewed_inventory_exit_reserved_v1","source":"moloch_live_dispatcher","receivedAt":daily_windows::iso(now)?,"bankScope":{"runId":self.run,"bankManifestHash":self.manifest,"tournamentId":r.tournament_id},"bankHead":bank.durable_head(),"expectedReportSha256":digest(&bank.engine().report()?)?,"exchangeId":exchange,"marketId":r.market_id,"side":r.side,"quantity":n,"limitPrice":money(limit),"expiresAt":daily_windows::iso(expires)?,"originSha256":c.origin_sha()?,"reviewSha256":digest(r)?,"adoptionRef":origin.adoption_ref,"adoptionReceiptHash":origin.approval.adoption_receipt_sha256,"expectedPositionSha256":digest(&bank.engine().wallet.positions()[exchange])?,"bookSha256":hashes.get(exchange).ok_or("FUNDAMENTAL_BOOK_HASH")?,"reason":reason,"expectedPayout":money(payout),"reviewCurrentUntilMs":until,"holdingDeadlineMs":deadline,"revoked":r.revoke_existing});
        need(queue_view(transport)?==queue,"FUNDAMENTAL_EXISTING_QUEUE_CHANGED")?;Ok(Some(ExistingExit{event}))
    }
    /// Mint after durable bind and before the original intent/attempt. After
    /// those original appends the worker MUST re-run check_bound, then the SDK
    /// performs its existing account/unknown/rate/physical authorization gates.
    pub fn physical_decision(&self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,spec:&RequestSpec,permit:&Permit,authority:&ReviewAuthority,now:u64)->Result<PhysicalDecision>{
        authority.validate()?;self.check_bound(bank,log,scope,qid,spec,authority.trusted(),now)?;
        let (ex,c)=self.cohorts.iter().find(|(_,c)|c.binding(qid).is_some()).ok_or("FUNDAMENTAL_DURABLE_BINDING_REQUIRED")?;
        let r=&self.policies[ex];let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("FUNDAMENTAL_QUOTE")?;
        let n=request_quantity(spec)?;let price=amount(q["price"].as_str().ok_or("FUNDAMENTAL_PRICE")?)?;
        need(q["retired"]==false&&q["activationAt"].as_u64().is_some_and(|t|t<=now)&&!q["cancelEffectiveAt"].is_u64()
            &&permit.owner=="Codex"&&permit.run_id==self.run&&permit.intent_id==qid.to_string()
            &&u64::from(permit.maximum_quantity)>=n&&amount(&permit.maximum_cost)?>=mul(price,n as i128)?,"FUNDAMENTAL_PHYSICAL_PERMIT_OR_NATIVE_WINDOW")?;
        let age=bank.engine().policy()["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?;
        let(books,_)=package_admission::native_books(bank,scope,now,age)?;let book=books.get(ex).ok_or("FUNDAMENTAL_PHYSICAL_BOOK_STALE")?;
        let mut expires=q["expiresAt"].as_u64().ok_or("FUNDAMENTAL_EXPIRATION")?.min(permit.expires_at_ms)
            .min(book.observed_ms.checked_add(age).and_then(|n|n.checked_add(1)).ok_or("FUNDAMENTAL_CLOCK_RANGE")?)
            .min(book.source.received_ms.checked_add(age).and_then(|n|n.checked_add(1)).ok_or("FUNDAMENTAL_CLOCK_RANGE")?);
        let deadline_exit=if c.entry_id()==Some(qid){false}else{
            matches!(self.assess(bank,log,scope,qid,authority.trusted(),now)?,Decision::Exit{reason,..}if reason=="reviewed_holding_deadline")
        };
        if !deadline_exit{
            expires=expires.min(r.expires_at_ms).min(r.hold_until_ms);
            for s in &r.sources{expires=expires.min(s.observed_at_ms.checked_add(s.maximum_age_ms).and_then(|n|n.checked_add(1)).ok_or("FUNDAMENTAL_CLOCK_RANGE")?);}
        }else{need(now>=c.hold_until_ms.min(r.hold_until_ms),"FUNDAMENTAL_DEADLINE_NOT_REACHED")?;}
        need(now>=self.clock&&expires>now,"FUNDAMENTAL_PHYSICAL_WINDOW_EMPTY")?;
        let ordinary_file=File::open(&self.ordinary_path)?;need(identity(&ordinary_file)?==self.ordinary_identity,"FUNDAMENTAL_ORIGINAL_DISPATCH_REQUIRED")?;
        Ok(PhysicalDecision{qid,planned_quantity:n,native_head:bank.durable_head(),quote_sha:digest(&q)?,ledger_sha:digest(&bank.engine().report()?)?,scope_sha:digest(scope)?,spec_sha:digest(spec)?,permit_sha:digest(permit)?,review_sha:digest(r)?,trust_sha:digest(authority.trusted())?,minted:now,expires,
            contract_market:r.market_id.clone(),contract_id:r.contract_id.clone(),contract_tournament:r.tournament_id.clone(),contract_root_sha:r.official_contract_root_sha256.clone(),
            sidecar:FilePin::from_held(self.file.try_clone()?,self.path.clone(),MAX_LOG,None)?,marker:self.marker.duplicate()?,sidecar_sequence:self.sequence,sidecar_tail:self.previous.clone(),authority:authority.pin.duplicate()?,ordinary_file,ordinary_path:self.ordinary_path.clone(),ordinary_identity:self.ordinary_identity.clone(),_owner:self._owner.clone()})
    }
    /// Explicit one-time owner operation; never invoked as missing-file recovery.
    pub fn initialize(root:&Path,bank:&Session,ordinary:&DispatchLog,owner:Arc<ExclusiveLock>)->Result<Self>{
        need(root.is_absolute()&&!fs::symlink_metadata(root)?.file_type().is_symlink(),"FUNDAMENTAL_ROOT")?;
        owner.descriptor_identity()?;let d=ordinary.descriptor_identity()?;
        path_pin(&root.join("dispatch.ndjson"),&json!({"device":d["device"],"inode":d["inode"]}))?;
        let marker_path=root.join("fundamental.initialized.json");need(!marker_path.exists(),"FUNDAMENTAL_INITIALIZATION_PREVIOUSLY_USED")?;
        let path=root.join("fundamental.ndjson");
        let mut options=OpenOptions::new();options.write(true).create_new(true);
        #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
        let f=options.open(path)?;f.sync_all()?;
        let mut body=json!({"version":1,"bankManifestHash":bank.manifest_hash(),"runId":bank.live_account_seed().ok_or("FUNDAMENTAL_LIVE_BANK")?["runId"],"sidecarIdentity":identity(&f)?});body["hash"]=json!(digest(&body)?);
        let mut marker=options.open(marker_path)?;marker.write_all(&serde_json::to_vec(&body)?)?;marker.write_all(b"\n")?;marker.sync_all()?;File::open(root)?.sync_all()?;
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
        let marker_path=root.join("fundamental.initialized.json");let marker_file=File::open(&marker_path)?;
        let marker=FilePin::from_held(marker_file,marker_path,4096,None)?;let mut mark:Value=serde_json::from_slice(&read_file(&marker.file,4096)?)?;
        let mark_sha=mark.as_object_mut().ok_or("FUNDAMENTAL_INITIALIZATION_MARKER")?.remove("hash").ok_or("FUNDAMENTAL_INITIALIZATION_MARKER")?;
        need(mark["version"]==1&&mark_sha==digest(&mark)?&&mark["bankManifestHash"]==*bank.manifest_hash()
            &&mark["runId"]==bank.live_account_seed().ok_or("FUNDAMENTAL_LIVE_BANK")?["runId"]&&mark["sidecarIdentity"]==identity(&file)?,"FUNDAMENTAL_INITIALIZATION_MARKER")?;
        let mut out=Self{file,path,ordinary_path,ordinary_identity,_owner:owner,
            marker,
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
                if let Some(old)=self.policies.get(&r.exchange_id){need(old.market_id==r.market_id&&old.contract_id==r.contract_id&&old.tournament_id==r.tournament_id&&old.side==r.side&&old.existing_origin==r.existing_origin&&r.reviewed_at_ms>old.reviewed_at_ms,"FUNDAMENTAL_REVIEW_REVISION")?;}
                need(self.policies.contains_key(&r.exchange_id)||self.policies.len()<32,"FUNDAMENTAL_POLICY_COUNT")?;
                if let Some(c)=self.cohorts.get(&r.exchange_id){need(r.hold_until_ms<=c.hold_until_ms,"FUNDAMENTAL_CANNOT_EXTEND_COHORT")?;}
                self.policies.insert(r.exchange_id.clone(),r);
            },
            Some("entry")=>{let ex=event["exchangeId"].as_str().ok_or("FUNDAMENTAL_EXCHANGE")?;let c:Cohort=serde_json::from_value(event["cohort"].clone())?;
                need(self.policies.contains_key(ex)&&!self.cohorts.contains_key(ex)&&c.exits.is_empty()&&c.entry.is_some()&&c.existing.is_none(),"FUNDAMENTAL_EXCLUSIVE_COHORT")?;self.cohorts.insert(ex.into(),c);},
            Some("existing_origin")=>{
                let r:ReviewInput=serde_json::from_value(event["review"].clone())?;structure(&r)?;let ex=r.exchange_id.clone();let c:Cohort=serde_json::from_value(event["cohort"].clone())?;
                need(r.run_id==self.run&&r.bank_manifest_sha256==self.manifest&&event["sha256"]==digest(&r)?&&!self.policies.contains_key(&ex)&&!self.cohorts.contains_key(&ex)
                    &&c.entry.is_none()&&c.exits.is_empty()&&c.existing.as_ref().is_some_and(|o|r.existing_origin.as_ref()==Some(&o.approval)),"FUNDAMENTAL_EXISTING_ORIGIN_EVENT")?;
                self.policies.insert(ex.clone(),r);self.cohorts.insert(ex,c);
            },
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
        self.marker.validate()?;
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
            need(c.entry.is_some()!=c.existing.is_some(),"FUNDAMENTAL_ORIGIN_VARIANT")?;
            for b in c.entry.iter().chain(c.exits.values()){
                need(digest(&b.spec)?==b.spec_sha256,"FUNDAMENTAL_BINDING_HASH")?;
                let n=request_quantity(&b.spec)?;request_matches(&b.spec,&b.quote,&self.run,&self.policies[ex].tournament_id,n)?;
                if let Some(original)=log.intents.get(&b.qid){let s:RequestSpec=serde_json::from_value(original["spec"].clone())?;need(digest(&s)?==b.spec_sha256&&original.get("fundamental")==self.durable_tag(b.qid).as_ref(),"FUNDAMENTAL_DISPATCH_REQUEST_OR_TAG_CHANGED")?;}
                else{need(log.never_attempted(b.qid),"FUNDAMENTAL_MISSING_ORIGINAL_INTENT")?;}
                let q=bank.engine().order_records().into_iter().find(|q|q["id"]==b.qid).ok_or("FUNDAMENTAL_NATIVE_QUOTE_MISSING")?;
                let expected=if bank.engine().live_order_bindings().contains_key(&b.qid){n}else{b.quote["quantity"].as_u64().ok_or("FUNDAMENTAL_QUANTITY")?};
                need(q["exchangeId"]==*ex&&q["side"]==b.quote["side"]&&q["marketId"]==b.quote["marketId"]&&q["action"]==b.quote["action"]&&q["price"]==b.quote["price"]&&q["quantity"].as_u64()==Some(expected),"FUNDAMENTAL_NATIVE_BINDING_CHANGED")?;
            }
            self.owned(bank,ex,c)?;
        }Ok(())
    }
    fn owned(&self,bank:&Session,ex:&str,c:&Cohort)->Result<u64>{
        let r=self.policies.get(ex).ok_or("FUNDAMENTAL_REVIEW_MISSING")?;let mut held=c.existing.as_ref().map_or(0,|o|o.quantity);
        need(c.first_fill_index<=bank.engine().fills().len(),"FUNDAMENTAL_BANK_PREFIX_MISSING")?;
        need(digest(&bank.engine().fills()[..c.first_fill_index])?==c.prior_fills_sha256
            &&bank.durable_head()["sequence"].as_u64()>=c.bank_head_at_entry["sequence"].as_u64(),"FUNDAMENTAL_FILL_PREFIX_CHANGED")?;
        for f in &bank.engine().fills()[c.first_fill_index..]{if f["exchangeId"]!=ex{continue}
            let id=f["orderId"].as_u64().ok_or("FUNDAMENTAL_FILL_ID")?;let n=f["quantity"].as_u64().ok_or("FUNDAMENTAL_FILL_QUANTITY")?;
            need(f["side"]==r.side&&f["counterfactual"]==false,"FUNDAMENTAL_FOREIGN_FILL")?;
            if c.entry_id()==Some(id)&&f["action"]=="buy"{held=held.checked_add(n).ok_or("FUNDAMENTAL_RANGE")?;}
            else if c.exits.contains_key(&id)&&f["action"]=="sell"{held=held.checked_sub(n).ok_or("FUNDAMENTAL_EXIT_EXCEEDS_COHORT")?;}
            else{return Err("FUNDAMENTAL_FOREIGN_FILL".into())}
        }
        let side=if r.side=="YES"{Side::Yes}else{Side::No};let opposite=if r.side=="YES"{Side::No}else{Side::Yes};
        need(bank.engine().wallet.quantity(ex,side)==held&&bank.engine().wallet.quantity(ex,opposite)==0,"FUNDAMENTAL_UNATTRIBUTED_HOLDING")?;
        if let Some(origin)=&c.existing{let remaining=fifo_remaining(&origin.lots,origin.quantity-held)?;
            need(r.existing_origin.as_ref()==Some(&origin.approval)&&digest(&origin.adopted_position)?==origin.approval.adopted_position_sha256
                &&bank.committed_source_hash(origin.adoption_ref["id"].as_str().ok_or("FUNDAMENTAL_ADOPTION_REF")?,origin.adoption_ref["sequence"].as_u64().ok_or("FUNDAMENTAL_ADOPTION_REF")?)==origin.adoption_ref["sha256"].as_str()
                &&bank.engine().wallet.positions()[ex][&r.side]==remaining,"FUNDAMENTAL_EXISTING_FIFO_CHANGED")?;}
        Ok(held)
    }
    pub fn manages(&self,exchange:&str)->bool{self.policies.contains_key(exchange)}
    pub fn managed_exchanges(&self)->BTreeSet<String>{self.policies.keys().cloned().collect()}
    /// Original POST and recovered-GET events pass here BEFORE native accept.
    /// The original quote's larger reservation is not proof of slice size.
    pub fn validate_acknowledgment(&self,qid:u64,event:&Value)->Result<()>{
        let binding=self.cohorts.values().find_map(|c|c.binding(qid));
        let Some(b)=binding else{return need(event["exchangeId"].as_str().is_none_or(|ex|!self.manages(ex)),"FUNDAMENTAL_ACK_WITHOUT_BINDING")};
        need(event["type"]=="actual_order_ack"&&event["source"]=="official_sig_api_v1"&&event["orderId"]==qid
            &&event["quantity"].as_u64()==Some(request_quantity(&b.spec)?)&&event["exchangeId"]==b.quote["exchangeId"]
            &&event["marketId"]==b.quote["marketId"]&&event["side"]==b.quote["side"]&&event["action"]==b.quote["action"]&&event["price"]==b.quote["price"],"FUNDAMENTAL_ACK_ADMISSION_MISMATCH")
    }
    /// An expired native quote is not a submit candidate, so `assess` cannot be
    /// used for the expired-quote read hint. This separate gate permits bounded
    /// refresh at the fixed deadline, reviewed revocation/revision, or an already
    /// observed executable value exit; final `assess` must still pass afterward.
    pub fn exit_refresh_needed(&self,bank:&Session,log:&DispatchLog,scope:&Scope,exchange:&str,trusted:&BTreeSet<String>,now:u64)->Result<bool>{
        self.check(bank,log)?;let Some(r)=self.policies.get(exchange)else{return Ok(true)};
        let Some(c)=self.cohorts.get(exchange)else{return Ok(false)};
        if self.owned(bank,exchange,c)?==0||c.pending_entry(log)
            ||c.exits.keys().any(|id|log.attempted.contains(id)&&!log.closed.contains(id)){return Ok(false)}
        need(scope.tournament_id==r.tournament_id&&scope.exchanges.get(exchange)==Some(&r.market_id),"FUNDAMENTAL_SCOPE_CHANGED")?;
        if now>=c.hold_until_ms.min(r.hold_until_ms){return Ok(true)}
        if !trusted.contains(&digest(r)?)||!current(r,now){return Ok(false)}
        if r.revoke_existing||c.entry.as_ref().is_some_and(|b|digest(r).is_ok_and(|h|h!=b.review_sha256)){return Ok(true)}
        let(books,_)=package_admission::native_books(bank,scope,now,bank.engine().policy()["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?)?;
        let value=add(structure(r)?,amount(&r.margin_per_share)?)?;
        Ok(books.get(exchange).and_then(|b|if r.side=="YES"{b.yes_bids.first()}else{b.no_bids.first()}).is_some_and(|l|l.price>=value))
    }
    pub fn durable_tag(&self,qid:u64)->Option<Value>{
        for c in self.cohorts.values(){if let Some(b)=c.binding(qid){
            let mut tag=json!({"version":1,"bankManifestHash":self.manifest,"runId":self.run,"qid":qid,"bindingSha256":digest(b).ok()?});
            if let Some(entry)=c.entry_id(){tag["entryQid"]=json!(entry)}else{tag["originKind"]=json!("reviewed_existing_snapshot");tag["originSha256"]=json!(c.origin_sha().ok()?)}return Some(tag);
        }}None
    }
    pub fn install(&mut self,r:Reviewed,bank:&Session,log:&DispatchLog,now:u64)->Result<()>{
        self.check(bank,log)?;need(r.input.run_id==self.run&&r.input.bank_manifest_sha256==self.manifest,"FUNDAMENTAL_REVIEW_BANK")?;
        if let Some(c)=self.cohorts.get(&r.input.exchange_id).filter(|c|c.existing.is_some()){
            need(r.input.existing_origin.as_ref()==c.existing.as_ref().map(|o|&o.approval)&&self.owned(bank,&r.input.exchange_id,c)?<=r.input.maximum_quantity
                &&bank.engine().wallet.market_cost(&r.input.exchange_id)<=amount(&r.input.maximum_principal)?,"FUNDAMENTAL_EXISTING_REVIEW_CAP_OR_ORIGIN")?;
        }
        if let Some(old)=self.policies.get(&r.input.exchange_id){if digest(old)?==r.sha{return Ok(())}}
        else{
            need(r.input.existing_origin.is_none(),"FUNDAMENTAL_EXISTING_REQUIRES_BOUND_SNAPSHOT")?;
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
        let n=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).and_then(|q|q["quantity"].as_u64()).ok_or("FUNDAMENTAL_QUANTITY")?;
        self.assess_quantity(bank,log,scope,qid,n,trusted,now)
    }
    /// Explicit one-shot planned size. Only new entries may be smaller than the
    /// immutable native draft; covered exits retain their full native quantity.
    pub fn assess_quantity(&self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,planned_quantity:u64,trusted:&BTreeSet<String>,now:u64)->Result<Decision>{
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
        let price=amount(q["price"].as_str().ok_or("FUNDAMENTAL_PRICE")?)?;let native=q["quantity"].as_u64().filter(|n|*n>0).ok_or("FUNDAMENTAL_QUANTITY")?;
        need(planned_quantity>0&&planned_quantity<=native,"FUNDAMENTAL_PLANNED_QUANTITY")?;let n=planned_quantity;
        if let Some(c)=self.cohorts.get(ex){
            let owned=self.owned(bank,ex,c)?;
            if c.entry_id()==Some(qid)&&log.never_attempted(qid)&&!log.intents.contains_key(&qid)&&owned==0{
                let entry=c.entry.as_ref().unwrap();need(n==request_quantity(&entry.spec)?&&q==entry.quote,"FUNDAMENTAL_BOUND_ENTRY_CANNOT_CHANGE")?;
                if digest(r)?!=entry.review_sha256{return Ok(hold("bound_entry_review_changed"))}
                // Continue only this exact sealed draft after a crash/stale
                // preflight BEFORE original intent/attempt; never replace it.
            }else{
            if q["action"]!="sell"||q["side"]!=r.side||owned==0{return Ok(hold("exclusive_cohort_no_additional_or_opposite_entry"))}
            need(n==native,"FUNDAMENTAL_EXIT_MUST_MATCH_NATIVE_QUANTITY")?;
            if c.pending_entry(log)||c.exits.keys().any(|id|*id!=qid&&log.attempted.contains(id)&&!log.closed.contains(id)){return Ok(hold("prior_owned_order_reconciliation_required"))}
            need(n<=owned,"FUNDAMENTAL_UNCOVERED_EXIT")?;
            // Verify actual depth, not the last print or nominal quote price.
            let levels=if r.side=="YES"{&book.yes_bids}else{&book.no_bids};
            let mut left=n;let mut proceeds=0i128;
            for l in levels{if l.price<price{break}let take=left.min(l.quantity);proceeds=add(proceeds,mul(l.price,take as i128)?)?;left-=take;if left==0{break}}
            if left!=0{return Ok(hold("covered_exit_not_executable_at_native_limit"))}
            // Depth is an observation, not a guaranteed execution price. A
            // value exit must retain that value in its physical native limit.
            let payout=add(ceiling,amount(&r.margin_per_share)?)?.max(0);
            let reason=if now>=c.hold_until_ms.min(r.hold_until_ms){Some("reviewed_holding_deadline")}
                else if approved&&r.revoke_existing{Some("reviewed_source_revocation")}
                else if approved&&price>=payout&&proceeds>=mul(payout,n as i128)?{Some("refreshed_value_exit")}
                else{None};
            if c.existing.is_some(){need(q["reviewedInventoryExit"]["originSha256"]==c.origin_sha()?&&q["reviewedInventoryExit"]["reviewSha256"]==digest(r)?,"FUNDAMENTAL_EXISTING_EXIT_REQUIRES_NATIVE_RESERVATION")?;}
            return Ok(reason.map_or_else(||hold(if approved{"terminal_value_hold_not_native_age_exit"}else{"stale_review_hold_until_deadline"}),|s|Decision::Exit{reason:s.into(),owned_quantity:owned}))
            }
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
    /// Advisory size becomes authority only after explicit bind_quantity seals
    /// that exact number in the original RequestSpec. No price/quote mutation.
    pub fn planned_entry_quantity(&self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,trusted:&BTreeSet<String>,now:u64)->Result<Option<u64>>{
        self.check(bank,log)?;
        let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("FUNDAMENTAL_QUOTE")?;
        let ex=q["exchangeId"].as_str().ok_or("FUNDAMENTAL_EXCHANGE")?;
        let Some(r)=self.policies.get(ex)else{return Ok(None)};
        if q["action"]!="buy"||q["side"]!=r.side{return Ok(None)}
        if let Some(c)=self.cohorts.get(ex){
            if c.entry_id()!=Some(qid)||!log.never_attempted(qid)||log.intents.contains_key(&qid){return Ok(None)}
            let n=request_quantity(&c.entry.as_ref().unwrap().spec)?;
            return Ok(if matches!(self.assess_quantity(bank,log,scope,qid,n,trusted,now)?,Decision::Entry{..}){Some(n)}else{None})
        }
        let price=amount(q["price"].as_str().ok_or("FUNDAMENTAL_PRICE")?)?;
        let mut n=q["quantity"].as_u64().ok_or("FUNDAMENTAL_QUANTITY")?.min(r.maximum_quantity);
        if price>0{n=n.min(u64::try_from((amount(&r.maximum_principal)?/price).min(u64::MAX as i128))?);}
        if n==0{return Ok(None)}
        Ok(if matches!(self.assess_quantity(bank,log,scope,qid,n,trusted,now)?,Decision::Entry{..}){Some(n)}else{None})
    }
    /// Qids with known actual ACKs only. Queue/original-log unknowns remain on
    /// their original recovery path; never invent an actual ID to cancel.
    pub fn entry_cancellations(&self,bank:&Session,log:&DispatchLog,trusted:&BTreeSet<String>,now:u64)->Result<Vec<u64>>{
        self.check(bank,log)?;let mut out=Vec::new();
        for(ex,c)in &self.cohorts{let r=&self.policies[ex];
            let Some(entry)=&c.entry else{continue};
            if !log.closed.contains(&entry.qid)&&log.acknowledgments.contains_key(&entry.qid)
                &&bank.engine().order_records().iter().any(|q|q["id"]==entry.qid&&q["retired"]==false)
                &&(!current(r,now)||r.revoke_existing||!trusted.contains(&digest(r)?)||digest(r)?!=entry.review_sha256||now>=c.hold_until_ms.min(r.hold_until_ms)){
                out.push(entry.qid);
            }
        }Ok(out)
    }
    /// Revised/stale value assumptions also retire known still-open covered
    /// SELLs through the original cancel path; never invent unknown actual IDs.
    pub fn existing_exit_cancellations(&self,bank:&Session,log:&DispatchLog,trusted:&BTreeSet<String>,now:u64)->Result<Vec<u64>>{
        self.check(bank,log)?;let mut out=Vec::new();for(ex,c)in &self.cohorts{if c.existing.is_none(){continue}let r=&self.policies[ex];
            for b in c.exits.values(){if log.acknowledgments.contains_key(&b.qid)&&!log.closed.contains(&b.qid)
                &&bank.engine().order_records().iter().any(|q|q["id"]==b.qid&&q["retired"]==false)
                &&(digest(r)?!=b.review_sha256||((!trusted.contains(&digest(r)?)||!current(r,now))&&b.quote["reviewedInventoryExit"]["reason"]!="reviewed_holding_deadline")){out.push(b.qid);}}
        }Ok(out)
    }
    /// Persist exact original request BEFORE any physical attempt, never after
    /// observing a fill. No partial-size substitution or guessed ACK is allowed.
    pub fn bind(&mut self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,spec:&RequestSpec,trusted:&BTreeSet<String>,now:u64)->Result<Decision>{
        let n=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).and_then(|q|q["quantity"].as_u64()).ok_or("FUNDAMENTAL_QUANTITY")?;
        self.bind_quantity(bank,log,scope,qid,n,spec,trusted,now)
    }
    pub fn bind_quantity(&mut self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,planned_quantity:u64,spec:&RequestSpec,trusted:&BTreeSet<String>,now:u64)->Result<Decision>{
        let decision=self.assess_quantity(bank,log,scope,qid,planned_quantity,trusted,now)?;
        need(matches!(decision,Decision::Entry{..}|Decision::Exit{..}),"FUNDAMENTAL_NOT_ELIGIBLE")?;
        need(log.never_attempted(qid)&&!log.intents.contains_key(&qid),"FUNDAMENTAL_BIND_MUST_PRECEDE_DISPATCH")?;
        let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("FUNDAMENTAL_QUOTE")?;let ex=q["exchangeId"].as_str().unwrap();let r=&self.policies[ex];
        request_matches(spec,&q,&self.run,&r.tournament_id,planned_quantity)?;
        if let Some(old)=self.cohorts.get(ex).and_then(|c|c.binding(qid)){
            need(old.spec_sha256==digest(spec)?&&old.quote==q&&old.review_sha256==digest(r)?
                &&request_quantity(&old.spec)?==planned_quantity,"FUNDAMENTAL_BOUND_REQUEST_CANNOT_CHANGE")?;
            return Ok(decision)
        }
        let b=Binding{qid,spec:spec.clone(),spec_sha256:digest(spec)?,quote:q.clone(),review_sha256:digest(r)?,bound_at_ms:now};
        if matches!(decision,Decision::Entry{..}){let c=Cohort{entry:Some(b),existing:None,first_fill_index:bank.engine().fills().len(),prior_fills_sha256:digest(&bank.engine().fills())?,bank_head_at_entry:bank.durable_head(),hold_until_ms:r.hold_until_ms,exits:BTreeMap::new()};self.append(json!({"kind":"entry","exchangeId":ex,"cohort":c}),now)?;}
        else{self.append(json!({"kind":"exit","exchangeId":ex,"binding":b}),now)?;}
        Ok(decision)
    }
    /// Policy-only physical gate. An existing attempt grants NO retry authority.
    /// The SDK/original dispatch retain unknown-acceptance and rate controls.
    pub fn check_bound(&self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,spec:&RequestSpec,trusted:&BTreeSet<String>,now:u64)->Result<()>{
        self.check(bank,log)?;
        let(ex,c)=self.cohorts.iter().find(|(_,c)|c.binding(qid).is_some()).ok_or("FUNDAMENTAL_DURABLE_BINDING_REQUIRED")?;
        let b=c.binding(qid).unwrap();need(digest(spec)?==b.spec_sha256,"FUNDAMENTAL_PHYSICAL_REQUEST_CHANGED")?;
        if c.entry_id()==Some(qid){
            let r=&self.policies[ex];need(trusted.contains(&digest(r)?)&&digest(r)?==b.review_sha256&&current(r,now)&&!r.revoke_existing&&now<c.hold_until_ms,"FUNDAMENTAL_PHYSICAL_REVIEW_CHANGED")?;
            need(self.owned(bank,ex,c)?==0&&log.attempted.iter().all(|id|*id==qid||log.acknowledgments.contains_key(id)),"FUNDAMENTAL_PHYSICAL_ENTRY_OWNERSHIP")?;
            need(scope.tournament_id==r.tournament_id&&scope.exchanges.get(ex)==Some(&r.market_id)
                &&now<daily_windows::clock(bank.engine().policy()["entryStopAt"].as_str().ok_or("FUNDAMENTAL_ENTRY_DEADLINE")?)?,"FUNDAMENTAL_PHYSICAL_SCOPE_OR_DEADLINE")?;
            let q=bank.engine().order_records().into_iter().find(|q|q["id"]==qid).ok_or("FUNDAMENTAL_QUOTE")?;
            need(q["retired"]==false&&q["quantity"]==b.quote["quantity"]&&q["activationAt"].as_u64().is_some_and(|t|t<=now)
                &&q["expiresAt"].as_u64().is_some_and(|t|t>now)&&!q["cancelEffectiveAt"].is_u64(),"FUNDAMENTAL_PHYSICAL_NATIVE_INACTIVE")?;
            let planned=request_quantity(&b.spec)?;request_matches(spec,&b.quote,&self.run,&r.tournament_id,planned)?;
            need(planned<=r.maximum_quantity&&mul(amount(b.quote["price"].as_str().ok_or("FUNDAMENTAL_PRICE")?)?,planned as i128)?<=amount(&r.maximum_principal)?,"FUNDAMENTAL_PHYSICAL_SLICE_CAP")?;
            let(books,_)=package_admission::native_books(bank,scope,now,bank.engine().policy()["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?)?;
            let book=books.get(ex).ok_or("FUNDAMENTAL_PHYSICAL_BOOK_STALE")?;let asks=if r.side=="YES"{&book.yes_asks}else{&book.no_asks};let price=amount(b.quote["price"].as_str().ok_or("FUNDAMENTAL_PRICE")?)?;
            need(price<=structure(r)?&&asks.first().is_some_and(|l|price<l.price),"FUNDAMENTAL_PHYSICAL_ENTRY_EDGE")?;
        }else{need(matches!(self.assess(bank,log,scope,qid,trusted,now)?,Decision::Exit{..}),"FUNDAMENTAL_PHYSICAL_EXIT_NOT_ELIGIBLE")?;}
        Ok(())
    }
}
fn request_quantity(s:&RequestSpec)->Result<u64>{s.body.as_ref().and_then(|b|b["quantity"].as_u64()).filter(|n|*n>0).ok_or("FUNDAMENTAL_PHYSICAL_QUANTITY".into())}
fn request_matches(s:&RequestSpec,q:&Value,run:&str,tournament:&str,planned_quantity:u64)->Result<()>{
    let b=s.body.as_ref().ok_or("FUNDAMENTAL_BODY")?;
    let expected_side=if q["side"]=="YES"{"yes"}else{"no"};
    need(s.method=="POST"&&s.endpoint=="/orders"&&b["idempotencyKey"]==format!("{run}:{}",q["id"].as_u64().ok_or("FUNDAMENTAL_QID")?)&&b["tournamentId"]==tournament
        &&planned_quantity>0&&q["quantity"].as_u64().is_some_and(|n|planned_quantity<=n)
        &&b["exchangeId"]==q["exchangeId"]&&b["side"]==expected_side&&b["action"]==q["action"]&&b["quantity"].as_u64()==Some(planned_quantity)
        &&decimal_units(&b["price"].to_string())?==amount(q["price"].as_str().ok_or("FUNDAMENTAL_PRICE")?)?
        &&daily_windows::clock(b["expirationDate"].as_str().ok_or("FUNDAMENTAL_EXPIRATION")?)?==q["expiresAt"].as_u64().ok_or("FUNDAMENTAL_EXPIRATION")?,"FUNDAMENTAL_EXACT_NATIVE_REQUEST")?;
    if let Some(raw)=&s.body_json{need(serde_json::from_str::<Value>(raw)?==*b,"FUNDAMENTAL_RAW_BODY_CHANGED")?;}Ok(())
}
