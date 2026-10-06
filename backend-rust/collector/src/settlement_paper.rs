// live-paper/settlement-paper.mjs — single-market settlement-value paper engine.
// State is kept as a serde Value to preserve JS object semantics byte-for-byte.
use baal_paper::fixed::{big_amount as amount, instant, money, TICK};
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("SETTLEMENT_PAPER_{}", code)); }
    Ok(())
}
fn digest(v: &V) -> String { crate::model::digest(v) }
fn instant_v(v: &V) -> Result<i64, String> { instant(v.as_str().unwrap_or("")) }
fn amount_v(v: &V) -> Result<BigInt, String> { amount(v.as_str().unwrap_or("")) }
fn tick() -> BigInt { BigInt::from(TICK) }
/// Number.isSafeInteger equivalent on a JSON number.
fn safe_int(v: &V) -> Option<i64> {
    match v {
        V::Number(n) => n.as_i64(),
        _ => None,
    }
}

/// SETTLEMENT_PAPER_POLICY — frozen policy object (insertion order matters).
pub fn settlement_paper_policy() -> V {
    json!({"minimumEdge": "0.020000", "maximumSpread": "0.020000", "maximumBookAgeMs": 20000,
        "maximumResearchAgeMs": 21600000, "fractionalKellyBps": 2500, "slippageTicks": 1,
        "depthFractionBps": 100, "maxOrderCost": "1.000000", "maxExposure": "3.000000",
        "cashReserve": "90.000000", "cumulativeBuyBudget": "10.000000",
        "groupCaps": {"national_election": "3.000000", "texas_governor": "1.000000"}})
}
/// SETTLEMENT_PAPER_FILLS
pub fn settlement_paper_fills() -> V {
    json!({"initialCash": "100.000000", "fees": 0, "sharesPerOrder": 1, "maxPositionShares": 1,
        "latenciesMs": [1000, 3000], "intentLifetimeMs": 180000, "cooldownMs": 900000})
}
/// validateSettlementPaperBank(bank)
pub fn validate_settlement_paper_bank(bank: &V) -> Result<(), String> {
    need(bank["schemaVersion"].as_i64() == Some(1)
        && bank["mode"].as_str() == Some("funded_settlement_value_paper_v1")
        && digest(&bank["policy"]) == digest(&settlement_paper_policy())
        && digest(&bank["fillPolicy"]) == digest(&settlement_paper_fills()), "POLICY")?;
    need(bank["research"].as_array().is_some_and(|r| r.len() == 1)
        && bank["identities"].as_object().is_some_and(|m| m.len() == 1), "SCOPE")?;
    let r = &bank["research"][0];
    let identity = &bank["identities"][r["marketId"].as_str().unwrap_or("")];
    crate::probability_paper::validate_research(r)?;
    need(r["side"].as_str() == Some("YES")
        && instant_v(&r["recordedAt"])? <= instant_v(&bank["cutoff"])?
        && identity["exchangeId"].as_str() == r["exchangeId"].as_str()
        && identity["predicateHash"] == r["predicateHash"]
        && bank["universe"]["exchanges"].as_array().cloned().unwrap_or_default().iter().any(|x| {
            x["marketId"] == r["marketId"] && x["exchangeId"] == r["exchangeId"] && x["option"].as_str() == Some("YES")
        }), "RESEARCH_BINDING")?;
    let risk_groups = bank["riskGroups"][r["marketId"].as_str().unwrap_or("")].as_array()
        .map(|g| g.iter().map(|x| x.as_str().unwrap_or("")).collect::<Vec<_>>().join("|"));
    need(risk_groups.as_deref() == Some("national_election|texas_governor")
        && safe_int(&bank["valuationDeadlineMs"]).is_some()
        && safe_int(&bank["valuationDeadlineMs"]).unwrap() > instant_v(&bank["cutoff"])?, "HORIZON")?;
    Ok(())
}
/// createSettlementPaperState(bank)
pub fn create_settlement_paper_state(bank: &V) -> Result<V, String> {
    validate_settlement_paper_bank(bank)?;
    let mut wallets = Map::new();
    for ms in bank["fillPolicy"]["latenciesMs"].as_array().cloned().unwrap_or_default() {
        let key = ms.as_i64().map(|i| i.to_string()).unwrap_or_else(|| ms.to_string());
        wallets.insert(key, json!({"cash": bank["fillPolicy"]["initialCash"], "realizedPnl": "0.000000",
            "cumulativeBuys": "0.000000", "positions": {}, "pending": {}, "fills": [],
            "consumedVersions": {}, "lastExitedAtMs": {}, "intents": 0}));
    }
    Ok(json!({"bankHash": digest(bank), "lastAvailableAt": bank["cutoff"],
        "barriers": {"global": bank["cutoff"], "markets": {}},
        "counts": {}, "decisions": [], "books": {}, "wallets": wallets}))
}
/// assertSettlementPaperState(state, bank)
pub fn assert_settlement_paper_state(state: &V, bank: &V) -> Result<(), String> {
    need(state["bankHash"].as_str() == Some(&digest(bank)), "BANK_CHANGED")?;
    for (_latency, w) in state["wallets"].as_object().cloned().unwrap_or_default() {
        let mut cash = amount(bank["fillPolicy"]["initialCash"].as_str().unwrap_or(""))?;
        let mut realized = BigInt::from(0);
        let mut buys = BigInt::from(0);
        let mut positions = Map::new();
        for f in w["fills"].as_array().cloned().unwrap_or_default() {
            need(f["quantity"].as_i64() == Some(1) && f["fee"].as_str() == Some("0.000000")
                && ["buy", "sell"].contains(&f["action"].as_str().unwrap_or("")), "FILL")?;
            let value = amount_v(&f["price"])?;
            let market = f["marketId"].as_str().unwrap_or("").to_string();
            if f["action"].as_str() == Some("buy") {
                need(!positions.contains_key(&market), "DUPLICATE_HOLDING")?;
                positions.insert(market, json!({"quantity": 1, "cost": f["price"], "enteredAt": f["at"]}));
                cash -= &value;
                buys += &value;
            } else {
                need(positions.contains_key(&market), "OVERSELL")?;
                realized += &value - amount_v(&positions[&market]["cost"])?;
                cash += &value;
                positions.shift_remove(&market);
            }
        }
        need(cash == amount_v(&w["cash"])?
            && realized == amount_v(&w["realizedPnl"])?
            && buys == amount_v(&w["cumulativeBuys"])?
            && digest(&V::Object(positions.clone())) == digest(&w["positions"]), "CONSERVATION")?;
        let reserved: BigInt = w["pending"].as_object().cloned().unwrap_or_default().values()
            .filter(|p| p["action"].as_str() == Some("buy"))
            .try_fold(BigInt::from(0), |sum, p| Ok::<BigInt, String>(sum + amount_v(&p["limit"])?))?;
        let held: BigInt = w["positions"].as_object().cloned().unwrap_or_default().values()
            .try_fold(BigInt::from(0), |sum, p| Ok::<BigInt, String>(sum + amount_v(&p["cost"])?))?;
        need(&cash - &reserved >= amount(bank["policy"]["cashReserve"].as_str().unwrap_or(""))?
            && &held + &reserved <= amount(bank["policy"]["maxExposure"].as_str().unwrap_or(""))?
            && &buys + &reserved <= amount(bank["policy"]["cumulativeBuyBudget"].as_str().unwrap_or(""))?, "RESERVATIONS")?;
        need(w["pending"].as_object().cloned().unwrap_or_default().iter().all(|(id, p)| {
            p["quantity"].as_i64() == Some(1) && p["marketId"].as_str() == Some(id.as_str())
                && w["positions"].as_object().is_some_and(|pos| pos.contains_key(id))
                    == (p["action"].as_str() == Some("sell"))
        }), "PENDING_BINDING")?;
    }
    Ok(())
}
/// boundary(s, id, at, reason)
fn boundary(s: &mut V, id: Option<&str>, at: &V, reason: &str) {
    match id {
        None => {
            s["barriers"]["global"] = at.clone();
            s["books"] = json!({});
        }
        Some(id) => {
            s["barriers"]["markets"][id] = at.clone();
            s["books"].as_object_mut().and_then(|m| m.shift_remove(id));
        }
    }
    for (_k, w) in s["wallets"].as_object_mut().map(|m| m.iter_mut().collect::<Vec<_>>()).unwrap_or_default() {
        let keys: Vec<String> = w["pending"].as_object().map(|p| p.keys().cloned().collect()).unwrap_or_default();
        for market in keys {
            if id.is_none() || id == Some(market.as_str()) {
                w["pending"].as_object_mut().and_then(|p| p.shift_remove(&market));
            }
        }
    }
    let n = s["counts"][reason].as_i64().unwrap_or(0);
    s["counts"][reason] = json!(n + 1);
}
/// settlementPaperInput(s, bank, latency, row, availableAt, account, {ignorePending})
pub fn settlement_paper_input(s: &V, bank: &V, latency: &str, row: &V, available_at: &str,
    account: &V, ignore_pending: bool) -> Result<V, String> {
    let b = &row["payload"]["book"];
    let w = &s["wallets"][latency];
    let known_at_ms = std::cmp::max(instant_v(&account["positionsAt"])?, instant_v(&account["ordersAt"])?);
    let mut positions = Vec::new();
    for (id, p) in w["positions"].as_object().cloned().unwrap_or_default() {
        positions.push(json!({"candidateId": format!("{}:YES", id), "quantity": p["quantity"],
            "cost": p["cost"], "enteredAtMs": instant_v(&p["enteredAt"])?}));
    }
    let reservations: Vec<V> = if ignore_pending { Vec::new() } else {
        w["pending"].as_object().cloned().unwrap_or_default().iter()
            .filter(|(_, p)| p["action"].as_str() == Some("buy"))
            .map(|(id, p)| json!({"candidateId": format!("{}:YES", id), "maximumCost": p["limit"]}))
            .collect()
    };
    let mut candidates = Vec::new();
    for r in bank["research"].as_array().cloned().unwrap_or_default() {
        let market_id = r["marketId"].as_str().unwrap_or("");
        let book = if market_id == b["marketId"].as_str().unwrap_or("") {
            json!({"requestStartedAtMs": instant_v(&b["requestStartedAt"])?,
                "receivedAtMs": instant_v(&b["receivedAt"])?,
                "availableAtMs": instant(available_at)?, "sequence": row["sequence"],
                "sourceHash": row["hash"], "representation": "direct_yes",
                "bids": b["bids"], "asks": b["asks"]})
        } else { V::Null };
        candidates.push(json!({"id": format!("{}:YES", market_id), "marketId": r["marketId"],
            "exchangeId": r["exchangeId"], "side": r["side"], "predicateHash": r["predicateHash"],
            "riskGroups": bank["riskGroups"][market_id], "settlesAtMs": bank["valuationDeadlineMs"],
            "research": {"id": r["id"], "predicateHash": r["predicateHash"], "side": r["side"],
                "lower": r["lower"], "upper": r["upper"], "recordedAtMs": instant_v(&r["recordedAt"])?,
                "sourceAtMs": instant_v(&r["sourceAt"])?, "expiresAtMs": instant_v(&r["expiresAt"])?,
                "method": r["method"], "settlementReviewed": r["settlementReviewed"],
                "flags": r["flags"], "sources": r["sources"]},
            "book": book,
            "account": {"positionsAtMs": instant_v(&account["positionsAt"])?,
                "ordersAtMs": instant_v(&account["ordersAt"])?, "knownAtMs": known_at_ms,
                "complete": true,
                "positionExcluded": account["excludedExchangeIds"].as_array().cloned().unwrap_or_default()
                    .iter().any(|x| x == &r["exchangeId"]),
                "openOrderExcluded": account["excludedExchangeIds"].as_array().cloned().unwrap_or_default()
                    .iter().any(|x| x == &r["exchangeId"])}}));
    }
    Ok(json!({"schemaVersion": 1, "mode": "settlement_value_policy_v1",
        "asOfMs": instant(available_at)?, "policy": bank["policy"],
        "wallet": {"capital": bank["fillPolicy"]["initialCash"], "cash": w["cash"],
            "cumulativeBuys": w["cumulativeBuys"], "positions": positions, "reservations": reservations},
        "candidates": candidates}))
}
// The native value policy chooses an action. Only a later, fresh REST request
// can simulate its one-share execution; the initial quote never makes a fill.
/// advanceSettlementPaper(s, row, bank, availableAt, account, evaluate)
pub fn advance_settlement_paper(s: &mut V, row: &V, bank: &V, available_at: &str, account: &V,
    evaluate: &mut dyn FnMut(&V) -> Result<V, String>) -> Result<(), String> {
    need(s["bankHash"].as_str() == Some(&digest(bank))
        && instant_v(&row["at"])? <= instant(available_at)?
        && instant(available_at)? >= instant_v(&s["lastAvailableAt"])?, "CLOCK")?;
    s["lastAvailableAt"] = json!(available_at);
    if row["kind"].as_str() != Some("book") {
        need(["session_start", "session_stop", "invalidation"].contains(&row["kind"].as_str().unwrap_or("")), "ROW_KIND")?;
        boundary(s, if row["kind"].as_str() == Some("invalidation") {
            Some(row["payload"]["marketId"].as_str().unwrap_or(""))
        } else { None }, &json!(available_at), "source_boundary");
        return Ok(());
    }
    let b = row["payload"]["book"].clone();
    if bank["identities"][b["marketId"].as_str().unwrap_or("")].is_null() { return Ok(()); }
    let now = instant(available_at)?;
    let request = instant_v(&b["requestStartedAt"])?;
    let id = b["marketId"].as_str().unwrap_or("").to_string();
    let pre_boundary = [&s["barriers"]["global"], &s["barriers"]["markets"][&id], &bank["cutoff"]]
        .iter().any(|at| !at.is_null() && request <= instant_v(at).unwrap_or(i64::MAX));
    if now as f64 - instant_v(&b["receivedAt"])? as f64 > bank["policy"]["maximumBookAgeMs"].as_f64().unwrap_or(0.0)
        || pre_boundary
    {
        boundary(s, Some(&id), &json!(available_at), "stale_or_pre_boundary");
        return Ok(());
    }
    if let Some(gate) = crate::probability_receipt::probability_account_gate(account, &b, available_at) {
        boundary(s, Some(&id), &json!(available_at), &gate);
        return Ok(());
    }
    let mut book_entry = b.clone();
    book_entry["availableAt"] = json!(available_at);
    book_entry["sourceHash"] = row["hash"].clone();
    book_entry["publicationSequence"] = row["sequence"].clone();
    s["books"][&id] = book_entry;
    let wallet_keys: Vec<String> = s["wallets"].as_object().map(|w| w.keys().cloned().collect()).unwrap_or_default();
    for latency in wallet_keys {
        let input = settlement_paper_input(s, bank, &latency, row, available_at, account, true)?;
        let output = evaluate(&input)?;
        let d = output["decisions"].as_array().cloned().unwrap_or_default()
            .into_iter().find(|c| c["marketId"].as_str() == Some(id.as_str()));
        need(d.is_some()
            && output["executionAuthority"] == V::Bool(false)
            && output["competitionOrders"].as_i64() == Some(0)
            && output["fills"].as_i64() == Some(0), "NATIVE_SCOPE")?;
        let d = d.unwrap();
        let expected_action: Option<&str> = match d["action"].as_str() {
            Some("candidate_buy") => Some("buy"),
            Some("reduce") => Some("sell"),
            _ => None,
        };
        let latency_ms = latency.parse::<i64>().unwrap_or(0);
        let mut filled = false;
        {
            let w = &mut s["wallets"][&latency];
            let has_pending = !w["pending"][&id].is_null();
            let mut pending = if has_pending { w["pending"][&id].clone() } else { V::Null };
            if has_pending {
                if expected_action != pending["action"].as_str().map(Some).unwrap_or(None)
                    || now - instant_v(&pending["createdAt"])? > bank["fillPolicy"]["intentLifetimeMs"].as_i64().unwrap_or(0) as i64
                {
                    w["pending"].as_object_mut().and_then(|p| p.shift_remove(&id));
                    pending = V::Null;
                } else if request >= instant_v(&pending["createdAt"])? + latency_ms
                    && request > instant_v(&pending["createdAt"])?
                    && row["sequence"].as_i64().unwrap_or(0) > pending["sourceSequence"].as_i64().unwrap_or(0)
                    && w["consumedVersions"][&id] != b["versionKey"]
                {
                    let levels = if pending["action"].as_str() == Some("buy") {
                        b["asks"].as_array().cloned().unwrap_or_default()
                    } else {
                        b["bids"].as_array().cloned().unwrap_or_default()
                    };
                    let buy = pending["action"].as_str() == Some("buy");
                    for level in levels {
                        let price = amount_v(&level["price"])? + if buy { tick() } else { -tick() };
                        let limit = amount_v(&pending["limit"]).unwrap_or_default();
                        if price < tick() || price > BigInt::from(1_000_000i64) - tick()
                            || (buy && price > limit) || (!buy && price < limit)
                        {
                            break;
                        }
                        let depth = (level["quantity"].as_f64().unwrap_or(0.0)
                            * bank["policy"]["depthFractionBps"].as_f64().unwrap_or(0.0) / 10000.0).floor();
                        if depth < 1.0 { continue; }
                        if buy && (!w["positions"][&id].is_null()
                            || amount_v(&w["cash"])? - &price < amount(bank["policy"]["cashReserve"].as_str().unwrap_or(""))?
                            || amount_v(&w["cumulativeBuys"])? + &price > amount(bank["policy"]["cumulativeBuyBudget"].as_str().unwrap_or(""))?)
                        {
                            break;
                        }
                        let fill = json!({"intentId": pending["id"], "marketId": id,
                            "exchangeId": b["exchangeId"], "instrument": b["instrument"],
                            "action": pending["action"], "quantity": 1, "price": money(&price),
                            "fee": "0.000000", "at": available_at,
                            "requestStartedAt": b["requestStartedAt"], "receivedAt": b["receivedAt"],
                            "sourceVersion": b["versionKey"], "sourceSequence": row["sequence"],
                            "sourceHash": row["hash"], "intentSourceHash": pending["sourceHash"],
                            "researchHash": pending["researchHash"], "latencyMs": latency_ms,
                            "model": "later_rest_request_counterfactual", "reason": pending["reason"]});
                        if buy {
                            w["cash"] = json!(money(&(amount_v(&w["cash"])? - &price)));
                            w["cumulativeBuys"] = json!(money(&(amount_v(&w["cumulativeBuys"])? + &price)));
                            w["positions"][&id] = json!({"quantity": 1, "cost": fill["price"], "enteredAt": available_at});
                        } else {
                            need(!w["positions"][&id].is_null(), "SELL_WITHOUT_POSITION")?;
                            let old_cost = amount_v(&w["positions"][&id]["cost"])?;
                            w["cash"] = json!(money(&(amount_v(&w["cash"])? + &price)));
                            w["realizedPnl"] = json!(money(&(amount_v(&w["realizedPnl"])? + &price - old_cost)));
                            w["positions"].as_object_mut().and_then(|p| p.shift_remove(&id));
                            w["lastExitedAtMs"][&id] = json!(now);
                        }
                        w["fills"].as_array_mut().map(|f| f.push(fill));
                        w["consumedVersions"][&id] = b["versionKey"].clone();
                        w["pending"].as_object_mut().and_then(|p| p.shift_remove(&id));
                        filled = true;
                        break;
                    }
                }
            }
            let reason = if filled { "later_receipt_fill" } else { d["reason"].as_str().unwrap_or("") };
            let n = s["counts"][reason].as_i64().unwrap_or(0);
            s["counts"][reason] = json!(n + 1);
            s["decisions"].as_array_mut().map(|v| v.push(json!({"at": available_at,
                "latencyMs": latency_ms, "marketId": id, "sourceSequence": row["sequence"],
                "sourceHash": row["hash"], "action": d["action"], "reason": reason,
                "scenarioEdge": d.get("topScenarioEdge").cloned().unwrap_or(V::Null),
                "liquidation": d.get("liquidation").cloned().unwrap_or(V::Null)})));
            if s["decisions"].as_array().map(|a| a.len()).unwrap_or(0) > 200 {
                s["decisions"].as_array_mut().map(|v| { v.remove(0); });
            }
            // A filled sell cannot immediately re-enter from the same sampled liquidity.
            let w = &mut s["wallets"][&latency];
            if !filled && w["pending"][&id].is_null() && expected_action.is_some()
                && d["quantity"].as_f64().unwrap_or(0.0) >= 1.0
                && (expected_action != Some("buy")
                    || now as f64 - w["lastExitedAtMs"][&id].as_f64().unwrap_or(0.0) >= bank["fillPolicy"]["cooldownMs"].as_f64().unwrap_or(0.0))
            {
                let limit = if expected_action == Some("buy") {
                    d["hardLimitWithCushion"].clone()
                } else {
                    let best = amount_v(&b["bids"][0]["price"])?;
                    json!(money(&(if best > tick() { best - tick() } else { tick() })))
                };
                w["intents"] = json!(w["intents"].as_i64().unwrap_or(0) + 1);
                let intents = w["intents"].clone();
                let research = bank["research"].as_array().cloned().unwrap_or_default()
                    .into_iter().find(|r| r["marketId"].as_str() == Some(id.as_str()));
                w["pending"][&id] = json!({"id": intents, "marketId": id, "action": expected_action.unwrap(),
                    "quantity": 1, "limit": limit, "createdAt": available_at,
                    "sourceSequence": row["sequence"], "sourceHash": row["hash"],
                    "researchHash": research.map(|r| digest(&r)).unwrap_or_default(),
                    "reason": d["reason"]});
            }
        }
    }
    assert_settlement_paper_state(s, bank)?;
    Ok(())
}
/// settlementPaperReport(s, bank, at)
pub fn settlement_paper_report(s: &V, bank: &V, at: &str) -> Result<V, String> {
    assert_settlement_paper_state(s, bank)?;
    let now = instant(at)?;
    let mut wallets = Map::new();
    for (ms, w) in s["wallets"].as_object().cloned().unwrap_or_default() {
        let mut proceeds = BigInt::from(0);
        let mut complete = true;
        let mut positions = Vec::new();
        for (id, p) in w["positions"].as_object().cloned().unwrap_or_default() {
            let b = &s["books"][&id];
            let fresh = !b.is_null() && now >= instant_v(&b["availableAt"])?
                && now as f64 - instant_v(&b["receivedAt"])? as f64 <= bank["policy"]["maximumBookAgeMs"].as_f64().unwrap_or(0.0);
            let level = if fresh {
                b["bids"].as_array().cloned().unwrap_or_default().into_iter().find(|l| {
                    (l["quantity"].as_f64().unwrap_or(0.0)
                        * bank["policy"]["depthFractionBps"].as_f64().unwrap_or(0.0) / 10000.0).floor()
                        >= p["quantity"].as_f64().unwrap_or(0.0)
                })
            } else { None };
            let liquidation = level.map(|l| amount_v(&l["price"]).map(|p| money(&(p - tick())))).transpose()?;
            if liquidation.is_none() { complete = false; } else { proceeds += amount(liquidation.as_deref().unwrap())?; }
            let mut obj = json!({"marketId": id,
                "instrument": format!("{}:YES", bank["identities"][&id]["exchangeId"].as_str().unwrap_or(""))});
            for (k, v) in p.as_object().cloned().unwrap_or_default() { obj[k] = v; }
            obj["liquidation"] = liquidation.clone().map(V::from).unwrap_or(V::Null);
            obj["liquidationPnl"] = match &liquidation {
                Some(l) => json!(money(&(amount(l)? - amount_v(&p["cost"])?))),
                None => V::Null,
            };
            let expires = bank["research"].as_array().cloned().unwrap_or_default()
                .into_iter().find(|r| r["marketId"].as_str() == Some(id.as_str()))
                .map(|r| instant_v(&r["expiresAt"])).transpose()?
                .ok_or("SETTLEMENT_PAPER_RESEARCH")?;
            obj["researchExpired"] = json!(now >= expires);
            positions.push(obj);
        }
        let reserved_cost: BigInt = w["pending"].as_object().cloned().unwrap_or_default().values()
            .filter(|p| p["action"].as_str() == Some("buy"))
            .try_fold(BigInt::from(0), |n, p| Ok::<BigInt, String>(n + amount_v(&p["limit"])?))?;
        let fills = w["fills"].as_array().cloned().unwrap_or_default();
        let closed = fills.iter().filter(|f| f["action"].as_str() == Some("sell")).count();
        let recent = fills.iter().rev().take(25).rev().cloned().collect::<Vec<_>>();
        wallets.insert(ms, json!({"cash": w["cash"], "realizedPnl": w["realizedPnl"],
            "cumulativeBuys": w["cumulativeBuys"], "reservedCost": money(&reserved_cost),
            "fills": fills.len(), "closedTrades": closed, "positions": positions,
            "pending": w["pending"].as_object().map(|p| p.values().cloned().collect::<Vec<_>>()).unwrap_or_default(),
            "recentFills": recent,
            "liquidationPnl": if complete {
                json!(money(&(amount_v(&w["cash"])? + proceeds - amount(bank["fillPolicy"]["initialCash"].as_str().unwrap_or(""))?)))
            } else { V::Null }}));
    }
    Ok(json!({"mode": bank["mode"], "cohort": bank["name"], "updatedAt": at,
        "bankHash": s["bankHash"], "cutoff": bank["cutoff"], "counts": s["counts"],
        "decisions": s["decisions"], "wallets": wallets, "executionAuthority": false,
        "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false, "fees": 0,
        "holdingPolicy": "current_fundamental_value_without_fixed_timeout_or_price_only_stop",
        "limitations": ["Settlement probabilities describe scenarios, not a near-term repricing forecast or a guaranteed payout.",
            "Expired research stops new entries and requires review; existing held risk remains attributed.",
            "One-share counterfactual fills require a later REST request, causal account exclusions, one adverse tick and one-percent depth.",
            "Latency wallets share the same opportunities; do not add their P&L together.",
            "A deadline is not a settlement event. This engine never manufactures a payout."]}))
}
