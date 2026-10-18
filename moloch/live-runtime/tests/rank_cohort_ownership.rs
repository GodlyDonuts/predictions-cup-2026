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
  "managedExchangeIds":["101"],"approvedReviewSetSha256":"b".repeat(64),"controlSha256":"c".repeat(64),"expectedPreviousExclusionSha256":null}));
 let state=f.store.as_ref().unwrap().rank_exit_state(&f.bank,&f.log,"101").unwrap();
 let body=state["cohortBodyText"].as_str().unwrap().as_bytes().to_vec();assert_eq!(sha(&body),state["cohortSha256"]);
 let proof:Value=["schemaVersion","exchangeId","generationSha256","reviewSha256","cohortSha256","originSha256","exclusionSha256"].into_iter().map(|k|(k.to_owned(),state[k].clone())).collect::<serde_json::Map<String,Value>>().into();
 let node=f.originals[2].clone();let node_sha=sha(&node);let body_sha=sha(&body);let engine=f.bank.engine();
 let c=json!({"schemaVersion":1,"objective":"declared_first_place_rank_risk","campaignStage":"direct","reviewedAtMs":NOW+8000,"validUntilMs":NOW+60000,
  "sources":[{"sha256":node_sha,"observedAtMs":NOW+7000,"maximumAgeMs":120000,"assumptionLabel":"Synthetic original contract source"},{"sha256":body_sha,"observedAtMs":NOW+7100,"maximumAgeMs":120000,"assumptionLabel":"Synthetic original cohort ownership, not probability evidence"}],
  "account":{"positionsSha256":digest(&engine.wallet.positions()),"cash":moloch_replay::money(engine.wallet.cash),"reservedCash":moloch_replay::money(engine.wallet.reserved().unwrap()),"actualFillsSha256":digest(&json!(engine.fills())),"modeledTerminalDebt":"0.000000","liabilityAssumption":"Synthetic no debt"},
  "contracts":[{"contractId":"301","exchangeId":"101","marketId":"201","semanticSourceSha256":node_sha,"officialContractRootSha256":f.review.official_contract_root_sha256}],
  "scenarios":[{"id":"no","kind":"ordinary","assumptionLabel":"Synthetic NO","payouts":{"301":"resolved_no"}},{"id":"yes","kind":"ordinary","assumptionLabel":"Synthetic YES","payouts":{"301":"resolved_yes"}}],
  "probabilityModels":[{"id":"declared","weightsPpm":[800000,200000],"assumptionLabel":"Synthetic uncalibrated weights"}],
  "fieldModels":[{"id":"unknown","thresholds":["2000.000000","1000.500000"],"completeField":false,"assumptionLabel":"Synthetic incomplete field"}],
  "modelPairs":[{"probabilityId":"declared","fieldId":"unknown"}],
  "action":{"contractId":"301","exchangeId":"101","marketId":"201","side":"NO","action":"SELL","quantity":1,"limitPrice":"0.680000","nominationBookSha256":digest(&engine.live_package_books()[0])},
  "managedExit":proof,"risk":{"maximumGrossCost":"1.000000","maximumQuantity":1,"maximumModeledLoss":"10.000000","minimumTerminalWealth":"0.000000","minimumTargetGainPpm":1},"unmodeledStateStatement":"Synthetic states are not exhaustive or calibrated"});
 let raw=serde_json::to_vec(&c).unwrap();let h=sha(&raw);let allowed=BTreeSet::from([h.clone()]);
 let sources=[rank_campaign::Original{sha256:&node_sha,bytes:&node},rank_campaign::Original{sha256:&body_sha,bytes:&body}];
 let review=rank_campaign::Reviewed::load(&raw,&h,&allowed,&sources,NOW+8000).unwrap();(f,review,c)
}
fn reserve(f:&mut F,c:&Value)->Value{
 let score=f.bank.engine().evaluate_rank_campaign(c,(NOW+8000)as i64).unwrap();assert_eq!(score["classification"],"negative_monetary_expectation");
 f.event(json!({"type":"reviewed_rank_campaign_reserved_v1","source":"moloch_live_dispatcher","receivedAt":iso(NOW+8000),
  "bankScope":{"runId":f.seed["runId"],"bankManifestHash":f.bank.manifest_hash(),"tournamentId":T},"bankHead":f.bank.durable_head(),"expectedReportSha256":digest(&f.bank.engine().report().unwrap()),"campaign":c,"campaignSha256":digest(c),"originSha256":"e".repeat(64),"evaluation":score,"currentBookSha256":score["currentBookSha256"],"expiresAt":iso(NOW+30000)}));
 f.book(NOW+11500);f.bank.engine().order_records().into_iter().find(|q|q["reviewedRankCampaign"]["campaignSha256"]==digest(c)).unwrap()
}
fn permit(q:&Value)->Permit{Permit{run_id:"passive-inherited-fixture".into(),intent_id:q["id"].as_u64().unwrap().to_string(),owner:"Codex".into(),expires_at_ms:NOW+25000,maximum_quantity:1,maximum_cost:"0.680000".into()}}

#[test]fn managed_declaration_cannot_bypass_original_cohort_or_become_an_alpha_order(){
 let(f,review,mut c)=setup();let head=f.bank.durable_head();let financial=f.bank.engine().report().unwrap();
 f.store.as_ref().unwrap().check_rank_exit_review(&f.bank,&f.log,&review).unwrap();
 c["managedExit"]["cohortSha256"]=json!("f".repeat(64));assert!(f.bank.engine().evaluate_rank_campaign(&c,(NOW+8000)as i64).is_err());
 c["action"]["action"]=json!("BUY");assert!(f.bank.engine().evaluate_rank_campaign(&c,(NOW+8000)as i64).is_err());assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().report().unwrap(),financial);
}
#[test]fn loss_sale_tracks_original_fifo_and_survives_reopen_without_rearming_old_strategy(){
 let(mut f,review,c)=setup();let q=reserve(&mut f,&c);let id=q["id"].as_u64().unwrap();let spec=f.spec(&q);let p=permit(&q);
 let before=f.bank.engine().wallet.positions();let cash=f.bank.engine().wallet.cash;let head=f.bank.durable_head();
 let token=f.store.as_mut().unwrap().bind_rank_exit(&f.bank,&f.log,&f.scope,&review,id,&spec,&p,NOW+12000,NOW+25000).unwrap();
 token.validate_frozen(&q,&f.bank.engine().report().unwrap(),id,NOW+12000).unwrap();token.authorize(&f.scope,&spec,&p,NOW+12000).unwrap();assert_eq!(f.bank.durable_head(),head);assert_eq!(f.bank.engine().wallet.positions(),before);assert_eq!(f.bank.engine().wallet.cash,cash);
 assert!(f.store.as_ref().unwrap().rank_paused("101"));assert!(f.store.as_ref().unwrap().existing_exit_hint(&f.bank,&f.log,&f.scope,"101",&f.trust,NOW+12000).unwrap().is_none());
 drop(token);f.attempt(&q,&spec,NOW+12100);f.store.as_ref().unwrap().check_rank_bound(&f.bank,&f.log,&f.scope,id,&spec).unwrap();assert!(f.store.as_mut().unwrap().bind_rank_exit(&f.bank,&f.log,&f.scope,&review,id,&spec,&p,NOW+12100,NOW+25000).is_err());
 f.ack(&q,NOW+12200);let tape=fixture::tape(&q,1,NOW+12300);for e in feedback::fills(&q,"901",T,&tape,&iso(NOW+12400)).unwrap(){f.event(e);}
 assert_eq!(f.bank.engine().wallet.positions()["101"]["NO"],json!([{"price":"0.500000","quantity":2}]));assert_eq!(f.bank.engine().wallet.cash,cash+680000);assert_eq!(f.bank.engine().wallet.realized,-220000);
 let f=f.reopen();let s=f.store.as_ref().unwrap().rank_exit_state(&f.bank,&f.log,"101").unwrap();assert_eq!(s["heldQuantity"],2);assert!(f.store.as_ref().unwrap().rank_paused("101"));assert!(f.store.as_ref().unwrap().entry_hints(&f.bank,&f.log,&f.scope,&f.trust,NOW+180000).unwrap().is_empty());
}
#[test]fn original_registration_reuses_only_unchanged_unattempted_binding_and_pins_sidecar(){
 let(mut f,review,c)=setup();let q=reserve(&mut f,&c);let id=q["id"].as_u64().unwrap();let spec=f.spec(&q);let p=permit(&q);
 let t=f.store.as_mut().unwrap().bind_rank_exit(&f.bank,&f.log,&f.scope,&review,id,&spec,&p,NOW+12000,NOW+25000).unwrap();drop(t);let before=std::fs::read(f.root.0.join("fundamental.ndjson")).unwrap();
 let t=f.store.as_mut().unwrap().bind_rank_exit(&f.bank,&f.log,&f.scope,&review,id,&spec,&p,NOW+12001,NOW+25000).unwrap();assert_eq!(std::fs::read(f.root.0.join("fundamental.ndjson")).unwrap(),before);
 let mut changed=p.clone();changed.maximum_quantity=2;assert!(t.authorize(&f.scope,&spec,&changed,NOW+12002).is_err());assert!(t.authorize(&f.scope,&spec,&p,NOW+25000).is_err());
}
