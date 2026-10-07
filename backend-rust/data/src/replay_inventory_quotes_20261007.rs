// data/replay-inventory-quotes-20261007.mjs — inventory quote wallet replay
// over seen capture. Counterfactual only; no execution authority.
use baal_collector::book_publication_v2::{apply_book_publication, create_book_publication_state};
use baal_collector::native_dip_host_v1::sealed_native_prefix;
use baal_paper::inventory_quote_wallet_v1::{
    advance_inventory_quote_wallet_v1, create_inventory_quote_wallet_v1,
    inventory_quote_wallet_report_v1,
};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Map, Value as V};
use std::fs;
use std::path::{Path, PathBuf};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default())
        .join("SigTradingCup")
        .join("paper-runtime")
}

pub fn run() -> Result<V, String> {
    let begun = std::time::Instant::now();
    let file = rt().join("public-books.jsonl");
    let seal = sealed_native_prefix(&file)?;
    let base = json!({
        "cutoff": "2026-10-06T00:00:00.000Z", "initialCash": "100.000000",
        "maximumCommitted": "25.000000", "maximumMarkets": 12,
        "maximumOrderCost": "5.000000", "quantity": 1, "latencyMs": 3000,
        "lifetimeMs": 120000, "maximumGapMs": 120000, "minimumSamples": 4,
        "minimumHistoryMs": 120000, "depthFractionBps": 100, "horizonSeconds": 300,
    });
    let policies: Vec<V> = [0i64, 5, 10, 20]
        .iter()
        .map(|ra| {
            let mut m = base.as_object().unwrap().clone();
            m.insert("riskAversion".into(), json!(ra));
            m.insert("id".into(), json!(format!("inventory-risk-{ra}")));
            V::Object(m)
        })
        .collect();
    let mut wallets: Vec<V> = policies
        .iter()
        .map(create_inventory_quote_wallet_v1)
        .collect::<Result<_, _>>()?;

    let bytes = fs::read(&file).map_err(|e| e.to_string())?;
    let prefix = &bytes[..(seal.bytes as usize).min(bytes.len())];
    let source_sha = sha256(prefix);
    let text = String::from_utf8_lossy(prefix).into_owned();
    let cutoff_ms = parse_iso_ms("2026-10-06T00:00:00.000Z").unwrap_or(f64::NAN);
    let mut state: Option<V> = None;
    let mut processed = 0i64;
    let mut first_at = V::Null;
    for line in text.trim_end().split('\n') {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if state.is_none() {
            state = Some(create_book_publication_state(get(
                get(&row, "payload"),
                "scope",
            ))?);
        }
        state = Some(apply_book_publication(state.as_ref().unwrap(), &row)?);
        if parse_iso_ms(get(&row, "at").as_str().unwrap_or("")).unwrap_or(f64::NAN) < cutoff_ms {
            continue;
        }
        if first_at.is_null() {
            first_at = get(&row, "at").clone();
        }
        let event = if get(&row, "kind").as_str() == Some("book") {
            json!({
                "kind": "book", "at": get(&row, "at").clone(),
                "book": get(get(&row, "payload"), "book").clone(),
                "sequence": get(&row, "sequence").clone(),
                "hash": get(&row, "hash").clone(),
            })
        } else {
            json!({
                "kind": "boundary", "at": get(&row, "at").clone(),
                "marketId": get(get(&row, "payload"), "marketId").clone(),
                "sequence": get(&row, "sequence").clone(),
                "hash": get(&row, "hash").clone(),
            })
        };
        for (i, w) in wallets.iter_mut().enumerate() {
            advance_inventory_quote_wallet_v1(w, &event, &policies[i])?;
        }
        processed += 1;
    }
    let st = fs::metadata(&file).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    let (dev, ino) = {
        use std::os::unix::fs::MetadataExt;
        (st.dev(), st.ino())
    };
    #[cfg(not(unix))]
    let (dev, ino) = (seal.dev, seal.ino);
    if ino != seal.ino || dev != seal.dev || st.len() < seal.bytes {
        return Err("INVENTORY_REPLAY_SOURCE_CHANGED".into());
    }
    let state = state.unwrap_or(V::Null);
    let cohorts: Vec<V> = policies
        .iter()
        .zip(wallets.iter())
        .map(|(policy, w)| {
            Ok(json!({
                "policy": policy.clone(),
                "report": inventory_quote_wallet_report_v1(w, policy)?,
                "fills": get(w, "fills").clone(),
                "redemptions": get(w, "redemptions").clone(),
            }))
        })
        .collect::<Result<Vec<V>, String>>()?;
    let output = json!({
        "schemaVersion": 1,
        "createdAt": ms_to_iso(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_millis() as f64
        ),
        "mode": "inventory_quote_seen_capture_development_replay_v1",
        "source": {
            "bytes": seal.bytes,
            "sha256": source_sha,
            "records": get(&state, "sequence").clone(),
            "chainHash": get(&state, "hash").clone(),
            "through": get(&state, "lastAt").clone(),
            "fullSemanticReplay": true,
        },
        "firstAt": first_at,
        "processed": processed,
        "elapsedMs": begun.elapsed().as_secs_f64() * 1000.0,
        "cohorts": cohorts,
        "networkRequests": 0,
        "competitionOrders": 0,
        "profitabilityEstablished": false,
        "limitations": [
            "Previously seen development captures, not held-out or prospective evidence.",
            "Strict later-book through fills are counterfactual; aggressor flow and queue priority are not proven.",
            "Complementary redemption models documented normal netting; this is not contest execution.",
            "Censored quote reservations and remaining inventory are not reset or counted as realized profit."
        ],
    });
    fs::write(
        "data/inventory-quote-replay-20261007.json",
        serde_json::to_string_pretty(&output).map_err(|e| e.to_string())? + "\n",
    )
    .map_err(|e| e.to_string())?;
    let summary: Vec<V> = get(&output, "cohorts")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|x| {
            let mut m = Map::new();
            m.insert("id".into(), get(get(x, "policy"), "id").clone());
            for (k, v) in get(x, "report").as_object().cloned().unwrap_or_default() {
                m.insert(k, v);
            }
            V::Object(m)
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string(&json!({
            "processed": processed,
            "elapsedMs": get(&output, "elapsedMs").clone(),
            "cohorts": summary,
        }))
        .map_err(|e| e.to_string())?
    );
    Ok(output)
}
