// Parity tests ported from paper/*.test.mjs (engine.test.mjs, invariants.test.mjs,
// split.test.mjs, cli.test.mjs).
use baal_paper::engine::{replay, PaperEngine};
use baal_paper::fixed::{big_amount, big_fee, big_price, big_sequence, money};
use baal_paper::schema::{validate_config, validate_event};
use baal_paper::split::chronological_split;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};

const BASE: i64 = 1_767_225_600_000; // Date.UTC(2026,0,1)
fn timestamp(ms: i64) -> String {
    baal_service_runtime::contract::ms_to_iso((BASE + ms) as f64)
}
fn merge(mut base: Map<String, V>, over: &V) -> V {
    if let V::Object(o) = over {
        for (k, v) in o {
            base.insert(k.clone(), v.clone());
        }
    }
    V::Object(base)
}
fn config(overrides: V) -> V {
    merge(
        json!({
            "schemaVersion": 1,
            "initialCash": "100.000000",
            "maxDataAgeMs": 10000,
            "orderLatencyMs": 0,
            "cancelLatencyMs": 50,
            "maxPositionShares": 100,
            "costs": {"feeBps": 0, "feePerShare": "0", "assumption": "unverified_scenario"},
        })
        .as_object()
        .unwrap()
        .clone(),
        &overrides,
    )
}
fn event(ty: &str, at: i64, fields: V) -> V {
    merge(
        json!({
            "schemaVersion": 1,
            "eventId": format!("{ty}-{at}"),
            "type": ty,
            "occurredAt": timestamp(at),
            "receivedAt": timestamp(at),
        })
        .as_object()
        .unwrap()
        .clone(),
        &fields,
    )
}
fn level(price: &str, quantity: i64, own_quantity: i64) -> V {
    json!({"price": price, "quantity": quantity, "ownQuantity": own_quantity})
}
fn snapshot(at: i64, fields: V) -> V {
    event(
        "snapshot",
        at,
        merge(
            json!({
                "instrument": "synthetic:YES", "sequence": "1", "authoritative": true,
                "nextExpiryAt": V::Null,
                "bids": [level("0.400", 10, 0)], "asks": [level("0.600", 10, 0)],
            })
            .as_object()
            .unwrap()
            .clone(),
            &fields,
        ),
    )
}
fn order(at: i64, fields: V) -> V {
    event(
        "submit",
        at,
        merge(
            json!({
                "instrument": "synthetic:YES", "orderId": format!("o-{at}"), "action": "buy",
                "quantity": 5, "limitPrice": "0.400", "expiresAt": V::Null,
            })
            .as_object()
            .unwrap()
            .clone(),
            &fields,
        ),
    )
}
fn trade(at: i64, fields: V) -> V {
    event(
        "trade",
        at,
        merge(
            json!({
                "instrument": "synthetic:YES", "sequence": at.to_string(), "bookSequence": "1",
                "price": "0.400", "quantity": 11, "aggressor": "sell", "evidence": "explicit_trade",
                "externalAggressor": true,
            })
            .as_object()
            .unwrap()
            .clone(),
            &fields,
        ),
    )
}
fn run(events: &[V]) -> V {
    replay(events, &config(json!({}))).unwrap()
}
fn run_cfg(events: &[V], cfg: V) -> V {
    replay(events, &cfg).unwrap()
}
fn oview(r: &V, idx: usize) -> V {
    r["orders"][idx].clone()
}
fn fills_id_qty(r: &V) -> Vec<(String, i64)> {
    r["fills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["orderId"].as_str().unwrap().to_string(), f["quantity"].as_i64().unwrap()))
        .collect()
}

#[test]
fn fixed_point_roundtrips() {
    assert_eq!(
        money(&big_amount("999999999999999999.123456").unwrap()),
        "999999999999999999.123456"
    );
    assert_eq!(money(&big_amount("-0.123").unwrap()), "-0.123000");
    assert_eq!(big_price("0.995").unwrap(), BigInt::from(995_000));
    for x in ["0.4a", "1e-3", "0.1234567", "01.0", "NaN", "Infinity"] {
        assert!(big_amount(x).is_err(), "{x}");
    }
    for x in ["0", "1", "0.004", "0.006"] {
        assert!(big_price(x).is_err(), "{x}");
    }
    assert_eq!(
        big_fee(&BigInt::from(5000), &BigInt::from(1), &json!({"feeBps": 1, "feePerShare": "0"})).unwrap(),
        BigInt::from(1)
    );
}

#[test]
fn snapshot_touch_never_fills() {
    let r = run(&[
        snapshot(0, json!({})),
        order(1, json!({})),
        snapshot(2, json!({"sequence": "2", "bids": [level("0.400", 1, 0)]})),
        event("mark", 3, json!({"instrument": "synthetic:YES", "source": "last", "price": "0.400"})),
    ]);
    assert_eq!(r["fills"].as_array().unwrap().len(), 0);
    assert_eq!(oview(&r, 0)["queueAhead"], "10");
    assert_eq!(r["cash"], "100.000000");
}

#[test]
fn trade_consumes_queue_then_fills() {
    let r = run(&[snapshot(0, json!({})), order(1, json!({})), trade(2, json!({"quantity": 12}))]);
    assert_eq!(r["fills"][0]["quantity"], 2);
    assert_eq!(oview(&r, 0)["remaining"], 3);
    assert_eq!(r["cash"], "99.200000");
    assert_eq!(r["reservedCash"], "1.200000");
    assert_eq!(r["remainingCostBasis"], "0.800000");
}

#[test]
fn same_price_orders_are_fifo() {
    let r = run(&[
        snapshot(0, json!({})),
        order(1, json!({})),
        order(2, json!({})),
        trade(3, json!({"quantity": 16})),
        trade(4, json!({"quantity": 4})),
    ]);
    assert_eq!(fills_id_qty(&r), vec![("o-1".into(), 5), ("o-2".into(), 1), ("o-2".into(), 4)]);
    assert_eq!(r["cash"], "96.000000");
    assert_eq!(r["reservedCash"], "0.000000");
}

#[test]
fn disappearing_depth_fifo() {
    let r = run(&[
        snapshot(0, json!({"bids": [level("0.400", 10, 0)]})),
        order(1, json!({"quantity": 10})),
        snapshot(2, json!({"sequence": "2", "bids": []})),
        order(3, json!({"quantity": 10})),
        order(4, json!({"quantity": 10})),
        trade(5, json!({"bookSequence": "2", "quantity": 15})),
        trade(6, json!({"bookSequence": "2", "quantity": 10})),
    ]);
    assert_eq!(fills_id_qty(&r), vec![("o-1".into(), 5), ("o-1".into(), 5), ("o-3".into(), 5)]);
    let on_trade6: i64 = r["fills"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["eventId"] == "trade-6")
        .map(|f| f["quantity"].as_i64().unwrap())
        .sum();
    assert_eq!(on_trade6, 10);
}

#[test]
fn pending_cancel_quotes_cannot_be_crossed() {
    let r = run(&[
        snapshot(0, json!({"bids": []})),
        order(1, json!({})),
        trade(2, json!({"quantity": 5})),
        order(3, json!({"limitPrice": "0.500"})),
        event("cancel", 4, json!({"orderId": "o-3"})),
        order(5, json!({"action": "sell", "limitPrice": "0.450"})),
    ]);
    assert_eq!(oview(&r, 2)["reason"], "crosses_own_paper_order");
}

#[test]
fn depleted_level_does_not_reject() {
    let r = run(&[
        snapshot(0, json!({"bids": [level("0.400", 1, 0)]})),
        order(1, json!({})),
        trade(2, json!({"quantity": 6})),
        order(3, json!({"action": "sell", "limitPrice": "0.400"})),
    ]);
    assert_eq!(oview(&r, 1)["status"], "open");
}

#[test]
fn own_depth_not_liquidation_liquidity() {
    let r = run(&[
        snapshot(0, json!({"bids": []})),
        order(1, json!({})),
        trade(2, json!({"quantity": 5})),
        snapshot(3, json!({"sequence": "3", "bids": [level("0.450", 5, 4)]})),
    ]);
    assert_eq!(r["positions"][0]["executable"]["soldQuantity"], 1);
    assert_eq!(r["positions"][0]["executable"]["unsoldQuantity"], 4);
    assert_eq!(r["estimatedExecutableEquity"], V::Null);
    assert_eq!(r["positions"][0]["executable"]["fullPositionValue"], V::Null);
}

#[test]
fn own_aggressor_no_self_fill() {
    let r = run(&[snapshot(0, json!({})), order(1, json!({})), trade(2, json!({"externalAggressor": false, "quantity": 100}))]);
    assert_eq!(r["fills"].as_array().unwrap().len(), 0);
    assert_eq!(r["metrics"]["ownAggressorExcluded"], 1);
}

#[test]
fn off_price_prints_not_volume_at_limit() {
    let r = run(&[
        snapshot(0, json!({})),
        order(1, json!({})),
        trade(2, json!({"price": "0.395", "quantity": 100})),
        trade(3, json!({"price": "0.405", "quantity": 100})),
    ]);
    assert_eq!(r["fills"].as_array().unwrap().len(), 0);
}

#[test]
fn latency_blocks_same_time_fills() {
    let r = run_cfg(
        &[
            snapshot(0, json!({"bids": []})),
            order(1, json!({})),
            trade(20, json!({"quantity": 5})),
            trade(51, json!({"quantity": 5})),
            trade(52, json!({"quantity": 3})),
        ],
        config(json!({"orderLatencyMs": 50})),
    );
    assert_eq!(r["fills"].as_array().unwrap().len(), 1);
    assert_eq!(r["fills"][0]["quantity"], 3);
    assert_eq!(r["fills"][0]["eventId"], "trade-52");
}

#[test]
fn cancel_fill_race() {
    let r = run(&[
        snapshot(0, json!({"bids": []})),
        order(1, json!({})),
        event("cancel", 2, json!({"orderId": "o-1"})),
        trade(20, json!({"quantity": 2})),
        trade(52, json!({"quantity": 9})),
        trade(60, json!({"quantity": 9})),
    ]);
    assert_eq!(r["fills"].as_array().unwrap().len(), 1);
    assert_eq!(oview(&r, 0)["status"], "cancelled");
    assert_eq!(oview(&r, 0)["remaining"], 3);
    assert_eq!(r["reservedCash"], "0.000000");
}

#[test]
fn expiry_exclusive_before_same_time_trade() {
    let r = run(&[
        snapshot(0, json!({"bids": []})),
        order(1, json!({"expiresAt": timestamp(3)})),
        trade(2, json!({"quantity": 2})),
        trade(3, json!({"quantity": 3})),
    ]);
    assert_eq!(oview(&r, 0)["status"], "expired");
    assert_eq!(r["fills"].as_array().unwrap().len(), 1);
}

#[test]
fn expiry_while_pending() {
    let r = run_cfg(
        &[snapshot(0, json!({})), order(1, json!({"expiresAt": timestamp(10)})), event("clock", 100, json!({}))],
        config(json!({"orderLatencyMs": 50})),
    );
    assert_eq!(oview(&r, 0)["status"], "expired");
    assert_eq!(r["fills"].as_array().unwrap().len(), 0);
}

#[test]
fn late_pre_cancel_print_fails_closed() {
    let mut engine = PaperEngine::new(&config(json!({}))).unwrap();
    for e in [
        snapshot(0, json!({"bids": []})),
        order(1, json!({})),
        event("cancel", 2, json!({"orderId": "o-1"})),
        event("clock", 60, json!({})),
    ] {
        engine.apply(&e).unwrap();
    }
    let t = trade(61, json!({"occurredAt": timestamp(20), "quantity": 2}));
    assert!(engine.apply(&t).unwrap_err().contains("Ambiguous late"));
    assert!(engine.report().unwrap_err().contains("failed closed"));
    assert!(engine.apply(&event("clock", 70, json!({}))).unwrap_err().contains("failed closed"));
}

#[test]
fn stale_evidence_no_bypass() {
    let r = replay(
        &[
            snapshot(0, json!({"bids": []})),
            order(1, json!({})),
            event("cancel", 2, json!({"orderId": "o-1"})),
            trade(100, json!({"occurredAt": timestamp(20)})),
        ],
        &config(json!({"maxDataAgeMs": 10})),
    );
    assert!(r.unwrap_err().contains("Ambiguous late"));
}

#[test]
fn stale_books_block_orders() {
    let r = run(&[snapshot(0, json!({"nextExpiryAt": timestamp(10)})), order(10, json!({}))]);
    assert_eq!(oview(&r, 0)["reason"], "book_not_fresh_at_submission");
    let r = run_cfg(&[snapshot(0, json!({})), order(11, json!({}))], config(json!({"maxDataAgeMs": 10})));
    assert_eq!(oview(&r, 0)["reason"], "book_not_fresh_at_submission");
    let r = run_cfg(
        &[snapshot(100, json!({"occurredAt": timestamp(0)})), order(101, json!({}))],
        config(json!({"maxDataAgeMs": 10})),
    );
    assert_eq!(oview(&r, 0)["reason"], "book_not_fresh_at_submission");
}

#[test]
fn activation_rechecks_freshness() {
    let r = run_cfg(
        &[snapshot(0, json!({})), order(1, json!({})), event("clock", 100, json!({}))],
        config(json!({"orderLatencyMs": 50, "maxDataAgeMs": 10})),
    );
    assert_eq!(oview(&r, 0)["reason"], "book_not_fresh_at_activation");
}

#[test]
fn gaps_block_until_authoritative() {
    let r = run(&[
        snapshot(0, json!({"bids": []})),
        order(1, json!({})),
        event("gap", 2, json!({"instrument": "synthetic:YES", "reason": "disconnect"})),
        trade(3, json!({})),
        snapshot(4, json!({"sequence": "4", "authoritative": false})),
        order(5, json!({})),
        snapshot(6, json!({"sequence": "6", "bids": []})),
        trade(7, json!({"bookSequence": "6", "quantity": 2})),
    ]);
    assert_eq!(r["fills"].as_array().unwrap().len(), 1);
    assert_eq!(r["fills"][0]["eventId"], "trade-7");
    assert_eq!(oview(&r, 1)["reason"], "book_not_fresh_at_submission");
    assert_eq!(r["metrics"]["gaps"], 1);
}

#[test]
fn trade_book_mismatch_resync() {
    let r = run(&[snapshot(0, json!({})), order(1, json!({})), trade(2, json!({"bookSequence": "0"})), trade(3, json!({}))]);
    assert_eq!(r["fills"].as_array().unwrap().len(), 0);
    assert_eq!(r["metrics"]["gaps"], 1);
}

#[test]
fn inflight_snapshot_cannot_resurrect() {
    let r = run(&[
        snapshot(0, json!({"bids": []})),
        order(1, json!({})),
        event("gap", 5, json!({"instrument": "synthetic:YES", "reason": "disconnect"})),
        snapshot(6, json!({"occurredAt": timestamp(2), "sequence": "2", "bids": []})),
        trade(7, json!({"sequence": "3", "bookSequence": "2", "quantity": 5})),
    ]);
    assert_eq!(r["fills"].as_array().unwrap().len(), 0);
    assert_eq!(r["cash"], "100.000000");
    assert!(r["audit"].as_array().unwrap().iter().any(|x| x["reason"] == "snapshot_before_recovery_barrier"));
}

#[test]
fn mismatched_trade_sets_floors() {
    let r = run(&[
        snapshot(0, json!({"bids": []})),
        order(1, json!({})),
        trade(5, json!({"sequence": "5", "bookSequence": "0", "quantity": 5})),
        snapshot(6, json!({"occurredAt": timestamp(2), "sequence": "2", "bids": []})),
        snapshot(7, json!({"sequence": "4", "bids": []})),
        trade(8, json!({"sequence": "3", "bookSequence": "4", "quantity": 5})),
        snapshot(9, json!({"sequence": "5", "bids": []})),
        trade(10, json!({"sequence": "6", "bookSequence": "5", "quantity": 2})),
    ]);
    assert_eq!(r["fills"].as_array().unwrap().len(), 1);
    assert_eq!(r["fills"][0]["eventId"], "trade-10");
    assert_eq!(r["fills"][0]["quantity"], 2);
}

#[test]
fn gap_before_first_book() {
    let r = run(&[
        event("gap", 5, json!({"instrument": "synthetic:YES", "reason": "initial disconnect"})),
        snapshot(6, json!({"occurredAt": timestamp(5), "sequence": "3", "bids": []})),
        order(7, json!({})),
        snapshot(8, json!({"sequence": "4", "authoritative": false, "bids": []})),
        order(9, json!({})),
        snapshot(10, json!({"sequence": "4", "bids": []})),
        order(11, json!({})),
        trade(12, json!({"sequence": "5", "bookSequence": "4", "quantity": 2})),
    ]);
    assert_eq!(oview(&r, 0)["reason"], "book_not_fresh_at_submission");
    assert_eq!(oview(&r, 1)["reason"], "book_not_fresh_at_submission");
    assert_eq!(r["fills"].as_array().unwrap().len(), 1);
    assert_eq!(r["fills"][0]["orderId"], "o-11");
}

#[test]
fn higher_versions_not_rolled_back() {
    let r = run(&[
        snapshot(0, json!({"bids": []})),
        order(1, json!({})),
        event("gap", 3, json!({"instrument": "synthetic:YES", "reason": "disconnect"})),
        trade(4, json!({"sequence": "20", "quantity": 5})),
        snapshot(5, json!({"sequence": "10", "bids": []})),
        snapshot(6, json!({"sequence": "21", "occurredAt": timestamp(2), "bids": []})),
        snapshot(7, json!({"sequence": "20", "bids": []})),
        snapshot(8, json!({"sequence": "21", "bids": []})),
        trade(9, json!({"sequence": "22", "bookSequence": "21", "quantity": 2})),
    ]);
    assert_eq!(r["fills"].as_array().unwrap().len(), 1);
    assert_eq!(r["fills"][0]["eventId"], "trade-9");
}

#[test]
fn repeated_gaps_raise_floor() {
    let r = run(&[
        snapshot(0, json!({"bids": []})),
        order(1, json!({})),
        event("gap", 3, json!({"instrument": "synthetic:YES", "reason": "first gap"})),
        snapshot(4, json!({"sequence": "20", "occurredAt": timestamp(2), "bids": []})),
        event("gap", 6, json!({"instrument": "synthetic:YES", "reason": "second gap"})),
        snapshot(7, json!({"sequence": "20", "occurredAt": timestamp(5), "bids": []})),
        order(8, json!({})),
        snapshot(9, json!({"sequence": "19", "bids": []})),
        order(10, json!({})),
        snapshot(11, json!({"sequence": "20", "bids": []})),
        trade(12, json!({"sequence": "21", "bookSequence": "20", "quantity": 2})),
    ]);
    assert_eq!(oview(&r, 1)["reason"], "book_not_fresh_at_submission");
    assert_eq!(oview(&r, 2)["reason"], "book_not_fresh_at_submission");
    assert_eq!(r["fills"].as_array().unwrap().len(), 1);
    assert_eq!(r["fills"][0]["eventId"], "trade-12");
}

#[test]
fn rejected_snapshots_retain_floors() {
    let r = run(&[
        snapshot(0, json!({"bids": []})),
        order(1, json!({})),
        event("gap", 5, json!({"instrument": "synthetic:YES", "reason": "disconnect"})),
        snapshot(10, json!({"sequence": "10", "authoritative": false, "bids": []})),
        snapshot(11, json!({"sequence": "10", "occurredAt": timestamp(8), "bids": []})),
        order(12, json!({})),
        snapshot(13, json!({"sequence": "10", "occurredAt": timestamp(10), "bids": []})),
        trade(14, json!({"sequence": "11", "bookSequence": "10", "quantity": 2})),
    ]);
    assert_eq!(oview(&r, 1)["reason"], "book_not_fresh_at_submission");
    assert_eq!(r["fills"].as_array().unwrap().len(), 1);
    assert_eq!(r["fills"][0]["eventId"], "trade-14");
}

#[test]
fn replenishment_moved_ahead() {
    let r = run(&[
        snapshot(0, json!({})),
        order(1, json!({})),
        trade(2, json!({"quantity": 5})),
        snapshot(3, json!({"sequence": "3", "bids": [level("0.400", 20, 0)]})),
        trade(4, json!({"bookSequence": "3", "quantity": 19})),
    ]);
    assert_eq!(r["fills"].as_array().unwrap().len(), 0);
    assert_eq!(oview(&r, 0)["queueAhead"], "1");
}

#[test]
fn duplicates_idempotent_conflicting_halt() {
    let s = snapshot(0, json!({}));
    let t = trade(2, json!({"quantity": 12}));
    let r = run(&[s.clone(), order(1, json!({})), t.clone(), t.clone()]);
    assert_eq!(r["fills"].as_array().unwrap().len(), 1);
    assert_eq!(r["metrics"]["duplicates"], 1);
    let mut s2 = s.clone();
    s2["bids"] = json!([]);
    assert!(replay(&[s.clone(), s2], &config(json!({}))).unwrap_err().contains("Conflicting duplicate"));
}

#[test]
fn older_sequence_ignored() {
    let r = run_cfg(
        &[
            snapshot(0, json!({})),
            snapshot(2, json!({"eventId": "older", "sequence": "0"})),
            snapshot(20, json!({"occurredAt": timestamp(0)})),
            order(21, json!({})),
        ],
        config(json!({"maxDataAgeMs": 10})),
    );
    assert_eq!(r["metrics"]["staleVersions"], 2);
    assert_eq!(oview(&r, 0)["reason"], "book_not_fresh_at_submission");
    assert!(replay(
        &[snapshot(0, json!({})), snapshot(1, json!({"occurredAt": timestamp(0), "bids": []}))],
        &config(json!({})),
    )
    .unwrap_err()
    .contains("Conflicting snapshot"));
}

#[test]
fn chronology_rejected() {
    assert!(replay(&[snapshot(2, json!({})), order(1, json!({}))], &config(json!({}))).unwrap_err().contains("chronological"));
    assert!(replay(&[snapshot(1, json!({"occurredAt": timestamp(2)}))], &config(json!({})))
        .unwrap_err()
        .contains("after receipt"));
    let r = run(&[
        snapshot(0, json!({})),
        order(1, json!({})),
        trade(10, json!({"quantity": 1})),
        trade(11, json!({"occurredAt": timestamp(5), "quantity": 100})),
    ]);
    assert_eq!(r["metrics"]["gaps"], 1);
    assert_eq!(r["fills"].as_array().unwrap().len(), 0);
}

#[test]
fn stale_trade_versions_not_reused() {
    let r = run_cfg(
        &[
            snapshot(0, json!({})),
            order(1, json!({})),
            trade(2, json!({"quantity": 12})),
            trade(3, json!({"sequence": "2", "occurredAt": timestamp(2), "quantity": 12})),
            trade(30, json!({})),
        ],
        config(json!({"maxDataAgeMs": 10})),
    );
    assert_eq!(r["fills"].as_array().unwrap().len(), 1);
    assert_eq!(r["metrics"]["staleVersions"], 1);
    assert_eq!(r["metrics"]["staleEvidence"], 1);
}

#[test]
fn same_seq_newer_time_conflicts() {
    assert!(replay(
        &[
            snapshot(0, json!({"bids": []})),
            order(1, json!({})),
            trade(2, json!({"quantity": 2})),
            trade(3, json!({"sequence": "2", "quantity": 2})),
        ],
        &config(json!({})),
    )
    .unwrap_err()
    .contains("Conflicting trade evidence"));
}

#[test]
fn duplicate_evidence_after_cancel_not_ambiguous() {
    let r = run(&[
        snapshot(0, json!({"bids": []})),
        order(1, json!({})),
        trade(2, json!({"quantity": 2})),
        event("cancel", 3, json!({"orderId": "o-1"})),
        trade(60, json!({"sequence": "2", "occurredAt": timestamp(2), "quantity": 2})),
        trade(61, json!({"sequence": "3", "occurredAt": timestamp(3), "quantity": 2, "externalAggressor": false})),
    ]);
    assert_eq!(r["fills"].as_array().unwrap().len(), 1);
    assert_eq!(oview(&r, 0)["status"], "cancelled");
}

#[test]
fn higher_seq_backward_time_resync() {
    let r = run(&[snapshot(10, json!({})), snapshot(11, json!({"sequence": "2", "occurredAt": timestamp(5)})), order(12, json!({}))]);
    assert_eq!(r["metrics"]["gaps"], 1);
    assert_eq!(oview(&r, 0)["reason"], "book_not_fresh_at_submission");
}

#[test]
fn reservations_prevent_overspend() {
    let r = run_cfg(
        &[snapshot(0, json!({})), order(1, json!({"quantity": 5})), order(2, json!({"quantity": 1}))],
        config(json!({"initialCash": "2", "orderLatencyMs": 100})),
    );
    assert_eq!(oview(&r, 1)["reason"], "insufficient_unreserved_cash");
    assert_eq!(r["reservedCash"], "2.000000");
}

#[test]
fn per_share_fee_reserve_solvency() {
    let cfg = config(json!({"initialCash": "0.015003", "costs": {"feeBps": 1, "feePerShare": "0", "assumption": "unverified_scenario"}}));
    let r = run_cfg(
        &[
            snapshot(0, json!({"bids": []})),
            order(1, json!({"quantity": 3, "limitPrice": "0.005"})),
            trade(2, json!({"price": "0.005", "quantity": 1})),
            trade(3, json!({"price": "0.005", "quantity": 1})),
            trade(4, json!({"price": "0.005", "quantity": 1})),
        ],
        cfg,
    );
    assert_eq!(r["cash"], "0.000000");
    assert_eq!(r["totalFees"], "0.000003");
    assert_eq!(r["remainingCostBasis"], "0.015003");
}

#[test]
fn position_limit_includes_pending() {
    let r = run_cfg(&[snapshot(0, json!({})), order(1, json!({})), order(2, json!({}))], config(json!({"maxPositionShares": 8})));
    assert_eq!(oview(&r, 1)["reason"], "inventory_limit");
}

#[test]
fn sells_reserve_inventory() {
    let r = run(&[
        snapshot(0, json!({"bids": []})),
        order(1, json!({})),
        trade(2, json!({"quantity": 5})),
        order(3, json!({"action": "sell", "quantity": 4, "limitPrice": "0.600"})),
        order(4, json!({"action": "sell", "quantity": 2, "limitPrice": "0.600"})),
    ]);
    assert_eq!(oview(&r, 2)["reason"], "insufficient_unreserved_inventory");
}

#[test]
fn marketable_orders_rejected() {
    let r = run(&[snapshot(0, json!({})), order(1, json!({"limitPrice": "0.600"}))]);
    assert_eq!(oview(&r, 0)["reason"], "marketable_order_unsupported");
}

#[test]
fn fifo_basis_conserves_micros() {
    let cfg = config(json!({"costs": {"feeBps": 1, "feePerShare": "0", "assumption": "unverified_scenario"}}));
    let r = run_cfg(
        &[
            snapshot(0, json!({"bids": [], "asks": []})),
            order(1, json!({"quantity": 3, "limitPrice": "0.005"})),
            trade(2, json!({"quantity": 3, "price": "0.005"})),
            order(3, json!({"action": "sell", "quantity": 3, "limitPrice": "0.010"})),
            trade(4, json!({"aggressor": "buy", "price": "0.010", "quantity": 1})),
            trade(5, json!({"aggressor": "buy", "price": "0.010", "quantity": 2})),
        ],
        cfg,
    );
    assert_eq!(r["remainingCostBasis"], "0.000000");
    assert_eq!(r["realizedPnl"], "0.014995");
    assert_eq!(r["cash"], "100.014995");
    assert_eq!(r["totalFees"], "0.000005");
}

#[test]
fn valuations_stay_separate() {
    let r = run(&[
        snapshot(0, json!({"bids": []})),
        order(1, json!({})),
        trade(2, json!({"quantity": 5})),
        snapshot(3, json!({"sequence": "3", "bids": [level("0.450", 2, 0), level("0.440", 3, 0)]})),
        event("mark", 4, json!({"instrument": "synthetic:YES", "source": "reference", "price": "0.5"})),
        event("mark", 5, json!({"instrument": "synthetic:YES", "source": "tournament", "price": "0.99"})),
        event("mark", 6, json!({"instrument": "synthetic:YES", "source": "last", "price": "0.9"})),
    ]);
    assert_eq!(r["realizedPnl"], "0.000000");
    assert_eq!(r["referenceUnrealizedPnl"], "0.500000");
    assert_eq!(r["referenceEquity"], "100.500000");
    assert_eq!(r["estimatedExecutableEquity"], "100.220000");
    assert_eq!(r["positions"][0]["marks"]["tournament"]["value"], "4.950000");
    assert_eq!(r["metrics"]["drawdownComplete"], false);
}

#[test]
fn stale_marks_null_valuation() {
    let r = run_cfg(
        &[
            snapshot(0, json!({"bids": []})),
            order(1, json!({})),
            trade(2, json!({"quantity": 5})),
            event("mark", 3, json!({"instrument": "synthetic:YES", "source": "reference", "price": "0.5"})),
            event("clock", 20, json!({})),
        ],
        config(json!({"maxDataAgeMs": 10})),
    );
    assert_eq!(r["referenceEquity"], V::Null);
    assert_eq!(r["estimatedExecutableEquity"], V::Null);
    assert_eq!(r["positions"][0]["executable"]["unsoldQuantity"], 5);
}

#[test]
fn extreme_fee_funding_shortfall() {
    let cfg = config(json!({"initialCash": "0.115", "costs": {"feeBps": 0, "feePerShare": "0.100", "assumption": "unverified_scenario"}}));
    let r = run_cfg(
        &[
            snapshot(0, json!({"bids": []})),
            order(1, json!({"quantity": 1, "limitPrice": "0.010"})),
            trade(2, json!({"quantity": 1, "price": "0.010"})),
            snapshot(3, json!({"sequence": "3", "bids": [level("0.005", 1, 0)]})),
        ],
        cfg,
    );
    assert_eq!(r["depthComplete"], true);
    assert_eq!(r["liquidationFeeFundingShortfall"], "0.090000");
    assert_eq!(r["executableComplete"], false);
    assert_eq!(r["estimatedExecutableEquity"], V::Null);
}

#[test]
fn deterministic_no_mutation() {
    let events = vec![snapshot(0, json!({})), order(1, json!({})), trade(2, json!({"quantity": 12}))];
    let copy = events.clone();
    assert_eq!(run(&events), run(&events));
    assert_eq!(events, copy);
}

#[test]
fn schema_rejects_bad() {
    assert!(validate_event(&snapshot(0, json!({"bids": [{"price": "0.4", "quantity": 1}]}))).is_err());
    assert!(validate_event(&trade(2, json!({"evidence": "touch"}))).is_err());
    let mut t = trade(2, json!({}));
    t.as_object_mut().unwrap().remove("externalAggressor");
    assert!(validate_event(&t).is_err());
    assert!(validate_event(&order(1, json!({"limitPrice": 0.4}))).is_err());
    assert!(validate_event(&order(1, json!({"quantity": 1.2}))).is_err());
    assert!(validate_event(&snapshot(0, json!({"bids": [level("0.600", 2, 0)]}))).is_err());
    assert!(validate_event(&snapshot(0, json!({"bids": [level("0.400", 1, 2)]}))).is_err());
    assert!(validate_event(&snapshot(0, json!({"receivedAt": "2026-02-30T00:00:00.000Z"}))).is_err());
    assert!(validate_event(&snapshot(0, json!({"sequence": 1}))).is_err());
    assert!(validate_config(&config(json!({"costs": {"feeBps": 0, "feePerShare": "0", "assumption": "verified"}}))).is_err());
}

#[test]
fn unknown_cancel_and_reused_ids_halt() {
    assert!(replay(&[event("cancel", 1, json!({"orderId": "unknown"}))], &config(json!({})))
        .unwrap_err()
        .contains("unknown order"));
    assert!(replay(&[snapshot(0, json!({})), order(1, json!({})), order(2, json!({"orderId": "o-1"}))], &config(json!({})))
        .unwrap_err()
        .contains("already used"));
}

#[test]
fn no_trade_baseline_constant() {
    let r = run(&[snapshot(0, json!({})), event("clock", 100, json!({}))]);
    assert_eq!(r["cash"], "100.000000");
    assert_eq!(r["realizedPnl"], "0.000000");
    assert_eq!(r["metrics"]["simulatedWriteRequests"], 0);
    assert_eq!(r["metrics"]["fillRatio"], V::Null);
    assert_eq!(r["noTradeBaseline"], json!({"cash": "100.000000", "pnl": "0.000000"}));
}

// invariants.test.mjs — 50 seeds x 300 events, all engine invariants.
#[test]
fn seeded_invariants() {
    let mut checked = 0;
    for seed in 1..=50u32 {
        let mut rng = seed;
        let mut r = |n: i64| -> i64 {
            rng = (rng as i32).wrapping_mul(1664525).wrapping_add(1013904223) as u32;
            (rng % n as u32) as i64
        };
        let cfg = json!({
            "schemaVersion": 1, "initialCash": "100", "maxDataAgeMs": 10000,
            "orderLatencyMs": r(5), "cancelLatencyMs": r(5), "maxPositionShares": 200,
            "costs": {"assumption": "unverified_scenario", "feeBps": r(10001),
                      "feePerShare": format!("0.{:06}", r(100001))},
        });
        let mut e = PaperEngine::new(&cfg).unwrap();
        let mut bs = "1".to_string();
        for t in 0..300i64 {
            let mut x = json!({
                "schemaVersion": 1, "eventId": format!("e{t}"),
                "receivedAt": timestamp(t), "occurredAt": timestamp(t),
            });
            let kind = if t == 0 { 0 } else { r(10) };
            if kind == 0 {
                bs = (t + 1).to_string();
                x.as_object_mut().unwrap().extend(json!({
                    "type": "snapshot", "instrument": "X", "sequence": bs, "authoritative": true,
                    "nextExpiryAt": V::Null,
                    "bids": [{"price": "0.3", "quantity": 1 + r(100), "ownQuantity": 0}],
                    "asks": [{"price": "0.7", "quantity": 1 + r(100), "ownQuantity": 0}],
                }).as_object().unwrap().clone());
            } else if kind < 4 {
                let action = if r(2) == 1 { "buy" } else { "sell" };
                x.as_object_mut().unwrap().extend(json!({
                    "type": "submit", "instrument": "X", "orderId": format!("o{t}"), "action": action,
                    "quantity": 1 + r(20), "limitPrice": if action == "buy" { "0.3" } else { "0.7" },
                    "expiresAt": if r(3) != 0 { V::Null } else { json!(timestamp(t + 1 + r(50))) },
                }).as_object().unwrap().clone());
            } else if kind < 8 {
                let ag = if r(2) == 1 { "buy" } else { "sell" };
                x.as_object_mut().unwrap().extend(json!({
                    "type": "trade", "instrument": "X", "sequence": (t + 1).to_string(), "bookSequence": bs,
                    "price": if ag == "buy" { "0.7" } else { "0.3" }, "quantity": 1 + r(100),
                    "aggressor": ag, "externalAggressor": r(5) != 0, "evidence": "explicit_trade",
                }).as_object().unwrap().clone());
            } else if kind == 8 && !e.order_ids().is_empty() {
                let ids = e.order_ids();
                x.as_object_mut().unwrap().extend(json!({
                    "type": "cancel", "orderId": ids[r(ids.len() as i64) as usize],
                }).as_object().unwrap().clone());
            } else {
                x.as_object_mut().unwrap().insert("type".into(), json!("clock"));
            }
            let old_fills = e.fills().len();
            e.apply(&x).unwrap();
            assert_eq!(
                e.cash_now() + e.basis("X") - e.realized(),
                e.initial_cash(),
                "cash invariant seed{seed} t{t}"
            );
            assert!(e.cash_now() >= e.reserved_cash());
            assert!(e.quantity("X") >= e.reserved_shares("X"));
            let fees: BigInt = e
                .fills()
                .iter()
                .map(|f| big_amount(f["fee"].as_str().unwrap()).unwrap())
                .sum();
            assert_eq!(fees, e.total_fees());
            if x["type"] == "trade" {
                let filled: i64 = e.fills()[old_fills..].iter().map(|f| f["quantity"].as_i64().unwrap()).sum();
                assert!(filled <= x["quantity"].as_i64().unwrap());
            }
            for action in ["buy", "sell"] {
                let mut floor = BigInt::from(0);
                let mut views: Vec<V> = e
                    .order_views()
                    .into_iter()
                    .filter(|o| {
                        ["open", "pending_cancel"].contains(&o["status"].as_str().unwrap_or(""))
                            && !o["activatedAt"].is_null()
                            && o["action"] == action
                    })
                    .collect();
                views.sort_by_key(|o| o["ordinal"].as_i64().unwrap());
                for o in views {
                    let qa: BigInt = o["queueAhead"].as_str().unwrap().parse().unwrap();
                    assert!(qa >= floor, "queue overlap seed{seed} t{t}");
                    floor = qa + BigInt::from(o["remaining"].as_i64().unwrap());
                }
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 15000);
}

// --- split.test.mjs ---
const SPLIT_BASE: i64 = 1_790_841_600_000; // Date.parse('2026-10-01T00:00:00.000Z')
fn sat(offset: i64) -> String {
    baal_service_runtime::contract::ms_to_iso((SPLIT_BASE + offset) as f64)
}
fn sevent(id: &str, received: i64, occurred: i64, payload: V) -> V {
    merge(
        json!({"schemaVersion": 1, "eventId": id, "receivedAt": sat(received), "occurredAt": sat(occurred)})
            .as_object()
            .unwrap()
            .clone(),
        &payload,
    )
}
fn sconfig(overrides: V) -> V {
    merge(
        json!({"trainEnd": sat(100), "validationEnd": sat(200), "labelHorizonMs": 0, "embargoMs": 0})
            .as_object()
            .unwrap()
            .clone(),
        &overrides,
    )
}
fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}
fn ids(m: &V) -> (Vec<String>, Vec<String>, Vec<String>) {
    let g = |n: &str| {
        m["partitions"][n]["eventIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };
    (g("train"), g("validation"), g("test"))
}

#[test]
fn split_half_open_cuts() {
    let events = vec![
        sevent("early", 0, 0, json!({})),
        sevent("before", 99, 99, json!({})),
        sevent("cut-a", 100, 1, json!({})),
        sevent("cut-b", 100, 0, json!({})),
        sevent("middle", 199, 10, json!({})),
        sevent("test-a", 200, 1, json!({})),
        sevent("test-b", 200, 0, json!({})),
        sevent("later", 201, 11, json!({})),
    ];
    let r = chronological_split(&events, &sconfig(json!({}))).unwrap();
    assert_eq!(
        ids(&r),
        (strs(&["early", "before"]), strs(&["cut-a", "cut-b", "middle"]), strs(&["test-a", "test-b", "later"]))
    );
    assert_eq!(r["counts"], json!({"source": 8, "train": 2, "validation": 3, "test": 3, "excluded": 0}));
    assert_eq!(r["basis"], "receivedAt");
    assert_eq!(r["partitions"]["train"]["startInclusive"], V::Null);
    assert_eq!(r["partitions"]["train"]["endExclusive"], sat(100));
}

#[test]
fn split_label_horizon_purge() {
    let events = vec![
        sevent("safe-train", 79, 79, json!({})),
        sevent("equal-train-a", 80, 80, json!({})),
        sevent("equal-train-b", 80, 80, json!({})),
        sevent("cross-train", 99, 99, json!({})),
        sevent("safe-validation", 179, 179, json!({})),
        sevent("equal-validation", 180, 180, json!({})),
        sevent("cross-validation", 199, 199, json!({})),
        sevent("test-label-unchecked", 200, 200, json!({})),
    ];
    let r = chronological_split(&events, &sconfig(json!({"labelHorizonMs": 20}))).unwrap();
    assert_eq!(ids(&r), (strs(&["safe-train"]), strs(&["safe-validation"]), strs(&["test-label-unchecked"])));
    let excluded: Vec<(String, Vec<String>)> = r["excluded"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["eventId"].as_str().unwrap().to_string(),
                e["reasons"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect(),
            )
        })
        .collect();
    assert_eq!(
        excluded,
        vec![
            ("equal-train-a".to_string(), vec!["label-horizon:trainEnd".to_string()]),
            ("equal-train-b".to_string(), vec!["label-horizon:trainEnd".to_string()]),
            ("cross-train".to_string(), vec!["label-horizon:trainEnd".to_string()]),
            ("equal-validation".to_string(), vec!["label-horizon:validationEnd".to_string()]),
            ("cross-validation".to_string(), vec!["label-horizon:validationEnd".to_string()]),
        ]
    );
}

#[test]
fn split_embargo_edges() {
    let events: Vec<V> = [89, 90, 99, 100, 109, 110, 189, 190, 199, 200, 209, 210]
        .into_iter()
        .map(|t| sevent(&format!("e{t}"), t, t, json!({})))
        .collect();
    let r = chronological_split(&events, &sconfig(json!({"embargoMs": 10}))).unwrap();
    assert_eq!(ids(&r), (strs(&["e89"]), strs(&["e110", "e189"]), strs(&["e210"])));
    assert_eq!(
        r["embargoWindows"],
        json!([
            {"cut": "trainEnd", "startInclusive": sat(90), "endExclusive": sat(110)},
            {"cut": "validationEnd", "startInclusive": sat(190), "endExclusive": sat(210)},
        ])
    );
    let excluded_ids: Vec<String> = r["excluded"].as_array().unwrap().iter().map(|e| e["eventId"].as_str().unwrap().to_string()).collect();
    assert_eq!(excluded_ids, vec!["e90", "e99", "e100", "e109", "e190", "e199", "e200", "e209"]);
}

#[test]
fn split_overlap_reasons() {
    let r = chronological_split(
        &[sevent("both", 95, 95, json!({})), sevent("purge-only", 175, 175, json!({})), sevent("safe", 210, 210, json!({}))],
        &sconfig(json!({"labelHorizonMs": 25, "embargoMs": 10})),
    )
    .unwrap();
    assert_eq!(
        r["excluded"],
        json!([
            {"eventId": "both", "receivedAt": sat(95), "nominalPartition": "train", "reasons": ["embargo:trainEnd", "label-horizon:trainEnd"]},
            {"eventId": "purge-only", "receivedAt": sat(175), "nominalPartition": "validation", "reasons": ["label-horizon:validationEnd"]},
        ])
    );
}

#[test]
fn split_late_old_events() {
    let r = chronological_split(
        &[sevent("train", 10, 10, json!({})), sevent("late", 210, 1, json!({}))],
        &sconfig(json!({})),
    )
    .unwrap();
    assert_eq!(ids(&r), (strs(&["train"]), strs(&[]), strs(&["late"])));
}

#[test]
fn split_one_ms_validation() {
    let r = chronological_split(
        &[sevent("valid", 101, 101, json!({}))],
        &sconfig(json!({"validationEnd": sat(104), "labelHorizonMs": 2, "embargoMs": 1})),
    )
    .unwrap();
    assert_eq!(r["partitions"]["validation"]["eventIds"], json!(["valid"]));
}

#[test]
fn split_empty() {
    let r = chronological_split(&[], &sconfig(json!({}))).unwrap();
    assert_eq!(r["counts"], json!({"source": 0, "train": 0, "validation": 0, "test": 0, "excluded": 0}));
    assert_eq!(r["source"]["firstReceivedAt"], V::Null);
    assert_eq!(r["source"]["lastReceivedAt"], V::Null);
    assert_eq!(r["source"]["checksum"], baal_service_runtime::json_text::sha256(b"[]"));
}

#[test]
fn split_canonical_checksum_order_independent() {
    let first = sevent("a", 1, 0, json!({"payload": {"z": [{"2": 2, "10": 10, "a": null}], "a": "µ"}}));
    let second = json!({"payload": {"a": "µ", "z": [{"a": null, "10": 10, "2": 2}]},
        "occurredAt": sat(0), "receivedAt": sat(1), "eventId": "a", "schemaVersion": 1});
    let r = chronological_split(&[first], &sconfig(json!({}))).unwrap();
    let r2 = chronological_split(&[second], &sconfig(json!({}))).unwrap();
    assert_eq!(r["source"]["checksum"], r2["source"]["checksum"]);
    let canonical = format!(
        "[{{\"eventId\":\"a\",\"occurredAt\":\"{}\",\"payload\":{{\"a\":\"µ\",\"z\":[{{\"10\":10,\"2\":2,\"a\":null}}]}},\"receivedAt\":\"{}\",\"schemaVersion\":1}}]",
        sat(0), sat(1)
    );
    assert_eq!(r["source"]["checksum"], baal_service_runtime::json_text::sha256(canonical.as_bytes()));
}

#[test]
fn split_validation_rejects() {
    // missing keys
    for key in ["trainEnd", "validationEnd", "labelHorizonMs", "embargoMs"] {
        let mut c = sconfig(json!({}));
        c.as_object_mut().unwrap().remove(key);
        assert!(chronological_split(&[], &c).is_err());
    }
    assert!(chronological_split(&[], &V::Null).is_err());
    assert!(chronological_split(&[], &json!([])).is_err());
    // durations
    for name in ["labelHorizonMs", "embargoMs"] {
        for v in [json!(-1), json!(0.1), json!("1"), V::Null] {
            let c = merge(sconfig(json!({})).as_object().unwrap().clone(), &json!({name: v}));
            assert!(chronological_split(&[], &c).is_err());
        }
    }
    // ordering
    assert!(chronological_split(&[], &sconfig(json!({"validationEnd": sat(100)}))).is_err());
    assert!(chronological_split(&[], &sconfig(json!({"embargoMs": 50}))).is_err());
    assert!(chronological_split(&[], &sconfig(json!({"labelHorizonMs": 100}))).is_err());
    // overflow
    assert!(chronological_split(&[], &sconfig(json!({"embargoMs": 9_007_199_254_740_991i64}))).is_err());
}

// cli.test.mjs golden fixture + arg validation.
#[test]
fn cli_golden_fixture() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../paper/fixtures");
    let events = root.join("synthetic.jsonl");
    let cfg = root.join("config.json");
    let args = vec![
        "--events".to_string(),
        events.to_string_lossy().into_owned(),
        "--config".to_string(),
        cfg.to_string_lossy().into_owned(),
    ];
    let actual = baal_paper::cli::main(&args).unwrap();
    let expected = std::fs::read_to_string(root.join("expected-report.json")).unwrap();
    assert_eq!(actual.trim(), expected.trim());
    assert_eq!(baal_paper::cli::main(&args).unwrap(), actual);
    let report: V = serde_json::from_str(&actual).unwrap();
    assert_eq!(report["noTrade"]["cash"], "100.000000");
    assert_eq!(report["noTrade"]["metrics"]["simulatedWriteRequests"], 0);
    assert_eq!(report["scriptedPassive"]["fills"].as_array().unwrap().len(), 4);
}

#[test]
fn cli_rejects_bad_args() {
    assert!(baal_paper::cli::main(&[]).is_err());
    assert!(baal_paper::cli::main(&["--live".to_string()]).is_err());
    assert!(baal_paper::cli::main(&["--events".to_string(), "https://example.com/x".to_string(), "--config".to_string(), "x".to_string()]).is_err());
    assert!(baal_paper::cli::main(&["--key".to_string(), "abc".to_string()]).is_err());
    assert!(baal_paper::cli::main(&["--events".to_string(), "--config".to_string()]).is_err());
    assert!(baal_paper::cli::main(&["--help".to_string()]).unwrap().contains("Offline paper"));
}
