// live-paper/edge-depth-audit.mjs — one-percent capacity diagnostics against
// published book ladders. Diagnostic only; CLI entry omitted.
use crate::book_publication::{apply_book_publication, create_book_publication_state};
use crate::model::digest;
use baal_paper::fixed::{big_amount, instant, money, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, sha256};
use num_bigint::BigInt;
use serde_json::{json, Value as V};

fn need(ok: bool, reason: &str) -> Result<(), String> { if !ok { return Err(format!("EDGE_DEPTH_{reason}")); } Ok(()) }

/// oneShareDepth(levels, action) — keep the fixed one-percent cap separately at
/// every level.
pub fn one_share_depth(levels: &V, action: &str) -> Result<V, String> {
    need(["buy", "sell"].contains(&action) && levels.is_array(), "SIDE")?;
    let mut previous: Option<BigInt> = None;
    for l in levels.as_array().unwrap() {
        let p = big_amount(l["price"].as_str().unwrap_or(""))?;
        need(p >= BigInt::from(0) && p <= BigInt::from(SCALE)
            && js_int(&l["quantity"]).is_some_and(|q| q > 0), "LEVEL")?;
        need(previous.is_none()
            || (action == "buy" && p > *previous.as_ref().unwrap())
            || (action == "sell" && p < *previous.as_ref().unwrap()), "ORDER")?;
        previous = Some(p);
    }
    let levels = levels.as_array().unwrap();
    let index = levels.iter().position(|l| l["quantity"].as_f64().map_or(false, |q| q >= 100.0));
    let index = match index {
        None => return Ok(json!({"status":"unavailable","levelIndex":null,"rawPrice":null,"modeledPrice":null})),
        Some(i) => i,
    };
    let raw = big_amount(levels[index]["price"].as_str().unwrap_or(""))?;
    let modeled = &raw + BigInt::from(if action == "buy" { TICK } else { -TICK });
    if modeled < BigInt::from(TICK) || modeled > BigInt::from(SCALE - TICK) {
        return Ok(json!({"status":"unavailable","levelIndex":null,"rawPrice":null,"modeledPrice":null}));
    }
    Ok(json!({"status":"available","levelIndex":index,"rawPrice":money(&raw),"modeledPrice":money(&modeled)}))
}

/// auditTrialDepth({bank, trials, sourceBytes})
pub fn audit_trial_depth(bank: &V, trials: &[V], source_bytes: &[u8]) -> Result<V, String> {
    let complete = source_bytes.iter().rposition(|&b| b == b'\n').map(|i| i + 1).unwrap_or(0);
    need(complete > 0, "EMPTY")?;
    let bytes = &source_bytes[..complete];
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.pop(); // trailing "" after final newline (JS slice(0,-1))
    let rows: Vec<V> = lines.iter()
        .map(|l| serde_json::from_str::<V>(l).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let mut publication = create_book_publication_state(&bank["universe"])?;
    for row in &rows { publication = apply_book_publication(&publication, row)?; }
    need(bank["config"]["feeBps"].as_f64() == Some(0.0)
        && bank["config"]["slippageTicks"].as_f64() == Some(1.0)
        && bank["config"]["depthFractionBps"].as_f64() == Some(100.0)
        && bank["config"]["orderShares"].as_f64() == Some(1.0), "POLICY")?;
    let mut results = Vec::with_capacity(trials.len());
    for t in trials {
        need(t["phase"] == "awaiting_outcome"
            && instant(t["dueAt"].as_str().unwrap_or(""))? == instant(t["filledAt"].as_str().unwrap_or(""))?
                + bank["policy"]["horizonMs"].as_i64().unwrap_or(0)
            && instant(t["expiresAt"].as_str().unwrap_or(""))? == instant(t["dueAt"].as_str().unwrap_or(""))?
                + bank["policy"]["labelLatenessMs"].as_i64().unwrap_or(0), "DEADLINE")?;
        let entry = rows.iter().find(|r| r["kind"] == "book"
            && r["payload"]["book"]["marketId"] == t["marketId"]
            && r["payload"]["book"]["receivedAt"] == t["filledAt"]);
        need(entry.is_some(), "ENTRY")?;
        let entry = entry.unwrap();
        let eb = &entry["payload"]["book"];
        need(instant(eb["requestStartedAt"].as_str().unwrap_or(""))?
            >= instant(t["preparedAt"].as_str().unwrap_or(""))? + bank["config"]["latencyMs"].as_i64().unwrap_or(0)
            && eb["bids"][0]["quantity"].as_f64().map_or(false, |q| q >= 100.0)
            && eb["asks"][0]["quantity"].as_f64().map_or(false, |q| q >= 100.0), "ENTRY_ELIGIBILITY")?;
        let revoke = rows.iter().find(|r| {
            r["sequence"].as_i64().unwrap_or(i64::MIN) > entry["sequence"].as_i64().unwrap_or(i64::MIN)
                && (["session_start", "session_stop"].contains(&r["kind"].as_str().unwrap_or(""))
                    || (r["kind"] == "invalidation"
                        && (r["payload"].get("marketId").map_or(false, |v| v.is_null())
                            || r["payload"]["marketId"] == t["marketId"])))
        });
        let due = instant(t["dueAt"].as_str().unwrap_or(""))?;
        let mut outcome: Option<&V> = None;
        for r in &rows {
            if r["sequence"].as_i64().unwrap_or(i64::MIN) > entry["sequence"].as_i64().unwrap_or(i64::MIN)
                && r["kind"] == "book"
                && r["payload"]["book"]["marketId"] == t["marketId"]
                && instant(r["payload"]["book"]["requestStartedAt"].as_str().unwrap_or(""))? >= due {
                outcome = Some(r);
                break;
            }
        }
        let mut base = t.clone();
        base["entrySequence"] = entry["sequence"].clone();
        base["entryHash"] = entry["hash"].clone();
        let outcome_missing = outcome.is_none()
            || (revoke.is_some()
                && revoke.unwrap()["sequence"].as_i64().unwrap_or(i64::MIN)
                    < outcome.unwrap()["sequence"].as_i64().unwrap_or(i64::MIN));
        if outcome_missing {
            base["status"] = json!(if revoke.is_some() { "revoked" } else { "not_observed" });
            base["revocationSequence"] = revoke.map(|r| r["sequence"].clone()).unwrap_or(V::Null);
            results.push(base);
            continue;
        }
        let outcome = outcome.unwrap();
        let b = &outcome["payload"]["book"];
        let entry_price = big_amount(eb["asks"][0]["price"].as_str().unwrap_or(""))? + BigInt::from(TICK);
        let ladder = one_share_depth(&b["bids"], "sell")?;
        let top_depth = b["bids"][0]["quantity"].as_f64().map_or(false, |q| q >= 100.0)
            && b["asks"][0]["quantity"].as_f64().map_or(false, |q| q >= 100.0);
        let timely = instant(b["receivedAt"].as_str().unwrap_or(""))? <= instant(t["expiresAt"].as_str().unwrap_or(""))?;
        let independent = b["versionKey"] != eb["versionKey"];
        base["outcomeSequence"] = outcome["sequence"].clone();
        base["outcomeHash"] = outcome["hash"].clone();
        base["outcomeReceivedAt"] = b["receivedAt"].clone();
        base["outcomeRequestStartedAt"] = b["requestStartedAt"].clone();
        base["topBid"] = b["bids"][0].clone();
        base["topAsk"] = b["asks"][0].clone();
        base["timely"] = json!(timely);
        base["independent"] = json!(independent);
        base["topDepth"] = json!(top_depth);
        base["status"] = json!(if !timely { "late_quote" } else if !independent { "unchanged_version" }
            else if !top_depth { "frozen_top_depth_censor" } else { "top_depth_available" });
        base["fullLadder"] = ladder.clone();
        base["diagnosticModeledNet"] = if timely && independent && ladder["status"] == "available" {
            json!(money(&(big_amount(ladder["modeledPrice"].as_str().unwrap_or(""))? - &entry_price)))
        } else { V::Null };
        results.push(base);
    }
    Ok(json!({
        "schemaVersion": 1, "mode": "source_verified_trial_depth_diagnostic",
        "bankHash": digest(bank), "sourceSequence": publication["sequence"],
        "sourceHash": publication["hash"], "sourceBytes": complete,
        "sourceBytesHash": sha256(bytes),
        "incompleteTailBytes": source_bytes.len() - complete,
        "trials": results, "networkRequests": 0, "ordersSubmitted": 0,
        "profitabilityEstablished": false,
        "limitations": [
            "Full-ladder values are alternative quote diagnostics, not original labels or actual fills.",
            "Original frozen top-depth censoring and wallet accounting remain unchanged.",
            "Receipt time checks cannot replace original consumer checkpoint availability and replay.",
            "One-percent capacity is floored separately at each price level; deeper liquidity receives its worse price."
        ],
    }))
}
