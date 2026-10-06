// research/profit-sprint-20261005/identities.mjs — catalogue every cached
// Governor/Senate market in Texas/Iowa/Michigan/Alaska plus their verified
// predicate records from production-metadata; writes market-identities.json.
use crate::files::{log_json, read_bytes, read_json, runtime_dir, sha256, write_json_pretty};
use crate::js::{js_truthy, now_iso, num};
use serde_json::{json, Value as V};
use std::path::Path;

pub fn run() -> Result<V, String> {
    let runtime = runtime_dir()?;
    let bytes = read_bytes(Path::new("data/markets.json"))?;
    let markets: V = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let universe = read_json(Path::new("live-paper/price-universe.json"))?;
    let title_words = ["Texas", "Iowa", "Michigan", "Alaska"];
    let kind_words = ["Governor", "Senate"];
    let selected: Vec<V> = markets
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|m| {
            let t = m.get("title").and_then(V::as_str).unwrap_or("");
            title_words.iter().any(|w| t.contains(w)) && kind_words.iter().any(|w| t.contains(w))
        })
        .map(|m| {
            json!({
                "marketId": m["id"],
                "title": m["title"],
                "exchangeId": m["exchanges"][0]["id"],
                "settlementDate": m["settlementDate"],
                "catalogueStatus": m["status"],
            })
        })
        .collect();
    let mut predicates: Vec<V> = Vec::new();
    let meta_root = runtime.join("production-metadata");
    let mut folders: Vec<_> = std::fs::read_dir(&meta_root)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .collect();
    // fs.readdirSync returns names sorted in Node's listing order on most
    // filesystems; keep a stable explicit sort.
    folders.sort_by_key(|e| e.file_name());
    for folder in folders {
        let mut files: Vec<_> = std::fs::read_dir(folder.path())
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .collect();
        files.sort_by_key(|e| e.file_name());
        for file in files {
            let name = file.file_name().to_string_lossy().to_string();
            // /^\d+\.json$/
            let stem = name.strip_suffix(".json").unwrap_or("");
            if stem.is_empty() || !stem.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let source = read_bytes(&file.path())?;
            let j: V = serde_json::from_slice(&source).map_err(|e| e.to_string())?;
            if !selected.iter().any(|m| m["marketId"] == j["marketId"])
                || !js_truthy(j["root"].get("contract_details"))
            {
                continue;
            }
            predicates.push(json!({
                "marketId": j["marketId"],
                "predicate": j["root"]["contract_details"],
                "rootHash": j["rootHash"],
                "receivedAt": j["receivedAt"],
                "sourceFile": format!("production-metadata/{}/{name}", folder.file_name().to_string_lossy()),
                "sourceHash": sha256(&source),
            }));
        }
    }
    let mut settlement_dates: Vec<V> = Vec::new();
    for m in markets.as_array().cloned().unwrap_or_default() {
        let d = m.get("settlementDate").cloned().unwrap_or(V::Null);
        if !settlement_dates.contains(&d) {
            settlement_dates.push(d);
        }
    }
    let report = json!({
        "schemaVersion": 1,
        "at": now_iso(),
        "cachedCatalogueHash": sha256(&bytes),
        "catalogueObservedAt": universe["catalogueObservedAt"],
        "marketCount": num(markets.as_array().map(|a| a.len() as f64).unwrap_or(0.0)),
        "settlementDates": settlement_dates,
        "selected": selected,
        "predicates": predicates,
        "executionAuthority": false,
        "liveBookEvidence": false,
        "notes": [
            "Texas Senate is293/ex982 Democratic and294/ex983 Republican.181/ex870 is not that Senate market.",
            "Texas Governor195/ex884 Democratic and196/ex885 Republican. Only195 cachedroot is available here;196 fullpredicate was not reverified.",
            "Texas ballot includesPatDixonLIBGovernor andTedBrownLIBSenate; Democratic and Republican party-winner markets cannot be treated as exhaustive binary complements.",
            "Separate same-exchangeYES/NO pairing mechanism is distinct from opposite-party markets. This research report does not verify a dispatch or execution quote.",
            "All237 cachedmarkets shareNovember4 17:00UTC settlement; none is an election-outcome payout beforeOctober5 6pmEastern."
        ],
    });
    // Plain overwrite write — the JS has no 'wx' flag here.
    write_json_pretty(
        Path::new("research/profit-sprint-20261005/market-identities.json"),
        &report,
    )?;
    let out = json!({
        "markets": report["marketCount"],
        "settlementDates": report["settlementDates"],
        "predicateRecords": num(predicates.len() as f64),
    });
    log_json(&out);
    Ok(out)
}
