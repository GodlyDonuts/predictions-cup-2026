use moloch_replay::{engine::Engine,session::Session};
use serde_json::{Value,json};
use std::{fs,path::PathBuf};

const DAY:i64=86_400_000;
fn ms(s:&str)->i64{chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()}
fn anchor()->i64{ms("2026-10-15T12:00:00.000Z")}
fn iso(n:i64)->String{chrono::DateTime::from_timestamp_millis(n).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
fn policy()->Value{json!({"initialCash":"100000.000000","maximumCommitted":"40000.000000","maximumMarketCost":"10000.000000","quantity":300,"entryStopAt":iso(anchor())})}
fn mandate()->Value{json!({"authorizationId":"human-daily-entry-windows-20261015","approvedAt":iso(anchor()-1000),"anchorMs":anchor(),"campaignEndMs":anchor()+20*DAY+5_000,"intervalMs":DAY,"maximumCommitted":"40000.000000","maximumMarketCost":"10000.000000","maximumQuantity":300})}
fn event(at:i64,previous:i64,end:i64)->Value{json!({"type":"live_entry_window","source":"moloch_live_dispatcher","receivedAt":iso(at),"processedAt":iso(at),"previousEntryStopAt":iso(previous),"entryStopAt":iso(end),"mandate":mandate()})}
fn no_financial_change(before:&Value,after:&Value){for key in ["cash","realized","fees","ownedCost","reservedCash","positions","fills","orders","activeOrders","unresolvedOrders","contestOrders","counts"]{assert_eq!(before[key],after[key],"{key}");}}

#[test]fn absent_mandate_preserves_legacy_report_shape_and_tick_behavior(){
 let mut live=Engine::new_live(&policy()).unwrap();let mut paper=Engine::new(&policy()).unwrap();
 assert!(live.report().unwrap().get("dailyEntryWindows").is_none());
 let tick=json!({"type":"tick","receivedAt":iso(anchor()-1)});live.advance(&tick).unwrap();paper.advance(&tick).unwrap();
 let mut l=live.report().unwrap();let mut p=paper.report().unwrap();
 for key in ["executionMode","fillModel"]{l.as_object_mut().unwrap().remove(key);p.as_object_mut().unwrap().remove(key);}
 assert_eq!(l,p);assert_eq!(live.policy(),&policy_with_defaults());
}
fn policy_with_defaults()->Value{Engine::new(&policy()).unwrap().policy().clone()}

#[test]fn first_window_changes_only_cutoff_and_adds_durable_mandate_report(){
 let mut e=Engine::new_live(&policy()).unwrap();let before=e.report().unwrap();let mut expected=e.policy().clone();
 e.advance(&event(anchor(),anchor(),anchor()+DAY)).unwrap();expected["entryStopAt"]=json!(iso(anchor()+DAY));assert_eq!(e.policy(),&expected);
 let after=e.report().unwrap();no_financial_change(&before,&after);assert_eq!(after["dailyEntryWindows"],json!({"mandate":mandate(),"entryStopAt":iso(anchor()+DAY)}));
}

#[test]fn half_open_utc_windows_and_offline_skips_use_exact_current_end(){
 for(offset,end)in [(0,DAY),(DAY-1,DAY),(DAY,2*DAY),(3*DAY+500,4*DAY)]{
  let mut e=Engine::new_live(&policy()).unwrap();e.advance(&event(anchor()+offset,anchor(),anchor()+end)).unwrap();assert_eq!(e.policy()["entryStopAt"],iso(anchor()+end));
 }
 let mut e=Engine::new_live(&policy()).unwrap();e.advance(&event(anchor(),anchor(),anchor()+DAY)).unwrap();e.advance(&event(anchor()+DAY,anchor()+DAY,anchor()+2*DAY)).unwrap();assert_eq!(e.report().unwrap()["dailyEntryWindows"]["mandate"],mandate());
}

#[test]fn campaign_final_partial_window_clamps_and_exact_end_denies(){
 let end=mandate()["campaignEndMs"].as_i64().unwrap();let mut e=Engine::new_live(&policy()).unwrap();e.advance(&event(end-1,anchor(),end)).unwrap();let before=e.report().unwrap();
 assert!(e.advance(&event(end,end,end+DAY)).is_err());assert_eq!(e.report().unwrap(),before);
}

#[test]fn early_wrong_end_previous_source_paper_and_missing_processed_reject_atomically(){
 for case in ["early","end","previous","source","processed","fractional_end"]{
  let mut e=Engine::new_live(&policy()).unwrap();let mut v=event(anchor(),anchor(),anchor()+DAY);
  match case{"early"=>{v["processedAt"]=json!(iso(anchor()-1));v["receivedAt"]=v["processedAt"].clone();},"end"=>v["entryStopAt"]=json!(iso(anchor()+DAY+1)),"previous"=>v["previousEntryStopAt"]=json!(iso(anchor()+1)),"source"=>v["source"]=json!("official_sig_api_v1"),"processed"=>{v.as_object_mut().unwrap().remove("processedAt");},_=>v["entryStopAt"]=json!(iso(anchor()+DAY).replace(".000Z",".000001Z"))}
  let before=e.report().unwrap();assert!(e.advance(&v).is_err(),"{case}");assert_eq!(e.report().unwrap(),before,"{case}");
 }
 let mut paper=Engine::new(&policy()).unwrap();let before=paper.report().unwrap();assert!(paper.advance(&event(anchor(),anchor(),anchor()+DAY)).is_err());assert_eq!(paper.report().unwrap(),before);
}

#[test]fn exact_eight_key_mandate_caps_clock_and_duration_are_required(){
 for case in ["extra","missing","empty_id","future_approval","anchor","duration","zero_campaign","interval","committed","market","quantity"]{
  let mut e=Engine::new_live(&policy()).unwrap();let mut v=event(anchor(),anchor(),anchor()+DAY);
  match case{"extra"=>v["mandate"]["reset"]=json!(true),"missing"=>{v["mandate"].as_object_mut().unwrap().remove("approvedAt");},"empty_id"=>v["mandate"]["authorizationId"]=json!(" "),"future_approval"=>v["mandate"]["approvedAt"]=json!(iso(anchor()+1)),"anchor"=>v["mandate"]["anchorMs"]=json!(anchor()-1),"duration"=>v["mandate"]["campaignEndMs"]=json!(anchor()+31*DAY+1),"zero_campaign"=>v["mandate"]["campaignEndMs"]=json!(anchor()),"interval"=>v["mandate"]["intervalMs"]=json!(DAY-1),"committed"=>v["mandate"]["maximumCommitted"]=json!("40001.000000"),"market"=>v["mandate"]["maximumMarketCost"]=json!("9999.000000"),_=>v["mandate"]["maximumQuantity"]=json!(301)}
  let before=e.report().unwrap();assert!(e.advance(&v).is_err(),"{case}");assert_eq!(e.report().unwrap(),before,"{case}");
 }
}

#[test]fn recorded_mandate_is_immutable_and_duplicate_transition_is_rejected(){
 let mut e=Engine::new_live(&policy()).unwrap();let first=event(anchor(),anchor(),anchor()+DAY);e.advance(&first).unwrap();let before=e.report().unwrap();
 assert!(e.advance(&first).is_err());assert_eq!(e.report().unwrap(),before);
 for key in ["authorizationId","approvedAt","campaignEndMs"]{
  let mut v=event(anchor()+DAY,anchor()+DAY,anchor()+2*DAY);v["mandate"][key]=match key{"authorizationId"=>json!("replacement-authorization"),"approvedAt"=>json!(iso(anchor()-2_000)),_=>json!(anchor()+21*DAY)};
  assert!(e.advance(&v).is_err(),"{key}");assert_eq!(e.report().unwrap(),before);
 }
}

#[test]fn utc_day_stays_twenty_four_hours_across_daylight_saving_boundary(){
 let mut p=policy();let a=ms("2026-10-31T18:00:00.000Z");p["entryStopAt"]=json!(iso(a));let mut v=event(a,a,a+DAY);v["mandate"]["anchorMs"]=json!(a);v["mandate"]["campaignEndMs"]=json!(a+4*DAY);v["mandate"]["approvedAt"]=json!(iso(a-1));
 let mut e=Engine::new_live(&p).unwrap();e.advance(&v).unwrap();assert_eq!(e.policy()["entryStopAt"],"2026-11-01T18:00:00.000Z");
}

struct Temp(PathBuf);
impl Temp{fn new()->Self{Self(std::env::temp_dir().join(format!("moloch-entry-windows-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())))}}
impl Drop for Temp{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
#[test]fn session_replay_uses_immutable_original_policy_and_preserves_window_event(){
 let temp=Temp::new();let p=policy();let seed=json!({"runId":"daily-window-fixture","availableCash":"100000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":iso(anchor()-1)});
 let expected;let original_manifest;
 {let mut bank=Session::open_live(&temp.0,&p,&seed).unwrap();original_manifest=fs::read(temp.0.join("manifest.json")).unwrap();bank.accept("moloch-live",1,&event(anchor(),anchor(),anchor()+DAY)).unwrap();bank.accept("moloch-live",2,&event(anchor()+2*DAY,anchor()+DAY,anchor()+3*DAY)).unwrap();expected=bank.engine().report().unwrap();}
 let bank=Session::open_live(&temp.0,&p,&seed).unwrap();assert_eq!(bank.engine().report().unwrap(),expected);assert_eq!(bank.engine().policy()["entryStopAt"],iso(anchor()+3*DAY));assert_eq!(fs::read(temp.0.join("manifest.json")).unwrap(),original_manifest);assert_eq!(bank.last_source_sequence("moloch-live"),2);
}

#[test]fn unsupported_seed_cannot_silently_drop_a_live_window_mandate(){
 assert!(Engine::from_seed_state(&json!({"dailyEntryWindows":{"mandate":mandate(),"entryStopAt":iso(anchor()+DAY)}})).is_err());
}
