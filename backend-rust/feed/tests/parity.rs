// feed parity tests: sanitize/capture/collector/replay.
use baal_feed::capture::{sanitize, Capture};
use baal_feed::collector::MarketCollector;
use baal_feed::replay::replay;
use serde_json::{json, Value as V};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn scope() -> V {
    json!({
        "tournamentId": "bda92870-621e-47b0-bc3c-3602c5c26f55",
        "marketId": "1",
        "exchangeIds": ["1"],
    })
}

#[test]
fn sanitize_redacts() {
    let v = json!({
        "Authorization": "Bearer abc.def.ghi",
        "nested": {"apiKey": "sig_abcdefghijklmnop", "ok": 1},
        "list": ["x=eyJabc.def.ghi", "plain"],
        "safe": "hello",
    });
    let out = sanitize(&v, 0);
    assert!(out.get("Authorization").is_none());
    assert!(out["nested"].get("apiKey").is_none());
    assert_eq!(out["list"][0], "x=[REDACTED]");
    assert_eq!(out["list"][1], "plain");
    assert_eq!(out["safe"], "hello");
    assert_eq!(sanitize(&json!({"a": {"b": [1, 2]}}), 0), json!({"a": {"b": [1, 2]}}));
}

#[test]
fn capture_exclusive_and_fsnc() {
    let dir = std::env::temp_dir().join(format!("baal-feed-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cap.jsonl");
    let _ = std::fs::remove_file(&path);
    let mut cap = Capture::new(&path, true).unwrap();
    cap.append(&json!({"kind": "test", "token": "abc"})).unwrap();
    cap.close();
    let text = std::fs::read_to_string(&path).unwrap();
    let row: V = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(row["format"], "sig-feed-capture");
    assert_eq!(row["schemaVersion"], 1);
    assert_eq!(row["captureIndex"], 0);
    assert_eq!(row["kind"], "test");
    assert!(row.get("token").is_none()); // sensitive key dropped
    // 'wx': existing file must not be truncated
    std::fs::write(&path, "sentinel").unwrap();
    assert!(Capture::new(&path, true).is_err());
}

#[test]
fn collector_lifecycle() {
    let now = Rc::new(Cell::new(1_767_225_600_000.0f64));
    let now_c = now.clone();
    let mut c = MarketCollector::new(&scope(), 10000.0, Rc::new(move || now_c.get()), None, 100).unwrap();
    assert!(c.begin_recovery().is_err());
    c.subscribed("INITIAL").unwrap();
    assert_eq!(c.state, "needs_resync");
    let ticket = c.begin_recovery().unwrap();
    let snapshot = json!({
        "contexts": [{
            "type": "tournament",
            "tournament": {"id": "bda92870-621e-47b0-bc3c-3602c5c26f55"},
            "orderbook": {"exchanges": [{
                "exchangeId": "1",
                "asOf": {"sequence": 5, "at": "2026-01-01T00:00:00.000Z"},
                "nextExpiryAt": null,
                "bids": [{"price": 0.4, "quantity": 3}],
                "asks": [{"price": 0.6, "quantity": 2}],
            }]},
        }],
    });
    assert!(c.complete_recovery(&ticket, &snapshot, "2026-01-01T00:00:00.000Z"));
    assert_eq!(c.state, "ready");
}

#[test]
fn collector_rejects_future() {
    let now = Rc::new(Cell::new(1_767_225_600_000.0f64)); // 2026-01-01
    let now_c = now.clone();
    let mut c = MarketCollector::new(&scope(), 10000.0, Rc::new(move || now_c.get()), None, 100).unwrap();
    c.subscribed("INITIAL").unwrap();
    let ticket = c.begin_recovery().unwrap();
    let snapshot = json!({
        "contexts": [{
            "type": "tournament",
            "tournament": {"id": "bda92870-621e-47b0-bc3c-3602c5c26f55"},
            "orderbook": {"exchanges": [{
                "exchangeId": "1",
                "asOf": {"sequence": 5, "at": "2026-01-01T00:00:00.000Z"},
                "nextExpiryAt": null,
                "bids": [{"price": 0.4, "quantity": 3}],
                "asks": [{"price": 0.6, "quantity": 2}],
            }]},
        }],
    });
    assert!(c.complete_recovery(&ticket, &snapshot, "2026-01-01T00:00:00.100Z"));
    assert_eq!(c.state, "ready");
    // stale after 10s
    now.set(1_767_225_611_000.0);
    c.tick();
    assert_eq!(c.state, "needs_resync");
}

#[test]
fn replay_end_to_end() {
    let text = [
        json!({"format": "sig-feed-replay", "schemaVersion": 1, "observedAt": "2026-01-01T00:00:00.000Z",
            "kind": "config", "scope": scope(), "staleMs": 10000}),
        json!({"format": "sig-feed-replay", "schemaVersion": 1, "observedAt": "2026-01-01T00:00:00.100Z",
            "kind": "subscribed"}),
        json!({"format": "sig-feed-replay", "schemaVersion": 1, "observedAt": "2026-01-01T00:00:00.200Z",
            "kind": "snapshot", "payload": {"contexts": [{"type": "tournament",
                "tournament": {"id": "bda92870-621e-47b0-bc3c-3602c5c26f55"},
                "orderbook": {"exchanges": [{"exchangeId": "1",
                    "asOf": {"sequence": 5, "at": "2026-01-01T00:00:00.000Z"}, "nextExpiryAt": null,
                    "bids": [{"price": 0.4, "quantity": 3}], "asks": [{"price": 0.6, "quantity": 2}]}]}}]}}),
        json!({"format": "sig-feed-replay", "schemaVersion": 1, "observedAt": "2026-01-01T00:00:00.300Z",
            "kind": "tick"}),
    ]
    .iter()
    .map(|r| baal_service_runtime::json_text::js_stringify(r))
    .collect::<Vec<_>>()
    .join("\n");
    let result = replay(&text, None, &|| false).unwrap();
    assert_eq!(result["state"], "ready");
    assert_eq!(result["coverage"]["observedExchanges"], 1);
    assert_eq!(result["metrics"]["recoveries"], 1);
}
