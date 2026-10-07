// data/replay-inventory-tape-20261007.mjs — strict tape-through wallet replay
// over books + anonymous trades. Counterfactual only.
use baal_collector::book_publication_v2::{apply_book_publication, create_book_publication_state};
use baal_collector::native_dip_host_v1::sealed_native_prefix;
use baal_paper::inventory_quote_wallet_v1::{
    advance_inventory_quote_wallet_v1, create_inventory_quote_wallet_v1,
    inventory_quote_wallet_report_v1,
};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_stringify, sha256};
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
    let root = rt();
    let cutoff = "2026-10-06T00:00:00.000Z";
    let cutoff_ms = parse_iso_ms(cutoff).unwrap_or(f64::NAN);
    let mut events: Vec<V> = Vec::new();
    let mut bindings = Map::new();
    let mut scope = V::Null;
    let mut state: Option<V> = None;
    for name in ["public-books.jsonl", "anonymous-trades.jsonl"] {
        let file = root.join(name);
        let seal = sealed_native_prefix(&file)?;
        let bytes = fs::read(&file).map_err(|e| e.to_string())?;
        let prefix = &bytes[..(seal.bytes as usize).min(bytes.len())];
        let hash = sha256(prefix);
        let text = String::from_utf8_lossy(prefix).into_owned();
        let mut count = 0i64;
        let mut previous = String::new();
        let mut last_at: Option<String> = None;
        for line in text.trim_end().split('\n') {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            count += 1;
            if name == "public-books.jsonl" {
                if state.is_none() {
                    scope = get(get(&row, "payload"), "scope").clone();
                    state = Some(create_book_publication_state(&scope)?);
                }
                state = Some(apply_book_publication(state.as_ref().unwrap(), &row)?);
                if parse_iso_ms(get(&row, "at").as_str().unwrap_or("")).unwrap_or(f64::NAN)
                    >= cutoff_ms
                {
                    events.push(if get(&row, "kind").as_str() == Some("book") {
                        json!({
                            "kind": "book", "at": get(&row, "at").clone(),
                            "book": get(get(&row, "payload"), "book").clone(),
                            "sequence": get(&row, "sequence").clone(),
                            "hash": get(&row, "hash").clone(),
                        })
                    } else {
                        json!({
                            "kind": "boundary",
                            "marketId": get(get(&row, "payload"), "marketId").clone(),
                            "at": get(&row, "at").clone(),
                        })
                    });
                }
            } else {
                let mut body = row.as_object().cloned().unwrap_or_default();
                let expected = body.shift_remove("hash").unwrap_or(V::Null);
                let row_at = get(&row, "at").as_str().unwrap_or("").to_string();
                let row_seq = get(&row, "sequence").as_i64().unwrap_or(-1);
                let row_prev = get(&row, "previous").as_str().unwrap_or("");
                if row_seq != count
                    || row_prev != previous
                    || sha256(js_stringify(&V::Object(body)).as_bytes())
                        != expected.as_str().unwrap_or("")
                    || last_at
                        .as_ref()
                        .is_some_and(|l| {
                            parse_iso_ms(&row_at).unwrap_or(f64::NAN)
                                < parse_iso_ms(l).unwrap_or(f64::NAN)
                        })
                {
                    return Err("TAPE_REPLAY_CHAIN".into());
                }
                previous = expected.as_str().unwrap_or("").to_string();
                last_at = Some(row_at.clone());
                let e = get(&row, "event");
                let in_scope = get(&scope, "exchanges")
                    .as_array()
                    .map(|a| {
                        a.iter().any(|m| {
                            get(m, "marketId") == get(e, "marketId")
                                && get(m, "exchangeId") == get(e, "exchangeId")
                        })
                    })
                    .unwrap_or(false);
                if get(e, "type").as_str() != Some("anonymous_trade_page")
                    || get(e, "tournamentId") != get(&scope, "tournamentId")
                    || !in_scope
                {
                    return Err("TAPE_REPLAY_SCOPE".into());
                }
                if parse_iso_ms(&row_at).unwrap_or(f64::NAN) >= cutoff_ms {
                    events.push(json!({
                        "kind": "trades", "at": get(&row, "at").clone(),
                        "page": e.clone(),
                    }));
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
        if ino != seal.ino || dev != seal.dev || st.len() < seal.bytes {
            return Err("TAPE_REPLAY_REPLACED".into());
        }
        bindings.insert(
            name.to_string(),
            json!({"bytes": seal.bytes, "sha256": hash, "records": count}),
        );
    }
    let kind_rank = |e: &V| match get(e, "kind").as_str().unwrap_or("") {
        "boundary" => 0,
        "book" => 1,
        _ => 2,
    };
    events.sort_by(|a, b| {
        parse_iso_ms(get(a, "at").as_str().unwrap_or(""))
            .unwrap_or(f64::NAN)
            .partial_cmp(&parse_iso_ms(get(b, "at").as_str().unwrap_or("")).unwrap_or(f64::NAN))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| kind_rank(a).cmp(&kind_rank(b)))
    });
    let base = json!({
        "cutoff": cutoff, "initialCash": "100.000000", "maximumCommitted": "25.000000",
        "maximumMarkets": 12, "maximumOrderCost": "5.000000", "quantity": 1,
        "latencyMs": 3000, "lifetimeMs": 120000, "maximumGapMs": 120000,
        "minimumSamples": 4, "minimumHistoryMs": 120000, "depthFractionBps": 100,
        "horizonSeconds": 300, "fillModel": "strict_tape_through",
        "requirePriorTape": true,
    });
    let policies: Vec<V> = [0i64, 5, 10, 20]
        .iter()
        .map(|ra| {
            let mut m = base.as_object().unwrap().clone();
            m.insert("riskAversion".into(), json!(ra));
            m.insert("id".into(), json!(format!("tape-risk-{ra}")));
            V::Object(m)
        })
        .collect();
    let mut wallets: Vec<V> = policies
        .iter()
        .map(create_inventory_quote_wallet_v1)
        .collect::<Result<_, _>>()?;
    for event in &events {
        for (i, w) in wallets.iter_mut().enumerate() {
            advance_inventory_quote_wallet_v1(w, event, &policies[i])?;
        }
    }
    let mut coverage = Map::new();
    for e in events.iter().filter(|e| get(e, "kind").as_str() == Some("trades")) {
        let page = get(e, "page");
        let ex = get(page, "exchangeId").as_str().unwrap_or("").to_string();
        let entry = coverage
            .entry(ex.clone())
            .or_insert_with(|| {
                json!({"pages": 0, "completePages": 0, "firstAt": get(e, "at").clone(), "lastAt": get(e, "at").clone()})
            });
        let obj = entry.as_object_mut().unwrap();
        let pages = get(&V::Object(obj.clone()), "pages").as_i64().unwrap_or(0);
        obj.insert("pages".into(), json!(pages + 1));
        if get(get(page, "coverage"), "complete").as_bool() == Some(true)
            && get(get(page, "pagination"), "hasMore").as_bool() == Some(false)
        {
            let cp = get(&V::Object(obj.clone()), "completePages")
                .as_i64()
                .unwrap_or(0);
            obj.insert("completePages".into(), json!(cp + 1));
        }
        obj.insert("lastAt".into(), get(e, "at").clone());
    }
    let cohorts: Vec<V> = policies
        .iter()
        .zip(wallets.iter())
        .map(|(policy, w)| {
            let unresolved: Vec<V> = get(w, "quotes")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|q| get(q, "closedAt").is_null())
                .collect();
            Ok(json!({
                "policy": policy.clone(),
                "report": inventory_quote_wallet_report_v1(w, policy)?,
                "unresolvedQuotes": unresolved,
                "fills": get(w, "fills").clone(),
                "redemptions": get(w, "redemptions").clone(),
            }))
        })
        .collect::<Result<Vec<V>, String>>()?;
    let report = json!({
        "schemaVersion": 1,
        "createdAt": ms_to_iso(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_millis() as f64
        ),
        "mode": "inventory_strict_tape_seen_capture_development_v1",
        "bindings": bindings,
        "processed": events.len() as i64,
        "elapsedMs": begun.elapsed().as_secs_f64() * 1000.0,
        "tradeCoverage": coverage,
        "cohorts": cohorts,
        "networkRequests": 0,
        "competitionOrders": 0,
        "profitabilityEstablished": false,
        "limitations": [
            "Previouslyseen development captures, not holdout/prospective evidence.",
            "Strict-through anonymous trades cannot prove actual queue priority or aggressor direction.",
            "Unknown/censored quotes retain reservations; normal complementary netting is counterfactual."
        ],
    });
    fs::write(
        "data/inventory-tape-replay-20261007.json",
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n",
    )
    .map_err(|e| e.to_string())?;
    let summary: Vec<V> = get(&report, "cohorts")
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
    let unresolved0: Vec<V> = get(&report, "cohorts")
        .as_array()
        .and_then(|a| a.first())
        .map(|c| get(c, "unresolvedQuotes").as_array().cloned().unwrap_or_default())
        .unwrap_or_default()
        .iter()
        .map(|q| {
            json!({
                "marketId": get(q, "marketId").clone(),
                "exchangeId": get(q, "exchangeId").clone(),
                "coverage": get(get(&report, "tradeCoverage"), get(q, "exchangeId").as_str().unwrap_or("")).clone(),
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string(&json!({
            "processed": events.len() as i64,
            "elapsedMs": get(&report, "elapsedMs").clone(),
            "cohorts": summary,
            "unresolved": unresolved0,
        }))
        .map_err(|e| e.to_string())?
    );
    Ok(report)
}
