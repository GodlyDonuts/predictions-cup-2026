// data/inspect-nh-settlement-spread-20261005.mjs — structural settlement
// spread diagnostic on the captured NH contract pair.
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn write_wx(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, body).map_err(|e| e.to_string())
}
fn parse_iso(s: &str) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(s).unwrap_or(f64::NAN)
}

pub fn run() -> Result<V, String> {
    let dir = localappdata()?.join(
        "SigTradingCup/paper-runtime/production-metadata/nh-settlement-spread-20261005-v1");
    let mut legs: Vec<V> = Vec::new();
    for market_id in ["381", "382"] {
        let files = [format!("{}.json", market_id), format!("{}.book.json", market_id)];
        let mut bytes: Vec<Vec<u8>> = Vec::new();
        for f in &files {
            bytes.push(std::fs::read(dir.join(f)).map_err(|e| e.to_string())?);
        }
        let node: V = serde_json::from_slice(&bytes[0]).map_err(|e| e.to_string())?;
        let receipt: V = serde_json::from_slice(&bytes[1]).map_err(|e| e.to_string())?;
        legs.push(json!({"marketId": market_id,
            "exchangeId": receipt["book"]["exchangeId"],
            "contract": node["root"],
            "contractReceivedAt": node["receivedAt"],
            "requestStartedAt": receipt["book"]["requestStartedAt"],
            "receivedAt": receipt["book"]["receivedAt"],
            "topBid": receipt["book"]["bids"][0],
            "topAsk": receipt["book"]["asks"][0],
            "sources": files.iter().enumerate().map(|(i, file)| json!({
                "file": file, "sha256": sha256(&bytes[i])})).collect::<Vec<_>>()}));
    }
    let a = &legs[0]["contract"]["contract_details"];
    let b = &legs[1]["contract"]["contract_details"];
    let common_predicate = ["raceId", "stageId", "usState", "raceName", "raceStage",
        "officeLevel", "contractType", "electionDate", "resolutionType"].iter()
        .all(|k| a[k] == b[k]);
    if !common_predicate || a["winnerName"] == b["winnerName"]
        || legs.iter().any(|l| !l["contract"]["settled_with"].is_null()) {
        return Err("CONTRACT_PAIR_CHANGED".into());
    }
    let scale = num_bigint::BigInt::from(baal_paper::fixed::SCALE);
    let tick = num_bigint::BigInt::from(baal_paper::fixed::TICK);
    let zero = num_bigint::BigInt::from(0);
    let no_cost = legs.iter().try_fold(zero.clone(), |sum, l|
        -> Result<num_bigint::BigInt, String> {
        Ok(sum + &scale
            - baal_paper::fixed::big_price(l["topBid"]["price"].as_str().unwrap_or(""))?
            + &tick)
    })?;
    let yes_cost = legs.iter().try_fold(zero, |sum, l|
        -> Result<num_bigint::BigInt, String> {
        Ok(sum + baal_paper::fixed::big_price(l["topAsk"]["price"].as_str().unwrap_or(""))?
            + &tick)
    })?;
    let window_ms = legs.iter()
        .map(|l| parse_iso(l["receivedAt"].as_str().unwrap_or("")))
        .fold(f64::NEG_INFINITY, f64::max)
        - legs.iter().map(|l| parse_iso(l["requestStartedAt"].as_str().unwrap_or("")))
            .fold(f64::INFINITY, f64::min);
    let out = json!({"schemaVersion": 1, "at": now_iso(),
        "mode": "scoped_structural_settlement_price_diagnostic",
        "legs": legs, "commonPredicate": common_predicate,
        "distinctPartyPredicates": true, "fees": "0.000000",
        "entrySlippagePerLeg": "0.005000",
        "indicativeTwoNoCost": baal_paper::fixed::money(&no_cost),
        "conditionalOrdinaryTwoNoMinimumPayout": "1.000000",
        "indicativeConditionalNoSurplus": baal_paper::fixed::money(&(scale - &no_cost)),
        "indicativeTwoYesCost": baal_paper::fixed::money(&yes_cost),
        "requestWindowMs": window_ms,
        "cofreshExecutableBooks": window_ms <= 10000.0,
        "reviewedRelationshipStillCurrent": false, "executionAuthority": false,
        "competitionOrders": 0, "profitabilityEstablished": false,
        "limitations": [
            "The exact contract predicates match, but ordinary mutually exclusive settlement is conditional; administrative overrides and other reviewed exceptions are not excluded.",
            "These auxiliary observations are not cofresh execution evidence. No dispatch or package fill is inferred.",
            "Refund returns the held refundable cost rather than a fixed face-value payout. Asymmetric cancellation and unmatched legs can lose principal.",
            "The older ballot/review attestation is expired; fresh API nodes alone do not renew it."],
        "primaryRules": ["https://sig.thesuper.market/docs/settlement-and-payouts",
            "https://sig.thesuper.market/docs/markets-and-trading"]});
    write_wx(Path::new("data/nh-settlement-spread-diagnostic-20261005.json"),
        format!("{}\n", serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?).as_bytes())?;
    Ok(json!({"noCost": out["indicativeTwoNoCost"],
        "noSurplus": out["indicativeConditionalNoSurplus"],
        "yesCost": out["indicativeTwoYesCost"], "requestWindowMs": window_ms,
        "cofreshExecutableBooks": out["cofreshExecutableBooks"],
        "competitionOrders": 0}))
}
