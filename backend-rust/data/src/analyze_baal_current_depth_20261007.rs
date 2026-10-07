// data/analyze-baal-current-depth-20261007.mjs — offline original-source
// valuation. No account calls, files changed in runtime, fills, portfolio
// adoption, or order authority. Reports stale/missing explicitly.
use baal_collector::book_publication_v3::{
    apply_book_publication_v3, create_book_publication_state_v3,
};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::fixed::{amount, money};
use baal_service_runtime::json_text::{js_truthy, js_stringify};
use serde_json::{json, Map, Value as V};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::baal_generic_no_controller_v5::{
    baal_generic_no_controller_v5_identity, replay_baal_generic_no_journal_v5,
};
use crate::no_liquidation_depth_value_v1::value_no_liquidation_depth_v1;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default())
        .join("SigTradingCup")
        .join("paper-runtime")
}
// JS ToNumber for `v > 0`-style checks: absent/undefined/NaN -> false.
fn js_num(v: &V) -> f64 {
    match v {
        V::Null => 0.0,
        V::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                0.0
            } else {
                t.parse::<f64>().unwrap_or(f64::NAN)
            }
        }
        _ => f64::NAN,
    }
}
// Numbers that are integral serialize without a fraction, like JSON.stringify.
fn num_v(x: f64) -> V {
    if x.is_finite() && x.fract() == 0.0 && x.abs() <= 9_007_199_254_740_991.0 {
        V::from(x as i64)
    } else {
        V::from(x)
    }
}
fn date_parse(v: &V) -> f64 {
    v.as_str()
        .and_then(parse_iso_ms)
        .unwrap_or(f64::NAN)
}

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let req: V = serde_json::from_slice(
        &std::fs::read(r.join("baal-generic-live-request-v3.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let actor_file = r
        .join("baal-generic-live-runs")
        .join(get(&req, "runId").as_str().unwrap_or(""))
        .join(get(&req, "epoch").as_str().unwrap_or(""))
        .join("controller")
        .join("baal-generic-no-v5-journal.jsonl");
    let actor = std::fs::read(&actor_file).map_err(|e| e.to_string())?;
    let actor_text = String::from_utf8_lossy(&actor).to_string();
    let mut actor_rows: Vec<V> = Vec::new();
    for line in actor_text.trim_end().split('\n') {
        actor_rows.push(serde_json::from_str(line).map_err(|e| e.to_string())?);
    }
    let custody_seed = get(
        &get(&get(actor_rows.first().unwrap_or(&V::Null), "event"), "input"),
        "custodySeed",
    )
    .clone();
    let replay = replay_baal_generic_no_journal_v5(
        &actor_text,
        &baal_generic_no_controller_v5_identity()?,
        &custody_seed,
    )?;
    let inventory = get(&get(&replay, "state"), "inventory");
    let owned: Vec<(String, V)> = inventory
        .as_object()
        .map(|m| {
            m.iter()
                .filter(|(_, p)| {
                    js_num(get(get(p, "proof"), "remainingOwnedQuantity")) > 0.0
                })
                .map(|(ex, p)| (ex.clone(), p.clone()))
                .collect()
        })
        .unwrap_or_default();
    let wanted: std::collections::HashSet<String> =
        owned.iter().map(|(ex, _)| ex.clone()).collect();

    let book_file = r.join("public-books.jsonl");
    let size = std::fs::metadata(&book_file)
        .map_err(|e| e.to_string())?
        .len() as usize;
    let tail_len = size.min(65536);
    let tail = {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(&book_file).map_err(|e| e.to_string())?;
        f.seek(SeekFrom::Start((size - tail_len) as u64))
            .map_err(|e| e.to_string())?;
        let mut b = vec![0u8; tail_len];
        f.read_exact(&mut b).map_err(|e| e.to_string())?;
        b
    };
    let end = (size - tail_len) as i64
        + tail.iter().rposition(|&b| b == 10).map(|i| i as i64 + 1).unwrap_or(0);
    if end <= 0 {
        return Err("No complete source".into());
    }

    let mut state = V::Null;
    let mut started = false;
    let mut records: i64 = 0;
    let mut latest: indexmap::IndexMap<String, V> = indexmap::IndexMap::new();
    let mut hash = Sha256::new();
    let prefix = std::fs::read(&book_file).map_err(|e| e.to_string())?;
    let prefix = &prefix[..end as usize];
    let mut lines: Vec<&[u8]> = prefix.split(|&b| b == 10).collect();
    lines.pop();
    for raw in lines {
        let line = match raw.last() {
            Some(&b) if b == 13 => &raw[..raw.len() - 1],
            _ => raw,
        };
        hash.update(line);
        hash.update([10]);
        let rec: V = serde_json::from_slice(line).map_err(|e| e.to_string())?;
        if !started {
            state = create_book_publication_state_v3(get(&rec, "payload").get("scope").unwrap_or(&V::Null))?;
            started = true;
        }
        state = apply_book_publication_v3(&state, &rec)?;
        records += 1;
        if get(&rec, "kind").as_str() == Some("book")
            && wanted.contains(get(&get(&rec, "payload"), "book").get("exchangeId").and_then(|v| v.as_str()).unwrap_or(""))
        {
            let book = get(&get(&rec, "payload"), "book");
            let entry = json!({
                "book": book,
                "recordHash": get(&rec, "hash"),
                "sequence": get(&rec, "sequence"),
                "availableAt": get(&rec, "at"),
            });
            latest.insert(
                get(book, "exchangeId").as_str().unwrap_or("").to_string(),
                entry,
            );
        }
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0);
    let mut markets: Vec<V> = Vec::new();
    for (exchange_id, p) in &owned {
        let proof = get(p, "proof");
        let mut base = Map::new();
        base.insert("marketId".into(), get(proof, "marketId").clone());
        base.insert("exchangeId".into(), json!(exchange_id));
        base.insert(
            "quantity".into(),
            get(proof, "remainingOwnedQuantity").clone(),
        );
        base.insert("cost".into(), get(proof, "remainingOwnedCost").clone());
        let source = latest.get(exchange_id);
        if source.is_none() {
            base.insert("missingDepth".into(), json!(true));
            markets.push(V::Object(base));
            continue;
        }
        let source = source.unwrap();
        let b = get(source, "book");
        let after_ownership =
            date_parse(get(b, "requestStartedAt")) >= date_parse(get(proof, "readAt"));
        let active = get(
            &get(&state, "books"),
            get(b, "instrument").as_str().unwrap_or(""),
        )
        .get("publicationHash")
            == Some(&get(source, "recordHash").clone());
        let mut m = base;
        m.insert(
            "source".into(),
            json!({
                "requestStartedAt": get(b, "requestStartedAt"),
                "receivedAt": get(b, "receivedAt"),
                "availableAt": get(source, "availableAt"),
                "recordHash": get(source, "recordHash"),
                "sequence": get(source, "sequence"),
            }),
        );
        m.insert(
            "receiptAgeMs".into(),
            num_v(now - date_parse(get(b, "receivedAt"))),
        );
        m.insert(
            "requestDurationMs".into(),
            num_v(date_parse(get(b, "receivedAt")) - date_parse(get(b, "requestStartedAt"))),
        );
        m.insert("afterOwnership".into(), json!(after_ownership));
        m.insert("activeInPublication".into(), json!(active));
        let quantity = m.get("quantity").cloned().unwrap_or(V::Null);
        let cost = m.get("cost").cloned().unwrap_or(V::Null);
        m.insert(
            "baseline".into(),
            value_no_liquidation_depth_v1(&json!({
                "quantity": quantity,
                "remainingCost": cost,
                "yesAsks": get(b, "asks"),
            }))?,
        );
        m.insert(
            "oneTick".into(),
            value_no_liquidation_depth_v1(&json!({
                "quantity": quantity,
                "remainingCost": cost,
                "yesAsks": get(b, "asks"),
                "slippageTicks": 1,
            }))?,
        );
        markets.push(V::Object(m));
    }

    let complete = markets.iter().all(|m| {
        js_truthy(get(m, "baseline").get("complete")) && js_truthy(m.get("afterOwnership"))
    });
    let fresh = complete
        && markets.iter().all(|m| {
            get(m, "receiptAgeMs").as_f64().is_some_and(|x| x <= 20000.0)
                && get(m, "requestDurationMs")
                    .as_f64()
                    .is_some_and(|x| x <= 20000.0)
                && js_truthy(m.get("activeInPublication"))
        });
    let realized: i128 = if owned.is_empty() {
        0
    } else {
        inventory
            .as_object()
            .map(|m| {
                m.values()
                    .map(|p| {
                        amount(
                            get(get(p, "proof"), "cumulativeRealizedPnl")
                                .as_str()
                                .unwrap_or(""),
                        )
                    })
                    .collect::<Result<Vec<i128>, String>>()
                    .map(|v| v.into_iter().sum())
            })
            .unwrap_or(Ok(0))?
    };
    let historical: Option<i128> = if complete {
        Some(
            markets
                .iter()
                .map(|m| {
                    amount(
                        get(get(m, "baseline"), "unrealizedPnl")
                            .as_str()
                            .unwrap_or(""),
                    )
                })
                .collect::<Result<Vec<i128>, String>>()?
                .into_iter()
                .sum(),
        )
    } else {
        None
    };
    let report = json!({
        "checkedAt": ms_to_iso(now),
        "mode": "dated_depth_counterfactual_NO_portfolio_value_v1",
        "actorAnchor": get(&replay, "anchor"),
        "publicPrefix": {
            "bytes": end,
            "sha256": format!("{:x}", hash.finalize()),
            "records": records,
            "tailHash": get(&state, "hash"),
        },
        "markets": markets,
        "realized": money(realized),
        "completeDepth": complete,
        "allFresh": fresh,
        "historicalDepthUnrealized": historical.map(money),
        "currentTotalPnl": V::Null,
        "executionAuthority": false,
        "actualNewFills": 0,
        "limitations": [
            "Historical depth is not simultaneous, fresh account reconciliation or a fill.",
            "Own-order contamination is not ruled out by this public capture alone.",
            "Missing depth or stale books cannot be valued at the top quote.",
        ],
    });
    std::fs::write(
        root.join("data/baal-current-depth-value-20261007.json"),
        format!("{}\n", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?),
    )
    .map_err(|e| e.to_string())?;
    let summary_markets: Vec<V> = markets
        .iter()
        .map(|m| {
            let mut o = Map::new();
            o.insert("exchangeId".into(), get(m, "exchangeId").clone());
            if let Some(v) = m.get("missingDepth") {
                o.insert("missing".into(), v.clone());
            }
            if let Some(v) = m.get("receiptAgeMs") {
                o.insert("ageMs".into(), v.clone());
            }
            o.insert("quantity".into(), get(m, "quantity").clone());
            if let Some(u) = m.get("baseline").and_then(|b| b.get("unrealizedPnl")) {
                o.insert("unrealized".into(), u.clone());
            }
            if let Some(v) = m.get("afterOwnership") {
                o.insert("afterOwnership".into(), v.clone());
            }
            V::Object(o)
        })
        .collect();
    println!(
        "{}",
        js_stringify(&json!({
            "checkedAt": get(&report, "checkedAt"),
            "markets": summary_markets,
            "allFresh": get(&report, "allFresh"),
            "historicalDepthUnrealized": get(&report, "historicalDepthUnrealized"),
            "realized": get(&report, "realized"),
            "currentTotalPnl": V::Null,
            "sourceRecords": records,
        }))
    );
    Ok(report)
}
