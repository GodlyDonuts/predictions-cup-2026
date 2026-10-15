//! Equal-condition, offline strategy comparison. Never changes the deployed bank.
use moloch_replay::{engine::Engine, legacy};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env,
    fs::{self, OpenOptions},
    io::Write,
};
fn policies(c: &Value) -> Result<Vec<(String, Engine)>, Box<dyn std::error::Error>> {
    let base = c["basePolicy"].as_object().ok_or("BASE_POLICY")?;
    let allowed = [
        "quoteMode",
        "insideTicks",
        "entryBackoffTicks",
        "minimumDepthImbalanceBps",
        "retainProfitableHedge",
        "repriceProfitableHedge",
        "requireCurrentFlow",
    ];
    let mut names = BTreeSet::new();
    let mut out = Vec::new();
    for v in c["variants"].as_array().ok_or("VARIANTS")? {
        let name = v["name"].as_str().filter(|s| !s.is_empty()).ok_or("NAME")?;
        if !names.insert(name.to_owned()) {
            return Err("DUPLICATE_NAME".into());
        }
        let mut p = Value::Object(base.clone());
        for (k, v) in v["overrides"].as_object().ok_or("OVERRIDES")? {
            if !allowed.contains(&k.as_str()) {
                return Err("COMPARISON_CONFOUNDING_OVERRIDE".into());
            }
            p[k] = v.clone();
        }
        out.push((name.to_owned(), Engine::new(&p)?));
    }
    if !(2..=16).contains(&out.len()) {
        return Err("VARIANT_COUNT".into());
    }
    Ok(out)
}
fn replay_episode(
    journal: &str,
    snapshot: &str,
    variants: &mut [(String, Engine)],
) -> Result<Value, Box<dyn std::error::Error>> {
    let terminal_bytes = fs::read(snapshot)?;
    let terminal: Value = serde_json::from_slice(&terminal_bytes)?;
    let mut terminal_body = terminal.as_object().ok_or("TERMINAL_OBJECT")?.clone();
    let terminal_hash = terminal_body.remove("hash").ok_or("TERMINAL_HASH")?;
    if terminal_hash.as_str() != Some(&legacy::hash(&Value::Object(terminal_body))?) {
        return Err("TERMINAL_SNAPSHOT_HASH".into());
    }
    if fs::metadata(journal)?.len() > 512 * 1024 * 1024 {
        return Err("SOURCE_SIZE".into());
    }
    let bytes = fs::read(journal)?;
    let mut events = Vec::new();
    let mut previous = String::new();
    let mut count = 0u64;
    for line in bytes
        .split(|b| *b == b'\n')
        .filter(|x| !x.iter().all(|b| b.is_ascii_whitespace()))
    {
        let row: Value = serde_json::from_slice(line)?;
        let mut body = row.as_object().ok_or("ROW")?.clone();
        let hash = body.remove("hash").ok_or("HASH")?;
        if body.len() != 4
            || !body.contains_key("stateHash")
            || body["sequence"].as_u64() != Some(count + 1)
            || body["previous"].as_str() != Some(&previous)
            || hash.as_str() != Some(&legacy::hash(&Value::Object(body.clone()))?)
        {
            return Err("JOURNAL_INTEGRITY".into());
        }
        previous = hash.as_str().ok_or("HASH")?.to_owned();
        count += 1;
        events.push(body["event"].clone());
    }
    if terminal["sequence"].as_u64() != Some(count)
        || terminal["lastHash"].as_str() != Some(&previous)
    {
        return Err("TERMINAL_CHAIN".into());
    }
    let first = events.first().map(|e| {
        e["processedAt"]
            .as_str()
            .or(e["receivedAt"].as_str())
            .unwrap_or("")
            .to_owned()
    });
    let last = events.last().map(|e| {
        e["processedAt"]
            .as_str()
            .or(e["receivedAt"].as_str())
            .unwrap_or("")
            .to_owned()
    });
    for event in events {
        for (_, engine) in variants.iter_mut() {
            engine.advance(&event)?;
        }
    }
    Ok(
        json!({"events":count,"from":first,"to":last,"sourceSha256":format!("{:x}",Sha256::digest(bytes)),"terminalSnapshotSha256":format!("{:x}",Sha256::digest(terminal_bytes)),"terminalHash":terminal_hash}),
    )
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let a = env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 4 {
        return Err(
            "USAGE: moloch-compare LEGACY_JOURNAL TERMINAL_SNAPSHOT CONFIG NEW_REPORT".into(),
        );
    }
    if std::path::Path::new(&a[3]).exists() {
        return Err("PRESERVE_EXISTING_REPORT".into());
    }
    let config: Value = serde_json::from_slice(&fs::read(&a[2])?)?;
    let mut variants = policies(&config)?;
    let mut sources = vec![(a[0].clone(), a[1].clone())];
    if let Some(extra) = config.get("continuationEpisodes") {
        for episode in extra.as_array().ok_or("CONTINUATION_EPISODES")? {
            sources.push((
                episode["journal"]
                    .as_str()
                    .ok_or("EPISODE_JOURNAL")?
                    .to_owned(),
                episode["snapshot"]
                    .as_str()
                    .ok_or("EPISODE_SNAPSHOT")?
                    .to_owned(),
            ));
        }
    }
    let mut episodes = Vec::new();
    let mut count = 0u64;
    for (journal, snapshot) in sources {
        let mut evidence = replay_episode(&journal, &snapshot, &mut variants)?;
        count += evidence["events"].as_u64().ok_or("EVENT_COUNT")?;
        evidence["variants"] = Value::Array(
            variants
                .iter()
                .map(|(name, engine)| Ok(json!({"name":name,"report":engine.report()?})))
                .collect::<Result<Vec<Value>, Box<dyn std::error::Error>>>()?,
        );
        episodes.push(evidence);
    }
    let mut results = Vec::new();
    for (name, engine) in variants {
        results.push(json!({"name":name,"policy":engine.policy(),"report":engine.report()?,"fillsSha256":format!("{:x}",Sha256::digest(serde_json::to_vec(engine.fills())?))}));
    }
    let result = json!({"runtime":"rust","mode":"offline_equal_condition_comparison","events":count,"episodes":episodes,"continuousWalletAcrossEpisodes":true,"sourceChainVerified":true,"processingClocksUnchanged":true,"independentDiagnosticBanks":true,"deployedWalletChanged":false,"variants":results,"selectedWinner":null,"profitabilityEstablished":false,"networkRequests":0,"contestOrders":0,"limitations":["Retrospective captured-market diagnostic; no untouched forward holdout.","Variants share capital, sizing, fees, fill model, latency, entry deadline and source clocks.","These are named Rust policy variants, not a verified reproduction of the entire Baal runtime.","Unclosed positions and hypothetical queue fills cannot establish realized profitability."]});
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&a[3])?;
    serde_json::to_writer_pretty(&mut out, &result)?;
    out.write_all(b"\n")?;
    out.sync_all()?;
    println!(
        "{}",
        json!({"events":count,"variants":results.len(),"networkRequests":0,"contestOrders":0})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("MOLOCH_COMPARE_FAILED: {e}");
        std::process::exit(1)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_capital_and_fill_models_rejected() {
        for k in [
            "initialCash",
            "quantity",
            "fillModel",
            "latencyMs",
            "feeBps",
            "entryStopAt",
        ] {
            let mut c = json!({"basePolicy":{},"variants":[{"name":"a","overrides":{}},{"name":"b","overrides":{}}]});
            c["variants"][1]["overrides"][k] = json!(1);
            assert!(policies(&c).is_err());
        }
    }
    #[test]
    fn strategy_variation_allowed_but_names_unique() {
        let mut c = json!({"basePolicy":{},"variants":[{"name":"paired","overrides":{}},{"name":"micro","overrides":{"quoteMode":"microprice"}}]});
        assert!(policies(&c).is_ok());
        c["variants"][1]["name"] = json!("paired");
        assert!(policies(&c).is_err());
    }
}

#[cfg(test)]
mod continuation_tests {
    use super::*;
    fn sealed_episode(root: &std::path::Path, name: &str, events: &[Value]) -> (String, String) {
        let mut previous = String::new();
        let mut data = String::new();
        for (i, event) in events.iter().enumerate() {
            let mut row =
                json!({"sequence":i+1,"previous":previous,"event":event,"stateHash":"fixture"});
            previous = legacy::hash(&row).unwrap();
            row["hash"] = json!(previous);
            data.push_str(&serde_json::to_string(&row).unwrap());
            data.push('\n');
        }
        let journal = root.join(format!("{name}.jsonl"));
        let snapshot = root.join(format!("{name}.json"));
        let mut terminal = json!({"sequence":events.len(),"lastHash":previous,"state":{}});
        terminal["hash"] = json!(legacy::hash(&terminal).unwrap());
        fs::write(&journal, data).unwrap();
        fs::write(&snapshot, serde_json::to_vec(&terminal).unwrap()).unwrap();
        (
            journal.to_str().unwrap().into(),
            snapshot.to_str().unwrap().into(),
        )
    }
    #[test]
    fn inventory_fees_and_pnl_carry_between_sealed_episodes() {
        let root = env::temp_dir().join(format!("moloch-carry-test-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let corpus: Value =
            serde_json::from_str(include_str!("../../tests/reference/golden.json")).unwrap();
        let scenario = &corpus[0];
        let events: Vec<Value> = scenario["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["event"].clone())
            .collect();
        let a = sealed_episode(&root, "a", &events[..2]);
        let b = sealed_episode(&root, "b", &events[2..]);
        let mut variants = vec![("paired".into(), Engine::new(&scenario["policy"]).unwrap())];
        replay_episode(&a.0, &a.1, &mut variants).unwrap();
        let first = variants[0].1.report().unwrap();
        assert_eq!(first["ownedCost"], "0.450000");
        assert_eq!(first["cash"], "999.549900");
        replay_episode(&b.0, &b.1, &mut variants).unwrap();
        let final_report = variants[0].1.report().unwrap();
        assert_eq!(final_report["cash"], "1000.004800");
        assert_eq!(final_report["realized"], "0.004800");
        assert_eq!(final_report["events"], 3);
        let mut corrupt: Value = serde_json::from_slice(&fs::read(&b.1).unwrap()).unwrap();
        corrupt["lastHash"] = json!("wrong");
        fs::write(&b.1, serde_json::to_vec(&corrupt).unwrap()).unwrap();
        assert!(replay_episode(&b.0, &b.1, &mut variants).is_err());
        assert_eq!(variants[0].1.report().unwrap(), final_report);
        fs::remove_dir_all(root).unwrap();
    }
}
