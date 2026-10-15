use moloch_replay::source::RestSource;
use serde_json::Value;
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
