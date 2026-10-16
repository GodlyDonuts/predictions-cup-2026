//! Canonical SDK evidence, real Session; no HTTP or account client.
use moloch_live_runtime::{package_portfolio_feedback as feedback,feedback::Result};
use baal_transport_service::Receipt;
use serde_json::{json,Value};
#[path="support/probabilistic_fixture.rs"]mod fixture;
use fixture::{A,iso};
fn setup()->(A,Value,Receipt){
 let mut a=A::new(true);let p=a.f.bank.engine().package_records().as_array().unwrap()[0].clone();let qid=p["legs"][0]["qid"].as_u64().unwrap();let q=a.f.bank.engine().order_records().into_iter().find(|q|q["id"]==qid).unwrap();let at=a.at;
 let ack=json!({"type":"actual_order_ack","source":"official_sig_api_v1","receiptHash":"c".repeat(64),"orderId":qid,"actualOrderId":"99001","marketId":q["marketId"],"exchangeId":q["exchangeId"],"side":q["side"],"action":q["action"],"quantity":q["quantity"],"price":q["price"],"receivedAt":iso(at)});a.f.bank.accept("canonical-fill-ack",1,&ack).unwrap();
 let n=q["quantity"].as_u64().unwrap() as i64;let no=q["side"]=="NO";
 let tape=Receipt{endpoint:"/orders/99001/fills?limit=200".into(),request_started_at:iso(at+100),received_at:iso(at+200),status:200,data:json!({"orderId":"99001","exchangeId":q["exchangeId"],"tournamentId":a.f.scope.tournament_id,"totalQuantityFilled":if no{-n}else{n},"coverage":{"complete":true},"pagination":{"limit":200,"hasMore":false,"nextCursor":null},"data":[{"id":"88001","quantity":if no{-n}else{n},"side":if no{"no"}else{"yes"},"price":serde_json::from_str::<Value>(q["price"].as_str().unwrap()).unwrap(),"filledAt":iso(at+50)}]})};
 let p=a.f.bank.engine().package_records().as_array().unwrap()[0].clone();(a,p,tape)
}
#[test]fn exact_actual_sdk_owned_fills_path_credits_only_original_tape_after_real_session_accept(){
 let(mut a,p,tape)=setup();let head=a.f.bank.durable_head();let before=a.f.bank.engine().fills().len();let events=feedback::fills(&a.f.bank,&p,&a.f.scope,0,&tape,&iso(a.at+300)).unwrap();assert_eq!(events.len(),1);assert_eq!(a.f.bank.durable_head(),head);assert_eq!(a.f.bank.engine().fills().len(),before);
 a.f.bank.accept("canonical-fill-tape",1,&events[0]).unwrap();assert_eq!(a.f.bank.engine().fills().len(),before+1);assert_eq!(events[0]["receiptHash"],moloch_live_runtime::feedback::digest(&tape).unwrap());assert_eq!(events[0]["quantity"],p["legs"][0]["quantity"]);
}
#[test]fn forged_query_or_body_scope_coverage_and_clocks_remain_rejected_without_financial_changes()->Result<()>{
 let(a,p,tape)=setup();let head=a.f.bank.durable_head();let report=a.f.bank.engine().report()?;
 for case in ["tournament_query","cursor_query","different_path","order","exchange","tournament","coverage","pagination","reversed","before_attempt","future"]{
  let mut bad=tape.clone();match case{
   "tournament_query"=>bad.endpoint.push_str(&format!("&tournamentId={}",a.f.scope.tournament_id)),"cursor_query"=>bad.endpoint.push_str("&cursor=unverified"),"different_path"=>bad.endpoint="/portfolio/fills".into(),"order"=>bad.data["orderId"]=json!("99002"),"exchange"=>bad.data["exchangeId"]=json!("999"),"tournament"=>bad.data["tournamentId"]=json!("other"),"coverage"=>bad.data["coverage"]["complete"]=json!(false),"pagination"=>bad.data["pagination"]["hasMore"]=json!(true),"reversed"=>bad.request_started_at=iso(a.at+201),"before_attempt"=>bad.request_started_at=iso(a.at-1),_=>bad.received_at=iso(a.at+301)}
  assert!(feedback::fills(&a.f.bank,&p,&a.f.scope,0,&bad,&iso(a.at+300)).is_err(),"{case}");assert_eq!(a.f.bank.durable_head(),head);assert_eq!(a.f.bank.engine().report()?,report);
 }Ok(())
}
