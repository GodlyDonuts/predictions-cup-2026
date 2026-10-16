//! Optional ordinary pairedSpread BUY selection, never financial authority.
//! Microprice is a displayed-depth proxy, not a calibrated fill-conditional EV.
//! Prices, fees and YES-floor/NO-complement rounding match the native planner.
use baal_transport_service::{prices::official_tick_units,Receipt,Scope};
use moloch_replay::{amount,fee,SCALE,TICK};
use serde::{Deserialize,Serialize};
use serde_json::Value;
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Mode { #[default] Disabled, Shadow, Enforce }
impl Mode {
    pub fn defers(self,a:&Assessment)->bool {
        self==Self::Enforce && matches!(a.class,Class::BelowMinimum|Class::Unavailable)
    }
}
#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize)]
#[serde(rename_all="snake_case")]
pub enum Class { NotApplicable, MeetsMinimum, BelowMinimum, Unavailable }
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Evidence {
    pub quote_sha256:String,
    pub policy_sha256:String,
    pub original_book_receipt_sha256:String,
    pub request_started_at_ms:u64,
    pub received_at_ms:u64,
    pub source_at_ms:u64,
    pub source_sequence:u64,
    pub assessed_at_ms:u64,
    pub valid_until_ms:u64,
    pub yes_microprice_micros:i128,
    pub side_value_micros:i128,
    pub limit_micros:i128,
    pub entry_fee_per_share_micros:i128,
    pub side_edge_micros:i128,
    pub minimum_edge_micros:i128,
}
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Assessment {
    pub class:Class,
    pub reason:String,
    pub evidence:Option<Evidence>,
    pub calibrated_probability:bool,
    pub execution_authority:bool,
}
fn result(class:Class,reason:impl Into<String>,evidence:Option<Evidence>)->Assessment {
    Assessment{class,reason:reason.into(),evidence,calibrated_probability:false,execution_authority:false}
}
type Checked<T>=Result<T,Box<dyn std::error::Error>>;
fn need(ok:bool,why:&'static str)->Checked<()> { if ok{Ok(())}else{Err(why.into())} }
fn hash(v:&impl Serialize)->Checked<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn clock(s:&str)->Checked<u64>{Ok(u64::try_from(chrono::DateTime::parse_from_rfc3339(s)?.timestamp_millis())?)}
fn decimal(v:&Value)->Checked<i128>{Ok(amount(v.as_str().ok_or("SIDE_EDGE_DECIMAL")?)?)}
fn levels(row:&Value,key:&str)->Checked<(i128,u64)> {
    let rows=row[key].as_array().filter(|v|!v.is_empty()&&v.len()<=200).ok_or("SIDE_EDGE_DEPTH")?;
    let mut previous=None;
    let mut top=None;
    for l in rows {
        let p=official_tick_units(&l["price"])?;
        let n=l["quantity"].as_u64().filter(|n|*n>0&&*n<=9_007_199_254_740_991).ok_or("SIDE_EDGE_QUANTITY")?;
        need((TICK..=SCALE-TICK).contains(&p)&&previous.is_none_or(|old|if key=="bids"{p<old}else{p>old}),"SIDE_EDGE_LEVEL")?;
        if top.is_none(){top=Some((p,n));} previous=Some(p);
    }
    top.ok_or_else(||"SIDE_EDGE_DEPTH".into())
}
fn checked(policy:&Value,scope:&Scope,q:&Value,r:&Receipt,now:u64)->Checked<Assessment> {
    let action=q["action"].as_str().filter(|s|["buy","sell"].contains(s)).ok_or("SIDE_EDGE_ACTION")?;
    if action!="buy" || q.get("reviewedFundamentalEntry").is_some_and(|v|!v.is_null()) || q.get("reviewedInventoryExit").is_some_and(|v|!v.is_null()) {
        return Ok(result(Class::NotApplicable,"ordinary_buy_only",None));
    }
    let mode=policy["quoteMode"].as_str().ok_or("SIDE_EDGE_POLICY")?;
    if mode!="pairedSpread" {return Ok(result(Class::NotApplicable,"paired_spread_only",None));}
    need(baal_transport_service::relationships::uuid(&scope.tournament_id),"SIDE_EDGE_TOURNAMENT")?;
    let ex=q["exchangeId"].as_str().ok_or("SIDE_EDGE_QUOTE_SCOPE")?;
    let market=q["marketId"].as_str().ok_or("SIDE_EDGE_QUOTE_SCOPE")?;
    need(scope.exchanges.get(ex).is_some_and(|m|m==market),"SIDE_EDGE_QUOTE_SCOPE")?;
    let side=q["side"].as_str().filter(|s|["YES","NO"].contains(s)).ok_or("SIDE_EDGE_SIDE")?;
    let n=q["quantity"].as_u64().filter(|n|*n>0).ok_or("SIDE_EDGE_QUOTE_QUANTITY")?;
    need(q["id"].as_u64().is_some_and(|id|id>0)&&q["filled"].as_u64()==Some(0)&&q["retired"]==false&&q.get("cancelEffectiveAt").is_none_or(Value::is_null),"SIDE_EDGE_QUOTE_STATE")?;
    let created=q["createdAt"].as_u64().ok_or("SIDE_EDGE_QUOTE_CLOCK")?;
    let activation=q["activationAt"].as_u64().ok_or("SIDE_EDGE_QUOTE_CLOCK")?;
    let expires=q["expiresAt"].as_u64().ok_or("SIDE_EDGE_QUOTE_CLOCK")?;
    need(n>0&&created<=activation&&activation<=now&&now<expires,"SIDE_EDGE_QUOTE_CLOCK")?;
    let limit=decimal(&q["price"])?;
    need((TICK..=SCALE-TICK).contains(&limit)&&limit%TICK==0,"SIDE_EDGE_QUOTE_PRICE")?;
    let age=policy["bookAgeMs"].as_u64().filter(|v|*v>0&&*v<=2_147_483_647).ok_or("SIDE_EDGE_POLICY_AGE")?;
    let minimum=decimal(&policy["minimumEdge"])?;
    need(minimum>0,"SIDE_EDGE_POLICY_EDGE")?;
    let bps=u32::try_from(policy["feeBps"].as_u64().filter(|v|*v<=2_147_483_647).ok_or("SIDE_EDGE_POLICY_FEES")?)?;
    let per_share=decimal(&policy["feePerShare"])?;
    // Exact same one-share fee ceiling used by Policy::fee(limit,1).
    let entry_fee=fee(limit,1,bps,per_share)?;
    need(r.status==200&&r.endpoint==format!("/markets/{market}/orderbook?tournamentId={}&depth=20",scope.tournament_id),"SIDE_EDGE_BOOK_ENDPOINT_OR_STATUS")?;
    let started=clock(&r.request_started_at)?;
    let received=clock(&r.received_at)?;
    need(started<=received&&received<=now&&now-started<=age,"SIDE_EDGE_RECEIPT_CLOCK")?;
    let contexts=r.data["contexts"].as_array().filter(|v|v.len()==1).ok_or("SIDE_EDGE_CONTEXT")?;
    need(contexts[0]["type"]=="tournament"&&contexts[0]["tournament"]["id"]==scope.tournament_id,"SIDE_EDGE_CONTEXT")?;
    let rows=contexts[0]["orderbook"]["exchanges"].as_array().ok_or("SIDE_EDGE_BOOK_ROWS")?;
    let expected:BTreeSet<_>=scope.exchanges.iter().filter(|(_,m)|m.as_str()==market).map(|(e,_)|e.as_str()).collect();
    need(rows.len()==expected.len(),"SIDE_EDGE_BOOK_COVERAGE")?;
    let mut seen=BTreeSet::new();
    let mut target=None;
    for row in rows {
        let id=row["exchangeId"].as_str().ok_or("SIDE_EDGE_BOOK_SCOPE")?;
        need(expected.contains(id)&&seen.insert(id),"SIDE_EDGE_BOOK_SCOPE")?;
        // An unrelated outcome may legitimately be stale or one-sided. Its
        // identity is needed for complete scope, not this target's proxy.
        if id!=ex{continue}
        let source=clock(row["asOf"]["at"].as_str().ok_or("SIDE_EDGE_SOURCE_CLOCK")?)?;
        let sequence=row["asOf"]["sequence"].as_u64().ok_or("SIDE_EDGE_SOURCE_SEQUENCE")?;
        need(source<=received&&now-source<=age,"SIDE_EDGE_SOURCE_CLOCK")?;
        let bid=levels(row,"bids")?;
        let ask=levels(row,"asks")?;
        need(bid.0<ask.0,"SIDE_EDGE_CROSSED_BOOK")?;
        if id==ex {target=Some((bid,ask,source,sequence));}
    }
    need(seen==expected,"SIDE_EDGE_BOOK_COVERAGE")?;
    let (bid,ask,source,sequence)=target.ok_or("SIDE_EDGE_TARGET_MISSING")?;
    let yes_micro=yes_microprice(bid,ask)?;
    // Native floors YES once, then complements it. Recomputing and flooring
    // a NO weighted average instead would lose one micro at uneven depths.
    let value=if side=="YES"{yes_micro}else{SCALE.checked_sub(yes_micro).ok_or("SIDE_EDGE_ARITHMETIC")?};
    let edge=value.checked_sub(limit).and_then(|v|v.checked_sub(entry_fee)).ok_or("SIDE_EDGE_ARITHMETIC")?;
    let until=started.checked_add(age).and_then(|v|source.checked_add(age).map(|s|v.min(s).min(expires))).ok_or("SIDE_EDGE_ARITHMETIC")?;
    let evidence=Evidence{quote_sha256:hash(q)?,policy_sha256:hash(policy)?,original_book_receipt_sha256:crate::feedback::digest(r)?,request_started_at_ms:started,received_at_ms:received,source_at_ms:source,source_sequence:sequence,assessed_at_ms:now,valid_until_ms:until,yes_microprice_micros:yes_micro,side_value_micros:value,limit_micros:limit,entry_fee_per_share_micros:entry_fee,side_edge_micros:edge,minimum_edge_micros:minimum};
    Ok(if edge<minimum {result(Class::BelowMinimum,"side_microprice_edge_below_original_minimum",Some(evidence))}else{result(Class::MeetsMinimum,"side_microprice_edge_meets_original_minimum",Some(evidence))})
}
/// Caller supplies the actual native policy/quote and ORIGINAL final REST book
/// from its unchanged seven-receipt physical preflight. No IO or renewed clocks.
/// Unavailable is explicit; it is a veto only in separately selected Enforce mode.
pub fn assess_original(policy:&Value,scope:&Scope,q:&Value,r:&Receipt,now:u64)->Assessment {
    match checked(policy,scope,q,r,now){Ok(a)=>a,Err(e)=>result(Class::Unavailable,e.to_string(),None)}
}

fn yes_microprice(bid:(i128,u64),ask:(i128,u64))->Checked<i128>{
    let numerator=ask.0.checked_mul(bid.1 as i128).and_then(|a|bid.0.checked_mul(ask.1 as i128).and_then(|b|a.checked_add(b))).ok_or("SIDE_EDGE_ARITHMETIC")?;
    let denominator=(bid.1 as i128).checked_add(ask.1 as i128).ok_or("SIDE_EDGE_ARITHMETIC")?;
    Ok(numerator.checked_div(denominator).ok_or("SIDE_EDGE_ARITHMETIC")?)
}

/// A native-admitted book reference, NOT a fabricated REST receipt. Displayed
/// depth includes own orders; this reuses the original proxy without claiming
/// an own-depth-adjusted probability, expected profit, or venue maker role.
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct RestingEvidence {
    pub qid:u64,pub actual_order_id:String,pub filled_quantity:u64,pub remaining_quantity:u64,
    pub native_head:Value,pub quote_sha256:String,pub policy_sha256:String,
    pub dispatch_descriptor_identity:Value,pub original_post_ack_sha256:String,
    pub native_book_sha256:String,pub source_sequence:String,
    pub request_started_at_ms:u64,pub received_at_ms:u64,pub source_at_ms:u64,
    pub original_post_ack_received_at_ms:u64,pub assessed_at_ms:u64,pub valid_until_ms:u64,
    pub yes_microprice_micros:i128,pub side_value_micros:i128,pub limit_micros:i128,
    pub entry_fee_per_share_micros:i128,pub side_edge_micros:i128,pub minimum_edge_micros:i128,
}
/// Only this module can construct a nomination. It never authorizes a DELETE;
/// the sole worker still needs its original owned-order/quiet/physical guards,
/// durable cancel intent and normal complete fill/terminal reconciliation.
#[derive(Debug)]
pub struct RestingDecision {evidence:RestingEvidence,context_sha256:String}
impl RestingDecision {
    pub fn qid(&self)->u64{self.evidence.qid}
    pub fn actual_order_id(&self)->&str{&self.evidence.actual_order_id}
    pub fn remaining_quantity(&self)->u64{self.evidence.remaining_quantity}
    pub fn valid_until_ms(&self)->u64{self.evidence.valid_until_ms}
    pub fn evidence(&self)->&RestingEvidence{&self.evidence}
    pub fn validate_current(&self,bank:&moloch_replay::session::Session,scope:&Scope,log:&crate::dispatch::DispatchLog,managed:&BTreeSet<String>,now:u64)->Checked<()> {
        need(now>=self.evidence.assessed_at_ms&&now<self.valid_until_ms(),"RESTING_EDGE_EXPIRED")?;
        let fresh=resting_checked(bank,scope,log,managed,self.qid(),now)?;
        let next=fresh.nomination.ok_or("RESTING_EDGE_NO_LONGER_BELOW")?;
        need(self.context_sha256==next.context_sha256,"RESTING_EDGE_CONTEXT_CHANGED")
    }
}
#[derive(Debug)]
pub struct RestingAssessment {pub class:Class,pub reason:String,nomination:Option<RestingDecision>}
impl RestingAssessment {
    pub fn nomination(&self)->Option<&RestingDecision>{self.nomination.as_ref()}
    pub fn into_nomination(self)->Option<RestingDecision>{self.nomination}
}
fn resting_result(class:Class,reason:&str)->RestingAssessment{RestingAssessment{class,reason:reason.into(),nomination:None}}
fn resting_checked(bank:&moloch_replay::session::Session,scope:&Scope,log:&crate::dispatch::DispatchLog,managed:&BTreeSet<String>,qid:u64,now:u64)->Checked<RestingAssessment>{
    need(bank.engine().is_live()&&bank.live_account_seed().is_some(),"RESTING_EDGE_LIVE_BANK")?;
    need(baal_transport_service::relationships::uuid(&scope.tournament_id)&&managed.iter().all(|ex|scope.exchanges.contains_key(ex)),"RESTING_EDGE_SCOPE")?;
    log.require_resolved_attempts()?;
    let orders=bank.engine().order_records();
    let q=orders.iter().find(|q|q["id"].as_u64()==Some(qid)).ok_or("RESTING_EDGE_QID")?;
    let ex=q["exchangeId"].as_str().ok_or("RESTING_EDGE_SCOPE")?;
    let market=q["marketId"].as_str().ok_or("RESTING_EDGE_SCOPE")?;
    need(scope.exchanges.get(ex).is_some_and(|m|m==market),"RESTING_EDGE_SCOPE")?;
    if q["action"]!="buy"||managed.contains(ex)||bank.engine().is_package_order(qid)
        ||["reviewedFundamentalEntry","reviewedInventoryExit"].iter().any(|k|q.get(*k).is_some_and(|v|!v.is_null())) {
        return Ok(resting_result(Class::NotApplicable,"ordinary_unmanaged_buy_only"))
    }
    // Complete package leases remain protected. Probabilistic non-wire held
    // components are not exposed in the public summary: conservatively bypass
    // ALL ordinary defensive nominations while such a protected lease exists.
    let packages=bank.engine().package_records();
    for p in packages.as_array().ok_or("RESTING_EDGE_PACKAGES")?{
        if p["phase"]=="no_execution"||p["protectionReleased"]==true{continue}
        if p.get("probabilistic").is_some_and(|v|!v.is_null())
            ||p["legs"].as_array().ok_or("RESTING_EDGE_PACKAGES")?.iter().any(|l|l["exchangeId"]==ex)
            ||p["portfolio"]["componentExchanges"].as_array().is_some_and(|xs|xs.iter().any(|v|v==ex)){
            return Ok(resting_result(Class::NotApplicable,"protected_package_inventory"))
        }
    }
    let side=q["side"].as_str().filter(|s|["YES","NO"].contains(s)).ok_or("RESTING_EDGE_SIDE")?;
    let opposite=if side=="YES"{moloch_replay::Side::No}else{moloch_replay::Side::Yes};
    if bank.engine().wallet.quantity(ex,opposite)>0{return Ok(resting_result(Class::NotApplicable,"any_opposite_inventory_coverage"))}
    if q["retired"]!=false||q.get("cancelEffectiveAt").is_some_and(|v|!v.is_null())||log.closed.contains(&qid)||log.cancel_acknowledged.contains(&qid)||log.cancel_attempted(qid){return Ok(resting_result(Class::NotApplicable,"already_terminal_or_canceling"))}
    let quantity=q["quantity"].as_u64().filter(|n|*n>0).ok_or("RESTING_EDGE_QUANTITY")?;
    let filled=q["filled"].as_u64().ok_or("RESTING_EDGE_QUANTITY")?;
    let remaining=quantity.checked_sub(filled).ok_or("RESTING_EDGE_QUANTITY")?;
    if remaining==0{return Ok(resting_result(Class::NotApplicable,"no_remaining_quantity"))}
    let actual=bank.engine().live_order_bindings().get(&qid).ok_or("RESTING_EDGE_NATIVE_BINDING")?;
    need(log.attempted.contains(&qid)&&!log.recovered.contains_key(&qid),"RESTING_EDGE_ORIGINAL_POST_REQUIRED")?;
    let intent=log.intents.get(&qid).ok_or("RESTING_EDGE_ORIGINAL_INTENT")?;
    if intent.get("fundamental").is_some_and(|v|!v.is_null())||intent.get("package").is_some_and(|v|!v.is_null()){return Ok(resting_result(Class::NotApplicable,"reviewed_or_package_intent"))}
    let original=&intent["quote"];
    for key in ["id","exchangeId","marketId","side","action","price","createdAt","activationAt","expiresAt"]{need(original[key]==q[key],"RESTING_EDGE_ORIGINAL_TERMS")?;}
    let ack:Receipt=serde_json::from_value(log.acknowledgments.get(&qid).ok_or("RESTING_EDGE_ACK")?.clone())?;
    need(ack.endpoint=="/orders"&&ack.status==200,"RESTING_EDGE_ORIGINAL_POST_REQUIRED")?;
    let ack_start=clock(&ack.request_started_at)?;let ack_received=clock(&ack.received_at)?;
    let ack_event=crate::feedback::acknowledgment(original,&ack,&ack.received_at)?;
    need(ack_start<=ack_received&&ack_received<=now&&ack_event["actualOrderId"]==*actual&&ack_event["quantity"].as_u64()==Some(quantity),"RESTING_EDGE_ACK_BINDING")?;
    let attempt=log.original_attempt(qid).ok_or("RESTING_EDGE_ORIGINAL_ATTEMPT")?;
    need(clock(attempt["at"].as_str().ok_or("RESTING_EDGE_ATTEMPT_CLOCK")?)?<=ack_start,"RESTING_EDGE_ATTEMPT_CLOCK")?;
    let created=q["createdAt"].as_u64().ok_or("RESTING_EDGE_QUOTE_CLOCK")?;
    let activation=q["activationAt"].as_u64().ok_or("RESTING_EDGE_QUOTE_CLOCK")?;
    let expires=q["expiresAt"].as_u64().ok_or("RESTING_EDGE_QUOTE_CLOCK")?;
    need(created<=activation&&activation<=ack_received&&now<expires,"RESTING_EDGE_QUOTE_CLOCK")?;
    let policy=bank.engine().policy();
    if policy["quoteMode"]!="pairedSpread"{return Ok(resting_result(Class::NotApplicable,"paired_spread_only"))}
    let age=policy["bookAgeMs"].as_u64().filter(|n|*n>0&&*n<=2_147_483_647).ok_or("RESTING_EDGE_POLICY_AGE")?;
    let books=bank.engine().live_package_books();
    let book=books.iter().find(|b|b["exchangeId"]==ex).ok_or("RESTING_EDGE_CURRENT_BOOK_UNAVAILABLE")?;
    need(book["marketId"]==market&&book["tournamentId"]==scope.tournament_id,"RESTING_EDGE_BOOK_SCOPE")?;
    let started=clock(book["requestStartedAt"].as_str().ok_or("RESTING_EDGE_BOOK_CLOCK")?)?;
    let received=clock(book["receivedAt"].as_str().ok_or("RESTING_EDGE_BOOK_CLOCK")?)?;
    let source=clock(book["sourceAt"].as_str().ok_or("RESTING_EDGE_BOOK_CLOCK")?)?;
    need(started<=received&&source<=received&&received<=now&&source<=now&&now-started<age&&now-source<age&&received>ack_received&&source>ack_received,"RESTING_EDGE_POST_ACK_FRESH_BOOK")?;
    let sequence=book["sequence"].as_str().filter(|s|s.parse::<u128>().is_ok()).ok_or("RESTING_EDGE_BOOK_SEQUENCE")?;
    let bid=levels(book,"bids")?;let ask=levels(book,"asks")?;
    need(bid.0<ask.0,"RESTING_EDGE_CROSSED_BOOK")?;
    let yes=yes_microprice(bid,ask)?;let value=if side=="YES"{yes}else{SCALE.checked_sub(yes).ok_or("SIDE_EDGE_ARITHMETIC")?};
    let limit=decimal(&q["price"])?;need((TICK..=SCALE-TICK).contains(&limit)&&limit%TICK==0,"RESTING_EDGE_PRICE")?;
    let minimum=decimal(&policy["minimumEdge"])?;need(minimum>0,"RESTING_EDGE_MINIMUM")?;
    let charge=fee(limit,1,u32::try_from(policy["feeBps"].as_u64().ok_or("RESTING_EDGE_FEES")?)?,decimal(&policy["feePerShare"])?)?;
    let edge=value.checked_sub(limit).and_then(|v|v.checked_sub(charge)).ok_or("SIDE_EDGE_ARITHMETIC")?;
    if edge>=minimum{return Ok(resting_result(Class::MeetsMinimum,"side_microprice_edge_meets_original_minimum"))}
    let until=started.checked_add(age).and_then(|t|source.checked_add(age).map(|s|t.min(s).min(expires))).ok_or("SIDE_EDGE_ARITHMETIC")?;
    let descriptor=log.descriptor_identity()?;
    let context=serde_json::json!({"head":bank.durable_head(),"quote":q,"book":book,"policy":policy,"scope":scope,"managed":managed,"dispatch":descriptor,"intent":intent,"attempt":attempt,"ack":ack,"proof":log.proof(qid)});
    let evidence=RestingEvidence{qid,actual_order_id:actual.clone(),filled_quantity:filled,remaining_quantity:remaining,native_head:bank.durable_head(),quote_sha256:hash(q)?,policy_sha256:hash(policy)?,dispatch_descriptor_identity:descriptor,original_post_ack_sha256:crate::feedback::digest(&ack)?,native_book_sha256:hash(book)?,source_sequence:sequence.into(),request_started_at_ms:started,received_at_ms:received,source_at_ms:source,original_post_ack_received_at_ms:ack_received,assessed_at_ms:now,valid_until_ms:until,yes_microprice_micros:yes,side_value_micros:value,limit_micros:limit,entry_fee_per_share_micros:charge,side_edge_micros:edge,minimum_edge_micros:minimum};
    Ok(RestingAssessment{class:Class::BelowMinimum,reason:"side_microprice_edge_below_original_minimum".into(),nomination:Some(RestingDecision{evidence,context_sha256:hash(&context)?})})
}
/// No reads, writes, renewed source clocks, synthetic receipts or filled=0 copy.
/// Unavailability NEVER nominates cancellation. Only current original POST ACK
/// ownership plus a genuinely newer admitted book can produce BelowMinimum.
pub fn assess_resting(bank:&moloch_replay::session::Session,scope:&Scope,log:&crate::dispatch::DispatchLog,managed:&BTreeSet<String>,qid:u64,now:u64)->RestingAssessment{
    match resting_checked(bank,scope,log,managed,qid,now){Ok(a)=>a,Err(e)=>resting_result(Class::Unavailable,&e.to_string())}
}

/// Selection bypass only: actual opposite holdings must cover the WHOLE frozen
/// remaining BUY after every other potential netting BUY/covered SELL pledge.
/// The caller supplies current native records and original unresolved attempts.
/// This is not a gain claim, a quantity clip, or permission to ignore unknowns.
pub fn fully_covered_opposite_buy(q:&Value,owned:u64,orders:&[Value],unclosed_attempts:&BTreeSet<u64>)->bool {
    let covered=||->Option<bool>{
        if q["action"]!="buy"{return Some(false)}
        let ex=q["exchangeId"].as_str()?;let id=q["id"].as_u64()?;
        let side=q["side"].as_str()?;let opposite=match side{"YES"=>"NO","NO"=>"YES",_=>return None};
        let remaining=q["quantity"].as_u64()?.checked_sub(q["filled"].as_u64()?)?;
        if remaining==0||owned<remaining{return Some(false)}
        let mut encumbered=0u64;
        for other in orders {
            if other["exchangeId"]!=ex||other["id"]==id{continue}
            let other_id=other["id"].as_u64()?;
            if other["retired"].as_bool()?&&!unclosed_attempts.contains(&other_id){continue}
            let action=other["action"].as_str()?;let other_side=other["side"].as_str()?;
            if !["buy","sell"].contains(&action)||!["YES","NO"].contains(&other_side){return None}
            if (action=="sell"&&other_side==opposite)||(action=="buy"&&other_side==side) {
                encumbered=encumbered.checked_add(other["quantity"].as_u64()?.checked_sub(other["filled"].as_u64()?)?)?;
            }
        }
        Some(owned.checked_sub(encumbered).is_some_and(|free|free>=remaining))
    };
    covered()==Some(true)
}

#[cfg(all(test,unix))]
mod resting_tests {
    use super::*;
    use crate::dispatch::DispatchLog;
    use moloch_replay::session::Session;
    use serde_json::json;
    use std::{path::PathBuf,sync::atomic::{AtomicU64,Ordering}};
    const BASE:u64=1_792_022_400_000;
    const T:&str="11111111-1111-4111-8111-111111111111";
    fn iso(t:u64)->String{chrono::DateTime::from_timestamp_millis((BASE+t)as i64).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
    struct F{root:PathBuf,bank:Session,log:DispatchLog,scope:Scope,qid:u64}
    impl Drop for F{fn drop(&mut self){let _=std::fs::remove_dir_all(&self.root);}}
    impl F{
        fn new(side:&str)->Self{Self::make(side,true)}
        fn make(side:&str,ack:bool)->Self{
            static NEXT:AtomicU64=AtomicU64::new(0);
            let root=std::env::temp_dir().join(format!("resting-edge-{}-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),NEXT.fetch_add(1,Ordering::Relaxed)));
            std::fs::create_dir_all(&root).unwrap();
            let policy=json!({"initialCash":"1000.000000","maximumCommitted":"1000.000000","maximumMarketCost":"500.000000","quantity":10,"maximumMarkets":4,"entryStopAt":iso(3600000),"lifetimeMs":600000,"minimumQuoteAgeMs":60000,"quoteMode":"pairedSpread","minimumEdge":"0.001000","feePerShare":"0.000000","feeBps":0});
            let seed=json!({"runId":"resting-fixture","tournamentId":T,"availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(0)});
            let bank=Session::open_live(&root.join("bank"),&policy,&seed).unwrap();
            let log=DispatchLog::open(&root.join("dispatch.ndjson")).unwrap();
            let scope=Scope{tournament_id:T.into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"201".into())].into()};
            let mut f=Self{root,bank,log,scope,qid:0};f.book(0,1000,1000);
            f.qid=f.bank.engine().order_records().iter().find(|q|q["action"]=="buy"&&q["side"]==side&&q["retired"]==false).unwrap()["id"].as_u64().unwrap();if ack{f.ack();}f
        }
        fn put(&mut self,e:Value){let n=self.bank.last_source_sequence("fixture")+1;self.bank.accept("fixture",n,&e).unwrap();}
        fn q(&self)->Value{self.bank.engine().order_records().into_iter().find(|q|q["id"].as_u64()==Some(self.qid)).unwrap()}
        fn book(&mut self,t:u64,bid_n:u64,ask_n:u64){self.put(json!({"type":"book","receivedAt":iso(t),"book":{"marketId":"201","exchangeId":"101","tournamentId":T,"sequence":(t+1).to_string(),"sourceAt":iso(t),"requestStartedAt":iso(t),"receivedAt":iso(t),"bids":[{"price":"0.400000","quantity":bid_n}],"asks":[{"price":"0.450000","quantity":ask_n}]}}));}
        fn ack(&mut self){let q=self.q();self.log.append(json!({"kind":"intent","qid":self.qid,"quote":q,"fundamental":null,"at":iso(3000)})).unwrap();self.log.append(json!({"kind":"attempt","qid":self.qid,"at":iso(3000)})).unwrap();let r=Receipt{endpoint:"/orders".into(),request_started_at:iso(3000),received_at:iso(3000),status:200,data:json!({"orderId":991,"exchangeId":"101","side":if q["side"]=="YES"{"yes"}else{"no"},"action":"buy","quantity":q["quantity"],"price":q["price"]})};let event=crate::feedback::acknowledgment(&q,&r,&iso(3000)).unwrap();self.put(event);self.log.append(json!({"kind":"ack","qid":self.qid,"receipt":r})).unwrap();}
        fn assess(&self,t:u64)->RestingAssessment{assess_resting(&self.bank,&self.scope,&self.log,&BTreeSet::new(),self.qid,BASE+t)}
        fn weak(&mut self){if self.q()["side"]=="YES"{self.book(4000,1000,100000);}else{self.book(4000,100000,1000);}}
        fn fill(&mut self,n:u64,t:u64){let q=self.q();self.put(json!({"type":"actual_fill","source":"official_sig_api_v1","receiptHash":"c".repeat(64),"orderId":self.qid,"actualOrderId":"991","marketId":"201","exchangeId":"101","side":q["side"],"action":"buy","quantity":n,"price":q["price"],"fillId":(800+t).to_string(),"fee":"0.000000","executedAt":iso(t),"receivedAt":iso(t)}));}
    }
    #[test]fn genuine_post_ack_below_minimum_is_read_only_and_bound_to_originals(){
        let mut f=F::new("YES");assert!(f.assess(3000).nomination().is_none());f.weak();
        let before=f.bank.engine().report().unwrap();let head=f.bank.durable_head();let descriptor=f.log.descriptor_identity().unwrap();let a=f.assess(4000);assert_eq!(a.class,Class::BelowMinimum);let d=a.nomination().unwrap();assert_eq!(d.qid(),f.qid);assert_eq!(d.actual_order_id(),"991");assert_eq!(d.remaining_quantity(),10);assert_eq!(d.evidence().filled_quantity,0);assert_eq!(d.evidence().native_head,head);assert_eq!(d.evidence().original_post_ack_received_at_ms,BASE+3000);assert_eq!(d.evidence().source_at_ms,BASE+4000);assert!(d.evidence().side_edge_micros<1000);d.validate_current(&f.bank,&f.scope,&f.log,&BTreeSet::new(),BASE+4001).unwrap();assert_eq!(before,f.bank.engine().report().unwrap());assert_eq!(descriptor,f.log.descriptor_identity().unwrap());
    }
    #[test]fn actual_partial_fill_preserves_owned_shares_filled_and_remaining_without_filled_zero_copy(){let mut f=F::new("YES");f.fill(3,3500);f.weak();let q=f.q();assert_eq!(q["filled"],3);let wallet=f.bank.engine().wallet.quantity("101",moloch_replay::Side::Yes);let a=f.assess(4000);let d=a.nomination().unwrap();assert_eq!(d.remaining_quantity(),7);assert_eq!(d.evidence().filled_quantity,3);assert_eq!(d.evidence().quote_sha256,hash(&q).unwrap());assert_eq!(wallet,3);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::Yes),wallet);}
    #[test]fn no_complement_uses_exact_yes_floor_complement_not_independent_rounding(){let mut f=F::new("NO");f.weak();let a=f.assess(4000);let e=a.nomination().unwrap().evidence();assert_eq!(e.side_value_micros,SCALE-e.yes_microprice_micros);assert_eq!(e.limit_micros,550000);assert!(e.side_edge_micros<1000);}
    #[test]fn unchanged_or_improved_proxy_does_not_nominate(){let mut f=F::new("YES");f.book(4000,1000,1000);assert_eq!(f.assess(4000).class,Class::MeetsMinimum);assert!(f.assess(4000).nomination().is_none());}
    #[test]fn original_pre_ack_book_and_future_and_exact_stale_boundary_do_not_nominate(){let mut f=F::new("YES");assert!(f.assess(3000).nomination().is_none());f.weak();assert_eq!(f.assess(3999).class,Class::Unavailable);let a=f.assess(4000);let until=a.nomination().unwrap().valid_until_ms();assert!(a.nomination().unwrap().validate_current(&f.bank,&f.scope,&f.log,&BTreeSet::new(),until).is_err());assert!(f.assess(until-BASE).nomination().is_none());}
    #[test]fn actual_gap_with_no_valid_native_book_cannot_nominate(){let mut f=F::new("YES");f.weak();f.put(json!({"type":"gap","exchangeIds":["101"],"receivedAt":iso(4001)}));assert!(f.assess(4001).nomination().is_none());}
    #[test]fn any_managed_or_original_fundamental_attribution_bypasses(){let mut f=F::new("YES");f.weak();assert_eq!(assess_resting(&f.bank,&f.scope,&f.log,&["101".into()].into(),f.qid,BASE+4000).class,Class::NotApplicable);f.log.intents.get_mut(&f.qid).unwrap()["fundamental"]=json!({"reviewSha256":"a".repeat(64)});assert_eq!(f.assess(4000).class,Class::NotApplicable);}
    #[test]fn exact_original_ack_binding_mismatch_and_recovered_get_cannot_nominate(){for change in 0..3{let mut f=F::new("YES");f.weak();match change{0=>f.log.acknowledgments.get_mut(&f.qid).unwrap()["data"]["orderId"]=json!(992),1=>{f.log.recovered.insert(f.qid,json!({}));},_=>f.log.intents.get_mut(&f.qid).unwrap()["quote"]["price"]=json!("0.395000")};assert!(f.assess(4000).nomination().is_none());}}
    #[test]fn original_cancel_attempt_blocks_even_without_ack_and_survives_restart(){let mut f=F::new("YES");f.weak();f.log.append(json!({"kind":"cancel_attempt","qid":f.qid,"at":iso(4000)})).unwrap();assert!(f.assess(4000).nomination().is_none());let reopened=DispatchLog::open(&f.root.join("dispatch.ndjson")).unwrap();assert!(assess_resting(&f.bank,&f.scope,&reopened,&BTreeSet::new(),f.qid,BASE+4000).nomination().is_none());}
    #[test]fn head_or_dispatch_progress_or_scope_changes_invalidates_opaque_nomination(){for change in 0..3{let mut f=F::new("YES");f.weak();let d=f.assess(4000).into_nomination().unwrap();let mut scope=f.scope.clone();match change{0=>f.put(json!({"type":"tick","receivedAt":iso(4001)})),1=>f.log.append(json!({"kind":"source","qid":f.qid,"at":iso(4001)})).unwrap(),_=>{scope.exchanges.insert("101".into(),"999".into());}}assert!(d.validate_current(&f.bank,&scope,&f.log,&BTreeSet::new(),BASE+4001).is_err());}}
    #[test]fn fully_filled_and_original_terminal_closed_do_not_nominate(){let mut f=F::new("YES");f.fill(10,3500);f.weak();assert!(f.assess(4000).nomination().is_none());let mut f=F::new("YES");f.weak();f.log.append(json!({"kind":"closed","qid":f.qid})).unwrap();assert!(f.assess(4000).nomination().is_none());}
    #[test]fn any_partial_opposite_coverage_bypasses_instead_of_canceling_potential_hedge(){let mut f=F::new("YES");let no=f.bank.engine().order_records().into_iter().find(|q|q["side"]=="NO"&&q["action"]=="buy").unwrap();let id=no["id"].as_u64().unwrap();f.put(json!({"type":"actual_order_ack","source":"official_sig_api_v1","receiptHash":"d".repeat(64),"orderId":id,"actualOrderId":"992","marketId":"201","exchangeId":"101","side":"NO","action":"buy","quantity":no["quantity"],"price":no["price"],"receivedAt":iso(3500)}));f.put(json!({"type":"actual_fill","source":"official_sig_api_v1","receiptHash":"e".repeat(64),"orderId":id,"actualOrderId":"992","marketId":"201","exchangeId":"101","side":"NO","action":"buy","quantity":1,"price":no["price"],"fillId":"882","fee":"0.000000","executedAt":iso(3501),"receivedAt":iso(3501)}));f.weak();assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),1);assert_eq!(f.assess(4000).class,Class::NotApplicable);}
    #[test]fn native_package_order_bypasses_without_masquerading_as_an_ordinary_original_post(){
        let mut f=F::make("YES",false);f.scope.exchanges.insert("102".into(),"202".into());
        f.put(json!({"type":"book","receivedAt":iso(0),"book":{"marketId":"202","exchangeId":"102","tournamentId":T,"sequence":"1","sourceAt":iso(0),"requestStartedAt":iso(0),"receivedAt":iso(0),"bids":[{"price":"0.400000","quantity":1000}],"asks":[{"price":"0.450000","quantity":1000}]}}));
        for q in f.bank.engine().order_records().into_iter().filter(|q|q["retired"]==false){f.put(json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","orderId":q["id"],"confirmedNetworkAttempts":0,"receiptHash":"a".repeat(64),"deferPlanningForPackage":true,"receivedAt":iso(0)}));}
        let books=f.bank.engine().live_package_books();let book_hash=|ex:&str|hash(books.iter().find(|b|b["exchangeId"]==ex).unwrap()).unwrap();
        f.put(json!({"type":"live_package_reserve","source":"moloch_live_dispatcher","receivedAt":iso(1),"processedAt":iso(1),"packageId":"resting-protected","proofHash":"a".repeat(64),"bankScope":{"runId":"resting-fixture","bankManifestHash":f.bank.manifest_hash(),"tournamentId":T},"expiresAt":iso(9000),"economics":{"maximumCost":"4.500000","minimumPayout":"5.000000","minimumSurplus":"0.500000","feeTotal":"0.000000"},"legs":[{"marketId":"201","exchangeId":"101","side":"YES","quantity":5,"limitPrice":"0.450000","bookHash":book_hash("101")},{"marketId":"202","exchangeId":"102","side":"YES","quantity":5,"limitPrice":"0.450000","bookHash":book_hash("102")}]}));
        f.qid=f.bank.engine().package_records()[0]["legs"][0]["qid"].as_u64().unwrap();assert!(f.bank.engine().is_package_order(f.qid));assert_eq!(f.assess(1).class,Class::NotApplicable);assert!(f.assess(1).nomination().is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const AT:u64=1_792_022_400_000;
    const T:&str="11111111-1111-4111-8111-111111111111";
    fn iso(t:u64)->String{chrono::DateTime::from_timestamp_millis(t as i64).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
    fn f(side:&str)->(Value,Scope,Value,Receipt){
        let p=json!({"quoteMode":"pairedSpread","bookAgeMs":10000,"minimumEdge":"0.001000","feeBps":0,"feePerShare":"0.000000"});
        let s=Scope{tournament_id:T.into(),tournament_slug:"cup".into(),exchanges:[("101".into(),"201".into())].into()};
        let q=json!({"id":1,"marketId":"201","exchangeId":"101","side":side,"action":"buy","quantity":100,"filled":0,"price":if side=="YES"{"0.400000"}else{"0.590000"},"createdAt":AT-4000,"activationAt":AT-1000,"expiresAt":AT+600000,"retired":false});
        let r=Receipt{endpoint:format!("/markets/201/orderbook?tournamentId={T}&depth=20"),request_started_at:iso(AT-100),received_at:iso(AT),status:200,data:json!({"contexts":[{"type":"tournament","tournament":{"id":T},"orderbook":{"exchanges":[{"exchangeId":"101","asOf":{"sequence":2,"at":iso(AT-50)},"bids":[{"price":0.400,"quantity":10}],"asks":[{"price":0.410,"quantity":990}]}]}}]})};
        (p,s,q,r)
    }
    fn row(r:&mut Receipt)->&mut Value{&mut r.data["contexts"][0]["orderbook"]["exchanges"][0]}
    fn a(p:&Value,s:&Scope,q:&Value,r:&Receipt)->Assessment{assess_original(p,s,q,r,AT)}
    #[test]fn weak_yes_is_rejected_while_its_complement_is_not_claimed_a_paired_fill(){
        let(p,s,q,r)=f("YES");let before=(p.clone(),q.clone(),serde_json::to_value(&r).unwrap());
        let yes=a(&p,&s,&q,&r);assert_eq!(yes.class,Class::BelowMinimum);assert_eq!(yes.evidence.unwrap().side_edge_micros,100);
        let(_,_,no,_)=f("NO");let no=a(&p,&s,&no,&r);assert_eq!(no.class,Class::MeetsMinimum);assert_eq!(no.evidence.unwrap().side_edge_micros,9900);
        assert_eq!(before,(p,q,serde_json::to_value(&r).unwrap()));
    }
    #[test]fn yes_integer_floor_then_exact_no_complement_and_threshold_equality(){
        let(mut p,s,mut q,mut r)=f("NO");row(&mut r)["bids"][0]["quantity"]=json!(1);row(&mut r)["asks"][0]["quantity"]=json!(2);row(&mut r)["asks"][0]["price"]=json!(0.405);q["price"]=json!("0.595000");
        p["minimumEdge"]=json!("0.003334");let d=a(&p,&s,&q,&r);assert_eq!(d.class,Class::MeetsMinimum);let e=d.evidence.unwrap();assert_eq!(e.yes_microprice_micros,401666);assert_eq!(e.side_value_micros,598334);assert_eq!(e.side_edge_micros,3334);
        p["minimumEdge"]=json!("0.003335");assert_eq!(a(&p,&s,&q,&r).class,Class::BelowMinimum);
    }
    #[test]fn exact_native_one_share_fee_ceiling_can_change_classification(){
        let(mut p,s,q,mut r)=f("YES");row(&mut r)["bids"][0]["quantity"]=json!(1);row(&mut r)["asks"][0]["quantity"]=json!(2);row(&mut r)["asks"][0]["price"]=json!(0.405);p["minimumEdge"]=json!("0.001626");
        assert_eq!(a(&p,&s,&q,&r).class,Class::MeetsMinimum);p["feeBps"]=json!(1);p["feePerShare"]=json!("0.000001");let d=a(&p,&s,&q,&r);assert_eq!(d.class,Class::BelowMinimum);let e=d.evidence.unwrap();assert_eq!(e.entry_fee_per_share_micros,fee(400000,1,1,1).unwrap());assert_eq!(e.side_edge_micros,1625);
    }
    #[test]fn absent_disabled_and_shadow_cannot_enforce_and_unknown_modes_reject(){
        #[derive(Deserialize)]struct C{#[serde(default)]ordinary_side_edge:Mode}
        assert_eq!(serde_json::from_value::<C>(json!({})).unwrap().ordinary_side_edge,Mode::Disabled);
        let(p,s,q,r)=f("YES");let d=a(&p,&s,&q,&r);for m in [Mode::Disabled,Mode::Shadow]{assert!(!m.defers(&d));}assert!(Mode::Enforce.defers(&d));
        assert_eq!(serde_json::from_value::<Mode>(json!("shadow")).unwrap(),Mode::Shadow);assert!(serde_json::from_value::<Mode>(json!("enabled")).is_err());
    }
    #[test]fn malformed_missing_or_foreign_book_is_unavailable_not_positive_edge(){
        let(p,s,q,r)=f("YES");for n in 0..9{let mut bad=r.clone();match n{0=>bad.status=500,1=>bad.endpoint.push_str("&extra=1"),2=>bad.data["contexts"][0]["tournament"]["id"]=json!("other"),3=>row(&mut bad)["exchangeId"]=json!("102"),4=>row(&mut bad)["asks"]=json!([]),5=>row(&mut bad)["bids"][0]["quantity"]=json!(0),6=>row(&mut bad)["asks"][0]["price"]=json!(0.411),7=>row(&mut bad)["asOf"]["sequence"]=Value::Null,_=>bad.data=Value::Null};let d=a(&p,&s,&q,&bad);assert_eq!(d.class,Class::Unavailable,"case{n}");assert!(Mode::Enforce.defers(&d));assert!(!Mode::Shadow.defers(&d));assert!(d.evidence.is_none());}
    }
    #[test]fn original_all_three_clocks_age_without_renewal(){
        let(p,s,q,r)=f("YES");for n in 0..6{let mut bad=r.clone();match n{0=>bad.request_started_at=iso(AT-10001),1=>bad.received_at=iso(AT+1),2=>row(&mut bad)["asOf"]["at"]=json!(iso(AT-10001)),3=>row(&mut bad)["asOf"]["at"]=json!(iso(AT+1)),4=>bad.request_started_at=iso(AT+1),_=>bad.received_at=iso(AT-101)};assert_eq!(a(&p,&s,&q,&bad).class,Class::Unavailable,"clock{n}");}
        assert_eq!(assess_original(&p,&s,&q,&r,AT+9900).class,Class::BelowMinimum);assert_eq!(assess_original(&p,&s,&q,&r,AT+9901).class,Class::Unavailable);
    }
    #[test]fn every_scoped_exchange_must_be_present_without_duplicates(){
        let(p,mut s,q,mut r)=f("YES");s.exchanges.insert("102".into(),"201".into());assert_eq!(a(&p,&s,&q,&r).class,Class::Unavailable);let mut extra=row(&mut r).clone();extra["exchangeId"]=json!("102");r.data["contexts"][0]["orderbook"]["exchanges"].as_array_mut().unwrap().push(extra);assert_eq!(a(&p,&s,&q,&r).class,Class::BelowMinimum);r.data["contexts"][0]["orderbook"]["exchanges"][1]["exchangeId"]=json!("101");assert_eq!(a(&p,&s,&q,&r).class,Class::Unavailable);
    }
    #[test]fn invalid_ordered_depth_crossed_book_and_fractional_sizes_never_get_filled_in(){
        let(p,s,q,r)=f("YES");for n in 0..4{let mut bad=r.clone();match n{0=>row(&mut bad)["bids"].as_array_mut().unwrap().push(json!({"price":0.405,"quantity":5})),1=>row(&mut bad)["asks"][0]["price"]=json!(0.400),2=>row(&mut bad)["asks"][0]["quantity"]=json!(1.5),_=>row(&mut bad)["bids"][0]["quantity"]=json!(9_007_199_254_740_992u64)};assert_eq!(a(&p,&s,&q,&bad).class,Class::Unavailable);}
    }
    #[test]fn unrelated_stale_one_sided_outcome_does_not_veto_a_valid_target(){
        let(p,mut s,q,mut r)=f("NO");s.exchanges.insert("102".into(),"201".into());let extra=json!({"exchangeId":"102","asOf":{"sequence":1,"at":iso(AT-86400000)},"bids":[],"asks":[]});r.data["contexts"][0]["orderbook"]["exchanges"].as_array_mut().unwrap().push(extra);
        assert_eq!(a(&p,&s,&q,&r).class,Class::MeetsMinimum);r.data["contexts"][0]["orderbook"]["exchanges"][1]["exchangeId"]=json!("101");assert_eq!(a(&p,&s,&q,&r).class,Class::Unavailable);
    }
    #[test]fn overflow_or_missing_native_policy_stays_unavailable(){
        let(p,s,q,r)=f("YES");for n in 0..5{let mut bad=p.clone();match n{0=>bad["feePerShare"]=json!("170141183460469231731687303715884.105728"),1=>{bad["feePerShare"]=json!("170141183460469231731687303715884.105727");bad["feeBps"]=json!(1);},2=>bad["bookAgeMs"]=json!(u64::MAX),3=>bad["minimumEdge"]=Value::Null,_=>bad["feeBps"]=json!(u64::MAX)};assert_eq!(a(&bad,&s,&q,&r).class,Class::Unavailable,"policy{n}");}
    }
    #[test]fn exact_quote_terms_and_original_receipt_policy_hashes_are_recorded(){
        let(p,s,q,r)=f("YES");let first=a(&p,&s,&q,&r).evidence.unwrap();assert_eq!(first.quote_sha256,hash(&q).unwrap());assert_eq!(first.policy_sha256,hash(&p).unwrap());assert_eq!(first.original_book_receipt_sha256,crate::feedback::digest(&r).unwrap());
        for (field,value) in [("price",json!("0.395000")),("quantity",json!(1)),("expiresAt",json!(AT+10000))]{let mut changed=q.clone();changed[field]=value;assert_ne!(a(&p,&s,&changed,&r).evidence.unwrap().quote_sha256,first.quote_sha256);}
        let mut changed=q.clone();changed["marketId"]=json!("202");assert_eq!(a(&p,&s,&changed,&r).class,Class::Unavailable);
    }
    #[test]fn exits_reviewed_entries_and_nonpaired_policy_are_not_subject_to_this_rule(){
        let(mut p,s,q,r)=f("YES");for field in ["reviewedFundamentalEntry","reviewedInventoryExit"]{let mut other=q.clone();other[field]=json!({"review":"original"});assert_eq!(a(&p,&s,&other,&r).class,Class::NotApplicable);}let mut exit=q.clone();exit["action"]=json!("sell");assert_eq!(a(&p,&s,&exit,&r).class,Class::NotApplicable);p["quoteMode"]=json!("microprice");let d=a(&p,&s,&q,&r);assert_eq!(d.class,Class::NotApplicable);assert!(!Mode::Enforce.defers(&d));
    }
    #[test]fn expired_canceling_filled_or_future_activation_quote_is_unavailable(){
        let(p,s,q,r)=f("YES");for(field,value)in [("expiresAt",json!(AT)),("activationAt",json!(AT+1)),("cancelEffectiveAt",json!(AT+100)),("retired",json!(true)),("filled",json!(1)),("quantity",json!(0))]{let mut bad=q.clone();bad[field]=value;assert_eq!(a(&p,&s,&bad,&r).class,Class::Unavailable);}
    }
    #[test]fn hedge_bypass_requires_full_unencumbered_opposite_inventory(){
        let(_,_,q,_)=f("YES");assert!(fully_covered_opposite_buy(&q,100,&[q.clone()],&BTreeSet::new()));assert!(!fully_covered_opposite_buy(&q,99,&[q.clone()],&BTreeSet::new()));
        let mut pledge=q.clone();pledge["id"]=json!(2);pledge["action"]=json!("sell");pledge["side"]=json!("NO");pledge["quantity"]=json!(1);
        assert!(!fully_covered_opposite_buy(&q,100,&[q.clone(),pledge.clone()],&BTreeSet::new()));assert!(fully_covered_opposite_buy(&q,101,&[q.clone(),pledge.clone()],&BTreeSet::new()));
        pledge["retired"]=json!(true);assert!(fully_covered_opposite_buy(&q,100,&[q.clone(),pledge.clone()],&BTreeSet::new()));assert!(!fully_covered_opposite_buy(&q,100,&[q.clone(),pledge],&[2].into()));
    }
    #[test]fn other_netting_buy_also_consumes_cover_and_no_side_is_reversed_twice(){
        let(_,_,q,_)=f("NO");let mut other=q.clone();other["id"]=json!(2);other["quantity"]=json!(1);assert!(!fully_covered_opposite_buy(&q,100,&[q.clone(),other.clone()],&BTreeSet::new()));assert!(fully_covered_opposite_buy(&q,101,&[q.clone(),other],&BTreeSet::new()));
    }
}
