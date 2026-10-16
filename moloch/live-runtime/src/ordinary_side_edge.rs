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
    let numerator=ask.0.checked_mul(bid.1 as i128).and_then(|a|bid.0.checked_mul(ask.1 as i128).and_then(|b|a.checked_add(b))).ok_or("SIDE_EDGE_ARITHMETIC")?;
    let denominator=(bid.1 as i128).checked_add(ask.1 as i128).ok_or("SIDE_EDGE_ARITHMETIC")?;
    let yes_micro=numerator.checked_div(denominator).ok_or("SIDE_EDGE_ARITHMETIC")?;
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
