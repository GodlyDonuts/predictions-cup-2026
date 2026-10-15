//! Equal-condition, offline strategy comparison. Never changes the deployed bank.
use moloch_replay::{engine::Engine, legacy};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
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
    let terminal_bytes = fs::read(&a[1])?;
    let terminal: Value = serde_json::from_slice(&terminal_bytes)?;
    let mut terminal_body = terminal.as_object().ok_or("TERMINAL_OBJECT")?.clone();
    let terminal_hash = terminal_body.remove("hash").ok_or("TERMINAL_HASH")?;
    if terminal_hash.as_str() != Some(&legacy::hash(&Value::Object(terminal_body))?) {
        return Err("TERMINAL_SNAPSHOT_HASH".into());
    }
    let mut variants = policies(&config)?;
    let mut previous = String::new();
    let mut count = 0u64;
    let mut digest = Sha256::new();
    let mut input = BufReader::new(File::open(&a[0])?);
    let mut line = Vec::new();
    while input.read_until(b'\n', &mut line)? > 0 {
        digest.update(&line);
        if !line.iter().all(|b| b.is_ascii_whitespace()) {
            let row: Value = serde_json::from_slice(&line)?;
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
            for (_, engine) in &mut variants {
                engine.advance(&body["event"])?;
            }
            previous = hash.as_str().ok_or("HASH")?.to_owned();
            count += 1;
        }
        line.clear();
    }
    if terminal["sequence"].as_u64() != Some(count)
        || terminal["lastHash"].as_str() != Some(&previous)
    {
        return Err("TERMINAL_CHAIN".into());
    }
    let mut results = Vec::new();
    for (name, engine) in variants {
        results.push(json!({"name":name,"policy":engine.policy(),"report":engine.report()?,"fillsSha256":format!("{:x}",Sha256::digest(serde_json::to_vec(engine.fills())?))}));
    }
    let result = json!({"runtime":"rust","mode":"offline_equal_condition_comparison","events":count,"sourceSha256":format!("{:x}",digest.finalize()),"terminalSnapshotSha256":format!("{:x}",Sha256::digest(terminal_bytes)),"sourceChainVerified":true,"processingClocksUnchanged":true,"independentDiagnosticBanks":true,"deployedWalletChanged":false,"variants":results,"selectedWinner":null,"profitabilityEstablished":false,"networkRequests":0,"contestOrders":0,"limitations":["Retrospective captured-market diagnostic; no untouched forward holdout.","Variants share capital, sizing, fees, fill model, latency, entry deadline and source clocks.","These are named Rust policy variants, not a verified reproduction of the entire Baal runtime.","Unclosed positions and hypothetical queue fills cannot establish realized profitability."]});
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
