//! Actual source tracker + durable native Session, fixed virtual clocks only.
//! No HTTP/client/worker dispatch is exercised or authorized by this fixture.
use baal_transport_service::{Receipt,Scope};
use moloch_live_runtime::exit_refresh::{BookAcceptance,PostBookPlanning,ReadHint,RefreshFacts,RefreshPlan,RefreshStage,Safety};
use moloch_replay::{session::{Session,BatchEntry},source::RestSource,Side};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{collections::BTreeMap,fs,path::PathBuf,sync::atomic::{AtomicU64,Ordering},time::{SystemTime,UNIX_EPOCH}};
const TOURNAMENT:&str="11111111-1111-4111-8111-111111111111";
const SLUG:&str="offline-lifecycle";
fn at(ms:u64)->String{let base=chrono::DateTime::parse_from_rfc3339("2026-10-15T00:00:00Z").unwrap().timestamp_millis();chrono::DateTime::from_timestamp_millis(base+ms as i64).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
fn ms(relative:u64)->u64{chrono::DateTime::parse_from_rfc3339(&at(relative)).unwrap().timestamp_millis()as u64}
fn scope()->Scope{Scope{tournament_id:TOURNAMENT.into(),tournament_slug:SLUG.into(),exchanges:BTreeMap::from([("2".into(),"1".into())])}}
struct Fixture{root:PathBuf,bank:Session,source:RestSource,hint:ReadHint,old:Value,cutoff:u64}
impl Drop for Fixture{fn drop(&mut self){/* Session owns its lock until field drop; fixtures are synthetic and retained. */}}
fn head(f:&Fixture)->[u8;32]{Sha256::digest(fs::read(f.root.join("bank/head.json")).unwrap()).into()}
fn safety(f:&Fixture)->Safety{Safety{never_attempted:true,has_intent:false,unresolved_attempts:false,global_unknown:false,stop:false,stream_healthy:true,owned_quantity:f.bank.engine().wallet.quantity("2",Side::Yes),reserved_sell_quantity:0,same_exchange_pending_physical_order:false,lifecycle_fingerprint:Sha256::digest(serde_json::to_vec(&f.bank.engine().report().unwrap()).unwrap()).into(),native_head_fingerprint:head(f)}}
fn commit(bank:&mut Session,events:&[Value],processed:u64)->Vec<Value>{
 let first=bank.last_source_sequence("offline-lifecycle")+1;let stamped=events.iter().map(|e|{let mut e=e.clone();e["processedAt"]=json!(at(processed));e}).collect::<Vec<_>>();
 let input=stamped.iter().enumerate().map(|(i,event)|BatchEntry{source_id:"offline-lifecycle",sequence:first+i as u64,event}).collect::<Vec<_>>();let result=bank.accept_batch(&input).unwrap();assert_eq!(result.appended_rows,events.len());assert!(result.accepted.iter().all(|v|*v));stamped
}
fn fixture()->Fixture{
 static N:AtomicU64=AtomicU64::new(0);let root=std::env::temp_dir().join(format!("moloch-exit-lifecycle-{}-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),N.fetch_add(1,Ordering::SeqCst)));fs::create_dir(&root).unwrap();#[cfg(unix)]{use std::os::unix::fs::PermissionsExt;fs::set_permissions(&root,fs::Permissions::from_mode(0o700)).unwrap();}
 let policy=json!({"quantity":1,"maximumInventoryAgeMs":1000,"minimumQuoteAgeMs":10000,"inventoryAgeClock":"execution","retainProfitableHedge":false});let seed=json!({"runId":"offline-source-lifecycle","availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":at(0)});let mut bank=Session::open_live(&root.join("bank"),&policy,&seed).unwrap();
 let book=json!({"type":"book","receivedAt":at(0),"book":{"tournamentId":TOURNAMENT,"marketId":"1","exchangeId":"2","sequence":"1","requestStartedAt":at(0),"sourceAt":at(0),"receivedAt":at(0),"bids":[{"price":"0.450000","quantity":1000}],"asks":[{"price":"0.455000","quantity":1000}]}});commit(&mut bank,&[book],0);
 let records=bank.engine().order_records();let yes=records.iter().find(|q|q["side"]=="YES"&&q["action"]=="buy").unwrap().clone();let no=records.iter().find(|q|q["side"]=="NO"&&q["action"]=="buy").unwrap().clone();
 let actual=|kind:&str,time:u64|json!({"type":kind,"source":"official_sig_api_v1","receiptHash":"b".repeat(64),"orderId":yes["id"],"actualOrderId":"201","exchangeId":"2","marketId":"1","side":"YES","action":"buy","quantity":1,"price":yes["price"],"receivedAt":at(time)});
 commit(&mut bank,&[actual("actual_order_ack",3000)],3000);let mut fill=actual("actual_fill",5000);fill["fillId"]=json!("301");fill["fee"]=json!("0.000000");fill["executedAt"]=json!(at(4000));commit(&mut bank,&[fill],5000);
 let mut terminal=actual("actual_order_terminal",5100);terminal["status"]=json!("filled");terminal["filledQuantity"]=json!(1);terminal["fillsComplete"]=json!(true);commit(&mut bank,&[terminal],5100);
 let retired=json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","receiptHash":"c".repeat(64),"orderId":no["id"],"confirmedNetworkAttempts":0,"receivedAt":at(5500)});commit(&mut bank,&[retired],5500);
 let old=bank.engine().order_records().into_iter().find(|q|q["action"]=="sell"&&q["inventoryExitReason"]=="maximumAge"&&q["retired"]==false).expect("actual held lot creates original maximum-age SELL");
 assert_eq!(old["activationAt"].as_u64().unwrap()-old["createdAt"].as_u64().unwrap(),3000);assert_eq!(old["expiresAt"].as_u64().unwrap()-old["createdAt"].as_u64().unwrap(),13000);
 let source=RestSource::new(&json!([{"marketId":"1","exchangeId":"2"}]),TOURNAMENT,&at(15000)).unwrap();
 // The hint comes from actual native output and actual held quantity, not a
 // hand-built successor. Journal/queue flags describe this offline no-dispatch fixture.
 let mut f=Fixture{root,bank,source,hint:ReadHint::from_expired(&old,Safety{never_attempted:true,has_intent:false,unresolved_attempts:false,global_unknown:false,stop:false,stream_healthy:true,owned_quantity:1,reserved_sell_quantity:0,same_exchange_pending_physical_order:false,lifecycle_fingerprint:[1;32],native_head_fingerprint:[1;32]},ms(20000)).unwrap(),old,cutoff:0};
 f.cutoff=f.bank.engine().order_records().iter().map(|q|q["id"].as_u64().unwrap()).max().unwrap();
 let cleanup=json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","receiptHash":"d".repeat(64),"orderId":f.old["id"],"confirmedNetworkAttempts":0,"receivedAt":at(20100)});commit(&mut f.bank,&[cleanup],20100);assert_eq!(f.bank.engine().wallet.quantity("2",Side::Yes),1);f
}
fn coverage(partial:bool)->Value{json!({"kind":"trades","source":"official_sig_api_v1","status":200,"marketId":"1","exchangeId":"2","tournamentId":TOURNAMENT,"requestStartedAt":at(19000),"receivedAt":at(19500),"endpoint":format!("/exchanges/2/trades?tournamentId={TOURNAMENT}&from={}&to={}",at(14000),at(19000)),"data":{"marketId":"1","exchangeId":"2","from":at(14000),"to":at(19000),"data":[],"pagination":{"hasMore":partial,"nextCursor":if partial{json!("next-page")}else{Value::Null}},"coverage":{"complete":true,"projectedThroughSequence":100}}})}
fn receipts()->[Receipt;7]{
 let context=json!([{"type":"tournament","tournament":{"id":TOURNAMENT},"status":"open"}]);let data=[json!({"id":TOURNAMENT,"slug":SLUG,"status":"active","myBalance":999.55}),json!({"positions":[{"exchangeId":"2","marketId":"1","quantity":1,"lots":[{"lotId":"301","side":"YES","quantity":1,"entryPrice":0.45,"openedAt":at(4000)}]}]}),json!({"data":[],"coverage":{"complete":true},"pagination":{"hasMore":false,"nextCursor":null}}),json!({"balance":999.55,"availableAdvance":0,"buyingPower":999.55}),json!({"id":"1","status":"open","isComposite":false,"isMultiOutcome":false,"contexts":context}),json!({"market_id":"1","root":{"node_type":"contract","id":"offline-contract"},"contexts":context}),json!({"contexts":[{"type":"tournament","tournament":{"id":TOURNAMENT},"orderbook":{"exchanges":[{"exchangeId":"2","asOf":{"sequence":100,"at":at(26000)},"bids":[{"price":0.45,"quantity":1000}],"asks":[{"price":0.455,"quantity":1000}]}]}}]})];
 let endpoints=[format!("/tournaments/{SLUG}"),format!("/tournaments/{SLUG}/portfolio/positions"),format!("/orders?status=open&limit=200&tournamentId={TOURNAMENT}"),format!("/account?tournamentId={TOURNAMENT}"),format!("/markets/1?tournamentId={TOURNAMENT}"),format!("/markets/1/nodes?tournamentId={TOURNAMENT}"),format!("/markets/1/orderbook?tournamentId={TOURNAMENT}&depth=20")];
 std::array::from_fn(|i|Receipt{endpoint:endpoints[i].clone(),status:200,request_started_at:at(20000+i as u64*1000),received_at:at(20500+i as u64*1000),data:data[i].clone()})
}
fn wrapped(book:&Receipt)->Value{json!({"source":"official_sig_api_v1","kind":"book","tournamentId":TOURNAMENT,"marketId":"1","exchangeId":"2","endpoint":book.endpoint,"status":book.status,"requestStartedAt":book.request_started_at,"receivedAt":book.received_at,"data":book.data})}
fn recover(f:&mut Fixture)->(RefreshFacts,Vec<Value>){
 let coverage=f.source.accept(&coverage(false)).unwrap();assert_eq!(coverage.len(),1);assert_eq!(coverage[0]["type"],"coverage");commit(&mut f.bank,&coverage,20200);assert_eq!(f.source.scopes()[0]["blocked"],true);
 let raw=receipts();let facts=RefreshFacts::new(&scope(),&f.hint,raw.clone(),ms(27000)).unwrap();let events=f.source.accept(&wrapped(&raw[6])).unwrap();assert_eq!(events.len(),2);assert_eq!(events[0]["book"]["invalid"],true);assert_ne!(events[1]["book"]["invalid"],true);let admitted=commit(&mut f.bank,&events,27000);assert_eq!(f.source.scopes()[0]["blocked"],false);(facts,admitted)
}
#[test]fn complete_tape_then_real_invalid_valid_books_commit_and_certify_original_new_quote(){
 let mut f=fixture();let(facts,events)=recover(&mut f);let proof=BookAcceptance::from_events(&facts,&events,true,head(&f),ms(27000),ms(27100)).unwrap();
 let q=f.bank.engine().order_records().into_iter().find(|q|q["id"].as_u64().unwrap()>f.cutoff&&q["action"]=="sell"&&q["inventoryExitReason"]=="maximumAge"&&q["retired"]==false).unwrap();let original=q.clone();let candidates=f.bank.engine().order_records();let safe=safety(&f);
 let plan=RefreshPlan::select(&facts,&proof,&candidates,f.cutoff,ms(20000),ms(27100),ms(27100),|_|safe).unwrap();assert_eq!(plan.quote(),&q);assert_eq!(plan.stage(&q,&facts,&scope(),safe,ms(27100)).unwrap(),RefreshStage::WaitUntil(ms(30000)));assert_eq!(plan.stage(&q,&facts,&scope(),safe,ms(30000)).unwrap(),RefreshStage::Ready);assert_eq!(q,original);assert_ne!(q["id"],f.old["id"]);assert_eq!(q["expiresAt"],ms(40000));
 // Read the actual sealed journal and report, rather than treating the Boolean
 // helper argument as independent proof that Session committed the source.
 let rows=fs::read_to_string(f.root.join("bank/events.jsonl")).unwrap().lines().map(|s|serde_json::from_str::<Value>(s).unwrap()).collect::<Vec<_>>();let tail=&rows[rows.len()-2..];for(i,row)in tail.iter().enumerate(){let mut actual=row["event"].clone();assert!(actual.as_object_mut().unwrap().remove("sourceRef").is_some());assert_eq!(actual,events[i]);}let report:Value=serde_json::from_slice(&fs::read(f.root.join("bank/report.json")).unwrap()).unwrap();assert_eq!(report["report"],f.bank.engine().report().unwrap());assert_eq!(report["report"]["fills"],1);assert_eq!(report["report"]["realized"],"0.000000");
}
#[test]fn missing_or_unfinished_tape_keeps_real_book_invalid_and_denies_certificate(){for partial in [false,true]{let mut f=fixture();if partial{let output=f.source.accept(&coverage(true)).unwrap();assert!(output.is_empty());}let raw=receipts();let facts=RefreshFacts::new(&scope(),&f.hint,raw.clone(),ms(27000)).unwrap();let events=f.source.accept(&wrapped(&raw[6])).unwrap();assert_eq!(events.len(),1);assert_eq!(events[0]["book"]["invalid"],true);let actual=commit(&mut f.bank,&events,27000);assert!(BookAcceptance::from_events(&facts,&actual,true,head(&f),ms(27000),ms(27100)).is_err());assert_eq!(f.source.scopes()[0]["blocked"],true);assert_eq!(f.bank.engine().report().unwrap()["fills"],1);}}
#[test]fn actual_later_source_gap_invalidates_previously_valid_committed_book(){let mut f=fixture();let(facts,mut events)=recover(&mut f);let gap=f.source.subscribed_market("1",&at(27200)).unwrap();assert!(gap.iter().any(|e|e["type"]=="gap"&&e["exchangeId"]=="2"));events.extend(commit(&mut f.bank,&gap,27200));assert!(BookAcceptance::from_events(&facts,&events,true,head(&f),ms(27000),ms(27300)).is_err());assert_eq!(f.source.scopes()[0]["blocked"],true);}
#[test]fn certificate_does_not_promote_uncommitted_events_or_ambiguous_actual_valid_emissions(){let mut f=fixture();let(facts,mut events)=recover(&mut f);assert!(BookAcceptance::from_events(&facts,&events,false,head(&f),ms(27000),ms(27100)).is_err());let raw=receipts();let repeated=f.source.accept(&wrapped(&raw[6])).unwrap();assert_eq!(repeated.len(),1);assert_ne!(repeated[0]["book"]["invalid"],true);events.extend(commit(&mut f.bank,&repeated,27200));assert!(BookAcceptance::from_events(&facts,&events,true,head(&f),ms(27000),ms(27300)).is_err());}

fn deny_initial_uncovered_book(f:&mut Fixture){
 let raw=receipts();let facts=RefreshFacts::new(&scope(),&f.hint,raw.clone(),ms(27000)).unwrap();let output=f.source.accept(&wrapped(&raw[6])).unwrap();assert_eq!(output.len(),1);assert_eq!(output[0]["book"]["invalid"],true);let admitted=commit(&mut f.bank,&output,27000);
 assert!(BookAcceptance::from_events(&facts,&admitted,true,head(f),ms(27000),ms(27100)).is_err());assert_eq!(f.source.scopes()[0]["blocked"],true);assert_eq!(f.source.scopes()[0]["bookReady"],true);assert_eq!(f.source.scopes()[0]["tapeReady"],false);
}
fn repair_window(earliest:u64,unfinished:bool,miss_boundary:bool)->Value{
 let from=earliest-ms(0)+if miss_boundary{2000}else{0};json!({"kind":"trades","source":"official_sig_api_v1","status":200,"marketId":"1","exchangeId":"2","tournamentId":TOURNAMENT,"requestStartedAt":at(28000),"receivedAt":at(28500),"endpoint":format!("/exchanges/2/trades?tournamentId={TOURNAMENT}&from={}&to={}",at(from),at(28000)),"data":{"marketId":"1","exchangeId":"2","from":at(from),"to":at(28000),"data":[],"pagination":{"hasMore":unfinished,"nextCursor":if unfinished{json!("remaining-page")}else{Value::Null}},"coverage":{"complete":true,"projectedThroughSequence":101}}})
}
fn later_facts(f:&Fixture)->(RefreshFacts,[Receipt;7]){
 let mut raw=receipts();for(i,r)in raw.iter_mut().enumerate(){r.request_started_at=at(29000+i as u64*1000);r.received_at=at(29500+i as u64*1000);}
 raw[6].data["contexts"][0]["orderbook"]["exchanges"][0]["asOf"]=json!({"sequence":101,"at":at(35000)});
 let facts=RefreshFacts::new(&scope(),&f.hint,raw.clone(),ms(36000)).unwrap();(facts,raw)
}
#[test]fn same_session_denial_then_exact_tape_repair_then_later_seven_facts_reaches_ready(){
 let mut f=fixture();deny_initial_uncovered_book(&mut f);let flow_cutoff=f.bank.engine().order_records().iter().map(|q|q["id"].as_u64().unwrap()).max().unwrap();let earliest=f.source.recovery_start_ms("1").unwrap();assert_eq!(earliest,ms(14999));
 let window=repair_window(earliest,false,false);let recovery=f.source.accept(&window).unwrap();assert_eq!(recovery.len(),2);assert_eq!(recovery[0]["type"],"coverage");assert_eq!(recovery[0]["from"],at(14999));assert_eq!(recovery[0]["to"],at(28000));assert_eq!(recovery[1]["type"],"book");assert_ne!(recovery[1]["book"]["invalid"],true);let recovered=commit(&mut f.bank,&recovery,28600);
 assert_eq!(f.source.scopes()[0]["tapeReady"],true);assert_eq!(f.source.scopes()[0]["blocked"],false);assert!(f.source.recovery_start_ms("1").is_none());
 // Coverage can legitimately release a valid cached book and create the new
 // successor BEFORE the seven snapshots. Its original deadline must survive.
 let original=f.bank.engine().order_records().into_iter().find(|q|q["id"].as_u64().unwrap()>flow_cutoff&&q["action"]=="sell"&&q["inventoryExitReason"]=="maximumAge"&&q["retired"]==false).unwrap();assert_eq!(original["createdAt"],ms(28600));assert_eq!(original["activationAt"],ms(31600));assert_eq!(original["expiresAt"],ms(41600));
 let(facts,raw)=later_facts(&f);assert!(BookAcceptance::from_events(&facts,&recovered,true,head(&f),ms(28600),ms(28700)).is_err());
 let output=f.source.accept(&wrapped(&raw[6])).unwrap();assert_eq!(output.len(),1);assert_ne!(output[0]["book"]["invalid"],true);let admitted=commit(&mut f.bank,&output,36000);let proof=BookAcceptance::from_events(&facts,&admitted,true,head(&f),ms(36000),ms(36100)).unwrap();
 let candidates=f.bank.engine().order_records();let current=candidates.iter().find(|q|q["id"]==original["id"]).unwrap();assert_eq!(current,&original);let safe=safety(&f);let plan=RefreshPlan::select(&facts,&proof,&candidates,flow_cutoff,ms(27500),ms(36100),ms(36100),|_|safe).unwrap();assert_eq!(plan.quote(),&original);assert_eq!(plan.stage(current,&facts,&scope(),safe,ms(36100)).unwrap(),RefreshStage::Ready);assert_ne!(plan.quote()["id"],f.old["id"]);assert_eq!(f.bank.engine().report().unwrap()["fills"],1);assert_eq!(f.bank.engine().report().unwrap()["realized"],"0.000000");
}
#[test]fn causal_repair_with_partial_pages_or_missing_boundary_cannot_reach_ready(){
 for(unfinished,miss_boundary)in [(true,false),(false,true)]{let mut f=fixture();deny_initial_uncovered_book(&mut f);let earliest=f.source.recovery_start_ms("1").unwrap();let output=f.source.accept(&repair_window(earliest,unfinished,miss_boundary)).unwrap();let admitted=commit(&mut f.bank,&output,28600);assert!(admitted.iter().all(|e|e["type"]!="book"));assert_eq!(f.source.scopes()[0]["tapeReady"],false);assert_eq!(f.source.scopes()[0]["blocked"],true);assert_eq!(f.source.recovery_start_ms("1"),Some(earliest));
  let(facts,raw)=later_facts(&f);let invalid=f.source.accept(&wrapped(&raw[6])).unwrap();assert_eq!(invalid.len(),1);assert_eq!(invalid[0]["book"]["invalid"],true);let committed=commit(&mut f.bank,&invalid,36000);assert!(BookAcceptance::from_events(&facts,&committed,true,head(&f),ms(36000),ms(36100)).is_err());assert_eq!(f.bank.engine().report().unwrap()["fills"],1);assert_eq!(f.bank.engine().wallet.quantity("2",Side::Yes),1);
 }
}

// Genuine inherited lots have no new-entry loss timer. Their profitable AON
// successors must be recomputed by the native planner, never relabeled age exits.
fn profit_session(side:Side)->(PathBuf,Session,Value){
 static N:AtomicU64=AtomicU64::new(0);
 let root=std::env::temp_dir().join(format!("profit-refresh-{}-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),N.fetch_add(1,Ordering::SeqCst)));
 let policy=json!({"quantity":5,"maximumMarkets":3,"entriesEnabled":false,"maximumInventoryAgeMs":60000,"inventoryAgeClock":"execution","inventoryTakerAfterMs":0,"inventoryMinimumGain":"0.005000","inventoryExitMode":"fixedGain","inventoryExitDepthBps":5000});
 let seed=json!({"runId":"profit-refresh","tournamentId":TOURNAMENT,"availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":at(0)});
 let mut bank=Session::open_live(&root,&policy,&seed).unwrap();let no=side==Side::No;let label=if no{"NO"}else{"YES"};let price=if no{"0.500000"}else{"0.200000"};let principal=if no{"2.500000"}else{"1.000000"};
 commit(&mut bank,&[json!({"type":"actual_position_adoption","source":"official_sig_api_v1","authority":"human_moloch_inherited_management_20261015","complete":true,"receiptHash":"d".repeat(64),"tournamentId":TOURNAMENT,"receivedAt":at(0),"requestStartedAt":at(0),"openingCost":principal,"effectiveMaximumMarkets":4,"positions":[{"marketId":"1","exchangeId":"2","quantity":if no{-5}else{5},"lotPrincipal":principal,"reportedCostBasis":principal,"reportedMinusLotPrincipal":"0.000000","basisClassification":"exact","lots":[{"lotId":"901","quantity":5,"side":label,"entryPrice":price,"openedAt":at(0)}]}]})],0);
 let r=receipts()[6].clone();let mut e=moloch_replay::source::receipt_events(&wrapped(&r)).unwrap().remove(0);e["book"]["sequence"]=json!("1");for k in ["sourceAt","requestStartedAt","receivedAt"]{e["book"][k]=json!(at(0))}e["receivedAt"]=json!(at(0));commit(&mut bank,&[e],0);
 let q=bank.engine().order_records().into_iter().find(|q|q["action"]=="sell"&&q["side"]==label&&q["retired"]==false).unwrap();assert_eq!(q["executionStyle"],"inventoryAON");assert!(q.get("inventoryExitReason").is_none());(root,bank,q)
}
fn profit_safety(bank:&Session,side:Side)->Safety{Safety{never_attempted:true,has_intent:false,unresolved_attempts:false,global_unknown:false,stop:false,stream_healthy:true,owned_quantity:bank.engine().wallet.quantity("2",side),reserved_sell_quantity:0,same_exchange_pending_physical_order:false,lifecycle_fingerprint:Sha256::digest(serde_json::to_vec(&bank.engine().report().unwrap()).unwrap()).into(),native_head_fingerprint:{let mut bytes=serde_json::to_vec(&bank.durable_head()).unwrap();bytes.push(b'\n');Sha256::digest(bytes).into()}}}
#[test]fn genuine_yes_and_no_inherited_profit_refresh_requires_new_native_quote_and_original_seven_facts(){
 use moloch_live_runtime::exit_refresh::{ReadKind,current_profit_qualified};
 for side in [Side::Yes,Side::No]{let(root,mut bank,old)=profit_session(side);let safe=profit_safety(&bank,side);
  assert!(ReadHint::from_expired(&old,safe,ms(20000)).is_err(),"legacy age API must not silently accept profit");
  let hint=ReadHint::from_expired_kind(&old,safe,ms(20000),ReadKind::Profit).unwrap();assert_eq!(hint.kind(),ReadKind::Profit);
  let cutoff=bank.engine().order_records().iter().filter_map(|q|q["id"].as_u64()).max().unwrap();
  commit(&mut bank,&[json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","receiptHash":"e".repeat(64),"orderId":old["id"],"confirmedNetworkAttempts":0,"deferPlanningForPackage":true,"receivedAt":at(20000)})],20000);
  let mut raw=receipts();let label=if side==Side::Yes{"YES"}else{"NO"};let price=if side==Side::Yes{0.2}else{0.5};
  raw[0].data["myBalance"]=json!(1000);raw[1].data=json!({"positions":[{"exchangeId":"2","marketId":"1","quantity":if side==Side::No{-5}else{5},"lots":[{"lotId":"901","side":label,"quantity":5,"entryPrice":price,"openedAt":at(0)}]}]});raw[3].data=json!({"balance":1000,"availableAdvance":0,"buyingPower":1000});
  let facts=RefreshFacts::new(&scope(),&hint,raw.clone(),ms(27000)).unwrap();let events=moloch_replay::source::receipt_events(&wrapped(&raw[6])).unwrap();let events=commit(&mut bank,&events,27000);
  let safe=profit_safety(&bank,side);let proof=BookAcceptance::from_events(&facts,&events,true,safe.native_head_fingerprint,ms(27000),ms(27100)).unwrap();
  let orders=bank.engine().order_records();let plan=RefreshPlan::select(&facts,&proof,&orders,cutoff,ms(20000),ms(27100),ms(27100),|_|safe).unwrap();assert!(plan.quote()["id"].as_u64().unwrap()>cutoff);assert!(current_profit_qualified(&bank,plan.quote()).unwrap());assert!(plan.quote().get("inventoryExitReason").is_none());
  assert_eq!(plan.stage(plan.quote(),&facts,&scope(),safe,ms(30000)).unwrap(),RefreshStage::Ready);
  for safety in [Safety{has_intent:true,..safe},Safety{never_attempted:false,..safe},Safety{global_unknown:true,..safe},Safety{same_exchange_pending_physical_order:true,..safe},Safety{owned_quantity:0,..safe}]{assert!(plan.stage(plan.quote(),&facts,&scope(),safety,ms(30000)).is_err());}
  assert!(plan.stage(plan.quote(),&facts,&scope(),safe,ms(36001)).is_err());assert_eq!(bank.engine().wallet.quantity("2",side),5);assert!(bank.engine().fills().is_empty());drop(bank);fs::remove_dir_all(root).unwrap();
 }
}
#[test]fn genuine_deteriorated_book_cannot_turn_inherited_profit_hint_into_a_loss_exit(){
 use moloch_live_runtime::exit_refresh::ReadKind;
 let(root,mut bank,old)=profit_session(Side::No);let hint=ReadHint::from_expired_kind(&old,profit_safety(&bank,Side::No),ms(20000),ReadKind::Profit).unwrap();let cutoff=old["id"].as_u64().unwrap();
 commit(&mut bank,&[json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","receiptHash":"e".repeat(64),"orderId":old["id"],"confirmedNetworkAttempts":0,"deferPlanningForPackage":true,"receivedAt":at(20000)})],20000);
 let mut raw=receipts();let row=&mut raw[6].data["contexts"][0]["orderbook"]["exchanges"][0];row["bids"]=json!([{"price":0.59,"quantity":1000}]);row["asks"]=json!([{"price":0.60,"quantity":1000}]);
 let facts=RefreshFacts::new(&scope(),&hint,raw.clone(),ms(27000)).unwrap();let events=moloch_replay::source::receipt_events(&wrapped(&raw[6])).unwrap();let events=commit(&mut bank,&events,27000);let safe=profit_safety(&bank,Side::No);let proof=BookAcceptance::from_events(&facts,&events,true,safe.native_head_fingerprint,ms(27000),ms(27100)).unwrap();
 assert!(RefreshPlan::select(&facts,&proof,&bank.engine().order_records(),cutoff,ms(20000),ms(27100),ms(27100),|_|safe).is_err());assert_eq!(bank.engine().wallet.quantity("2",Side::No),5);assert!(bank.engine().fills().is_empty());drop(bank);fs::remove_dir_all(root).unwrap();
}

#[test]fn early_target_book_and_real_gap_allow_a_native_profit_successor_before_the_final_packet(){
 use moloch_live_runtime::exit_refresh::{ReadKind,current_profit_qualified};
 fn book(ex:&str,market:&str,t:u64,sequence:u64)->Value{json!({"type":"book","receivedAt":at(t),"book":{"marketId":market,"exchangeId":ex,"tournamentId":TOURNAMENT,"sequence":sequence.to_string(),"sourceAt":at(t),"requestStartedAt":at(t),"receivedAt":at(t),"bids":[{"price":"0.400000","quantity":1000}],"asks":[{"price":"0.420000","quantity":1000}]}})}
 for warm in [false,true]{
  let root=std::env::temp_dir().join(format!("profit-seed-budget-{}-{}-{warm}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
  let policy=json!({"quantity":5,"maximumMarkets":4,"entriesEnabled":false,"maximumInventoryAgeMs":60000,"inventoryAgeClock":"execution","inventoryTakerAfterMs":0,"inventoryMinimumGain":"0.005000","inventoryExitMode":"fixedGain","inventoryExitDepthBps":5000,"writeGapMs":2500,"writesPerMinute":24});
  let seed=json!({"runId":"profit-budget-refresh","tournamentId":TOURNAMENT,"availableCash":"1000.000000","accountSnapshotHash":"a".repeat(64),"observedAt":at(0)});
  let mut bank=Session::open_live(&root,&policy,&seed).unwrap();
  let positions:Vec<_>=[("1","3"),("2","1"),("3","4")].into_iter().enumerate().map(|(i,(ex,market))|json!({"marketId":market,"exchangeId":ex,"quantity":-5,"lotPrincipal":"2.500000","reportedCostBasis":"2.500000","reportedMinusLotPrincipal":"0.000000","basisClassification":"exact","lots":[{"lotId":(901+i).to_string(),"quantity":5,"side":"NO","entryPrice":"0.500000","openedAt":at(0)}]})).collect();
  commit(&mut bank,&[json!({"type":"actual_position_adoption","source":"official_sig_api_v1","authority":"human_moloch_inherited_management_20261015","complete":true,"receiptHash":"d".repeat(64),"tournamentId":TOURNAMENT,"receivedAt":at(0),"requestStartedAt":at(0),"openingCost":"7.500000","effectiveMaximumMarkets":7,"positions":positions})],0);
  commit(&mut bank,&[book("2","1",0,1)],0);
  let old=bank.engine().order_records().into_iter().find(|q|q["exchangeId"]=="2"&&q["action"]=="sell").unwrap();
  commit(&mut bank,&[json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","receiptHash":"e".repeat(64),"orderId":old["id"],"confirmedNetworkAttempts":0,"deferPlanningForPackage":true,"receivedAt":at(20000)})],20000);
  // A genuine other holding consumes the unchanged global write gap.
  commit(&mut bank,&[book("1","3",25000,1)],25000);
  let cutoff=bank.engine().order_records().iter().filter_map(|q|q["id"].as_u64()).max().unwrap();
  let hint=ReadHint::from_expired_kind(&old,profit_safety(&bank,Side::No),ms(26000),ReadKind::Profit).unwrap();
  if warm{
   commit(&mut bank,&[book("2","1",27000,2)],27000);
   assert!(!bank.engine().order_records().iter().any(|q|q["exchangeId"]=="2"&&q["id"].as_u64().unwrap()>cutoff),"real native gap still denies the early book");
   commit(&mut bank,&[json!({"type":"tick","receivedAt":at(27499)})],27499);
   assert!(!bank.engine().order_records().iter().any(|q|q["exchangeId"]=="2"&&q["id"].as_u64().unwrap()>cutoff),"one millisecond before the real gap is not permission");
   commit(&mut bank,&[json!({"type":"tick","receivedAt":at(29500)})],29500);
  }
  // Another actual native write shortly before the final book prevents a late
  // successor, reproducing the observed NO_FRESH_CANDIDATE path without seeding.
  commit(&mut bank,&[book("3","4",35000,1)],35000);
  let mut raw=receipts();for(i,r)in raw.iter_mut().enumerate(){r.request_started_at=at(30000+i as u64*1000);r.received_at=at(30100+i as u64*1000);}
  raw[0].data["myBalance"]=json!(1000);raw[1].data=json!({"positions":[{"exchangeId":"2","marketId":"1","quantity":-5,"lots":[{"lotId":"902","side":"NO","quantity":5,"entryPrice":0.5,"openedAt":at(0)}]}]});raw[3].data=json!({"balance":1000,"availableAdvance":0,"buyingPower":1000});
  raw[6].data["contexts"][0]["orderbook"]["exchanges"][0]["asOf"]["at"]=json!(at(36000));raw[6].data["contexts"][0]["orderbook"]["exchanges"][0]["asOf"]["sequence"]=json!(3);
  let facts=RefreshFacts::new(&scope(),&hint,raw.clone(),ms(36200)).unwrap();let events=moloch_replay::source::receipt_events(&wrapped(&raw[6])).unwrap();let events=commit(&mut bank,&events,36100);
  let safe=profit_safety(&bank,Side::No);let proof=BookAcceptance::from_events(&facts,&events,true,safe.native_head_fingerprint,ms(36100),ms(36200)).unwrap();let quotes=bank.engine().order_records();
  let plan=RefreshPlan::select(&facts,&proof,&quotes,cutoff,ms(26000),ms(36200),ms(36200),|_|safe);
  if warm{let plan=plan.unwrap();assert!(current_profit_qualified(&bank,plan.quote()).unwrap());assert_eq!(plan.quote()["createdAt"],ms(29500));assert_ne!(plan.quote()["id"],old["id"]);assert_eq!(plan.stage(plan.quote(),&facts,&scope(),safe,ms(36200)).unwrap(),RefreshStage::Ready);}
  else{assert_eq!(plan.err().unwrap().to_string(),"EXIT_REFRESH_NO_FRESH_CANDIDATE");}
  assert!(bank.engine().fills().is_empty());assert_eq!(bank.engine().wallet.quantity("2",Side::No),5);drop(bank);fs::remove_dir_all(root).unwrap();
 }
}

// The same real seven receipts are admitted before one unused live draft is
// retired normally. The retirement creates a new native quote, not a renewed old
// qid. Virtual elapsed clocks model the original three-second activation only.
fn post_book_profit(side:Side)->(PathBuf,Session,RefreshFacts,BookAcceptance,Value,u64){
 let(root,mut bank,old)=profit_session(side);
 let hint=ReadHint::from_live_profit(&old,profit_safety(&bank,side),ms(1000)).unwrap();
 let mut raw=receipts();for(i,r)in raw.iter_mut().enumerate(){r.request_started_at=at(2000+i as u64*1000);r.received_at=at(2100+i as u64*1000);}
 let label=if side==Side::Yes{"YES"}else{"NO"};let price=if side==Side::Yes{0.2}else{0.5};
 raw[0].data["myBalance"]=json!(1000);raw[1].data=json!({"positions":[{"exchangeId":"2","marketId":"1","quantity":if side==Side::No{-5}else{5},"lots":[{"lotId":"901","side":label,"quantity":5,"entryPrice":price,"openedAt":at(0)}]}]});raw[3].data=json!({"balance":1000,"availableAdvance":0,"buyingPower":1000});
 raw[6].data["contexts"][0]["orderbook"]["exchanges"][0]["asOf"]["at"]=json!(at(8000));
 let facts=RefreshFacts::new(&scope(),&hint,raw.clone(),ms(8200)).unwrap();
 let output=moloch_replay::source::receipt_events(&wrapped(&raw[6])).unwrap();let events=commit(&mut bank,&output,8200);
 let safe=profit_safety(&bank,side);let proof=BookAcceptance::from_events(&facts,&events,true,safe.native_head_fingerprint,ms(8200),ms(8300)).unwrap();
 let cutoff=bank.engine().order_records().iter().filter_map(|q|q["id"].as_u64()).max().unwrap();
 assert_eq!(bank.engine().order_records().iter().find(|q|q["id"]==old["id"]).unwrap(),&old);
 (root,bank,facts,proof,old,cutoff)
}
fn retire_live_profit(bank:&mut Session,old:&Value){
 commit(bank,&[json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher","receiptHash":"e".repeat(64),"orderId":old["id"],"confirmedNetworkAttempts":0,"receivedAt":at(9000)})],9000);
}
#[test]fn actual_post_book_retirement_reconcile_and_three_second_activation_yes_and_no(){
 for side in [Side::Yes,Side::No]{
  let(root,mut bank,facts,acceptance,old,cutoff)=post_book_profit(side);
  assert!(RefreshPlan::select(&facts,&acceptance,&bank.engine().order_records(),cutoff,ms(1000),ms(8300),ms(8300),|_|profit_safety(&bank,side)).is_err());
  retire_live_profit(&mut bank,&old);let safe=profit_safety(&bank,side);
  let actual_head:[u8;32]=Sha256::digest(fs::read(root.join("head.json")).unwrap()).into();assert_eq!(safe.native_head_fingerprint,actual_head);
  let binding=PostBookPlanning::from_session(&facts,&acceptance,&bank,ms(9000),ms(9100),safe.native_head_fingerprint).unwrap();
  assert_eq!(binding.book_acceptance_completed_ms(),ms(8300));assert_eq!(binding.planning_started_ms(),ms(9000));assert_eq!(binding.planning_completed_ms(),ms(9100));assert_eq!(binding.native_head(),&bank.durable_head());
  let plan=RefreshPlan::select_after_planning(&facts,&binding,&bank,cutoff,ms(9100),|_|safe).unwrap();
  assert!(plan.quote()["id"].as_u64().unwrap()>cutoff);assert_ne!(plan.quote()["id"],old["id"]);assert_eq!(plan.quote()["createdAt"],ms(9000));assert_eq!(plan.quote()["activationAt"],ms(12000));assert_eq!(plan.quote()["expiresAt"],ms(22000));
  assert_eq!(plan.stage(plan.quote(),&facts,&scope(),safe,ms(9100)).unwrap(),RefreshStage::WaitUntil(ms(12000)));
  assert_eq!(plan.stage(plan.quote(),&facts,&scope(),safe,ms(12000)).unwrap(),RefreshStage::Ready);
  assert!(plan.stage(plan.quote(),&facts,&scope(),safe,ms(18001)).is_err());
  assert!(RefreshPlan::select_after_planning(&facts,&binding,&bank,plan.quote()["id"].as_u64().unwrap(),ms(9100),|_|safe).is_err());
  for denied in [Safety{has_intent:true,..safe},Safety{never_attempted:false,..safe},Safety{unresolved_attempts:true,..safe},Safety{global_unknown:true,..safe},Safety{same_exchange_pending_physical_order:true,..safe},Safety{reserved_sell_quantity:5,..safe}]{assert!(RefreshPlan::select_after_planning(&facts,&binding,&bank,cutoff,ms(9100),|_|denied).is_err());}
  assert!(PostBookPlanning::from_session(&facts,&acceptance,&bank,ms(9000),ms(9100),[42;32]).is_err());
  let before=bank.durable_head();let owned=bank.engine().wallet.quantity("2",side);let fills=bank.engine().fills();
  assert!(RefreshPlan::select_after_planning(&facts,&binding,&bank,cutoff,ms(9100),|_|safe).is_ok());assert_eq!(bank.durable_head(),before);assert_eq!(bank.engine().wallet.quantity("2",side),owned);assert_eq!(bank.engine().fills(),fills);
  commit(&mut bank,&[json!({"type":"tick","receivedAt":at(9200)})],9200);
  assert!(RefreshPlan::select_after_planning(&facts,&binding,&bank,cutoff,ms(9200),|_|profit_safety(&bank,side)).is_err());
  drop(bank);fs::remove_dir_all(root).unwrap();
 }
}
#[test]fn post_book_binding_rejects_changed_or_gapped_actual_book_and_stale_original_facts(){
 for gap in [false,true]{let(root,mut bank,facts,acceptance,old,cutoff)=post_book_profit(Side::No);retire_live_profit(&mut bank,&old);
  let safe=profit_safety(&bank,Side::No);assert!(PostBookPlanning::from_session(&facts,&acceptance,&bank,ms(9000),ms(18001),safe.native_head_fingerprint).is_err());
  let event=if gap{json!({"type":"gap","exchangeId":"2","receivedAt":at(9200)})}else{let mut e=moloch_replay::source::receipt_events(&wrapped(facts.book())).unwrap().remove(0);e["book"]["sequence"]=json!("101");e["book"]["bids"][0]["quantity"]=json!(999);e};
  commit(&mut bank,&[event],9200);let safe=profit_safety(&bank,Side::No);
  assert!(PostBookPlanning::from_session(&facts,&acceptance,&bank,ms(9000),ms(9300),safe.native_head_fingerprint).is_err());
  let _=cutoff;drop(bank);fs::remove_dir_all(root).unwrap();
 }
}
#[test]fn live_profit_read_hint_preserves_original_window_and_never_accepts_partial_or_age_exit(){
 let(root,bank,old)=profit_session(Side::No);let safe=profit_safety(&bank,Side::No);
 assert!(ReadHint::from_live_profit(&old,safe,ms(1000)).is_ok());assert!(ReadHint::from_live_profit(&old,safe,ms(13000)).is_err());assert!(ReadHint::from_live_profit(&old,safe,ms(0)-1).is_err());
 for key in ["filled","retired","inventoryExitReason","cancelEffectiveAt"]{let mut q=old.clone();q[key]=match key{"filled"=>json!(1),"retired"=>json!(true),"inventoryExitReason"=>json!("maximumAge"),_=>json!(ms(500))};assert!(ReadHint::from_live_profit(&q,safe,ms(1000)).is_err());}
 assert_eq!(bank.engine().order_records().iter().find(|q|q["id"]==old["id"]).unwrap(),&old);drop(bank);fs::remove_dir_all(root).unwrap();
}

#[test]fn post_book_profit_selection_rejects_actual_ack_binding_and_partial_fill(){
 let(root,mut bank,facts,acceptance,old,cutoff)=post_book_profit(Side::No);retire_live_profit(&mut bank,&old);
 let safe=profit_safety(&bank,Side::No);let binding=PostBookPlanning::from_session(&facts,&acceptance,&bank,ms(9000),ms(9100),safe.native_head_fingerprint).unwrap();
 let q=RefreshPlan::select_after_planning(&facts,&binding,&bank,cutoff,ms(9100),|_|safe).unwrap().quote().clone();
 let actual=|kind:&str,t:u64|json!({"type":kind,"source":"official_sig_api_v1","receiptHash":"b".repeat(64),"orderId":q["id"],"actualOrderId":"201","exchangeId":"2","marketId":"1","side":"NO","action":"sell","quantity":q["quantity"],"price":q["price"],"receivedAt":at(t)});
 commit(&mut bank,&[actual("actual_order_ack",12000)],12000);
 let safe=profit_safety(&bank,Side::No);let binding=PostBookPlanning::from_session(&facts,&acceptance,&bank,ms(9000),ms(12100),safe.native_head_fingerprint).unwrap();
 assert!(RefreshPlan::select_after_planning(&facts,&binding,&bank,cutoff,ms(12100),|_|safe).is_err(),"native binding cannot be overridden by caller never_attempted");
 let mut fill=actual("actual_fill",12200);fill["quantity"]=json!(1);fill["fee"]=json!("0.000000");fill["executedAt"]=json!(at(12100));fill["fillId"]=json!("301");commit(&mut bank,&[fill],12200);
 let safe=profit_safety(&bank,Side::No);let binding=PostBookPlanning::from_session(&facts,&acceptance,&bank,ms(9000),ms(12300),safe.native_head_fingerprint).unwrap();
 assert!(RefreshPlan::select_after_planning(&facts,&binding,&bank,cutoff,ms(12300),|_|safe).is_err());assert_eq!(bank.engine().wallet.quantity("2",Side::No),4);assert_eq!(bank.engine().fills().len(),1);
 drop(bank);fs::remove_dir_all(root).unwrap();
}
#[test]fn post_book_profit_phase_refuses_actual_managed_target_even_with_caller_safe_flags(){
 let(root,mut bank,facts,acceptance,old,_)=post_book_profit(Side::No);retire_live_profit(&mut bank,&old);
 let event=json!({"type":"managed_inventory_planning_v1","source":"moloch_live_dispatcher","receivedAt":at(9200),"bankScope":{"runId":"profit-refresh","tournamentId":TOURNAMENT,"bankManifestHash":bank.manifest_hash()},"controlSha256":"a".repeat(64),"approvedReviewSetSha256":"b".repeat(64),"generationSha256":"c".repeat(64),"managedExchangeIds":["2"],"expectedPreviousModeSha256":null});
 commit(&mut bank,&[event],9200);let safe=profit_safety(&bank,Side::No);
 assert!(PostBookPlanning::from_session(&facts,&acceptance,&bank,ms(9000),ms(9300),safe.native_head_fingerprint).is_err());
 assert_eq!(bank.engine().wallet.quantity("2",Side::No),5);assert!(bank.engine().fills().is_empty());drop(bank);fs::remove_dir_all(root).unwrap();
}
