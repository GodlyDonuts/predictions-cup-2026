//! The flag only changes planning after a proved, local, unsubmitted retirement.
//! No network/client, inferred fill, persistent suppression or economic claim.
use moloch_replay::{engine::Engine, session::Session, money};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(1_792_022_400_000 + ms)
        .unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn hash(v: &Value) -> String { format!("{:x}", Sha256::digest(serde_json::to_vec(v).unwrap())) }
fn policy() -> Value {
    json!({"initialCash":"1000.000000","maximumCommitted":"100.000000",
        "maximumMarketCost":"50.000000","quantity":12,"maximumMarkets":3,
        "entryStopAt":iso(60_000)})
}
fn book(index: usize) -> Value {
    let ex = (101 + index).to_string(); let at = iso(index as i64 * 2500);
    json!({"type":"book","receivedAt":at,"book":{"tournamentId":"cup",
        "marketId":ex,"exchangeId":ex,"sequence":"1","sourceAt":at,
        "requestStartedAt":at,"receivedAt":at,
        "bids":[{"price":"0.400000","quantity":100}],
        "asks":[{"price":"0.450000","quantity":100}]}})
}
fn live(n: usize) -> Engine {
    let mut e = Engine::new_live(&policy()).unwrap();
    for i in 0..n { e.advance(&book(i)).unwrap(); }
    assert_eq!(e.order_records().len(), n * 2);
    e
}
fn retirement(qid: u64, at: i64, flag: Option<Value>) -> Value {
    let mut e = json!({"type":"unsubmitted_intent_retired","source":"moloch_live_dispatcher",
        "orderId":qid,"confirmedNetworkAttempts":0,"receiptHash":"a".repeat(64),
        "receivedAt":iso(at),"processedAt":iso(at)});
    if let Some(v) = flag { e["deferPlanningForPackage"] = v; }
    e
}
fn tick(at: i64) -> Value { json!({"type":"tick","receivedAt":iso(at)}) }
fn unchanged_after_rejection(e: &mut Engine, event: &Value) {
    let report = e.report().unwrap(); let orders = e.order_records();
    assert!(e.advance(event).is_err(), "accepted {event}");
    assert_eq!(e.report().unwrap(), report); assert_eq!(e.order_records(), orders);
}
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("package-planning-{}-{}",
            std::process::id(), std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())))
    }
}
impl Drop for Temp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
fn seed() -> Value { json!({"runId":"package-planning-fixture",
    "availableCash":"1000.000000","accountSnapshotHash":"b".repeat(64),"observedAt":iso(0)}) }

#[test]
fn generated_component_quotes_retire_then_reserve_without_intervening_planning() {
    for n in [2, 3] {
        let temp = Temp::new(); let p = policy(); let seed = seed();
        let mut bank = Session::open_live(&temp.0, &p, &seed).unwrap();
        for i in 0..n { bank.accept("official-books", i as u64 + 1, &book(i)).unwrap(); }
        let original = bank.engine().order_records(); assert_eq!(original.len(), n * 2);
        let at = n as i64 * 2500;
        for (i, q) in original.iter().enumerate() {
            bank.accept("proved-never-sent", i as u64 + 1,
                &retirement(q["id"].as_u64().unwrap(), at, Some(json!(true)))).unwrap();
            assert_eq!(bank.engine().order_records().len(), original.len());
        }
        assert!(bank.engine().order_records().iter().all(|q| q["retired"] == true));
        assert_eq!(bank.engine().report().unwrap()["reservedCash"], "0.000000");
        assert_eq!(bank.engine().report().unwrap()["cash"], "1000.000000");
        let cost = 900_000 * n as i128; let payout = 1_000_000 * n as i128;
        let reserve = json!({"type":"live_package_reserve","source":"moloch_live_dispatcher",
            "receivedAt":iso(at),"processedAt":iso(at),"packageId":"planning-package",
            "proofHash":"c".repeat(64),"bankScope":{"runId":seed["runId"],
                "bankManifestHash":bank.manifest_hash(),"tournamentId":"cup"},
            "expiresAt":iso(at + 9000),"economics":{"maximumCost":money(cost),
                "minimumPayout":money(payout),"minimumSurplus":money(payout-cost),
                "feeTotal":"0.000000"},"legs":(0..n).map(|i|{
                    let b=book(i);json!({"marketId":b["book"]["marketId"],
                        "exchangeId":b["book"]["exchangeId"],"side":"YES","quantity":2,
                        "limitPrice":"0.450000","bookHash":hash(&b["book"])})
                }).collect::<Vec<_>>()});
        bank.accept("package-owner", 1, &reserve).unwrap();
        assert_eq!(bank.engine().report().unwrap()["reservedCash"], money(cost));
        assert_eq!(bank.engine().report().unwrap()["realized"], "0.000000");
        assert!(bank.engine().fills().is_empty());
        bank.accept("ordinary-ticks", 1, &tick(at+1)).unwrap();
        assert_eq!(bank.engine().order_records().len(), n*3);
        for q in bank.engine().order_records().iter().filter(|q| q["retired"] == false) {
            assert!(bank.engine().is_package_order(q["id"].as_u64().unwrap()));
        }
        // The optional flag can never abandon one of these package reservations.
        let q=bank.engine().order_records().into_iter().find(|q|q["retired"]==false).unwrap();
        let before=bank.engine().report().unwrap();
        assert!(bank.accept("proved-never-sent", original.len() as u64 + 1,
            &retirement(q["id"].as_u64().unwrap(),at+2,Some(json!(true)))).is_err());
        assert_eq!(bank.engine().report().unwrap(),before);
        let expected=bank.engine().report().unwrap(); let orders=bank.engine().order_records(); drop(bank);
        let bank=Session::open_live(&temp.0,&p,&seed).unwrap();
        assert_eq!(bank.engine().report().unwrap(),expected);
        assert_eq!(bank.engine().order_records(),orders);
    }
}

#[test]
fn no_candidate_or_restart_leaves_next_tick_free_to_plan_normally() {
    let temp=Temp::new();let p=policy();let seed=seed();
    let mut bank=Session::open_live(&temp.0,&p,&seed).unwrap();
    for i in 0..2 {bank.accept("official-books",i as u64+1,&book(i)).unwrap();}
    let original=bank.engine().order_records();
    for(i,q)in original.iter().enumerate(){bank.accept("proved-never-sent",i as u64+1,
        &retirement(q["id"].as_u64().unwrap(),5000,Some(json!(true)))).unwrap();}
    let expected=bank.engine().report().unwrap();drop(bank);
    let mut bank=Session::open_live(&temp.0,&p,&seed).unwrap();
    assert_eq!(bank.engine().report().unwrap(),expected);
    assert!(bank.engine().order_records().iter().all(|q|q["retired"]==true));
    bank.accept("ordinary-ticks",1,&tick(5001)).unwrap();
    assert_eq!(bank.engine().order_records().iter().filter(|q|q["retired"]==false).count(),4);
    assert_eq!(bank.engine().order_records().len(),8);
    assert_ne!(bank.engine().report().unwrap()["reservedCash"],"0.000000");
    assert!(bank.engine().fills().is_empty());
}

#[test]
fn absent_and_false_flags_keep_identical_ordinary_replacement_behavior() {
    let mut absent=live(1);let mut explicit_false=absent.clone();
    let qid=absent.order_records()[0]["id"].as_u64().unwrap();
    absent.advance(&retirement(qid,2500,None)).unwrap();
    explicit_false.advance(&retirement(qid,2500,Some(json!(false)))).unwrap();
    assert_eq!(absent.report().unwrap(),explicit_false.report().unwrap());
    assert_eq!(absent.order_records(),explicit_false.order_records());
    assert!(absent.order_records().len()>2,"old path must still replan immediately");
    assert!(absent.order_records().iter().any(|q|q["id"]!=qid&&q["retired"]==false));
}

#[test]
fn malformed_or_wrong_event_flag_is_rejected_transactionally() {
    for flag in [Value::Null,json!(0),json!("true"),json!({}),json!([])] {
        let mut e=live(1);let qid=e.order_records()[0]["id"].as_u64().unwrap();
        unchanged_after_rejection(&mut e,&retirement(qid,2500,Some(flag)));
    }
    for flag in [false,true] {
        let mut e=live(1);let mut event=tick(2500);event["deferPlanningForPackage"]=json!(flag);
        unchanged_after_rejection(&mut e,&event);
    }
}

#[test]
fn deferral_cannot_bypass_any_original_unsubmitted_proof_or_retirement_guard() {
    for case in ["source","attempt","hash","unknown_qid","already_retired"] {
        let mut e=live(1);let qid=e.order_records()[0]["id"].as_u64().unwrap();
        let mut event=retirement(qid,2500,Some(json!(true)));
        match case {"source"=>event["source"]=json!("untrusted"),
            "attempt"=>event["confirmedNetworkAttempts"]=json!(1),
            "hash"=>event["receiptHash"]=json!("invalid"),
            "unknown_qid"=>event["orderId"]=json!(999),
            _=>{e.advance(&event).unwrap();}}
        unchanged_after_rejection(&mut e,&event);
    }
}

#[test]
fn actual_acknowledgments_and_fills_stay_owned_and_cannot_be_retired_with_flag() {
    for filled in [false,true] {
        let mut e=live(1);let q=e.order_records().into_iter().find(|q|q["side"]=="YES").unwrap();
        let ack=json!({"type":"actual_order_ack","source":"official_sig_api_v1",
            "receiptHash":"d".repeat(64),"orderId":q["id"],"actualOrderId":"901",
            "exchangeId":q["exchangeId"],"marketId":q["marketId"],"side":q["side"],
            "action":q["action"],"quantity":q["quantity"],"price":q["price"],"receivedAt":iso(1)});
        e.advance(&ack).unwrap();
        if filled {let mut f=ack;f["type"]=json!("actual_fill");f["fillId"]=json!("801");
            f["quantity"]=json!(1);f["fee"]=json!("0.000000");
            f["executedAt"]=json!(iso(3001));f["receivedAt"]=json!(iso(3001));e.advance(&f).unwrap();}
        unchanged_after_rejection(&mut e,&retirement(q["id"].as_u64().unwrap(),5000,Some(json!(true))));
        assert_eq!(e.live_order_bindings().get(&q["id"].as_u64().unwrap()).map(String::as_str),Some("901"));
        assert_eq!(e.fills().len(),usize::from(filled));
    }
}
