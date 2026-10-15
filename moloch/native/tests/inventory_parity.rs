use moloch_replay::engine::Engine;
use serde_json::Value;
#[test]
fn seeded_balance_and_liabilities_cannot_be_forged() {
    let corpus: Value =
        serde_json::from_str(include_str!("reference/inventory-golden.json")).unwrap();
    let mut seed = corpus
        .as_array()
        .unwrap()
        .iter()
        .find(|s| !s["seed"]["fills"].as_array().unwrap().is_empty())
        .unwrap()["seed"]
        .clone();
    assert!(Engine::from_seed_state(&seed).is_ok());
    let original = seed.clone();
    seed["cash"] = serde_json::json!(moloch_replay::money(
        moloch_replay::amount(seed["cash"].as_str().unwrap()).unwrap() + 5000
    ));
    assert!(Engine::from_seed_state(&seed).is_err());
    seed = original;
    seed["orders"] = serde_json::json!([{"retired":false}]);
    assert!(Engine::from_seed_state(&seed).is_err());
}
#[test]
fn policy_change_cannot_reset_initial_capital_or_erase_loss() {
    let corpus: Value =
        serde_json::from_str(include_str!("reference/inventory-golden.json")).unwrap();
    let seed = &corpus
        .as_array()
        .unwrap()
        .iter()
        .find(|s| !s["seed"]["fills"].as_array().unwrap().is_empty())
        .unwrap()["seed"];
    let mut engine = Engine::from_seed_state(seed).unwrap();
    let original = engine.report().unwrap();
    let mut policy = seed["policy"].clone();
    policy["initialCash"] = serde_json::json!("2000.000000");
    assert!(engine.update_policy(&policy).is_err());
    assert_eq!(engine.report().unwrap(), original);
}
#[test]
fn original_engine_paths_preserve_financial_orders_and_fills() {
    let corpus: Value =
        serde_json::from_str(include_str!("reference/inventory-golden.json")).unwrap();
    for scenario in corpus.as_array().unwrap() {
        let mut engine = Engine::from_seed_state(&scenario["seed"])
            .unwrap_or_else(|e| panic!("{} seed: {e}", scenario["name"]));
        for (i, step) in scenario["steps"].as_array().unwrap().iter().enumerate() {
            engine
                .update_policy(&step["policy"])
                .unwrap_or_else(|e| panic!("{} step {i} policy: {e}", scenario["name"]));
            engine
                .advance(&step["event"])
                .unwrap_or_else(|e| panic!("{} step {i} event: {e}", scenario["name"]));
            let actual = engine.report().unwrap();
            for (field, expected) in step["expected"].as_object().unwrap() {
                assert_eq!(
                    &actual[field], expected,
                    "{} step {i} field {field}",
                    scenario["name"]
                )
            }
            assert_eq!(
                serde_json::json!(engine.fills()),
                step["fillRecords"],
                "{} step {i} fill records",
                scenario["name"]
            );
            assert_eq!(
                serde_json::json!(engine.order_records()),
                step["orderRecords"],
                "{} step {i} order records",
                scenario["name"]
            );
        }
    }
}
