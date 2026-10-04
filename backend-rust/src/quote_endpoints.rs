//! Separate fixed-horizon price labels. Intermediate gaps stay visible, while
//! a valid endpoint need not assume prices, stop execution or depth inside gaps.
use crate::publication::QuoteTick;
use serde_json::{json,Value};
use std::collections::{BTreeMap,HashMap};
pub fn run(ticks:&[QuoteTick],strict:&Value,through:i64)->Result<Value,String>{
 let mut groups=HashMap::<&str,Vec<&QuoteTick>>::new();for q in ticks{groups.entry(&q.market).or_default().push(q);}
 for rows in groups.values_mut(){rows.sort_by_key(|q|(q.at,q.sequence));}
 let mut cards=Vec::new();
 for c in strict["cards"].as_array().ok_or("ENDPOINT_CARDS")?{
  let(mut observed,mut complete,mut incomplete,mut net,mut midpoint_twice,mut spread_twice,mut slippage)=(0u64,0u64,0u64,0i128,0i128,0i128,0i128);
  let mut statuses=BTreeMap::<String,u64>::new();let mut rows=Vec::new();
  for e in c["episodes"].as_array().ok_or("ENDPOINT_EPISODES")?{
   let m=e["market"].as_str().ok_or("ENDPOINT_MARKET")?;let target=e["targetAt"].as_i64();
   let mut row=json!({"market":m,"signalSequence":e["signal"]["sequence"],"strictStatus":e["status"],"entry":e["entry"],"targetAt":e["targetAt"],"endpointStatus":"no_confirmed_entry","outcome":null,"quotedNetMarkoutMicros":null});
   let label=if let Some(target)=target{
    let q=groups.get(m).and_then(|quotes|quotes.iter().find(|q|q.request>=target&&q.at<=target+120000&&q.bid.is_some()&&q.ask.is_some()));
    if let Some(q)=q{
     let(entry_bid,entry_ask)=(e["entry"]["bid"].as_i64().ok_or("ENDPOINT_ENTRY")?as i128,e["entry"]["ask"].as_i64().ok_or("ENDPOINT_ENTRY")?as i128);
     let(bid,ask)=(q.bid.unwrap(),q.ask.unwrap());let slip=e["slippageCostMicros"].as_i64().ok_or("ENDPOINT_SLIPPAGE")?as i128;
     let value=bid-entry_ask-slip;let movement=bid+ask-entry_bid-entry_ask;let spread=entry_ask-entry_bid+ask-bid;
     if movement-spread-2*slip!=2*value{return Err("ENDPOINT_ECONOMICS".into());}
     let path_complete=e["status"]=="observed";
     if path_complete{if e["quotedNetMarkoutMicros"].as_i64()!=Some(value as i64)||e["outcome"]["sequence"].as_u64()!=Some(q.sequence as u64){return Err("ENDPOINT_COMPLETE_PATH_CHANGED".into());}complete+=1;}else{incomplete+=1;}
     row["endpointStatus"]=json!(if path_complete{"observed_complete_path"}else{"observed_incomplete_path"});
     row["outcome"]=json!({"publishedAt":q.at,"requestStartedAt":q.request,"receivedAt":q.received,"sequence":q.sequence,"bid":bid,"ask":ask});row["quotedNetMarkoutMicros"]=json!(value);
     row["midpointMoveTwiceMicros"]=json!(movement);row["spreadCostTwiceMicros"]=json!(spread);row["slippageCostMicros"]=json!(slip);
     observed+=1;net+=value;midpoint_twice+=movement;spread_twice+=spread;slippage+=slip;
     if path_complete{"observed_complete_path"}else{"observed_incomplete_path"}
    }else if through<=target+120000{row["endpointStatus"]=json!("right_censored");"right_censored"}
    else{row["endpointStatus"]=json!("endpoint_not_observed");"endpoint_not_observed"}
   }else{"no_confirmed_entry"};
   *statuses.entry(label.into()).or_default()+=1;rows.push(row);
  }
  let mean=|total:i128,scale:f64|if observed==0{Value::Null}else{json!(total as f64/observed as f64/scale)};
  cards.push(json!({"strategy":c["strategy"],"parameters":c["parameters"],"scenario":c["scenario"],"featurePolicy":c["featurePolicy"],"summary":{"episodes":rows.len(),"observed":observed,"completePathObserved":complete,"incompletePathObserved":incomplete,"statusCounts":statuses,"meanQuotedNetMarkout":mean(net,1000000.0),"meanMidpointMove":mean(midpoint_twice,2000000.0),"meanSpreadCost":mean(spread_twice,2000000.0),"meanSlippageCost":mean(slippage,1000000.0)},"episodes":rows}));
 }
 Ok(json!({"schemaVersion":1,"mode":"fixed_horizon_endpoint_price_sensitivity_v1","cards":cards,"fees":0,"competitionOrders":0,"profitabilityEstablished":false,"interpretation":{"signalSelection":"identical_to_original_strict_path_episodes_no_refit","entrySelection":"original_confirmed_entry_retained_no_entry_invented","outcome":"first_valid_independent_request_at_fixed_target_within120s","pathGaps":"visible_and_not_imputed","intraperiodStopsOrProfitTargets":"not_simulated","execution":"indicative_quote_endpoints_no_depth_queue_or_account_eligibility","samples":"overlapping_cases_correlated","censoring":"all_original_episode_denominators_retained"}}))
}
