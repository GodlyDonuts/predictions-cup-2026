// execution/canonical-fifo.mjs — pure attribution projection, not an execution
// permit or account reconciliation. Canonicalization uses the complete
// pre-order signed position, never a guessed holding or a dashboard mark.
// Unknown collateral economics remain rejected.
use crate::core::{canonical_payload, decimal_micros, format_micros, payload_hash, SCALE};
use crate::side_accounting::effective_fill_price;
use baal_service_runtime::json_text::{js_int, js_truthy};
use serde_json::{json, Map, Value as V};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(c.into()) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn side(x: &V) -> bool {
    x.as_str() == Some("yes") || x.as_str() == Some("no")
}
fn time(x: &V) -> bool {
    x.as_str().is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok())
}
fn parse_ms(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0 && n <= 9_007_199_254_740_991)
    })
}
fn money(x: &V) -> Result<i128, String> {
    need(x.is_string(), "EXACT_DECIMAL_REQUIRED")?;
    decimal_micros(x, "exact")
}
fn signed_money(x: &V) -> Result<i128, String> {
    need(x.is_string(), "EXACT_DECIMAL_REQUIRED")?;
    match x.as_str().unwrap().strip_prefix('-') {
        Some(rest) => Ok(-decimal_micros(&json!(rest), "exact")?),
        None => decimal_micros(x, "exact"),
    }
}
fn signed_format(x: i128) -> String {
    if x < 0 { format!("-{}", format_micros(-x)) } else { format_micros(x) }
}
fn price(x: &V) -> Result<i128, String> {
    let n = money(x)?;
    need(n >= 5_000 && n <= 995_000 && n % 5_000 == 0, "INVALID_TICK")?;
    Ok(n)
}
fn sign(s: &str) -> i64 {
    if s == "yes" { 1 } else { -1 }
}

// projectCanonicalFifo({wallet,binding,ack,rows,source,readAt})
pub fn project_canonical_fifo(input: &V) -> Result<V, String> {
    let wallet = get(input, "wallet");
    let binding = get(input, "binding");
    let ack = get(input, "ack");
    let rows = get(input, "rows");
    let source = get(input, "source").as_str().unwrap_or("");
    let read_at = get(input, "readAt");
    need(
        js_truthy(Some(wallet)) && js_truthy(Some(binding)) && js_truthy(Some(ack))
            && time(read_at) && time(get(binding, "createdAt"))
            && parse_ms(read_at.as_str().unwrap()) >= parse_ms(get(binding, "createdAt").as_str().unwrap()),
        "INVALID_CANONICAL_INPUT",
    )?;
    need(
        id(get(binding, "orderId")) && id(get(binding, "exchangeId")) && id(get(binding, "marketId"))
            && get(binding, "tournamentId").as_str().is_some_and(|s| !s.is_empty())
            && get(binding, "ownerId").as_str().is_some_and(|s| !s.is_empty())
            && get(wallet, "ownerId") == get(binding, "ownerId")
            && get(wallet, "exchangeId") == get(binding, "exchangeId")
            && get(wallet, "marketId") == get(binding, "marketId")
            && get(wallet, "tournamentId") == get(binding, "tournamentId"),
        "ATTRIBUTION_SCOPE",
    )?;
    let binding_side = get(binding, "side").as_str().unwrap_or("");
    let binding_action = get(binding, "action").as_str().unwrap_or("");
    let bound_quantity = js_int(get(binding, "quantity")).unwrap_or(i64::MIN);
    need(
        side(get(binding, "side"))
            && (binding_action == "buy" || binding_action == "sell")
            && js_int(get(binding, "quantity")).is_some_and(|q| q > 0 && q <= 100),
        "INVALID_CANONICAL_TERMS",
    )?;
    let applied: Vec<V> = if get(wallet, "appliedOrderIds").is_null() {
        Vec::new()
    } else {
        get(wallet, "appliedOrderIds").as_array().cloned().unwrap_or_default()
    };
    let applied_ok = get(wallet, "appliedOrderIds").is_null()
        || (get(wallet, "appliedOrderIds").is_array() && applied.len() <= 10_000);
    need(
        applied_ok
            && applied.iter().all(id)
            && applied.iter().map(|a| a.as_str().unwrap_or("")).collect::<BTreeSet<_>>().len() == applied.len()
            && !applied.iter().any(|a| a == get(binding, "orderId")),
        "ORDER_ALREADY_PROJECTED_OR_INVALID_HISTORY",
    )?;
    let limit = price(get(binding, "limit"))?;
    let cash = money(get(wallet, "cash"))?;
    let realized = signed_money(get(wallet, "realizedPnl"))?;
    need(
        cash >= 0 && get(wallet, "lots").is_array() && get(wallet, "lots").as_array().unwrap().len() <= 10_000,
        "INVALID_FIFO_WALLET",
    )?;
    let mut keys: BTreeSet<String> = BTreeSet::new();
    let mut held: i128 = 0;
    let mut initial_basis: i128 = 0;
    let mut held_side: Option<String> = None;
    let mut lots: VecDeque<V> = VecDeque::new();
    for l in get(wallet, "lots").as_array().unwrap() {
        let lot_id = get(l, "lotId").as_str().unwrap_or("").to_string();
        need(
            !lot_id.is_empty() && !keys.contains(&lot_id)
                && side(get(l, "side"))
                && js_int(get(l, "quantity")).is_some_and(|q| q > 0)
                && time(get(l, "openedAt"))
                && parse_ms(get(l, "openedAt").as_str().unwrap()) <= parse_ms(get(binding, "createdAt").as_str().unwrap()),
            "INVALID_FIFO_LOT",
        )?;
        keys.insert(lot_id);
        let lside = get(l, "side").as_str().unwrap().to_string();
        if held_side.is_none() {
            held_side = Some(lside.clone());
        }
        need(held_side.as_deref() == Some(lside.as_str()), "MIXED_SIGNED_LOTS")?;
        held += js_int(get(l, "quantity")).unwrap() as i128;
        need(held <= 9_007_199_254_740_991, "FIFO_QUANTITY_OVERFLOW")?;
        let unit = price(get(l, "entryPrice"))?;
        initial_basis += js_int(get(l, "quantity")).unwrap() as i128 * unit;
        let mut lot = l.as_object().cloned().unwrap_or_default();
        lot.insert("entryPrice".into(), json!(format_micros(unit)));
        lots.push_back(V::Object(lot));
    }
    let lots_vec: Vec<V> = lots.iter().cloned().collect();
    for i in 1..lots_vec.len() {
        need(
            parse_ms(get(&lots_vec[i], "openedAt").as_str().unwrap()) >= parse_ms(get(&lots_vec[i - 1], "openedAt").as_str().unwrap()),
            "FIFO_LOT_CLOCK",
        )?;
    }
    let signed = held * sign(held_side.as_deref().unwrap_or("yes")) as i128;
    need(
        js_int(get(wallet, "accountSignedQuantity")).is_some_and(|q| q as i128 == signed),
        "UNATTRIBUTED_ACCOUNT_HOLDING",
    )?;
    let held_i = held as i64;
    let backed = binding_action == "sell" && held_side.as_deref() == Some(binding_side) && held_i >= bound_quantity;
    let canonical_action = if binding_action == "buy" || !backed { "buy" } else { "sell" };
    let complement = binding_action == "sell" && !backed;
    let canonical_side = if complement {
        if binding_side == "yes" { "no" } else { "yes" }
    } else {
        binding_side
    };
    let canonical_limit = if complement { SCALE - limit } else { limit };
    need(
        crate::js_key(get(ack, "orderId")) == get(binding, "orderId").as_str().unwrap_or("")
            && get(ack, "exchangeId") == get(binding, "exchangeId")
            && get(ack, "side").as_str() == Some(canonical_side)
            && get(ack, "action").as_str() == Some(canonical_action)
            && get(ack, "quantity") == get(binding, "quantity")
            && price(get(ack, "price"))? == canonical_limit
            && ack.get("all") == Some(&V::Null),
        "CANONICAL_ACK_OR_COLLATERAL_UNVERIFIED",
    )?;
    need(
        rows.is_array() && rows.as_array().unwrap().len() <= 10_000
            && get(ack, "open").is_boolean()
            && js_int(get(ack, "remainingQuantity")).is_some_and(|q| q >= 0)
            && js_int(get(ack, "quantityTraded")).is_some_and(|q| q >= 0)
            && js_int(get(ack, "quantityTraded")).unwrap() + js_int(get(ack, "remainingQuantity")).unwrap() == bound_quantity,
        "INCOMPLETE_CANONICAL_ACK",
    )?;
    let mut seen: BTreeMap<i64, V> = BTreeMap::new();
    for f in rows.as_array().unwrap() {
        let fq = js_int(get(f, "quantity"));
        let filled = get(f, "filledAt");
        need(
            js_int(get(f, "id")).is_some_and(|n| n > 0)
                && crate::js_key(get(f, "orderId")) == get(binding, "orderId").as_str().unwrap_or("")
                && get(f, "exchangeId") == get(binding, "exchangeId")
                && get(f, "marketId") == get(binding, "marketId")
                && get(f, "tournamentId") == get(binding, "tournamentId")
                && get(f, "side").as_str() == Some(canonical_side)
                && fq.is_some_and(|q| q != 0 && q.signum() == sign(canonical_side))
                && time(filled)
                && parse_ms(filled.as_str().unwrap()) >= parse_ms(get(binding, "createdAt").as_str().unwrap())
                && parse_ms(filled.as_str().unwrap()) <= parse_ms(read_at.as_str().unwrap()),
            "CANONICAL_FILL_BINDING",
        )?;
        let unit = decimal_micros(&json!(effective_fill_price(get(f, "price"), canonical_side, source)?), "exact")?;
        need(
            if canonical_action == "buy" { unit <= canonical_limit } else { unit >= canonical_limit },
            "CANONICAL_FILL_LIMIT",
        )?;
        let row = json!({
            "id": js_int(get(f, "id")).unwrap(),
            "quantity": fq.unwrap().abs(),
            "unit": format_micros(unit),
            "filledAt": filled,
        });
        if let Some(prior_row) = seen.get(&js_int(get(f, "id")).unwrap()) {
            need(
                canonical_payload(prior_row)? == canonical_payload(&row)?,
                "CONFLICTING_CANONICAL_FILL",
            )?;
        } else {
            seen.insert(js_int(get(f, "id")).unwrap(), row);
        }
    }
    let mut fills: Vec<V> = seen.into_values().collect();
    fills.sort_by(|a, b| {
        parse_ms(get(a, "filledAt").as_str().unwrap())
            .cmp(&parse_ms(get(b, "filledAt").as_str().unwrap()))
            .then(js_int(get(a, "id")).unwrap_or(0).cmp(&js_int(get(b, "id")).unwrap_or(0)))
    });
    let fill_total: i64 = fills.iter().map(|f| js_int(get(f, "quantity")).unwrap_or(0)).sum();
    need(
        fill_total == js_int(get(ack, "quantityTraded")).unwrap()
            && (get(ack, "open").as_bool() == Some(true)
                || js_int(get(ack, "remainingQuantity")) == Some(0)
                || get(ack, "terminalReasonCode").as_str().is_some_and(|s| !s.is_empty())),
        "CANONICAL_FILL_TOTAL",
    )?;
    let mut cash_delta: i128 = 0;
    let mut realized_delta: i128 = 0;
    let mut nominal: i128 = 0;
    let mut closed_quantity: i64 = 0;
    let mut opened_quantity: i64 = 0;
    for f in &fills {
        let mut left = js_int(get(f, "quantity")).unwrap();
        let unit = money(get(f, "unit"))?;
        nominal += left as i128 * unit;
        let front_mismatch = lots.front().is_some_and(|l| get(l, "side").as_str() != Some(canonical_side));
        if canonical_action == "sell" || front_mismatch {
            let proceeds = if canonical_action == "sell" { unit } else { SCALE - unit };
            while left > 0 && !lots.is_empty() {
                let l = lots.front_mut().unwrap();
                let lq = js_int(get(l, "quantity")).unwrap();
                let q = left.min(lq);
                cash_delta += q as i128 * proceeds;
                realized_delta += q as i128 * (proceeds - money(get(l, "entryPrice"))?);
                closed_quantity += q;
                left -= q;
                let new_q = lq - q;
                if let Some(obj) = l.as_object_mut() {
                    obj.insert("quantity".into(), json!(new_q));
                }
                if new_q == 0 {
                    lots.pop_front();
                }
            }
            need(canonical_action != "sell" || left == 0, "SELL_EXCEEDS_ATTRIBUTED_LOTS")?;
        }
        if left > 0 {
            let lot_id = format!("order:{}:fill:{}", get(binding, "orderId").as_str().unwrap_or(""), js_int(get(f, "id")).unwrap());
            need(!keys.contains(&lot_id), "FIFO_LOT_ID_COLLISION")?;
            keys.insert(lot_id.clone());
            lots.push_back(json!({
                "lotId": lot_id,
                "side": canonical_side,
                "quantity": left,
                "entryPrice": get(f, "unit"),
                "openedAt": get(f, "filledAt"),
            }));
            cash_delta -= left as i128 * unit;
            opened_quantity += left;
        }
    }
    need(money(get(ack, "totalCost"))? == nominal, "CANONICAL_ACK_PRINCIPAL_MISMATCH")?;
    need(cash + cash_delta >= 0, "FIFO_CASH_UNDERFLOW")?;
    let basis: i128 = lots
        .iter()
        .map(|l| js_int(get(l, "quantity")).unwrap_or(0) as i128 * money(get(l, "entryPrice")).unwrap_or(0))
        .sum();
    need(cash_delta + basis - initial_basis == realized_delta, "FIFO_ACCOUNTING_IDENTITY")?;
    let mut next_wallet = wallet.as_object().cloned().unwrap_or_default();
    next_wallet.insert("cash".into(), json!(format_micros(cash + cash_delta)));
    next_wallet.insert("realizedPnl".into(), json!(signed_format(realized + realized_delta)));
    next_wallet.insert("lots".into(), V::Array(lots.iter().cloned().collect()));
    let next_signed: i64 = lots
        .iter()
        .map(|l| sign(get(l, "side").as_str().unwrap_or("yes")) * js_int(get(l, "quantity")).unwrap_or(0))
        .sum();
    next_wallet.insert("accountSignedQuantity".into(), json!(next_signed));
    let mut next_applied = applied.clone();
    next_applied.push(get(binding, "orderId").clone());
    next_wallet.insert("appliedOrderIds".into(), V::Array(next_applied));
    let remaining_reservation = if canonical_action == "buy" && get(ack, "open").as_bool() == Some(true) {
        format_micros(js_int(get(ack, "remainingQuantity")).unwrap() as i128 * canonical_limit)
    } else {
        "0.000000".to_string()
    };
    let mut result = Map::from_iter([
        ("schemaVersion".into(), json!(1)),
        ("bindingHash".into(), json!(payload_hash(binding)?)),
        ("inputWalletHash".into(), json!(payload_hash(wallet)?)),
        (
            "evidenceHash".into(),
            json!(payload_hash(&json!({"ack": ack, "rows": rows, "source": source, "readAt": read_at}))?),
        ),
        ("canonicalAction".into(), json!(canonical_action)),
        ("canonicalSide".into(), json!(canonical_side)),
        ("complement".into(), json!(complement)),
        ("filledQuantity".into(), get(ack, "quantityTraded").clone()),
        ("closedQuantity".into(), json!(closed_quantity)),
        ("openedQuantity".into(), json!(opened_quantity)),
        ("nominalPrincipal".into(), json!(format_micros(nominal))),
        ("cashDelta".into(), json!(signed_format(cash_delta))),
        ("realizedDelta".into(), json!(signed_format(realized_delta))),
        ("remainingReservation".into(), json!(remaining_reservation)),
        ("wallet".into(), V::Object(next_wallet)),
        ("executionAuthority".into(), json!(false)),
        ("authoritativeReconciliation".into(), json!(false)),
    ]);
    let ph = payload_hash(&V::Object(result.clone()))?;
    result.insert("projectionHash".into(), json!(ph));
    Ok(V::Object(result))
}
