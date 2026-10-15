use moloch_replay::engine::Engine;
use serde_json::{Value,json};
fn corpus()->Value{serde_json::from_str(include_str!("reference/golden.json")).unwrap()}
fn yes_order(e:&Engine)->Value{e.order_records().into_iter().find(|q|q["side"]=="YES").unwrap()}
fn live()->Engine{let mut e=Engine::new_live(&json!({"quantity":3,"feePerShare":"0.000100","entryStopAt":"2026-10-13T00:00:05Z"})).unwrap();e.advance(&corpus()[0]["steps"][0]["event"]).unwrap();e}
fn event(kind:&str,q:&Value,actual:&str,at:&str)->Value{json!({"type":kind,"source":"official_sig_api_v1","receiptHash":"a".repeat(64),"orderId":q["id"],"actualOrderId":actual,"exchangeId":q["exchangeId"],"marketId":q["marketId"],"side":q["side"],"action":q["action"],"quantity":q["quantity"],"price":q["price"],"receivedAt":at})}
fn ack(e:&mut Engine,q:&Value,id:&str){e.advance(&event("actual_order_ack",q,id,"2026-10-13T00:00:00.001Z")).unwrap();}
#[test]fn only_demonstrably_unattempted_unbound_intents_can_release_funding(){
 let mut e=live();let q=yes_order(&e);let mut v=json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","orderId":q["id"],"confirmedNetworkAttempts":1,"receiptHash":"a".repeat(64),"receivedAt":"2026-10-13T00:00:00.002Z"});
 assert!(e.advance(&v).is_err());ack(&mut e,&q,"201");v["confirmedNetworkAttempts"]=json!(0);assert!(e.advance(&v).is_err());
 let mut e=live();let before=e.wallet.reserved().unwrap();e.advance(&v).unwrap();assert!(e.wallet.reserved().unwrap()<before);assert_eq!(yes_order(&e)["retired"],true);
}
fn fill(q:&Value,id:&str,fill_id:&str,n:u64,at:&str)->Value{let mut f=event("actual_fill",q,id,at);f["fillId"]=json!(fill_id);f["quantity"]=json!(n);f["price"]=json!("0.445000");f["fee"]=json!("0.000100");f["executedAt"]=json!("2026-10-13T00:00:03Z");f}
#[test]fn market_prints_and_coverage_never_create_live_fills_or_release_cash(){
 let mut e=live();let reserve=e.report().unwrap()["reservedCash"].clone();
 for s in corpus()[0]["steps"].as_array().unwrap().iter().skip(1){e.advance(&s["event"]).unwrap();}
 let mut b=corpus()[0]["steps"][0]["event"].clone();let at="2026-10-13T00:03:00Z";b["receivedAt"]=json!(at);for k in ["sourceAt","receivedAt","requestStartedAt"]{b["book"][k]=json!(at)}b["book"]["sequence"]=json!("2");e.advance(&b).unwrap();
 e.advance(&json!({"type":"coverage","tournamentId":"cup","marketId":"101","exchangeId":"101","from":"2026-10-13T00:00:00Z","to":"2026-10-13T00:03:00Z","receivedAt":"2026-10-13T00:03:01Z","complete":true,"projectedThroughSequence":"2"})).unwrap();
 assert_eq!(e.fills().len(),0);assert_eq!(e.report().unwrap()["cash"],"1000.000000");assert_eq!(e.report().unwrap()["reservedCash"],reserve);
}
#[test]fn executed_price_fees_partial_fill_and_duplicates_are_accounted_exactly(){
 let mut e=live();let q=yes_order(&e);ack(&mut e,&q,"201");
 let f=fill(&q,"201","301",1,"2026-10-13T00:00:04Z");e.advance(&f).unwrap();
 assert_eq!(e.report().unwrap()["cash"],"999.554900");assert_eq!(e.report().unwrap()["ownedCost"],"0.445000");assert_eq!(e.report().unwrap()["realized"],"-0.000100");assert_eq!(e.fills()[0]["counterfactual"],false);
 let before=e.wallet.cash;let mut duplicate=f.clone();duplicate["receivedAt"]=json!("2026-10-13T00:00:05Z");duplicate["receiptHash"]=json!("b".repeat(64));e.advance(&duplicate).unwrap();assert_eq!(e.wallet.cash,before);assert_eq!(e.fills().len(),1);
 let mut conflict=duplicate;conflict["price"]=json!("0.440000");assert!(e.advance(&conflict).is_err());assert_eq!(e.wallet.cash,before);
}
#[test]fn terminal_status_waits_for_actual_partial_fills_then_releases_only_remainder(){
 let mut e=live();let q=yes_order(&e);ack(&mut e,&q,"201");e.advance(&fill(&q,"201","301",1,"2026-10-13T00:00:04Z")).unwrap();
 let mut terminal=event("actual_order_terminal",&q,"201","2026-10-13T00:00:05Z");terminal["status"]=json!("canceled");terminal["fillsComplete"]=json!(true);terminal["filledQuantity"]=json!(2);
 let before=e.wallet.reserved().unwrap();assert!(e.advance(&terminal).is_err());assert_eq!(e.wallet.reserved().unwrap(),before);
 terminal["filledQuantity"]=json!(1);e.advance(&terminal).unwrap();assert_eq!(before-e.wallet.reserved().unwrap(),900200);assert_eq!(e.wallet.cash,999554900);
}
#[test]fn foreign_ack_conflicting_binding_and_impossible_fills_fail_atomically(){
 let mut e=live();let q=yes_order(&e);let mut a=event("actual_order_ack",&q,"201","2026-10-13T00:00:00.001Z");a["quantity"]=json!(300);assert!(e.advance(&a).is_err());ack(&mut e,&q,"201");
 a=event("actual_order_ack",&q,"202","2026-10-13T00:00:00.002Z");assert!(e.advance(&a).is_err());
 let before=e.report().unwrap();for (id,n,price) in [("999",1,"0.445000"),("201",4,"0.445000"),("201",1,"0.455000")]{let mut f=fill(&q,id,"301",n,"2026-10-13T00:00:04Z");f["price"]=json!(price);assert!(e.advance(&f).is_err());assert_eq!(e.report().unwrap(),before);}
}
#[test]fn paper_bank_rejects_actual_account_events(){
 let mut e=Engine::new(&json!({})).unwrap();e.advance(&corpus()[0]["steps"][0]["event"]).unwrap();let q=yes_order(&e);let before=e.report().unwrap();assert!(e.advance(&event("actual_order_ack",&q,"201","2026-10-13T00:00:00.001Z")).is_err());assert_eq!(e.report().unwrap(),before);
}
#[test]fn fill_after_requested_cancel_is_not_lost_before_terminal_confirmation(){
 let mut e=live();let q=yes_order(&e);ack(&mut e,&q,"201");e.advance(&fill(&q,"201","301",1,"2026-10-13T00:00:04Z")).unwrap();
 let mut late=fill(&q,"201","302",1,"2026-10-13T00:00:09Z");late["executedAt"]=json!("2026-10-13T00:00:06Z");e.advance(&late).unwrap();assert_eq!(e.fills().len(),2);assert_eq!(e.report().unwrap()["ownedCost"],"0.890000");
}

#[test]fn covered_actual_sale_preserves_fifo_cost_and_realized_profit(){
 let mut e=live();let orders=e.order_records();let yes=yes_order(&e);let no=orders.into_iter().find(|q|q["side"]=="NO").unwrap();ack(&mut e,&yes,"201");
 let mut no_ack=event("actual_order_ack",&no,"202","2026-10-13T00:00:00.002Z");e.advance(&no_ack).unwrap();
 e.advance(&fill(&yes,"201","301",1,"2026-10-13T00:00:04Z")).unwrap();
 for (q,id,at,n) in [(&yes,"201","2026-10-13T00:00:05Z",1),(&no,"202","2026-10-13T00:00:07Z",0)]{let mut t=event("actual_order_terminal",q,id,at);t["status"]=json!("canceled");t["filledQuantity"]=json!(n);t["fillsComplete"]=json!(true);e.advance(&t).unwrap();}
 let sell=e.order_records().into_iter().find(|q|q["action"]=="sell"&&!q["retired"].as_bool().unwrap()).unwrap();
 no_ack=event("actual_order_ack",&sell,"203","2026-10-13T00:00:07.001Z");e.advance(&no_ack).unwrap();let mut f=fill(&sell,"203","302",1,"2026-10-13T00:00:11Z");f["price"]=json!("0.460000");f["executedAt"]=json!("2026-10-13T00:00:10.001Z");e.advance(&f).unwrap();assert_eq!(e.report().unwrap()["cash"],"1000.014800");assert_eq!(e.report().unwrap()["realized"],"0.014800");assert_eq!(e.report().unwrap()["ownedCost"],"0.000000");
}

#[test]fn live_session_replays_actual_fills_without_paper_seed_or_double_credit(){
 use moloch_replay::session::Session;
 let root=std::env::temp_dir().join(format!("moloch-live-bank-test-{}",std::process::id()));
 let policy=json!({"quantity":3,"feePerShare":"0.000100","entryStopAt":"2026-10-13T00:00:05Z"});let seed=json!({"runId":"fixture-live-run","accountSnapshotHash":"c".repeat(64),"availableCash":"1000.000000","observedAt":"2026-10-13T00:00:00Z"});
 let bank_path=root.join("bank");std::fs::create_dir(&root).unwrap();let q;let expected;
 {let mut bank=Session::open_live(&bank_path,&policy,&seed).unwrap();bank.accept("market",1,&corpus()[0]["steps"][0]["event"]).unwrap();q=yes_order(bank.engine());bank.accept("account",1,&event("actual_order_ack",&q,"201","2026-10-13T00:00:00.001Z")).unwrap();bank.accept("account",2,&fill(&q,"201","301",1,"2026-10-13T00:00:04Z")).unwrap();expected=bank.engine().report().unwrap();assert_eq!(expected["fillModel"],"actualAccountFills");}
 assert!(Session::open(&bank_path,None,&policy).is_err());
 {let mut bank=Session::open_live(&bank_path,&policy,&seed).unwrap();assert_eq!(bank.engine().report().unwrap(),expected);assert!(!bank.accept("account",2,&fill(&q,"201","301",1,"2026-10-13T00:00:04Z")).unwrap());assert_eq!(bank.engine().report().unwrap(),expected);}
 let bad_seed=json!({"runId":"fixture-live-run","accountSnapshotHash":"c".repeat(64),"availableCash":"999.000000","observedAt":"2026-10-13T00:00:00Z"});assert!(Session::open_live(&root.join("unfunded"),&policy,&bad_seed).is_err());assert!(!root.join("unfunded").exists());
 std::fs::remove_dir_all(root).unwrap();
}

#[test]fn admission_resize_releases_only_unadmitted_quantity_and_binding_stays_fixed(){
 let mut e=live();let q=yes_order(&e);let before=e.wallet.reserved().unwrap();let mut a=event("actual_order_ack",&q,"201","2026-10-13T00:00:00.001Z");a["quantity"]=json!(2);e.advance(&a).unwrap();assert_eq!(before-e.wallet.reserved().unwrap(),450100);assert_eq!(yes_order(&e)["quantity"],2);let mut conflict=a.clone();conflict["quantity"]=json!(1);assert!(e.advance(&conflict).is_err());e.advance(&fill(&q,"201","301",1,"2026-10-13T00:00:04Z")).unwrap();assert_eq!(e.wallet.cash,999554900);
}
