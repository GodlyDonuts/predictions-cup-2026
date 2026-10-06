// execution/side-accounting.mjs — fill-price conventions and the bound-BUY
// fill projection. REST schemas specify signed quantities but do not specify
// the NO price basis; it is never inferred from the realtime guide.
use crate::core::{canonical_payload, decimal_micros, format_micros, payload_hash, SCALE};
use crate::gates::normalize_official_fill_tick;
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Map, Value as V};
use std::collections::BTreeMap;

fn require_that(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.into()) }
}
fn identity(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0)
    })
}
fn time(x: &V) -> bool {
    x.as_str().is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok())
}
fn parse_ms(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn side_of(x: &V) -> Result<String, String> {
    match x.as_str() {
        Some("yes") | Some("no") => Ok(x.as_str().unwrap().to_string()),
        _ => Err("INVALID_OUTCOME_SIDE".into()),
    }
}
fn price_of(x: &V) -> Result<i128, String> {
    require_that(
        x.is_string() || x.as_f64().is_some_and(|n| n.is_finite()),
        "MISSING_EXECUTION_PRICE",
    )?;
    let t = normalize_official_fill_tick(x)?;
    let p = decimal_micros(get(&t, "price"), "exact")?;
    require_that(p >= 0 && p <= SCALE, "INVALID_EXECUTION_PRICE")?;
    Ok(p)
}

// effectiveFillPrice(price, side, source)
pub fn effective_fill_price(price: &V, side: &str, source: &str) -> Result<String, String> {
    require_that(side == "yes" || side == "no", "INVALID_OUTCOME_SIDE")?;
    require_that(
        source == "realtime_account_batch" || source == "rest_order_fills",
        "UNKNOWN_FILL_SOURCE",
    )?;
    require_that(
        side == "yes" || source == "realtime_account_batch",
        "UNVERIFIED_REST_NO_PRICE_BASIS",
    )?;
    let raw = price_of(price)?;
    Ok(format_micros(if side == "no" { SCALE - raw } else { raw }))
}

// projectBoundBuyFills options ({source, readAt, complete = false, prior = null}).
pub struct BoundFillOptions<'a> {
    pub source: &'a str,
    pub read_at: &'a V,
    pub complete: bool,
    pub prior: Option<&'a V>,
}

// projectBoundBuyFills(binding, rows, options). Pure, non-authoritative
// projection; the binding is an already canonical BUY with a side-relative
// limit. It neither creates nor dispatches orders.
pub fn project_bound_buy_fills(binding: &V, rows: &V, options: BoundFillOptions) -> Result<V, String> {
    require_that(
        baal_service_runtime::json_text::js_truthy(Some(binding))
            && identity(get(binding, "orderId"))
            && identity(get(binding, "exchangeId"))
            && identity(get(binding, "marketId"))
            && (get(binding, "tournamentId").is_null()
                || get(binding, "tournamentId").as_str().is_some_and(|s| !s.is_empty()))
            && get(binding, "action").as_str() == Some("buy")
            && time(get(binding, "createdAt")),
        "INVALID_BUY_BINDING",
    )?;
    let side = side_of(get(binding, "side"))?;
    let bound_quantity = js_int(get(binding, "quantity"));
    require_that(
        bound_quantity.is_some_and(|q| q > 0 && q <= 100),
        "INVALID_BOUND_QUANTITY",
    )?;
    let bound_quantity = bound_quantity.unwrap();
    let limit = price_of(get(binding, "sideRelativeLimit"))?;
    require_that(limit >= 5_000 && limit <= 995_000 && limit % 5_000 == 0, "INVALID_BOUND_LIMIT")?;
    require_that(
        time(options.read_at)
            && parse_ms(options.read_at.as_str().unwrap()) >= parse_ms(get(binding, "createdAt").as_str().unwrap()),
        "INVALID_FILL_READ_TIME",
    )?;
    require_that(rows.is_array() && rows.as_array().unwrap().len() <= 10_000, "INVALID_FILL_ROWS")?;
    // Source/basis probe identical to the JS validation call.
    effective_fill_price(&json!("0.5"), &side, options.source)?;
    let bound = json!({
        "orderId": get(binding, "orderId"),
        "exchangeId": get(binding, "exchangeId"),
        "marketId": get(binding, "marketId"),
        "tournamentId": get(binding, "tournamentId"),
        "action": "buy",
        "side": get(binding, "side"),
        "quantity": get(binding, "quantity"),
        "sideRelativeLimit": format_micros(limit),
        "createdAt": get(binding, "createdAt"),
    });
    let binding_hash = payload_hash(&bound)?;
    let mut seen: BTreeMap<i64, V> = BTreeMap::new();
    for row in rows.as_array().unwrap() {
        let rid = js_int(get(row, "id"));
        require_that(
            row.is_object()
                && rid.is_some_and(|n| n > 0)
                && time(get(row, "filledAt"))
                && get(row, "orderId") == get(&bound, "orderId")
                && get(row, "exchangeId") == get(&bound, "exchangeId")
                && get(row, "marketId") == get(&bound, "marketId")
                && get(row, "tournamentId") == get(&bound, "tournamentId")
                && get(row, "side") == get(&bound, "side"),
            "FILL_IDENTITY_MISMATCH",
        )?;
        let rq = js_int(get(row, "quantity"));
        require_that(
            rq.is_some_and(|q| q != 0 && if get(row, "side").as_str() == Some("yes") { q > 0 } else { q < 0 }),
            "FILL_SIDE_SIGN_MISMATCH",
        )?;
        let filled_ms = parse_ms(get(row, "filledAt").as_str().unwrap());
        require_that(
            filled_ms >= parse_ms(get(&bound, "createdAt").as_str().unwrap())
                && filled_ms <= parse_ms(options.read_at.as_str().unwrap()),
            "INVALID_FILL_TIME",
        )?;
        let effective_price = effective_fill_price(get(row, "price"), get(row, "side").as_str().unwrap(), options.source)?;
        let p = decimal_micros(&json!(effective_price), "exact")?;
        require_that(p <= limit, "FILL_OUTSIDE_SIDE_RELATIVE_BUY_LIMIT")?;
        let tick = normalize_official_fill_tick(get(row, "price"))?;
        let fill = json!({
            "id": rid,
            "signedQuantity": rq,
            "quantity": rq.unwrap().abs(),
            "rawPrice": format_micros(price_of(get(row, "price"))?),
            "rawApiPrice": crate::js_key(get(row, "price")),
            "priceRepresentation": get(&tick, "representation"),
            "effectivePrice": effective_price,
            "filledAt": get(row, "filledAt"),
        });
        if let Some(prior_fill) = seen.get(&rid.unwrap()) {
            require_that(
                canonical_payload(prior_fill)? == canonical_payload(&fill)?,
                "CONFLICTING_DUPLICATE_FILL",
            )?;
        } else {
            seen.insert(rid.unwrap(), fill);
        }
    }
    let mut fills: Vec<V> = seen.into_values().collect();
    fills.sort_by(|a, b| {
        parse_ms(get(a, "filledAt").as_str().unwrap())
            .cmp(&parse_ms(get(b, "filledAt").as_str().unwrap()))
            .then(js_int(get(a, "id")).unwrap_or(0).cmp(&js_int(get(b, "id")).unwrap_or(0)))
    });
    // JS sums in f64 then requires Number.isSafeInteger; i128 accumulation
    // preserves precision past 2^53, so mirror the check explicitly.
    let quantity_wide: i128 = fills.iter().map(|f| js_int(get(f, "quantity")).unwrap_or(0) as i128).sum();
    require_that(
        quantity_wide <= 9_007_199_254_740_991 && quantity_wide <= bound_quantity as i128,
        "OVERFILLED_BOUND_ORDER",
    )?;
    let quantity = quantity_wide as i64;
    if let Some(prior) = options.prior {
        require_that(
            get(prior, "bindingHash").as_str() == Some(binding_hash.as_str())
                && get(prior, "source").as_str() == Some(options.source),
            "PRIOR_BINDING_MISMATCH",
        )?;
        // payloadHash(prior.binding) throws INVALID_JSON_VALUE when the key is
        // absent (JS canonicalPayload(undefined)); propagate before comparing.
        let prior_binding_hash = match prior.get("binding") {
            Some(b) => payload_hash(b)?,
            None => return Err("INVALID_JSON_VALUE".into()),
        };
        require_that(
            prior_binding_hash == binding_hash && get(prior, "fills").is_array(),
            "PRIOR_BINDING_MISMATCH",
        )?;
        require_that(
            time(get(prior, "readAt"))
                && parse_ms(get(prior, "readAt").as_str().unwrap()) <= parse_ms(options.read_at.as_str().unwrap()),
            "FILL_READ_CLOCK_REGRESSION",
        )?;
        let mut previous_projection: Map<String, V> = prior.as_object().cloned().unwrap_or_default();
        let projection_hash = previous_projection.shift_remove("projectionHash").unwrap_or(V::Null);
        require_that(
            projection_hash.as_str() == Some(payload_hash(&V::Object(previous_projection))?.as_str()),
            "INVALID_PRIOR_PROJECTION",
        )?;
        require_that(
            !baal_service_runtime::json_text::js_truthy(Some(get(prior, "complete"))) || options.complete,
            "COMPLETE_HISTORY_REGRESSION",
        )?;
        for old in get(prior, "fills").as_array().unwrap() {
            require_that(
                fills.iter().any(|f| canonical_payload(f).ok() == canonical_payload(old).ok()),
                "FILL_HISTORY_CHANGED",
            )?;
        }
    }
    let cost: i128 = fills
        .iter()
        .map(|f| js_int(get(f, "quantity")).unwrap_or(0) as i128 * decimal_micros(get(f, "effectivePrice"), "exact").unwrap_or(0))
        .sum();
    let signed_position_delta = if get(&bound, "side").as_str() == Some("yes") || quantity == 0 {
        quantity
    } else {
        -quantity
    };
    let mut projection = Map::from_iter([
        ("schemaVersion".into(), json!(1)),
        ("binding".into(), bound.clone()),
        ("bindingHash".into(), json!(binding_hash)),
        ("source".into(), json!(options.source)),
        ("readAt".into(), options.read_at.clone()),
        ("complete".into(), json!(options.complete)),
        ("fills".into(), V::Array(fills)),
        ("filledQuantity".into(), json!(quantity)),
        ("signedPositionDelta".into(), json!(signed_position_delta)),
        ("principalCost".into(), json!(format_micros(cost))),
        (
            "maximumRemainingReservation".into(),
            json!(format_micros((bound_quantity - quantity) as i128 * limit)),
        ),
        ("actualOrderAuthority".into(), json!(false)),
        ("authoritativeReconciliation".into(), json!(false)),
    ]);
    let ph = payload_hash(&V::Object(projection.clone()))?;
    projection.insert("projectionHash".into(), json!(ph));
    Ok(V::Object(projection))
}
