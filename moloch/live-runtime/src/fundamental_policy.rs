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
// One bounded generation may contain all32 original reviewed inputs.
const MAX_ROW:usize=3*1024*1024;
// Rank intents retain the full reviewed campaign and execution evidence.
// Ordinary-row verification must accept the same 8MiB records already read
// by worker adoption/cohort recovery, including unrelated rank records.
// This is a parser bound, not permission to abandon an attempted order.
const MAX_DISPATCH_ROW:usize=8*1024*1024;
const MAX_LOG:u64=16*1024*1024;
// One original source may use the entire unchanged per-review source budget.
const MAX_SOURCE_BYTES:usize=16*1024*1024;
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
#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase")]
pub enum ExecutionMode{JoinBid,ImproveOneTick,BoundedLimitMayRest}
impl ExecutionMode{
    fn alpha(self)->crate::alpha_proposal::Mode{match self{Self::JoinBid=>crate::alpha_proposal::Mode::JoinBid,Self::ImproveOneTick=>crate::alpha_proposal::Mode::ImproveOneTick,Self::BoundedLimitMayRest=>crate::alpha_proposal::Mode::BoundedLimitMayRest}}
    fn wire(self)->&'static str{match self{Self::JoinBid=>"joinBid",Self::ImproveOneTick=>"improveOneTick",Self::BoundedLimitMayRest=>"boundedLimitMayRest"}}
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ExecutionPolicy{pub version:u32,pub allowed_modes:Vec<ExecutionMode>}
/// V2 economic objective. Scheduled settlement is evidence, never a promise of
/// payout or liquidity by that time. Terminal holdings survive a delayed payout;
/// only explicit revocation/value exits (or official settlement) may close them.
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(tag="kind",rename_all="camelCase",rename_all_fields="camelCase",deny_unknown_fields)]
pub enum ValuationObjective {
    TerminalSettlement { timing_source_sha256:String, timing_json_pointer:String,
        scheduled_settlement_at_ms:u64, delay_policy:TerminalDelayPolicy },
    DeadlineLiquidation { deadline_ms:u64, expected_cash_per_share:String,
        source_sha256:String, model_sha256:String },
}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase")]
pub enum TerminalDelayPolicy { HoldForActualSettlement }
/// Retained, explicitly reviewed MODEL artifact, not an official cash receipt
/// or a calibration certificate. Neither terminal probability nor the current
/// executable bid is automatically a forecast of deadline liquidation cash.
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct DeadlineLiquidationEvidence {
    pub version:u32, pub kind:String, pub tournament_id:String, pub market_id:String,
    pub exchange_id:String, pub contract_id:String, pub side:String, pub deadline_ms:u64,
    pub expected_cash_per_share:String, pub model_sha256:String, pub model:Value,
    pub assumption_label:String,
}
/// Explicit cumulative same-cohort acquisition permission. This is an operator
/// risk limit, not a probability/model qualification or retry capability.
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct SameSideTopUps {pub version:u32,pub maximum_entries:u32}
/// Explicit inherited-inventory value limit; it may rest without a fill claim.
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct PassiveValueExit {pub version:u32,pub maximum_quantity:u64,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub pricing_objective:Option<PassivePricingObjective>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(tag="kind",deny_unknown_fields)]
pub enum PassivePricingObjective {
    #[serde(rename="fifoProfitTurnover")]
    FifoProfitTurnover {#[serde(rename="minimumNetGain")] minimum_net_gain:String}
}
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
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub passive_value_exit:Option<PassiveValueExit>,
    /// Omitted legacy reviews retain exactly their original bytes/hash. When
    /// present, principal/quantity cap ALL held cohort lots plus the new order.
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub same_side_top_ups:Option<SameSideTopUps>,
    /// Separately operator-approved execution choices. Omission preserves the
    /// exact historical canonical review bytes/hash and join-only behavior.
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub execution_policy:Option<ExecutionPolicy>,
    /// Omission preserves exact V1 review bytes, hashes and replay behavior.
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub valuation_objective:Option<ValuationObjective>,
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
    if let Some(p)=&r.passive_value_exit{if let Some(PassivePricingObjective::FifoProfitTurnover{minimum_net_gain})=&p.pricing_objective{need(r.version==2&&r.existing_origin.is_none()&&(5000..=UNIT).contains(&amount(minimum_net_gain)?),"FUNDAMENTAL_FIFO_PROFIT_POLICY")?;}need(p.version==1&&(r.existing_origin.is_some()||r.version==2)&&p.maximum_quantity>0&&p.maximum_quantity<=r.maximum_quantity,"FUNDAMENTAL_PASSIVE_EXIT_POLICY")?;}
    if let Some(p)=&r.same_side_top_ups{need(((p.version==1&&r.existing_origin.is_none())||(p.version==2&&r.existing_origin.is_some()))&&(2..=16).contains(&p.maximum_entries)&&r.version==2,"FUNDAMENTAL_TOP_UP_POLICY")?;}
    if let Some(p)=&r.execution_policy{need(p.version==1&&!p.allowed_modes.is_empty()&&p.allowed_modes.len()<=3,"FUNDAMENTAL_EXECUTION_POLICY")?;
        for(i,m)in p.allowed_modes.iter().enumerate(){need(!p.allowed_modes[..i].contains(m),"FUNDAMENTAL_DUPLICATE_EXECUTION_MODE")?;}}

    if let Some(a)=&r.existing_origin{need(hex(&a.adoption_receipt_sha256)&&hex(&a.adopted_position_sha256),"FUNDAMENTAL_EXISTING_APPROVAL")?;}
    need(matches!(r.version,1|2)&&atom(&r.reviewer_id)&&atom(&r.run_id)&&hex(&r.bank_manifest_sha256)
        &&atom(&r.tournament_id)&&atom(&r.market_id)&&atom(&r.exchange_id)&&atom(&r.contract_id)&&hex(&r.official_contract_root_sha256)
        &&matches!(r.side.as_str(),"YES"|"NO")&&r.payout_mapping_is_operator_assumption
        &&!r.assumption_label.trim().is_empty()&&r.assumption_label.len()<=4096,"FUNDAMENTAL_REVIEW_SCOPE_OR_ASSUMPTION")?;
    let p=amount(&r.ordinary_probability)?;let e=amount(&r.exceptional_probability_allowance)?;
    let exceptional=amount(&r.minimum_exceptional_payout)?;let margin=amount(&r.margin_per_share)?;
    need(p<=UNIT&&e<=UNIT&&exceptional<=UNIT&&margin<UNIT&&amount(&r.maximum_principal)?>0
        &&r.maximum_quantity>0&&r.maximum_quantity<=1_000_000,"FUNDAMENTAL_REVIEW_ECONOMICS")?;
    let review_span=r.expires_at_ms.checked_sub(r.reviewed_at_ms).ok_or("FUNDAMENTAL_REVIEW_HORIZON")?;
    // Terminal's nominal hold clock bounds NEW acquisition, not management of
    // delayed settlement. Fresh revisions keep the original schedule/cohort hold.
    let horizon=if terminal_hold(r){r.hold_until_ms>0&&r.hold_until_ms.checked_sub(r.reviewed_at_ms).is_none_or(|d|d<=366*86_400_000)}
        else{r.expires_at_ms<=r.hold_until_ms&&r.hold_until_ms.checked_sub(r.reviewed_at_ms).is_some_and(|d|d<=366*86_400_000)};
    need(review_span>0&&review_span<=366*86_400_000&&horizon,"FUNDAMENTAL_REVIEW_HORIZON")?;
    need((3..=8).contains(&r.sources.len()),"FUNDAMENTAL_SOURCES")?;
    let mut kinds=BTreeSet::new();let mut hashes=BTreeSet::new();
    for s in &r.sources{
        need(matches!(s.kind.as_str(),"forecast"|"official_ballot"|"official_contract_node"|"resolver"|"official_settlement_timing"|"deadline_liquidation_model")
            &&s.url.starts_with("https://")&&s.url.len()<=2048&&!s.url.contains('@')
            &&hex(&s.sha256)&&hashes.insert(&s.sha256)&&kinds.insert(s.kind.as_str())
            &&s.published_at_ms<=s.observed_at_ms&&s.observed_at_ms<=r.reviewed_at_ms
            &&s.maximum_age_ms>0&&s.maximum_age_ms<=366*86_400_000,"FUNDAMENTAL_SOURCE_BINDING")?;
    }
    need(["forecast","official_ballot","official_contract_node"].iter().all(|k|kinds.contains(k)),"FUNDAMENTAL_REQUIRED_SOURCE")?;
    let terminal=add(mul(UNIT-e,p)?,mul(e,exceptional)?)?/UNIT;
    let value=match (r.version,&r.valuation_objective) {
        (1,None)=>terminal,
        (2,Some(ValuationObjective::TerminalSettlement{timing_source_sha256,timing_json_pointer,scheduled_settlement_at_ms,..}))=>{
            need(hex(timing_source_sha256)&&timing_json_pointer.starts_with('/')&&timing_json_pointer.len()<=256
                &&matches!(timing_json_pointer.rsplit('/').next(),Some("settlement_date"|"scheduledSettlementAt"|"scheduled_settlement_at"))
                &&*scheduled_settlement_at_ms>0&&r.hold_until_ms>=*scheduled_settlement_at_ms
                &&r.sources.iter().any(|s|&s.sha256==timing_source_sha256&&matches!(s.kind.as_str(),"official_contract_node"|"official_settlement_timing")),"FUNDAMENTAL_TERMINAL_TIMING")?;
            terminal
        },
        (2,Some(ValuationObjective::DeadlineLiquidation{deadline_ms,expected_cash_per_share,source_sha256,model_sha256}))=>{
            need(*deadline_ms==r.hold_until_ms&&hex(source_sha256)&&hex(model_sha256)
                &&r.sources.iter().any(|s|&s.sha256==source_sha256&&s.kind=="deadline_liquidation_model"),"FUNDAMENTAL_DEADLINE_MODEL_BINDING")?;
            let cash=amount(expected_cash_per_share)?;need(cash<=UNIT,"FUNDAMENTAL_DEADLINE_CASH_VALUE")?;cash
        },
        _=>return Err("FUNDAMENTAL_VALUATION_VERSION_OR_OBJECTIVE".into()),
    };
    // Every entry/exit/comparator consumes THIS objective-specific value.
    Ok(value-margin)
}
fn execution_model_sha(r:&ReviewInput)->Result<String>{
    if r.version==1 {return digest(&json!({"model":"operator_conditional_binary_mixture_v1","ordinaryProbability":r.ordinary_probability,"exceptionalProbabilityAllowance":r.exceptional_probability_allowance,"minimumExceptionalPayout":r.minimum_exceptional_payout,"marginPerShare":r.margin_per_share,"assumptionLabel":r.assumption_label,"payoutMappingIsOperatorAssumption":r.payout_mapping_is_operator_assumption,"horizonBasis":"remaining_operator_holding_deadline_not_cash_release"}))}
    let economic_model=match &r.valuation_objective {
        Some(ValuationObjective::TerminalSettlement{..})=>json!({"model":"reviewed_terminal_mixture_v2","ordinaryProbability":r.ordinary_probability,"exceptionalProbabilityAllowance":r.exceptional_probability_allowance,"minimumExceptionalPayout":r.minimum_exceptional_payout}),
        Some(ValuationObjective::DeadlineLiquidation{model_sha256,..})=>json!({"model":"reviewed_deadline_cash_v2","modelSha256":model_sha256}),
        None=>return Err("FUNDAMENTAL_VALUATION_VERSION_OR_OBJECTIVE".into()),
    };
    digest(&json!({"version":2,"economicModel":economic_model,"valuationObjective":r.valuation_objective,"marginPerShare":r.margin_per_share,"assumptionLabel":r.assumption_label}))
}
fn terminal_hold(r:&ReviewInput)->bool {matches!(&r.valuation_objective,Some(ValuationObjective::TerminalSettlement{..}))}
fn liquidation_due(r:&ReviewInput,c:&Cohort,now:u64)->bool {!terminal_hold(r)&&now>=c.hold_until_ms.min(r.hold_until_ms)}
fn same_objective_horizon(old:&ReviewInput,new:&ReviewInput)->bool {
    if old.version!=new.version{return false}
    match (&old.valuation_objective,&new.valuation_objective) {
        (None,None)=>true,
        (Some(ValuationObjective::DeadlineLiquidation{deadline_ms:a,..}),Some(ValuationObjective::DeadlineLiquidation{deadline_ms:b,..}))=>a==b,
        (Some(ValuationObjective::TerminalSettlement{scheduled_settlement_at_ms:a,delay_policy:ap,..}),Some(ValuationObjective::TerminalSettlement{scheduled_settlement_at_ms:b,delay_policy:bp,..}))=>a==b&&ap==bp,
        _=>false,
    }
}
fn validate_objective_sources(r:&ReviewInput,originals:&[SourceBytes<'_>])->Result<()> {
    match &r.valuation_objective {
        None=>Ok(()),
        Some(ValuationObjective::TerminalSettlement{timing_source_sha256,timing_json_pointer,scheduled_settlement_at_ms,..})=>{
            let original=originals.iter().find(|s|s.sha256==timing_source_sha256).ok_or("FUNDAMENTAL_TIMING_SOURCE_MISSING")?;
            let data:Value=serde_json::from_slice(original.bytes)?;
            let timing=data.pointer(timing_json_pointer).and_then(Value::as_str).ok_or("FUNDAMENTAL_OFFICIAL_TIMING_UNAVAILABLE")?;
            need(daily_windows::clock(timing)?==*scheduled_settlement_at_ms,"FUNDAMENTAL_OFFICIAL_TIMING_CHANGED")?;
            // A separately retained timing document must bind the same contract;
            // a node receipt already gets exact root/scope validation below.
            if r.sources.iter().any(|s|s.sha256==*timing_source_sha256&&s.kind=="official_settlement_timing") {
                need(data["tournamentId"]==r.tournament_id&&data["marketId"]==r.market_id&&data["contractId"]==r.contract_id,"FUNDAMENTAL_TIMING_CONTRACT_SCOPE")?;
            } Ok(())
        },
        Some(ValuationObjective::DeadlineLiquidation{deadline_ms,expected_cash_per_share,source_sha256,model_sha256})=>{
            let original=originals.iter().find(|s|s.sha256==source_sha256).ok_or("FUNDAMENTAL_DEADLINE_SOURCE_MISSING")?;
            let model:DeadlineLiquidationEvidence=serde_json::from_slice(original.bytes)?;
            need(model.version==1&&model.kind=="reviewed_deadline_liquidation_cash_value"&&model.tournament_id==r.tournament_id
                &&model.market_id==r.market_id&&model.exchange_id==r.exchange_id&&model.contract_id==r.contract_id&&model.side==r.side
                &&model.deadline_ms==*deadline_ms&&model.expected_cash_per_share==*expected_cash_per_share&&model.model_sha256==*model_sha256
                &&model.model.is_object()&&digest(&model.model)?==*model_sha256&&!model.assumption_label.trim().is_empty()&&model.assumption_label.len()<=4096,"FUNDAMENTAL_DEADLINE_SOURCE_ECONOMICS")
        },
    }
}
fn execution_ceiling(r:&ReviewInput)->Result<i128>{Ok(structure(r)?.min(UNIT-5000)/5000*5000)}
fn execution_for_quote(r:&ReviewInput,q:&Value)->Result<ExecutionMode>{
    let metadata=&q["reviewedFundamentalEntry"];
    if r.execution_policy.is_none()&&metadata.is_null(){return Ok(ExecutionMode::JoinBid)}
    need(metadata.is_object()&&metadata["reviewSha256"]==digest(r)?,"FUNDAMENTAL_EXECUTION_REVIEW_BINDING")?;
    let mode=match metadata.get("executionMode").and_then(Value::as_str){None|Some("joinBid")=>ExecutionMode::JoinBid,Some("improveOneTick")=>ExecutionMode::ImproveOneTick,Some("boundedLimitMayRest")=>ExecutionMode::BoundedLimitMayRest,_=>return Err("FUNDAMENTAL_EXECUTION_MODE".into())};
    need(r.execution_policy.as_ref().map_or(mode==ExecutionMode::JoinBid,|p|p.allowed_modes.contains(&mode)),"FUNDAMENTAL_EXECUTION_MODE_NOT_APPROVED")?;
    if mode!=ExecutionMode::JoinBid{
        need(q["action"]=="buy"&&q["side"]==r.side&&metadata["modelSha256"]==execution_model_sha(r)?&&metadata["sourceSha256"]==digest(&r.sources)?
            &&metadata["acquisitionCeiling"]==money(execution_ceiling(r)?)&&metadata["reservedQuantity"]==q["quantity"]
            &&metadata["actionSpec"]==json!({"marketId":r.market_id,"exchangeId":r.exchange_id,"tournamentId":r.tournament_id,"side":r.side,"action":"buy","quantity":q["quantity"],"limitPrice":q["price"]})
            &&amount(q["price"].as_str().ok_or("FUNDAMENTAL_EXECUTION_PRICE")?)?<=execution_ceiling(r)?,"FUNDAMENTAL_EXECUTION_PROVENANCE")?;
    }Ok(mode)
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
        need(found.len()==1&&!found[0].bytes.is_empty()&&found[0].bytes.len()<=MAX_SOURCE_BYTES
            &&bytes_sha(found[0].bytes)==s.sha256,"FUNDAMENTAL_ORIGINAL_SOURCE_HASH")?;
        if s.kind=="official_contract_node"{
            let original:Value=serde_json::from_slice(found[0].bytes)?;
            let data=if original.get("root").is_some(){&original}else{
                need(original["status"]==200&&original["endpoint"]==format!("/markets/{}/nodes?tournamentId={}",input.market_id,input.tournament_id),"FUNDAMENTAL_ORIGINAL_CONTRACT_RECEIPT")?;
                &original["data"]
            };
            validate_contract_data(&input,data)?;
        }
    }validate_objective_sources(&input,originals)?;Ok(Reviewed{input,sha})
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
    /// Explicit migration signal; V1 remains usable for original cohort management
    /// and exact bound-entry recovery, but cannot originate a new logical cohort.
    pub fn supports_new_cohort(&self)->bool{self.input.version==2}
    pub fn valuation_objective(&self)->Option<&ValuationObjective>{self.input.valuation_objective.as_ref()}

}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Binding{qid:u64,spec:RequestSpec,spec_sha256:String,quote:Value,review_sha256:String,bound_at_ms:u64}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Cohort{#[serde(default,skip_serializing_if="Option::is_none")]entry:Option<Binding>,#[serde(default,skip_serializing_if="BTreeMap::is_empty")]top_ups:BTreeMap<u64,Binding>,#[serde(default,skip_serializing_if="BTreeMap::is_empty")]retired_top_ups:BTreeMap<u64,RetiredTopUp>,#[serde(default,skip_serializing_if="Option::is_none")]existing:Option<ExistingOrigin>,first_fill_index:usize,prior_fills_sha256:String,bank_head_at_entry:Value,hold_until_ms:u64,exits:BTreeMap<u64,Binding>,#[serde(default,skip_serializing_if="BTreeMap::is_empty")]rank_trades:BTreeMap<u64,Binding>}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct ExistingOrigin{approval:ExistingApproval,adoption_ref:Value,adopted_position:Value,lots:Value,quantity:u64,principal:String,review_sha256:String}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct RetiredTopUp{not_sent:Value,retirement:Value}
/// Historical zero-exposure entry retained independently of future policies.
/// Its original quote/side/spec and immutable fill prefix are never reinterpreted.
#[derive(Clone)]
struct AbandonedCohort{exchange:String,cohort:Cohort,not_sent:Value,retirement:Value,closed_zero:Option<Value>}
impl Cohort{
    fn entry_id(&self)->Option<u64>{self.entry.as_ref().map(|b|b.qid)}
    fn entries(&self)->impl Iterator<Item=&Binding>{self.entry.iter().chain(self.top_ups.values())}
    fn entry_binding(&self,qid:u64)->Option<&Binding>{self.entries().find(|b|b.qid==qid)}
    fn is_entry(&self,qid:u64)->bool{self.entry_binding(qid).is_some()}
    fn binding(&self,qid:u64)->Option<&Binding>{self.entry_binding(qid).or_else(||self.exits.get(&qid)).or_else(||self.rank_trades.get(&qid))}
    fn pending_entry(&self,log:&DispatchLog)->bool{self.entries().any(|b|!self.retired_top_ups.contains_key(&b.qid)&&!log.closed.contains(&b.qid))}
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
    let snapshot=crate::order_snapshot::snapshot(bank);let orders=snapshot.records();
    for id in bank.engine().live_order_bindings().keys(){let q=snapshot.quote(*id).ok_or("FUNDAMENTAL_UNMAPPED_ACTUAL_BINDING")?;
        need(q["exchangeId"]!=ex||log.closed.contains(id),"FUNDAMENTAL_EXISTING_ACTUAL_NOT_CLOSED")?;}
    for id in log.acknowledgments.keys().filter(|id|!log.closed.contains(id)){
        let q=snapshot.quote(*id).ok_or("FUNDAMENTAL_UNMAPPED_OPEN_ACK")?;need(q["exchangeId"]!=ex,"FUNDAMENTAL_EXISTING_LIABILITY")?;
    }
    need(!orders.iter().any(|q|q["exchangeId"]==ex&&q["retired"]==false)
        &&!log.acknowledgments.keys().any(|id|!log.closed.contains(id)&&orders.iter().any(|q|q["id"]==*id&&q["exchangeId"]==ex))
        &&!bank.engine().package_records().as_array().ok_or("FUNDAMENTAL_PACKAGE_RECORDS")?.iter().any(|p|p["phase"]!="no_execution"&&p["protectionReleased"]!=true
            &&(p["legs"].as_array().is_some_and(|a|a.iter().any(|l|l["exchangeId"]==ex))||p["portfolio"]["componentExchanges"].as_array().is_some_and(|a|a.iter().any(|x|x==ex)))),"FUNDAMENTAL_EXISTING_LIABILITY")
}
// A durable abort is not a venue ACK/terminal or a never-attempted intent.
// Exclude only SDK-proved no-network attempts; actual bindings/ACKs still win
// over any contradictory local proof. Cohort lifecycle remains separately held.
fn physical_pending(bank:&Session,log:&DispatchLog,id:u64)->bool{
    !log.closed.contains(&id)&&(bank.engine().live_order_bindings().contains_key(&id)||log.acknowledgments.contains_key(&id)
        ||(log.attempted.contains(&id)&&!log.proved_no_network(id)&&!log.exposure_resolved(id)))
}
// The active value-exit floor is structure + margin: structure already removed
// that margin. Do not add it twice or substitute the inherited FIFO cost floor.
fn passive_value_limit(r:&ReviewInput,policy:&Value)->Result<Option<i128>>{
    let floor=add(structure(r)?,amount(&r.margin_per_share)?)?.max(0);
    let bps=u32::try_from(policy["feeBps"].as_u64().ok_or("FUNDAMENTAL_PASSIVE_EXIT_FEES")?)?;
    let share=amount(policy["feePerShare"].as_str().ok_or("FUNDAMENTAL_PASSIVE_EXIT_FEES")?)?;
    // A one-share fee floor also covers every positive partial-fill quantity:
    // ceil(n*x) <= n*ceil(x), with the same original nonnegative fixed fees.
    for tick in 1..200{let price=tick*5000;let fee=moloch_replay::fee(price,1,bps,share)?;
        if price.checked_sub(fee).is_some_and(|net|net>=floor){return Ok(Some(price))}}
    Ok(None)
}
// Only authenticated new-entry FIFO cohorts can select this objective. Historical
// adoption lots do not establish their original acquisition fees.
fn fifo_gain(r:&ReviewInput)->Result<Option<i128>>{match r.passive_value_exit.as_ref().and_then(|p|p.pricing_objective.as_ref()){
    None=>Ok(None),Some(PassivePricingObjective::FifoProfitTurnover{minimum_net_gain})=>Ok(Some(amount(minimum_net_gain)?))}}
fn passive_model_sha(r:&ReviewInput,basis:&Value)->Result<String>{digest(&json!({"forecastModelSha256":execution_model_sha(r)?,"reviewSha256":digest(r)?,"pricingBasis":basis}))}
fn fee_floor_tick(policy:&Value,floor:i128)->Result<Option<i128>>{
    let bps=u32::try_from(policy["feeBps"].as_u64().ok_or("FUNDAMENTAL_PASSIVE_EXIT_FEES")?)?;
    let share=amount(policy["feePerShare"].as_str().ok_or("FUNDAMENTAL_PASSIVE_EXIT_FEES")?)?;
    for tick in 1..200{let price=tick*5000;if price-moloch_replay::fee(price,1,bps,share)?>=floor{return Ok(Some(price))}}Ok(None)
}
fn cohort_fifo_cost(bank:&Session,ex:&str,c:&Cohort,side:&str,n:u64)->Result<Value>{
    need(c.existing.is_none()&&c.entry.is_some(),"FUNDAMENTAL_FIFO_PROFIT_COHORT")?;
    let entries=c.entries().map(|b|b.qid).collect();let exits=c.exits.keys().copied().collect();
    Ok(bank.engine().reviewed_fifo_cost(ex,if side=="YES"{moloch_replay::Side::Yes}else{moloch_replay::Side::No},&entries,&exits,n)?)
}
// Price selection does not assert executable depth or a maker fill. Prefer the
// longest affordable FIFO prefix at ask-minus-one-tick; never skip leading lots.
// If none qualifies there, the capped prefix may rest at its own profit floor.
fn passive_terms(r:&ReviewInput,bank:&Session,ex:&str,c:&Cohort,cap:u64,ask:Option<i128>)->Result<Option<(u64,i128,Option<Value>)>>{
    let p=r.passive_value_exit.as_ref().ok_or("FUNDAMENTAL_PASSIVE_EXIT_POLICY")?;let cap=cap.min(p.maximum_quantity);if cap==0{return Ok(None)}
    let Some(gain)=fifo_gain(r)?else{return Ok(passive_value_limit(r,bank.engine().policy())?.map(|price|(cap,price,None)))};
    let Some(ask)=ask else{return Ok(None)};let inside=(ask-5000).max(5000);let policy=bank.engine().policy();
    let bps=u32::try_from(policy["feeBps"].as_u64().ok_or("FUNDAMENTAL_PASSIVE_EXIT_FEES")?)?;
    let share=amount(policy["feePerShare"].as_str().ok_or("FUNDAMENTAL_PASSIVE_EXIT_FEES")?)?;
    let all=cohort_fifo_cost(bank,ex,c,&r.side,cap)?;let mut prefix=0u64;
    // If improving loses the first FIFO unit, joining the unchanged observed
    // ask may still preserve a genuinely profitable prefix (never later lots).
    for candidate in [inside,ask]{let net=candidate-moloch_replay::fee(candidate,1,bps,share)?;
        for l in all["lots"].as_array().ok_or("FUNDAMENTAL_FIFO_COST")?{if add(amount(l["unitCostUpper"].as_str().ok_or("FUNDAMENTAL_FIFO_COST")?)?,gain)?>net{break}prefix=prefix.checked_add(l["quantity"].as_u64().ok_or("FUNDAMENTAL_FIFO_COST")?.min(cap-prefix)).ok_or("FUNDAMENTAL_FIFO_RANGE")?;if prefix==cap{break}}if prefix>0{break}}
    let n=if prefix>0{prefix}else{cap};let cost=cohort_fifo_cost(bank,ex,c,&r.side,n)?;
    let floor=add(amount(cost["maximumFifoUnitCost"].as_str().ok_or("FUNDAMENTAL_FIFO_COST")?)?,gain)?;
    let Some(tick)=fee_floor_tick(policy,floor)?else{return Ok(None)};
    Ok(Some((n,inside.max(tick),Some(cost))))
}
fn fifo_basis(r:&ReviewInput,head:&Value,position:&Value,book:&Value,n:u64,cost:&Value)->Result<Value>{
    let floor=add(amount(cost["maximumFifoUnitCost"].as_str().ok_or("FUNDAMENTAL_FIFO_COST")?)?,fifo_gain(r)?.ok_or("FUNDAMENTAL_FIFO_POLICY")?)?;
    Ok(json!({"pricingPolicy":"improveAskOneTickFifoFloor","bankHead":head,"positionsSha256":position,"quantity":n,"bookSha256":book,"fifoCostSha256":cost["fifoCostSha256"],"maximumFifoUnitCost":cost["maximumFifoUnitCost"],"minimumNetSalePrice":money(floor),"pricingObjective":r.passive_value_exit.as_ref().unwrap().pricing_objective,"forecastExpectedPayout":money(add(structure(r)?,amount(&r.margin_per_share)?)?.max(0))}))
}
fn passive_exit_clear(bank:&Session,log:&DispatchLog,ex:&str,current:Option<u64>,allow_drafts:bool)->Result<bool>{
    if !bank.engine().managed_inventory_planning_excludes(ex)
        ||log.attempted.iter().any(|id|Some(*id)!=current&&!log.acknowledgments.contains_key(id)&&!log.proved_no_network(*id)&&!log.exposure_resolved(*id))
        ||bank.engine().package_blocks_submissions()
        ||bank.engine().package_records().as_array().ok_or("FUNDAMENTAL_PACKAGE_RECORDS")?.iter().any(|p|generation_package_protects(p,ex)){return Ok(false)}
    let snapshot=crate::order_snapshot::snapshot(bank);
    for id in bank.engine().live_order_bindings().keys().filter(|id|Some(**id)!=current&&!log.closed.contains(id)){
        let q=snapshot.quote(*id).ok_or("FUNDAMENTAL_UNMAPPED_ACTUAL_BINDING")?;if q["exchangeId"]==ex{return Ok(false)}
    }
    for id in log.acknowledgments.keys().filter(|id|Some(**id)!=current&&!log.closed.contains(id)){
        let q=snapshot.quote(*id).ok_or("FUNDAMENTAL_UNMAPPED_OPEN_ACK")?;if q["exchangeId"]==ex{return Ok(false)}
    }
    Ok(!snapshot.records().iter().any(|q|q["exchangeId"]==ex&&q["id"].as_u64()!=current
        &&(q["id"].as_u64().is_some_and(|id|physical_pending(bank,log,id))
            ||q["retired"]==false&&(!allow_drafts||q["id"].as_u64().is_none_or(|id|!log.never_attempted(id)||log.intents.contains_key(&id)||bank.engine().fills().iter().any(|f|f["orderId"]==id))))))
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
        need(transport.scope().exchanges.get(exchange).is_some_and(|m|row["marketId"]==*m)&&bank.engine().wallet.quantity(exchange,if signed>0{Side::No}else{Side::Yes})==0,"FUNDAMENTAL_EXISTING_SCOPE")?;
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
/// Non-authoritative nomination: it may trigger the existing seven reads even
/// when no ordinary pair is currently eligible. It never reserves capital.
#[derive(Clone,Debug,Serialize,PartialEq,Eq)]
#[serde(rename_all="camelCase")]
pub struct EntryHint{pub exchange_id:String,pub market_id:String,pub contract_id:String,pub side:String,pub review_sha256:String}
/// Opaque, short-lived native reservation proposal, not HTTP authority.
/// Discard after any bank, original dispatch, queue or review mutation.
pub struct EntryPlan{event:Value,scope_sha:String,head:Value,report_sha:String,dispatch:Value,queue:Value,account_sha:String,
    review:ReviewInput,minted:u64,until:u64,sidecar:FilePin,marker:FilePin,authority:FilePin,_owner:Arc<ExclusiveLock>}
impl EntryPlan{
    pub fn event(&self)->&Value{&self.event}
    pub fn exchange_id(&self)->&str{&self.review.exchange_id}
    pub fn valid_until_ms(&self)->u64{self.until}
    pub fn validate_current(&self,bank:&Session,log:&DispatchLog,transport:&Transport,authority:&ReviewAuthority,now:u64)->Result<()>{
        self.sidecar.validate()?;self.marker.validate()?;self.authority.validate()?;authority.validate()?;
        need(now>=self.minted&&now<self.until&&current(&self.review,now)&&authority.trusted().contains(&digest(&self.review)?)
            &&digest(transport.scope())?==self.scope_sha&&bytes_sha(transport.account_scope().as_bytes())==self.account_sha
            &&bank.durable_head()==self.head&&digest(&bank.engine().report()?)?==self.report_sha
            &&log.descriptor_identity()?==self.dispatch&&queue_view(transport)?==self.queue,"FUNDAMENTAL_ENTRY_PLAN_CHANGED")?;
        no_existing_liability(bank,log,&self.review.exchange_id)?;
        if let Some(p)=self.event.get("sameSideTopUp"){
            need(digest(&bank.engine().wallet.positions()[&self.review.exchange_id])?==p["positionsSha256"]
                &&p["heldQuantity"].as_u64().is_some_and(|n|n>0)&&bank.engine().wallet.quantity(&self.review.exchange_id,if self.review.side=="YES"{Side::Yes}else{Side::No})==p["heldQuantity"].as_u64().unwrap(),"FUNDAMENTAL_TOP_UP_POSITION_CHANGED")
        }else{need(bank.engine().wallet.quantity(&self.review.exchange_id,Side::Yes)==0&&bank.engine().wallet.quantity(&self.review.exchange_id,Side::No)==0,"FUNDAMENTAL_ENTRY_NOT_FLAT")}
    }
}
/// Read hint only; a timeout or absent open order does not prove non-execution.
#[derive(Clone,Debug,Serialize,PartialEq,Eq)]
#[serde(rename_all="camelCase")]
pub struct AbandonHint{pub exchange_id:String,pub qid:u64,pub binding_sha256:String}
/// Positive original-journal + committed-native-retirement evidence. This only
/// permits one sidecar archive transition, never an order or native mutation.
pub struct AbandonPlan{event:Value,head:Value,report_sha:String,dispatch:Value,queue:Value,scope_sha:String,account_sha:String,
    minted:u64,until:u64,sidecar:FilePin,marker:FilePin,ordinary:FilePin,sdk_archive:Option<FilePin>,_owner:Arc<ExclusiveLock>}
impl AbandonPlan{
    pub fn exchange_id(&self)->&str{self.event["exchangeId"].as_str().expect("validated plan")}
    pub fn qid(&self)->u64{self.event["qid"].as_u64().expect("validated plan")}
    pub fn valid_until_ms(&self)->u64{self.until}
    fn validate(&self,bank:&Session,log:&DispatchLog,transport:&Transport,now:u64)->Result<()>{
        self.sidecar.validate()?;self.marker.validate()?;self.ordinary.validate()?;
        if let Some(pin)=&self.sdk_archive{pin.validate()?;}
        need(now>=self.minted&&now<self.until&&bank.durable_head()==self.head&&digest(&bank.engine().report()?)?==self.report_sha
            &&log.descriptor_identity()?==self.dispatch&&queue_view(transport)?==self.queue&&digest(transport.scope())?==self.scope_sha
            &&bytes_sha(transport.account_scope().as_bytes())==self.account_sha,"FUNDAMENTAL_ABANDON_PLAN_CHANGED")?;
        let physical=if self.event["kind"]=="rearm_closed_zero"{log.attempted.contains(&self.qid())&&log.closed.contains(&self.qid())&&bank.engine().live_order_bindings().get(&self.qid()).is_some_and(|id|self.event["closedZeroEvidence"]["actualId"]==*id)}else if self.event["kind"]=="abandon_proved_no_network"{log.proved_no_network(self.qid())&&log.attempted.contains(&self.qid())&&!log.acknowledgments.contains_key(&self.qid())&&!log.closed.contains(&self.qid())&&!bank.engine().live_order_bindings().contains_key(&self.qid())}else{log.never_attempted(self.qid())&&!bank.engine().live_order_bindings().contains_key(&self.qid())};
        need(physical&&!bank.engine().fills().iter().any(|f|f["orderId"]==self.qid()),"FUNDAMENTAL_ABANDON_PHYSICAL_EVENT")
    }
}
/// Requires this process's opaque SDK hydration; JSON presence never suffices.
fn aborted_archive(log:&DispatchLog,qid:u64)->Result<FilePin>{
    need(log.proved_no_network(qid)&&log.attempted.contains(&qid)&&!log.acknowledgments.contains_key(&qid)&&!log.closed.contains(&qid)&&!log.recovered.contains_key(&qid)&&!log.cancel_attempted(qid),"FUNDAMENTAL_ABORT_PROOF_REQUIRED")?;
    let row=log.aborted_records().get(&qid).ok_or("FUNDAMENTAL_ABORT_ORIGINAL_ROW")?;
    let intent=log.intents.get(&qid).ok_or("FUNDAMENTAL_ABORT_INTENT")?;let attempt=log.original_attempt(qid).ok_or("FUNDAMENTAL_ABORT_ATTEMPT")?;let error=log.original_error(qid).ok_or("FUNDAMENTAL_ABORT_ERROR")?;
    need(row["originalIntentSha256"]==digest(intent)?&&row["originalAttemptSha256"]==digest(attempt)?&&row["originalErrorSha256"]==digest(error)?,"FUNDAMENTAL_ABORT_ORIGINAL_CHANGED")?;
    let proof=&row["proof"];let directory=PathBuf::from(proof["queueDirectory"].as_str().ok_or("FUNDAMENTAL_ABORT_QUEUE")?);let name=proof["sourceArchive"].as_str().ok_or("FUNDAMENTAL_ABORT_ARCHIVE")?;
    need(directory.is_absolute()&&Path::new(name).file_name().and_then(|s|s.to_str())==Some(name)&&name.starts_with("ordinary-unadmitted-")&&name.ends_with(".json"),"FUNDAMENTAL_ABORT_ARCHIVE_PATH")?;
    let expected=proof["sourceArchiveSha256"].as_str().filter(|s|hex(s)).ok_or("FUNDAMENTAL_ABORT_ARCHIVE_HASH")?;
    FilePin::open(&directory.join(name),expected,768*1024*1024)
}
/// Read-only hint for a previously attempted, acknowledged and independently
/// closed entry with zero actual fills. This is never retry permission.
#[derive(Clone,Debug,Serialize,PartialEq,Eq)]
#[serde(rename_all="camelCase")]
pub struct RearmHint{pub exchange_id:String,pub qid:u64,pub actual_id:String,pub binding_sha256:String}
pub struct RearmPlan{inner:AbandonPlan}
impl RearmPlan{pub fn exchange_id(&self)->&str{self.inner.exchange_id()}pub fn qid(&self)->u64{self.inner.qid()}pub fn valid_until_ms(&self)->u64{self.inner.valid_until_ms()}}
fn validate_zero_closure(binding:&Binding,proof:&Value,terminal:&Value)->Result<()>{
    let actual=proof["actualId"].as_str().filter(|s|!s.starts_with('0')&&!s.is_empty()&&s.bytes().all(|b|b.is_ascii_digit())).ok_or("FUNDAMENTAL_REARM_ACTUAL_ID")?;
    let ack:Receipt=serde_json::from_value(proof["ack"].clone())?;let order:Receipt=serde_json::from_value(proof["order"].clone())?;let fills:Receipt=serde_json::from_value(proof["fills"].clone())?;
    let n=request_quantity(&binding.spec)?;let tournament=binding.spec.body.as_ref().and_then(|v|v["tournamentId"].as_str()).ok_or("FUNDAMENTAL_REARM_TOURNAMENT")?;
    let processed=terminal["processedAt"].as_str().ok_or("FUNDAMENTAL_REARM_TERMINAL_CLOCK")?;let at=daily_windows::clock(processed)?;
    let ack_start=daily_windows::clock(&ack.request_started_at)?;let ack_end=daily_windows::clock(&ack.received_at)?;
    need(ack.status==200&&binding.bound_at_ms<=ack_start&&ack_start<=ack_end&&ack_end<=at,"FUNDAMENTAL_REARM_ACK_CLOCK")?;
    let acknowledged=if proof["recovered"].is_null(){
        need(ack.endpoint=="/orders","FUNDAMENTAL_REARM_POST_ACK")?;crate::feedback::acknowledgment(&binding.quote,&ack,&ack.received_at)?
    }else{let recovery=&proof["recovered"];need(recovery["kind"]=="recovered_ack"&&recovery["qid"]==binding.qid&&recovery["actualOrderId"]==actual&&recovery["order"]==proof["ack"],"FUNDAMENTAL_REARM_RECOVERY")?;
        let original_fills:Receipt=serde_json::from_value(recovery["fills"].clone())?;
        need(original_fills.endpoint==format!("/orders/{actual}/fills?limit=200"),"FUNDAMENTAL_REARM_RECOVERY_FILLS")?;
        let recovery_processed=recovery["event"]["processedAt"].as_str().ok_or("FUNDAMENTAL_REARM_RECOVERY_EVENT")?;
        let e=crate::feedback::recovered_acknowledgment(&binding.quote,actual,tournament,&ack,&original_fills,recovery_processed)?;
        need(e==recovery["event"]&&crate::feedback::fills(&binding.quote,actual,tournament,&original_fills,recovery_processed)?.is_empty(),"FUNDAMENTAL_REARM_RECOVERY_EVENT")?;e
    };
    need(acknowledged["actualOrderId"]==actual&&acknowledged["quantity"]==n,"FUNDAMENTAL_REARM_ACK_BINDING")?;
    entry_receipt(&order,&format!("/orders/{actual}"),at,at)?;entry_receipt(&fills,&format!("/orders/{actual}/fills?limit=200"),at,at)?;
    need(daily_windows::clock(order.data["createdAt"].as_str().ok_or("FUNDAMENTAL_REARM_ORDER_CLOCK")?)?>=binding.quote["createdAt"].as_u64().ok_or("FUNDAMENTAL_REARM_INTENT_CLOCK")?
        &&daily_windows::clock(order.data["expirationDate"].as_str().ok_or("FUNDAMENTAL_REARM_ORDER_EXPIRY")?)?==binding.quote["expiresAt"].as_u64().ok_or("FUNDAMENTAL_REARM_INTENT_EXPIRY")?,"FUNDAMENTAL_REARM_ORDER_CLOCK")?;
    need(ack_end<=daily_windows::clock(&order.request_started_at)?&&order.data["quantity"]==n&&order.data["quantityFilled"]==0&&order.data["open"]==false
        &&fills.data["totalQuantityFilled"]==0&&fills.data["data"].as_array().is_some_and(|a|a.is_empty()),"FUNDAMENTAL_REARM_ZERO_FILLS_REQUIRED")?;
    let mut q=binding.quote.clone();q["quantity"]=json!(n);
    let expected=crate::feedback::terminal(&q,actual,tournament,&order,&fills,processed)?.ok_or("FUNDAMENTAL_REARM_TERMINAL_REQUIRED")?;
    need(expected==*terminal&&matches!(terminal["status"].as_str(),Some("canceled"|"expired"))&&terminal["filledQuantity"]==0&&terminal["fillsComplete"]==true,"FUNDAMENTAL_REARM_TERMINAL_BINDING")?;
    let rows=proof["rows"].as_array().ok_or("FUNDAMENTAL_REARM_ORIGINAL_ROWS")?;need(rows.len()==4&&rows[0]["event"]["kind"]=="intent"&&rows[1]["event"]["kind"]=="attempt"
        &&matches!(rows[2]["event"]["kind"].as_str(),Some("ack"|"recovered_ack"))&&rows[3]["event"]["kind"]=="closed","FUNDAMENTAL_REARM_ORIGINAL_LIFECYCLE")?;
    let spec:RequestSpec=serde_json::from_value(rows[0]["event"]["spec"].clone())?;
    need(digest(&spec)?==binding.spec_sha256&&rows.iter().all(|r|r["event"]["qid"]==binding.qid)
        &&rows[3]["event"]["orderReceiptHash"]==crate::feedback::digest(&order)?&&rows[3]["event"]["fillReceiptHash"]==crate::feedback::digest(&fills)?
        &&if rows[2]["event"]["kind"]=="ack"{rows[2]["event"]["receipt"]==proof["ack"]&&proof["recovered"].is_null()}else{rows[2]["event"]==proof["recovered"]},"FUNDAMENTAL_REARM_ORIGINAL_RECEIPT_HASH")
}
fn entry_receipt(r:&Receipt,endpoint:&str,now:u64,age:u64)->Result<(u64,u64)>{
    let start=daily_windows::clock(&r.request_started_at)?;
    let end=daily_windows::clock(&r.received_at)?;
    need(r.status==200&&r.endpoint==endpoint&&start<=end&&end<=now&&now-start<=age,"FUNDAMENTAL_ENTRY_RECEIPT_SCOPE_OR_STALE")?;Ok((start,end))
}
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
fn generation_package_protects(p:&Value,ex:&str)->bool{
    p["phase"]!="no_execution"&&p.get("protectionReleased")!=Some(&json!(true))&&(
        p["legs"].as_array().is_some_and(|ls|ls.iter().any(|l|l["exchangeId"]==ex))
        ||p["portfolio"]["componentExchanges"].as_array().is_some_and(|xs|xs.iter().any(|x|x==ex))
        // Public native records omit probabilistic nonwire contracts. Until a
        // getter proves the full scope, ANY protected such package blocks a
        // new manager; wire-leg absence must never be treated as release.
        ||p.get("probabilistic").is_some())
}
// Bounded original dispatch parse cache. No account/native/book/clock facts.
const CLOSED_CACHE_BYTES:usize=8*1024*1024;
const CLOSED_CACHE_COUNT:usize=32;
struct CachedClosedRows{rows:BTreeMap<u64,Vec<Value>>,pin:FilePin}
#[derive(Default)]struct ClosedRowsCache{descriptor:Option<Value>,entries:BTreeMap<BTreeSet<u64>,CachedClosedRows>,bytes:usize,#[cfg(test)]parses:u64}
fn original_closed_rows(cache:&std::sync::Mutex<ClosedRowsCache>,ordinary_path:&PathBuf,ordinary_identity:&Value,log:&DispatchLog,wanted:&BTreeSet<u64>)->Result<(BTreeMap<u64,Vec<Value>>,FilePin)>{
        need(wanted.len()<=128&&wanted.iter().all(|id|log.attempted.contains(id)&&log.closed.contains(id)&&log.acknowledgments.contains_key(id)),"FUNDAMENTAL_REARM_NOT_CLOSED")?;
        let before=log.descriptor_identity()?;
        {
            let mut retained=cache.lock().map_err(|_|"FUNDAMENTAL_CLOSED_CACHE_LOCK")?;
            if retained.descriptor.as_ref()!=Some(&before){retained.entries.clear();retained.bytes=0;retained.descriptor=Some(before.clone());}
            if let Some(entry)=retained.entries.get(wanted){
                need(entry.pin.path==*ordinary_path&&entry.pin.identity==*ordinary_identity,"FUNDAMENTAL_ORIGINAL_DISPATCH_REQUIRED")?;
                // The sole owner already verified this exact dispatch head.
                // Retain inode, size and nanosecond mtime/ctime fences here;
                // rehashing all history on every decision starves dispatch.
                // Misses and physical-plan validation still verify full bytes.
                entry.pin.validate_metadata()?;
                need(before==log.descriptor_identity()?,"FUNDAMENTAL_REARM_DISPATCH_CHANGED")?;
                let pin=FilePin{file:entry.pin.file.try_clone()?,path:entry.pin.path.clone(),identity:entry.pin.identity.clone(),bytes:entry.pin.bytes,sha:entry.pin.sha.clone(),maximum:entry.pin.maximum,version:entry.pin.version.clone()};
                pin.validate_metadata()?;return Ok((entry.rows.clone(),pin));
            }
            #[cfg(test)]{retained.parses+=1;}
        }
        let pin=FilePin::from_held(File::open(ordinary_path)?,ordinary_path.clone(),64*1024*1024,None)?;
        need(pin.identity==*ordinary_identity,"FUNDAMENTAL_ORIGINAL_DISPATCH_REQUIRED")?;let bytes=read_file(&pin.file,pin.maximum)?;need(bytes.is_empty()||bytes.last()==Some(&b'\n'),"FUNDAMENTAL_REARM_DISPATCH_TORN")?;
        let mut sequence=0u64;let mut previous="0".repeat(64);let mut found:BTreeMap<u64,Vec<Value>>=BTreeMap::new();
        for line in bytes.split(|b|*b==b'\n').filter(|l|!l.is_empty()){
            need(line.len()<=MAX_DISPATCH_ROW,"FUNDAMENTAL_REARM_DISPATCH_ROW_BOUND")?;let original:Value=serde_json::from_slice(line)?;let mut row=original.clone();let sha=row.as_object_mut().ok_or("FUNDAMENTAL_REARM_DISPATCH_ROW")?.remove("hash").ok_or("FUNDAMENTAL_REARM_DISPATCH_HASH")?;
            need(row["sequence"]==sequence+1&&row["previous"]==previous&&sha==digest(&row)?,"FUNDAMENTAL_REARM_DISPATCH_CHAIN")?;
            if let Some(id)=row["event"]["qid"].as_u64().filter(|id|wanted.contains(id)){
                if matches!(row["event"]["kind"].as_str(),Some("intent"|"attempt"|"ack"|"recovered_ack"|"closed")){let rows=found.entry(id).or_default();need(rows.len()<4,"FUNDAMENTAL_REARM_DUPLICATE_LIFECYCLE")?;rows.push(original);}}
            sequence+=1;previous=sha.as_str().ok_or("FUNDAMENTAL_REARM_DISPATCH_HASH")?.into();
        }
        pin.validate()?;need(before==log.descriptor_identity()?&&before["sequence"]==sequence&&before["previous"]==previous&&found.len()==wanted.len()&&found.values().all(|r|r.len()==4),"FUNDAMENTAL_REARM_DISPATCH_CHANGED")?;
        let serialized=serde_json::to_vec(&found)?.len().saturating_add(wanted.len().saturating_mul(std::mem::size_of::<u64>()));
        if serialized<=CLOSED_CACHE_BYTES{
            let cached_pin=FilePin{file:pin.file.try_clone()?,path:pin.path.clone(),identity:pin.identity.clone(),bytes:pin.bytes,sha:pin.sha.clone(),maximum:pin.maximum,version:pin.version.clone()};
            let mut retained=cache.lock().map_err(|_|"FUNDAMENTAL_CLOSED_CACHE_LOCK")?;
            if retained.descriptor.as_ref()!=Some(&before){retained.entries.clear();retained.bytes=0;retained.descriptor=Some(before.clone());}
            if retained.entries.len()>=CLOSED_CACHE_COUNT||retained.bytes.saturating_add(serialized)>CLOSED_CACHE_BYTES{retained.entries.clear();retained.bytes=0;}
            retained.bytes+=serialized;retained.entries.insert(wanted.clone(),CachedClosedRows{rows:found.clone(),pin:cached_pin});
        }
        Ok((found,pin))
    }
#[cfg(all(test,unix))]
#[path="../../../research/moloch-fundamental-closure-cache-20261017/tests.rs"]
mod closed_rows_cache_tests;
// Only ordinary NEVER-ATTEMPTED original rows are memoized. Hits retain the
// exact dispatch head and held file version; SDK-proved aborts use the parser.
const NOT_SENT_CACHE_BYTES:usize=8*1024*1024;
const NOT_SENT_CACHE_COUNT:usize=32;
struct CachedNotSentRows{rows:BTreeMap<u64,Value>,pin:FilePin}
#[derive(Default)]struct NotSentRowsCache{descriptor:Option<Value>,entries:BTreeMap<BTreeSet<u64>,CachedNotSentRows>,bytes:usize,#[cfg(test)]parses:u64}
fn original_not_sent_rows(cache:&std::sync::Mutex<NotSentRowsCache>,ordinary_path:&PathBuf,ordinary_identity:&Value,log:&DispatchLog,wanted:&BTreeSet<u64>)->Result<(BTreeMap<u64,Value>,FilePin)>{
    need(wanted.len()<=4096&&wanted.iter().all(|qid|log.never_attempted(*qid)||log.proved_no_network(*qid)),"FUNDAMENTAL_ABANDON_ATTEMPTED")?;
    // Do not retain or serve any SDK-proved branch: its original archive must
    // be opened and independently checked on EVERY original parser invocation.
    if wanted.iter().any(|qid|log.proved_no_network(*qid)){
        return original_not_sent_rows_uncached(ordinary_path,ordinary_identity,log,wanted)
    }
    let before=log.descriptor_identity()?;
    if let Ok(mut retained)=cache.lock(){
        if retained.descriptor.as_ref()!=Some(&before){retained.entries.clear();retained.bytes=0;retained.descriptor=Some(before.clone());}
        if let Some(entry)=retained.entries.get(wanted){
            need(entry.pin.path==*ordinary_path&&entry.pin.identity==*ordinary_identity,"FUNDAMENTAL_ORIGINAL_DISPATCH_REQUIRED")?;
            entry.pin.validate_metadata()?;
            need(before==log.descriptor_identity()?,"FUNDAMENTAL_ABANDON_DISPATCH_CHANGED")?;
            let pin=FilePin{file:entry.pin.file.try_clone()?,path:entry.pin.path.clone(),identity:entry.pin.identity.clone(),bytes:entry.pin.bytes,sha:entry.pin.sha.clone(),maximum:entry.pin.maximum,version:entry.pin.version.clone()};
            pin.validate_metadata()?;return Ok((entry.rows.clone(),pin))
        }
        #[cfg(test)]{retained.parses+=1;}
    }
    // A poisoned optimization lock is not financial authority. Fall back to
    // unchanged original verification, including all original errors/fences.
    let(found,pin)=original_not_sent_rows_uncached(ordinary_path,ordinary_identity,log,wanted)?;
    need(before==log.descriptor_identity()?,"FUNDAMENTAL_ABANDON_DISPATCH_CHANGED")?;
    let serialized=serde_json::to_vec(&found)?.len().saturating_add(wanted.len().saturating_mul(std::mem::size_of::<u64>()));
    if serialized<=NOT_SENT_CACHE_BYTES{
        if let Ok(mut retained)=cache.lock(){
            let cached_pin=FilePin{file:pin.file.try_clone()?,path:pin.path.clone(),identity:pin.identity.clone(),bytes:pin.bytes,sha:pin.sha.clone(),maximum:pin.maximum,version:pin.version.clone()};
            if retained.descriptor.as_ref()!=Some(&before){retained.entries.clear();retained.bytes=0;retained.descriptor=Some(before.clone());}
            if retained.entries.len()>=NOT_SENT_CACHE_COUNT||retained.bytes.saturating_add(serialized)>NOT_SENT_CACHE_BYTES{retained.entries.clear();retained.bytes=0;}
            retained.bytes+=serialized;retained.entries.insert(wanted.clone(),CachedNotSentRows{rows:found.clone(),pin:cached_pin});
        }
    }
    Ok((found,pin))
}
fn original_not_sent_rows_uncached(ordinary_path:&PathBuf,ordinary_identity:&Value,log:&DispatchLog,wanted:&BTreeSet<u64>)->Result<(BTreeMap<u64,Value>,FilePin)>{
        need(wanted.len()<=4096&&wanted.iter().all(|qid|log.never_attempted(*qid)||log.proved_no_network(*qid)),"FUNDAMENTAL_ABANDON_ATTEMPTED")?;
        let before=log.descriptor_identity()?;let pin=FilePin::from_held(File::open(ordinary_path)?,ordinary_path.clone(),64*1024*1024,None)?;
        need(pin.identity==*ordinary_identity,"FUNDAMENTAL_ORIGINAL_DISPATCH_REQUIRED")?;
        let bytes=read_file(&pin.file,pin.maximum)?;need(bytes.is_empty()||bytes.last()==Some(&b'\n'),"FUNDAMENTAL_ABANDON_DISPATCH_TORN")?;
        let mut sequence=0u64;let mut previous="0".repeat(64);let mut found=BTreeMap::new();
        for line in bytes.split(|b|*b==b'\n').filter(|l|!l.is_empty()){
            need(line.len()<=MAX_DISPATCH_ROW,"FUNDAMENTAL_ABANDON_DISPATCH_ROW_BOUND")?;
            let original:Value=serde_json::from_slice(line)?;let mut row=original.clone();let sha=row.as_object_mut().ok_or("FUNDAMENTAL_ABANDON_DISPATCH_ROW")?.remove("hash").ok_or("FUNDAMENTAL_ABANDON_DISPATCH_HASH")?;
            need(row["sequence"]==sequence+1&&row["previous"]==previous&&sha==digest(&row)?,"FUNDAMENTAL_ABANDON_DISPATCH_CHAIN")?;
            if let Some(qid)=row["event"]["qid"].as_u64().filter(|id|wanted.contains(id)){
                if log.proved_no_network(qid){
                    need(!matches!(row["event"]["kind"].as_str(),Some("ack"|"recovered_ack"|"closed"|"cancel_attempt"|"cancel_ack")),"FUNDAMENTAL_ABORT_PHYSICAL_EVENT")?;
                    if row["event"]["kind"]=="aborted_before_admission"{need(!found.contains_key(&qid)&&log.aborted_records().get(&qid)==Some(&row["event"]),"FUNDAMENTAL_ABORT_ORIGINAL_ROW")?;let _=aborted_archive(log,qid)?;found.insert(qid,original);}
                }else{
                need(!matches!(row["event"]["kind"].as_str(),Some("attempt"|"ack"|"recovered_ack")),"FUNDAMENTAL_ABANDON_ATTEMPTED")?;
                if row["event"]["kind"]=="not_sent"{let proof=&row["event"]["proof"];
                    need(!found.contains_key(&qid)&&*proof==json!({"qid":qid,"chain":previous,"sequence":sequence,"networkAttempts":0}),"FUNDAMENTAL_ABANDON_POSITIVE_NOT_SENT")?;found.insert(qid,original);}}
            }
            sequence+=1;previous=sha.as_str().ok_or("FUNDAMENTAL_ABANDON_DISPATCH_HASH")?.into();
        }
        pin.validate()?;need(log.descriptor_identity()?==before&&before["sequence"]==sequence&&before["previous"]==previous,"FUNDAMENTAL_ABANDON_DISPATCH_CHANGED")?;
        need(found.len()==wanted.len(),"FUNDAMENTAL_ABANDON_POSITIVE_NOT_SENT_REQUIRED")?;Ok((found,pin))
    }
#[cfg(all(test,unix))]
#[path="../../../research/moloch-fundamental-not-sent-cache-20261017/tests.rs"]
mod not_sent_rows_cache_tests;
pub struct Cohorts{
    file:File,path:PathBuf,ordinary_path:PathBuf,ordinary_identity:Value,
    marker:FilePin,
    _owner:Arc<ExclusiveLock>,manifest:String,run:String,
    sequence:u64,previous:String,clock:u64,poisoned:bool,generation:Option<Value>,
    policies:BTreeMap<String,ReviewInput>,cohorts:BTreeMap<String,Cohort>,abandoned:BTreeMap<u64,AbandonedCohort>,
    rank_bindings:BTreeMap<u64,Value>,
    aborted_archives:std::sync::Mutex<BTreeMap<u64,FilePin>>,
    closed_rows_cache:std::sync::Mutex<ClosedRowsCache>,
    not_sent_rows_cache:std::sync::Mutex<NotSentRowsCache>,
}
#[path="fundamental_rank_ownership.rs"]mod rank_ownership;
pub use rank_ownership::RankInventoryOwnership;
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
#[cfg(unix)]fn file_version(file:&File)->Result<Value>{use std::os::unix::fs::MetadataExt;let m=file.metadata()?;Ok(json!({"device":m.dev(),"inode":m.ino(),"bytes":m.len(),"mtime":m.mtime(),"mtimeNs":m.mtime_nsec(),"ctime":m.ctime(),"ctimeNs":m.ctime_nsec()}))}
#[cfg(not(unix))]fn file_version(_: &File)->Result<Value>{Err("FUNDAMENTAL_DESCRIPTOR_UNSUPPORTED".into())}
struct FilePin{file:File,path:PathBuf,identity:Value,bytes:u64,sha:String,maximum:u64,version:Value}
/// Full bytes were verified at staging/recovery. Generic read guards only use
/// immutable held metadata; physical preparation rehashes the original bytes.
pub(crate) struct GenerationFiles{pins:Vec<FilePin>}
impl GenerationFiles{
    pub(crate) fn validate(&self)->Result<()>{for p in &self.pins{p.validate_metadata()?;}Ok(())}
    pub(crate) fn validate_full(&self)->Result<()>{for p in &self.pins{p.validate()?;}Ok(())}
}
impl FilePin{
    fn from_held(file:File,path:PathBuf,maximum:u64,expected:Option<&str>)->Result<Self>{
        let identity=identity(&file)?;path_pin(&path,&identity)?;let bytes=read_file(&file,maximum)?;let sha=bytes_sha(&bytes);
        need(expected.is_none_or(|e|hex(e)&&e==sha),"FUNDAMENTAL_PINNED_FILE_HASH")?;
        let version=file_version(&file)?;Ok(Self{file,path,identity,bytes:bytes.len() as u64,sha,maximum,version})
    }
    fn open(path:&Path,expected:&str,maximum:u64)->Result<Self>{
        need(path.is_absolute(),"FUNDAMENTAL_ABSOLUTE_SOURCE_PATH_REQUIRED")?;
        Self::from_held(File::open(path)?,path.into(),maximum,Some(expected))
    }
    fn validate_metadata(&self)->Result<()>{path_pin(&self.path,&self.identity)?;need(file_version(&self.file)?==self.version,"FUNDAMENTAL_PINNED_FILE_CHANGED")}
    fn validate(&self)->Result<()>{self.validate_metadata()?;
        need(self.file.metadata()?.len()==self.bytes&&bytes_sha(&read_file(&self.file,self.maximum)?)==self.sha,"FUNDAMENTAL_PINNED_FILE_CHANGED")?;self.validate_metadata()
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
pub struct LoadedSettings{authority:ReviewAuthority,reviews:Vec<Reviewed>,initialize:bool,pins:Vec<FilePin>,settings_sha:String}
impl Settings{
    pub fn load(&self)->Result<LoadedSettings>{
        need(self.version==1&&(1..=32).contains(&self.reviews.len()),"FUNDAMENTAL_SETTINGS")?;
        let authority=ReviewAuthority::load(&self.authority_path,&self.authority_sha256)?;
        let mut reviews=Vec::new();let mut exchanges=BTreeSet::new();let mut pins=Vec::new();
        for r in &self.reviews{
            let pin=FilePin::open(&r.path,&r.file_sha256,64*1024)?;
            let input:ReviewInput=serde_json::from_slice(&read_file(&pin.file,pin.maximum)?)?;
            need(exchanges.insert(input.exchange_id.clone())&&r.source_files.len()==input.sources.len(),"FUNDAMENTAL_DUPLICATE_SETTING_OR_SOURCES")?;
            let mut data=Vec::new();let mut total=0usize;
            for s in &r.source_files{let p=FilePin::open(&s.path,&s.sha256,MAX_SOURCE_BYTES as u64)?;let bytes=read_file(&p.file,p.maximum)?;
                total=total.checked_add(bytes.len()).ok_or("FUNDAMENTAL_SOURCE_BOUND")?;need(total<=MAX_SOURCE_BYTES,"FUNDAMENTAL_SOURCE_BOUND")?;p.validate()?;data.push(bytes);pins.push(p);}
            let sources=r.source_files.iter().zip(&data).map(|(s,b)|SourceBytes{sha256:&s.sha256,bytes:b}).collect::<Vec<_>>();
            reviews.push(verify_snapshot(input,&sources,authority.trusted())?);pin.validate()?;pins.push(pin);
        }authority.validate()?;Ok(LoadedSettings{authority,reviews,initialize:self.allow_initialization,pins,settings_sha:digest(self)?})
    }
}
impl LoadedSettings{
    pub(crate) fn retained_files(&self)->Result<GenerationFiles>{
        self.validate_files()?;let mut pins=self.pins.iter().map(|p|Ok(FilePin{file:p.file.try_clone()?,path:p.path.clone(),identity:p.identity.clone(),bytes:p.bytes,sha:p.sha.clone(),maximum:p.maximum,version:p.version.clone()})).collect::<Result<Vec<_>>>()?;pins.push(self.authority.pin.duplicate()?);
        Ok(GenerationFiles{pins})
    }
    pub fn settings_sha(&self)->&str{&self.settings_sha}
    pub fn validate_files(&self)->Result<()>{self.authority.validate()?;for pin in &self.pins{pin.validate()?;}Ok(())}
    pub fn into_authority(self)->ReviewAuthority{self.authority}

    /// Verified loaded existing-origin scopes only; this grants no adoption,
    /// retirement or order authority. The caller still proves every draft unsent.
    pub fn existing_exchanges(&self)->BTreeSet<String>{self.reviews.iter().filter(|r|r.input.existing_origin.is_some()).map(|r|r.input.exchange_id.clone()).collect()}
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
    marketable_reviewed_buy:bool,qid:u64,planned_quantity:u64,native_head:Value,quote_sha:String,ledger_sha:String,scope_sha:String,
    spec_sha:String,permit_sha:String,review_sha:String,trust_sha:String,
    contract_market:String,contract_id:String,contract_tournament:String,contract_root_sha:String,
    minted:u64,expires:u64,sidecar:FilePin,marker:FilePin,sidecar_sequence:u64,sidecar_tail:String,
    authority:FilePin,ordinary_file:File,ordinary_path:PathBuf,ordinary_identity:Value,
    _owner:Arc<ExclusiveLock>,
}
impl PhysicalDecision{
    /// True only for an opaque token minted from the exact approved native
    /// bounded action. The caller MUST validate_frozen and authorize first.
    pub fn allows_marketable_reviewed_buy(&self)->bool{self.marketable_reviewed_buy}
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
    /// This deliberately allows a retired/expired original qid as a read hint.
    /// No eligibility is reset until both positive original witnesses verify.
    pub fn abandonment_hints(&self,bank:&Session,log:&DispatchLog,scope:&Scope,_now:u64)->Result<Vec<AbandonHint>>{
        self.check(bank,log)?;log.require_resolved_attempts()?;let mut out=Vec::new();
        for(ex,c)in &self.cohorts{if self.rank_paused(ex){continue}let Some(b)=&c.entry else{continue};
            need(scope.tournament_id==self.policies[ex].tournament_id&&scope.exchanges.get(ex)==Some(&self.policies[ex].market_id),"FUNDAMENTAL_ABANDON_SCOPE")?;
            if c.existing.is_some()||!c.top_ups.is_empty()||!c.exits.is_empty()||!(log.never_attempted(b.qid)||log.proved_no_network(b.qid))||bank.engine().live_order_bindings().contains_key(&b.qid)
                ||bank.engine().fills().iter().any(|f|f["orderId"]==b.qid)||bank.engine().wallet.quantity(ex,Side::Yes)!=0||bank.engine().wallet.quantity(ex,Side::No)!=0{continue}
            out.push(AbandonHint{exchange_id:ex.clone(),qid:b.qid,binding_sha256:digest(b)?});
        }
        for(ex,c)in &self.cohorts{if self.rank_paused(ex){continue}if c.existing.is_none()&&!c.exits.is_empty(){continue}for b in c.top_ups.values(){
            if !c.retired_top_ups.contains_key(&b.qid)&&log.never_attempted(b.qid)&&!bank.engine().live_order_bindings().contains_key(&b.qid)
                &&!bank.engine().fills().iter().any(|f|f["orderId"]==b.qid){out.push(AbandonHint{exchange_id:ex.clone(),qid:b.qid,binding_sha256:digest(b)?});}
        }}Ok(out)
    }
    fn original_not_sent_records(&self,log:&DispatchLog,wanted:&BTreeSet<u64>)->Result<(BTreeMap<u64,Value>,FilePin)>{
        original_not_sent_rows(&self.not_sent_rows_cache,&self.ordinary_path,&self.ordinary_identity,log,wanted)
    }
    /// Original retirement must include its ACTUAL prepared processedAt bytes
    /// and be retained in this Session's source references, not reconstructed.
    pub fn prepare_abandon(&self,bank:&Session,log:&DispatchLog,transport:&Transport,scope:&Scope,hint:&AbandonHint,f:&crate::authority::CancelSnapshotSet,source_id:&str,sequence:u64,retirement:&Value,now:u64)->Result<AbandonPlan>{
        need(self.abandonment_hints(bank,log,scope,now)?.iter().any(|h|h==hint)&&digest(scope)?==digest(transport.scope())?,"FUNDAMENTAL_ABANDON_HINT_CHANGED")?;
        no_existing_liability(bank,log,&hint.exchange_id)?;let queue=queue_view(transport)?;
        let c=&self.cohorts[&hint.exchange_id];let top_up=c.top_ups.contains_key(&hint.qid);let b=c.binding(hint.qid).ok_or("FUNDAMENTAL_ABANDON_ORIGIN")?;
        let owned=if top_up{self.owned(bank,&hint.exchange_id,c)?}else{0};let wanted=if self.policies[&hint.exchange_id].side=="YES"{owned as i64}else{-(owned as i64)};let mut position_seen=false;
        let q=crate::order_snapshot::quote(bank,hint.qid).ok_or("FUNDAMENTAL_ABANDON_QUOTE")?;
        need(q["retired"]==true&&q["filled"]==0,"FUNDAMENTAL_ABANDON_NATIVE_RETIREMENT_REQUIRED")?;
        let(mut not_sents,ordinary)=self.original_not_sent_records(log,&[hint.qid].into())?;let not_sent=not_sents.remove(&hint.qid).ok_or("FUNDAMENTAL_ABANDON_POSITIVE_NOT_SENT_REQUIRED")?;
        let proved_abort=log.proved_no_network(hint.qid);
        let sdk_archive=if proved_abort{
            need(!top_up&&log.intents.get(&hint.qid).is_some_and(|v|v.get("fundamental")==self.durable_tag(hint.qid).as_ref()),"FUNDAMENTAL_ABORT_ORIGINAL_TAG")?;
            let(spec,permit,attempt,error)=log.pre_admission_originals(hint.qid)?;
            need(digest(&spec)?==b.spec_sha256&&not_sent["event"]==log.aborted_records()[&hint.qid],"FUNDAMENTAL_ABORT_ORIGINAL_REQUEST")?;
            for key in ["id","marketId","exchangeId","side","action","quantity","price"]{need(log.intents[&hint.qid]["quote"][key]==b.quote[key]&&q[key]==b.quote[key],"FUNDAMENTAL_ABORT_ORIGINAL_QUOTE")?;}
            let proof=&not_sent["event"]["proof"];
            let config=baal_transport_service::Config{queue_dir:PathBuf::from(proof["queueDirectory"].as_str().ok_or("FUNDAMENTAL_ABORT_QUEUE")?),account_scope:transport.account_scope().into(),scope:scope.clone(),maximum_pages:1,writes_enabled:false};
            let verified=baal_transport_service::verify_retained_ordinary_pre_admission(&config,&spec,&permit,attempt,&error,proof)?;
            verified.validate_binding(&spec,&permit,attempt,&error)?;Some(aborted_archive(log,hint.qid)?)
        }else{None};
        let mut event=retirement.clone();let reference=event.as_object_mut().ok_or("FUNDAMENTAL_ABANDON_RETIREMENT")?.remove("sourceRef");let sha=digest(&event)?;
        need(sequence>0&&bank.committed_source_hash(source_id,sequence)==Some(sha.as_str())&&reference.is_none_or(|r|r==json!({"id":source_id,"sequence":sequence,"hash":sha}))
            &&event["type"]=="unsubmitted_intent_retired"&&event["source"]=="moloch_live_dispatcher"&&event["orderId"]==hint.qid&&event["confirmedNetworkAttempts"]==0
            &&event["receiptHash"]==digest(&not_sent["event"]["proof"])? ,"FUNDAMENTAL_ABANDON_COMMITTED_RETIREMENT")?;
        let retired_at=daily_windows::clock(event["processedAt"].as_str().ok_or("FUNDAMENTAL_ABANDON_RETIREMENT_CLOCK")?)?;
        let tid=&scope.tournament_id;let endpoints=[format!("/tournaments/{}",scope.tournament_slug),format!("/tournaments/{}/portfolio/positions",scope.tournament_slug),format!("/orders?status=open&limit=200&tournamentId={tid}"),format!("/account?tournamentId={tid}")];
        let receipts=[&f.account,&f.positions,&f.open_orders,&f.cash_balance];let mut last=retired_at;let mut until=u64::MAX;
        for(receipt,endpoint)in receipts.iter().zip(&endpoints){let(start,end)=entry_receipt(receipt,endpoint,now,20000)?;need(start>=last,"FUNDAMENTAL_ABANDON_RECEIPT_ORDER")?;last=end;until=until.min(start.checked_add(20000).ok_or("FUNDAMENTAL_CLOCK_RANGE")?);}
        need(f.account.data["id"]==*tid&&f.account.data["slug"]==scope.tournament_slug&&f.account.data["status"]=="active"&&f.positions.data.get("pagination").is_none()
            &&f.open_orders.data["coverage"]["complete"]==true&&f.open_orders.data["pagination"]["hasMore"]==false&&f.open_orders.data["pagination"]["nextCursor"].is_null(),"FUNDAMENTAL_ABANDON_ACCOUNT_COVERAGE")?;
        let mut seen=BTreeSet::new();for row in f.positions.data["positions"].as_array().ok_or("FUNDAMENTAL_ABANDON_POSITIONS")?{let ex=row["exchangeId"].as_str().ok_or("FUNDAMENTAL_ABANDON_POSITION_SCOPE")?;
            need(seen.insert(ex),"FUNDAMENTAL_ABANDON_DUPLICATE_POSITION")?;if ex==hint.exchange_id{position_seen=true;need(row["quantity"].as_i64()==Some(wanted),"FUNDAMENTAL_ABANDON_NOT_FLAT")?;}}
        need(owned==0||position_seen,"FUNDAMENTAL_TOP_UP_POSITION_MISSING")?;
        for row in f.open_orders.data["data"].as_array().ok_or("FUNDAMENTAL_ABANDON_ORDERS")?{need(row["exchangeId"].as_str().is_some_and(|ex|ex!=hint.exchange_id),"FUNDAMENTAL_ABANDON_OPEN_ORDER")?;}
        need(baal_transport_service::relationships::parse_buying_power(&f.cash_balance.data,scope)?.gross_cash_units==Some(crate::authority::reconciled_expected_cash(bank)?),"FUNDAMENTAL_ABANDON_CASH")?;
        let plan=AbandonPlan{event:json!({"kind":if proved_abort{"abandon_proved_no_network"}else if top_up{"abandon_top_up_never_sent"}else{"abandon_never_sent"},"exchangeId":hint.exchange_id,"qid":hint.qid,"cohortSha256":digest(c)?,"originalSide":b.quote["side"],"originalBindingSha256":digest(b)?,"originalNotSent":not_sent,"committedRetirement":{"id":source_id,"sequence":sequence,"sha256":sha,"event":event},"factsSha256":digest(&receipts)?}),
            head:bank.durable_head(),report_sha:digest(&bank.engine().report()?)?,dispatch:log.descriptor_identity()?,queue,scope_sha:digest(scope)?,account_sha:bytes_sha(transport.account_scope().as_bytes()),minted:now,until,
            sidecar:FilePin::from_held(self.file.try_clone()?,self.path.clone(),MAX_LOG,None)?,marker:self.marker.duplicate()?,ordinary,sdk_archive,_owner:self._owner.clone()};
        plan.validate(bank,log,transport,now)?;Ok(plan)
    }
    /// Consumes the opaque plan once, archives historical tags, and changes NO
    /// bank financial state. The next entry still requires a fresh approved plan.
    pub fn apply_abandon(&mut self,plan:AbandonPlan,bank:&Session,log:&DispatchLog,transport:&Transport,now:u64)->Result<()>{
        self.check(bank,log)?;plan.validate(bank,log,transport,now)?;
        need(self.cohorts.get(plan.exchange_id()).is_some_and(|c|c.binding(plan.qid()).is_some()&&digest(c).ok().as_ref()==plan.event["cohortSha256"].as_str().map(str::to_owned).as_ref()),"FUNDAMENTAL_ABANDON_COHORT_CHANGED")?;
        self.append(plan.event,now)?;self.check(bank,log)
    }
    /// Known closure is essential: retired/native-filled/open-list absence is
    /// not enough. Complete original receipt proof is checked by prepare_rearm.
    pub fn rearm_hints(&self,bank:&Session,log:&DispatchLog,scope:&Scope,_now:u64)->Result<Vec<RearmHint>>{
        self.check(bank,log)?;log.require_resolved_attempts()?;let mut out=Vec::new();
        for(ex,c)in &self.cohorts{if self.rank_paused(ex){continue}let Some(b)=&c.entry else{continue};need(scope.tournament_id==self.policies[ex].tournament_id&&scope.exchanges.get(ex)==Some(&self.policies[ex].market_id),"FUNDAMENTAL_REARM_SCOPE")?;
            if c.existing.is_some()||!c.top_ups.is_empty()||!c.exits.is_empty()||!log.attempted.contains(&b.qid)||!log.closed.contains(&b.qid)||!log.acknowledgments.contains_key(&b.qid)
                ||bank.engine().fills().iter().any(|f|f["orderId"]==b.qid)||bank.engine().wallet.quantity(ex,Side::Yes)!=0||bank.engine().wallet.quantity(ex,Side::No)!=0{continue}
            let Some(actual)=bank.engine().live_order_bindings().get(&b.qid)else{continue};let Some(q)=crate::order_snapshot::quote(bank,b.qid)else{return Err("FUNDAMENTAL_REARM_NATIVE_QUOTE".into())};
            if q["retired"]!=true||q["filled"]!=0{continue}out.push(RearmHint{exchange_id:ex.clone(),qid:b.qid,actual_id:actual.clone(),binding_sha256:digest(b)?});
        }Ok(out)
    }
    fn original_closed_records(&self,log:&DispatchLog,wanted:&BTreeSet<u64>)->Result<(BTreeMap<u64,Vec<Value>>,FilePin)>{
        original_closed_rows(&self.closed_rows_cache,&self.ordinary_path,&self.ordinary_identity,log,wanted)
    }
    pub fn prepare_rearm(&self,bank:&Session,log:&DispatchLog,transport:&Transport,scope:&Scope,hint:&RearmHint,f:&crate::authority::CancelSnapshotSet,order:&Receipt,fills:&Receipt,source_id:&str,sequence:u64,terminal:&Value,now:u64)->Result<RearmPlan>{
        need(self.rearm_hints(bank,log,scope,now)?.iter().any(|h|h==hint)&&digest(scope)?==digest(transport.scope())?,"FUNDAMENTAL_REARM_HINT_CHANGED")?;
        no_existing_liability(bank,log,&hint.exchange_id)?;let queue=queue_view(transport)?;let c=&self.cohorts[&hint.exchange_id];let b=c.entry.as_ref().ok_or("FUNDAMENTAL_REARM_ORIGIN")?;
        let(mut records,ordinary)=self.original_closed_records(log,&[hint.qid].into())?;let rows=records.remove(&hint.qid).ok_or("FUNDAMENTAL_REARM_ORIGINAL_ROWS")?;
        let proof=json!({"actualId":hint.actual_id,"rows":rows,"ack":log.acknowledgments[&hint.qid],"recovered":log.recovered.get(&hint.qid),"order":order,"fills":fills});
        let mut event=terminal.clone();let reference=event.as_object_mut().ok_or("FUNDAMENTAL_REARM_TERMINAL")?.remove("sourceRef");let sha=digest(&event)?;
        need(sequence>0&&bank.committed_source_hash(source_id,sequence)==Some(sha.as_str())&&reference.is_none_or(|r|r==json!({"id":source_id,"sequence":sequence,"hash":sha})),"FUNDAMENTAL_REARM_COMMITTED_TERMINAL")?;
        validate_zero_closure(b,&proof,&event)?;
        let mut last=daily_windows::clock(event["processedAt"].as_str().ok_or("FUNDAMENTAL_REARM_TERMINAL_CLOCK")?)?;let mut until=u64::MAX;let tid=&scope.tournament_id;
        let endpoints=[format!("/tournaments/{}",scope.tournament_slug),format!("/tournaments/{}/portfolio/positions",scope.tournament_slug),format!("/orders?status=open&limit=200&tournamentId={tid}"),format!("/account?tournamentId={tid}")];let receipts=[&f.account,&f.positions,&f.open_orders,&f.cash_balance];
        for(receipt,endpoint)in receipts.iter().zip(&endpoints){let(start,end)=entry_receipt(receipt,endpoint,now,20000)?;need(start>=last,"FUNDAMENTAL_REARM_ACCOUNT_CLOCK")?;last=end;until=until.min(start.checked_add(20000).ok_or("FUNDAMENTAL_CLOCK_RANGE")?);}
        need(f.account.data["id"]==*tid&&f.account.data["slug"]==scope.tournament_slug&&f.account.data["status"]=="active"&&f.positions.data.get("pagination").is_none()
            &&f.open_orders.data["coverage"]["complete"]==true&&f.open_orders.data["pagination"]["hasMore"]==false&&f.open_orders.data["pagination"]["nextCursor"].is_null(),"FUNDAMENTAL_REARM_ACCOUNT_COVERAGE")?;
        let mut seen=BTreeSet::new();for row in f.positions.data["positions"].as_array().ok_or("FUNDAMENTAL_REARM_POSITIONS")?{let ex=row["exchangeId"].as_str().ok_or("FUNDAMENTAL_REARM_POSITION_SCOPE")?;need(seen.insert(ex),"FUNDAMENTAL_REARM_DUPLICATE_POSITION")?;if ex==hint.exchange_id{need(row["quantity"].as_i64()==Some(0),"FUNDAMENTAL_REARM_NOT_FLAT")?;}}
        for row in f.open_orders.data["data"].as_array().ok_or("FUNDAMENTAL_REARM_OPEN_ORDERS")?{need(row["exchangeId"].as_str().is_some_and(|ex|ex!=hint.exchange_id),"FUNDAMENTAL_REARM_OPEN_ORDER")?;}
        need(baal_transport_service::relationships::parse_buying_power(&f.cash_balance.data,scope)?.gross_cash_units==Some(crate::authority::reconciled_expected_cash(bank)?),"FUNDAMENTAL_REARM_CASH")?;
        let inner=AbandonPlan{event:json!({"kind":"rearm_closed_zero","exchangeId":hint.exchange_id,"qid":hint.qid,"cohortSha256":digest(c)?,"originalSide":b.quote["side"],"originalBindingSha256":digest(b)?,"closedZeroEvidence":proof,"committedTerminal":{"id":source_id,"sequence":sequence,"sha256":sha,"event":event},"factsSha256":digest(&receipts)?}),
            head:bank.durable_head(),report_sha:digest(&bank.engine().report()?)?,dispatch:log.descriptor_identity()?,queue,scope_sha:digest(scope)?,account_sha:bytes_sha(transport.account_scope().as_bytes()),minted:now,until,
            sidecar:FilePin::from_held(self.file.try_clone()?,self.path.clone(),MAX_LOG,None)?,marker:self.marker.duplicate()?,ordinary,sdk_archive:None,_owner:self._owner.clone()};inner.validate(bank,log,transport,now)?;Ok(RearmPlan{inner})
    }
    pub fn apply_rearm(&mut self,plan:RearmPlan,bank:&Session,log:&DispatchLog,transport:&Transport,now:u64)->Result<()>{
        self.check(bank,log)?;plan.inner.validate(bank,log,transport,now)?;
        need(self.cohorts.get(plan.exchange_id()).is_some_and(|c|c.entry_id()==Some(plan.qid())&&digest(c).ok().as_ref()==plan.inner.event["cohortSha256"].as_str().map(str::to_owned).as_ref()),"FUNDAMENTAL_REARM_COHORT_CHANGED")?;
        self.append(plan.inner.event,now)?;self.check(bank,log)
    }
    // Exact FIFO attribution is required for new cumulative acquisitions. The
    // original single-entry verifier and all old serialized prefixes stay intact.
    fn cohort_lots(&self,bank:&Session,ex:&str,c:&Cohort)->Result<Value>{
        let mut lots:Vec<Value>=c.existing.as_ref().map(|o|o.lots.as_array().cloned().ok_or("FUNDAMENTAL_FIFO_LOTS")).transpose()?.unwrap_or_default();
        for f in &bank.engine().fills()[c.first_fill_index..]{if f["exchangeId"]!=ex{continue}
            let id=f["orderId"].as_u64().ok_or("FUNDAMENTAL_FILL_ID")?;let mut n=f["quantity"].as_u64().filter(|n|*n>0).ok_or("FUNDAMENTAL_FILL_QUANTITY")?;
            if c.is_entry(id)&&f["action"]=="buy"{need(f["executedAt"].is_string(),"FUNDAMENTAL_TOP_UP_EXECUTION_CLOCK")?;lots.push(json!({"quantity":n,"price":f["price"],"executedAt":f["executedAt"]}));}
            else if c.exits.contains_key(&id)&&f["action"]=="sell"{while n>0{let first=lots.first_mut().ok_or("FUNDAMENTAL_EXIT_EXCEEDS_COHORT")?;let q=first["quantity"].as_u64().ok_or("FUNDAMENTAL_FIFO_QUANTITY")?;let take=n.min(q);n-=take;if take==q{lots.remove(0);}else{first["quantity"]=json!(q-take);}}}
            else{return Err("FUNDAMENTAL_FOREIGN_FILL".into())}
        }Ok(json!(lots))
    }
    /// No HTTP or new authority: exact current ownership and prior durable
    /// closures, used before nomination and again around the native reservation.
    /// allow_drafts is read-hint only; a plan/bind requires no other reservation.
    fn top_up_state(&self,bank:&Session,log:&DispatchLog,ex:&str,current_qid:Option<u64>,allow_drafts:bool)->Result<Option<Value>>{
        if self.rank_paused(ex){return Ok(None)}let r=&self.policies[ex];let Some(p)=&r.same_side_top_ups else{return Ok(None)};let Some(c)=self.cohorts.get(ex)else{return Ok(None)};
        if (c.existing.is_none()&&(c.entry.is_none()||!c.exits.is_empty()))||(c.existing.is_some()&&!bank.engine().managed_inventory_planning_excludes(ex))||current_qid.is_some()&&current_qid==c.entry_id(){return Ok(None)}
        let prior:Vec<_>=c.entries().filter(|b|Some(b.qid)!=current_qid&&!c.retired_top_ups.contains_key(&b.qid)).collect();if (prior.is_empty()&&c.existing.is_none())||prior.len()+usize::from(c.existing.is_some())>=p.maximum_entries as usize||(c.top_ups.len()>=128&&!current_qid.is_some_and(|id|c.top_ups.contains_key(&id))){return Ok(None)}
        let owned=self.owned(bank,ex,c)?;let cost=bank.engine().wallet.market_cost(ex);
        if owned==0||owned>=r.maximum_quantity||cost>=amount(&r.maximum_principal)?{return Ok(None)}
        let lots=self.cohort_lots(bank,ex,c)?;need(bank.engine().wallet.positions()[ex][&r.side]==lots,"FUNDAMENTAL_TOP_UP_FIFO_CHANGED")?;
        let snapshot=crate::order_snapshot::snapshot(bank);let orders=snapshot.records();let mut proof=Vec::new();
        if prior.iter().any(|b|!log.attempted.contains(&b.qid)||!log.closed.contains(&b.qid)||!log.acknowledgments.contains_key(&b.qid)){return Ok(None)}
        let wanted=prior.iter().map(|b|b.qid).collect();let records=self.original_closed_records(log,&wanted)?.0;
        for b in prior{if !log.attempted.contains(&b.qid)||!log.closed.contains(&b.qid)||!log.acknowledgments.contains_key(&b.qid){return Ok(None)}
            let Some(actual)=bank.engine().live_order_bindings().get(&b.qid)else{return Ok(None)};
            let q=snapshot.quote(b.qid).ok_or("FUNDAMENTAL_NATIVE_QUOTE_MISSING")?;
            if q["retired"]!=true{return Ok(None)}
            let filled=bank.engine().fills().iter().filter(|f|f["orderId"]==b.qid).try_fold(0u64,|n,f|n.checked_add(f["quantity"].as_u64().ok_or("FUNDAMENTAL_FILL_QUANTITY")?).ok_or("FUNDAMENTAL_RANGE"))?;
            need(q["filled"]==filled,"FUNDAMENTAL_TOP_UP_FILLED_MISMATCH")?;
            let rows=records.get(&b.qid).ok_or("FUNDAMENTAL_TOP_UP_CLOSED_RECORDS")?;let terminal=&rows.last().ok_or("FUNDAMENTAL_TOP_UP_CLOSED_RECORDS")?["event"];
            need(hex(terminal["orderReceiptHash"].as_str().unwrap_or(""))&&hex(terminal["fillReceiptHash"].as_str().unwrap_or("")),"FUNDAMENTAL_TOP_UP_ORIGINAL_TERMINAL_HASHES")?;
            let ack:Receipt=serde_json::from_value(log.acknowledgments[&b.qid].clone())?;
            let event=if let Some(recovered)=log.recovered.get(&b.qid){let fills:Receipt=serde_json::from_value(recovered["fills"].clone())?;need(fills.endpoint==format!("/orders/{actual}/fills?limit=200"),"FUNDAMENTAL_TOP_UP_ORIGINAL_ACK_ENDPOINT")?;crate::feedback::recovered_acknowledgment(&b.quote,actual,&r.tournament_id,&ack,&fills,&fills.received_at)?}else{need(ack.endpoint=="/orders","FUNDAMENTAL_TOP_UP_ORIGINAL_ACK_ENDPOINT")?;crate::feedback::acknowledgment(&b.quote,&ack,&ack.received_at)?};
            self.validate_acknowledgment(b.qid,&event)?;need(event["actualOrderId"]==*actual,"FUNDAMENTAL_TOP_UP_ORIGINAL_ACK_CHANGED")?;
            proof.push(json!({"qid":b.qid,"actualOrderId":actual,"filledQuantity":filled,"closedDispatchSha256":digest(rows)?}));
        }
        if let Some(id)=current_qid{if log.closed.contains(&id)||log.acknowledgments.contains_key(&id)||bank.engine().live_order_bindings().contains_key(&id)||bank.engine().fills().iter().any(|f|f["orderId"]==id){return Ok(None)}}
        if orders.iter().any(|q|q["exchangeId"]==ex&&q["id"].as_u64()!=current_qid&&q["retired"]==false&&(!allow_drafts||q["id"].as_u64().is_none_or(|id|!log.never_attempted(id)||log.intents.contains_key(&id)))){return Ok(None)}
        if orders.iter().any(|q|q["exchangeId"]==ex&&q["id"].as_u64()!=current_qid&&q["id"].as_u64().is_some_and(|id|physical_pending(bank,log,id))){return Ok(None)}
        if bank.engine().package_blocks_submissions()||bank.engine().package_records().as_array().ok_or("FUNDAMENTAL_PACKAGE_RECORDS")?.iter().any(|p|p["phase"]!="no_execution"&&p["protectionReleased"]!=true&&(p["legs"].as_array().is_some_and(|ls|ls.iter().any(|l|l["exchangeId"]==ex))||p["portfolio"]["componentExchanges"].as_array().is_some_and(|ls|ls.iter().any(|l|l==ex)))){return Ok(None)}
        let mut original=c.clone();if let Some(id)=current_qid{original.top_ups.remove(&id);}
        if c.existing.is_some(){
            let Some(origin)=self.cohort_exit_origin(bank,log,ex,&original)?else{return Ok(None)};
            return Ok(Some(json!({"version":2,"cohortSha256":digest(&original)?,"positionsSha256":digest(&bank.engine().wallet.positions()[ex])?,"heldQuantity":owned,"heldCost":money(cost),"priorEntries":proof,"priorExits":origin["exits"],"inheritedOrigin":origin["inheritedOrigin"],"maximumEntries":p.maximum_entries,"maximumTotalQuantity":r.maximum_quantity})))
        }
        Ok(Some(json!({"version":1,"cohortSha256":digest(&original)?,"positionsSha256":digest(&bank.engine().wallet.positions()[ex])?,"heldQuantity":owned,"heldCost":money(cost),"priorEntries":proof,"maximumEntries":p.maximum_entries,"maximumTotalQuantity":r.maximum_quantity})))
    }
    /// Stale/missing books and missing ordinary quotes do not suppress reads.
    /// Physical/unknown liabilities and already-owned cohorts still suppress nomination.
    pub fn entry_hints(&self,bank:&Session,log:&DispatchLog,scope:&Scope,trusted:&BTreeSet<String>,now:u64)->Result<Vec<EntryHint>>{
        self.entry_hints_matching(bank,log,scope,trusted,now,None)
    }
    fn entry_hints_matching(&self,bank:&Session,log:&DispatchLog,scope:&Scope,trusted:&BTreeSet<String>,now:u64,only:Option<&str>)->Result<Vec<EntryHint>>{
        self.check(bank,log)?;log.require_resolved_attempts()?;let mut out=Vec::new();
        for (ex,r) in &self.policies{
            if self.rank_paused(ex)||r.version!=2||(r.existing_origin.is_some()&&!self.cohorts.contains_key(&r.exchange_id))||r.revoke_existing||!current(r,now)||now>=r.hold_until_ms||!trusted.contains(&digest(r)?)||structure(r)?<=0{continue}
            need(scope.tournament_id==r.tournament_id&&scope.exchanges.get(ex)==Some(&r.market_id),"FUNDAMENTAL_ENTRY_SCOPE")?;
            // Keep global integrity/unknown and every cheap review/scope check,
            // but rebuild costly ownership/closure proofs only for this packet.
            if only.is_some_and(|target|target!=ex){continue}
            let top_up=self.cohorts.contains_key(ex);
            if top_up&&self.top_up_state(bank,log,ex,None,true)?.is_none(){continue}
            if !top_up&&(bank.engine().wallet.quantity(ex,Side::Yes)>0||bank.engine().wallet.quantity(ex,Side::No)>0){continue}
            if crate::order_snapshot::snapshot(bank).records().iter().any(|q|q["exchangeId"]==*ex&&q["id"].as_u64().is_some_and(|id|physical_pending(bank,log,id))){continue}
            if bank.engine().package_records().as_array().ok_or("FUNDAMENTAL_PACKAGE_RECORDS")?.iter().any(|p|p["phase"]!="no_execution"&&p["protectionReleased"]!=true
                &&(p["legs"].as_array().is_some_and(|ls|ls.iter().any(|l|l["exchangeId"]==*ex))||p["portfolio"]["componentExchanges"].as_array().is_some_and(|ls|ls.iter().any(|l|l==ex)))){continue}
            let policy=bank.engine().policy();if policy["entriesEnabled"]!=true||policy["entryStopAt"].as_str().map(daily_windows::clock).transpose()?.is_some_and(|t|now>=t){continue}
            out.push(EntryHint{exchange_id:ex.clone(),market_id:r.market_id.clone(),contract_id:r.contract_id.clone(),side:r.side.clone(),review_sha256:digest(r)?});
        }need(out.len()<=32,"FUNDAMENTAL_ENTRY_HINT_BOUND")?;Ok(out)
    }
    /// DIAGNOSTIC ONLY: compares hypothetical execution modes from the exact
    /// installed review and actually admitted book. This grants no reservation,
    /// fill probability, request, retry or physical authority. The caller must
    /// obtain `trusted` from its separately pinned ReviewAuthority; the wrapper
    /// below additionally verifies that pin here. No transport/account reads.
    /// Horizon is remaining OPERATOR holding deadline, not promised cash release
    /// or settlement timing. The model SHA identifies the explicit conditional
    /// mixture assumptions, not a trained or independently calibrated model.
    pub fn compare_entry_actions(&self,bank:&Session,scope:&Scope,hint:&EntryHint,trusted:&BTreeSet<String>,now:u64)->Result<Option<crate::alpha_proposal::Comparison>>{
        need(!self.poisoned&&bank.manifest_hash().as_str()==Some(self.manifest.as_str())&&bank.live_account_seed().is_some_and(|s|s["runId"]==self.run),"FUNDAMENTAL_COMPARISON_BANK")?;
        self._owner.descriptor_identity()?;self.marker.validate()?;let pin=FilePin::from_held(self.file.try_clone()?,self.path.clone(),MAX_LOG,None)?;
        let bytes=read_file(&pin.file,pin.maximum)?;let line=bytes.strip_suffix(b"\n").ok_or("FUNDAMENTAL_TORN_TAIL")?.rsplit(|b|*b==b'\n').next().ok_or("FUNDAMENTAL_TAIL")?;
        let mut row:Value=serde_json::from_slice(line)?;let claimed=row.as_object_mut().ok_or("FUNDAMENTAL_ROW")?.remove("hash").ok_or("FUNDAMENTAL_HASH")?;
        need(row["sequence"]==self.sequence&&claimed==self.previous&&claimed==digest(&row)?,"FUNDAMENTAL_COMPARISON_POLICY_CHANGED")?;
        let Some(r)=self.policies.get(&hint.exchange_id)else{return Ok(None)};let review_sha=digest(r)?;
        need(hint.market_id==r.market_id&&hint.contract_id==r.contract_id&&hint.side==r.side&&hint.review_sha256==review_sha
            &&scope.tournament_id==r.tournament_id&&scope.exchanges.get(&r.exchange_id)==Some(&r.market_id),"FUNDAMENTAL_COMPARISON_HINT_CHANGED")?;
        if r.version!=2||(r.existing_origin.is_some()&&!self.cohorts.contains_key(&r.exchange_id))||r.revoke_existing||!trusted.contains(&review_sha)||!current(r,now)||now>=r.hold_until_ms
            {return Ok(None)}
        // Diagnostic comparison has no DispatchLog argument and grants no
        // plan/closure authority. prepare_entry independently proves closures.
        let (held,held_cost)=if let Some(c)=self.cohorts.get(&r.exchange_id){
            let Some(p)=&r.same_side_top_ups else{return Ok(None)};
            if (c.existing.is_none()&&!c.exits.is_empty())||c.entries().filter(|b|!c.retired_top_ups.contains_key(&b.qid)).count()+usize::from(c.existing.is_some())>=p.maximum_entries as usize||c.top_ups.len()>=128{return Ok(None)}
            let held=self.owned(bank,&r.exchange_id,c)?;let cost=bank.engine().wallet.market_cost(&r.exchange_id);
            if held==0||held>=r.maximum_quantity||cost>=amount(&r.maximum_principal)?{return Ok(None)}
            (held,cost)
        }else{if bank.engine().wallet.quantity(&r.exchange_id,Side::Yes)!=0||bank.engine().wallet.quantity(&r.exchange_id,Side::No)!=0{return Ok(None)}(0,0)};
        let ceiling=structure(r)?.min(UNIT-5000);if ceiling<5000{return Ok(None)}
        let mut until=r.expires_at_ms.min(r.hold_until_ms);for source in &r.sources{until=until.min(source.observed_at_ms.checked_add(source.maximum_age_ms).ok_or("FUNDAMENTAL_SOURCE_CLOCK_RANGE")?);}
        if let Some(stop)=bank.engine().policy()["entryStopAt"].as_str().map(daily_windows::clock).transpose()?{until=until.min(stop);}
        if until<=now{return Ok(None)}
        let model_sha=execution_model_sha(r)?;
        let bounds=crate::alpha_proposal::ReviewBounds{exchange_id:r.exchange_id.clone(),market_id:r.market_id.clone(),side:if r.side=="YES"{Side::Yes}else{Side::No},review_sha256:review_sha,
            model_sha256:model_sha,source_sha256:digest(&r.sources)?,reviewed_at_ms:r.reviewed_at_ms,valid_until_ms:until,horizon_ms:r.hold_until_ms.checked_sub(now).ok_or("FUNDAMENTAL_COMPARISON_HORIZON")?,
            expected_payout:add(structure(r)?,amount(&r.margin_per_share)?)?,acquisition_ceiling:ceiling,maximum_principal:amount(&r.maximum_principal)?.checked_sub(held_cost).ok_or("FUNDAMENTAL_RANGE")?,maximum_quantity:r.maximum_quantity.checked_sub(held).ok_or("FUNDAMENTAL_RANGE")?,
            modes:vec![crate::alpha_proposal::Mode::JoinBid,crate::alpha_proposal::Mode::ImproveOneTick,crate::alpha_proposal::Mode::BoundedLimitMayRest]};
        let result=crate::alpha_proposal::compare(bank,scope,&bounds,now)?;pin.validate()?;Ok(Some(result))
    }
    pub fn compare_entry_actions_with_authority(&self,bank:&Session,scope:&Scope,hint:&EntryHint,authority:&ReviewAuthority,now:u64)->Result<Option<crate::alpha_proposal::Comparison>>{
        authority.validate()?;let result=self.compare_entry_actions(bank,scope,hint,authority.trusted(),now)?;authority.validate()?;Ok(result)
    }
    /// Call ONLY after original seven scoped reads, actual source admission, and
    /// retirement of proved never-sent blocking drafts. Quantity is derived by
    /// the native primitive; no ordinary pair or stale quote is repurposed.
    pub fn prepare_entry(&self,bank:&Session,log:&DispatchLog,transport:&Transport,scope:&Scope,hint:&EntryHint,f:&crate::authority::SnapshotSet,authority:&ReviewAuthority,now:u64)->Result<Option<EntryPlan>>{
        authority.validate()?;let hints=self.entry_hints_matching(bank,log,scope,authority.trusted(),now,Some(&hint.exchange_id))?;
        if !hints.iter().any(|h|h==hint){return Ok(None)}
        need(digest(scope)?==digest(transport.scope())?,"FUNDAMENTAL_ENTRY_TRANSPORT_SCOPE")?;
        no_existing_liability(bank,log,&hint.exchange_id)?;let queue=queue_view(transport)?;let r=&self.policies[&hint.exchange_id];
        let top_up=if self.cohorts.contains_key(&hint.exchange_id){let Some(v)=self.top_up_state(bank,log,&hint.exchange_id,None,false)?else{return Ok(None)};Some(v)}else{None};
        let held=top_up.as_ref().and_then(|v|v["heldQuantity"].as_u64()).unwrap_or(0);
        let wanted=if r.side=="YES"{held as i64}else{-(held as i64)};let mut target_position_seen=false;
        let tid=&scope.tournament_id;let market=&hint.market_id;
        let endpoints=[format!("/tournaments/{}",scope.tournament_slug),format!("/tournaments/{}/portfolio/positions",scope.tournament_slug),format!("/orders?status=open&limit=200&tournamentId={tid}"),format!("/account?tournamentId={tid}"),format!("/markets/{market}?tournamentId={tid}"),format!("/markets/{market}/nodes?tournamentId={tid}"),format!("/markets/{market}/orderbook?tournamentId={tid}&depth=20")];
        let receipts=[&f.account,&f.positions,&f.open_orders,&f.cash_balance,&f.market,&f.contract,&f.book];let mut last=0;let mut fact_until=u64::MAX;
        for (i,(receipt,endpoint)) in receipts.iter().zip(&endpoints).enumerate(){let age=if i<4{20000}else{10000};let(start,end)=entry_receipt(receipt,endpoint,now,age)?;
            need(start>=last,"FUNDAMENTAL_ENTRY_RECEIPT_ORDER")?;last=end;fact_until=fact_until.min(start.checked_add(age).ok_or("FUNDAMENTAL_CLOCK_RANGE")?);}
        need(f.account.data["id"]==*tid&&f.account.data["slug"]==scope.tournament_slug&&f.account.data["status"]=="active"
            &&f.positions.data.get("pagination").is_none()&&f.open_orders.data["coverage"]["complete"]==true&&f.open_orders.data["pagination"]["hasMore"]==false
            &&f.open_orders.data["pagination"]["nextCursor"].is_null(),"FUNDAMENTAL_ENTRY_ACCOUNT_COVERAGE")?;
        let mut seen=BTreeSet::new();for p in f.positions.data["positions"].as_array().ok_or("FUNDAMENTAL_ENTRY_POSITIONS")?{
            let ex=p["exchangeId"].as_str().ok_or("FUNDAMENTAL_ENTRY_POSITION_SCOPE")?;need(seen.insert(ex),"FUNDAMENTAL_ENTRY_DUPLICATE_POSITION")?;
            if ex==hint.exchange_id{target_position_seen=true;need(p["quantity"].as_i64()==Some(wanted),if held==0{"FUNDAMENTAL_ENTRY_NOT_FLAT"}else{"FUNDAMENTAL_ENTRY_POSITION_MISMATCH"})?;}}
        need(held==0||target_position_seen,"FUNDAMENTAL_TOP_UP_POSITION_MISSING")?;
        for o in f.open_orders.data["data"].as_array().ok_or("FUNDAMENTAL_ENTRY_OPEN_ORDERS")?{
            need(o["exchangeId"].as_str().is_some_and(|ex|ex!=hint.exchange_id),"FUNDAMENTAL_ENTRY_PHYSICAL_LIABILITY")?;}
        let cash=baal_transport_service::relationships::parse_buying_power(&f.cash_balance.data,scope)?;
        need(cash.gross_cash_units==Some(crate::authority::reconciled_expected_cash(bank)?),"FUNDAMENTAL_ENTRY_CASH_RECONCILIATION")?;
        need(f.market.data["id"]==*market&&f.market.data["status"]=="open"&&f.market.data["isComposite"]==false&&f.market.data["isMultiOutcome"]==false
            &&f.market.data["contexts"].as_array().is_some_and(|cs|cs.iter().any(|c|c["status"]=="open"&&c["type"]=="tournament"&&c["tournament"]["id"]==*tid)),"FUNDAMENTAL_ENTRY_MARKET")?;
        validate_contract_data(r,&f.contract.data)?;
        let wrapper=json!({"source":"official_sig_api_v1","kind":"book","tournamentId":tid,"marketId":market,"exchangeId":hint.exchange_id,"endpoint":f.book.endpoint,"status":f.book.status,"requestStartedAt":f.book.request_started_at,"receivedAt":f.book.received_at,"data":f.book.data});
        let events=moloch_replay::source::receipt_events(&wrapper)?;let expected=events.iter().find(|e|e["type"]=="book").ok_or("FUNDAMENTAL_ENTRY_ORIGINAL_BOOK")?;
        let current_book=bank.engine().live_package_books().into_iter().find(|b|b["exchangeId"]==hint.exchange_id).ok_or("FUNDAMENTAL_ENTRY_NATIVE_BOOK")?;
        let mut compare=current_book.clone();if compare["invalid"]==false{compare.as_object_mut().ok_or("FUNDAMENTAL_ENTRY_NATIVE_BOOK")?.remove("invalid");}
        need(compare==expected["book"],"FUNDAMENTAL_ENTRY_BOOK_NOT_ADMITTED")?;
        let policy=bank.engine().policy();let age=policy["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?;
        let(books,hashes)=package_admission::native_books(bank,scope,now,age)?;let Some(book)=books.get(&hint.exchange_id)else{return Ok(None)};
        let(bids,asks)=if r.side=="YES"{(&book.yes_bids,&book.yes_asks)}else{(&book.no_bids,&book.no_asks)};
        let Some(bid)=bids.first()else{return Ok(None)};let Some(ask)=asks.first()else{return Ok(None)};
        if bid.price<=0||bid.price>=ask.price||bid.price>structure(r)?{return Ok(None)}
        need(policy["feeBps"]==0&&amount(policy["feePerShare"].as_str().ok_or("FUNDAMENTAL_ENTRY_FEES")?)?==0,"FUNDAMENTAL_ENTRY_UNKNOWN_FEES")?;
        let mut selected_mode=ExecutionMode::JoinBid;let mut selected_limit=bid.price;let mut selected_maximum=r.maximum_quantity;
        if let Some(execution)=&r.execution_policy{
            let Some(comparison)=self.compare_entry_actions_with_authority(bank,scope,hint,authority,now)?else{return Ok(None)};
            let Some((mode,candidate))=execution.allowed_modes.iter().find_map(|mode|comparison.candidates.iter().find(|c|c.mode()==mode.alpha()).map(|c|(*mode,c)))else{return Ok(None)};
            need(candidate.provenance()["reviewSha256"]==digest(r)?&&candidate.provenance()["modelSha256"]==execution_model_sha(r)?&&candidate.provenance()["sourceSha256"]==digest(&r.sources)?,"FUNDAMENTAL_EXECUTION_CANDIDATE")?;
            selected_mode=mode;selected_limit=candidate.limit_price();selected_maximum=candidate.quantity();
        }
        let mut until=r.expires_at_ms.min(r.hold_until_ms);for source in &r.sources{until=until.min(source.observed_at_ms.checked_add(source.maximum_age_ms).ok_or("FUNDAMENTAL_CLOCK_RANGE")?);}
        if let Some(stop)=policy["entryStopAt"].as_str().map(daily_windows::clock).transpose()?{until=until.min(stop);}
        let expires=now.checked_add(policy["lifetimeMs"].as_u64().ok_or("FUNDAMENTAL_LIFETIME")?).ok_or("FUNDAMENTAL_CLOCK_RANGE")?.min(until);
        if expires<=now.checked_add(policy["latencyMs"].as_u64().ok_or("FUNDAMENTAL_LATENCY")?).ok_or("FUNDAMENTAL_CLOCK_RANGE")?{return Ok(None)}
        let head=bank.durable_head();let report_sha=digest(&bank.engine().report()?)?;let dispatch=log.descriptor_identity()?;
        let origin=digest(&json!({"review":r,"receipts":receipts,"scope":scope,"bankHead":head,"dispatch":dispatch,"queue":queue}))?;
        let mut event=json!({"type":"reviewed_fundamental_entry_reserved_v1","source":"moloch_live_dispatcher","receivedAt":daily_windows::iso(now)?,"bankScope":{"runId":self.run,"bankManifestHash":self.manifest,"tournamentId":tid},"bankHead":head,"expectedReportSha256":report_sha,
            "marketId":market,"exchangeId":hint.exchange_id,"side":hint.side,"originSha256":origin,"reviewSha256":hint.review_sha256,"bookSha256":hashes.get(&hint.exchange_id).ok_or("FUNDAMENTAL_BOOK_HASH")?,"reviewCurrentUntilMs":until,"reviewMaximumQuantity":selected_maximum,"reviewMaximumPrincipal":r.maximum_principal,"expectedPayout":money(add(structure(r)?,amount(&r.margin_per_share)?)?),"marginPerShare":r.margin_per_share,"limitPrice":money(selected_limit),"expiresAt":daily_windows::iso(expires)?});
        if let Some(proof)=top_up{event["sameSideTopUp"]=proof;}
        if selected_mode!=ExecutionMode::JoinBid{event["executionMode"]=json!(selected_mode.wire());event["modelSha256"]=json!(execution_model_sha(r)?);event["sourceSha256"]=json!(digest(&r.sources)?);event["acquisitionCeiling"]=json!(money(execution_ceiling(r)?));}
        let plan=EntryPlan{event,scope_sha:digest(scope)?,head,report_sha,dispatch,queue,account_sha:bytes_sha(transport.account_scope().as_bytes()),review:r.clone(),minted:now,until:fact_until.min(until).min(expires).min(book.observed_ms.checked_add(age).ok_or("FUNDAMENTAL_CLOCK_RANGE")?).min(book.source.received_ms.checked_add(age).ok_or("FUNDAMENTAL_CLOCK_RANGE")?),
            sidecar:FilePin::from_held(self.file.try_clone()?,self.path.clone(),MAX_LOG,None)?,marker:self.marker.duplicate()?,authority:authority.pin.duplicate()?,_owner:self._owner.clone()};
        plan.validate_current(bank,log,transport,authority,now)?;Ok(Some(plan))
    }
    pub fn existing_exit_hint(&self,bank:&Session,log:&DispatchLog,scope:&Scope,exchange:&str,trusted:&BTreeSet<String>,now:u64)->Result<Option<ExistingExitHint>>{
        self.check(bank,log)?;if self.rank_paused(exchange){return Ok(None)} log.require_resolved_attempts()?;
        let Some(c)=self.cohorts.get(exchange).filter(|c|c.existing.is_some()||(c.entry.is_some()&&bank.engine().managed_inventory_planning_excludes(exchange)))else{return Ok(None)};
        if c.existing.is_none()&&c.pending_entry(log){return Ok(None)}
        if self.owned(bank,exchange,c)?==0||c.exits.keys().any(|id|log.attempted.contains(id)&&!log.closed.contains(id)){return Ok(None)}
        let r=&self.policies[exchange];need(scope.tournament_id==r.tournament_id&&scope.exchanges.get(exchange)==Some(&r.market_id),"FUNDAMENTAL_EXISTING_SCOPE")?;
        let reason=if liquidation_due(r,c,now){"reviewed_holding_deadline"}
            else if !trusted.contains(&digest(r)?)||!current(r,now){return Ok(None)}
            else if r.revoke_existing{"reviewed_source_revocation"}
            else{let(books,_)=package_admission::native_books(bank,scope,now,bank.engine().policy()["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?)?;
                if let Some(book)=books.get(exchange){let levels=if r.side=="YES"{&book.yes_bids}else{&book.no_bids};let payout=add(structure(r)?,amount(&r.margin_per_share)?)?.max(0);
                    if (c.existing.is_some()||c.entry.is_some())&&r.passive_value_exit.is_some()&&passive_exit_clear(bank,log,exchange,None,true)?&&passive_terms(r,bank,exchange,c,self.owned(bank,exchange,c)?.min(r.maximum_quantity).min(bank.engine().policy()["quantity"].as_u64().ok_or("FUNDAMENTAL_QUANTITY")?),if r.side=="YES"{book.yes_asks.first()}else{book.no_asks.first()}.map(|l|l.price))?.is_some(){"reviewed_passive_value_exit"}
                    else{if levels.first().is_none_or(|l|l.price<payout){return Ok(None)}"refreshed_value_exit"}
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
        snapshot.origin.review_sha256=r.sha.clone();let c=Cohort{entry:None,top_ups:BTreeMap::new(),retired_top_ups:BTreeMap::new(),existing:Some(snapshot.origin),first_fill_index:snapshot.fill_count,prior_fills_sha256:snapshot.fill_sha,bank_head_at_entry:snapshot.head,hold_until_ms:r.input.hold_until_ms,exits:BTreeMap::new(),rank_trades:BTreeMap::new()};
        self.append(json!({"kind":"existing_origin","review":r.input,"sha256":r.sha,"cohort":c}),now)
    }
    /// Proposes a genuine covered native SELL; this is not an API request or an
    /// execution. Retire proved never-sent drafts first, accept this event once,
    /// then bind its actual native qid/spec and mint the normal opaque token.
    pub fn prepare_existing_exit(&self,bank:&Session,log:&DispatchLog,transport:&Transport,scope:&Scope,exchange:&str,trusted:&BTreeSet<String>,now:u64)->Result<Option<ExistingExit>>{
        self.check(bank,log)?;if self.rank_paused(exchange){return Ok(None)} let queue=queue_view(transport)?;let Some(c)=self.cohorts.get(exchange)else{return Ok(None)};
        if c.existing.is_none()&&(!bank.engine().managed_inventory_planning_excludes(exchange)||c.entry.is_none()){return Ok(None)}
        let r=&self.policies[exchange];let owned=self.owned(bank,exchange,c)?;if owned==0{return Ok(None)}
        need(digest(scope)?==digest(transport.scope())?&&scope.tournament_id==r.tournament_id&&scope.exchanges.get(exchange)==Some(&r.market_id),"FUNDAMENTAL_EXISTING_SCOPE")?;
        no_existing_liability(bank,log,exchange)?;
        let deadline=c.hold_until_ms.min(r.hold_until_ms);let approved=trusted.contains(&digest(r)?)&&current(r,now);let payout=add(structure(r)?,amount(&r.margin_per_share)?)?.max(0);
        let reason=if liquidation_due(r,c,now){"reviewed_holding_deadline"}else if approved&&r.revoke_existing{"reviewed_source_revocation"}else if approved{"refreshed_value_exit"}else{return Ok(None)};
        let policy=bank.engine().policy();let (books,hashes)=package_admission::native_books(bank,scope,now,policy["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?)?;let Some(book)=books.get(exchange)else{return Ok(None)};
        let cap=owned.min(r.maximum_quantity).min(policy["quantity"].as_u64().ok_or("FUNDAMENTAL_QUANTITY")?);
        let passive=reason=="refreshed_value_exit"&&(c.existing.is_some()||c.entry.is_some())&&r.passive_value_exit.is_some()&&bank.engine().managed_inventory_planning_excludes(exchange);
        let mut fifo_cost=None;let(n,limit)=if passive{
            if !passive_exit_clear(bank,log,exchange,None,false)?{return Ok(None)}
            let Some((n,limit,cost))=passive_terms(r,bank,exchange,c,cap,if r.side=="YES"{book.yes_asks.first()}else{book.no_asks.first()}.map(|l|l.price))?else{return Ok(None)};fifo_cost=cost;(n,limit)
        }else{
            let levels=if r.side=="YES"{&book.yes_bids}else{&book.no_bids};let mut n=0u64;let mut limit=None;
            for l in levels{if reason=="refreshed_value_exit"&&l.price<payout{break}let take=(cap-n).min(l.quantity);if take>0{n+=take;limit=Some(l.price)}if n==cap{break}}
            let Some(limit)=limit else{return Ok(None)};(n,limit)
        };need(n>0,"FUNDAMENTAL_EXISTING_DEPTH")?;
        let until=r.sources.iter().try_fold(r.expires_at_ms,|end,s|Ok::<_,Box<dyn std::error::Error>>(end.min(s.observed_at_ms.checked_add(s.maximum_age_ms).ok_or("FUNDAMENTAL_SOURCE_CLOCK_RANGE")?)))?;
        let mut expires=now.checked_add(policy["lifetimeMs"].as_u64().ok_or("FUNDAMENTAL_LIFETIME")?).ok_or("FUNDAMENTAL_CLOCK_RANGE")?;
        if reason!="reviewed_holding_deadline"{expires=expires.min(until);if !terminal_hold(r){expires=expires.min(deadline);}}
        if expires<=now.checked_add(policy["latencyMs"].as_u64().ok_or("FUNDAMENTAL_LATENCY")?).ok_or("FUNDAMENTAL_CLOCK_RANGE")?{return Ok(None)}
        let mut event=json!({"type":"reviewed_inventory_exit_reserved_v1","source":"moloch_live_dispatcher","receivedAt":daily_windows::iso(now)?,"bankScope":{"runId":self.run,"bankManifestHash":self.manifest,"tournamentId":r.tournament_id},"bankHead":bank.durable_head(),"expectedReportSha256":digest(&bank.engine().report()?)?,"exchangeId":exchange,"marketId":r.market_id,"side":r.side,"quantity":n,"limitPrice":money(limit),"expiresAt":daily_windows::iso(expires)?,"originSha256":c.origin_sha()?,"reviewSha256":digest(r)?,"expectedPositionSha256":digest(&bank.engine().wallet.positions()[exchange])?,"bookSha256":hashes.get(exchange).ok_or("FUNDAMENTAL_BOOK_HASH")?,"reason":reason,"expectedPayout":money(payout),"reviewCurrentUntilMs":until,"holdingDeadlineMs":deadline,"revoked":r.revoke_existing});
        if passive{event["executionMode"]=json!("passiveValueLimit");event["maximumPassiveQuantity"]=json!(r.passive_value_exit.as_ref().unwrap().maximum_quantity);event["modelSha256"]=json!(execution_model_sha(r)?);event["sourceSha256"]=json!(digest(&r.sources)?);}
        if let Some(cost)=fifo_cost{let basis=fifo_basis(r,&event["bankHead"],&event["expectedPositionSha256"],&event["bookSha256"],n,&cost)?;
            event["reason"]=json!("reviewed_fifo_profit_turnover");event["pricingObjective"]=serde_json::to_value(r.passive_value_exit.as_ref().unwrap().pricing_objective.as_ref().unwrap())?;
            event["minimumNetSalePrice"]=basis["minimumNetSalePrice"].clone();event["forecastExpectedPayout"]=event["expectedPayout"].clone();event["modelSha256"]=json!(passive_model_sha(r,&basis)?);event["pricingBasis"]=basis;
        }
        if let Some(origin)=c.existing.as_ref().filter(|_|c.top_ups.is_empty()){event["adoptionRef"]=origin.adoption_ref.clone();event["adoptionReceiptHash"]=json!(origin.approval.adoption_receipt_sha256);}
        else{event["type"]=json!("reviewed_cohort_exit_reserved_v1");let Some(proof)=self.cohort_exit_origin(bank,log,exchange,c)?else{return Ok(None)};event["cohortOrigin"]=proof;}
        need(queue_view(transport)?==queue,"FUNDAMENTAL_EXISTING_QUEUE_CHANGED")?;Ok(Some(ExistingExit{event}))
    }
    fn cohort_exit_origin(&self,bank:&Session,log:&DispatchLog,ex:&str,c:&Cohort)->Result<Option<Value>>{
        // A proved no-network attempt is not a closed cohort entry. Preserve
        // its original resolution requirements instead of minting an exit
        // which the unchanged physical pending-entry gate cannot dispatch.
        if c.pending_entry(log){return Ok(None)}
        let r=&self.policies[ex];let lots=self.cohort_lots(bank,ex,c)?;need(bank.engine().wallet.positions()[ex][&r.side]==lots,"FUNDAMENTAL_EXIT_FIFO_CHANGED")?;
        let snapshot=crate::order_snapshot::snapshot(bank);let records=snapshot.records();let mut bindings=Vec::new();
        for b in c.entries().chain(c.exits.values()).chain(c.rank_trades.values()){
            if !bank.engine().live_order_bindings().contains_key(&b.qid){need(log.never_attempted(b.qid)||log.proved_no_network(b.qid)||log.exposure_resolved(b.qid),"FUNDAMENTAL_EXIT_UNKNOWN_BINDING")?;need(!bank.engine().fills().iter().any(|f|f["orderId"]==b.qid),"FUNDAMENTAL_EXIT_UNBOUND_FILL")?;continue}
            if !log.closed.contains(&b.qid)||!log.acknowledgments.contains_key(&b.qid){return Ok(None)}bindings.push(b);
        }
        let wanted=bindings.iter().map(|b|b.qid).collect();let closed=self.original_closed_records(log,&wanted)?.0;let mut entries=Vec::new();let mut exits=Vec::new();
        for b in bindings{let q=records.iter().find(|q|q["id"]==b.qid).ok_or("FUNDAMENTAL_EXIT_NATIVE_QUOTE")?;need(q["retired"]==true,"FUNDAMENTAL_EXIT_PRIOR_OPEN")?;
            let actual=&bank.engine().live_order_bindings()[&b.qid];let rows=closed.get(&b.qid).ok_or("FUNDAMENTAL_EXIT_CLOSED_ROWS")?;let terminal=&rows.last().ok_or("FUNDAMENTAL_EXIT_CLOSED_ROWS")?["event"];
            need(hex(terminal["orderReceiptHash"].as_str().unwrap_or(""))&&hex(terminal["fillReceiptHash"].as_str().unwrap_or("")),"FUNDAMENTAL_EXIT_TERMINAL_HASHES")?;
            let ack:Receipt=serde_json::from_value(log.acknowledgments[&b.qid].clone())?;
            let event=if let Some(recovered)=log.recovered.get(&b.qid){let fills:Receipt=serde_json::from_value(recovered["fills"].clone())?;need(fills.endpoint==format!("/orders/{actual}/fills?limit=200"),"FUNDAMENTAL_EXIT_ACK_ENDPOINT")?;crate::feedback::recovered_acknowledgment(&b.quote,actual,&r.tournament_id,&ack,&fills,&fills.received_at)?}else{need(ack.endpoint=="/orders","FUNDAMENTAL_EXIT_ACK_ENDPOINT")?;crate::feedback::acknowledgment(&b.quote,&ack,&ack.received_at)?};
            self.validate_acknowledgment(b.qid,&event)?;need(event["actualOrderId"]==*actual,"FUNDAMENTAL_EXIT_ACK_CHANGED")?;
            let row=json!({"qid":b.qid,"actualOrderId":actual,"filledQuantity":q["filled"],"bindingSha256":digest(b)?,"closedDispatchSha256":digest(rows)?});if c.is_entry(b.qid){entries.push(row)}else{exits.push(row)}
        }
        entries.sort_by_key(|r|r["qid"].as_u64());exits.sort_by_key(|r|r["qid"].as_u64());need((!entries.is_empty()||c.existing.is_some())&&entries.len()<=16&&exits.len()<=4096,"FUNDAMENTAL_EXIT_ORIGIN_LIMIT")?;
        let mut proof=json!({"version":1,"originSha256":c.origin_sha()?,"positionsSha256":digest(&bank.engine().wallet.positions()[ex])?,"heldQuantity":self.owned(bank,ex,c)?,"heldCost":money(bank.engine().wallet.market_cost(ex)),"entries":entries,"exits":exits});
        if let Some(o)=&c.existing{proof["version"]=json!(2);proof["cohortSha256"]=json!(digest(c)?);proof["priorEntries"]=proof["entries"].clone();proof["priorExits"]=proof["exits"].clone();proof["inheritedOrigin"]=json!({"receiptHash":o.approval.adoption_receipt_sha256,"adoptedPosition":o.adopted_position,"snapshotLots":o.lots,"firstFillIndex":c.first_fill_index,"priorFillsSha256":c.prior_fills_sha256});}
        Ok(Some(proof))
    }
    /// Mint after durable bind and before the original intent/attempt. After
    /// those original appends the worker MUST re-run check_bound, then the SDK
    /// performs its existing account/unknown/rate/physical authorization gates.
    pub fn physical_decision(&self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,spec:&RequestSpec,permit:&Permit,authority:&ReviewAuthority,now:u64)->Result<PhysicalDecision>{
        authority.validate()?;self.check_bound(bank,log,scope,qid,spec,authority.trusted(),now)?;
        let (ex,c)=self.cohorts.iter().find(|(_,c)|c.binding(qid).is_some()).ok_or("FUNDAMENTAL_DURABLE_BINDING_REQUIRED")?;
        let r=&self.policies[ex];let q=crate::order_snapshot::quote(bank,qid).ok_or("FUNDAMENTAL_QUOTE")?;
        let n=request_quantity(spec)?;let price=amount(q["price"].as_str().ok_or("FUNDAMENTAL_PRICE")?)?;
        need(q["retired"]==false&&q["activationAt"].as_u64().is_some_and(|t|t<=now)&&!q["cancelEffectiveAt"].is_u64()
            &&permit.owner=="Codex"&&permit.run_id==self.run&&permit.intent_id==qid.to_string()
            &&u64::from(permit.maximum_quantity)>=n&&amount(&permit.maximum_cost)?>=mul(price,n as i128)?,"FUNDAMENTAL_PHYSICAL_PERMIT_OR_NATIVE_WINDOW")?;
        let age=bank.engine().policy()["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?;
        let(books,_)=package_admission::native_books(bank,scope,now,age)?;let book=books.get(ex).ok_or("FUNDAMENTAL_PHYSICAL_BOOK_STALE")?;
        let mut expires=q["expiresAt"].as_u64().ok_or("FUNDAMENTAL_EXPIRATION")?.min(permit.expires_at_ms)
            .min(book.observed_ms.checked_add(age).and_then(|n|n.checked_add(1)).ok_or("FUNDAMENTAL_CLOCK_RANGE")?)
            .min(book.source.received_ms.checked_add(age).and_then(|n|n.checked_add(1)).ok_or("FUNDAMENTAL_CLOCK_RANGE")?);
        let deadline_exit=if c.is_entry(qid){false}else{
            matches!(self.assess(bank,log,scope,qid,authority.trusted(),now)?,Decision::Exit{reason,..}if reason=="reviewed_holding_deadline")
        };
        if !deadline_exit{
            expires=expires.min(r.expires_at_ms);
            if c.is_entry(qid)||!terminal_hold(r){expires=expires.min(r.hold_until_ms);}
            for s in &r.sources{expires=expires.min(s.observed_at_ms.checked_add(s.maximum_age_ms).and_then(|n|n.checked_add(1)).ok_or("FUNDAMENTAL_CLOCK_RANGE")?);}
        }else{need(liquidation_due(r,c,now),"FUNDAMENTAL_DEADLINE_NOT_REACHED")?;}
        need(now>=self.clock&&expires>now,"FUNDAMENTAL_PHYSICAL_WINDOW_EMPTY")?;
        let ordinary_file=File::open(&self.ordinary_path)?;need(identity(&ordinary_file)?==self.ordinary_identity,"FUNDAMENTAL_ORIGINAL_DISPATCH_REQUIRED")?;
        Ok(PhysicalDecision{marketable_reviewed_buy:q["action"]=="buy"&&execution_for_quote(r,&q)?==ExecutionMode::BoundedLimitMayRest,qid,planned_quantity:n,native_head:bank.durable_head(),quote_sha:digest(&q)?,ledger_sha:digest(&bank.engine().report()?)?,scope_sha:digest(scope)?,spec_sha:digest(spec)?,permit_sha:digest(permit)?,review_sha:digest(r)?,trust_sha:digest(authority.trusted())?,minted:now,expires,
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
            sequence:0,previous:"0".repeat(64),clock:0,poisoned:false,generation:None,policies:BTreeMap::new(),cohorts:BTreeMap::new(),abandoned:BTreeMap::new(),rank_bindings:BTreeMap::new(),aborted_archives:Default::default(),closed_rows_cache:Default::default(),not_sent_rows_cache:Default::default()};
        let mut reader=BufReader::new(out.file.try_clone()?);let mut line=Vec::new();
        loop{line.clear();let n=reader.by_ref().take((MAX_ROW+1) as u64).read_until(b'\n',&mut line)?;if n==0{break}
            need(line.len()<=MAX_ROW&&line.last()==Some(&b'\n'),"FUNDAMENTAL_TORN_OR_OVERSIZED_ROW")?;
            let mut row:Value=serde_json::from_slice(&line)?;let sha=row.as_object_mut().ok_or("FUNDAMENTAL_ROW")?.remove("hash").ok_or("FUNDAMENTAL_ROW_HASH")?;
            need(row["sequence"]==out.sequence+1&&row["previous"]==out.previous&&sha==digest(&row)?
                &&row["manifestHash"]==out.manifest&&row["runId"]==out.run,"FUNDAMENTAL_CHAIN_OR_BANK")?;
            let at=row["atMs"].as_u64().filter(|at|*at>=out.clock).ok_or("FUNDAMENTAL_CLOCK")?;
            out.apply(&row["event"])?;out.sequence+=1;out.previous=sha.as_str().ok_or("FUNDAMENTAL_HASH")?.into();out.clock=at;
        }
        out.check(bank,ordinary)?;
        if !out.abandoned.is_empty(){let wanted=out.abandoned.iter().filter(|(_,a)|a.closed_zero.is_none()).map(|(id,_)|*id).collect();let(records,_)=out.original_not_sent_records(ordinary,&wanted)?;
            for(id,a)in out.abandoned.iter().filter(|(_,a)|a.closed_zero.is_none()){need(records.get(id)==Some(&a.not_sent),"FUNDAMENTAL_ABANDONED_ORIGINAL_CHANGED")?;}
            let wanted=out.abandoned.iter().filter(|(_,a)|a.closed_zero.is_some()).map(|(id,_)|*id).collect();let(records,_)=out.original_closed_records(ordinary,&wanted)?;
            for(id,a)in out.abandoned.iter().filter(|(_,a)|a.closed_zero.is_some()){need(records.get(id).map(serde_json::to_value).transpose()?.as_ref()==a.closed_zero.as_ref().map(|v|&v["rows"]),"FUNDAMENTAL_REARMED_ORIGINAL_CHANGED")?;}}
        Ok(out)
    }
    fn apply(&mut self,event:&Value)->Result<()>{
        match event["kind"].as_str(){
            Some("review_generation")=>{
                let meta=&event["generation"];let request=&meta["request"];
                let raw=meta["requestBodyText"].as_str().filter(|s|s.len()<16*1024).ok_or("FUNDAMENTAL_GENERATION_ORIGINAL_REQUEST")?;
                let parsed:crate::fundamental_reload::Request=serde_json::from_str(raw)?;
                need(bytes_sha(raw.as_bytes())==meta["generationSha256"]&&serde_json::to_value(parsed)?==*request,"FUNDAMENTAL_GENERATION_ORIGINAL_REQUEST")?;
                need(meta["version"]==1&&hex(meta["generationSha256"].as_str().ok_or("FUNDAMENTAL_GENERATION_HASH")?)
                    &&request["previousGenerationSha256"]==self.generation_sha()
                    &&request["bankManifestSha256"]==self.manifest&&request["runId"]==self.run
                    &&meta["peer"].is_object()&&meta["controlIdentity"].is_object(),"FUNDAMENTAL_GENERATION_CHAIN")?;
                let rows=event["reviews"].as_array().filter(|r|!r.is_empty()&&r.len()<=32).ok_or("FUNDAMENTAL_GENERATION_REVIEWS")?;
                let origins:BTreeMap<String,Cohort>=match event.get("existingOrigins"){None=>BTreeMap::new(),Some(v)=>{need(event["existingOriginsVersion"]==1,"FUNDAMENTAL_GENERATION_EXISTING_VERSION")?;serde_json::from_value(v.clone())?}};
                need(origins.len()<=32&&origins.keys().all(|ex|rows.iter().any(|r|r["review"]["exchangeId"]==*ex)),"FUNDAMENTAL_GENERATION_EXISTING_SCOPE")?;
                let mut exchanges=BTreeSet::new();
                for row in rows{let r:ReviewInput=serde_json::from_value(row["review"].clone())?;
                    need(exchanges.insert(r.exchange_id.clone()),"FUNDAMENTAL_GENERATION_DUPLICATE")?;
                    if let Some(c)=origins.get(&r.exchange_id){self.apply(&json!({"kind":"existing_origin","review":r,"sha256":row["sha256"],"cohort":c}))?;}
                    else if self.policies.get(&r.exchange_id).is_some_and(|old|digest(old).ok().as_deref()==row["sha256"].as_str()){
                        need(row["sha256"]==digest(&r)?,"FUNDAMENTAL_GENERATION_REVIEW_HASH")?;
                    }else{self.apply(&json!({"kind":"review","review":r,"sha256":row["sha256"]}))?;}
                }
                need(self.policies.keys().all(|ex|exchanges.contains(ex)),"FUNDAMENTAL_GENERATION_REMOVAL")?;
                self.generation=Some(meta.clone());
            },
            Some("review")=>{let r:ReviewInput=serde_json::from_value(event["review"].clone())?;structure(&r)?;
                need(event["sha256"]==review_sha256(&r)?&&r.run_id==self.run&&r.bank_manifest_sha256==self.manifest,"FUNDAMENTAL_REVIEW_BANK")?;
                if let Some(old)=self.policies.get(&r.exchange_id){need(old.market_id==r.market_id&&old.contract_id==r.contract_id&&old.tournament_id==r.tournament_id&&old.side==r.side&&old.existing_origin==r.existing_origin&&r.reviewed_at_ms>old.reviewed_at_ms,"FUNDAMENTAL_REVIEW_REVISION")?;}
                need(self.policies.contains_key(&r.exchange_id)||self.policies.len()<32,"FUNDAMENTAL_POLICY_COUNT")?;
                if let Some(c)=self.cohorts.get(&r.exchange_id){need(r.hold_until_ms<=c.hold_until_ms,"FUNDAMENTAL_CANNOT_EXTEND_COHORT")?;
                    need(self.policies.get(&r.exchange_id).is_some_and(|old|same_objective_horizon(old,&r)),"FUNDAMENTAL_CANNOT_REINTERPRET_COHORT_OBJECTIVE")?;}
                self.policies.insert(r.exchange_id.clone(),r);
            },
            Some("entry")=>{let ex=event["exchangeId"].as_str().ok_or("FUNDAMENTAL_EXCHANGE")?;let c:Cohort=serde_json::from_value(event["cohort"].clone())?;
                need(self.policies.contains_key(ex)&&!self.cohorts.contains_key(ex)&&c.exits.is_empty()&&c.top_ups.is_empty()&&c.entry.is_some()&&c.existing.is_none(),"FUNDAMENTAL_EXCLUSIVE_COHORT")?;self.cohorts.insert(ex.into(),c);},
            Some("top_up")=>{
                let ex=event["exchangeId"].as_str().ok_or("FUNDAMENTAL_EXCHANGE")?;let r=self.policies.get(ex).ok_or("FUNDAMENTAL_REVIEW_MISSING")?;
                let p=r.same_side_top_ups.as_ref().ok_or("FUNDAMENTAL_TOP_UP_NOT_APPROVED")?;let c=self.cohorts.get(ex).ok_or("FUNDAMENTAL_NO_COHORT")?;
                let b:Binding=serde_json::from_value(event["binding"].clone())?;
                need(((c.entry.is_some()&&c.existing.is_none()&&c.exits.is_empty())||(c.existing.is_some()&&p.version==2&&event["proof"]["version"]==2))&&c.entries().filter(|b|!c.retired_top_ups.contains_key(&b.qid)).count()+usize::from(c.existing.is_some())<p.maximum_entries as usize&&c.top_ups.len()<128&&!c.is_entry(b.qid)
                    &&event["cohortSha256"]==digest(c)?&&event["proof"]["cohortSha256"]==digest(c)?&&b.quote["reviewedFundamentalEntry"]["sameSideTopUp"]==event["proof"]
                    &&b.review_sha256==digest(r)?&&b.quote["action"]=="buy"&&b.quote["exchangeId"]==ex&&b.quote["side"]==r.side,"FUNDAMENTAL_TOP_UP_EVENT")?;
                self.cohorts.get_mut(ex).ok_or("FUNDAMENTAL_NO_COHORT")?.top_ups.insert(b.qid,b);
            },
            Some("existing_origin")=>{
                let r:ReviewInput=serde_json::from_value(event["review"].clone())?;structure(&r)?;let ex=r.exchange_id.clone();let c:Cohort=serde_json::from_value(event["cohort"].clone())?;
                need(r.run_id==self.run&&r.bank_manifest_sha256==self.manifest&&event["sha256"]==digest(&r)?&&!self.policies.contains_key(&ex)&&!self.cohorts.contains_key(&ex)
                    &&c.entry.is_none()&&c.top_ups.is_empty()&&c.exits.is_empty()&&c.existing.as_ref().is_some_and(|o|r.existing_origin.as_ref()==Some(&o.approval)),"FUNDAMENTAL_EXISTING_ORIGIN_EVENT")?;
                self.policies.insert(ex.clone(),r);self.cohorts.insert(ex,c);
            },
            Some("abandon_top_up_never_sent")=>{
                let ex=event["exchangeId"].as_str().ok_or("FUNDAMENTAL_EXCHANGE")?;let id=event["qid"].as_u64().ok_or("FUNDAMENTAL_QID")?;let c=self.cohorts.get(ex).ok_or("FUNDAMENTAL_NO_COHORT")?;
                let b=c.top_ups.get(&id).ok_or("FUNDAMENTAL_TOP_UP_BINDING")?;
                need(((c.existing.is_none()&&c.exits.is_empty())||(c.existing.is_some()&&b.quote["reviewedFundamentalEntry"]["sameSideTopUp"]["version"]==2))&&!c.retired_top_ups.contains_key(&id)&&event["cohortSha256"]==digest(c)?&&event["originalBindingSha256"]==digest(b)?
                    &&event["originalNotSent"].is_object()&&event["committedRetirement"].is_object(),"FUNDAMENTAL_TOP_UP_ABANDON_EVENT")?;
                self.cohorts.get_mut(ex).ok_or("FUNDAMENTAL_NO_COHORT")?.retired_top_ups.insert(id,RetiredTopUp{not_sent:event["originalNotSent"].clone(),retirement:event["committedRetirement"].clone()});
            },
            Some("abandon_never_sent"|"abandon_proved_no_network")=>{
                let ex=event["exchangeId"].as_str().ok_or("FUNDAMENTAL_EXCHANGE")?;let id=event["qid"].as_u64().ok_or("FUNDAMENTAL_QID")?;
                let c=self.cohorts.get(ex).ok_or("FUNDAMENTAL_NO_COHORT")?;
                need(c.entry_id()==Some(id)&&c.existing.is_none()&&c.top_ups.is_empty()&&c.exits.is_empty()&&event["cohortSha256"]==digest(c)?
                    &&!self.abandoned.contains_key(&id)&&self.abandoned.len()<128&&event["originalNotSent"].is_object()&&event["committedRetirement"].is_object()
                    &&hex(event["factsSha256"].as_str().ok_or("FUNDAMENTAL_ABANDON_FACTS")?),"FUNDAMENTAL_ABANDON_EVENT")?;
                let c=self.cohorts.remove(ex).ok_or("FUNDAMENTAL_NO_COHORT")?;self.abandoned.insert(id,AbandonedCohort{exchange:ex.into(),cohort:c,not_sent:event["originalNotSent"].clone(),retirement:event["committedRetirement"].clone(),closed_zero:None});
            },
            Some("rearm_closed_zero")=>{
                let ex=event["exchangeId"].as_str().ok_or("FUNDAMENTAL_EXCHANGE")?;let id=event["qid"].as_u64().ok_or("FUNDAMENTAL_QID")?;let c=self.cohorts.get(ex).ok_or("FUNDAMENTAL_NO_COHORT")?;
                need(c.entry_id()==Some(id)&&c.existing.is_none()&&c.top_ups.is_empty()&&c.exits.is_empty()&&event["cohortSha256"]==digest(c)?&&!self.abandoned.contains_key(&id)&&self.abandoned.len()<128
                    &&event["closedZeroEvidence"].is_object()&&event["committedTerminal"].is_object()&&hex(event["factsSha256"].as_str().ok_or("FUNDAMENTAL_REARM_FACTS")?),"FUNDAMENTAL_REARM_EVENT")?;
                let c=self.cohorts.remove(ex).ok_or("FUNDAMENTAL_NO_COHORT")?;self.abandoned.insert(id,AbandonedCohort{exchange:ex.into(),cohort:c,not_sent:Value::Null,retirement:event["committedTerminal"].clone(),closed_zero:Some(event["closedZeroEvidence"].clone())});
            },
            Some("rank_trade")=>self.apply_rank_trade(event)?,
            Some("exit")=>{let ex=event["exchangeId"].as_str().ok_or("FUNDAMENTAL_EXCHANGE")?;let b:Binding=serde_json::from_value(event["binding"].clone())?;let c=self.cohorts.get_mut(ex).ok_or("FUNDAMENTAL_NO_COHORT")?;
                need(c.exits.len()<128&&!c.exits.contains_key(&b.qid),"FUNDAMENTAL_EXIT_BINDING_LIMIT_OR_DUPLICATE")?;c.exits.insert(b.qid,b);},
            _=>return Err("FUNDAMENTAL_EVENT".into()),
        }Ok(())
    }
    fn append(&mut self,event:Value,at:u64)->Result<()>{self.append_guarded(event,at,||Ok(()))}
    fn append_guarded(&mut self,event:Value,at:u64,final_boundary:impl FnOnce()->Result<()>)->Result<()>{
        need(!self.poisoned&&at>=self.clock,"FUNDAMENTAL_POISONED_OR_CLOCK")?;
        path_pin(&self.path,&identity(&self.file)?)?;
        // Apply to small metadata maps before writing; no financial state copy.
        let policies=self.policies.clone();let cohorts=self.cohorts.clone();let abandoned=self.abandoned.clone();let generation=self.generation.clone();let rank_bindings=self.rank_bindings.clone();
        let result=self.apply(&event);self.policies=policies;self.cohorts=cohorts;self.abandoned=abandoned;self.generation=generation;self.rank_bindings=rank_bindings;result?;
        let mut row=json!({"sequence":self.sequence+1,"previous":self.previous,"manifestHash":self.manifest,"runId":self.run,"atMs":at,"event":event});
        let sha=digest(&row)?;row["hash"]=json!(sha);let mut bytes=serde_json::to_vec(&row)?;bytes.push(b'\n');
        need(bytes.len()<=MAX_ROW&&self.file.metadata()?.len().checked_add(bytes.len() as u64).is_some_and(|n|n<=MAX_LOG),"FUNDAMENTAL_LOG_BOUND")?;
        final_boundary()?;
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
        for(id,a)in &self.abandoned{
            let b=a.cohort.entry.as_ref().ok_or("FUNDAMENTAL_ABANDONED_ORIGIN")?;
            let retirement=&a.retirement;let source=retirement["id"].as_str().ok_or("FUNDAMENTAL_ABANDONED_REFERENCE")?;let seq=retirement["sequence"].as_u64().ok_or("FUNDAMENTAL_ABANDONED_REFERENCE")?;
            need(bank.committed_source_hash(source,seq)==retirement["sha256"].as_str()&&retirement["sha256"]==digest(&retirement["event"])?&&retirement["event"]["orderId"]==*id,"FUNDAMENTAL_ABANDONED_SOURCE_CHANGED")?;
            let physical=if let Some(proof)=&a.closed_zero{
                need(log.attempted.contains(id)&&log.closed.contains(id)&&log.acknowledgments.get(id)==Some(&proof["ack"])
                    &&bank.engine().live_order_bindings().get(id).is_some_and(|actual|proof["actualId"]==*actual),"FUNDAMENTAL_REARMED_ORIGINAL_CHANGED")?;
                validate_zero_closure(b,proof,&retirement["event"])?;true
            }else{
                need(retirement["event"]["type"]=="unsubmitted_intent_retired"&&retirement["event"]["receiptHash"]==digest(&a.not_sent["event"]["proof"])? ,"FUNDAMENTAL_ABANDONED_SOURCE_CHANGED")?;
                let original=&a.not_sent["event"];
                let no_network=if original["kind"]=="aborted_before_admission"{
                    need(log.proved_no_network(*id)&&log.aborted_records().get(id)==Some(original),"FUNDAMENTAL_ABORT_PROOF_REQUIRED")?;
                    // Full raw archive verification at open/mint; cheap held-file
                    // identity/version checks thereafter, under the same-UID
                    // owner boundary. Do not hash SDK history on every quote.
                    let mut pins=self.aborted_archives.lock().map_err(|_|"FUNDAMENTAL_ABORT_ARCHIVE_LOCK")?;
                    if !pins.contains_key(id){pins.insert(*id,aborted_archive(log,*id)?);}
                    pins.get(id).ok_or("FUNDAMENTAL_ABORT_ARCHIVE_REQUIRED")?.validate_metadata()?;true
                }else{log.never_attempted(*id)};
                no_network&&!bank.engine().live_order_bindings().contains_key(id)
            };
            need(b.qid==*id&&a.cohort.existing.is_none()&&a.cohort.top_ups.is_empty()&&a.cohort.exits.is_empty()&&physical
                &&!bank.engine().fills().iter().any(|f|f["orderId"]==*id),"FUNDAMENTAL_ABANDONED_LATE_PHYSICAL_EVENT")?;
            need(a.cohort.first_fill_index<=bank.engine().fills().len()&&digest(&bank.engine().fills()[..a.cohort.first_fill_index])?==a.cohort.prior_fills_sha256
                &&digest(&b.spec)?==b.spec_sha256,"FUNDAMENTAL_ABANDONED_PREFIX_CHANGED")?;
            let q=crate::order_snapshot::quote(bank,*id).ok_or("FUNDAMENTAL_ABANDONED_QUOTE")?;
            need(q["retired"]==true&&q["filled"]==0&&q["exchangeId"]==a.exchange&&q["side"]==b.quote["side"]&&q["action"]==b.quote["action"]
                &&q["price"]==b.quote["price"]&&q["quantity"].as_u64()==Some(if a.closed_zero.is_some(){request_quantity(&b.spec)?}else{b.quote["quantity"].as_u64().ok_or("FUNDAMENTAL_QUANTITY")?}),"FUNDAMENTAL_ABANDONED_NATIVE_CHANGED")?;
            if let Some(original)=log.intents.get(id){let spec:RequestSpec=serde_json::from_value(original["spec"].clone())?;need(digest(&spec)?==b.spec_sha256&&original.get("fundamental")==self.durable_tag(*id).as_ref(),"FUNDAMENTAL_ABANDONED_ORIGINAL_CHANGED")?;}
        }
        let retired:BTreeSet<_>=self.cohorts.values().flat_map(|c|c.retired_top_ups.keys().copied()).collect();
        let records=if retired.is_empty(){BTreeMap::new()}else{self.original_not_sent_records(log,&retired)?.0};
        for(ex,c)in &self.cohorts{
            need(c.entry.is_some()!=c.existing.is_some(),"FUNDAMENTAL_ORIGIN_VARIANT")?;
            for(id,p)in &c.retired_top_ups{
                need(c.top_ups.contains_key(id)&&records.get(id)==Some(&p.not_sent)&&log.never_attempted(*id)&&!bank.engine().live_order_bindings().contains_key(id)
                    &&!bank.engine().fills().iter().any(|f|f["orderId"]==*id),"FUNDAMENTAL_TOP_UP_ABANDON_PHYSICAL")?;
                let e=&p.retirement["event"];let source=p.retirement["id"].as_str().ok_or("FUNDAMENTAL_ADOPTION_REF")?;let seq=p.retirement["sequence"].as_u64().ok_or("FUNDAMENTAL_ADOPTION_REF")?;
                need(bank.committed_source_hash(source,seq)==p.retirement["sha256"].as_str()&&p.retirement["sha256"]==digest(e)?&&e["type"]=="unsubmitted_intent_retired"
                    &&e["orderId"]==*id&&e["confirmedNetworkAttempts"]==0&&e["receiptHash"]==digest(&p.not_sent["event"]["proof"])?
                    &&crate::order_snapshot::snapshot(bank).records().iter().any(|q|q["id"]==*id&&q["retired"]==true&&q["filled"]==0),"FUNDAMENTAL_TOP_UP_ABANDON_SOURCE_CHANGED")?;
            }
            for b in c.entries().chain(c.exits.values()).chain(c.rank_trades.values()){
                need(digest(&b.spec)?==b.spec_sha256,"FUNDAMENTAL_BINDING_HASH")?;
                let n=request_quantity(&b.spec)?;request_matches(&b.spec,&b.quote,&self.run,&self.policies[ex].tournament_id,n)?;
                if let Some(original)=log.intents.get(&b.qid){let s:RequestSpec=serde_json::from_value(original["spec"].clone())?;need(digest(&s)?==b.spec_sha256&&original.get("fundamental")==self.durable_tag(b.qid).as_ref(),"FUNDAMENTAL_DISPATCH_REQUEST_OR_TAG_CHANGED")?;}
                else{need(log.never_attempted(b.qid),"FUNDAMENTAL_MISSING_ORIGINAL_INTENT")?;}
                let q=crate::order_snapshot::quote(bank,b.qid).ok_or("FUNDAMENTAL_NATIVE_QUOTE_MISSING")?;
                let expected=if bank.engine().live_order_bindings().contains_key(&b.qid){n}else{b.quote["quantity"].as_u64().ok_or("FUNDAMENTAL_QUANTITY")?};
                need(q["exchangeId"]==*ex&&q["side"]==b.quote["side"]&&q["marketId"]==b.quote["marketId"]&&q["action"]==b.quote["action"]&&q["price"]==b.quote["price"]&&q["quantity"].as_u64()==Some(expected),"FUNDAMENTAL_NATIVE_BINDING_CHANGED")?;
            }
            self.owned(bank,ex,c)?;
        }Ok(())
    }
    fn owned(&self,bank:&Session,ex:&str,c:&Cohort)->Result<u64>{
        if self.rank_paused(ex){return self.rank_owned(bank,ex,c)}
        let r=self.policies.get(ex).ok_or("FUNDAMENTAL_REVIEW_MISSING")?;let mut held=c.existing.as_ref().map_or(0,|o|o.quantity);
        need(c.first_fill_index<=bank.engine().fills().len(),"FUNDAMENTAL_BANK_PREFIX_MISSING")?;
        need(digest(&bank.engine().fills()[..c.first_fill_index])?==c.prior_fills_sha256
            &&bank.durable_head()["sequence"].as_u64()>=c.bank_head_at_entry["sequence"].as_u64(),"FUNDAMENTAL_FILL_PREFIX_CHANGED")?;
        for f in &bank.engine().fills()[c.first_fill_index..]{if f["exchangeId"]!=ex{continue}
            let id=f["orderId"].as_u64().ok_or("FUNDAMENTAL_FILL_ID")?;let n=f["quantity"].as_u64().ok_or("FUNDAMENTAL_FILL_QUANTITY")?;
            need(f["side"]==r.side&&f["counterfactual"]==false,"FUNDAMENTAL_FOREIGN_FILL")?;
            if c.is_entry(id)&&f["action"]=="buy"{held=held.checked_add(n).ok_or("FUNDAMENTAL_RANGE")?;}
            else if c.exits.contains_key(&id)&&f["action"]=="sell"{held=held.checked_sub(n).ok_or("FUNDAMENTAL_EXIT_EXCEEDS_COHORT")?;}
            else{return Err("FUNDAMENTAL_FOREIGN_FILL".into())}
        }
        let side=if r.side=="YES"{Side::Yes}else{Side::No};let opposite=if r.side=="YES"{Side::No}else{Side::Yes};
        need(bank.engine().wallet.quantity(ex,side)==held&&bank.engine().wallet.quantity(ex,opposite)==0,"FUNDAMENTAL_UNATTRIBUTED_HOLDING")?;
        if !c.top_ups.is_empty(){need(bank.engine().wallet.positions()[ex][&r.side]==self.cohort_lots(bank,ex,c)?,"FUNDAMENTAL_TOP_UP_FIFO_CHANGED")?;}
        if let Some(origin)=&c.existing{let remaining=self.cohort_lots(bank,ex,c)?;
            need(r.existing_origin.as_ref()==Some(&origin.approval)&&digest(&origin.adopted_position)?==origin.approval.adopted_position_sha256
                &&bank.committed_source_hash(origin.adoption_ref["id"].as_str().ok_or("FUNDAMENTAL_ADOPTION_REF")?,origin.adoption_ref["sequence"].as_u64().ok_or("FUNDAMENTAL_ADOPTION_REF")?)==origin.adoption_ref["sha256"].as_str()
                &&bank.engine().wallet.positions()[ex][&r.side]==remaining,"FUNDAMENTAL_EXISTING_FIFO_CHANGED")?;}
        Ok(held)
    }
    pub fn manages(&self,exchange:&str)->bool{self.policies.contains_key(exchange)}
    pub fn managed_exchanges(&self)->BTreeSet<String>{self.policies.keys().cloned().collect()}
    /// STARTUP RESTORATION ONLY. A closed-zero archive may replay its original
    /// immutable official ACK after independent closure proof is reverified.
    /// This accepts no new actual ID, economics, receipt or physical attempt.
    /// Normal live ACK admission MUST continue using validate_acknowledgment.
    pub fn validate_restored_acknowledgment(&self,bank:&Session,log:&DispatchLog,qid:u64,event:&Value)->Result<()>{
        self.check(bank,log)?;let Some(archive)=self.abandoned.get(&qid)else{return self.validate_acknowledgment(qid,event)};
        let proof=archive.closed_zero.as_ref().ok_or("FUNDAMENTAL_NEVER_SENT_ARCHIVE_HAS_NO_ACK")?;let binding=archive.cohort.entry.as_ref().ok_or("FUNDAMENTAL_REARM_ORIGIN")?;
        let(records,_)=self.original_closed_records(log,&[qid].into())?;
        need(records.get(&qid).map(serde_json::to_value).transpose()?.as_ref()==Some(&proof["rows"]),"FUNDAMENTAL_RESTORED_ORIGINAL_CHANGED")?;
        validate_zero_closure(binding,proof,&archive.retirement["event"])?;
        let ack:Receipt=serde_json::from_value(proof["ack"].clone())?;let processed=event["processedAt"].as_str().ok_or("FUNDAMENTAL_RESTORED_ACK_CLOCK")?;
        need(daily_windows::clock(processed)?>=daily_windows::clock(archive.retirement["event"]["processedAt"].as_str().ok_or("FUNDAMENTAL_REARM_TERMINAL_CLOCK")?)?,"FUNDAMENTAL_RESTORED_ACK_PRECEDES_CLOSURE")?;
        let expected=if proof["recovered"].is_null(){crate::feedback::acknowledgment(&binding.quote,&ack,processed)?}else{
            let fills:Receipt=serde_json::from_value(proof["recovered"]["fills"].clone())?;
            crate::feedback::recovered_acknowledgment(&binding.quote,proof["actualId"].as_str().ok_or("FUNDAMENTAL_REARM_ACTUAL_ID")?,binding.spec.body.as_ref().and_then(|v|v["tournamentId"].as_str()).ok_or("FUNDAMENTAL_REARM_TOURNAMENT")?,&ack,&fills,processed)?
        };
        need(*event==expected&&log.acknowledgments.get(&qid)==Some(&proof["ack"])&&log.closed.contains(&qid)
            &&bank.engine().live_order_bindings().get(&qid).is_some_and(|id|proof["actualId"]==*id),"FUNDAMENTAL_RESTORED_ACK_NOT_ORIGINAL")
    }
    /// Original POST and recovered-GET events pass here BEFORE native accept.
    /// The original quote's larger reservation is not proof of slice size.
    pub fn validate_acknowledgment(&self,qid:u64,event:&Value)->Result<()>{
        need(!self.abandoned.contains_key(&qid)&&!self.cohorts.values().any(|c|c.retired_top_ups.contains_key(&qid)),"FUNDAMENTAL_ABANDONED_ACK")?;
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
        self.check(bank,log)?;if self.rank_paused(exchange){return Ok(false)} let Some(r)=self.policies.get(exchange)else{return Ok(true)};
        let Some(c)=self.cohorts.get(exchange)else{return Ok(false)};
        if self.owned(bank,exchange,c)?==0||c.pending_entry(log)
            ||c.exits.keys().any(|id|log.attempted.contains(id)&&!log.closed.contains(id)){return Ok(false)}
        need(scope.tournament_id==r.tournament_id&&scope.exchanges.get(exchange)==Some(&r.market_id),"FUNDAMENTAL_SCOPE_CHANGED")?;
        if liquidation_due(r,c,now){return Ok(true)}
        if !trusted.contains(&digest(r)?)||!current(r,now){return Ok(false)}
        // A revised source/cap hash is attribution, not an executable exit reason.
        // Reprice against the CURRENT reviewed value below; never rewrite the
        // original entry binding or repeatedly preempt acquisitions on hash alone.
        if r.revoke_existing{return Ok(true)}
        let(books,_)=package_admission::native_books(bank,scope,now,bank.engine().policy()["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?)?;
        if (c.existing.is_some()||c.entry.is_some())&&r.passive_value_exit.is_some()&&books.contains_key(exchange)
            &&passive_exit_clear(bank,log,exchange,None,true)?{
            let book=&books[exchange];if passive_terms(r,bank,exchange,c,self.owned(bank,exchange,c)?.min(r.maximum_quantity).min(bank.engine().policy()["quantity"].as_u64().ok_or("FUNDAMENTAL_QUANTITY")?),if r.side=="YES"{book.yes_asks.first()}else{book.no_asks.first()}.map(|l|l.price))?.is_some(){return Ok(true)}}
        let value=add(structure(r)?,amount(&r.margin_per_share)?)?;
        Ok(books.get(exchange).and_then(|b|if r.side=="YES"{b.yes_bids.first()}else{b.no_bids.first()}).is_some_and(|l|l.price>=value))
    }
    pub fn durable_tag(&self,qid:u64)->Option<Value>{
        for c in self.cohorts.values().chain(self.abandoned.values().map(|a|&a.cohort)){if let Some(b)=c.binding(qid){
            let mut tag=json!({"version":1,"bankManifestHash":self.manifest,"runId":self.run,"qid":qid,"bindingSha256":digest(b).ok()?});
            if let Some(entry)=c.entry_id(){tag["entryQid"]=json!(entry)}else{tag["originKind"]=json!("reviewed_existing_snapshot");tag["originSha256"]=json!(c.origin_sha().ok()?)}return Some(tag);
        }}None
    }
    pub fn generation_poisoned(&self)->bool{self.poisoned}
    pub fn active_generation(&self)->Option<&Value>{self.generation.as_ref()}
    pub fn generation_sha(&self)->&str{self.generation.as_ref().and_then(|g|g["generationSha256"].as_str()).unwrap_or(crate::fundamental_reload::ZERO)}
    pub(crate) fn validate_generation_settings(&self,loaded:&LoadedSettings)->Result<()>{
        loaded.validate_files()?;
        let held=self._owner.descriptor_identity()?;
        need(self.generation.as_ref().is_some_and(|g|g["request"]["ownerLease"]["device"]==held["device"]&&g["request"]["ownerLease"]["inode"]==held["inode"]),"FUNDAMENTAL_GENERATION_OWNER_LEASE_CHANGED")?;
        need(loaded.reviews.len()==self.policies.len()&&loaded.reviews.iter().all(|r|self.policies.get(&r.input.exchange_id).is_some_and(|p|digest(p).ok().as_deref()==Some(r.sha.as_str()))),"FUNDAMENTAL_GENERATION_SETTINGS_CHANGED")
    }
    /// One fsynced append under the ORIGINAL held sidecar lock. Validation and
    /// trial-application precede the append; no per-review durable partial state.
    pub fn install_generation(&mut self,approved:&crate::fundamental_reload::Approved,loaded:&LoadedSettings,bank:&Session,log:&DispatchLog,scope:&Scope,f:&crate::authority::CancelSnapshotSet,base:&[u8],now:u64,final_boundary:impl FnOnce()->Result<()>)->Result<()>{
        self.install_generation_inner(approved,loaded,bank,log,scope,f,base,now,None,final_boundary)
    }
    /// New inherited scopes must be captured from the ORIGINAL committed
    /// adoption and CURRENT held owner/Transport. All origins and reviews are
    /// trial-applied and committed in ONE generation row, never install_existing
    /// followed by a separately fallible generation append.
    pub fn install_generation_with_existing(&mut self,approved:&crate::fundamental_reload::Approved,loaded:&LoadedSettings,bank:&Session,log:&DispatchLog,scope:&Scope,f:&crate::authority::CancelSnapshotSet,base:&[u8],now:u64,transport:&Transport,existing:Vec<ExistingSnapshot>,final_boundary:impl FnOnce()->Result<()>)->Result<()>{
        self.install_generation_inner(approved,loaded,bank,log,scope,f,base,now,Some((transport,existing)),final_boundary)
    }
    fn install_generation_inner(&mut self,approved:&crate::fundamental_reload::Approved,loaded:&LoadedSettings,bank:&Session,log:&DispatchLog,scope:&Scope,f:&crate::authority::CancelSnapshotSet,base:&[u8],now:u64,existing:Option<(&Transport,Vec<ExistingSnapshot>)>,final_boundary:impl FnOnce()->Result<()>)->Result<()>{
        let elapsed=std::time::Instant::now();
        let mut origins=BTreeMap::new();
        if let Some((transport,snapshots))=existing{for snapshot in snapshots{
            let ex=snapshot.origin.adopted_position["exchangeId"].as_str().ok_or("FUNDAMENTAL_EXISTING_SCOPE")?.to_owned();
            let r=loaded.reviews.iter().find(|r|r.input.exchange_id==ex).ok_or("FUNDAMENTAL_GENERATION_EXISTING_REVIEW")?;
            no_existing_liability(bank,log,&ex)?;
            need(snapshot.head==bank.durable_head()&&snapshot.report_sha==digest(&bank.engine().report()?)?&&snapshot.fill_count==bank.engine().fills().len()&&snapshot.fill_sha==digest(bank.engine().fills())?
                &&snapshot.dispatch==log.descriptor_identity()?&&snapshot.queue==queue_view(transport)?&&snapshot.scope_sha==digest(scope)?&&snapshot.scope_sha==digest(transport.scope())?&&snapshot.account_sha==bytes_sha(transport.account_scope().as_bytes())
                ,"FUNDAMENTAL_EXISTING_ORIGINAL_BOUNDARY_CHANGED")?;
            need(!self.policies.contains_key(&ex)&&!self.cohorts.contains_key(&ex)&&r.input.existing_origin.as_ref()==Some(&snapshot.origin.approval)
                &&snapshot.origin.adopted_position["marketId"]==r.input.market_id&&snapshot.origin.adopted_position["quantity"].as_i64().is_some_and(|n|if n<0{r.input.side=="NO"}else{r.input.side=="YES"})
                &&snapshot.origin.quantity<=r.input.maximum_quantity&&amount(&snapshot.origin.principal)?<=amount(&r.input.maximum_principal)?,"FUNDAMENTAL_EXISTING_SNAPSHOT_OR_REVIEW_CHANGED")?;
            let mut origin=snapshot.origin;origin.review_sha256=r.sha.clone();
            let cohort=Cohort{entry:None,top_ups:BTreeMap::new(),retired_top_ups:BTreeMap::new(),existing:Some(origin),first_fill_index:snapshot.fill_count,prior_fills_sha256:snapshot.fill_sha,bank_head_at_entry:snapshot.head,hold_until_ms:r.input.hold_until_ms,exits:BTreeMap::new(),rank_trades:BTreeMap::new()};
            need(origins.insert(ex,cohort).is_none(),"FUNDAMENTAL_GENERATION_EXISTING_DUPLICATE")?;
        }}
        self.check(bank,log)?;approved.validate(bank,log,&self._owner,base,self.generation_sha(),now)?;approved.validate_loaded(loaded,base)?;
        let incoming:BTreeSet<_>=loaded.reviews.iter().map(|r|r.input.exchange_id.clone()).collect();
        need(incoming.len()<=32&&self.policies.keys().all(|e|incoming.contains(e)),"FUNDAMENTAL_GENERATION_REMOVAL_OR_COUNT")?;
        let tid=&scope.tournament_id;let endpoints=[format!("/tournaments/{}",scope.tournament_slug),format!("/tournaments/{}/portfolio/positions",scope.tournament_slug),format!("/orders?status=open&limit=200&tournamentId={tid}"),format!("/account?tournamentId={tid}")];
        let receipts=[&f.account,&f.positions,&f.open_orders,&f.cash_balance];let mut last=0;
        for(receipt,endpoint)in receipts.iter().zip(&endpoints){let(start,end)=entry_receipt(receipt,endpoint,now,20000)?;need(start>=last,"FUNDAMENTAL_GENERATION_RECEIPT_ORDER")?;last=end;}
        need(f.account.data["id"]==*tid&&f.account.data["slug"]==scope.tournament_slug&&f.account.data["status"]=="active"
            &&f.positions.data.get("pagination").is_none()&&f.open_orders.data["coverage"]["complete"]==true
            &&f.open_orders.data["pagination"]["hasMore"]==false&&f.open_orders.data["pagination"]["nextCursor"].is_null(),"FUNDAMENTAL_GENERATION_ACCOUNT_COVERAGE")?;
        need(baal_transport_service::relationships::parse_buying_power(&f.cash_balance.data,scope)?.gross_cash_units==Some(crate::authority::reconciled_expected_cash(bank)?),"FUNDAMENTAL_GENERATION_CASH")?;
        let positions=f.positions.data["positions"].as_array().ok_or("FUNDAMENTAL_GENERATION_POSITIONS")?;let open=f.open_orders.data["data"].as_array().ok_or("FUNDAMENTAL_GENERATION_ORDERS")?;
        let mut seen=BTreeSet::new();for row in positions{let ex=row["exchangeId"].as_str().ok_or("FUNDAMENTAL_GENERATION_POSITION_SCOPE")?;need(seen.insert(ex)&&row["quantity"].as_i64().is_some(),"FUNDAMENTAL_GENERATION_POSITION_DUPLICATE_OR_QUANTITY")?;}
        for row in open{need(row["exchangeId"].as_str().is_some(),"FUNDAMENTAL_GENERATION_OPEN_SCOPE")?;}
        let snapshot=crate::order_snapshot::snapshot(bank);let orders=snapshot.records();let packages=bank.engine().package_records();
        for r in &loaded.reviews{let ex=&r.input.exchange_id;
            need(r.input.tournament_id==*tid&&scope.exchanges.get(ex)==Some(&r.input.market_id)&&r.input.bank_manifest_sha256==self.manifest&&r.input.run_id==self.run,"FUNDAMENTAL_GENERATION_SCOPE")?;
            let changed=self.policies.get(ex).is_none_or(|p|digest(p).ok().as_deref()!=Some(r.sha.as_str()));
            if !changed{continue}
            need(current(&r.input,now),"FUNDAMENTAL_INSTALL_REVIEW_EXPIRED")?;
            let signed=bank.engine().wallet.quantity(ex,Side::Yes) as i128-bank.engine().wallet.quantity(ex,Side::No) as i128;
            let actual=positions.iter().find(|row|row["exchangeId"]==*ex);
            need(actual.map_or(Some(0),|row|row["quantity"].as_i64().map(i128::from))==Some(signed)
                &&actual.is_none_or(|row|row.get("marketId").is_none_or(|m|m==&json!(r.input.market_id))),"FUNDAMENTAL_GENERATION_POSITION_ATTRIBUTION")?;
            need(!orders.iter().any(|q|q["exchangeId"]==*ex&&q["id"].as_u64().is_some_and(|id|physical_pending(bank,log,id)))
                &&!open.iter().any(|row|row["exchangeId"].as_str()==Some(ex.as_str())),"FUNDAMENTAL_GENERATION_TARGET_OPEN")?;
            if !self.policies.contains_key(ex){
                need(((r.input.existing_origin.is_none()&&bank.engine().wallet.quantity(ex,Side::Yes)==0&&bank.engine().wallet.quantity(ex,Side::No)==0&&bank.engine().wallet.market_cost(ex)==0
                    &&positions.iter().all(|row|row["exchangeId"]!=*ex||row["quantity"].as_i64()==Some(0)))||origins.contains_key(ex))
                    &&!orders.iter().any(|q|q["exchangeId"]==*ex&&q["retired"]!=true)
                    &&!packages.as_array().ok_or("FUNDAMENTAL_PACKAGE_RECORDS")?.iter().any(|p|generation_package_protects(p,ex)),"FUNDAMENTAL_GENERATION_NEW_SCOPE_NOT_FLAT")?;
            }
            if let Some(c)=self.cohorts.get(ex){
                let owned=self.owned(bank,ex,c)?;
                need(owned<=r.input.maximum_quantity&&bank.engine().wallet.market_cost(ex)<=amount(&r.input.maximum_principal)?,"FUNDAMENTAL_GENERATION_OWNED_CAP")?;
                if let Some(origin)=&c.existing{need(r.input.existing_origin.as_ref()==Some(&origin.approval),"FUNDAMENTAL_GENERATION_ORIGIN_CHANGED")?;}
            }
        }
        // Both source pins and actual bank/log ownership are rechecked after
        // all staging. A failed durable append poisons the existing writer.
        approved.validate_loaded(loaded,base)?;self.check(bank,log)?;
        let final_now=now.checked_add(u64::try_from(elapsed.elapsed().as_millis())?).ok_or("FUNDAMENTAL_CLOCK_RANGE")?;
        approved.validate(bank,log,&self._owner,base,self.generation_sha(),final_now)?;
        for(receipt,endpoint)in receipts.iter().zip(&endpoints){entry_receipt(receipt,endpoint,final_now,20000)?;}
        for r in &loaded.reviews{if self.policies.get(&r.input.exchange_id).is_none_or(|old|digest(old).ok().as_deref()!=Some(r.sha.as_str())){need(current(&r.input,final_now),"FUNDAMENTAL_INSTALL_REVIEW_EXPIRED")?;}}
        let mut event=json!({"kind":"review_generation","generation":approved.metadata(),"reviews":loaded.reviews.iter().map(|r|json!({"review":r.input,"sha256":r.sha})).collect::<Vec<_>>(),"nativeHead":bank.durable_head(),"dispatch":log.descriptor_identity()?,"factsSha256":digest(&receipts)?});
        if !origins.is_empty(){event["existingOriginsVersion"]=json!(1);event["existingOrigins"]=serde_json::to_value(origins)?;}
        self.append_guarded(event,final_now,final_boundary)
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
                &&!crate::order_snapshot::snapshot(bank).records().iter().any(|q|q["exchangeId"]==*ex
                    &&q["id"].as_u64().is_some_and(|id|physical_pending(bank,log,id)))
                &&!bank.engine().package_records().as_array().ok_or("FUNDAMENTAL_PACKAGE_RECORDS")?.iter().any(|p|
                    p["phase"]!="no_execution"&&p.get("protectionReleased")!=Some(&json!(true))
                    &&p["legs"].as_array().is_some_and(|legs|legs.iter().any(|l|l["exchangeId"]==*ex))),
                "FUNDAMENTAL_CANNOT_ADOPT_OR_CLAIM_EXISTING_EXPOSURE")?;
        }
        need(current(&r.input,now),"FUNDAMENTAL_INSTALL_REVIEW_EXPIRED")?;
        self.append(json!({"kind":"review","review":r.input,"sha256":r.sha}),now)
    }
    /// Ordinary assessment never resets a cohort. Explicit opaque abandonment
    /// or independently proved closed-zero rearm is required for that transition.
    pub fn assess(&self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,trusted:&BTreeSet<String>,now:u64)->Result<Decision>{
        let n=crate::order_snapshot::quote(bank,qid).and_then(|q|q["quantity"].as_u64()).ok_or("FUNDAMENTAL_QUANTITY")?;
        self.assess_quantity(bank,log,scope,qid,n,trusted,now)
    }
    /// Explicit one-shot planned size. Only new entries may be smaller than the
    /// immutable native draft; covered exits retain their full native quantity.
    pub fn assess_quantity(&self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,planned_quantity:u64,trusted:&BTreeSet<String>,now:u64)->Result<Decision>{
        self.check(bank,log)?;
        let q=crate::order_snapshot::quote(bank,qid).ok_or("FUNDAMENTAL_QUOTE")?;
        let ex=q["exchangeId"].as_str().ok_or("FUNDAMENTAL_EXCHANGE")?;let Some(r)=self.policies.get(ex)else{return Ok(Decision::Unmanaged)};
        if self.rank_paused(ex){return Ok(hold("tartarus_registered_inventory_handoff"))}
        if log.proved_no_network(qid){return Ok(hold("proved_aborted_intent_cannot_retry"))}
        need(scope.tournament_id==r.tournament_id&&scope.exchanges.get(ex)==Some(&r.market_id)&&q["marketId"]==r.market_id,"FUNDAMENTAL_SCOPE_CHANGED")?;
        if bank.engine().is_package_order(qid){return Ok(hold("package_cannot_consume_fundamental_inventory"))}
        if q["retired"]!=false||q["expiresAt"].as_u64().is_none_or(|t|t<=now)||q["cancelEffectiveAt"].is_u64(){return Ok(hold("inactive_native_quote"))}
        let approved=trusted.contains(&digest(r)?)&&current(r,now);
        let ceiling=structure(r)?;
        let (books,_)=package_admission::native_books(bank,scope,now,bank.engine().policy()["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?)?;
        let Some(book)=books.get(ex)else{return Ok(hold("fresh_official_book_required"))};
        let price=amount(q["price"].as_str().ok_or("FUNDAMENTAL_PRICE")?)?;let native=q["quantity"].as_u64().filter(|n|*n>0).ok_or("FUNDAMENTAL_QUANTITY")?;
        need(planned_quantity>0&&planned_quantity<=native,"FUNDAMENTAL_PLANNED_QUANTITY")?;let n=planned_quantity;
        let mut top_up=None;
        if let Some(c)=self.cohorts.get(ex){
            let owned=self.owned(bank,ex,c)?;
            if c.entry_id()==Some(qid)&&log.never_attempted(qid)&&!log.intents.contains_key(&qid)&&owned==0{
                let entry=c.entry.as_ref().unwrap();need(n==request_quantity(&entry.spec)?&&q==entry.quote,"FUNDAMENTAL_BOUND_ENTRY_CANNOT_CHANGE")?;
                if digest(r)?!=entry.review_sha256{return Ok(hold("bound_entry_review_changed"))}
                // Continue only this exact sealed draft after a crash/stale
                // preflight BEFORE original intent/attempt; never replace it.
            }else if q["action"]=="buy"&&q["side"]==r.side&&c.entry_id()!=Some(qid)&&r.same_side_top_ups.is_some(){
                if q["reviewedFundamentalEntry"].get("sameSideTopUp").is_none(){return Ok(hold("explicit_reviewed_top_up_reservation_required"))}
                let Some(proof)=self.top_up_state(bank,log,ex,Some(qid),false)?else{return Ok(hold("explicit_same_side_top_up_not_eligible"))};
                need(q["reviewedFundamentalEntry"]["sameSideTopUp"]==proof&&n==native,"FUNDAMENTAL_TOP_UP_NATIVE_PROOF_CHANGED")?;
                if let Some(b)=c.entry_binding(qid){need(q==b.quote&&n==request_quantity(&b.spec)?&&b.review_sha256==digest(r)?,"FUNDAMENTAL_BOUND_ENTRY_CANNOT_CHANGE")?;}
                top_up=Some(proof);
            }else{
            if q["action"]!="sell"||q["side"]!=r.side||owned==0{return Ok(hold("exclusive_cohort_no_additional_or_opposite_entry"))}
             if c.existing.is_none()&&bank.engine().managed_inventory_planning_excludes(ex){
                 if q["reviewedInventoryExit"]["originKind"]!="original_entry_cohort"{return Ok(hold("reviewed_cohort_exit_reservation_required"))}
                 need(q["reviewedInventoryExit"]["originSha256"]==c.origin_sha()?,"FUNDAMENTAL_COHORT_EXIT_ORIGIN_CHANGED")?;
                 if q["reviewedInventoryExit"]["reviewSha256"]!=digest(r)?{return Ok(hold("reviewed_cohort_exit_review_changed"))}
             }
            if c.existing.is_some()&&q.get("reviewedInventoryExit").is_none(){return Ok(hold("reviewed_existing_exit_reservation_required"))}
            need(n==native,"FUNDAMENTAL_EXIT_MUST_MATCH_NATIVE_QUANTITY")?;
            if c.pending_entry(log)||c.exits.keys().any(|id|*id!=qid&&log.attempted.contains(id)&&!log.closed.contains(id)){return Ok(hold("prior_owned_order_reconciliation_required"))}
            need(n<=owned,"FUNDAMENTAL_UNCOVERED_EXIT")?;
            if let Some(mode)=q["reviewedInventoryExit"].get("executionMode"){
                need(mode=="passiveValueLimit","FUNDAMENTAL_PASSIVE_EXIT_MODE")?;
                let p=r.passive_value_exit.as_ref().ok_or("FUNDAMENTAL_PASSIVE_EXIT_NOT_APPROVED")?;
                need((c.existing.is_some()||c.entry.is_some())&&bank.engine().managed_inventory_planning_excludes(ex)&&n<=p.maximum_quantity,"FUNDAMENTAL_PASSIVE_EXIT_ORIGIN_OR_QUANTITY")?;
                if !approved||r.revoke_existing||liquidation_due(r,c,now){return Ok(hold("current_passive_value_review_required"))}
                let payout=add(ceiling,amount(&r.margin_per_share)?)?.max(0);let metadata=&q["reviewedInventoryExit"];
                let fifo=fifo_gain(r)?.is_some();let (floor,reason)=if fifo{
                    let cost=cohort_fifo_cost(bank,ex,c,&r.side,n)?;let basis=&metadata["pricingBasis"];
                    let expected=fifo_basis(r,&basis["bankHead"],&json!(digest(&bank.engine().wallet.positions()[ex])?),&metadata["bookSha256"],n,&cost)?;
                    need(basis==&expected&&metadata["pricingObjective"]==serde_json::to_value(p.pricing_objective.as_ref().unwrap())?
                        &&metadata["minimumNetSalePrice"]==basis["minimumNetSalePrice"]&&metadata["forecastExpectedPayout"]==money(payout)
                        &&metadata["modelSha256"]==passive_model_sha(r,basis)?&&metadata.get("minimumNetValue").is_none(),"FUNDAMENTAL_FIFO_PROFIT_PROVENANCE")?;
                    (amount(basis["minimumNetSalePrice"].as_str().ok_or("FUNDAMENTAL_FIFO_COST")?)?,"reviewed_fifo_profit_turnover")
                }else{need(metadata["minimumNetValue"]==money(payout)&&metadata["modelSha256"]==execution_model_sha(r)?&&metadata.get("pricingObjective").is_none(),"FUNDAMENTAL_PASSIVE_EXIT_PROVENANCE")?;(payout,"refreshed_value_exit")};
                need(metadata["reason"]==reason&&metadata["reviewSha256"]==digest(r)?&&metadata["originSha256"]==c.origin_sha()?
                    &&metadata["maximumPassiveQuantity"]==p.maximum_quantity&&metadata["sourceSha256"]==digest(&r.sources)?,"FUNDAMENTAL_PASSIVE_EXIT_PROVENANCE")?;
                let policy=bank.engine().policy();let bps=u32::try_from(policy["feeBps"].as_u64().ok_or("FUNDAMENTAL_PASSIVE_EXIT_FEES")?)?;
                let fee=moloch_replay::fee(price,1,bps,amount(policy["feePerShare"].as_str().ok_or("FUNDAMENTAL_PASSIVE_EXIT_FEES")?)?)?;
                need(price.checked_sub(fee).is_some_and(|net|net>=floor),"FUNDAMENTAL_PASSIVE_EXIT_NET_VALUE")?;
                if !passive_exit_clear(bank,log,ex,Some(qid),false)?{return Ok(hold("passive_value_exit_competing_liability"))}
                // A value limit is authorization to rest, never evidence of a
                // maker fill or of full currently executable bid depth.
                return Ok(Decision::Exit{reason:reason.into(),owned_quantity:owned})
            }
            // Verify actual depth, not the last print or nominal quote price.
            let levels=if r.side=="YES"{&book.yes_bids}else{&book.no_bids};
            let mut left=n;let mut proceeds=0i128;
            for l in levels{if l.price<price{break}let take=left.min(l.quantity);proceeds=add(proceeds,mul(l.price,take as i128)?)?;left-=take;if left==0{break}}
            if left!=0{return Ok(hold("covered_exit_not_executable_at_native_limit"))}
            // Depth is an observation, not a guaranteed execution price. A
            // value exit must retain that value in its physical native limit.
            let payout=add(ceiling,amount(&r.margin_per_share)?)?.max(0);
            let reason=if liquidation_due(r,c,now){Some("reviewed_holding_deadline")}
                else if approved&&r.revoke_existing{Some("reviewed_source_revocation")}
                else if approved&&price>=payout&&proceeds>=mul(payout,n as i128)?{Some("refreshed_value_exit")}
                else{None};
            if c.existing.is_some(){need(q["reviewedInventoryExit"]["originSha256"]==c.origin_sha()?,"FUNDAMENTAL_EXISTING_EXIT_ORIGIN_CHANGED")?;
                if q["reviewedInventoryExit"]["reviewSha256"]!=digest(r)?{return Ok(hold("reviewed_existing_exit_review_changed"))}}
            return Ok(reason.map_or_else(||hold(if terminal_hold(r){"terminal_settlement_hold_for_actual_payout"}else if approved{"terminal_value_hold_not_native_age_exit"}else{"stale_review_hold_until_deadline"}),|s|Decision::Exit{reason:s.into(),owned_quantity:owned}))
            }
        }
        if r.version==1&&!self.cohorts.get(ex).is_some_and(|c|c.entry_id()==Some(qid)){return Ok(hold("legacy_review_requires_explicit_valuation_objective_for_new_cohort"))}
        if !approved||r.revoke_existing||now>=r.hold_until_ms{return Ok(hold("current_review_required_for_entry"))}
        if q["action"]!="buy"||q["side"]!=r.side{return Ok(hold("reviewed_entry_side_only"))}
        if top_up.is_none()&&(bank.engine().wallet.quantity(ex,Side::Yes)!=0||bank.engine().wallet.quantity(ex,Side::No)!=0){return Ok(hold("existing_inventory_cannot_be_adopted"))}
        if let Some(proof)=&top_up{let held=proof["heldQuantity"].as_u64().ok_or("FUNDAMENTAL_TOP_UP_QUANTITY")?;let cost=amount(proof["heldCost"].as_str().ok_or("FUNDAMENTAL_TOP_UP_COST")?)?;
            need(held.checked_add(n).is_some_and(|n|n<=r.maximum_quantity)&&add(cost,mul(price,n as i128)?)?<=amount(&r.maximum_principal)?,"FUNDAMENTAL_TOP_UP_TOTAL_CAP")?;}
        if log.attempted.iter().any(|id|!log.acknowledgments.contains_key(id)&&!log.proved_no_network(*id)&&!log.exposure_resolved(*id)){return Ok(hold("unknown_physical_acceptance"))}
        if crate::order_snapshot::snapshot(bank).records().iter().any(|other|other["exchangeId"]==ex
            &&other["id"].as_u64().is_some_and(|id|physical_pending(bank,log,id))){return Ok(hold("same_exchange_physical_order_open"))}
        if bank.engine().package_blocks_submissions(){return Ok(hold("unresolved_package"))}
        if now>=daily_windows::clock(bank.engine().policy()["entryStopAt"].as_str().ok_or("FUNDAMENTAL_ENTRY_DEADLINE")?)?{return Ok(hold("original_entry_window_expired"))}
        let source_until=r.sources.iter().try_fold(r.expires_at_ms,|end,s|Ok::<_,Box<dyn std::error::Error>>(end.min(s.observed_at_ms.checked_add(s.maximum_age_ms).ok_or("FUNDAMENTAL_SOURCE_CLOCK_RANGE")?)))?;
        if q["expiresAt"].as_u64().is_none_or(|t|t>source_until){return Ok(hold("native_order_outlives_reviewed_source_window"))}
        let asks=if r.side=="YES"{&book.yes_asks}else{&book.no_asks};
        let execution=match execution_for_quote(r,&q){Ok(mode)=>mode,Err(_)=>return Ok(hold("reviewed_execution_reservation_required_or_changed"))};
        if execution!=ExecutionMode::JoinBid{need(n==native,"FUNDAMENTAL_EXECUTION_WHOLE_NATIVE_REQUEST")?;}
        if execution!=ExecutionMode::BoundedLimitMayRest&&asks.first().is_none_or(|l|price>=l.price){return Ok(hold("ordinary_entry_must_remain_passive"))}
        if price>ceiling||n>r.maximum_quantity||mul(price,n as i128)?>amount(&r.maximum_principal)?{return Ok(hold("whole_native_quote_exceeds_reviewed_value_or_risk_cap"))}
        Ok(Decision::Entry{review_sha256:digest(r)?,maximum_principal:r.maximum_principal.clone(),ceiling:money(ceiling)})
    }
    /// Advisory size becomes authority only after explicit bind_quantity seals
    /// that exact number in the original RequestSpec. No price/quote mutation.
    pub fn planned_entry_quantity(&self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,trusted:&BTreeSet<String>,now:u64)->Result<Option<u64>>{
        self.check(bank,log)?;
        let q=crate::order_snapshot::quote(bank,qid).ok_or("FUNDAMENTAL_QUOTE")?;
        let ex=q["exchangeId"].as_str().ok_or("FUNDAMENTAL_EXCHANGE")?;
        let Some(r)=self.policies.get(ex)else{return Ok(None)};
        if q["action"]!="buy"||q["side"]!=r.side{return Ok(None)}
        if let Some(b)=self.cohorts.get(ex).and_then(|c|c.entry_binding(qid)){
            if !log.never_attempted(qid)||log.intents.contains_key(&qid){return Ok(None)}
            let n=request_quantity(&b.spec)?;
            return Ok(if matches!(self.assess_quantity(bank,log,scope,qid,n,trusted,now)?,Decision::Entry{..}){Some(n)}else{None})
        }
        let top_up=if self.cohorts.contains_key(ex){let Some(v)=self.top_up_state(bank,log,ex,Some(qid),false)?else{return Ok(None)};Some(v)}else{None};
        let held=top_up.as_ref().and_then(|v|v["heldQuantity"].as_u64()).unwrap_or(0);
        let cost=top_up.as_ref().map(|v|amount(v["heldCost"].as_str().ok_or("FUNDAMENTAL_TOP_UP_COST")?)).transpose()?.unwrap_or(0);
        let price=amount(q["price"].as_str().ok_or("FUNDAMENTAL_PRICE")?)?;
        let mut n=q["quantity"].as_u64().ok_or("FUNDAMENTAL_QUANTITY")?.min(r.maximum_quantity.checked_sub(held).ok_or("FUNDAMENTAL_RANGE")?);
        if price>0{n=n.min(u64::try_from((amount(&r.maximum_principal)?.checked_sub(cost).ok_or("FUNDAMENTAL_RANGE")?/price).min(u64::MAX as i128))?);}
        if n==0{return Ok(None)}
        Ok(if matches!(self.assess_quantity(bank,log,scope,qid,n,trusted,now)?,Decision::Entry{..}){Some(n)}else{None})
    }
    /// Qids with known actual ACKs only. Queue/original-log unknowns remain on
    /// their original recovery path; never invent an actual ID to cancel.
    pub fn entry_cancellations(&self,bank:&Session,log:&DispatchLog,trusted:&BTreeSet<String>,now:u64)->Result<Vec<u64>>{
        self.check(bank,log)?;let mut out=Vec::new();
        for(ex,c)in &self.cohorts{let r=&self.policies[ex];for entry in c.entries(){
            if !log.closed.contains(&entry.qid)&&log.acknowledgments.contains_key(&entry.qid)
                &&crate::order_snapshot::snapshot(bank).records().iter().any(|q|q["id"]==entry.qid&&q["retired"]==false)
                &&(!current(r,now)||r.revoke_existing||!trusted.contains(&digest(r)?)||digest(r)?!=entry.review_sha256||liquidation_due(r,c,now)){
                out.push(entry.qid);
            }
        }}Ok(out)
    }
    /// Revised/stale value assumptions also retire known still-open covered
    /// SELLs through the original cancel path; never invent unknown actual IDs.
    pub fn existing_exit_cancellations(&self,bank:&Session,log:&DispatchLog,trusted:&BTreeSet<String>,now:u64)->Result<Vec<u64>>{
        self.check(bank,log)?;let mut out=Vec::new();for(ex,c)in &self.cohorts{let r=&self.policies[ex];
            for b in c.exits.values(){if self.rank_bindings.contains_key(&b.qid){continue}
                let cohort_exit=c.existing.is_none()&&c.entry.is_some()&&b.quote["reviewedInventoryExit"]["originKind"]=="original_entry_cohort";
                if c.existing.is_none()&&!cohort_exit{continue}
                if !log.acknowledgments.contains_key(&b.qid)||log.closed.contains(&b.qid){continue}
                let Some(q)=crate::order_snapshot::quote(bank,b.qid).filter(|q|q["retired"]==false)else{continue};
                if cohort_exit{
                    need(q["reviewedInventoryExit"]==b.quote["reviewedInventoryExit"]&&q["reviewedInventoryExit"]["originSha256"]==c.origin_sha()?
                        &&q["exchangeId"]==*ex&&q["side"]==r.side&&q["action"]=="sell","FUNDAMENTAL_COHORT_CANCEL_ORIGIN_CHANGED")?;
                    let actual=bank.engine().live_order_bindings().get(&b.qid).ok_or("FUNDAMENTAL_COHORT_CANCEL_ACTUAL_REQUIRED")?;
                    let ack:Receipt=serde_json::from_value(log.acknowledgments[&b.qid].clone())?;
                    let event=if let Some(recovered)=log.recovered.get(&b.qid){let fills:Receipt=serde_json::from_value(recovered["fills"].clone())?;need(fills.endpoint==format!("/orders/{actual}/fills?limit=200"),"FUNDAMENTAL_COHORT_CANCEL_ACK_ENDPOINT")?;crate::feedback::recovered_acknowledgment(&b.quote,actual,&r.tournament_id,&ack,&fills,&fills.received_at)?}else{need(ack.endpoint=="/orders","FUNDAMENTAL_COHORT_CANCEL_ACK_ENDPOINT")?;crate::feedback::acknowledgment(&b.quote,&ack,&ack.received_at)?};
                    self.validate_acknowledgment(b.qid,&event)?;need(event["actualOrderId"]==*actual,"FUNDAMENTAL_COHORT_CANCEL_ACK_CHANGED")?;
                }
                let passive=b.quote["reviewedInventoryExit"]["executionMode"]=="passiveValueLimit";
                if digest(r)?!=b.review_sha256||((!trusted.contains(&digest(r)?)||!current(r,now))&&b.quote["reviewedInventoryExit"]["reason"]!="reviewed_holding_deadline")
                    ||passive&&(liquidation_due(r,c,now)||r.revoke_existing||r.passive_value_exit.is_none()||!bank.engine().managed_inventory_planning_excludes(ex)){out.push(b.qid);}
            }
        }Ok(out)
    }
    /// Persist exact original request BEFORE any physical attempt, never after
    /// observing a fill. No partial-size substitution or guessed ACK is allowed.
    pub fn bind(&mut self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,spec:&RequestSpec,trusted:&BTreeSet<String>,now:u64)->Result<Decision>{
        let n=crate::order_snapshot::quote(bank,qid).and_then(|q|q["quantity"].as_u64()).ok_or("FUNDAMENTAL_QUANTITY")?;
        self.bind_quantity(bank,log,scope,qid,n,spec,trusted,now)
    }
    pub fn bind_quantity(&mut self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,planned_quantity:u64,spec:&RequestSpec,trusted:&BTreeSet<String>,now:u64)->Result<Decision>{
        let decision=self.assess_quantity(bank,log,scope,qid,planned_quantity,trusted,now)?;
        need(matches!(decision,Decision::Entry{..}|Decision::Exit{..}),"FUNDAMENTAL_NOT_ELIGIBLE")?;
        need(log.never_attempted(qid)&&!log.intents.contains_key(&qid),"FUNDAMENTAL_BIND_MUST_PRECEDE_DISPATCH")?;
        let q=crate::order_snapshot::quote(bank,qid).ok_or("FUNDAMENTAL_QUOTE")?;let ex=q["exchangeId"].as_str().unwrap();let r=&self.policies[ex];
        request_matches(spec,&q,&self.run,&r.tournament_id,planned_quantity)?;
        if let Some(old)=self.cohorts.get(ex).and_then(|c|c.binding(qid)){
            need(old.spec_sha256==digest(spec)?&&old.quote==q&&old.review_sha256==digest(r)?
                &&request_quantity(&old.spec)?==planned_quantity,"FUNDAMENTAL_BOUND_REQUEST_CANNOT_CHANGE")?;
            return Ok(decision)
        }
        let b=Binding{qid,spec:spec.clone(),spec_sha256:digest(spec)?,quote:q.clone(),review_sha256:digest(r)?,bound_at_ms:now};
        if matches!(decision,Decision::Entry{..}){if let Some(c)=self.cohorts.get(ex){
            need(q["reviewedFundamentalEntry"]["sameSideTopUp"]["cohortSha256"]==digest(c)?,"FUNDAMENTAL_TOP_UP_COHORT_CHANGED")?;
            self.append(json!({"kind":"top_up","exchangeId":ex,"cohortSha256":digest(c)?,"binding":b,"proof":q["reviewedFundamentalEntry"]["sameSideTopUp"]}),now)?;
        }else{let c=Cohort{entry:Some(b),top_ups:BTreeMap::new(),retired_top_ups:BTreeMap::new(),existing:None,first_fill_index:bank.engine().fills().len(),prior_fills_sha256:digest(&bank.engine().fills())?,bank_head_at_entry:bank.durable_head(),hold_until_ms:r.hold_until_ms,exits:BTreeMap::new(),rank_trades:BTreeMap::new()};self.append(json!({"kind":"entry","exchangeId":ex,"cohort":c}),now)?;}}
        else{self.append(json!({"kind":"exit","exchangeId":ex,"binding":b}),now)?;}
        Ok(decision)
    }
    /// Policy-only physical gate. An existing attempt grants NO retry authority.
    /// The SDK/original dispatch retain unknown-acceptance and rate controls.
    pub fn check_bound(&self,bank:&Session,log:&DispatchLog,scope:&Scope,qid:u64,spec:&RequestSpec,trusted:&BTreeSet<String>,now:u64)->Result<()>{
        self.check(bank,log)?;
        need(!log.proved_no_network(qid)&&!log.exposure_resolved(qid)&&!self.cohorts.values().any(|c|c.retired_top_ups.contains_key(&qid)),"FUNDAMENTAL_ABORTED_INTENT_CANNOT_RETRY")?;
        let(ex,c)=self.cohorts.iter().find(|(_,c)|c.binding(qid).is_some()).ok_or("FUNDAMENTAL_DURABLE_BINDING_REQUIRED")?;
        let b=c.binding(qid).unwrap();need(digest(spec)?==b.spec_sha256,"FUNDAMENTAL_PHYSICAL_REQUEST_CHANGED")?;
        if c.is_entry(qid){
            let r=&self.policies[ex];need(trusted.contains(&digest(r)?)&&digest(r)?==b.review_sha256&&current(r,now)&&!r.revoke_existing&&now<c.hold_until_ms,"FUNDAMENTAL_PHYSICAL_REVIEW_CHANGED")?;
            if c.top_ups.contains_key(&qid){let proof=self.top_up_state(bank,log,ex,Some(qid),false)?.ok_or("FUNDAMENTAL_TOP_UP_PHYSICAL_OWNERSHIP")?;
                need(b.quote["reviewedFundamentalEntry"]["sameSideTopUp"]==proof,"FUNDAMENTAL_TOP_UP_NATIVE_PROOF_CHANGED")?;
                let held=proof["heldQuantity"].as_u64().ok_or("FUNDAMENTAL_TOP_UP_QUANTITY")?;let n=request_quantity(spec)?;
                let cost=amount(proof["heldCost"].as_str().ok_or("FUNDAMENTAL_TOP_UP_COST")?)?;
                need(held.checked_add(n).is_some_and(|n|n<=r.maximum_quantity)&&add(cost,mul(amount(b.quote["price"].as_str().ok_or("FUNDAMENTAL_PRICE")?)?,n as i128)?)?<=amount(&r.maximum_principal)?,"FUNDAMENTAL_TOP_UP_TOTAL_CAP")?;
            }else{need(self.owned(bank,ex,c)?==0,"FUNDAMENTAL_PHYSICAL_ENTRY_OWNERSHIP")?;}
            need(log.attempted.iter().all(|id|*id==qid||log.acknowledgments.contains_key(id)||log.proved_no_network(*id)||log.exposure_resolved(*id)),"FUNDAMENTAL_PHYSICAL_ENTRY_OWNERSHIP")?;
            need(scope.tournament_id==r.tournament_id&&scope.exchanges.get(ex)==Some(&r.market_id)
                &&now<daily_windows::clock(bank.engine().policy()["entryStopAt"].as_str().ok_or("FUNDAMENTAL_ENTRY_DEADLINE")?)?,"FUNDAMENTAL_PHYSICAL_SCOPE_OR_DEADLINE")?;
            let q=crate::order_snapshot::quote(bank,qid).ok_or("FUNDAMENTAL_QUOTE")?;
            need(q["retired"]==false&&q["quantity"]==b.quote["quantity"]&&q["activationAt"].as_u64().is_some_and(|t|t<=now)
                &&q["expiresAt"].as_u64().is_some_and(|t|t>now)&&!q["cancelEffectiveAt"].is_u64(),"FUNDAMENTAL_PHYSICAL_NATIVE_INACTIVE")?;
            let planned=request_quantity(&b.spec)?;request_matches(spec,&b.quote,&self.run,&r.tournament_id,planned)?;
            need(planned<=r.maximum_quantity&&mul(amount(b.quote["price"].as_str().ok_or("FUNDAMENTAL_PRICE")?)?,planned as i128)?<=amount(&r.maximum_principal)?,"FUNDAMENTAL_PHYSICAL_SLICE_CAP")?;
            let(books,_)=package_admission::native_books(bank,scope,now,bank.engine().policy()["bookAgeMs"].as_u64().ok_or("FUNDAMENTAL_BOOK_POLICY")?)?;
            let book=books.get(ex).ok_or("FUNDAMENTAL_PHYSICAL_BOOK_STALE")?;let asks=if r.side=="YES"{&book.yes_asks}else{&book.no_asks};let price=amount(b.quote["price"].as_str().ok_or("FUNDAMENTAL_PRICE")?)?;
            let execution=execution_for_quote(r,&q)?;if execution!=ExecutionMode::JoinBid{need(planned==q["quantity"].as_u64().ok_or("FUNDAMENTAL_QUANTITY")?,"FUNDAMENTAL_EXECUTION_WHOLE_NATIVE_REQUEST")?;}
            need(price<=structure(r)?&&(execution==ExecutionMode::BoundedLimitMayRest||asks.first().is_some_and(|l|price<l.price)),"FUNDAMENTAL_PHYSICAL_ENTRY_EDGE")?;
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

#[cfg(test)]mod generation_package_guard_tests{
 use super::*;
 #[test]fn completed_nonwire_portfolio_and_unexposed_probabilistic_components_stay_protected(){
  let p=json!({"phase":"complete","legs":[{"exchangeId":"101"}],"portfolio":{"componentExchanges":["101","102"]},"protectionReleased":false});assert!(generation_package_protects(&p,"102"));assert!(!generation_package_protects(&p,"103"));
  let mut p=json!({"phase":"complete","legs":[{"exchangeId":"101"}],"probabilistic":{"kind":"reviewed_probabilistic_portfolio"}});assert!(generation_package_protects(&p,"103"),"hidden nonwire scope cannot be inferred absent");p["protectionReleased"]=json!(true);assert!(!generation_package_protects(&p,"103"));
 }
}
