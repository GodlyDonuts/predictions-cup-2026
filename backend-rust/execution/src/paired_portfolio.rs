// execution/paired-portfolio.mjs — paired NO/NO execution ledger state. Pure
// transitions: reservations and post-fill application; never a dispatch permit.
use crate::canonical_fifo::project_canonical_fifo;
use crate::core::{canonical_payload, decimal_micros, format_micros, payload_hash};
use baal_service_runtime::json_text::{js_int, js_truthy};
use serde_json::{json, Map, Value as V};

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(c.into()) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn money(x: &V) -> Result<i128, String> {
    need(x.is_string(), "PAIRED_EXACT_MONEY")?;
    decimal_micros(x, "exact")
}
fn signed(x: &V) -> Result<i128, String> {
    if x.as_str().is_some_and(|s| s.starts_with('-')) {
        Ok(-money(&json!(x.as_str().unwrap()[1..]))?)
    } else {
        money(x)
    }
}
fn hash(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn at(x: &V) -> Result<i64, String> {
    need(
        x.as_str().is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok()),
        "PAIRED_TIME",
    )?;
    Ok(chrono::DateTime::parse_from_rfc3339(x.as_str().unwrap()).unwrap().timestamp_millis())
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0 && n <= 9_007_199_254_740_991)
    })
}

// PAIRED_EXECUTION_LIMITS (frozen object).
pub fn paired_execution_limits() -> V {
    json!({
        "capital": "25.000000",
        "maxExposure": "25.000000",
        "maxOrderCost": "5.000000",
        "maxOrphanPrincipal": "5.000000",
        "maxCumulativeBuy": "100.000000",
        "maxShares": 6,
        "maxAccountAgeMs": 180000,
        "maxLifetimeMs": 30000,
    })
}

fn basis(lots: &V) -> Result<i128, String> {
    let mut n: i128 = 0;
    for l in lots.as_array().cloned().unwrap_or_default() {
        n += js_int(get(&l, "quantity")).unwrap_or(0) as i128 * money(get(&l, "entryPrice"))?;
    }
    Ok(n)
}

#[derive(Clone)]
struct Account {
    start: i64,
    end: i64,
    cash: i128,
    positions: Map<String, V>,
}

fn account(snapshot: &V, scope: &V, now: i64) -> Result<Account, String> {
    need(
        get(snapshot, "tournamentId") == get(scope, "tournamentId")
            && get(snapshot, "complete") == &json!(true)
            && hash(get(snapshot, "positionsSourceHash"))
            && get(snapshot, "positions").is_array()
            && get(snapshot, "openOrders").as_array().is_some_and(|o| o.is_empty()),
        "PAIRED_ACCOUNT_INCOMPLETE",
    )?;
    let start = at(get(snapshot, "startedAt"))?;
    let end = at(get(snapshot, "completedAt"))?;
    need(
        start <= end && end <= now && now - start <= 180_000,
        "PAIRED_ACCOUNT_STALE",
    )?;
    let cash = get(snapshot, "cashReceipt");
    need(
        get(cash, "kind").as_str() == Some("tournament_cash")
            && get(cash, "tournamentId") == get(scope, "tournamentId")
            && get(cash, "joined") == &json!(true)
            && get(cash, "method").as_str() == Some("GET")
            && get(cash, "endpoint").as_str()
                == Some(format!("/tournaments/{}", get(scope, "tournamentSlug").as_str().unwrap_or("")).as_str())
            && get(get(cash, "fields"), "myBalance").get("available") == Some(&json!(true))
            && hash(get(cash, "sourceBodyHash"))
            && at(get(cash, "receivedAt"))? >= start
            && at(get(cash, "receivedAt"))? <= end,
        "PAIRED_CASH_RECEIPT",
    )?;
    let amount = money(get(get(get(cash, "fields"), "myBalance"), "decimal"))?;
    let mut positions = Map::new();
    for p in get(snapshot, "positions").as_array().unwrap() {
        need(
            id(get(p, "exchangeId")) && id(get(p, "marketId"))
                && js_int(get(p, "quantity")).is_some()
                && !positions.contains_key(get(p, "exchangeId").as_str().unwrap_or("")),
            "PAIRED_POSITION_INVALID",
        )?;
        if js_truthy(Some(get(p, "quantity"))) {
            positions.insert(
                get(p, "exchangeId").as_str().unwrap().to_string(),
                json!({"marketId": get(p, "marketId"), "quantity": get(p, "quantity")}),
            );
        }
    }
    Ok(Account { start, end, cash: amount, positions })
}

fn expected_positions(s: &V, replace: Option<&V>) -> Result<V, String> {
    let mut p = get(s, "externalPositions").as_object().cloned().unwrap_or_default();
    for m in get(get(s, "scope"), "markets").as_array().cloned().unwrap_or_default() {
        let exchange_id = get(&m, "exchangeId").as_str().unwrap_or("");
        let owned = if replace.and_then(|r| r.get("exchangeId").and_then(V::as_str)) == Some(exchange_id) {
            replace.unwrap().clone()
        } else {
            get(get(s, "owned"), exchange_id).clone()
        };
        let quantity: i64 = get(&owned, "lots").as_array().cloned().unwrap_or_default().iter()
            .map(|l| (if get(l, "side").as_str() == Some("no") { -1 } else { 1 }) * js_int(get(l, "quantity")).unwrap_or(0))
            .sum();
        if quantity != 0 {
            p.insert(exchange_id.to_string(), json!({"marketId": get(&m, "marketId"), "quantity": quantity}));
        } else {
            p.shift_remove(exchange_id);
        }
    }
    Ok(V::Object(p))
}

fn check(s: &V) -> Result<i128, String> {
    need(
        get(s, "schemaVersion") == &json!(1)
            && get(s, "scopeHash").as_str() == Some(payload_hash(get(s, "scope"))?.as_str())
            && get(s, "policyHash").as_str() == Some(payload_hash(&paired_execution_limits())?.as_str()),
        "PAIRED_STATE_IDENTITY",
    )?;
    let mut total: i128 = 0;
    for (_k, w) in get(s, "owned").as_object().cloned().unwrap_or_default() {
        total += basis(get(&w, "lots"))?;
    }
    need(
        money(get(s, "cash"))? + total == money(get(&paired_execution_limits(), "capital"))? + signed(get(s, "realizedPnl"))?,
        "PAIRED_CASH_BASIS_IDENTITY",
    )?;
    need(
        total <= money(get(&paired_execution_limits(), "maxExposure"))?
            && money(get(s, "cumulativeBuy"))? <= money(get(&paired_execution_limits(), "maxCumulativeBuy"))?,
        "PAIRED_LEDGER_LIMIT",
    )?;
    Ok(total)
}

fn slug(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|seg| !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

// createPairedPortfolio(scope, snapshot, readAt)
pub fn create_paired_portfolio(scope: &V, snapshot: &V, read_at: &V) -> Result<V, String> {
    let markets = get(scope, "markets");
    need(
        get(scope, "ownerId").as_str() == Some("Codex")
            && get(scope, "runId").as_str().is_some_and(|s| !s.is_empty())
            && get(scope, "tournamentId").is_string()
            && get(scope, "tournamentSlug").as_str().is_some_and(slug)
            && markets.as_array().is_some_and(|ms| {
                ms.len() == 2
                    && ms.iter().all(|m| id(get(m, "marketId")) && id(get(m, "exchangeId")))
                    && ms.iter().map(|m| get(m, "exchangeId").as_str().unwrap_or("")).collect::<std::collections::BTreeSet<_>>().len() == 2
                    && ms.iter().map(|m| get(m, "marketId").as_str().unwrap_or("")).collect::<std::collections::BTreeSet<_>>().len() == 2
            }),
        "PAIRED_SCOPE",
    )?;
    let a = account(snapshot, scope, at(read_at)?)?;
    need(a.cash >= money(get(&paired_execution_limits(), "capital"))?, "PAIRED_INITIAL_CASH")?;
    need(
        markets.as_array().unwrap().iter().all(|m| !a.positions.contains_key(get(m, "exchangeId").as_str().unwrap_or(""))),
        "PAIRED_EXISTING_HOLDING",
    )?;
    let mut owned = Map::new();
    for m in markets.as_array().unwrap() {
        owned.insert(
            get(m, "exchangeId").as_str().unwrap().to_string(),
            json!({
                "exchangeId": get(m, "exchangeId"),
                "marketId": get(m, "marketId"),
                "lots": [],
                "appliedOrderIds": [],
            }),
        );
    }
    Ok(json!({
        "schemaVersion": 1,
        "scope": scope.clone(),
        "scopeHash": payload_hash(scope)?,
        "policyHash": payload_hash(&paired_execution_limits())?,
        "cash": "25.000000",
        "realizedPnl": "0.000000",
        "cumulativeBuy": "0.000000",
        "externalPositions": V::Object(a.positions),
        "owned": V::Object(owned),
        "active": V::Null,
        "balanceMode": V::Null,
        "completed": [],
        "initialAccountEvidenceHash": payload_hash(snapshot)?,
        "executionAuthority": false,
    }))
}

// reservePairedOrder(s, spec, snapshot, readAt)
pub fn reserve_paired_order(s: &V, spec: &V, snapshot: &V, read_at: &V) -> Result<V, String> {
    let intent_id = get(spec, "intentId");
    let idempotency_key = get(spec, "idempotencyKey");
    let market_id = get(spec, "marketId");
    let exchange_id = get(spec, "exchangeId");
    let action = get(spec, "action").as_str().unwrap_or("");
    let quantity = js_int(get(spec, "quantity")).unwrap_or(i64::MIN);
    let limit = get(spec, "limit");
    let created_at = get(spec, "createdAt");
    let expiration_date = get(spec, "expirationDate");
    let total = check(s)?;
    need(s.get("active") == Some(&V::Null), "PAIRED_ACTIVE_INTENT")?;
    let now = at(read_at)?;
    let a = account(snapshot, get(s, "scope"), now)?;
    need(
        canonical_payload(&V::Object(a.positions.clone()))? == canonical_payload(&expected_positions(s, None)?)?,
        "PAIRED_ACCOUNT_POSITION_DRIFT",
    )?;
    let markets = get(get(s, "scope"), "markets").as_array().cloned().unwrap_or_default();
    let m = markets.iter().find(|m| get(m, "marketId") == market_id && get(m, "exchangeId") == exchange_id);
    need(
        m.is_some()
            && intent_id.as_str().is_some_and(|s| !s.is_empty())
            && idempotency_key.as_str().is_some_and(|s| !s.is_empty())
            && !get(s, "completed").as_array().cloned().unwrap_or_default().iter().any(|x| {
                get(x, "intentId") == intent_id || get(x, "idempotencyKey") == idempotency_key
            }),
        "PAIRED_ORDER_IDENTITY",
    )?;
    need(
        (action == "buy" || action == "sell")
            && js_int(get(spec, "quantity")).is_some_and(|q| q > 0 && q <= 6),
        "PAIRED_ORDER_TERMS",
    )?;
    let p = money(limit)?;
    need(p >= 5_000 && p <= 995_000 && p % 5_000 == 0, "PAIRED_LIMIT_TICK")?;
    need(
        at(created_at)? == now && at(expiration_date)? > now && at(expiration_date)? - now <= 30_000,
        "PAIRED_EXPIRATION",
    )?;
    let ex = exchange_id.as_str().unwrap_or("");
    let own = get(get(s, "owned"), ex);
    let own_lots = get(own, "lots").as_array().cloned().unwrap_or_default();
    let q: i64 = own_lots.iter().map(|l| js_int(get(l, "quantity")).unwrap_or(0)).sum();
    let reservation: i128 = if action == "buy" { p * quantity as i128 } else { 0 };
    let other = markets.iter().find(|x| get(x, "exchangeId") != exchange_id).cloned().unwrap_or(V::Null);
    let other_lots = get(get(get(s, "owned"), get(&other, "exchangeId").as_str().unwrap_or("")), "lots")
        .as_array().cloned().unwrap_or_default();
    let other_quantity: i64 = other_lots.iter().map(|l| js_int(get(l, "quantity")).unwrap_or(0)).sum();
    if q != other_quantity {
        need(
            if action == "buy" {
                get(s, "balanceMode").as_str() == Some("entry") && q < other_quantity && quantity <= other_quantity - q
            } else {
                q > other_quantity && quantity <= q - other_quantity
            },
            "PAIRED_BALANCE_LEG_QUANTITY",
        )?;
    }
    need(
        own_lots.iter().all(|l| get(l, "side").as_str() == Some("no"))
            && (action != "sell" || q >= quantity),
        "PAIRED_NAKED_OR_COMPLEMENT_SELL",
    )?;
    // Every partial outcome must fit the orphan cap. Use the most expensive
    // held shares for the unmatched side; never assume the second order fills.
    for filled in 1..=quantity {
        let mut projected: std::collections::VecDeque<V> = own_lots.clone().into();
        if action == "buy" {
            projected.push_back(json!({"side": "no", "quantity": filled, "entryPrice": limit}));
        } else {
            let mut left = filled;
            while left > 0 && !projected.is_empty() {
                let l = projected.front_mut().unwrap();
                let lq = js_int(get(l, "quantity")).unwrap_or(0);
                let n = left.min(lq);
                left -= n;
                if let Some(o) = l.as_object_mut() {
                    o.insert("quantity".into(), json!(lq - n));
                }
                if lq - n == 0 {
                    projected.pop_front();
                }
            }
        }
        let projected_quantity = q + if action == "buy" { filled } else { -filled };
        let dominant: Vec<V> = if projected_quantity > other_quantity {
            projected.into_iter().collect()
        } else {
            other_lots.clone()
        };
        let mut unmatched = (projected_quantity - other_quantity).abs();
        let mut orphan: i128 = 0;
        let mut keyed: Vec<(i128, V)> = Vec::new();
        for l in &dominant {
            keyed.push((money(get(l, "entryPrice"))?, l.clone()));
        }
        keyed.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, l) in &keyed {
            let n = unmatched.min(js_int(get(l, "quantity")).unwrap_or(0));
            orphan += n as i128 * money(get(l, "entryPrice"))?;
            unmatched -= n;
            if unmatched == 0 {
                break;
            }
        }
        need(orphan <= money(get(&paired_execution_limits(), "maxOrphanPrincipal"))?, "PAIRED_ORPHAN_LIMIT")?;
    }
    need(
        reservation <= money(get(&paired_execution_limits(), "maxOrderCost"))?
            && reservation <= money(get(s, "cash"))?
            && reservation <= a.cash
            && total + reservation <= money(get(&paired_execution_limits(), "maxExposure"))?
            && money(get(s, "cumulativeBuy"))? + reservation <= money(get(&paired_execution_limits(), "maxCumulativeBuy"))?,
        "PAIRED_RESERVATION_LIMIT",
    )?;
    let binding = json!({
        "ownerId": get(get(s, "scope"), "ownerId"),
        "tournamentId": get(get(s, "scope"), "tournamentId"),
        "marketId": market_id,
        "exchangeId": exchange_id,
        "side": "no",
        "action": action,
        "quantity": quantity,
        "limit": limit,
        "createdAt": created_at,
        "expirationDate": expiration_date,
    });
    let mut wallet = own.as_object().cloned().unwrap_or_default();
    wallet.insert("ownerId".into(), get(get(s, "scope"), "ownerId").clone());
    wallet.insert("tournamentId".into(), get(get(s, "scope"), "tournamentId").clone());
    wallet.insert("cash".into(), get(s, "cash").clone());
    wallet.insert("realizedPnl".into(), get(s, "realizedPnl").clone());
    wallet.insert("accountSignedQuantity".into(), json!(-q));
    let mut next = s.clone();
    next.as_object_mut().unwrap().insert(
        "active".into(),
        json!({
            "preparation": {"intentId": intent_id, "idempotencyKey": idempotency_key, "binding": binding, "wallet": V::Object(wallet)},
            "before": snapshot.clone(),
            "reservation": format_micros(reservation),
            "beforeStateHash": payload_hash(s)?,
        }),
    );
    Ok(next)
}

// applyReconciledPairedOrder(s, orderStatus, after, activity, readAt)
// Local proposed facts only: an API adapter must supply source-bound complete
// evidence; this pure function never converts its result into a dispatch permit.
pub fn apply_reconciled_paired_order(
    s: &V,
    order_status: &V,
    after: &V,
    activity: &V,
    read_at: &V,
) -> Result<V, String> {
    check(s)?;
    need(
        js_truthy(s.get("active"))
            && get(order_status, "failed") == &json!(false)
            && get(order_status, "unconfirmedPersistence") == &json!(false)
            && get(get(order_status, "state"), "phase").as_str() == Some("terminal"),
        "PAIRED_ORDER_NOT_TERMINAL",
    )?;
    let order = get(order_status, "state");
    let pre = get(get(s, "active"), "preparation");
    need(
        get(order, "intentId") == get(pre, "intentId")
            && get(order, "idempotencyKey") == get(pre, "idempotencyKey")
            && canonical_payload(get(order, "binding"))? == canonical_payload(get(pre, "binding"))?
            && payload_hash(get(order, "wallet"))? == payload_hash(get(pre, "wallet"))?,
        "PAIRED_ORDER_BASELINE_CHANGED",
    )?;
    need(
        id(get(order, "orderId"))
            && !get(s, "completed").as_array().cloned().unwrap_or_default().iter().any(|x| {
                get(x, "orderId") == get(order, "orderId")
            }),
        "PAIRED_ORDER_ALREADY_APPLIED",
    )?;
    let mut fifo_input = Map::new();
    fifo_input.insert("wallet".into(), get(pre, "wallet").clone());
    let mut binding = get(pre, "binding").as_object().cloned().unwrap_or_default();
    binding.insert("orderId".into(), get(order, "orderId").clone());
    fifo_input.insert("binding".into(), V::Object(binding));
    if let Some(ev) = get(order, "evidence").as_object() {
        for (k, v) in ev {
            fifo_input.insert(k.clone(), v.clone());
        }
    }
    let projection = project_canonical_fifo(&V::Object(fifo_input))?;
    need(
        get(&projection, "projectionHash").as_str() == get(get(order, "projection"), "projectionHash").as_str(),
        "PAIRED_ORDER_PROJECTION_CHANGED",
    )?;
    let now = at(read_at)?;
    let a = account(after, get(s, "scope"), now)?;
    let b = account(get(get(s, "active"), "before"), get(s, "scope"), at(get(get(pre, "binding"), "createdAt"))?)?;
    let mut last_fill = at(get(get(pre, "binding"), "createdAt"))?;
    for f in get(get(order, "evidence"), "rows").as_array().cloned().unwrap_or_default() {
        last_fill = last_fill.max(at(get(&f, "filledAt"))?);
    }
    need(
        a.start >= last_fill && a.start >= at(get(get(order, "evidence"), "readAt"))?,
        "PAIRED_ACCOUNT_BEFORE_TERMINAL_EVIDENCE",
    )?;
    need(
        a.cash - b.cash == signed(get(&projection, "cashDelta"))?,
        "PAIRED_OFFICIAL_CASH_MISMATCH",
    )?;
    need(
        canonical_payload(&V::Object(a.positions.clone()))?
            == canonical_payload(&expected_positions(s, Some(get(&projection, "wallet")))?)?,
        "PAIRED_OFFICIAL_POSITION_MISMATCH",
    )?;
    need(
        get(activity, "tournamentId") == get(get(s, "scope"), "tournamentId")
            && get(activity, "complete") == &json!(true)
            && hash(get(activity, "sourceHash"))
            && at(get(activity, "from"))? <= b.start
            && at(get(activity, "through"))? >= a.end
            && at(get(activity, "through"))? <= now
            && get(activity, "events").is_array(),
        "PAIRED_ACTIVITY_INCOMPLETE",
    )?;
    let rows = get(get(order, "evidence"), "rows").as_array().cloned().unwrap_or_default();
    let fills: std::collections::BTreeMap<i64, V> = rows
        .iter()
        .filter_map(|f| js_int(get(f, "id")).map(|i| (i, f.clone())))
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    for e in get(activity, "events").as_array().unwrap() {
        let fill_id = js_int(get(e, "fillId"));
        let f = fill_id.and_then(|i| fills.get(&i));
        need(
            get(e, "kind").as_str() == Some("fill")
                && get(e, "orderId") == get(order, "orderId")
                && f.is_some()
                && fill_id.is_some_and(|i| !seen.contains(&i))
                && get(e, "exchangeId") == get(get(pre, "binding"), "exchangeId")
                && get(e, "marketId") == get(get(pre, "binding"), "marketId")
                && at(get(e, "at"))? == at(get(f.unwrap_or(&V::Null), "filledAt"))?,
            "PAIRED_UNRELATED_ECONOMIC_ACTIVITY",
        )?;
        seen.insert(fill_id.unwrap());
    }
    need(seen.len() == fills.len(), "PAIRED_ACTIVITY_FILL_MISSING")?;
    let mut next = s.clone();
    let next_obj = next.as_object_mut().unwrap();
    next_obj.insert("cash".into(), get(get(&projection, "wallet"), "cash").clone());
    next_obj.insert("realizedPnl".into(), get(get(&projection, "wallet"), "realizedPnl").clone());
    let owned_ex = get(get(pre, "binding"), "exchangeId").as_str().unwrap_or("").to_string();
    next_obj
        .entry("owned".to_string())
        .or_insert_with(|| V::Object(Map::new()))
        .as_object_mut()
        .unwrap()
        .insert(
            owned_ex.clone(),
            json!({
                "exchangeId": get(get(pre, "binding"), "exchangeId"),
                "marketId": get(get(pre, "binding"), "marketId"),
                "lots": get(get(&projection, "wallet"), "lots"),
                "appliedOrderIds": get(get(&projection, "wallet"), "appliedOrderIds"),
            }),
        );
    if get(&projection, "canonicalAction").as_str() == Some("buy") {
        next_obj.insert(
            "cumulativeBuy".into(),
            json!(format_micros(money(get(s, "cumulativeBuy"))? + money(get(&projection, "nominalPrincipal"))?)),
        );
    }
    let completed_entry = json!({
        "intentId": get(pre, "intentId"),
        "idempotencyKey": get(pre, "idempotencyKey"),
        "orderId": get(order, "orderId"),
        "action": get(get(pre, "binding"), "action"),
        "projectionHash": get(&projection, "projectionHash"),
        "accountEvidenceHash": payload_hash(after)?,
        "activityHash": payload_hash(activity)?,
        "cashDelta": get(&projection, "cashDelta"),
        "realizedDelta": get(&projection, "realizedDelta"),
    });
    next_obj
        .entry("completed".to_string())
        .or_insert_with(|| V::Array(Vec::new()))
        .as_array_mut()
        .unwrap()
        .push(completed_entry);
    next_obj.insert("active".into(), V::Null);
    let _ = next_obj;
    let quantities: Vec<i64> = get(get(&next, "scope"), "markets").as_array().cloned().unwrap_or_default().iter()
        .map(|m| {
            get(get(&next, "owned"), get(m, "exchangeId").as_str().unwrap_or(""))
                .get("lots").and_then(V::as_array).cloned().unwrap_or_default().iter()
                .map(|l| js_int(get(l, "quantity")).unwrap_or(0))
                .sum()
        })
        .collect();
    let balance_mode = if quantities.get(0) == quantities.get(1) {
        V::Null
    } else if !get(s, "balanceMode").is_null() {
        get(s, "balanceMode").clone()
    } else {
        json!(if get(get(pre, "binding"), "action").as_str() == Some("buy") { "entry" } else { "exit" })
    };
    next.as_object_mut().unwrap().insert("balanceMode".into(), balance_mode);
    check(&next)?;
    Ok(next)
}
