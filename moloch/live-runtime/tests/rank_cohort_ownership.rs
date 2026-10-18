//! Synthetic real Session/sidecar/dispatch integration; no contest connection.
#[path="support/fundamental_passive_fixture.rs"]mod fixture;
use fixture::*;
use moloch_live_runtime::{rank_campaign,feedback};
use baal_transport_service::Permit;
use serde_json::{json,Value};
use std::collections::BTreeSet;

fn setup()->(F,rank_campaign::Reviewed,Value){
 let mut f=F::new(None,"0.900000",0,"0.000000",true);
 f.event(json!({"type":"ordinary_entry_exclusions_v1","source":"moloch_live_dispatcher","receivedAt":iso(NOW+7100),
  "bankScope":{"runId":f.seed["runId"],"bankManifestHash":f.bank.manifest_hash(),"tournamentId":T},
  "bankHead":f.bank.durable_head(),"expectedReportSha256":digest(&f.bank.engine().report().unwrap()),"managedExchangeIds":["101"],"approvedReviewSetSha256":"b".repeat(64),"controlSha256":"c".repeat(64),"expectedPreviousExclusionSha256":null}));
 let state=f.store.as_ref().unwrap().rank_inventory_state(&f.bank,&f.log,"101").unwrap();
 let body=state["sourceBodyText"].as_str().unwrap().as_bytes().to_vec();assert_eq!(sha(&body),state["sourceSha256"]);
 let proof:Value=["schemaVersion","exchangeId","generationSha256","reviewSha256","cohortSha256","originSha256","exclusionSha256","positionsSha256","handoffEstablished","sourceSha256"].into_iter().map(|k|(k.to_owned(),state[k].clone())).collect::<serde_json::Map<String,Value>>().into();
 let node=f.originals[2].clone();let node_sha=sha(&node);let body_sha=sha(&body);let engine=f.bank.engine();
 let c=json!({"schemaVersion":1,"objective":"declared_first_place_rank_risk","campaignStage":"direct","reviewedAtMs":NOW+8000,"validUntilMs":NOW+60000,
  "sources":[{"sha256":node_sha,"observedAtMs":NOW+7000,"maximumAgeMs":120000,"assumptionLabel":"Synthetic original contract source"},{"sha256":body_sha,"observedAtMs":NOW+7100,"maximumAgeMs":120000,"assumptionLabel":"Synthetic original cohort ownership, not probability evidence"}],
  "account":{"positionsSha256":digest(&engine.wallet.positions()),"cash":moloch_replay::money(engine.wallet.cash),"reservedCash":moloch_replay::money(engine.wallet.reserved().unwrap()),"actualFillsSha256":digest(&json!(engine.fills())),"modeledTerminalDebt":"0.000000","liabilityAssumption":"Synthetic no debt"},
  "contracts":[{"contractId":"301","exchangeId":"101","marketId":"201","semanticSourceSha256":node_sha,"officialContractRootSha256":f.review.official_contract_root_sha256}],
  "scenarios":[{"id":"no","kind":"ordinary","assumptionLabel":"Synthetic NO","payouts":{"301":"resolved_no"}},{"id":"yes","kind":"ordinary","assumptionLabel":"Synthetic YES","payouts":{"301":"resolved_yes"}}],
  "probabilityModels":[{"id":"declared","weightsPpm":[800000,200000],"assumptionLabel":"Synthetic uncalibrated weights"}],
  "fieldModels":[{"id":"unknown","thresholds":["2000.000000",moloch_replay::money(engine.wallet.cash+500000)],"completeField":false,"assumptionLabel":"Synthetic incomplete field"}],
  "modelPairs":[{"probabilityId":"declared","fieldId":"unknown"}],
  "action":{"contractId":"301","exchangeId":"101","marketId":"201","side":"NO","action":"SELL","quantity":1,"limitPrice":"0.680000","nominationBookSha256":digest(&engine.live_package_books()[0])},
  "managedInventory":proof,"risk":{"maximumGrossCost":"1.000000","maximumQuantity":1,"maximumModeledLoss":"10.000000","minimumTerminalWealth":"0.000000","minimumTargetGainPpm":1},"unmodeledStateStatement":"Synthetic states are not exhaustive or calibrated"});
 let raw=serde_json::to_vec(&c).unwrap();let h=sha(&raw);let allowed=BTreeSet::from([h.clone()]);
 let sources=[rank_campaign::Original{sha256:&node_sha,bytes:&node},rank_campaign::Original{sha256:&body_sha,bytes:&body}];
 let review=rank_campaign::Reviewed::load(&raw,&h,&allowed,&sources,NOW+8000).unwrap();(f,review,c)
}
fn reserve(f:&mut F,c:&Value)->Value{
 let at=c["reviewedAtMs"].as_u64().unwrap();let score=f.bank.engine().evaluate_rank_campaign(c,at as i64).unwrap();assert_eq!(score["classification"],"negative_monetary_expectation");
 f.event(json!({"type":"reviewed_rank_campaign_reserved_v1","source":"moloch_live_dispatcher","receivedAt":iso(at),
  "bankScope":{"runId":f.seed["runId"],"bankManifestHash":f.bank.manifest_hash(),"tournamentId":T},"bankHead":f.bank.durable_head(),"expectedReportSha256":digest(&f.bank.engine().report().unwrap()),"campaign":c,"campaignSha256":digest(c),"originSha256":"e".repeat(64),"evaluation":score,"currentBookSha256":score["currentBookSha256"],"expiresAt":iso(at+22000)}));
 f.book(at+3500);f.bank.engine().order_records().into_iter().find(|q|q["reviewedRankCampaign"]["campaignSha256"]==digest(c)).unwrap()
}
fn permit(q:&Value)->Permit{Permit{run_id:"passive-inherited-fixture".into(),intent_id:q["id"].as_u64().unwrap().to_string(),owner:"Codex".into(),expires_at_ms:NOW+25000,maximum_quantity:1,maximum_cost:"0.680000".into()}}

#[test]fn managed_declaration_cannot_bypass_original_cohort_or_become_an_alpha_order(){
 let(f,review,mut c)=setup();let head=f.bank.durable_head();let financial=f.bank.engine().report().unwrap();
 f.store.as_ref().unwrap().check_rank_review(&f.bank,&f.log,&review).unwrap();
 c["managedInventory"]["cohortSha256"]=json!("f".repeat(64));assert!(f.bank.engine().evaluate_rank_campaign(&c,(NOW+8000)as i64).is_err());
 c["action"]["action"]=json!("BUY");assert!(f.bank.engine().evaluate_rank_campaign(&c,(NOW+8000)as i64).is_err());assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().report().unwrap(),financial);
}
#[test]fn loss_sale_tracks_original_fifo_and_survives_reopen_without_rearming_old_strategy(){
 let(mut f,review,c)=setup();let q=reserve(&mut f,&c);let id=q["id"].as_u64().unwrap();let spec=f.spec(&q);let p=permit(&q);
 let before=f.bank.engine().wallet.positions();let cash=f.bank.engine().wallet.cash;let head=f.bank.durable_head();
 let token=f.store.as_mut().unwrap().bind_rank_trade(&f.bank,&f.log,&f.scope,&review,id,&spec,&p,NOW+12000,NOW+25000).unwrap();
 token.validate_frozen(&q,&f.bank.engine().report().unwrap(),id,NOW+12000).unwrap();token.authorize(&f.scope,&spec,&p,NOW+12000).unwrap();assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().wallet.positions(),before);assert_eq!(f.bank.engine().wallet.cash,cash);
 assert!(f.store.as_ref().unwrap().rank_paused("101"));assert!(f.store.as_ref().unwrap().existing_exit_hint(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+12000).unwrap().is_none());
 drop(token);f.attempt(&q,&spec,NOW+12100);f.store.as_ref().unwrap().check_rank_bound(&f.bank,&f.log,&f.scope,id,&spec).unwrap();assert!(f.store.as_mut().unwrap().bind_rank_trade(&f.bank,&f.log,&f.scope,&review,id,&spec,&p,NOW+12100,NOW+25000).is_err());
 f.ack(&q,NOW+12200);let tape=fixture::tape(&q,1,NOW+12300);for e in feedback::fills(&q,"901",T,&tape,&iso(NOW+12400)).unwrap(){f.event(e);}
 assert_eq!(f.bank.engine().wallet.positions()["101"]["NO"],json!([{"price":"0.500000","quantity":2}]));assert_eq!(f.bank.engine().wallet.cash,cash+680000);assert_eq!(f.bank.engine().wallet.realized,-220000);
 let f=f.reopen();let s=f.store.as_ref().unwrap().rank_inventory_state(&f.bank,&f.log,"101").unwrap();assert_eq!(s["heldQuantity"],2);assert!(f.store.as_ref().unwrap().rank_paused("101"));assert!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+180000).unwrap().is_empty());
}
#[test]fn original_registration_reuses_only_unchanged_unattempted_binding_and_pins_sidecar(){
 let(mut f,review,c)=setup();let q=reserve(&mut f,&c);let id=q["id"].as_u64().unwrap();let spec=f.spec(&q);let p=permit(&q);
 let t=f.store.as_mut().unwrap().bind_rank_trade(&f.bank,&f.log,&f.scope,&review,id,&spec,&p,NOW+12000,NOW+25000).unwrap();drop(t);let before=std::fs::read(f.root.0.join("fundamental.ndjson")).unwrap();
 let t=f.store.as_mut().unwrap().bind_rank_trade(&f.bank,&f.log,&f.scope,&review,id,&spec,&p,NOW+12001,NOW+25000).unwrap();assert_eq!(std::fs::read(f.root.0.join("fundamental.ndjson")).unwrap(),before);
 let mut changed=p.clone();changed.maximum_quantity=2;assert!(t.authorize(&f.scope,&spec,&changed,NOW+12002).is_err());assert!(t.authorize(&f.scope,&spec,&p,NOW+25000).is_err());
}

fn refreshed(f:&F,mut c:Value,at:u64,action:&str,side:&str,n:u64,price:&str)->(rank_campaign::Reviewed,Value){
 let state=f.store.as_ref().unwrap().rank_inventory_state(&f.bank,&f.log,"101").unwrap();let body=state["sourceBodyText"].as_str().unwrap().as_bytes().to_vec();let body_sha=sha(&body);let node=&f.originals[2];let node_sha=sha(node);
 c["managedInventory"]=["schemaVersion","exchangeId","generationSha256","reviewSha256","cohortSha256","originSha256","exclusionSha256","positionsSha256","handoffEstablished","sourceSha256"].into_iter().map(|k|(k.to_owned(),state[k].clone())).collect::<serde_json::Map<String,Value>>().into();
 c["reviewedAtMs"]=json!(at);c["sources"][1]["sha256"]=json!(body_sha);c["sources"][1]["observedAtMs"]=json!(at);c["account"]["positionsSha256"]=json!(digest(&f.bank.engine().wallet.positions()));c["account"]["cash"]=json!(moloch_replay::money(f.bank.engine().wallet.cash));c["account"]["reservedCash"]=json!(moloch_replay::money(f.bank.engine().wallet.reserved().unwrap()));c["account"]["actualFillsSha256"]=json!(digest(&json!(f.bank.engine().fills())));
 c["fieldModels"][0]["thresholds"][1]=json!(moloch_replay::money(f.bank.engine().wallet.cash+500000));c["action"]["action"]=json!(action);c["action"]["side"]=json!(side);c["action"]["quantity"]=json!(n);c["action"]["limitPrice"]=json!(price);c["risk"]["maximumQuantity"]=json!(n);c["action"]["nominationBookSha256"]=json!(digest(&f.bank.engine().live_package_books()[0]));
 let raw=serde_json::to_vec(&c).unwrap();let h=sha(&raw);let allowed=BTreeSet::from([h.clone()]);let sources=[rank_campaign::Original{sha256:&node_sha,bytes:node},rank_campaign::Original{sha256:&body_sha,bytes:&body}];let r=rank_campaign::Reviewed::load(&raw,&h,&allowed,&sources,at).unwrap();(r,c)
}
fn register(f:&mut F,r:&rank_campaign::Reviewed,c:&Value,at:u64)->(Value,baal_transport_service::RequestSpec){
 let q=reserve(f,c);let id=q["id"].as_u64().unwrap();let price=serde_json::from_str::<Value>(q["price"].as_str().unwrap()).unwrap();let side=if q["side"]=="YES"{"yes"}else{"no"};let spec=baal_transport_service::RequestSpec{method:"POST".into(),endpoint:"/orders".into(),body:Some(json!({"idempotencyKey":format!("passive-inherited-fixture:{id}"),"tournamentId":T,"exchangeId":"101","side":side,"action":q["action"],"quantity":q["quantity"],"price":price,"expirationDate":iso(q["expiresAt"].as_u64().unwrap())})),body_json:None};let p=Permit{run_id:"passive-inherited-fixture".into(),intent_id:id.to_string(),owner:"Codex".into(),expires_at_ms:at+10000,maximum_quantity:q["quantity"].as_u64().unwrap()as u32,maximum_cost:moloch_replay::money(baal_transport_service::decimal_units(q["price"].as_str().unwrap()).unwrap()*q["quantity"].as_u64().unwrap()as i128)};
 let token=f.store.as_mut().unwrap().bind_rank_trade(&f.bank,&f.log,&f.scope,r,id,&spec,&p,at,at+10000).unwrap();token.validate_frozen(&q,&f.bank.engine().report().unwrap(),id,at).unwrap();drop(token);(q,spec)
}
fn fill_and_close(f:&mut F,q:&Value,spec:&baal_transport_service::RequestSpec,actual:u64,at:u64){
 let id=q["id"].as_u64().unwrap();let side=if q["side"]=="YES"{"yes"}else{"no"};let price=serde_json::from_str::<Value>(q["price"].as_str().unwrap()).unwrap();let quantity=q["quantity"].as_u64().unwrap();f.attempt(q,spec,at);
 let ack=fixture::receipt("/orders",json!({"orderId":actual.to_string(),"exchangeId":"101","side":side,"action":q["action"],"quantity":quantity,"price":price}),at+1,at+1);let event=feedback::acknowledgment(q,&ack,&iso(at+1)).unwrap();f.store.as_ref().unwrap().validate_acknowledgment(id,&event).unwrap();f.log.append(json!({"kind":"ack","qid":id,"receipt":ack})).unwrap();f.event(event);
 let tape=fixture::receipt(&format!("/orders/{actual}/fills?limit=200"),json!({"orderId":actual.to_string(),"exchangeId":"101","tournamentId":T,"totalQuantityFilled":quantity,"data":[{"id":actual*100,"quantity":quantity,"side":side,"price":price,"filledAt":iso(at+2)}],"pagination":{"hasMore":false,"nextCursor":null},"coverage":{"complete":true,"projectedThroughSequence":actual*100+1}}),at+3,at+3);
 for event in feedback::fills(q,&actual.to_string(),T,&tape,&iso(at+4)).unwrap(){f.event(event);}f.log.append(json!({"kind":"closed","qid":id,"at":iso(at+5)})).unwrap();
}
#[test]fn completed_handoff_allows_opposite_buy_and_preserves_every_original_lot_through_restart(){
 let(mut f,_,c)=setup();let starting_cash=f.bank.engine().wallet.cash;let(r,c)=refreshed(&f,c,NOW+8000,"SELL","NO",3,"0.680000");let(q,s)=register(&mut f,&r,&c,NOW+12000);fill_and_close(&mut f,&q,&s,901,NOW+12100);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),0);
 f.book(NOW+16000);let(r,c)=refreshed(&f,c,NOW+16000,"BUY","YES",3,"0.320000");assert_eq!(c["managedInventory"]["handoffEstablished"],true);let(q,s)=register(&mut f,&r,&c,NOW+20000);fill_and_close(&mut f,&q,&s,902,NOW+20100);
 assert_eq!(f.bank.engine().wallet.cash,starting_cash+2040000-960000);let f=f.reopen();let state=f.store.as_ref().unwrap().rank_inventory_state(&f.bank,&f.log,"101").unwrap();assert_eq!(state["side"],"YES");assert_eq!(state["heldQuantity"],3);assert_eq!(f.bank.engine().wallet.quantity("101",moloch_replay::Side::No),0);assert_eq!(f.bank.engine().wallet.realized,140000);
 assert_eq!(f.bank.engine().wallet.positions()["101"]["YES"],json!([{"price":"0.320000","quantity":3,"executedAt":iso(NOW+20102)}]));assert!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+21000).unwrap().is_empty());
}
