// live-paper/dual-no-pair-paper-v1.mjs — sequential dual-NO counterfactual paper ledger.
use baal_execution::core::payload_hash;
use baal_paper::fixed::{big_amount, money, SCALE, TICK};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_int, js_truthy};
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};

/// DUAL_NO_PAIR_POLICY (frozen)
pub fn dual_no_pair_policy() -> V {
    json!({"initialCash":"100.000000","quantity":1,"latencyMs":2000,"intentLifetimeMs":120000,
        "maxAgeMs":20000,"maxSkewMs":10000,"depthFractionBps":100,"adverseTicks":1,
        "maxOwnedAndReserved":"1.000000","cumulativeBuyBudget":"1.000000","minimumPackageMargin":"0.010000",
        "takeProfit":"0.010000","packageStopLoss":"1.000000","maximumPairedHoldMs":3024000000i64,
        "cooldownMs":60000,"feePerShare":"0.000000"})
}
fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("DUAL_NO_PAIR_{}", code)); } Ok(()) }
fn inc(s: &mut V, k: &str) { let n = s["counts"][k].as_i64().unwrap_or(0) + 1; s["counts"][k] = json!(n); }
fn ids(b: &V) -> Vec<V> { b["review"]["legs"].as_array().map(|l| l.iter().map(|x| x["exchangeId"].clone()).collect()).unwrap_or_default() }
fn amount(v: &V) -> Result<BigInt, String> { big_amount(v.as_str().unwrap_or("")) }
fn instant(v: &V) -> Result<i64, String> { crate::model::instant(v.as_str().unwrap_or("")) }
fn instant_str(s: &str) -> Result<i64, String> { crate::model::instant(s) }
fn big_int_of(v: &V) -> Result<BigInt, String> {
    match v {
        V::Number(n) => n.as_i64().map(BigInt::from).or_else(|| n.as_u64().map(BigInt::from))
            .ok_or_else(|| "Cannot convert value to a BigInt".to_string()),
        V::String(s) => s.parse::<BigInt>().map_err(|_| "Cannot convert value to a BigInt".to_string()),
        V::Bool(x) => Ok(BigInt::from(*x as i64)),
        _ => Err("Cannot convert value to a BigInt".to_string()),
    }
}
fn cost(s: &V) -> Result<BigInt, String> {
    let mut n = BigInt::from(0);
    if let Some(m) = s["positions"].as_object() { for p in m.values() { n += amount(&p["cost"])?; } }
    Ok(n)
}
fn reserve(s: &V) -> Result<BigInt, String> {
    if s["pending"]["action"].as_str() == Some("buy") {
        if s["pending"]["stage"].as_str() == Some("entry_first") {
            let mut n = BigInt::from(0);
            if let Some(m) = s["package"]["entryLimits"].as_object() { for v in m.values() { n += amount(v)?; } }
            Ok(n)
        } else { amount(&s["pending"]["limit"]) }
    } else { Ok(BigInt::from(0)) }
}
fn current_review(b: &V, at: &str) -> Result<bool, String> {
    let at_ms = instant_str(at)?;
    Ok(at_ms >= instant(&b["review"]["recordedAt"])? && at_ms < instant(&b["review"]["expiresAt"])?)
}
fn check_bank(b: &V) -> Result<(), String> {
    crate::dual_no_review_v1::validate_dual_no_review(&b["review"])?;
    need(b["mode"].as_str() == Some("dual_no_sequential_paper_v1")
        && payload_hash(&b["policy"])? == payload_hash(&dual_no_pair_policy())?
        && b["review"]["legs"].as_array().is_some_and(|l| l.len() == 2), "BANK")?;
    instant(&b["cutoff"])?;
    Ok(())
}
/// createDualNoPairState(bank)
pub fn create_dual_no_pair_state(bank: &V) -> Result<V, String> {
    check_bank(bank)?;
    Ok(json!({"bankHash":payload_hash(bank)?,"lastAt":bank["cutoff"].clone(),"cash":bank["policy"]["initialCash"].clone(),
        "realizedPnl":"0.000000","cumulativeBuys":"0.000000","positions":{},"books":{},"fills":[],"counts":{},
        "pending":V::Null,"package":V::Null,"anchor":V::Null,"nextIntentId":1,"nextPackageId":1,"cooldownUntil":0,"roundtrips":[]}))
}
/// assertDualNoPair(s, b)
pub fn assert_dual_no_pair(s: &V, b: &V) -> Result<(), String> {
    need(s["bankHash"].as_str() == Some(payload_hash(b)?.as_str()), "BANK_CHANGED")?;
    let leg_ids = ids(b);
    let mut buys = BigInt::from(0);
    let mut sells = BigInt::from(0);
    let mut quantities: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for f in s["fills"].as_array().cloned().unwrap_or_default() {
        let v = amount(&f["price"])? * big_int_of(&f["quantity"])?;
        need(js_int(&f["quantity"]) == Some(1) && leg_ids.iter().any(|id| id == &f["exchangeId"]), "FILL_SCOPE")?;
        let exid = f["exchangeId"].as_str().unwrap_or("").to_string();
        if f["action"].as_str() == Some("buy") { buys += &v; *quantities.entry(exid).or_insert(0) += 1; }
        else {
            need(f["action"].as_str() == Some("sell"), "FILL_ACTION")?;
            sells += &v; *quantities.entry(exid.clone()).or_insert(0) -= 1;
            need(*quantities.get(&exid).unwrap() >= 0, "OVERSELL")?;
        }
    }
    need(amount(&s["cash"])? == amount(&b["policy"]["initialCash"])? - &buys + &sells
        && amount(&s["cash"])? >= reserve(s)?, "CASH_CONSERVATION")?;
    need(amount(&s["realizedPnl"])? == &sells - &buys + cost(s)?
        && amount(&s["cumulativeBuys"])? == buys, "FIFO_CONSERVATION")?;
    need(cost(s)? + reserve(s)? <= amount(&b["policy"]["maxOwnedAndReserved"])?
        && &buys + reserve(s)? <= amount(&b["policy"]["cumulativeBuyBudget"])?, "CAPITAL")?;
    for id in &leg_ids {
        let key = id.as_str().unwrap_or("");
        need(js_int(&s["positions"][key]["quantity"]).unwrap_or(0) == *quantities.get(key).unwrap_or(&0), "INVENTORY_CONSERVATION")?;
    }
    need(s["positions"].as_object().map(|m| m.values().all(|p| js_int(&p["quantity"]) == Some(1))).unwrap_or(false)
        && (s["pending"].is_null() || js_int(&s["pending"]["quantity"]) == Some(1)), "QUANTITY")?;
    if s["pending"]["action"].as_str() == Some("sell") {
        need(js_truthy(Some(&s["positions"][s["pending"]["exchangeId"].as_str().unwrap_or("")])), "COVERED_SELL")?;
    }
    if s["pending"]["action"].as_str() == Some("buy") {
        need(!js_truthy(Some(&s["positions"][s["pending"]["exchangeId"].as_str().unwrap_or("")])), "DUPLICATE_BUY")?;
    }
    need(js_truthy(Some(&s["package"])) == (cost(s)? > BigInt::from(0) || js_truthy(Some(&s["pending"]))), "PACKAGE_OWNERSHIP")?;
    Ok(())
}
fn fresh(book: &V, account: &V, at: &str, b: &V) -> Result<bool, String> {
    if !js_truthy(Some(book)) { return Ok(false); }
    let at_ms = instant_str(at)?;
    Ok(instant(&book["receivedAt"])? <= at_ms
        && at_ms - instant(&book["requestStartedAt"])? <= b["policy"]["maxAgeMs"].as_i64().unwrap_or(0)
        && crate::probability_receipt::probability_account_gate(account, book, at).is_none())
}
fn pair_books(s: &V, account: &V, at: &str, b: &V) -> Result<Option<Vec<V>>, String> {
    let books: Vec<V> = ids(b).iter().map(|id| s["books"][id.as_str().unwrap_or("")].clone()).collect();
    for x in &books { if !fresh(x, account, at, b)? { return Ok(None); } }
    let receipts: Vec<i64> = books.iter().map(|x| instant(&x["receivedAt"])).collect::<Result<_,_>>()?;
    let starts: Vec<i64> = books.iter().map(|x| instant(&x["requestStartedAt"])).collect::<Result<_,_>>()?;
    Ok(if receipts.iter().max().unwrap() - starts.iter().min().unwrap() <= b["policy"]["maxSkewMs"].as_i64().unwrap_or(0) { Some(books) } else { None })
}
struct Level { unit: BigInt, quantity: i64, index: usize, raw_yes_price: V }
fn executable_level(book: &V, action: &str, b: &V) -> Result<Option<Level>, String> {
    let levels = book[if action == "buy" { "bids" } else { "asks" }].as_array().cloned().unwrap_or_default();
    for (index, level) in levels.iter().enumerate() {
        let quantity = big_int_of(&level["quantity"])? * BigInt::from(b["policy"]["depthFractionBps"].as_i64().unwrap_or(0)) / BigInt::from(10000);
        let unit = if action == "buy" { BigInt::from(SCALE) - amount(&level["price"])? + BigInt::from(TICK) }
            else { BigInt::from(SCALE) - amount(&level["price"])? - BigInt::from(TICK) };
        if quantity >= BigInt::from(1) && unit >= BigInt::from(TICK) && unit <= BigInt::from(SCALE - TICK) {
            return Ok(Some(Level { unit, quantity: i64::try_from(quantity).unwrap_or(i64::MAX), index, raw_yes_price: level["price"].clone() }));
        }
    }
    Ok(None)
}
fn buy_unit(book: &V, b: &V) -> Result<BigInt, String> { Ok(executable_level(book, "buy", b)?.map(|l| l.unit).unwrap_or_else(|| BigInt::from(SCALE))) }
fn sell_unit(book: &V, b: &V) -> Result<BigInt, String> { Ok(executable_level(book, "sell", b)?.map(|l| l.unit).unwrap_or_else(|| BigInt::from(0))) }
fn depth(book: &V, action: &str, b: &V) -> Result<i64, String> { Ok(executable_level(book, action, b)?.map(|l| l.quantity).unwrap_or(0)) }
fn pending(s: &mut V, b: &V, id: &str, action: &str, stage: &str, limit: BigInt, at: &str, at_ms: i64) -> Result<(), String> {
    let book = s["books"][id].clone();
    let leg = b["review"]["legs"].as_array().map(|l| l.iter().find(|x| x["exchangeId"].as_str() == Some(id)).cloned().unwrap_or(V::Null)).unwrap_or(V::Null);
    need(limit >= BigInt::from(TICK) && limit <= BigInt::from(SCALE - TICK) && &limit % BigInt::from(TICK) == BigInt::from(0), "LIMIT")?;
    let intent_id = s["nextIntentId"].as_i64().unwrap_or(0);
    s["nextIntentId"] = json!(intent_id + 1);
    s["pending"] = json!({"id":intent_id,"packageId":s["package"]["id"].clone(),"marketId":leg["marketId"].clone(),
        "exchangeId":id,"action":action,"stage":stage,"quantity":1,"limit":money(&limit),
        "createdAt":at,"expiresAt":ms_to_iso((at_ms + b["policy"]["intentLifetimeMs"].as_i64().unwrap_or(0)) as f64),
        "decisionVersion":book["versionKey"].clone()});
    inc(s, &format!("intent_{}", stage));
    Ok(())
}
fn finish(s: &mut V, b: &V, at: &str, at_ms: i64, reason: &V) -> Result<(), String> {
    need(s["positions"].as_object().is_some_and(|m| m.is_empty()), "FINISH_OWNED")?;
    let p = s["package"].clone();
    let rt = json!({"id":p["id"].clone(),"openedAt":p["createdAt"].clone(),"closedAt":at,"reason":reason.clone(),
        "buyCost":p["buyCost"].clone(),"saleProceeds":p["saleProceeds"].clone(),
        "realizedPnl":money(&(amount(&p["saleProceeds"])? - amount(&p["buyCost"])?)),"completedPair":p["completedPair"].clone()});
    s["roundtrips"].as_array_mut().map(|a| a.push(rt));
    s["package"] = V::Null; s["pending"] = V::Null; s["anchor"] = V::Null;
    s["cooldownUntil"] = json!(at_ms + b["policy"]["cooldownMs"].as_i64().unwrap_or(0));
    inc(s, "closed_packages");
    Ok(())
}
fn orphan(s: &mut V, b: &V, at: &str, at_ms: i64, reason: &V) -> Result<(), String> {
    s["pending"] = V::Null; s["anchor"] = V::Null;
    if s["positions"].as_object().map(|m| m.is_empty()).unwrap_or(true) { return finish(s, b, at, at_ms, reason); }
    s["package"]["stage"] = json!("orphan");
    s["package"]["orphanReason"] = reason.clone();
    inc(s, "orphan_obligation");
    Ok(())
}
fn expired(s: &mut V, b: &V, at: &str, at_ms: i64, reason: &V) -> Result<(), String> {
    let i = s["pending"].clone();
    need(js_truthy(Some(&i)), "EXPIRY")?;
    s["pending"] = V::Null; inc(s, "intent_expired");
    let stage = i["stage"].as_str().unwrap_or("");
    if stage == "entry_first" { finish(s, b, at, at_ms, reason) }
    else if stage == "entry_second" || stage == "exit_second" { orphan(s, b, at, at_ms, reason) }
    else if stage == "exit_first" { s["package"]["stage"] = json!("holding"); s["package"]["exitLimits"] = V::Null; Ok(()) }
    else { need(stage == "orphan", "EXPIRY_STAGE") }
}
fn fill(s: &mut V, book: &V, at: &str, at_ms: i64, b: &V) -> Result<(), String> {
    let i = s["pending"].clone();
    if !js_truthy(Some(&i)) || i["exchangeId"] != book["exchangeId"] || book["versionKey"] == i["decisionVersion"]
        || instant(&book["requestStartedAt"])? < instant(&i["createdAt"])? + b["policy"]["latencyMs"].as_i64().unwrap_or(0)
        || instant(&book["receivedAt"])? > instant(&i["expiresAt"])?
        || at_ms > instant(&i["expiresAt"])?
        || depth(book, i["action"].as_str().unwrap_or(""), b)? < 1 { return Ok(()); }
    let is_buy = i["action"].as_str() == Some("buy");
    let unit = if is_buy { buy_unit(book, b)? } else { sell_unit(book, b)? };
    if unit < BigInt::from(TICK) || unit > BigInt::from(SCALE - TICK) { return Ok(()); }
    let lim = amount(&i["limit"])?;
    if (is_buy && unit > lim) || (!is_buy && unit < lim) { return Ok(()); }
    let exid = i["exchangeId"].as_str().unwrap_or("").to_string();
    let level = executable_level(book, i["action"].as_str().unwrap_or(""), b)?.unwrap();
    if is_buy {
        need(!js_truthy(Some(&s["positions"][&exid])), "DUPLICATE_POSITION")?;
        s["positions"][&exid] = json!({"marketId":book["marketId"].clone(),"exchangeId":book["exchangeId"].clone(),
            "quantity":1,"cost":money(&unit),"enteredAt":at});
        s["cash"] = json!(money(&(amount(&s["cash"])? - &unit)));
        s["cumulativeBuys"] = json!(money(&(amount(&s["cumulativeBuys"])? + &unit)));
        s["package"]["buyCost"] = json!(money(&(amount(&s["package"]["buyCost"])? + &unit)));
    } else {
        let position = s["positions"][&exid].clone();
        need(js_truthy(Some(&position)), "COVERED_EXIT")?;
        s["cash"] = json!(money(&(amount(&s["cash"])? + &unit)));
        s["realizedPnl"] = json!(money(&(amount(&s["realizedPnl"])? + &unit - amount(&position["cost"])?)));
        s["package"]["saleProceeds"] = json!(money(&(amount(&s["package"]["saleProceeds"])? + &unit)));
        s["positions"].as_object_mut().map(|m| m.remove(&exid));
    }
    let mut frec = i.clone();
    frec["price"] = json!(money(&unit)); frec["at"] = json!(at); frec["fees"] = json!("0.000000");
    frec["model"] = json!("counterfactual_later_independent_book_not_proven_execution");
    frec["source"] = json!({"requestStartedAt":book["requestStartedAt"].clone(),"receivedAt":book["receivedAt"].clone(),
        "version":book["versionKey"].clone(),"publicationSequence":book["publicationSequence"].clone(),
        "publicationHash":book["publicationHash"].clone(),"eligibleDepth":level.quantity,"levelIndex":level.index as i64,
        "rawYesPrice":level.raw_yes_price.clone()});
    s["fills"].as_array_mut().map(|a| a.push(frec));
    inc(s, &format!("hypothetical_{}", i["action"].as_str().unwrap_or("undefined")));
    s["pending"] = V::Null; s["anchor"] = V::Null;
    let stage = i["stage"].as_str().unwrap_or("").to_string();
    if stage == "entry_first" {
        s["package"]["stage"] = json!("entry_second");
        let second = s["package"]["second"].as_str().unwrap_or("").to_string();
        let limit = amount(&s["package"]["entryLimits"][&second])?;
        pending(s, b, &second, "buy", "entry_second", limit, at, at_ms)?;
    } else if stage == "entry_second" {
        need(cost(s)? <= BigInt::from(SCALE) - amount(&b["policy"]["minimumPackageMargin"])?, "PACKAGE_MARGIN")?;
        s["package"]["stage"] = json!("holding"); s["package"]["completedPair"] = json!(true);
        s["package"]["completedAt"] = json!(at); inc(s, "completed_pairs");
    } else if stage == "exit_first" {
        s["package"]["stage"] = json!("exit_second");
        let second_exit = s["package"]["secondExit"].as_str().unwrap_or("").to_string();
        let limit = amount(&s["package"]["exitLimits"][&second_exit])?;
        pending(s, b, &second_exit, "sell", "exit_second", limit, at, at_ms)?;
    } else if stage == "exit_second" || stage == "orphan" {
        let reason = if stage == "orphan" { s["package"]["orphanReason"].clone() } else { s["package"]["exitReason"].clone() };
        finish(s, b, at, at_ms, &reason)?;
    } else { need(false, "FILL_STAGE")?; }
    Ok(())
}
/// advanceDualNoPair(input, event, bank)
pub fn advance_dual_no_pair(input: &V, event: &V, bank: &V) -> Result<V, String> {
    check_bank(bank)?; assert_dual_no_pair(input, bank)?;
    let mut s = input.clone();
    let b = bank;
    let at = event["availableAt"].as_str().unwrap_or("").to_string();
    let at_ms = instant_str(&at)?;
    need(at_ms >= instant(&s["lastAt"])? && at_ms >= instant(&b["cutoff"])?, "CLOCK")?;
    s["lastAt"] = json!(at);
    // A confirmation marker is not executable depth. Bound its lifetime while a
    // serialized refresh temporarily removes the old counterpart.
    if js_truthy(Some(&s["anchor"])) && at_ms - instant(&s["anchor"]["at"])? > b["policy"]["intentLifetimeMs"].as_i64().unwrap_or(0) {
        s["anchor"] = V::Null; inc(&mut s, "confirmation_anchor_expired");
    }
    if event["kind"].as_str() == Some("boundary") {
        s["books"] = json!({}); s["anchor"] = V::Null;
        if !s["pending"].is_null() { expired(&mut s, b, &at, at_ms, &json!("source_boundary"))?; }
        inc(&mut s, "source_boundary"); assert_dual_no_pair(&s, b)?; return Ok(s);
    }
    let legs = b["review"]["legs"].as_array().cloned().unwrap_or_default();
    need(event["kind"].as_str() == Some("book")
        && legs.iter().any(|l| l["exchangeId"] == event["book"]["exchangeId"] && l["marketId"] == event["book"]["marketId"]), "BOOK_SCOPE")?;
    let book = event["book"].clone();
    let account = event["accountEligibility"].clone();
    let keys: Vec<String> = s["books"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    for id in keys {
        if !fresh(&s["books"][&id], &account, &at, b)? { s["books"].as_object_mut().map(|m| m.remove(&id)); }
    }
    for mark in event["pairBooks"].as_array().cloned().unwrap_or_default() {
        if legs.iter().any(|l| l["exchangeId"] == mark["exchangeId"] && l["marketId"] == mark["marketId"]) && fresh(&mark, &account, &at, b)? {
            if let Some(k) = mark["exchangeId"].as_str() { s["books"][k] = mark.clone(); }
        }
    }
    let book_fresh = fresh(&book, &account, &at, b)?;
    if book_fresh {
        if let Some(k) = book["exchangeId"].as_str() { s["books"][k] = book.clone(); }
    } else if let Some(k) = book["exchangeId"].as_str() { s["books"].as_object_mut().map(|m| m.remove(k)); }
    let leg_ids = ids(b);
    let excluded_leg = account["excludedExchangeIds"].as_array()
        .is_some_and(|a| leg_ids.iter().any(|id| a.iter().any(|x| x == id)));
    if !s["pending"].is_null() && (at_ms > instant(&s["pending"]["expiresAt"])?
        || (s["pending"]["action"].as_str() == Some("buy") && (!current_review(b, &at)? || excluded_leg))) {
        expired(&mut s, b, &at, at_ms, &json!(if excluded_leg { "account_exposure" } else { "intent_or_review_expiry" }))?;
    }
    if book_fresh { fill(&mut s, &book, &at, at_ms, b)?; } else { inc(&mut s, "unavailable_or_excluded_book"); }
    let books = pair_books(&s, &account, &at, b)?;
    if s["package"]["stage"].as_str() == Some("orphan") && s["pending"].is_null() {
        if let Some(id) = s["positions"].as_object().and_then(|m| m.keys().next()).cloned() {
            let mark = s["books"][&id].clone();
            if book["exchangeId"].as_str() == Some(id.as_str()) && fresh(&mark, &account, &at, b)? && sell_unit(&mark, b)? >= BigInt::from(TICK) {
                pending(&mut s, b, &id, "sell", "orphan", sell_unit(&mark, b)?, &at, at_ms)?;
            }
        }
    } else if s["package"]["stage"].as_str() == Some("holding") && s["pending"].is_null() && books.is_some() {
        let books = books.unwrap();
        let units: Vec<BigInt> = books.iter().map(|x| sell_unit(x, b)).collect::<Result<_,_>>()?;
        let proceeds: BigInt = units.iter().fold(BigInt::from(0), |a, v| a + v);
        let basis = cost(&s)?;
        let take_profit = amount(&b["policy"]["takeProfit"])?;
        let stop = at_ms - instant(&s["package"]["completedAt"])? >= b["policy"]["maximumPairedHoldMs"].as_i64().unwrap_or(0)
            || &basis - &proceeds >= amount(&b["policy"]["packageStopLoss"])?;
        let mut depth_ok = true;
        for x in &books { if depth(x, "sell", b)? < 1 { depth_ok = false; break; } }
        if units.iter().all(|x| *x >= BigInt::from(TICK)) && depth_ok && (proceeds >= &basis + &take_profit || stop) {
            s["package"]["exitReason"] = json!(if proceeds >= &basis + &take_profit { "paired_profit_target" }
                else if at_ms - instant(&s["package"]["completedAt"])? >= b["policy"]["maximumPairedHoldMs"].as_i64().unwrap_or(0) { "paired_holding_limit" }
                else { "paired_loss_limit" });
            let mut exit_limits = Map::new();
            for (i, x) in books.iter().enumerate() { exit_limits.insert(x["exchangeId"].as_str().unwrap_or("").to_string(), json!(money(&units[i]))); }
            s["package"]["exitLimits"] = V::Object(exit_limits);
            let mut ordered: Vec<(BigInt, V)> = s["positions"].as_object().map(|m|
                m.values().map(|p| Ok((amount(&p["cost"])?, p.clone()))).collect::<Result<Vec<_>, String>>()).unwrap_or(Ok(Vec::new()))?;
            ordered.sort_by(|x, z| z.0.cmp(&x.0));
            let first = ordered[0].1["exchangeId"].as_str().unwrap_or("").to_string();
            s["package"]["secondExit"] = json!(leg_ids.iter().find(|id| id.as_str() != Some(first.as_str())).cloned().unwrap_or(V::Null));
            s["package"]["stage"] = json!("exit_first");
            let limit = amount(&s["package"]["exitLimits"][&first])?;
            pending(&mut s, b, &first, "sell", "exit_first", limit, &at, at_ms)?;
            inc(&mut s, "paired_exit_obligation");
        }
    } else if s["package"].is_null() && s["pending"].is_null()
        && js_int(&s["cooldownUntil"]).is_some_and(|c| at_ms >= c)
        && books.is_some() && js_truthy(Some(&event["admission"]["positiveConditionalCapacity"])) && current_review(b, &at)? {
        let books = books.clone().unwrap();
        let mut post_cutoff = true;
        for x in &books { if instant(&x["requestStartedAt"])? <= instant(&b["cutoff"])? { post_cutoff = false; break; } }
        if post_cutoff {
            let units: Vec<BigInt> = books.iter().map(|x| buy_unit(x, b)).collect::<Result<_,_>>()?;
            let sum: BigInt = units.iter().fold(BigInt::from(0), |a, v| a + v);
            let versions: Vec<V> = books.iter().map(|x| x["versionKey"].clone()).collect();
            let mut depth_ok = true;
            for x in &books { if depth(x, "buy", b)? < 1 { depth_ok = false; break; } }
            if units.iter().all(|v| *v >= BigInt::from(TICK) && *v <= BigInt::from(SCALE - TICK)) && depth_ok
                && sum <= BigInt::from(SCALE) - amount(&b["policy"]["minimumPackageMargin"])?
                && sum <= amount(&b["policy"]["maxOwnedAndReserved"])?
                && amount(&s["cumulativeBuys"])? + &sum <= amount(&b["policy"]["cumulativeBuyBudget"])?
                && amount(&s["cash"])? >= sum {
                let mut confirmed = false;
                if js_truthy(Some(&s["anchor"])) {
                    let versions_differ = versions.iter().enumerate().all(|(i, v)| *v != s["anchor"]["versions"][i]);
                    if versions_differ {
                        let anchor_at = instant(&s["anchor"]["at"])?;
                        confirmed = true;
                        for x in &books { if instant(&x["requestStartedAt"])? <= anchor_at { confirmed = false; break; } }
                    }
                }
                if confirmed {
                    // Cheaper first bounds the principal lost if the price-capped hedge fails.
                    let first = books[if units[0] <= units[1] { 0 } else { 1 }]["exchangeId"].as_str().unwrap_or("").to_string();
                    let second = leg_ids.iter().find(|id| id.as_str() != Some(first.as_str())).cloned().unwrap_or(V::Null);
                    let pkg_id = s["nextPackageId"].as_i64().unwrap_or(0);
                    s["nextPackageId"] = json!(pkg_id + 1);
                    let mut entry_limits = Map::new();
                    for (i, x) in books.iter().enumerate() { entry_limits.insert(x["exchangeId"].as_str().unwrap_or("").to_string(), json!(money(&units[i]))); }
                    s["package"] = json!({"id":pkg_id,"first":first,"second":second,"stage":"entry_first","createdAt":at,
                        "completedPair":false,"buyCost":"0.000000","saleProceeds":"0.000000",
                        "entryLimits":V::Object(entry_limits),"entryVersions":versions});
                    let limit = amount(&s["package"]["entryLimits"][&first])?;
                    pending(&mut s, b, &first, "buy", "entry_first", limit, &at, at_ms)?;
                    s["anchor"] = V::Null; inc(&mut s, "paired_entry_obligation");
                } else if s["anchor"].is_null() { s["anchor"] = json!({"versions":versions.clone(),"at":at}); }
            } else { s["anchor"] = V::Null; inc(&mut s, "no_package_capacity"); }
        }
    } else if s["package"].is_null() {
        // Keep the bounded marker across an intermediate, valid incoming refresh.
        // An executable intent still requires a complete cofresh, positively admitted
        // pair and two later independent requests above. Explicit uncertainty,
        // exclusion, expired rules, or a complete ineligible pair clears it.
        if !book_fresh || excluded_leg || !current_review(b, &at)? || books.is_some() { s["anchor"] = V::Null; }
    }
    assert_dual_no_pair(&s, b)?;
    Ok(s)
}
/// dualNoPairReport(s, b, at)
pub fn dual_no_pair_report(s: &V, b: &V, at: &str) -> Result<V, String> {
    assert_dual_no_pair(s, b)?;
    let at_ms = instant_str(at)?;
    let max_age = b["policy"]["maxAgeMs"].as_i64().unwrap_or(0);
    let mut liquidation = BigInt::from(0);
    let mut complete = true;
    let mut positions = Vec::new();
    if let Some(m) = s["positions"].as_object() {
        for p in m.values() {
            let book = s["books"][p["exchangeId"].as_str().unwrap_or("")].clone();
            let mut fresh_mark = false;
            if js_truthy(Some(&book)) {
                fresh_mark = at_ms >= instant(&book["receivedAt"])?
                    && at_ms - instant(&book["requestStartedAt"])? <= max_age
                    && depth(&book, "sell", b)? >= 1 && sell_unit(&book, b)? >= BigInt::from(TICK);
            }
            let value = if fresh_mark { json!(money(&sell_unit(&book, b)?)) } else { V::Null };
            if value.is_null() { complete = false; } else { liquidation += amount(&value)?; }
            let mut po = p.clone();
            po["estimatedLiquidationProceeds"] = value.clone();
            po["unsoldShares"] = json!(if value.is_null() { 1 } else { 0 });
            po["quoteReceivedAt"] = if book.is_object() { book["receivedAt"].clone() } else { V::Null };
            po["quoteRequestStartedAt"] = if book.is_object() { book["requestStartedAt"].clone() } else { V::Null };
            po["quoteAgeMs"] = if js_truthy(Some(&book)) { json!(at_ms - instant(&book["requestStartedAt"])?) } else { V::Null };
            positions.push(po);
        }
    }
    let paired = positions.len() == 2 && js_truthy(Some(&s["package"]["completedPair"]));
    let review_current = current_review(b, at)?;
    let cost_total = cost(s)?;
    let fills = s["fills"].as_array().cloned().unwrap_or_default();
    let roundtrips = s["roundtrips"].as_array().cloned().unwrap_or_default();
    let mut closed = 0i64;
    for x in &roundtrips { if amount(&x["buyCost"])? > BigInt::from(0) { closed += 1; } }
    Ok(json!({"schemaVersion":1,"mode":b["mode"].clone(),"cohort":b["name"].clone(),"bankHash":s["bankHash"].clone(),
        "cutoff":b["cutoff"].clone(),"updatedAt":at,"reviewExpiresAt":b["review"]["expiresAt"].clone(),
        "cash":s["cash"].clone(),"realizedPnl":s["realizedPnl"].clone(),
        "estimatedLiquidationPnl":if complete { json!(money(&(amount(&s["cash"])? + &liquidation - amount(&b["policy"]["initialCash"])?))) } else { V::Null },
        "positions":positions,"pending":s["pending"].clone(),"package":s["package"].clone(),"reserved":money(&reserve(s)?),
        "cumulativeBuys":s["cumulativeBuys"].clone(),"counts":s["counts"].clone(),
        "fills":fills.len() as i64,"recentFills":fills[fills.len().saturating_sub(10)..].to_vec(),
        "closedPackages":closed,"recentRoundtrips":roundtrips[roundtrips.len().saturating_sub(10)..].to_vec(),"fees":"0.000000",
        "ordinaryConditionalSettlementSurplus":if paired { json!(money(&(BigInt::from(SCALE) - &cost_total))) } else { V::Null },
        "zeroPayoutExceptionLoss":money(&(-&cost_total)),"reviewCurrent":review_current,
        "executionAuthority":false,"competitionOrders":0,"networkRequests":0,"profitabilityEstablished":false,
        "limitations":["Two sequential hypothetical fills are not atomic execution or evidence of queue priority.",
            "Ordinary settlement surplus is conditional on reviewed rules and is never booked as realized or liquidation P&L.",
            "A failed hedge or second exit creates an orphan obligation that may realize a loss.",
            "Matched inventory may be held through the reviewed election horizon; no settlement payout is booked without explicit settlement evidence. Entry expiry does not settle inventory. All costs and losses remain attributed."]}))
}
