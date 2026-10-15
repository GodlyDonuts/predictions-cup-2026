use moloch_replay::engine::Engine;
use serde_json::Value;
#[test]
fn expired_quote_recovery_requests_only_its_original_window() {
    let corpus: Value = serde_json::from_str(include_str!("reference/golden.json")).unwrap();
    let mut engine = Engine::new(&corpus[0]["policy"]).unwrap();
    engine.advance(&corpus[0]["steps"][0]["event"]).unwrap();
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-15T00:00:00Z")
        .unwrap()
        .timestamp_millis();
    let (from, to) = engine.expired_recovery_window("101", now).unwrap().unwrap();
    assert_eq!(from, "2026-10-13T00:00:03.000Z");
    assert_eq!(to, "2026-10-13T00:02:00.000Z");
    assert!(engine.required_exchanges().contains("101"));
}
#[test]
fn identical_events_match_reference_at_every_checkpoint() {
    let corpus: Value = serde_json::from_str(include_str!("reference/golden.json")).unwrap();
    for scenario in corpus.as_array().unwrap() {
        let mut engine = Engine::new(&scenario["policy"]).unwrap();
        for (i, step) in scenario["steps"].as_array().unwrap().iter().enumerate() {
            engine
                .advance(&step["event"])
                .unwrap_or_else(|e| panic!("{} event {i}: {e}", scenario["name"]));
            let actual = engine.report().unwrap();
            for (field, expected) in step["expected"].as_object().unwrap() {
                assert_eq!(
                    &actual[field], expected,
                    "{} event {i} field {field}",
                    scenario["name"]
                );
            }
        }
    }
}
#[test]
fn rejected_event_does_not_mutate_accounting() {
    let corpus: Value = serde_json::from_str(include_str!("reference/golden.json")).unwrap();
    let s = &corpus[0];
    let mut engine = Engine::new(&s["policy"]).unwrap();
    engine.advance(&s["steps"][0]["event"]).unwrap();
    let old = engine.report().unwrap();
    let mut bad = s["steps"][1]["event"].clone();
    bad["quantity"] = serde_json::json!(-1);
    assert!(engine.advance(&bad).is_err());
    assert_eq!(old, engine.report().unwrap());
}
