// live-paper/training-options.mjs — CLI training option parser.
use serde_json::{json, Value as V};
use std::collections::BTreeMap;

/// trainingOptions(argv)
pub fn training_options(argv: &[String]) -> Result<V, String> {
    let value_flags = ["--journal","--config","--out","--family","--history-policy","--since","--target-policy","--training-scope","--horizons"];
    let switches = ["--require-instrument-coverage","--allow-undercovered-exploration","--breadth-context"];
    let mut args: BTreeMap<String, V> = BTreeMap::new();
    let mut i = 0;
    while i < argv.len() {
        let flag = argv[i].as_str();
        if (!value_flags.contains(&flag) && !switches.contains(&flag)) || args.contains_key(flag) {
            return Err(format!("Unknown or repeated training option: {}", flag));
        }
        if switches.contains(&flag) { args.insert(flag.into(), json!(true)); i += 1; }
        else {
            i += 1;
            if i >= argv.len() || argv[i].is_empty() || argv[i].starts_with("--") {
                return Err(format!("Required value for {}", flag));
            }
            args.insert(flag.into(), json!(argv[i].clone()));
            i += 1;
        }
    }
    for flag in ["--journal", "--config", "--out"] {
        if !args.contains_key(flag) { return Err(format!("Required {}", flag)); }
    }
    if args.contains_key("--require-instrument-coverage") && args.contains_key("--allow-undercovered-exploration") {
        return Err("Conflicting coverage options".into());
    }
    let target = crate::roundtrip_target::target_policy(
        args.get("--target-policy").and_then(|v| v.as_str()).unwrap_or(crate::roundtrip_target::ROUND_TRIP))?;
    let coverage = !args.contains_key("--allow-undercovered-exploration");
    let scope = args.get("--training-scope").and_then(|v| v.as_str()).map(String::from)
        .unwrap_or_else(|| if target == crate::roundtrip_target::ROUND_TRIP && coverage { "supported-instruments-v1".into() } else { "all-markets-v1".into() });
    if !["all-markets-v1","supported-instruments-v1"].contains(&scope.as_str())
        || (scope == "supported-instruments-v1" && (!coverage || target == crate::roundtrip_target::LEGACY_TARGET)) {
        return Err("Supported scope requires coverage-gated roundtrip target".into());
    }
    let family = args.get("--family").and_then(|v| v.as_str()).unwrap_or("ridge");
    if !["ridge","boosted_stumps"].contains(&family) { return Err("Unsupported model family".into()); }
    let since = args.get("--since").and_then(|v| v.as_str()).map(String::from);
    if let Some(s) = &since {
        if baal_service_runtime::contract::parse_iso_ms(s).is_none() { return Err("Invalid training window start".into()); }
    }
    let mut options = json!({"family": family, "targetPolicy": target,
        "historyPolicy": crate::history_policy::history_policy(
            args.get("--history-policy").and_then(|v| v.as_str()).unwrap_or(crate::history_policy::ISOLATED))?,
        "since": since, "requireInstrumentCoverage": coverage,
        "breadthContext": args.contains_key("--breadth-context"), "trainingScope": scope});
    if let Some(h) = args.get("--horizons").and_then(|v| v.as_str()) {
        options["horizons"] = json!(crate::training_horizons::parse_training_horizons(h)?);
    }
    Ok(json!({"journal": args["--journal"], "config": args["--config"], "out": args["--out"], "options": options}))
}
