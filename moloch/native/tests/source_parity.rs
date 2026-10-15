use moloch_replay::source::RestSource;
use serde_json::Value;
#[test]
fn restart_preserves_verified_native_precision_for_rest_duplicates() {
    let corpus: Value =
        serde_json::from_str(include_str!("reference/realtime-golden.json")).unwrap();
    let c = &corpus["config"];
    let mut source = RestSource::new(
        &c["pairs"],
        c["tournamentId"].as_str().unwrap(),
        "2026-10-14T00:01:05Z",
    )
    .unwrap();
    let mut fact = corpus["steps"][4]["expected"][0].clone();
    fact["idNamespace"] = serde_json::json!("sig_engine_match");
    fact["executedAt"] = serde_json::json!("2026-10-14T00:01:01.000123456Z");
    source.seed_trade_facts(vec![fact.clone()]).unwrap();
    let events = source
        .accept(&corpus["steps"][4]["input"]["receipt"])
        .unwrap();
    let trade = events.iter().find(|e| e["type"] == "trade").unwrap();
    assert_eq!(trade["executedAt"], fact["executedAt"]);
    assert_eq!(trade["originalExecutedAt"], "2026-10-14T00:01:01.000Z");
}
#[test]
fn realtime_gap_recovery_and_precision_match_original() {
    let corpus: Value =
        serde_json::from_str(include_str!("reference/realtime-golden.json")).unwrap();
    let c = &corpus["config"];
    let mut source = RestSource::new(
        &c["pairs"],
        c["tournamentId"].as_str().unwrap(),
        c["startedAt"].as_str().unwrap(),
    )
    .unwrap();
    for (i, step) in corpus["steps"].as_array().unwrap().iter().enumerate() {
        let input = &step["input"];
        let actual = match input["kind"].as_str().unwrap() {
            "rest" => source.accept(&input["receipt"]),
            "subscribed" => source.subscribed(input["receivedAt"].as_str().unwrap()),
            "broadcast" => source.broadcast(
                input["event"].as_str().unwrap(),
                &input["payload"],
                input["receivedAt"].as_str().unwrap(),
            ),
            _ => panic!("fixture type"),
        }
        .unwrap_or_else(|e| panic!("step {i}: {e}"));
        assert_eq!(
            serde_json::json!(actual),
            step["expected"],
            "realtime source step {i}"
        );
    }
}
#[test]
fn complete_receipt_windows_match_original_source() {
    let corpus: Value = serde_json::from_str(include_str!("reference/rest-golden.json")).unwrap();
    for scenario in corpus.as_array().unwrap() {
        let c = &scenario["config"];
        let mut s = RestSource::new(
            &c["pairs"],
            c["tournamentId"].as_str().unwrap(),
            c["startedAt"].as_str().unwrap(),
        )
        .unwrap();
        for (i, step) in scenario["steps"].as_array().unwrap().iter().enumerate() {
            let actual = s.accept(&step["receipt"]).unwrap();
            assert_eq!(
                serde_json::json!(actual),
                step["expected"],
                "{} receipt {i}",
                scenario["name"]
            );
        }
    }
}
