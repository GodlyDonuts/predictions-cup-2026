// Direct inclusion keeps this candidate independent of parent-owned lib/worker.
#[path = "../src/reconciliation_plan.rs"]
mod reconciliation_plan;
use reconciliation_plan::*;
use baal_transport_service::{Receipt, Scope};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn base() -> u64 { chrono::DateTime::parse_from_rfc3339("2026-10-15T09:00:00Z").unwrap().timestamp_millis() as u64 }
fn iso(ms: u64) -> String { chrono::DateTime::from_timestamp_millis(ms as i64).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true) }
fn scope() -> Scope { Scope { tournament_id: "bda92870-621e-47b0-bc3c-3602c5c26f55".into(), tournament_slug: "midterm-elections".into(), exchanges: BTreeMap::from([("994".into(), "305".into())]) } }
fn owned(side: &str, action: &str, credited: u64) -> OwnedOrder { OwnedOrder {
    logical_id: 1, actual_order_id: "201".into(), exchange_id: "994".into(), market_id: "305".into(),
    side: side.into(), action: action.into(), limit_price: "0.120000".into(), admitted_quantity: 10,
    credited_filled_quantity: credited, created_at_ms: base() - 1000, expires_at_ms: base() + 300_000,
    cancel_pending: false, acceptance_uncertain: false,
} }
fn row(q: &OwnedOrder) -> Value { json!({"id":q.actual_order_id.parse::<u64>().unwrap(),"exchangeId":q.exchange_id,"tournamentId":scope().tournament_id,"side":q.side,"action":q.action,
    "quantity":q.admitted_quantity-q.credited_filled_quantity,"priceLimit":0.12,"open":true,"createdAt":iso(base()),"expirationDate":iso(q.expires_at_ms),"quantityFilled":null,"terminalReasonCode":null}) }
fn receipt(endpoint: String, data: Value, start: u64, end: u64) -> Receipt { Receipt { endpoint, data, request_started_at: iso(start), received_at: iso(end), status: 200 } }
fn aggregate(rows: Vec<Value>, start: u64, sequence: u64) -> Receipt { receipt(format!("/orders?status=open&limit=200&tournamentId={}",scope().tournament_id),
    json!({"data":rows,"coverage":{"complete":true,"projectedThroughSequence":sequence},"pagination":{"limit":200,"hasMore":false,"nextCursor":null}}),start,start+100) }
fn tape(q: &OwnedOrder, start: u64, sequence: u64) -> Receipt {
    let signed = if q.side == "no" { -(q.credited_filled_quantity as i64) } else { q.credited_filled_quantity as i64 };
    let data = if signed==0 {vec![]} else {vec![json!({"id":301,"quantity":signed,"side":q.side,"price":0.12,"filledAt":iso(start-100)})]};
    receipt(format!("/orders/{}/fills?limit=200",q.actual_order_id),json!({"orderId":q.actual_order_id.parse::<u64>().unwrap(),"exchangeId":q.exchange_id,"tournamentId":scope().tournament_id,
        "data":data,"totalQuantityFilled":signed,"coverage":{"complete":true,"projectedThroughSequence":sequence},"pagination":{"limit":200,"hasMore":false,"nextCursor":null}}),start,start+100)
}
fn planned(q: &OwnedOrder, r: &Receipt) -> ReadPlan { plan(&scope(),std::slice::from_ref(q),&[],None,Some(r),false,base()+1000,Limits::default()).unwrap() }
fn open(p: &ReadPlan) -> &OpenMember { match &p.decisions[0] { OrderDecision::OpenMemberNeedsFillTape(v)=>v,_=>panic!("expected open member: {:?}",p) } }
fn reason(p: &ReadPlan) -> Reason { match &p.decisions[0] { OrderDecision::ExactPairFallback(v)=>{assert!(v.order_before_complete_fills);v.reason},_=>panic!("expected exact pair") } }

#[test] fn all_four_canonical_side_action_pairs_keep_every_fill_read() {
    for side in ["yes","no"] { for action in ["buy","sell"] {
        let q=owned(side,action,2);let r=aggregate(vec![row(&q)],base()+200,100);let p=planned(&q,&r);
        assert!(open(&p).complete_fill_tape_required);assert_eq!(p.full_fill_tapes_required,1);
        assert_eq!(p.owned_endpoint_operations,1);assert!(!p.execution_authority);assert!(!p.blocks_new_submissions);
        assert_eq!(open(&p).aggregate_endpoint,r.endpoint);assert_eq!(open(&p).aggregate_row_index,0);
        assert!(!open(&p).aggregate_endpoint.starts_with("/orders/201"));
    } }
}
#[test] fn realistic_sixty_second_cycle_saves_reads_without_deferring_any_tape() {
    let mut orders=Vec::new();let mut watermarks=Vec::new();
    for i in 0..20 {let mut q=owned("no","sell",2);q.logical_id=i+1;q.actual_order_id=(201+i).to_string();
        watermarks.push(fill_watermark(&scope(),&q,&tape(&q,base()+100,100),base()+1000).unwrap());orders.push(q);}
    let r=aggregate(orders.iter().map(row).collect(),base()+60_000,200);
    let p=plan(&scope(),&orders,&watermarks,None,Some(&r),false,base()+60_500,Limits::default()).unwrap();
    assert_eq!(p.full_fill_tapes_required,20);assert_eq!(p.owned_endpoint_operations,20);
    assert_eq!(p.endpoint_operations_including_aggregate,21);assert_eq!(p.baseline_endpoint_operations,40);
    assert!(p.decisions.iter().all(|d|matches!(d,OrderDecision::OpenMemberNeedsFillTape(_))));
    assert_eq!(open(&p).retained_fill_watermark,Some(watermarks[0].clone()));
}
#[test] fn smaller_remainder_is_neither_a_fill_nor_a_new_admission() {
    let q=owned("no","buy",2);let mut v=row(&q);v["quantity"]=json!(5);
    let p=planned(&q,&aggregate(vec![v],base()+200,100));assert_eq!(open(&p).observed_executable_remainder,5);
    assert_eq!(p.full_fill_tapes_required,1);assert_eq!(q.admitted_quantity,10);assert_eq!(q.credited_filled_quantity,2);
}
#[test] fn simulated_worker_after_sixty_seconds_really_requests_all_twenty_fill_tapes() {
    let mut orders=Vec::new();for i in 0..20 {let mut q=owned("yes","buy",0);q.logical_id=i+1;q.actual_order_id=(201+i).to_string();orders.push(q);}
    let r=aggregate(orders.iter().map(row).collect(),base()+60_000,200);
    let p=plan(&scope(),&orders,&[],None,Some(&r),false,base()+60_100,Limits::default()).unwrap();
    let mut at=base()+60_100;let mut operations=vec!["open-list"];let mut tapes=0;
    for decision in &p.decisions {match decision {
        OrderDecision::OpenMemberNeedsFillTape(member) if member_is_fresh(member,at)=>{operations.push("full-fills");tapes+=1;at+=600;},
        _=>{operations.push("exact-order");operations.push("full-fills");tapes+=1;at+=1200;}
    }}
    assert_eq!(tapes,20);assert_eq!(operations.len(),21);assert_eq!(operations.iter().filter(|op|**op=="exact-order").count(),0);
}
#[test] fn slow_cohort_simulation_rechecks_age_and_falls_back_instead_of_skipping_fills() {
    let mut orders=Vec::new();for i in 0..20 {let mut q=owned("yes","buy",0);q.logical_id=i+1;q.actual_order_id=(201+i).to_string();orders.push(q);}
    let r=aggregate(orders.iter().map(row).collect(),base()+60_000,200);
    let p=plan(&scope(),&orders,&[],None,Some(&r),false,base()+60_100,Limits::default()).unwrap();
    let mut at=base()+60_100;let mut operations=1;let mut tapes=0;let mut fallbacks=0;
    for decision in &p.decisions {match decision {
        OrderDecision::OpenMemberNeedsFillTape(member) if member_is_fresh(member,at)=>{operations+=1;tapes+=1;at+=2000;},
        _=>{operations+=2;tapes+=1;fallbacks+=1;at+=4000;}
    }}
    assert_eq!(tapes,20);assert_eq!(fallbacks,10);assert_eq!(operations,31);assert!(operations<p.baseline_endpoint_operations);
}
#[test] fn zero_negative_larger_or_nonintegral_remainder_falls_back() {
    let q=owned("no","buy",2);
    for n in [json!(0),json!(-8),json!(9),json!(8.5)] {let mut v=row(&q);v["quantity"]=n;
        assert_eq!(reason(&planned(&q,&aggregate(vec![v],base()+200,100))),Reason::RemainderInvalid);}
}
#[test] fn missing_member_never_means_terminal() {
    let q=owned("yes","sell",0);let p=planned(&q,&aggregate(vec![],base()+200,100));
    assert_eq!(reason(&p),Reason::OrderMissing);assert_eq!(p.full_fill_tapes_required,1);assert_eq!(p.owned_endpoint_operations,2);
}
#[test] fn canonical_terms_and_official_nulls_are_strict() {
    let q=owned("no","buy",0);
    for (key,value) in [("exchangeId",json!("995")),("side",json!("yes")),("action",json!("sell")),("priceLimit",json!(0.125)),("quantityFilled",json!(0)),("terminalReasonCode",json!("Expired")),("expirationDate",json!(iso(q.expires_at_ms+1))),("createdAt",json!(iso(q.created_at_ms-1))),("marketId",json!("999"))] {
        let mut v=row(&q);v[key]=value;let p=planned(&q,&aggregate(vec![v],base()+200,100));
        assert_eq!(reason(&p),Reason::OrderTermsChanged,"{key}");assert!(p.blocks_new_submissions);
    }
}
#[test] fn absent_open_null_field_and_wrong_tournament_fail_closed() {
    let q=owned("no","buy",0);let mut v=row(&q);v.as_object_mut().unwrap().remove("quantityFilled");
    assert_eq!(reason(&planned(&q,&aggregate(vec![v],base()+200,100))),Reason::OrderTermsChanged);
    let mut v=row(&q);v["tournamentId"]=json!("foreign");assert_eq!(reason(&planned(&q,&aggregate(vec![v],base()+200,100))),Reason::AggregateInvalid);
}
#[test] fn partial_pages_and_unknown_or_missing_projection_never_replace_get() {
    let q=owned("no","buy",0);
    for key in ["complete","hasMore","cursor","sequence","missing"] {let mut r=aggregate(vec![row(&q)],base()+200,100);
        match key {"complete"=>r.data["coverage"]["complete"]=json!(false),"hasMore"=>r.data["pagination"]["hasMore"]=json!(true),"cursor"=>r.data["pagination"]["nextCursor"]=json!("next"),"sequence"=>r.data["coverage"]["projectedThroughSequence"]=Value::Null,_=>{r.data.as_object_mut().unwrap().remove("coverage");}}
        assert_eq!(reason(&planned(&q,&r)),Reason::AggregateInvalid);}
}
#[test] fn wrong_status_filter_or_unscoped_endpoint_is_rejected() {
    let q=owned("no","buy",0);
    for path in ["/orders?status=all","/orders?status=open","/orders/201"] {let mut r=aggregate(vec![row(&q)],base()+200,100);r.endpoint=path.into();assert_eq!(reason(&planned(&q,&r)),Reason::AggregateInvalid);}
}
#[test] fn duplicate_or_foreign_account_orders_gate_submissions() {
    let q=owned("no","buy",0);let r=aggregate(vec![row(&q),row(&q)],base()+200,100);assert_eq!(reason(&planned(&q,&r)),Reason::AggregateInvalid);
    let mut foreign=row(&q);foreign["id"]=json!(999);let p=planned(&q,&aggregate(vec![row(&q),foreign],base()+200,100));
    assert_eq!(reason(&p),Reason::ForeignOpenOrder);assert!(p.blocks_new_submissions);assert_eq!(p.foreign_open_order_ids,vec!["999"]);
}
#[test] fn future_reversed_and_stale_receipts_fall_back() {
    let q=owned("no","buy",0);
    for mode in 0..4 {let mut r=aggregate(vec![row(&q)],base()+200,100);match mode {
        0=>r.received_at=iso(base()+1001),1=>r.request_started_at=iso(base()+400),2=>r.request_started_at=iso(base()-20_000),_=>r.status=503}
        let p=planned(&q,&r);assert!(matches!(reason(&p),Reason::AggregateInvalid|Reason::AggregateStale));}
}
#[test] fn later_sequence_cannot_hide_receipt_clock_rollback() {
    let q=owned("no","buy",0);let a=aggregate(vec![row(&q)],base()+200,100);let first=planned(&q,&a);
    let r=aggregate(vec![row(&q)],base()+100,200);
    let p=plan(&scope(),&[q],&[],first.observed_projection.as_ref(),Some(&r),false,base()+1000,Limits::default()).unwrap();assert_eq!(reason(&p),Reason::ProjectionRegressed);
}
#[test] fn projection_checkpoint_cannot_regress_below_any_prior_complete_fill() {
    let q=owned("no","buy",2);let mark=fill_watermark(&scope(),&q,&tape(&q,base()+100,200),base()+1000).unwrap();
    let r=aggregate(vec![row(&q)],base()+300,199);let p=plan(&scope(),&[q],&[mark],None,Some(&r),false,base()+1000,Limits::default()).unwrap();assert_eq!(reason(&p),Reason::ProjectionRegressed);
}
#[test] fn cancellation_expiry_unknown_acceptance_and_fully_filled_always_use_exact_pair() {
    for mode in 0..5 {let mut q=owned("no","sell",0);match mode{0=>q.cancel_pending=true,1=>q.expires_at_ms=base()+900,2=>q.acceptance_uncertain=true,3=>q.credited_filled_quantity=10,_=>{}}
        let r=aggregate(vec![row(&q)],base()+200,100);let p=plan(&scope(),&[q],&[],None,Some(&r),mode==4,base()+1000,Limits::default()).unwrap();
        assert!(matches!(reason(&p),Reason::CancelPending|Reason::OrderExpiring|Reason::AcceptanceUncertain));assert_eq!(p.owned_endpoint_operations,2);}
}
#[test] fn large_cohort_hint_expires_before_a_later_fill_request() {
    let q=owned("no","buy",0);let r=aggregate(vec![row(&q)],base()+200,100);let p=planned(&q,&r);let m=open(&p);
    assert!(member_is_fresh(m,base()+500));assert!(!member_is_fresh(m,base()+20_201));
    assert!(!member_is_fresh(m,base()+299));
}
#[test] fn fill_request_must_follow_original_aggregate_and_cover_its_checkpoint() {
    let q=owned("no","buy",0);let r=aggregate(vec![row(&q)],base()+200,100);let p=planned(&q,&r);let m=open(&p);
    validate_member_fills(m,&tape(&q,base()+400,100),base()+1000).unwrap();
    for (start,seq) in [(base()+299,100),(base()+20_201,100),(base()+400,99)] {assert!(validate_member_fills(m,&tape(&q,start,seq),base()+30_000).is_err());}
    let mut f=tape(&q,base()+400,100);f.endpoint="/orders/202/fills?limit=200".into();assert!(validate_member_fills(m,&f,base()+1000).is_err());
}
#[test] fn full_fill_race_does_not_create_terminal_proof_in_the_plan() {
    let q=owned("no","sell",0);let r=aggregate(vec![row(&q)],base()+200,100);let p=planned(&q,&r);
    let mut after=q.clone();after.credited_filled_quantity=10;let f=tape(&after,base()+400,110);
    validate_member_fills(open(&p),&f,base()+1000).unwrap();fill_watermark(&scope(),&after,&f,base()+1000).unwrap();
    assert!(matches!(p.decisions[0],OrderDecision::OpenMemberNeedsFillTape(_)));assert!(!p.execution_authority);
    let next=planned(&after,&aggregate(vec![],base()+600,120));assert_eq!(reason(&next),Reason::OrderExpiring);
}
#[test] fn fill_watermark_requires_exact_complete_official_ledger_count() {
    let q=owned("no","buy",2);
    for mode in 0..6 {let mut f=tape(&q,base()+100,100);match mode {
        0=>f.data["totalQuantityFilled"]=json!(2),1=>f.data["data"][0]["quantity"]=json!(-1),2=>f.data["pagination"]["hasMore"]=json!(true),3=>f.data["orderId"]=json!(999),4=>{let first=f.data["data"][0].clone();f.data["data"].as_array_mut().unwrap().push(first);},_=>f.data["data"][0]["filledAt"]=json!(iso(base()+1001))}
        assert!(fill_watermark(&scope(),&q,&f,base()+1000).is_err());}
}
#[test] fn changed_ledger_and_duplicate_or_unowned_input_watermarks_reject_reuse() {
    let q=owned("no","buy",2);let mark=fill_watermark(&scope(),&q,&tape(&q,base()+100,100),base()+1000).unwrap();
    let mut changed=q.clone();changed.credited_filled_quantity=3;let r=aggregate(vec![row(&changed)],base()+300,110);
    let p=plan(&scope(),&[changed],std::slice::from_ref(&mark),None,Some(&r),false,base()+1000,Limits::default()).unwrap();assert_eq!(reason(&p),Reason::FillWatermarkChanged);
    assert!(plan(&scope(),std::slice::from_ref(&q),&[mark.clone(),mark],None,Some(&r),false,base()+1000,Limits::default()).is_err());
    assert!(plan(&scope(),&[q.clone(),q],&[],None,Some(&r),false,base()+1000,Limits::default()).is_err());
}
#[test] fn canonical_seven_fractional_rfc3339_clocks_are_supported() {
    let q=owned("no","buy",0);let mut v=row(&q);v["createdAt"]=json!("2026-10-15T09:00:00.0000000+00:00");assert!(open(&planned(&q,&aggregate(vec![v],base()+200,100))).complete_fill_tape_required);
}
#[test] fn invalid_bounds_and_owned_scope_are_not_silently_truncated() {
    let q=owned("no","buy",0);let r=aggregate(vec![row(&q)],base()+200,100);
    for age in [0,20_001] {assert!(plan(&scope(),std::slice::from_ref(&q),&[],None,Some(&r),false,base()+1000,Limits{maximum_snapshot_age_ms:age,..Limits::default()}).is_err());}
    let mut wrong=q.clone();wrong.market_id="999".into();assert!(plan(&scope(),&[wrong],&[],None,Some(&r),false,base()+1000,Limits::default()).is_err());
    let r=aggregate(vec![row(&q),row(&q)],base()+200,100);let p=plan(&scope(),&[q],&[],None,Some(&r),false,base()+1000,Limits{maximum_rows:1,..Limits::default()}).unwrap();assert_eq!(reason(&p),Reason::AggregateInvalid);
}
