// execution/no-order-economics.mjs — order-local evidence only. No dispatch,
// ownership adoption or global convention. Snapshots require exact gross cash,
// not available collateral or rounded marks.
use crate::core::{canonical_payload, decimal_micros, format_micros, payload_hash, SCALE};
use crate::gates::normalize_official_fill_tick;
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Map, Value as V};
use std::collections::{BTreeMap, BTreeSet};

fn need(x: bool, code: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(code.into()) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0 && n <= 9_007_199_254_740_991)
    })
}
fn time(x: &V) -> Result<i64, String> {
    need(
        x.as_str().is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok()),
        "INVALID_ECONOMICS_TIME",
    )?;
    Ok(chrono::DateTime::parse_from_rfc3339(x.as_str().unwrap()).unwrap().timestamp_millis())
}
fn tick(x: &V) -> Result<i128, String> {
    let t = normalize_official_fill_tick(x)?;
    decimal_micros(get(&t, "price"), "exact")
}
fn canon_or_null(v: Option<&V>) -> Result<String, String> {
    canonical_payload(&v.cloned().unwrap_or(V::Null))
}

struct Snapshot {
    start: i64,
    end: i64,
    cash: i128,
    positions: BTreeMap<String, V>,
}

// verifyNoBuyEconomics({binding,ack,fills,before,after,activity,readAt})
pub fn verify_no_buy_economics(input: &V) -> Result<V, String> {
    let binding = get(input, "binding");
    let ack = get(input, "ack");
    let fills = get(input, "fills");
    let before = get(input, "before");
    let after = get(input, "after");
    let activity = get(input, "activity");
    let read_at = get(input, "readAt");
    let bound_quantity = js_int(get(binding, "quantity")).unwrap_or(i64::MIN);
    need(
        !binding.is_null()
            && id(get(binding, "orderId"))
            && id(get(binding, "exchangeId"))
            && id(get(binding, "marketId"))
            && get(binding, "tournamentId").as_str().is_some_and(|s| !s.is_empty())
            && get(binding, "side").as_str() == Some("no")
            && get(binding, "action").as_str() == Some("buy")
            && js_int(get(binding, "quantity")).is_some_and(|q| q > 0 && q <= 6),
        "INVALID_NO_BUY_BINDING",
    )?;
    let limit = tick(get(binding, "sideRelativeLimit"))?;
    let prepared = time(get(binding, "preparedAt"))?;
    let now = time(read_at)?;
    need(limit >= 5_000 && limit <= 995_000 && now >= prepared, "INVALID_NO_BUY_LIMIT_OR_TIME")?;
    need(
        !ack.is_null()
            && crate::js_key(get(ack, "orderId")) == get(binding, "orderId").as_str().unwrap_or("")
            && get(ack, "exchangeId") == get(binding, "exchangeId")
            && get(ack, "tournamentId") == get(binding, "tournamentId")
            && get(ack, "side").as_str() == Some("no")
            && get(ack, "action").as_str() == Some("buy")
            && get(ack, "quantity") == get(binding, "quantity")
            && tick(get(ack, "price"))? == limit
            && get(ack, "open") == &json!(false)
            && get(ack, "remainingQuantity") == &json!(0)
            && get(ack, "quantityTraded") == get(binding, "quantity"),
        "NONTERMINAL_OR_MISMATCHED_NO_ACK",
    )?;
    // Aggregated money must retain exact decimal evidence; do not round API doubles.
    let cost = decimal_micros(get(ack, "totalCost"), "exact")?;
    need(cost > 0 && cost <= bound_quantity as i128 * limit, "INVALID_ACK_PRINCIPAL")?;
    need(
        get(fills, "complete") == &json!(true)
            && get(fills, "rows").as_array().is_some_and(|r| r.len() <= 10_000)
            && get(fills, "orderId") == get(binding, "orderId")
            && get(fills, "exchangeId") == get(binding, "exchangeId")
            && get(fills, "tournamentId") == get(binding, "tournamentId"),
        "INCOMPLETE_NO_FILL_EVIDENCE",
    )?;
    let mut unique: BTreeMap<i64, V> = BTreeMap::new();
    for row in get(fills, "rows").as_array().unwrap() {
        need(
            js_int(get(row, "id")).is_some_and(|i| i > 0)
                && get(row, "orderId") == get(binding, "orderId")
                && get(row, "marketId") == get(binding, "marketId")
                && get(row, "exchangeId") == get(binding, "exchangeId")
                && get(row, "tournamentId") == get(binding, "tournamentId")
                && get(row, "side").as_str() == Some("no")
                && js_int(get(row, "quantity")).is_some_and(|q| q < 0),
            "NO_FILL_IDENTITY_OR_SIGN",
        )?;
        let at_ms = time(get(row, "filledAt"))?;
        need(at_ms >= prepared && at_ms <= now, "NO_FILL_TIME_OUTSIDE_ORDER")?;
        tick(get(row, "price"))?;
        let rid = js_int(get(row, "id")).unwrap();
        if let Some(prior_row) = unique.get(&rid) {
            need(
                canonical_payload(prior_row)? == canonical_payload(row)?,
                "CONFLICTING_NO_FILL",
            )?;
        } else {
            unique.insert(rid, row.clone());
        }
    }
    let mut rows: Vec<V> = unique.values().cloned().collect();
    rows.sort_by(|a, b| {
        time(get(a, "filledAt")).unwrap_or(0)
            .cmp(&time(get(b, "filledAt")).unwrap_or(0))
            .then(js_int(get(a, "id")).unwrap_or(0).cmp(&js_int(get(b, "id")).unwrap_or(0)))
    });
    let total: i64 = rows.iter().map(|r| -js_int(get(r, "quantity")).unwrap_or(0)).sum();
    need(total == bound_quantity, "NO_FILL_TOTAL_MISMATCH")?;
    let snapshot = |s: &V| -> Result<Snapshot, String> {
        need(
            !s.is_null()
                && get(s, "tournamentId") == get(binding, "tournamentId")
                && get(s, "complete") == &json!(true)
                && get(s, "cashBasis").as_str() == Some("gross_cash")
                && get(s, "positions").is_array()
                && get(s, "openOrders").as_array().is_some_and(|o| o.is_empty()),
            "UNISOLATED_ACCOUNT_SNAPSHOT",
        )?;
        let start = time(get(s, "startedAt"))?;
        let end = time(get(s, "completedAt"))?;
        need(start <= end && end <= now && now - end <= 180_000, "STALE_ACCOUNT_SNAPSHOT")?;
        let mut positions = BTreeMap::new();
        for p in get(s, "positions").as_array().unwrap() {
            need(
                id(get(p, "exchangeId")) && id(get(p, "marketId"))
                    && js_int(get(p, "signedQuantity")).is_some_and(|q| q != 0)
                    && !positions.contains_key(get(p, "exchangeId").as_str().unwrap_or("")),
                "INVALID_SIGNED_POSITION",
            )?;
            positions.insert(
                get(p, "exchangeId").as_str().unwrap().to_string(),
                json!({"marketId": get(p, "marketId"), "signedQuantity": get(p, "signedQuantity")}),
            );
        }
        Ok(Snapshot {
            start,
            end,
            cash: decimal_micros(get(s, "cash"), "exact")?,
            positions,
        })
    };
    let pre = snapshot(before)?;
    let post = snapshot(after)?;
    let max_fill = rows.iter().map(|r| time(get(r, "filledAt"))).collect::<Result<Vec<i64>, String>>()?
        .into_iter().max().unwrap_or(i64::MIN);
    need(
        pre.end <= prepared && post.start >= max_fill && post.start >= pre.end,
        "ACCOUNT_SNAPSHOT_ORDERING",
    )?;
    // Independent complete account activity is needed to exclude unrelated
    // fills, settlement, fees, transfers and collateral changes during the
    // snapshots.
    need(
        get(activity, "complete") == &json!(true)
            && get(activity, "tournamentId") == get(binding, "tournamentId")
            && time(get(activity, "from"))? <= pre.start
            && time(get(activity, "through"))? >= post.end
            && time(get(activity, "through"))? <= now
            && get(activity, "rows").is_array(),
        "INCOMPLETE_ACTIVITY_WINDOW",
    )?;
    need(
        get(activity, "rows").as_array().unwrap().len() == rows.len(),
        "ACCOUNT_ACTIVITY_NOT_ISOLATED",
    )?;
    let mut activity_ids: BTreeSet<i64> = BTreeSet::new();
    for a in get(activity, "rows").as_array().unwrap() {
        let fill_id = js_int(get(a, "fillId"));
        need(
            get(a, "kind").as_str() == Some("fill")
                && get(a, "orderId") == get(binding, "orderId")
                && fill_id.is_some_and(|i| unique.contains_key(&i))
                && fill_id.is_some_and(|i| !activity_ids.contains(&i))
                && time(get(a, "at"))? == time(get(&unique[&fill_id.unwrap_or(-1)], "filledAt"))?,
            "ACCOUNT_ACTIVITY_NOT_ISOLATED",
        )?;
        activity_ids.insert(fill_id.unwrap());
    }
    need(pre.cash - post.cash == cost, "NO_CASH_PRINCIPAL_MISMATCH")?;
    let binding_ex = get(binding, "exchangeId").as_str().unwrap_or("");
    let old = pre.positions.get(binding_ex);
    let next = post.positions.get(binding_ex);
    need(
        old.is_none() || js_int(get(old.unwrap(), "signedQuantity")).unwrap_or(0) < 0,
        "NO_BUY_WOULD_NET_EXISTING_YES",
    )?;
    need(
        next.is_some()
            && get(next.unwrap(), "marketId") == get(binding, "marketId")
            && (old.is_none() || get(old.unwrap(), "marketId") == get(binding, "marketId"))
            && js_int(get(next.unwrap(), "signedQuantity"))
                == Some(js_int(&old.cloned().unwrap_or(V::Null).get("signedQuantity").cloned().unwrap_or(V::Null)).unwrap_or(0) - bound_quantity),
        "NO_POSITION_DELTA_MISMATCH",
    )?;
    for exchange in pre
        .positions
        .keys()
        .chain(post.positions.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
    {
        if exchange != binding_ex {
            need(
                canon_or_null(pre.positions.get(&exchange))?
                    == canon_or_null(post.positions.get(&exchange))?,
                "UNRELATED_POSITION_CHANGED",
            )?;
        }
    }
    let mut candidates = Vec::new();
    for basis in ["side_relative", "yes_denominated"] {
        let mut projected = Vec::new();
        for r in &rows {
            let raw = tick(get(r, "price"))?;
            let mut row_obj = r.as_object().cloned().unwrap_or_default();
            row_obj.insert(
                "effectivePrice".into(),
                json!(format_micros(if basis == "side_relative" { raw } else { SCALE - raw })),
            );
            projected.push(V::Object(row_obj));
        }
        let principal: i128 = projected
            .iter()
            .map(|r| (-js_int(get(r, "quantity")).unwrap_or(0)) as i128 * decimal_micros(get(r, "effectivePrice"), "exact").unwrap_or(0))
            .sum();
        let valid = principal == cost
            && projected.iter().all(|r| {
                decimal_micros(get(r, "effectivePrice"), "exact").map(|p| p <= limit).unwrap_or(false)
            });
        if valid {
            candidates.push((basis, principal, projected));
        }
    }
    need(candidates.len() == 1, "AMBIGUOUS_OR_INCONSISTENT_NO_PRICE_BASIS")?;
    let (basis_name, _, projected) = candidates.into_iter().next().unwrap();
    let mut result = Map::from_iter([
        ("schemaVersion".into(), json!(1)),
        ("orderId".into(), get(binding, "orderId").clone()),
        ("bindingHash".into(), json!(payload_hash(binding)?)),
        (
            "evidenceHash".into(),
            json!(payload_hash(&json!({"ack": ack, "fills": fills, "before": before, "after": after, "activity": activity}))?),
        ),
        ("readAt".into(), read_at.clone()),
        ("priceBasis".into(), json!(basis_name)),
        ("principalCost".into(), json!(format_micros(cost))),
        ("signedPositionDelta".into(), json!(-bound_quantity)),
        ("filledQuantity".into(), json!(bound_quantity)),
        ("fills".into(), V::Array(projected)),
        ("scope".into(), json!("this_terminal_order_only")),
        ("globalConventionEstablished".into(), json!(false)),
        ("dispatchAuthority".into(), json!(false)),
        ("authoritativeReconciliation".into(), json!(false)),
    ]);
    let vh = payload_hash(&V::Object(result.clone()))?;
    result.insert("verificationHash".into(), json!(vh));
    Ok(V::Object(result))
}
