// Parity tests for the ported execution path — assert the same boundaries the
// .test.mjs suites pin down in Node.
use baal_execution::core::{payload_hash, prepare_intent};
use baal_execution::decision::assess_entry_gates;
use baal_execution::gates::{evaluate_entry_gates, production_entry_state};
use baal_execution::median_controller::{validate_median_bank, validate_proposals};
use baal_execution::settlement::{native_publication_binding, validate_settlement_bank};
use baal_execution::transport::{OrderTransport, OrderTransportOptions, TransportScope};
use serde_json::{json, Value as V};

fn policy() -> V {
    json!({
        "minimumPrice": 0.05,
        "maximumPrice": 0.95,
        "maximumSpread": 0.02,
        "minimumDip": 0.02,
        "minimumRelativeDip": 0.03,
        "maximumDip": 0.1,
        "minimumZ": 1.5,
    })
}
fn reference(center: f64, deviation: f64) -> V {
    json!({
        "ready": true,
        "center": center,
        "robustDeviation": deviation,
        "samples": 8,
    })
}
fn tests(v: &V) -> V {
    v.get("tests").cloned().unwrap_or(V::Null)
}

#[test]
fn entry_gate_boundaries() {
    // 'observed exact spread and minimum discount boundaries admit'
    let g = evaluate_entry_gates(&reference(0.275, 0.005), &json!("0.260000"), &json!("0.240000"), &policy()).unwrap();
    assert_eq!(tests(&g).get("spread"), Some(&V::Bool(true)));
    assert_eq!(tests(&g).get("minimumDiscount"), Some(&V::Bool(false)));
    let g = evaluate_entry_gates(&reference(0.275, 0.005), &json!("0.255000"), &json!("0.240000"), &policy()).unwrap();
    assert_eq!(g.get("admitted"), Some(&V::Bool(true)));
    let g = evaluate_entry_gates(&reference(0.28, 0.005), &json!("0.260000"), &json!("0.240000"), &policy()).unwrap();
    assert_eq!(g.get("admitted"), Some(&V::Bool(true)));
}

#[test]
fn entry_gate_spread_lock() {
    for bid in ["0.239999", "0.260000", "0.265000"] {
        let g = evaluate_entry_gates(&reference(0.28, 0.005), &json!("0.260000"), &json!(bid), &policy()).unwrap();
        assert_eq!(tests(&g).get("spread"), Some(&V::Bool(false)), "bid {bid}");
    }
}

#[test]
fn entry_gate_submicro_precision() {
    let g = evaluate_entry_gates(&reference(0.274999999999999, 0.005), &json!("0.255000"), &json!("0.240000"), &policy()).unwrap();
    assert_eq!(tests(&g).get("minimumDiscount"), Some(&V::Bool(false)));
    let g = evaluate_entry_gates(&reference(0.275000000000001, 0.005), &json!("0.255000"), &json!("0.240000"), &policy()).unwrap();
    assert_eq!(tests(&g).get("minimumDiscount"), Some(&V::Bool(true)));
    let g = evaluate_entry_gates(&reference(0.355000000000001, 0.005), &json!("0.255000"), &json!("0.240000"), &policy()).unwrap();
    assert_eq!(tests(&g).get("maximumDiscount"), Some(&V::Bool(false)));
    let g = evaluate_entry_gates(&reference(0.355, 0.005), &json!("0.255000"), &json!("0.240000"), &policy()).unwrap();
    assert_eq!(tests(&g).get("maximumDiscount"), Some(&V::Bool(true)));
}

#[test]
fn entry_gate_price_relative_z() {
    assert_eq!(
        tests(&evaluate_entry_gates(&reference(0.075, 0.005), &json!("0.050000"), &json!("0.045000"), &policy()).unwrap()).get("price"),
        Some(&V::Bool(true))
    );
    assert_eq!(
        tests(&evaluate_entry_gates(&reference(0.075, 0.005), &json!("0.049999"), &json!("0.045000"), &policy()).unwrap()).get("price"),
        Some(&V::Bool(false))
    );
    assert_eq!(
        tests(&evaluate_entry_gates(&reference(0.97, 0.005), &json!("0.950000"), &json!("0.945000"), &policy()).unwrap()).get("price"),
        Some(&V::Bool(true))
    );
    assert_eq!(
        tests(&evaluate_entry_gates(&reference(0.97, 0.005), &json!("0.950001"), &json!("0.945000"), &policy()).unwrap()).get("price"),
        Some(&V::Bool(false))
    );
    let p2 = json!({
        "minimumPrice": 0.05, "maximumPrice": 0.95, "maximumSpread": 0.02,
        "minimumDip": 0.02, "minimumRelativeDip": 0.04, "maximumDip": 0.1, "minimumZ": 1.5,
    });
    assert_eq!(
        tests(&evaluate_entry_gates(&reference(0.5, 0.005), &json!("0.480000"), &json!("0.475000"), &p2).unwrap()).get("relativeDiscount"),
        Some(&V::Bool(true))
    );
    assert_eq!(
        tests(&evaluate_entry_gates(&reference(0.5, 0.005), &json!("0.480001"), &json!("0.475000"), &p2).unwrap()).get("relativeDiscount"),
        Some(&V::Bool(false))
    );
    assert_eq!(
        tests(&evaluate_entry_gates(&reference(0.275, 0.01), &json!("0.260000"), &json!("0.245000"), &policy()).unwrap()).get("robustZ"),
        Some(&V::Bool(true))
    );
    assert_eq!(
        tests(&evaluate_entry_gates(&reference(0.275, 0.010000000000001), &json!("0.260000"), &json!("0.245000"), &policy()).unwrap()).get("robustZ"),
        Some(&V::Bool(false))
    );
    for d in [0.0, -0.01] {
        assert_eq!(
            tests(&evaluate_entry_gates(&reference(0.275, d), &json!("0.260000"), &json!("0.245000"), &policy()).unwrap()).get("robustZ"),
            Some(&V::Bool(false))
        );
    }
}

#[test]
fn entry_gate_assess_matches_execute() {
    for tick in 10..=190i64 {
        let ask = format!("{:.6}", tick as f64 * 5000.0 / 1e6);
        let bid = format!("{:.6}", (tick - 2) as f64 * 5000.0 / 1e6);
        let center = ask.parse::<f64>().unwrap() + 0.025;
        let a = assess_entry_gates(&reference(center, 0.005), &json!(ask), &json!(bid), &policy()).unwrap();
        let g = evaluate_entry_gates(&reference(center, 0.005), &json!(ask), &json!(bid), &policy()).unwrap();
        assert_eq!(a.get("tests"), g.get("tests"), "tick {tick}");
    }
    assert_eq!(
        evaluate_entry_gates(&json!({"ready": false}), &V::Null, &V::Null, &V::Null)
            .unwrap()
            .get("admitted"),
        Some(&V::Bool(false))
    );
}

#[test]
fn transport_dispatch_guard_requires_fences() {
    // DISPATCH_GUARD_NOT_CALLED when the dispatch does not run its fences.
    let transport = OrderTransport::new(OrderTransportOptions {
        dispatch: Box::new(|_spec, _bd, _bn| {
            Ok(json!({"status": 200, "data": {"id": "9"}, "endpoint": "/orders"}))
        }),
        scope: TransportScope {
            tournament_id: baal_execution::TOURNAMENT_ID.into(),
            tournament_slug: baal_execution::TOURNAMENT_SLUG.into(),
            by_exchange: [("1".to_string(), "377".to_string())].into_iter().collect(),
            by_market: [("377".to_string(), vec!["1".to_string()])].into_iter().collect(),
        },
        can_write: Box::new(|| true),
        owns_order: Box::new(|_| false),
        validate_lease: Box::new(|_, _| Ok(true)),
        validate_permit: Box::new(|_, _| Ok(true)),
        validate_network_permit: Box::new(|_, _| Ok(true)),
        now: Box::new(|| 0),
        max_pages: 5,
    })
    .unwrap();
    let r = transport.place(
        &json!({
            "tournamentId": baal_execution::TOURNAMENT_ID,
            "exchangeId": "1",
            "side": "yes",
            "action": "buy",
            "quantity": 1,
            "price": 0.5,
            "expirationDate": "2026-10-07T00:00:00.000Z",
            "idempotencyKey": "k-1",
        }),
        &json!({
            "permit": {"intentId": "i-1", "runId": "r", "retry": false},
            "lease": {"runId": "r", "ownerId": "Codex", "scope": baal_execution::TOURNAMENT_ID},
        }),
    );
    assert!(r.is_err());
    assert_eq!(r.err().unwrap().code, "DISPATCH_GUARD_NOT_CALLED");
}

#[test]
fn transport_delete_requires_ownership() {
    let transport = OrderTransport::new(OrderTransportOptions {
        dispatch: Box::new(|_spec, _bd, _bn| {
            Ok(json!({"status": 200, "data": {}, "endpoint": "/orders/55"}))
        }),
        scope: TransportScope {
            tournament_id: baal_execution::TOURNAMENT_ID.into(),
            tournament_slug: baal_execution::TOURNAMENT_SLUG.into(),
            by_exchange: [("1".to_string(), "377".to_string())].into_iter().collect(),
            by_market: [("377".to_string(), vec!["1".to_string()])].into_iter().collect(),
        },
        can_write: Box::new(|| true),
        owns_order: Box::new(|_| false),
        validate_lease: Box::new(|_, _| Ok(true)),
        validate_permit: Box::new(|_, _| Ok(true)),
        validate_network_permit: Box::new(|_, _| Ok(true)),
        now: Box::new(|| 0),
        max_pages: 5,
    })
    .unwrap();
    let r = transport.cancel(&json!("55"), &json!({
        "permit": {"kind": "cancel", "intentId": "i-1", "orderId": "55", "runId": "r"},
        "lease": {"runId": "r", "ownerId": "Codex", "scope": baal_execution::TOURNAMENT_ID},
    }));
    assert!(r.is_err());
}

#[test]
fn production_entry_state_shape() {
    let activation = json!({
        "schemaVersion": 2,
        "startedAt": "2026-10-06T00:00:00.000Z",
        "entryExpiresAt": "2026-10-08T00:00:00.000Z",
    });
    let v = production_entry_state(&activation, 1_775_500_000_000.0, &["377".to_string()], &json!({}), None, "monitoring", "");
    assert!(v.is_object());
}

fn universe() -> V {
    json!({
        "hash": "u",
        "tournamentId": baal_execution::TOURNAMENT_ID,
        "exchanges": [{"marketId": "377", "exchangeId": "1"}],
    })
}
fn median_activation() -> V {
    json!({
        "strategyId": "historical-median-receipt-v1",
        "tournamentId": baal_execution::TOURNAMENT_ID,
        "startedAt": "2026-10-01T00:00:00.000Z",
        "strategyBankHash": "",
        "runId": "run-1",
    })
}

#[test]
fn median_proposals_reject_bad_state() {
    let s = json!({"schemaVersion": 2});
    assert!(validate_proposals(&s, "h", "r", &universe()).is_err());
    let s = json!({
        "schemaVersion": 1,
        "bankHash": "h",
        "runId": "r",
        "proposals": {
            "median-00000000-0000-0000-0000-000000000000": {
                "intentId": "median-00000000-0000-0000-0000-000000000000",
                "idempotencyKey": "k",
                "bankHash": "h",
                "marketId": "377",
                "exchangeId": "1",
                "action": "buy",
                "quantity": 1,
                "limit": "0.500000",
                "reason": "ask_below_earlier_hour_bid_median",
                "createdAt": "2026-10-01T00:00:00.000Z",
                "expiresAt": "2026-10-01T00:02:00.000Z",
                "status": "proposed",
                "originalBook": {
                    "marketId": "377",
                    "exchangeId": "1",
                    "sequence": "9",
                    "versionKey": "v",
                    "sourceAt": "2026-10-01T00:00:00.000Z",
                    "requestStartedAt": "2026-10-01T00:00:00.000Z",
                    "receivedAt": "2026-10-01T00:00:00.000Z",
                },
                "proof": {
                    "bankHash": "h",
                    "decisionAvailableAt": "2026-10-01T00:00:00.000Z",
                    "originalBook": {
                        "marketId": "377",
                        "exchangeId": "1",
                        "sequence": "9",
                        "versionKey": "v",
                        "sourceAt": "2026-10-01T00:00:00.000Z",
                        "requestStartedAt": "2026-10-01T00:00:00.000Z",
                        "receivedAt": "2026-10-01T00:00:00.000Z",
                    },
                    "reference": {"outcome": "reference"},
                },
            }
        },
        "books": {},
    });
    // proofHash must be the payload hash of the proof
    let proof = s.pointer("/proposals/median-00000000-0000-0000-0000-000000000000/proof").unwrap().clone();
    let mut s = s;
    s.pointer_mut("/proposals/median-00000000-0000-0000-0000-000000000000")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("proofHash".into(), json!(payload_hash(&proof).unwrap()));
    assert!(validate_proposals(&s, "h", "r", &universe()).is_ok());
    s.pointer_mut("/proposals/median-00000000-0000-0000-0000-000000000000")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("reason".into(), json!("bogus"));
    assert!(validate_proposals(&s, "h", "r", &universe()).is_err());
}

#[test]
fn median_bank_rejects_wrong_mode() {
    let bank = json!({"schemaVersion": 1, "mode": "wrong"});
    assert!(validate_median_bank(&bank, &median_activation(), &universe(), std::path::Path::new(".")).is_err());
}

#[test]
fn settlement_bank_rejects_wrong_mode() {
    let activation = json!({
        "schemaVersion": 2,
        "strategyId": "settlement-native-v1",
        "ownerAssignmentCommentId": "x",
    });
    assert!(validate_settlement_bank(&json!({"schemaVersion": 1, "mode": "nope"}), &activation, &universe()).is_err());
}

#[test]
fn native_binding_rejects_bad_shape() {
    assert!(native_publication_binding(std::path::Path::new("/tmp"), &json!({"schemaVersion": 1})).is_err());
    assert!(native_publication_binding(
        std::path::Path::new("relative"),
        &json!({
            "schemaVersion": 1,
            "mode": "native_public_book_prefix_v1",
            "binaryHash": "a".repeat(64),
            "engineHash": "b".repeat(64),
            "runtimeLibraryHash": "c".repeat(64),
        }),
    )
    .is_err());
}

#[test]
fn prepare_intent_rejects_foreign_market() {
    let state = json!({
        "schemaVersion": 1,
        "intents": {},
        "owned": {},
        "control": {"exits": {}, "cancellations": {}},
        "policy": {"maxOrderCost": "1.000000"},
    });
    let spec = json!({
        "intentId": "intent-1",
        "idempotencyKey": "k",
        "marketId": "999",
        "exchangeId": "1",
        "side": "yes",
        "action": "buy",
        "quantity": 1,
        "price": "0.500000",
        "expirationDate": "2026-10-06T00:00:30.000Z",
        "attribution": {"runId": "r", "strategyId": "s", "modelHash": "m", "signalId": "x", "reason": "r"},
    });
    let snapshot = json!({"tournamentId": baal_execution::TOURNAMENT_ID});
    let r = prepare_intent(&state, &spec, &snapshot, &json!(1_775_500_000_000i64));
    // 999 is not in snapshot book scope — must fail closed.
    assert!(r.get("ok") == Some(&V::Bool(false)) || r.get("ok").is_none());
}
