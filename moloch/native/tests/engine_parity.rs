use moloch_replay::engine::Engine;
use serde_json::Value;
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
