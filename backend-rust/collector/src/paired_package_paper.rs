// live-paper/paired-package-paper.mjs — sequential dual-NO paired paper engine (v1).
use baal_paper::fixed::{big_amount, big_price, instant, money, SCALE, TICK};
use baal_service_runtime::json_text::digest;
use num_bigint::BigInt;
use serde_json::{json, Value as V};

/// pairedPaperHash(x) — sha256(JSON.stringify(x)).
pub fn paired_paper_hash(x: &V) -> String { digest(x) }
/// PAIRED_PAPER_POLICY (frozen).
pub fn paired_paper_policy() -> V {
    json!({"latencyMs":2000,"entryLifetimeMs":30000,"maxQuoteAgeMs":20000,"maxPairSkewMs":3000,
        "depthFractionBps":100,"maxShares":6,"maxPackageCost":"6.000000","maxOrphanPrincipal":"5.000000",
        "maxExposure":"25.000000","maxCumulativeBuys":"100.000000","minimumUnitMargin":"0.010000",
        "cooldownMs":60000,"feesBps":0})
}
/// PAIRED_PAPER_VARIANTS (frozen).
pub fn paired_paper_variants() -> V { json!(["sequential_dual_no"]) }
/// PAIRED_PAPER_CONFIRMATION (frozen).
pub fn paired_paper_confirmation() -> V { json!({"independentPairedObservations":2}) }
const LEGS: [&str; 2] = ["381", "382"];
fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(code.to_string()); }
    Ok(())
}
fn bank_check(b: &V) -> Result<(), String> {
    need(!b.is_null()
        && b["markets"].as_array().is_some_and(|m| m.len() == 2
            && LEGS.iter().all(|id| m.iter().any(|x| x["marketId"].as_str() == Some(id))))
        && paired_paper_hash(&b["policy"]) == paired_paper_hash(&paired_paper_policy()),
        "PAIRED_BANK_INVALID")?;
    instant(b["cutoff"].as_str().unwrap_or(""))?;
    Ok(())
}
/// createPairedPaper(bank)
pub fn create_paired_paper(bank: &V) -> Result<V, String> {
    bank_check(bank)?;
    Ok(json!({"bankHash":paired_paper_hash(bank),"lastEventId":0,"lastAvailableAt":V::Null,
        "books":{},"anchor":V::Null,"lastEntryAt":V::Null,"nextIntentId":1,"nextPackageId":1,
        "pending":V::Null,"active":V::Null,"packages":[],"positions":{},"fills":[],"outcomes":[],
        "cash":"100000.000000","realizedPnl":"0.000000","cumulativeBuyCost":"0.000000",
        "counts":{"books":0,"signals":0,"firstLegUnfilled":0,"secondLegUnfilled":0,
            "orphanExits":0,"completedPairs":0}}))
}
/// warmPairedPaper(s) — JS ignores the warm event list and returns s.
pub fn warm_paired_paper(s: &V) -> V { s.clone() }
fn intent(s: &mut V, market_id: &str, quantity: i64, limit: &V, action: &str, stage: &str,
    at: &str, b: &V) -> Result<V, String> {
    if s["active"].is_null() {
        // JS: s.active.id on null active — TypeError.
        return Err("Cannot read properties of null (reading 'id')".into());
    }
    let book = s["books"][market_id].clone();
    let leg = b["markets"].as_array().and_then(|ms| {
        ms.iter().find(|m| m["marketId"].as_str() == Some(market_id)).cloned()
    });
    let Some(leg) = leg else {
        return Err("Cannot read properties of undefined (reading 'exchangeId')".into());
    };
    let id = s["nextIntentId"].as_i64().unwrap_or(0);
    s["nextIntentId"] = json!(id + 1);
    let expires = instant(at)? + b["policy"]["entryLifetimeMs"].as_i64().unwrap_or(0);
    let mut o = json!({"id":id,"packageId":s["active"]["id"],"marketId":market_id,
        "exchangeId":leg["exchangeId"],"side":"no","action":action,"stage":stage,
        "quantity":quantity,"remaining":quantity,"limit":limit,"createdAt":at,
        "expiresAt":baal_service_runtime::contract::ms_to_iso(expires as f64)});
    // JS: decisionVersion:book?.versionKey — undefined is dropped by JSON.stringify.
    if let Some(v) = book.get("versionKey") { o["decisionVersion"] = v.clone(); }
    o["observedVersions"] = json!([]);
    o["filledQuantity"] = json!(0);
    Ok(o)
}
fn owned_cost(s: &V) -> BigInt {
    s["positions"].as_object().map(|o| {
        o.values().fold(BigInt::from(0), |n, p| {
            n + big_amount(p["cost"].as_str().unwrap_or("")).unwrap_or_default()
        })
    }).unwrap_or_default()
}
fn push_outcome(s: &mut V, outcome: V) {
    let mut outs = s["outcomes"].as_array().cloned().unwrap_or_default();
    outs.push(outcome);
    s["outcomes"] = json!(outs);
}
fn bump(s: &mut V, key: &str) {
    let n = s["counts"][key].as_i64().unwrap_or(0);
    s["counts"][key] = json!(n + 1);
}
fn complete_first(s: &mut V, at: &str, b: &V) -> Result<(), String> {
    let filled = s["active"]["fills"][s["active"]["first"].as_str().unwrap_or("")].as_i64().unwrap_or(0);
    if filled == 0 {
        bump(s, "firstLegUnfilled");
        push_outcome(s, json!({"packageId":s["active"]["id"],"status":"first_leg_unfilled","at":at}));
        s["active"] = V::Null;
        return Ok(());
    }
    let second = s["active"]["second"].as_str().unwrap_or("").to_string();
    let limit = s["active"]["limits"][&second].clone();
    let i = intent(s, &second, filled, &limit, "buy", "second", at, b)?;
    s["pending"] = i;
    s["active"]["stage"] = json!("second");
    Ok(())
}
fn finish_package(s: &mut V, at: &str, b: &V) -> Result<(), String> {
    let paired = LEGS.iter()
        .map(|id| s["active"]["fills"][id].as_i64().unwrap_or(0)).min().unwrap_or(0);
    let orphans: Vec<&str> = LEGS.iter()
        .filter(|id| s["active"]["fills"][id].as_i64().unwrap_or(0) > paired).copied().collect();
    s["active"]["pairedQuantity"] = json!(paired);
    s["active"]["stage"] = json!("holding");
    s["active"]["completedAt"] = json!(at);
    if paired > 0 { bump(s, "completedPairs"); }
    if !orphans.is_empty() {
        bump(s, "secondLegUnfilled");
        let id = orphans[0];
        let bid = s["books"][id]["asks"][0].clone();
        s["active"]["stage"] = json!("orphan_exit");
        let quantity = s["active"]["fills"][id].as_i64().unwrap_or(0) - paired;
        let limit = if !bid.is_null() {
            json!(money(&(BigInt::from(SCALE) - big_price(bid["price"].as_str().unwrap_or(""))? - BigInt::from(TICK))))
        } else {
            json!("0.005000")
        };
        let mut i = intent(s, id, quantity, &limit, "sell", "orphan_exit", at, b)?;
        if big_amount(i["limit"].as_str().unwrap_or(""))? < BigInt::from(TICK) {
            i["limit"] = json!("0.005000");
        }
        s["pending"] = i;
    } else {
        let mut pkgs = s["packages"].as_array().cloned().unwrap_or_default();
        pkgs.push(s["active"].clone());
        s["packages"] = json!(pkgs);
        s["active"] = V::Null;
    }
    Ok(())
}
fn terminal(s: &mut V, at: &str, b: &V) -> Result<(), String> {
    let i = s["pending"].clone();
    s["pending"] = V::Null;
    let stage = i["stage"].as_str().unwrap_or("").to_string();
    if stage == "first" {
        complete_first(s, at, b)
    } else if stage == "second" {
        finish_package(s, at, b)
    } else if i["remaining"].as_i64().unwrap_or(0) != 0 {
        s["active"]["stage"] = json!("orphan_exit_blocked");
        s["active"]["unresolvedOrphan"] = i["remaining"].clone();
        push_outcome(s, json!({"packageId":s["active"]["id"],"status":"orphan_exit_censored",
            "quantity":i["remaining"],"at":at}));
        Ok(())
    } else {
        s["active"]["stage"] = json!("holding");
        let mut pkgs = s["packages"].as_array().cloned().unwrap_or_default();
        pkgs.push(s["active"].clone());
        s["packages"] = json!(pkgs);
        s["active"] = V::Null;
        bump(s, "orphanExits");
        Ok(())
    }
}
fn fill(s: &mut V, event: &V, at: &str, b: &V) -> Result<(), String> {
    let i = s["pending"].clone();
    let book = &event["book"];
    if i.is_null() { return Ok(()); }
    if book.is_null() {
        // JS: book.marketId on undefined book — TypeError.
        return Err("Cannot read properties of undefined (reading 'marketId')".into());
    }
    if book["marketId"] != i["marketId"]
        || book["versionKey"] == i["decisionVersion"]
        || instant(book["requestStartedAt"].as_str().unwrap_or(""))?
            < instant(i["createdAt"].as_str().unwrap_or(""))? + b["policy"]["latencyMs"].as_i64().unwrap_or(0)
        || instant(book["receivedAt"].as_str().unwrap_or(""))?
            > instant(i["expiresAt"].as_str().unwrap_or(""))? {
        return Ok(());
    }
    if i["observedVersions"].as_array().is_some_and(|a| a.contains(&book["versionKey"])) {
        return Ok(());
    }
    {
        let mut observed = i["observedVersions"].as_array().cloned().unwrap_or_default();
        observed.push(book["versionKey"].clone());
        s["pending"]["observedVersions"] = json!(observed);
    }
    let buy = i["action"].as_str() == Some("buy");
    let side = if buy { &book["bids"] } else { &book["asks"] };
    if !side.is_array() {
        // JS: for..of over undefined side — TypeError.
        return Err("side is not iterable".into());
    }
    let mut remaining = i["remaining"].as_i64().unwrap_or(0);
    let mut cost = BigInt::from(0);
    let mut quantity = 0i64;
    for level in side.as_array().cloned().unwrap_or_default() {
        let lp = big_price(level["price"].as_str().unwrap_or(""))?;
        let effective = if buy {
            BigInt::from(SCALE) - &lp + BigInt::from(TICK)
        } else {
            BigInt::from(SCALE) - &lp - BigInt::from(TICK)
        };
        if effective < BigInt::from(TICK) || effective > BigInt::from(SCALE - TICK) { continue; }
        let limit = big_price(i["limit"].as_str().unwrap_or(""))?;
        if (buy && effective > limit) || (!buy && effective < limit) { break; }
        let cap = (level["quantity"].as_i64().unwrap_or(0) as i128
            * b["policy"]["depthFractionBps"].as_i64().unwrap_or(0) as i128 / 10000) as i64;
        let n = remaining.min(cap);
        if n != 0 {
            cost += BigInt::from(n) * &effective;
            quantity += n;
            remaining -= n;
        }
        if remaining == 0 { break; }
    }
    if quantity == 0 { return Ok(()); }
    let market_id = i["marketId"].as_str().unwrap_or("").to_string();
    let mut p = if s["positions"][&market_id].is_null() {
        json!({"marketId":market_id,"quantity":0,"cost":"0.000000"})
    } else {
        s["positions"][&market_id].clone()
    };
    if buy {
        p["quantity"] = json!(p["quantity"].as_i64().unwrap_or(0) + quantity);
        p["cost"] = json!(money(&(big_amount(p["cost"].as_str().unwrap_or(""))? + &cost)));
        s["cash"] = json!(money(&(big_amount(s["cash"].as_str().unwrap_or(""))? - &cost)));
        s["cumulativeBuyCost"] = json!(money(&(big_amount(s["cumulativeBuyCost"].as_str().unwrap_or(""))? + &cost)));
        let f = s["active"]["fills"][&market_id].as_i64().unwrap_or(0) + quantity;
        s["active"]["fills"][&market_id] = json!(f);
        let c = money(&(big_amount(s["active"]["costs"][&market_id].as_str().unwrap_or(""))? + &cost));
        s["active"]["costs"][&market_id] = json!(c);
    } else {
        need(p["quantity"].as_i64().unwrap_or(0) >= quantity, "PAIRED_OVERSELL")?;
        let basis = big_amount(p["cost"].as_str().unwrap_or(""))?
            * BigInt::from(quantity) / BigInt::from(p["quantity"].as_i64().unwrap_or(0));
        p["quantity"] = json!(p["quantity"].as_i64().unwrap_or(0) - quantity);
        p["cost"] = json!(money(&(big_amount(p["cost"].as_str().unwrap_or(""))? - &basis)));
        s["cash"] = json!(money(&(big_amount(s["cash"].as_str().unwrap_or(""))? + &cost)));
        s["realizedPnl"] = json!(money(&(big_amount(s["realizedPnl"].as_str().unwrap_or(""))? + &cost - &basis)));
        let f = s["active"]["fills"][&market_id].as_i64().unwrap_or(0) - quantity;
        s["active"]["fills"][&market_id] = json!(f);
        let c = money(&(big_amount(s["active"]["costs"][&market_id].as_str().unwrap_or(""))? - &basis));
        s["active"]["costs"][&market_id] = json!(c);
    }
    s["positions"][&market_id] = p;
    s["pending"]["remaining"] = json!(remaining);
    let fq = s["pending"]["filledQuantity"].as_i64().unwrap_or(0) + quantity;
    s["pending"]["filledQuantity"] = json!(fq);
    {
        let mut fills = s["fills"].as_array().cloned().unwrap_or_default();
        fills.push(json!({"intentId":i["id"],"packageId":i["packageId"],"marketId":i["marketId"],
            "action":i["action"],"side":"no","quantity":quantity,"principal":money(&cost),
            "sourceVersion":book["versionKey"],"receivedAt":book["receivedAt"],"availableAt":at,
            "model":"hypothetical_next_independent_book"}));
        s["fills"] = json!(fills);
    }
    if remaining == 0 { terminal(s, at, b)?; }
    Ok(())
}
fn eligible_books(s: &V, at: &str, b: &V) -> Result<Option<Vec<V>>, String> {
    let now = instant(at)?;
    let books: Vec<V> = LEGS.iter().map(|id| s["books"][id].clone()).collect();
    let ok = books.iter().all(|x| {
        !x.is_null()
            && x["bids"].as_array().is_some_and(|a| !a.is_empty())
            && x["asks"].as_array().is_some_and(|a| !a.is_empty())
            && instant(x["receivedAt"].as_str().unwrap_or("")).map(|r| r <= now).unwrap_or(false)
            && instant(x["receivedAt"].as_str().unwrap_or("")).map(|r| now - r <= b["policy"]["maxQuoteAgeMs"].as_i64().unwrap_or(0)).unwrap_or(false)
    }) && (instant(books[0]["receivedAt"].as_str().unwrap_or(""))?
        - instant(books[1]["receivedAt"].as_str().unwrap_or(""))?).abs()
        <= b["policy"]["maxPairSkewMs"].as_i64().unwrap_or(0);
    Ok(if ok { Some(books) } else { None })
}
/// advancePairedPaper(input, events, bank, {availableAt, allowDecisions})
pub fn advance_paired_paper(input: &V, events: &[V], bank: &V, available_at: &str,
    allow_decisions: bool) -> Result<V, String> {
    bank_check(bank)?;
    need(input["bankHash"].as_str() == Some(paired_paper_hash(bank).as_str()), "PAIRED_BANK_CHANGED")?;
    let at = instant(available_at)?;
    need(input["lastAvailableAt"].is_null()
        || at >= instant(input["lastAvailableAt"].as_str().unwrap_or(""))?,
        "PAIRED_CLOCK_ROLLBACK")?;
    let mut s = input.clone();
    for e in events {
        need(e["eventId"] == json!(s["lastEventId"].as_i64().unwrap_or(0) + 1)
            && e["tournamentId"] == bank["tournamentId"]
            && instant(e["confirmedAt"].as_str().unwrap_or(""))? <= at,
            "PAIRED_EVENT_ORDER")?;
        s["lastEventId"] = e["eventId"].clone();
        if e["kind"].as_str() == Some("invalidate") {
            if let Some(m) = s["books"].as_object_mut() {
                m.shift_remove(e["marketId"].as_str().unwrap_or(""));
            }
            s["anchor"] = V::Null;
            continue;
        }
        if !LEGS.contains(&e["marketId"].as_str().unwrap_or("")) { continue; }
        if e["book"].is_null() {
            // JS: e.book.exchangeId on undefined book — TypeError.
            return Err("Cannot read properties of undefined (reading 'exchangeId')".into());
        }
        need(e["book"]["exchangeId"].as_str()
            == bank["markets"].as_array().and_then(|ms| {
                ms.iter().find(|m| m["marketId"] == e["marketId"])
            }).and_then(|m| m["exchangeId"].as_str()),
            "PAIRED_EXCHANGE_MISMATCH")?;
        s["books"][e["marketId"].as_str().unwrap_or("")] = e["book"].clone();
        bump(&mut s, "books");
        fill(&mut s, e, available_at, bank)?;
    }
    if !s["pending"].is_null()
        && at > instant(s["pending"]["expiresAt"].as_str().unwrap_or(""))? {
        let pending = s["pending"].clone();
        push_outcome(&mut s, json!({"intentId":pending["id"],"packageId":pending["packageId"],
            "status":"expired","remaining":pending["remaining"],"at":available_at}));
        terminal(&mut s, available_at, bank)?;
    }
    let books = eligible_books(&s, available_at, bank)?;
    if allow_decisions && s["pending"].is_null()
        && s["active"]["stage"].as_str() == Some("orphan_exit_blocked") {
        let p = s["active"].clone();
        let id = LEGS.iter().find(|id| {
            p["fills"][id].as_i64().unwrap_or(0) > p["pairedQuantity"].as_i64().unwrap_or(0)
        }).copied();
        let book = id.map(|id| s["books"][id].clone()).unwrap_or(V::Null);
        if !book.is_null()
            && events.iter().any(|e| e["kind"].as_str() == Some("book") && e["marketId"].as_str() == id)
            && book["asks"].as_array().is_some_and(|a| !a.is_empty())
            && at - instant(book["receivedAt"].as_str().unwrap_or(""))?
                <= bank["policy"]["maxQuoteAgeMs"].as_i64().unwrap_or(0) {
            let unit = BigInt::from(SCALE)
                - big_price(book["asks"][0]["price"].as_str().unwrap_or(""))? - BigInt::from(TICK);
            if unit >= BigInt::from(TICK) {
                s["active"]["stage"] = json!("orphan_exit");
                let quantity = s["active"]["fills"][id.unwrap()].as_i64().unwrap_or(0)
                    - s["active"]["pairedQuantity"].as_i64().unwrap_or(0);
                let i = intent(&mut s, id.unwrap(), quantity, &json!(money(&unit)),
                    "sell", "orphan_exit", available_at, bank)?;
                s["pending"] = i;
            }
        }
    }
    if books.is_none() { s["anchor"] = V::Null; }
    if allow_decisions && s["pending"].is_null() && s["active"].is_null()
        && at > instant(bank["cutoff"].as_str().unwrap_or(""))? && books.is_some()
        && (s["lastEntryAt"].is_null()
            || at - instant(s["lastEntryAt"].as_str().unwrap_or(""))?
                >= bank["policy"]["cooldownMs"].as_i64().unwrap_or(0)) {
        let books = books.clone().unwrap();
        let mut limits = Vec::new();
        for x in &books {
            limits.push(BigInt::from(SCALE)
                - big_price(x["bids"][0]["price"].as_str().unwrap_or(""))? + BigInt::from(TICK));
        }
        let sum = &limits[0] + &limits[1];
        let versions: Vec<V> = books.iter().map(|x| x["versionKey"].clone()).collect();
        if BigInt::from(SCALE) - &sum >= big_amount(bank["policy"]["minimumUnitMargin"].as_str().unwrap_or(""))?
            && limits.iter().all(|x| *x >= BigInt::from(TICK) && *x <= BigInt::from(SCALE - TICK)) {
            if !s["anchor"].is_null()
                && versions.iter().enumerate().all(|(i, v)| *v != s["anchor"]["versions"][i]) {
                let mut q = books.iter().fold(bank["policy"]["maxShares"].as_i64().unwrap_or(0), |q, x| {
                    q.min((x["bids"][0]["quantity"].as_i64().unwrap_or(0) as i128
                        * bank["policy"]["depthFractionBps"].as_i64().unwrap_or(0) as i128 / 10000) as i64)
                });
                let exposure = owned_cost(&s);
                let used = big_amount(s["cumulativeBuyCost"].as_str().unwrap_or(""))?;
                let max_orphan = big_amount(bank["policy"]["maxOrphanPrincipal"].as_str().unwrap_or(""))?;
                while q > 0
                    && (&BigInt::from(q) * &sum > big_amount(bank["policy"]["maxPackageCost"].as_str().unwrap_or(""))?
                        || &exposure + &BigInt::from(q) * &sum > big_amount(bank["policy"]["maxExposure"].as_str().unwrap_or(""))?
                        || &used + &BigInt::from(q) * &sum > big_amount(bank["policy"]["maxCumulativeBuys"].as_str().unwrap_or(""))?
                        || limits.iter().any(|x| &BigInt::from(q) * x > max_orphan)) {
                    q -= 1;
                }
                if q != 0 {
                    let first = if limits[0] >= limits[1] { LEGS[0] } else { LEGS[1] };
                    let second = *LEGS.iter().find(|x| **x != first).unwrap();
                    let pkg_id = s["nextPackageId"].as_i64().unwrap_or(0);
                    s["nextPackageId"] = json!(pkg_id + 1);
                    s["active"] = json!({"id":pkg_id,"first":first,"second":second,"stage":"first",
                        "quantity":q,
                        "limits":{"381":money(&limits[0]),"382":money(&limits[1])},
                        "fills":{"381":0,"382":0},
                        "costs":{"381":"0.000000","382":"0.000000"},
                        "createdAt":available_at,"versions":versions,
                        "conditionalNormalSettlementOnly":true});
                    let limit = s["active"]["limits"][first].clone();
                    let i = intent(&mut s, first, q, &limit, "buy", "first", available_at, bank)?;
                    s["pending"] = i;
                    bump(&mut s, "signals");
                    s["lastEntryAt"] = json!(available_at);
                    s["anchor"] = V::Null;
                }
            } else if s["anchor"].is_null() {
                s["anchor"] = json!({"versions":versions,"at":available_at});
            }
        } else {
            s["anchor"] = V::Null;
        }
    }
    s["lastAvailableAt"] = json!(available_at);
    need(big_amount(s["cash"].as_str().unwrap_or(""))? + owned_cost(&s)
        == BigInt::from(100_000_000_000u64) + big_amount(s["realizedPnl"].as_str().unwrap_or(""))?,
        "PAIRED_CASH_BASIS_IDENTITY")?;
    Ok(s)
}
/// pairedPaperReport(s, b, at)
pub fn paired_paper_report(s: &V, b: &V, at: &str) -> Result<V, String> {
    let fresh = eligible_books(s, at, b)?;
    let mut liquidation = BigInt::from(0);
    let mut complete = true;
    let mut positions = Vec::new();
    for (_, p) in s["positions"].as_object().map(|o| {
        // JS Object.values: integer-index keys first in numeric order.
        let mut ks: Vec<&String> = o.keys().collect();
        ks.sort_by(|a, b| {
            let ia = a.parse::<u32>().ok().filter(|v| *v < u32::MAX && v.to_string() == **a);
            let ib = b.parse::<u32>().ok().filter(|v| *v < u32::MAX && v.to_string() == **b);
            match (ia, ib) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            }
        });
        ks.into_iter().map(|k| (k.clone(), o[k].clone())).collect::<Vec<_>>()
    }).unwrap_or_default() {
        if p["quantity"].as_i64().unwrap_or(0) <= 0 { continue; }
        let book = &s["books"][p["marketId"].as_str().unwrap_or("")];
        let mut remain = p["quantity"].as_i64().unwrap_or(0);
        let mut value = BigInt::from(0);
        if fresh.is_some() {
            for l in book["asks"].as_array().cloned().unwrap_or_default() {
                let n = remain.min((l["quantity"].as_i64().unwrap_or(0) as i128
                    * b["policy"]["depthFractionBps"].as_i64().unwrap_or(0) as i128 / 10000) as i64);
                let unit = BigInt::from(SCALE)
                    - big_price(l["price"].as_str().unwrap_or(""))? - BigInt::from(TICK);
                if unit >= BigInt::from(TICK) {
                    value += BigInt::from(n) * &unit;
                    remain -= n;
                }
                if remain == 0 { break; }
            }
        }
        let ready = remain == 0;
        complete = complete && ready;
        liquidation += &value;
        let mut out = p.clone();
        out["markReady"] = json!(ready);
        out["liquidationValue"] = if ready { json!(money(&value)) } else { V::Null };
        out["uncoveredQuantity"] = json!(remain);
        positions.push(out);
    }
    let q = LEGS.iter().map(|id| s["positions"][id]["quantity"].as_i64().unwrap_or(0)).min().unwrap_or(0);
    let conditional = BigInt::from(q) * BigInt::from(SCALE) - owned_cost(s);
    let pnl = if complete {
        json!(money(&(big_amount(s["cash"].as_str().unwrap_or(""))? + &liquidation - BigInt::from(100_000_000_000u64))))
    } else {
        V::Null
    };
    let fills = s["fills"].as_array().cloned().unwrap_or_default();
    let outcomes = s["outcomes"].as_array().cloned().unwrap_or_default();
    Ok(json!({"schemaVersion":1,"mode":"prospective_sequential_dual_no_paper","cutoff":b["cutoff"],
        "bankHash":s["bankHash"],"reportedAt":at,"lastEventId":s["lastEventId"],
        "lastAvailableAt":s["lastAvailableAt"],
        "wallets":{"sequential_dual_no":{"cash":s["cash"],"realizedPnl":s["realizedPnl"],
            "totalPnl":pnl,"liquidationPnl":pnl,"marksComplete":complete,"positions":positions,
            "conditionalNormalSettlementPnl":money(&conditional),"conditionalNormalPairedShares":q,
            "pending":s["pending"],"activePackage":s["active"],"counts":s["counts"],
            "cumulativeBuyCost":s["cumulativeBuyCost"],"fees":"0.000000",
            "fills":fills.len() as i64,"recentFills":fills.iter().skip(fills.len().saturating_sub(20)).collect::<Vec<_>>(),
            "recentOutcomes":outcomes.iter().skip(outcomes.len().saturating_sub(20)).collect::<Vec<_>>() }},
        "profitabilityEstablished":false,"competitionOrders":0,"executionAuthority":false,
        "assumptions":[
            "All fills are local counterfactuals with independent subsequent request,2s latency,1% depth and one adverse tick per executed side.",
            "Expensive leg is filled first; second leg is sized to actual first-leg fills. A missing second leg creates bounded orphan exposure.",
            "Paired shares are held; the conditional normal settlement floor is neither booked profit nor refund/override protection.",
            "No inference of resting queue fills, official NO REST price convention or continuous price paths."]}))
}
