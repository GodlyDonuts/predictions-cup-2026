// live-paper/baal-pair-paper-v3.mjs — sequential NO-pair paper engine (bounded confirmation anchor).
use baal_paper::fixed::{big_amount, instant, money, SCALE, TICK};
use baal_service_runtime::ledger::payload_hash;
use num_bigint::BigInt;
use serde_json::{json, Value as V};

fn policy() -> V {
    json!({"initialCash":"100.000000","quantity":1,"latencyMs":3000,
        "intentLifetimeMs":120000,"maxAgeMs":20000,"maxSkewMs":10000,"depthFractionBps":100,"adverseTicks":1,
        "maxOwnedAndReserved":"5.000000","cumulativeBuyBudget":"25.000000","minimumPackageMargin":"0.010000",
        "takeProfit":"0.010000","packageStopLoss":"0.050000","maximumPairedHoldMs":21600000,"cooldownMs":60000,"feePerShare":"0.000000"})
}
pub fn baal_pair_policy() -> V { policy() }
fn need(v: bool, c: &str) -> Result<(), String> { if !v { return Err(format!("BAAL_PAIR_{c}")); } Ok(()) }
fn inc(s: &mut V, k: &str) {
    let n = s["counts"][k].as_i64().unwrap_or(0);
    s["counts"][k] = json!(n + 1);
}
fn ids(b: &V) -> Vec<String> {
    b["review"]["legs"].as_array().cloned().unwrap_or_default().iter()
        .filter_map(|l| l["exchangeId"].as_str().map(String::from)).collect()
}
fn cost(s: &V) -> BigInt {
    s["positions"].as_object().map(|o| o.values().try_fold(BigInt::from(0), |n, p| {
        big_amount(p["cost"].as_str().unwrap_or("0")).map(|a| n + a)
    }).unwrap_or_default()).unwrap_or_default()
}
fn reserve(s: &V) -> BigInt {
    if s["pending"]["action"].as_str() == Some("buy") {
        big_amount(s["pending"]["limit"].as_str().unwrap_or("0")).unwrap_or_default()
    } else { BigInt::from(0) }
}
fn current_review(b: &V, at: &str) -> bool {
    let t = instant(at).unwrap_or(0);
    instant(b["review"]["recordedAt"].as_str().unwrap_or("")).map(|r| t >= r).unwrap_or(false)
        && instant(b["review"]["expiresAt"].as_str().unwrap_or("")).map(|r| t < r).unwrap_or(false)
}
fn check_bank(b: &V) -> Result<(), String> {
    need(b["mode"].as_str() == Some("baal_sequential_no_pair_paper")
        && payload_hash(&b["policy"]).ok() == payload_hash(&policy()).ok()
        && b["review"]["legs"].as_array().map(|a| a.len() == 2).unwrap_or(false), "BANK")?;
    instant(b["cutoff"].as_str().unwrap_or("")).map_err(|_| "BAAL_PAIR_BANK".to_string())?;
    Ok(())
}
pub fn create_baal_pair_state(bank: &V) -> Result<V, String> {
    check_bank(bank)?;
    Ok(json!({"bankHash": payload_hash(bank).unwrap_or_default(), "lastAt": bank["cutoff"],
        "cash": bank["policy"]["initialCash"], "realizedPnl": "0.000000",
        "cumulativeBuys": "0.000000", "positions": {}, "books": {}, "fills": [], "counts": {},
        "pending": null, "package": null, "anchor": null, "nextIntentId": 1,
        "nextPackageId": 1, "cooldownUntil": 0, "roundtrips": []}))
}
pub fn assert_baal_pair(s: &V, b: &V) -> Result<(), String> {
    need(s["bankHash"].as_str() == Some(payload_hash(b).unwrap_or_default().as_str()), "BANK_CHANGED")?;
    let mut buys = BigInt::from(0);
    let mut sells = BigInt::from(0);
    let mut quantities: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    let leg_ids = ids(b);
    for f in s["fills"].as_array().cloned().unwrap_or_default() {
        let v = big_amount(f["price"].as_str().unwrap_or("0")).unwrap_or_default()
            * BigInt::from(f["quantity"].as_i64().unwrap_or(0));
        need(f["quantity"].as_i64() == Some(1) && leg_ids.contains(&f["exchangeId"].as_str().unwrap_or("").to_string()), "FILL_SCOPE")?;
        let id = f["exchangeId"].as_str().unwrap_or("").to_string();
        if f["action"].as_str() == Some("buy") {
            buys += &v;
            *quantities.entry(id).or_insert(0) += 1;
        } else {
            need(f["action"].as_str() == Some("sell"), "FILL_ACTION")?;
            sells += &v;
            *quantities.entry(id.clone()).or_insert(0) -= 1;
            need(*quantities.get(&id).unwrap_or(&0) >= 0, "OVERSELL")?;
        }
    }
    need(big_amount(s["cash"].as_str().unwrap_or("0")).unwrap_or_default()
        == big_amount(b["policy"]["initialCash"].as_str().unwrap_or("0")).unwrap_or_default() - &buys + &sells
        && big_amount(s["cash"].as_str().unwrap_or("0")).unwrap_or_default() >= reserve(s), "CASH_CONSERVATION")?;
    need(big_amount(s["realizedPnl"].as_str().unwrap_or("0")).unwrap_or_default()
        == &sells - &buys + cost(s)
        && big_amount(s["cumulativeBuys"].as_str().unwrap_or("0")).unwrap_or_default() == buys, "FIFO_CONSERVATION")?;
    need(cost(s) + reserve(s) <= big_amount(b["policy"]["maxOwnedAndReserved"].as_str().unwrap_or("0")).unwrap_or_default()
        && &buys + reserve(s) <= big_amount(b["policy"]["cumulativeBuyBudget"].as_str().unwrap_or("0")).unwrap_or_default(), "CAPITAL")?;
    for id in &leg_ids {
        need(s["positions"][id]["quantity"].as_i64().unwrap_or(0) == *quantities.get(id).unwrap_or(&0), "INVENTORY_CONSERVATION")?;
    }
    need(s["positions"].as_object().map(|o| o.values().all(|p| p["quantity"].as_i64() == Some(1))).unwrap_or(true)
        && (s["pending"].is_null() || s["pending"]["quantity"].as_i64() == Some(1)), "QUANTITY")?;
    if s["pending"]["action"].as_str() == Some("sell") {
        need(!s["positions"][s["pending"]["exchangeId"].as_str().unwrap_or("")].is_null(), "COVERED_SELL")?;
    }
    if s["pending"]["action"].as_str() == Some("buy") {
        need(s["positions"][s["pending"]["exchangeId"].as_str().unwrap_or("")].is_null(), "DUPLICATE_BUY")?;
    }
    need(s["package"].is_null() == !(cost(s) > BigInt::from(0) || !s["pending"].is_null()), "PACKAGE_OWNERSHIP")?;
    Ok(())
}
fn fresh(book: &V, account: &V, at: &str, b: &V) -> bool {
    if book.is_null() { return false; }
    let t = instant(at).unwrap_or(0);
    let max_age = b["policy"]["maxAgeMs"].as_i64().unwrap_or(0);
    instant(book["receivedAt"].as_str().unwrap_or("")).map(|r| r <= t).unwrap_or(false)
        && instant(book["requestStartedAt"].as_str().unwrap_or("")).map(|r| t - r <= max_age).unwrap_or(false)
        && crate::probability_receipt::probability_account_gate(account, book, at).is_none()
}
fn pair_books(s: &V, account: &V, at: &str, b: &V) -> Option<Vec<V>> {
    let books: Vec<V> = ids(b).iter().map(|id| s["books"][id].clone()).collect();
    if !books.iter().all(|x| fresh(x, account, at, b)) { return None; }
    let recv: Vec<i64> = books.iter().filter_map(|x| instant(x["receivedAt"].as_str().unwrap_or("")).ok()).collect();
    let req: Vec<i64> = books.iter().filter_map(|x| instant(x["requestStartedAt"].as_str().unwrap_or("")).ok()).collect();
    if recv.iter().max().unwrap_or(&0) - req.iter().min().unwrap_or(&0) <= b["policy"]["maxSkewMs"].as_i64().unwrap_or(0) {
        Some(books)
    } else { None }
}
fn executable_level(book: &V, action: &str, b: &V) -> Option<(BigInt, i64, usize, V)> {
    let side = if action == "buy" { "bids" } else { "asks" };
    let bps = BigInt::from(b["policy"]["depthFractionBps"].as_i64().unwrap_or(0));
    for (index, level) in book[side].as_array().cloned().unwrap_or_default().iter().enumerate() {
        let quantity = (BigInt::from(level["quantity"].as_i64().unwrap_or(0)) * &bps / BigInt::from(10000))
            .try_into().unwrap_or(0);
        let unit = if action == "buy" {
            BigInt::from(SCALE) - big_amount(level["price"].as_str().unwrap_or("0")).unwrap_or_default() + BigInt::from(TICK)
        } else {
            BigInt::from(SCALE) - big_amount(level["price"].as_str().unwrap_or("0")).unwrap_or_default() - BigInt::from(TICK)
        };
        if quantity >= 1 && unit >= BigInt::from(TICK) && unit <= BigInt::from(SCALE - TICK) {
            return Some((unit, quantity, index, level["price"].clone()));
        }
    }
    None
}
fn buy_unit(book: &V, b: &V) -> BigInt {
    executable_level(book, "buy", b).map(|(u, _, _, _)| u).unwrap_or_else(|| BigInt::from(SCALE))
}
fn sell_unit(book: &V, b: &V) -> BigInt {
    executable_level(book, "sell", b).map(|(u, _, _, _)| u).unwrap_or_else(|| BigInt::from(0))
}
fn depth(book: &V, action: &str, b: &V) -> i64 {
    executable_level(book, action, b).map(|(_, q, _, _)| q).unwrap_or(0)
}
fn pending(s: &mut V, b: &V, id: &str, action: &str, stage: &str, limit: BigInt, at: &str) -> Result<(), String> {
    let book = s["books"][id].clone();
    let leg = b["review"]["legs"].as_array().unwrap().iter()
        .find(|l| l["exchangeId"].as_str() == Some(id)).cloned().unwrap_or(V::Null);
    need(limit >= BigInt::from(TICK) && limit <= BigInt::from(SCALE - TICK)
        && &limit % BigInt::from(TICK) == BigInt::from(0), "LIMIT")?;
    let intent_id = s["nextIntentId"].as_i64().unwrap_or(1);
    s["pending"] = json!({"id": intent_id, "packageId": s["package"]["id"], "marketId": leg["marketId"],
        "exchangeId": id, "action": action, "stage": stage, "quantity": 1, "limit": money(&limit),
        "createdAt": at, "expiresAt": baal_service_runtime::contract::ms_to_iso(
            instant(at).unwrap_or(0) as f64 + b["policy"]["intentLifetimeMs"].as_f64().unwrap_or(0.0)),
        "decisionVersion": if book["versionKey"].is_null() { V::Null } else { book["versionKey"].clone() }});
    s["nextIntentId"] = json!(intent_id + 1);
    inc(s, &format!("intent_{stage}"));
    Ok(())
}
fn finish(s: &mut V, _b: &V, at: &str, reason: &str) -> Result<(), String> {
    need(s["positions"].as_object().map(|o| o.is_empty()).unwrap_or(true), "FINISH_OWNED")?;
    let p = s["package"].clone();
    let rts = s["roundtrips"].as_array().cloned().unwrap_or_default();
    let mut rts = rts;
    rts.push(json!({"id": p["id"], "openedAt": p["createdAt"], "closedAt": at, "reason": reason,
        "buyCost": p["buyCost"], "saleProceeds": p["saleProceeds"],
        "realizedPnl": money(&(big_amount(p["saleProceeds"].as_str().unwrap_or("0")).unwrap_or_default()
            - big_amount(p["buyCost"].as_str().unwrap_or("0")).unwrap_or_default())),
        "completedPair": p["completedPair"]}));
    s["roundtrips"] = json!(rts);
    s["package"] = V::Null;
    s["pending"] = V::Null;
    s["anchor"] = V::Null;
    s["cooldownUntil"] = json!(instant(at).unwrap_or(0) + 0); // +cooldownMs applied by caller via b
    Ok(())
}
fn finish_with_cooldown(s: &mut V, b: &V, at: &str, reason: &str) -> Result<(), String> {
    finish(s, b, at, reason)?;
    s["cooldownUntil"] = json!(instant(at).unwrap_or(0) + b["policy"]["cooldownMs"].as_i64().unwrap_or(0));
    inc(s, "closed_packages");
    Ok(())
}
fn orphan(s: &mut V, b: &V, at: &str, reason: &str) -> Result<(), String> {
    s["pending"] = V::Null;
    s["anchor"] = V::Null;
    if s["positions"].as_object().map(|o| o.is_empty()).unwrap_or(true) {
        return finish_with_cooldown(s, b, at, reason);
    }
    s["package"]["stage"] = json!("orphan");
    s["package"]["orphanReason"] = json!(reason);
    inc(s, "orphan_obligation");
    Ok(())
}
fn expired(s: &mut V, b: &V, at: &str, reason: &str) -> Result<(), String> {
    let i = s["pending"].clone();
    need(!i.is_null(), "EXPIRY")?;
    s["pending"] = V::Null;
    inc(s, "intent_expired");
    let stage = i["stage"].as_str().unwrap_or("");
    if stage == "entry_first" {
        finish_with_cooldown(s, b, at, reason)?;
    } else if stage == "entry_second" || stage == "exit_second" {
        orphan(s, b, at, reason)?;
    } else if stage == "exit_first" {
        s["package"]["stage"] = json!("holding");
        s["package"]["exitLimits"] = V::Null;
    } else {
        need(stage == "orphan", "EXPIRY_STAGE")?;
    }
    Ok(())
}
fn fill(s: &mut V, book: &V, at: &str, b: &V) -> Result<(), String> {
    let i = s["pending"].clone();
    if i.is_null() || i["exchangeId"].as_str() != book["exchangeId"].as_str()
        || book["versionKey"] == i["decisionVersion"]
        || instant(book["requestStartedAt"].as_str().unwrap_or("")).unwrap_or(0)
            < instant(i["createdAt"].as_str().unwrap_or("")).unwrap_or(0) + b["policy"]["latencyMs"].as_i64().unwrap_or(0)
        || instant(book["receivedAt"].as_str().unwrap_or("")).unwrap_or(0)
            > instant(i["expiresAt"].as_str().unwrap_or("")).unwrap_or(0)
        || instant(at).unwrap_or(0) > instant(i["expiresAt"].as_str().unwrap_or("")).unwrap_or(0)
        || depth(book, i["action"].as_str().unwrap_or(""), b) < 1 {
        return Ok(());
    }
    let unit = if i["action"].as_str() == Some("buy") { buy_unit(book, b) } else { sell_unit(book, b) };
    let i_limit = big_amount(i["limit"].as_str().unwrap_or("0")).unwrap_or_default();
    if unit < BigInt::from(TICK) || unit > BigInt::from(SCALE - TICK)
        || (i["action"].as_str() == Some("buy") && unit > i_limit)
        || (i["action"].as_str() == Some("sell") && unit < i_limit) {
        return Ok(());
    }
    let exid = i["exchangeId"].as_str().unwrap_or("").to_string();
    if i["action"].as_str() == Some("buy") {
        need(s["positions"][&exid].is_null(), "DUPLICATE_POSITION")?;
        s["positions"][&exid] = json!({"marketId": book["marketId"], "exchangeId": book["exchangeId"],
            "quantity": 1, "cost": money(&unit), "enteredAt": at});
        s["cash"] = json!(money(&(big_amount(s["cash"].as_str().unwrap_or("0")).unwrap_or_default() - &unit)));
        s["cumulativeBuys"] = json!(money(&(big_amount(s["cumulativeBuys"].as_str().unwrap_or("0")).unwrap_or_default() + &unit)));
        let bc = big_amount(s["package"]["buyCost"].as_str().unwrap_or("0")).unwrap_or_default() + &unit;
        s["package"]["buyCost"] = json!(money(&bc));
    } else {
        need(!s["positions"][&exid].is_null(), "COVERED_EXIT")?;
        let position = s["positions"][&exid].clone();
        s["cash"] = json!(money(&(big_amount(s["cash"].as_str().unwrap_or("0")).unwrap_or_default() + &unit)));
        s["realizedPnl"] = json!(money(&(big_amount(s["realizedPnl"].as_str().unwrap_or("0")).unwrap_or_default()
            + &unit - big_amount(position["cost"].as_str().unwrap_or("0")).unwrap_or_default())));
        let sp = big_amount(s["package"]["saleProceeds"].as_str().unwrap_or("0")).unwrap_or_default() + &unit;
        s["package"]["saleProceeds"] = json!(money(&sp));
        s["positions"].as_object_mut().unwrap().remove(&exid);
    }
    let mut fills = s["fills"].as_array().cloned().unwrap_or_default();
    let mut f = i.clone();
    f["price"] = json!(money(&unit));
    f["at"] = json!(at);
    f["fees"] = json!("0.000000");
    f["model"] = json!("counterfactual_later_independent_book_not_proven_execution");
    f["source"] = json!({"requestStartedAt": book["requestStartedAt"], "receivedAt": book["receivedAt"],
        "version": book["versionKey"], "publicationSequence": book["publicationSequence"],
        "publicationHash": book["publicationHash"], "eligibleDepth": depth(book, i["action"].as_str().unwrap_or(""), b),
        "levelIndex": executable_level(book, i["action"].as_str().unwrap_or(""), b).map(|(_, _, i, _)| json!(i)).unwrap_or(V::Null),
        "rawYesPrice": executable_level(book, i["action"].as_str().unwrap_or(""), b).map(|(_, _, _, p)| p).unwrap_or(V::Null)});
    fills.push(f);
    s["fills"] = json!(fills);
    inc(s, &format!("hypothetical_{}", i["action"].as_str().unwrap_or("")));
    s["pending"] = V::Null;
    s["anchor"] = V::Null;
    let stage = i["stage"].as_str().unwrap_or("").to_string();
    if stage == "entry_first" {
        s["package"]["stage"] = json!("entry_second");
        let second = s["package"]["second"].as_str().unwrap_or("").to_string();
        let limit = big_amount(s["package"]["entryLimits"][&second].as_str().unwrap_or("0")).unwrap_or_default();
        pending(s, b, &second, "buy", "entry_second", limit, at)?;
    } else if stage == "entry_second" {
        need(cost(s) <= BigInt::from(SCALE) - big_amount(b["policy"]["minimumPackageMargin"].as_str().unwrap_or("0")).unwrap_or_default(), "PACKAGE_MARGIN")?;
        s["package"]["stage"] = json!("holding");
        s["package"]["completedPair"] = json!(true);
        s["package"]["completedAt"] = json!(at);
        inc(s, "completed_pairs");
    } else if stage == "exit_first" {
        s["package"]["stage"] = json!("exit_second");
        let second_exit = s["package"]["secondExit"].as_str().unwrap_or("").to_string();
        let limit = big_amount(s["package"]["exitLimits"][&second_exit].as_str().unwrap_or("0")).unwrap_or_default();
        pending(s, b, &second_exit, "sell", "exit_second", limit, at)?;
    } else if stage == "exit_second" || stage == "orphan" {
        let reason = if stage == "orphan" {
            s["package"]["orphanReason"].as_str().unwrap_or("").to_string()
        } else {
            s["package"]["exitReason"].as_str().unwrap_or("").to_string()
        };
        finish_with_cooldown(s, b, at, &reason)?;
    } else {
        return need(false, "FILL_STAGE");
    }
    Ok(())
}
pub fn advance_baal_pair(input: &V, event: &V, bank: &V) -> Result<V, String> {
    check_bank(bank)?;
    assert_baal_pair(input, bank)?;
    let mut s = input.clone();
    let b = bank;
    let at = event["availableAt"].as_str().unwrap_or("");
    need(instant(at).unwrap_or(0) >= instant(s["lastAt"].as_str().unwrap_or("")).unwrap_or(0)
        && instant(at).unwrap_or(0) >= instant(b["cutoff"].as_str().unwrap_or("")).unwrap_or(0), "CLOCK")?;
    s["lastAt"] = json!(at);
    if !s["anchor"].is_null()
        && instant(at).unwrap_or(0) - instant(s["anchor"]["at"].as_str().unwrap_or("")).unwrap_or(0)
            > b["policy"]["intentLifetimeMs"].as_i64().unwrap_or(0) {
        s["anchor"] = V::Null;
        inc(&mut s, "confirmation_anchor_expired");
    }
    if event["kind"].as_str() == Some("boundary") {
        s["books"] = json!({});
        s["anchor"] = V::Null;
        if !s["pending"].is_null() { expired(&mut s, b, at, "source_boundary")?; }
        inc(&mut s, "source_boundary");
        assert_baal_pair(&s, b)?;
        return Ok(s);
    }
    let leg_ids = ids(b);
    need(event["kind"].as_str() == Some("book")
        && b["review"]["legs"].as_array().unwrap().iter().any(|l|
            l["exchangeId"].as_str() == event["book"]["exchangeId"].as_str()
            && l["marketId"].as_str() == event["book"]["marketId"].as_str()), "BOOK_SCOPE")?;
    let book = &event["book"];
    let account = &event["accountEligibility"];
    let stale: Vec<String> = s["books"].as_object().map(|o| o.iter()
        .filter(|(_, mark)| !fresh(mark, account, at, b)).map(|(id, _)| id.clone()).collect()).unwrap_or_default();
    for id in stale { s["books"].as_object_mut().unwrap().remove(&id); }
    for mark in event["pairBooks"].as_array().cloned().unwrap_or_default() {
        let exid = mark["exchangeId"].as_str().unwrap_or("").to_string();
        if b["review"]["legs"].as_array().unwrap().iter().any(|l|
                l["exchangeId"].as_str() == mark["exchangeId"].as_str()
                && l["marketId"].as_str() == mark["marketId"].as_str())
            && fresh(&mark, account, at, b) {
            s["books"][&exid] = mark;
        }
    }
    if fresh(book, account, at, b) {
        s["books"][book["exchangeId"].as_str().unwrap_or("")] = book.clone();
    } else {
        s["books"].as_object_mut().unwrap().remove(book["exchangeId"].as_str().unwrap_or(""));
    }
    let excluded_leg = account["excludedExchangeIds"].as_array().map(|a|
        leg_ids.iter().any(|id| a.iter().any(|x| x.as_str() == Some(id.as_str())))).unwrap_or(false);
    if !s["pending"].is_null()
        && (instant(at).unwrap_or(0) > instant(s["pending"]["expiresAt"].as_str().unwrap_or("")).unwrap_or(0)
            || (s["pending"]["action"].as_str() == Some("buy") && (!current_review(b, at) || excluded_leg))) {
        expired(&mut s, b, at, if excluded_leg { "account_exposure" } else { "intent_or_review_expiry" })?;
    }
    if fresh(book, account, at, b) {
        fill(&mut s, book, at, b)?;
    } else {
        inc(&mut s, "unavailable_or_excluded_book");
    }
    let books = pair_books(&s, account, at, b);
    let has_package = !s["package"].is_null();
    if s["package"]["stage"].as_str() == Some("orphan") && s["pending"].is_null() {
        let id = s["positions"].as_object().map(|o| o.keys().next().cloned()).flatten().unwrap_or_default();
        let mark = s["books"][&id].clone();
        if book["exchangeId"].as_str() == Some(id.as_str()) && fresh(&mark, account, at, b)
            && sell_unit(&mark, b) >= BigInt::from(TICK) {
            pending(&mut s, b, &id, "sell", "orphan", sell_unit(&mark, b), at)?;
        }
    } else if s["package"]["stage"].as_str() == Some("holding") && s["pending"].is_null() && books.is_some() {
        let books = books.clone().unwrap();
        let units: Vec<BigInt> = books.iter().map(|x| sell_unit(x, b)).collect();
        let proceeds: BigInt = units.iter().fold(BigInt::from(0), |a, v| a + v);
        let basis = cost(&s);
        let p = s["package"].clone();
        let stop = instant(at).unwrap_or(0) - instant(p["completedAt"].as_str().unwrap_or("")).unwrap_or(0)
            >= b["policy"]["maximumPairedHoldMs"].as_i64().unwrap_or(0)
            || &basis - &proceeds >= big_amount(b["policy"]["packageStopLoss"].as_str().unwrap_or("0")).unwrap_or_default();
        let take = big_amount(b["policy"]["takeProfit"].as_str().unwrap_or("0")).unwrap_or_default();
        if units.iter().all(|x| *x >= BigInt::from(TICK)) && books.iter().all(|x| depth(x, "sell", b) >= 1)
            && (proceeds >= &basis + &take || stop) {
            let reason = if proceeds >= &basis + take { "paired_profit_target" }
                else if instant(at).unwrap_or(0) - instant(p["completedAt"].as_str().unwrap_or("")).unwrap_or(0)
                    >= b["policy"]["maximumPairedHoldMs"].as_i64().unwrap_or(0) { "paired_holding_limit" }
                else { "paired_loss_limit" };
            s["package"]["exitReason"] = json!(reason);
            let exit_limits: serde_json::Map<String, V> = books.iter().zip(units.iter())
                .map(|(x, u)| (x["exchangeId"].as_str().unwrap_or("").to_string(), json!(money(u)))).collect();
            s["package"]["exitLimits"] = json!(exit_limits);
            let mut positions: Vec<(String, BigInt)> = s["positions"].as_object().unwrap().iter()
                .map(|(k, v)| (k.clone(), big_amount(v["cost"].as_str().unwrap_or("0")).unwrap_or_default())).collect();
            positions.sort_by(|a, z| z.1.cmp(&a.1));
            let first = positions[0].0.clone();
            let second = leg_ids.iter().find(|id| **id != first).cloned().unwrap_or_default();
            s["package"]["secondExit"] = json!(second);
            s["package"]["stage"] = json!("exit_first");
            let limit = big_amount(s["package"]["exitLimits"][&first].as_str().unwrap_or("0")).unwrap_or_default();
            pending(&mut s, b, &first, "sell", "exit_first", limit, at)?;
            inc(&mut s, "paired_exit_obligation");
        }
    } else if !has_package && s["pending"].is_null()
        && instant(at).unwrap_or(0) >= s["cooldownUntil"].as_i64().unwrap_or(0)
        && books.is_some() && event["admission"]["positiveConditionalCapacity"].as_bool().unwrap_or(false)
        && current_review(b, at) {
        let books = books.clone().unwrap();
        let all_after_cutoff = books.iter().all(|x|
            instant(x["requestStartedAt"].as_str().unwrap_or("")).unwrap_or(0)
                > instant(b["cutoff"].as_str().unwrap_or("")).unwrap_or(0));
        if all_after_cutoff {
            let units: Vec<BigInt> = books.iter().map(|x| buy_unit(x, b)).collect();
            let sum: BigInt = units.iter().fold(BigInt::from(0), |a, v| a + v);
            let versions: Vec<V> = books.iter().map(|x| x["versionKey"].clone()).collect();
            if units.iter().all(|v| *v >= BigInt::from(TICK) && *v <= BigInt::from(SCALE - TICK))
                && books.iter().all(|x| depth(x, "buy", b) >= 1)
                && sum <= BigInt::from(SCALE) - big_amount(b["policy"]["minimumPackageMargin"].as_str().unwrap_or("0")).unwrap_or_default()
                && sum <= big_amount(b["policy"]["maxOwnedAndReserved"].as_str().unwrap_or("0")).unwrap_or_default()
                && big_amount(s["cumulativeBuys"].as_str().unwrap_or("0")).unwrap_or_default() + &sum
                    <= big_amount(b["policy"]["cumulativeBuyBudget"].as_str().unwrap_or("0")).unwrap_or_default()
                && big_amount(s["cash"].as_str().unwrap_or("0")).unwrap_or_default() >= sum {
                let anchor_matches = !s["anchor"].is_null() && {
                    let av = s["anchor"]["versions"].as_array().cloned().unwrap_or_default();
                    versions.iter().enumerate().all(|(i, v)| av.get(i) != Some(v))
                };
                let anchor_requests_later = books.iter().all(|x|
                    instant(x["requestStartedAt"].as_str().unwrap_or("")).unwrap_or(0)
                        > instant(s["anchor"]["at"].as_str().unwrap_or("")).unwrap_or(0));
                if anchor_matches && anchor_requests_later {
                    let first_idx = if units[0] <= units[1] { 0 } else { 1 };
                    let first = books[first_idx]["exchangeId"].as_str().unwrap_or("").to_string();
                    let second = leg_ids.iter().find(|id| **id != first).cloned().unwrap_or_default();
                    let pkg_id = s["nextPackageId"].as_i64().unwrap_or(1);
                    let entry_limits: serde_json::Map<String, V> = books.iter().zip(units.iter())
                        .map(|(x, u)| (x["exchangeId"].as_str().unwrap_or("").to_string(), json!(money(u)))).collect();
                    s["package"] = json!({"id": pkg_id, "first": first, "second": second,
                        "stage": "entry_first", "createdAt": at, "completedPair": false,
                        "buyCost": "0.000000", "saleProceeds": "0.000000",
                        "entryLimits": entry_limits, "entryVersions": versions});
                    s["nextPackageId"] = json!(pkg_id + 1);
                    let limit = big_amount(s["package"]["entryLimits"][&first].as_str().unwrap_or("0")).unwrap_or_default();
                    pending(&mut s, b, &first, "buy", "entry_first", limit, at)?;
                    s["anchor"] = V::Null;
                    inc(&mut s, "paired_entry_obligation");
                } else if s["anchor"].is_null() {
                    s["anchor"] = json!({"versions": versions, "at": at});
                }
            } else {
                s["anchor"] = V::Null;
                inc(&mut s, "no_package_capacity");
            }
        }
    } else if !has_package {
        if !fresh(book, account, at, b) || excluded_leg || !current_review(b, at) || books.is_some() {
            s["anchor"] = V::Null;
        }
    }
    assert_baal_pair(&s, b)?;
    Ok(s)
}
pub fn baal_pair_report(s: &V, b: &V, at: &str) -> Result<V, String> {
    assert_baal_pair(s, b)?;
    let mut liquidation = BigInt::from(0);
    let mut complete = true;
    let positions: Vec<V> = s["positions"].as_object().map(|o| o.values().map(|p| {
        let book = &s["books"][p["exchangeId"].as_str().unwrap_or("")];
        let t = instant(at).unwrap_or(0);
        let fresh_mark = !book.is_null()
            && instant(book["receivedAt"].as_str().unwrap_or("")).map(|r| t >= r).unwrap_or(false)
            && instant(book["requestStartedAt"].as_str().unwrap_or("")).map(|r| t - r <= b["policy"]["maxAgeMs"].as_i64().unwrap_or(0)).unwrap_or(false)
            && depth(book, "sell", b) >= 1 && sell_unit(book, b) >= BigInt::from(TICK);
        let value = if fresh_mark { Some(money(&sell_unit(book, b))) } else { None };
        if let Some(v) = &value { liquidation += big_amount(v).unwrap_or_default(); } else { complete = false; }
        let mut row = p.clone();
        row["estimatedLiquidationProceeds"] = value.map(|v| json!(v)).unwrap_or(V::Null);
        row["unsoldShares"] = json!(if row["estimatedLiquidationProceeds"].is_null() { 1 } else { 0 });
        row["quoteReceivedAt"] = book["receivedAt"].clone();
        row["quoteRequestStartedAt"] = book["requestStartedAt"].clone();
        row["quoteAgeMs"] = if book.is_null() { V::Null } else {
            json!(t - instant(book["requestStartedAt"].as_str().unwrap_or("")).unwrap_or(0)) };
        row
    }).collect()).unwrap_or_default();
    let paired = positions.len() == 2 && s["package"]["completedPair"].as_bool().unwrap_or(false);
    let review_current = current_review(b, at);
    Ok(json!({"schemaVersion":1,"mode":b["mode"],"cohort":b["name"],"bankHash":s["bankHash"],
        "cutoff":b["cutoff"],"updatedAt":at,"reviewExpiresAt":b["review"]["expiresAt"],
        "cash":s["cash"],"realizedPnl":s["realizedPnl"],
        "estimatedLiquidationPnl": if complete { json!(money(&(big_amount(s["cash"].as_str().unwrap_or("0")).unwrap_or_default() + liquidation - big_amount(b["policy"]["initialCash"].as_str().unwrap_or("0")).unwrap_or_default()))) } else { V::Null },
        "positions":positions,"pending":s["pending"],"package":s["package"],
        "reserved":money(&reserve(s)),"cumulativeBuys":s["cumulativeBuys"],"counts":s["counts"],
        "fills":s["fills"].as_array().map(|a| a.len()).unwrap_or(0),
        "recentFills":s["fills"].as_array().cloned().unwrap_or_default().iter().rev().take(10).rev().cloned().collect::<Vec<_>>(),
        "closedPackages":s["roundtrips"].as_array().cloned().unwrap_or_default().iter()
            .filter(|x| big_amount(x["buyCost"].as_str().unwrap_or("0")).unwrap_or_default() > BigInt::from(0)).count(),
        "recentRoundtrips":s["roundtrips"].as_array().cloned().unwrap_or_default().iter().rev().take(10).rev().cloned().collect::<Vec<_>>(),
        "fees":"0.000000",
        "ordinaryConditionalSettlementSurplus": if paired && review_current {
            json!(money(&(BigInt::from(SCALE) - cost(s)))) } else { V::Null },
        "zeroPayoutExceptionLoss": money(&-cost(s)),"reviewCurrent":review_current,
        "executionAuthority":false,"competitionOrders":0,"networkRequests":0,"profitabilityEstablished":false,
        "limitations":["Two sequential hypothetical fills are not atomic execution or evidence of queue priority.","Ordinary settlement surplus is conditional on reviewed rules and is never booked as realized or liquidation P&L.","A failed hedge or second exit creates an orphan obligation that may realize a loss.","Matched inventory remains bounded by the loss and six-hour holding limits; all costs and losses stay in the original journal."]}))
}
