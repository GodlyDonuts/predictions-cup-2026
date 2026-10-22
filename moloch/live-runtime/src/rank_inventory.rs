//! Keep a monetary acquisition's remaining actual lots out of the unrelated
//! ordinary maximum-age liquidation strategy. This grants no order authority.
use serde_json::Value;
use std::collections::BTreeSet;

pub type Held = BTreeSet<(String,String)>;
fn key(row:&Value)->Option<(String,String,u64)>{
    Some((row["exchangeId"].as_str()?.into(),row["side"].as_str()?.into(),
        moloch_live_runtime::daily_windows::clock(row["executedAt"].as_str()?).ok()?))
}
pub fn held(positions:&Value,fills:&[Value],is_rank_buy:impl Fn(u64)->bool)->Held{
    let clocks:BTreeSet<_>=fills.iter().filter(|f|f["counterfactual"]==false&&f["action"]=="buy"
        &&f["quantity"].as_u64().is_some_and(|n|n>0)
        &&f["orderId"].as_u64().is_some_and(&is_rank_buy)).filter_map(key).collect();
    let mut result=Held::new();
    if let Some(markets)=positions.as_object(){for(ex,sides)in markets{for side in ["YES","NO"]{
        if sides[side].as_array().is_some_and(|lots|lots.iter().any(|lot|{
            lot["quantity"].as_u64().is_some_and(|n|n>0)&&lot["executedAt"].as_str()
                .and_then(|at|moloch_live_runtime::daily_windows::clock(at).ok())
                .is_some_and(|at|clocks.contains(&(ex.clone(),side.into(),at)))
        })){result.insert((ex.clone(),side.into()));}
    }}}
    result
}
pub fn forced_age_sale(q:&Value,held:&Held)->bool{
    q["action"]=="sell"&&q["inventoryExitReason"]=="maximumAge"
        &&!q.get("reviewedRankCampaign").is_some_and(Value::is_object)
        &&q["exchangeId"].as_str().zip(q["side"].as_str())
            .is_some_and(|(ex,side)|held.contains(&(ex.into(),side.into())))
}
// A SELL limit must cover every original FIFO unit it can consume, including
// a one-unit partial fill and the conservative per-unit fee. Unknown/uncovered
// input cannot establish a cash-safe exit. This never grants order authority.
fn cash_safe_sale(q:&Value,positions:&Value,policy:&Value)->Option<bool>{
    let ex=q["exchangeId"].as_str()?;let side=q["side"].as_str()?;
    let price=baal_transport_service::decimal_units(q["price"].as_str()?).ok()?;
    let bps=u32::try_from(policy["feeBps"].as_u64()?).ok()?;
    let share=baal_transport_service::decimal_units(policy["feePerShare"].as_str()?).ok()?;
    let net=price.checked_sub(moloch_replay::fee(price,1,bps,share).ok()?)?;
    let mut remaining=q["quantity"].as_u64()?;if remaining==0{return Some(false)}
    for lot in positions.get(ex)?.get(side)?.as_array()?{
        let n=lot["quantity"].as_u64()?;if n==0{continue}
        let cost=baal_transport_service::decimal_units(lot["price"].as_str()?).ok()?;
        if cost>net{return Some(false)}
        remaining=remaining.saturating_sub(n);if remaining==0{return Some(true)}
    }Some(false)
}
pub fn loss_forced_age_sale(q:&Value,held:&Held,positions:&Value,policy:&Value)->bool{
    forced_age_sale(q,held)&&cash_safe_sale(q,positions,policy)!=Some(true)
}

#[cfg(test)]mod tests{
    use super::*;use serde_json::json;
    fn fill()->Value{json!({"orderId":7,"exchangeId":"948","side":"NO","action":"buy","quantity":10,"executedAt":"2026-10-22T13:11:07.116Z","counterfactual":false})}
    fn positions()->Value{json!({"948":{"YES":[],"NO":[{"quantity":4,"price":"0.900000"},{"quantity":8,"price":"0.890000","executedAt":"2026-10-22T13:11:07.116Z"}]}})}
    #[test]fn remaining_actual_rank_lots_protect_mixed_position_without_rewriting_basis(){let p=positions();let original=p.clone();assert_eq!(held(&p,&[fill()],|id|id==7),[("948".into(),"NO".into())].into());assert_eq!(p,original);}
    #[test]fn consumed_rank_lot_does_not_protect_later_ordinary_inventory(){let mut p=positions();p["948"]["NO"][1]["executedAt"]=json!("2026-10-22T13:12:07.116Z");assert!(held(&p,&[fill()],|_|true).is_empty());assert!(held(&json!({}),&[fill()],|_|true).is_empty());}
    #[test]fn virtual_zero_quantity_sell_and_nonrank_fills_grant_no_protection(){for field in ["counterfactual","quantity","action"]{let mut f=fill();f[field]=match field{"counterfactual"=>json!(true),"quantity"=>json!(0),_=>json!("sell")};assert!(held(&positions(),&[f],|_|true).is_empty());}assert!(held(&positions(),&[fill()],|_|false).is_empty());}
    #[test]fn exact_market_side_and_execution_clock_are_required(){for field in ["exchangeId","side","executedAt"]{let mut f=fill();f[field]=json!(match field{"exchangeId"=>"1071","side"=>"YES",_=>"2026-10-22T13:11:07.117Z"});assert!(held(&positions(),&[f],|_|true).is_empty());}}
    #[test]fn equivalent_original_clock_formats_match(){let mut f=fill();f["executedAt"]=json!("2026-10-22T13:11:07.116+00:00");assert_eq!(held(&positions(),&[f],|_|true).len(),1);}
    #[test]fn only_unreviewed_age_sale_is_suppressed(){let h=held(&positions(),&[fill()],|_|true);let q=json!({"action":"sell","inventoryExitReason":"maximumAge","exchangeId":"948","side":"NO"});assert!(forced_age_sale(&q,&h));let mut profit=q.clone();profit.as_object_mut().unwrap().remove("inventoryExitReason");assert!(!forced_age_sale(&profit,&h));let mut reviewed=q.clone();reviewed["reviewedRankCampaign"]=json!({"campaignSha256":"explicit"});assert!(!forced_age_sale(&reviewed,&h));let mut other=q.clone();other["side"]=json!("YES");assert!(!forced_age_sale(&other,&h));}
    fn age_quote(price:&str,n:u64)->Value{json!({"action":"sell","inventoryExitReason":"maximumAge","exchangeId":"948","side":"NO","price":price,"quantity":n})}
    fn fees()->Value{json!({"feeBps":0,"feePerShare":"0.000000"})}
    #[test]fn profitable_and_cash_flat_age_exits_remain_available(){let p=positions();let h=held(&p,&[fill()],|_|true);assert!(!loss_forced_age_sale(&age_quote("0.900000",8),&h,&p,&fees()));assert!(!loss_forced_age_sale(&age_quote("0.905000",8),&h,&p,&fees()));}
    #[test]fn full_order_average_profit_cannot_hide_losing_first_fifo_partial(){let p=positions();let h=held(&p,&[fill()],|_|true);assert!(loss_forced_age_sale(&age_quote("0.895000",12),&h,&p,&fees()));}
    #[test]fn actual_fees_and_uncovered_quantity_prevent_cash_safe_classification(){let p=positions();let h=held(&p,&[fill()],|_|true);let mut f=fees();f["feeBps"]=json!(100);assert!(loss_forced_age_sale(&age_quote("0.900000",8),&h,&p,&f));f=fees();f["feePerShare"]=json!("0.001000");assert!(loss_forced_age_sale(&age_quote("0.900000",8),&h,&p,&f));assert!(loss_forced_age_sale(&age_quote("0.910000",20),&h,&p,&fees()));}
    #[test]fn consumed_expensive_fifo_prefix_no_longer_blocks_cash_safe_exit(){let mut p=positions();p["948"]["NO"].as_array_mut().unwrap().remove(0);let h=held(&p,&[fill()],|_|true);assert!(!loss_forced_age_sale(&age_quote("0.890000",8),&h,&p,&fees()));assert!(loss_forced_age_sale(&age_quote("0.885000",8),&h,&p,&fees()));}
}
