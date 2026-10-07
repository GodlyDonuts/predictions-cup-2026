// data/analyze-official-entry-horizons-20261007.mjs — offline original-source
// horizon diagnostic over actual official entries. No account calls, fills,
// portfolio adoption, or order authority.
use baal_collector::book_publication_v2::{apply_book_publication, create_book_publication_state};
use baal_collector::native_dip_host_v1::sealed_native_prefix;
use baal_service_runtime::fixed::{amount, money};
use baal_service_runtime::json_text::{js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::baal_generic_no_controller_v9::{
    baal_generic_no_controller_v9_identity, replay_baal_generic_no_journal_v9,
};
use crate::official_entry_horizon_v1::entry_horizon_observation_v1;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default())
        .join("SigTradingCup")
        .join("paper-runtime")
}
const HORIZONS: [i64; 5] = [5, 15, 30, 60, 120];

pub fn run() -> Result<V, String> {
    let begun = std::time::Instant::now();
    let root = rt();
    let r: V = serde_json::from_str(
        &fs::read_to_string(root.join("baal-expansion-live-request-v5.json"))
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let actor_file = root
        .join("baal-generic-live-runs")
        .join(get(&r, "runId").as_str().unwrap_or(""))
        .join(get(&r, "epoch").as_str().unwrap_or(""))
        .join("controller")
        .join("baal-generic-no-v9-journal.jsonl");
    let actor = fs::read_to_string(&actor_file).map_err(|e| e.to_string())?;
    let genesis: V = serde_json::from_str(actor.split('\n').next().unwrap_or(""))
        .map_err(|e| e.to_string())?;
    let custody_seed = get(get(get(&genesis, "event"), "input"), "custodySeed").clone();
    let current = replay_baal_generic_no_journal_v9(
        &actor,
        &baal_generic_no_controller_v9_identity()?,
        &custody_seed,
    )?;

    let mut orders: Vec<V> = get(&custody_seed, "inheritedClosedOrders")
        .as_array()
        .cloned()
        .unwrap_or_default();
    orders.extend(
        get(get(&current, "state"), "orders")
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    let mut seen: HashSet<String> = HashSet::new();
    let mut entries: Vec<V> = Vec::new();
    for o in &orders {
        let action = get(get(&o, "body"), "action").as_str().unwrap_or("");
        let proof = get(&o, "proof");
        if action != "buy"
            || !js_truthy(Some(get(proof, "filledQuantity")))
            || seen.contains(get(&o, "orderId").as_str().unwrap_or(""))
        {
            continue;
        }
        seen.insert(get(&o, "orderId").as_str().unwrap_or("").to_string());
        let fills = get(proof, "fills").as_array().cloned().unwrap_or_default();
        entries.push(json!({
            "runId": get(proof, "runId").clone(),
            "marketId": get(proof, "marketId").clone(),
            "exchangeId": get(proof, "exchangeId").clone(),
            "orderId": get(&o, "orderId").clone(),
            "quantity": get(proof, "filledQuantity").clone(),
            "cost": get(proof, "scopedPrincipalCost").clone(),
            "fillAt": get(fills.last().unwrap_or(&V::Null), "filledAt").clone(),
            "proofHash": get(proof, "verificationHash").clone(),
            "conditionalGap": get(get(&o, "proposal"), "conditionalOrdinaryGap").clone(),
            "horizons": {},
        }));
    }

    let file = root.join("public-books.jsonl");
    let seal = sealed_native_prefix(&file)?;
    let bytes = fs::read(&file).map_err(|e| e.to_string())?;
    let prefix = &bytes[..(seal.bytes as usize).min(bytes.len())];
    let source_sha = sha256(prefix);
    let text = String::from_utf8_lossy(prefix).into_owned();
    let mut state: Option<V> = None;
    let mut rows = 0i64;
    let mut boundaries = 0i64;
    for line in text.trim_end().split('\n') {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if state.is_none() {
            state = Some(create_book_publication_state(get(
                get(&row, "payload"),
                "scope",
            ))?);
        }
        state = Some(apply_book_publication(state.as_ref().unwrap(), &row)?);
        rows += 1;
        if get(&row, "kind").as_str().unwrap_or("") != "book" {
            boundaries += 1;
            continue;
        }
        let b = get(get(&row, "payload"), "book");
        for e in entries.iter_mut() {
            if get(e, "exchangeId") != get(b, "exchangeId") {
                continue;
            }
            for h in HORIZONS {
                if !get(get(e, "horizons"), &h.to_string()).is_null() {
                    continue;
                }
                let result = entry_horizon_observation_v1(&json!({
                    "entry": e.clone(),
                    "book": b.clone(),
                    "horizonMinutes": h,
                }))?;
                if !result.is_null() {
                    let mut m = result.as_object().cloned().unwrap_or_default();
                    m.insert("sourceSequence".into(), get(&row, "sequence").clone());
                    m.insert("sourceHash".into(), get(&row, "hash").clone());
                    e.as_object_mut()
                        .unwrap()
                        .get_mut("horizons")
                        .unwrap()
                        .as_object_mut()
                        .unwrap()
                        .insert(h.to_string(), V::Object(m));
                }
            }
        }
    }
    let st = fs::metadata(&file).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    let (dev, ino) = {
        use std::os::unix::fs::MetadataExt;
        (st.dev(), st.ino())
    };
    #[cfg(not(unix))]
    let (dev, ino) = (seal.dev, seal.ino);
    if dev != seal.dev || ino != seal.ino || st.len() < seal.bytes {
        return Err("HORIZON_SOURCE_REPLACED".into());
    }

    let mut aggregate = Map::new();
    for h in HORIZONS {
        let key = h.to_string();
        let has: Vec<&V> = entries
            .iter()
            .filter(|e| !get(get(e, "horizons"), &key).is_null())
            .collect();
        let available: Vec<&&V> = has
            .iter()
            .filter(|e| {
                js_truthy(Some(get(
                    get(get(get(e, "horizons"), &key), "oneTickStress"),
                    "complete",
                )))
            })
            .collect();
        let sum = available.iter().try_fold(0i128, |n, e| {
            Ok::<i128, String>(
                n + amount(get(
                    get(get(get(e, "horizons"), &key), "oneTickStress"),
                    "unrealizedPnl",
                )
                .as_str()
                .unwrap_or(""))?,
            )
        })?;
        aggregate.insert(
            key.clone(),
            json!({
                "entries": entries.len() as i64,
                "observed": has.len() as i64,
                "fullDepthObserved": available.len() as i64,
                "unavailable": entries.len() as i64 - has.len() as i64,
                "sumSnapshotPnl": money(sum),
                "positive": available.iter().filter(|e| {
                    amount(get(get(get(get(e, "horizons"), &key), "oneTickStress"), "unrealizedPnl").as_str().unwrap_or(""))
                        .unwrap_or(0) > 0
                }).count() as i64,
                "negative": available.iter().filter(|e| {
                    amount(get(get(get(get(e, "horizons"), &key), "oneTickStress"), "unrealizedPnl").as_str().unwrap_or(""))
                        .unwrap_or(0) < 0
                }).count() as i64,
            }),
        );
    }
    let state = state.unwrap_or(V::Null);
    let report = json!({
        "schemaVersion": 1,
        "createdAt": baal_service_runtime::contract::ms_to_iso(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_millis() as f64
        ),
        "mode": "actual_official_entries_future_depth_diagnostic_v1",
        "source": {
            "bytes": seal.bytes,
            "sha256": source_sha,
            "records": rows,
            "chainHash": get(&state, "hash").clone(),
            "through": get(&state, "lastAt").clone(),
            "fullSemanticReplay": true,
            "boundaries": boundaries,
        },
        "actorAnchor": get(&current, "anchor").clone(),
        "entries": entries,
        "aggregate": aggregate,
        "elapsedMs": begun.elapsed().as_secs_f64() * 1000.0,
        "networkRequests": 0,
        "competitionOrders": 0,
        "actualSalesSimulated": 0,
        "fees": "0.000000",
        "profitabilityEstablished": false,
        "limitations": [
            "Already seen development captures; not held-out evidence.",
            "Future snapshot values are not actual exits or a funded portfolio replay.",
            "Five-minute timing tolerance and receipt gaps remain explicit; continuous path/queue execution is not proven.",
            "Ordinary package payout gap does not establish first-leg direction or short-horizon recovery."
        ],
    });
    fs::write(
        "data/official-entry-horizons-20261007.json",
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n",
    )
    .map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string(&json!({
            "entries": get(&report, "entries").as_array().map(|a| a.len() as i64).unwrap_or(0),
            "sourceRecords": rows,
            "elapsedMs": get(&report, "elapsedMs").clone(),
            "aggregate": get(&report, "aggregate").clone(),
        }))
        .map_err(|e| e.to_string())?
    );
    Ok(report)
}
