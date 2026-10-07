// execution/baal-scaled-owned-no-hedge-proposal-v1.mjs — pure attributed
// counterpart proposal, never a hedge order/activation or a claim of cash
// profit. Each paired first-leg unit retains its actual basis.
use baal_collector::model::normalize_book;
use baal_execution::core::payload_hash;
use baal_execution::party_winner_no_screen_v1::screen_party_winner_no_pairs;
use baal_execution::receipts::parse_number_lexemes;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use serde_json::{json, Map, Value as V};
use std::collections::BTreeMap;

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("SCALED_OWNED_HEDGE_{c}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    static NULL: V = V::Null;
    v.as_object().and_then(|m| m.get(k)).unwrap_or(&NULL)
}
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn num_id(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok()
    })
}
fn minimum(v: &[i128]) -> i128 {
    v.iter().copied().reduce(|a, b| if a < b { a } else { b }).unwrap()
}
fn sorted(rows: &V) -> V {
    let mut v = rows.as_array().cloned().unwrap_or_default();
    v.sort_by(|a, b| {
        let na = get(a, "id").as_f64().or_else(|| get(a, "id").as_str().and_then(|s| s.parse::<f64>().ok()));
        let nb = get(b, "id").as_f64().or_else(|| get(b, "id").as_str().and_then(|s| s.parse::<f64>().ok()));
        match (na, nb) {
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
            _ => std::cmp::Ordering::Equal,
        }
    });
    json!(v)
}
fn slug_ok(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}
fn iso(ms: i64) -> Result<String, String> {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .ok_or_else(|| "INVALID_TIMESTAMP".to_string())
}
fn bigint_str(s: &str) -> Result<i128, String> {
    s.parse::<i128>().map_err(|_| "INVALID_BIGINT".to_string())
}
fn bigint_v(v: &V) -> Result<i128, String> {
    js_int(v).map(|q| q as i128).ok_or_else(|| "INVALID_BIGINT".to_string())
}

struct Raw {
    data: V,
    numbers: BTreeMap<String, String>,
    hash: String,
    start: i64,
    end: i64,
}
fn raw(r: &V, endpoint: &str, at: i64, age: i64) -> Result<Raw, String> {
    need(
        get(r, "method").as_str() == Some("GET")
            && js_int(get(r, "status")) == Some(200)
            && get(r, "endpoint").as_str() == Some(endpoint)
            && get(r, "bodyText").is_string()
            && get(r, "bodyText").as_str().unwrap().as_bytes().len() <= 8 * 1024 * 1024,
        "RAW_RECEIPT",
    )?;
    let start = instant(get(r, "requestStartedAt").as_str().unwrap_or(""))?;
    let end = instant(get(r, "receivedAt").as_str().unwrap_or(""))?;
    need(start <= end && end <= at && at - start <= age && end - start <= 20000, "FRESH_SOURCE")?;
    let hash = sha256(get(r, "bodyText").as_str().unwrap_or("").as_bytes());
    need(
        get(r, "sourceBodyHash").is_null() || get(r, "sourceBodyHash").as_str() == Some(hash.as_str()),
        "SOURCE_HASH",
    )?;
    let (data, numbers) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap())?;
    Ok(Raw { data, numbers, hash, start, end })
}

fn owned_proof(owned: &V, scope: &V) -> Result<V, String> {
    let p = get(owned, "proof");
    let mut body = p.as_object().cloned().unwrap_or_default();
    body.remove("verificationHash");
    let verification_hash = get(p, "verificationHash");
    need(
        hex(verification_hash)
            && payload_hash(&V::Object(body))? == verification_hash.as_str().unwrap_or("")
            && get(p, "mode").as_str() == Some("scoped_rest_no_multishare_fifo_v1")
            && get(p, "scopedTradeVerified").as_bool() == Some(true)
            && get(p, "side").as_str() == Some("no")
            && get(p, "ownerId") == get(scope, "ownerId")
            && get(p, "runId") == get(scope, "runId")
            && get(p, "tournamentId") == get(scope, "tournamentId")
            && num_id(get(p, "marketId"))
            && num_id(get(p, "exchangeId"))
            && js_int(get(p, "remainingOwnedQuantity")).is_some_and(|q| q > 0)
            && get(p, "remainingLots").is_array(),
        "ACTUAL_OWNED_PROOF",
    )?;
    let mut q: i64 = 0;
    let mut cost: i128 = 0;
    let mut lots = std::collections::BTreeSet::new();
    for l in get(p, "remainingLots").as_array().unwrap() {
        let lot_id = get(l, "lotId");
        let quantity = js_int(get(l, "quantity"));
        need(
            lot_id.as_str().is_some_and(|s| !s.is_empty())
                && !lots.contains(lot_id.as_str().unwrap())
                && quantity.is_some_and(|n| n > 0),
            "OWNED_FIFO",
        )?;
        lots.insert(lot_id.as_str().unwrap().to_string());
        q += quantity.unwrap();
        cost += price(get(l, "entryPrice").as_str().unwrap_or(""))? * quantity.unwrap() as i128;
    }
    need(
        q == js_int(get(p, "remainingOwnedQuantity")).unwrap()
            && money(cost) == get(p, "remainingOwnedCost").as_str().unwrap_or(""),
        "EXACT_REMAINING_BASIS",
    )?;
    Ok(p.clone())
}

fn full_account(a: &V, exchange_id: &str) -> V {
    let scoped = match get(a, "scopedOrdersPagesByExchange").get(exchange_id) {
        Some(v) if !v.is_null() => v.clone(),
        _ => get(a, "scopedOrdersPages").clone(),
    };
    json!({
        "cash": get(a, "cash"),
        "positions": get(a, "positions"),
        "openOrdersPages": get(a, "openOrdersPages"),
        "scopedOrdersPages": scoped,
    })
}

fn sources(s: &V) -> V {
    json!(s
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|r| json!({
            "endpoint": get(r, "endpoint"),
            "sourceBodyHash": get(r, "sourceBodyHash"),
            "receivedAt": get(r, "receivedAt"),
        }))
        .collect::<Vec<_>>())
}

fn account_continuity(owned: &V, p: &V, current: &V, scope: &V, _now: &V) -> Result<(), String> {
    let mut binding = scope.as_object().cloned().unwrap_or_default();
    binding.insert("exchangeId".into(), get(p, "exchangeId").clone());
    let old = crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account(&json!({
        "account": get(owned, "after"),
        "binding": V::Object(binding),
        "readAt": get(p, "readAt"),
    }))?;
    need(
        payload_hash(&sources(get(&old, "sourceReceipts")))?
            == payload_hash(&sources(get(get(p, "sourceReceipts"), "after")))?
            && instant(get(p, "readAt").as_str().unwrap_or(""))?
                <= instant(get(current, "startedAt").as_str().unwrap_or(""))?
            && payload_hash(&sorted(get(&old, "scopedOrders")))?
                == payload_hash(&sorted(get(current, "scopedOrders")))?,
        "ORIGINAL_AFTER_OR_INTERVENING_ACTIVITY",
    )?;
    let position = get(current, "positions")
        .as_array()
        .and_then(|rows| rows.iter().find(|r| get(r, "exchangeId") == get(p, "exchangeId")))
        .cloned()
        .unwrap_or(V::Null);
    need(
        get(&position, "marketId") == get(p, "marketId")
            && js_int(get(&position, "quantity")) == Some(-js_int(get(p, "remainingOwnedQuantity")).unwrap_or(0))
            && !js_bool(get(&position, "settled")),
        "EXACT_CURRENT_NO_HOLDING",
    )?;
    Ok(())
}
fn js_bool(v: &V) -> bool {
    baal_service_runtime::json_text::js_truthy(Some(v))
}

// planBaalOwnedNoHedge({scope,owned,pairCensus,account,rootReceipts,otherBookReceipt,funding,globalRisk,now})
pub fn plan_baal_owned_no_hedge(input: &V) -> V {
    let scope = get(input, "scope");
    let owned = get(input, "owned");
    let c = get(input, "pairCensus");
    let account = get(input, "account");
    let root_receipts = get(input, "rootReceipts");
    let other_book_receipt = get(input, "otherBookReceipt");
    let funding = get(input, "funding");
    let g = get(input, "globalRisk");
    let now = get(input, "now");

    let base = json!({
        "schemaVersion": 1,
        "mode": "baal_multishare_owned_no_counterpart_proposal_v1",
        "at": now,
        "proposalReady": false,
        "quantity": 0,
        "orderEligible": false,
        "executionAuthority": false,
        "newCodeRevisionAndAuthorizationRequired": true,
        "networkRequests": 0,
        "competitionOrders": 0,
        "cashProfitEstablished": false,
        "originalInventoryChanged": false,
    });
    let base_c = base.clone();
    let out = (|| -> Result<V, String> {
        let at = instant(now.as_str().unwrap_or(""))?;
        need(
            get(scope, "ownerId").as_str() == Some("Codex")
                && get(scope, "runId").as_str().is_some_and(|s| !s.is_empty())
                && get(scope, "tournamentId").is_string()
                && slug_ok(get(scope, "tournamentSlug").as_str().unwrap_or("")),
            "SCOPE",
        )?;
        let p = owned_proof(owned, scope)?;
        need(
            get(g, "ownerId") == get(scope, "ownerId")
                && get(g, "runId") == get(scope, "runId")
                && get(g, "tournamentId") == get(scope, "tournamentId")
                && get(g, "complete").as_bool() == Some(true)
                && get(g, "eventScopeComplete").as_bool() == Some(true)
                && get(g, "soleCollector").as_bool() == Some(true)
                && get(g, "stopped").as_bool() == Some(false)
                && get(g, "unknownAcceptance").as_bool() == Some(false)
                && get(g, "unresolvedIntentIds").as_array().is_some_and(|a| a.is_empty())
                && hex(get(g, "sourceHash"))
                && hex(get(g, "allocationHash"))
                && instant(get(g, "at").as_str().unwrap_or(""))? <= at
                && at - instant(get(g, "at").as_str().unwrap_or(""))? <= 20000,
            "UNKNOWN_OR_GLOBAL_SCOPE",
        )?;
        need(
            get(c, "complete").as_bool() == Some(true)
                && get(c, "ownerId") == get(scope, "ownerId")
                && get(c, "runId") == get(scope, "runId")
                && get(c, "tournamentId") == get(scope, "tournamentId")
                && get(c, "firstProofHash") == get(&p, "verificationHash")
                && hex(get(c, "sourceHash"))
                && get(c, "matchedLots").is_array()
                && instant(get(c, "at").as_str().unwrap_or(""))? >= instant(get(&p, "readAt").as_str().unwrap_or(""))?
                && instant(get(c, "at").as_str().unwrap_or(""))? <= at
                && at - instant(get(c, "at").as_str().unwrap_or(""))? <= 20000,
            "EXACT_PAIR_CENSUS",
        )?;
        need(
            root_receipts.as_array().is_some_and(|a| a.len() == 2),
            "TWO_ROOTS",
        )?;
        let mut roots = Vec::new();
        for x in root_receipts.as_array().unwrap() {
            need(
                num_id(get(x, "marketId")) && num_id(get(x, "exchangeId")),
                "ROOT_IDS",
            )?;
            let endpoint = format!(
                "/markets/{}/nodes?tournamentId={}",
                get(x, "marketId").as_str().unwrap_or(""),
                get(scope, "tournamentId").as_str().unwrap_or("")
            );
            let r = raw(get(x, "receipt"), &endpoint, at, 300000)?;
            let d = &r.data;
            need(
                get(d, "market_id") == get(x, "marketId")
                    && get(d, "contexts").as_array().is_some_and(|a| a.len() == 1)
                    && get(&get(d, "contexts")[0], "type").as_str() == Some("tournament")
                    && get(&get(&get(d, "contexts")[0], "tournament"), "id") == get(scope, "tournamentId"),
                "ROOT_SCOPE",
            )?;
            roots.push(json!({
                "marketId": get(x, "marketId"),
                "exchangeId": get(x, "exchangeId"),
                "option": "YES",
                "tournamentId": get(scope, "tournamentId"),
                "root": get(d, "root"),
                "rootHash": payload_hash(get(d, "root"))?,
                "sourceHash": r.hash,
                "requestStartedAt": get(get(x, "receipt"), "requestStartedAt"),
                "receivedAt": get(get(x, "receipt"), "receivedAt"),
                "knownAt": get(get(x, "receipt"), "receivedAt"),
            }));
        }
        let distinct_markets: std::collections::BTreeSet<String> =
            roots.iter().map(|r| baal_service_runtime::json_text::js_stringify(get(r, "marketId"))).collect();
        need(
            distinct_markets.len() == 2
                && roots.iter().any(|r| {
                    get(r, "marketId") == get(&p, "marketId") && get(r, "exchangeId") == get(&p, "exchangeId")
                }),
            "FIRST_ROOT_SCOPE",
        )?;
        let other = roots
            .iter()
            .find(|r| get(r, "exchangeId") != get(&p, "exchangeId") && get(r, "marketId") != get(&p, "marketId"))
            .cloned();
        need(other.is_some(), "OPPOSING_ROOT")?;
        let other = other.unwrap();
        let semantic = screen_party_winner_no_pairs(&json!({
            "universe": {
                "tournamentId": get(scope, "tournamentId"),
                "exchanges": roots.iter().map(|r| json!({
                    "marketId": get(r, "marketId"),
                    "exchangeId": get(r, "exchangeId"),
                    "option": get(r, "option"),
                })).collect::<Vec<_>>(),
            },
            "rootCaptures": roots,
            "quoteCaptures": [],
            "account": V::Null,
            "now": now,
        }))?;
        let pairs = get(&semantic, "pairs");
        let first_pair = get(pairs, "0");
        need(
            js_int(get(&semantic, "semanticPairCount")) == Some(1)
                && pairs.as_array().is_some_and(|a| !a.is_empty())
                && js_bool(get(&first_pair, "rootsCurrent"))
                && get(get(owned, "package"), "eventId") == get(&first_pair, "eventKey")
                && get(get(owned, "package"), "legs").as_array().is_some_and(|legs| {
                    legs.iter().any(|l| {
                        get(l, "marketId") == get(&other, "marketId")
                            && get(l, "exchangeId") == get(&other, "exchangeId")
                    })
                }),
            "SAME_ORIGINAL_PARTY_WINNER_PACKAGE",
        )?;
        let event_id = get(&first_pair, "eventKey").clone();

        let mk_binding = |exchange_id: &V| -> V {
            let mut b = scope.as_object().cloned().unwrap_or_default();
            b.insert("exchangeId".into(), exchange_id.clone());
            V::Object(b)
        };
        let first_current = crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account(&json!({
            "account": full_account(account, get(&p, "exchangeId").as_str().unwrap_or("")),
            "binding": mk_binding(get(&p, "exchangeId")),
            "readAt": now,
        }))?;
        let other_current = crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account(&json!({
            "account": full_account(account, get(&other, "exchangeId").as_str().unwrap_or("")),
            "binding": mk_binding(get(&other, "exchangeId")),
            "readAt": now,
        }))?;
        account_continuity(owned, &p, &first_current, scope, now)?;
        let (positions_data, positions_numbers) =
            parse_number_lexemes(get(get(account, "positions"), "bodyText").as_str().unwrap_or(""))?;
        for (i, v) in get(&positions_data, "positions")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let key = serde_json::to_string(&json!(["positions", i, "quantity"])).unwrap();
            let raw_q = positions_numbers.get(&key);
            let qty = bigint_v(get(v, "quantity"))?;
            let ok = match raw_q {
                Some(s) if s.len() <= 128 => {
                    let re_ok = {
                        let (sign, rest) = if let Some(r) = s.strip_prefix('-') { ("-", r) } else { ("", s.as_str()) };
                        let _ = sign;
                        let int_part = rest.split('.').next().unwrap_or("");
                        let frac_ok = match rest.split_once('.') {
                            Some((_, f)) => !f.is_empty() && f.bytes().all(|b| b == b'0'),
                            None => true,
                        };
                        !int_part.is_empty()
                            && int_part.bytes().all(|b| b.is_ascii_digit())
                            && (int_part == "0" || !int_part.starts_with('0'))
                            && frac_ok
                    };
                    re_ok && bigint_str(s.split('.').next().unwrap_or("")).map(|n| n == qty).unwrap_or(false)
                }
                _ => false,
            };
            need(ok, "EXACT_POSITION_INTEGER")?;
        }
        let mut counterpart: Option<V> = None;
        if !get(c, "counterpartOwned").is_null() {
            let cp = owned_proof(get(c, "counterpartOwned"), scope)?;
            need(
                get(&cp, "marketId") == get(&other, "marketId")
                    && get(&cp, "exchangeId") == get(&other, "exchangeId"),
                "ATTRIBUTED_COUNTERPART",
            )?;
            account_continuity(get(c, "counterpartOwned"), &cp, &other_current, scope, now)?;
            counterpart = Some(cp);
        } else {
            need(
                !get(&other_current, "positions").as_array().is_some_and(|rows| {
                    rows.iter().any(|v| {
                        get(v, "exchangeId") == get(&other, "exchangeId")
                            && get(v, "quantity").as_f64() != Some(0.0)
                    })
                }),
                "FOREIGN_COUNTERPART_HOLDING",
            )?;
        }
        let mut matched_first: Map<String, V> = Map::new();
        let mut matched_other: Map<String, V> = Map::new();
        let mut matched: i64 = 0;
        let mut assignment_ids = std::collections::BTreeSet::new();
        for m in get(c, "matchedLots").as_array().unwrap() {
            need(
                get(m, "assignmentId").as_str().is_some_and(|s| !s.is_empty())
                    && !assignment_ids.contains(get(m, "assignmentId").as_str().unwrap())
                    && get(m, "firstLotId").is_string()
                    && get(m, "counterpartLotId").is_string()
                    && js_int(get(m, "quantity")).is_some_and(|q| q > 0)
                    && counterpart.is_some(),
                "ATTRIBUTED_PAIR_ASSIGNMENT",
            )?;
            assignment_ids.insert(get(m, "assignmentId").as_str().unwrap().to_string());
            let first_lot = get(m, "firstLotId").as_str().unwrap_or("");
            let other_lot = get(m, "counterpartLotId").as_str().unwrap_or("");
            need(
                get(&p, "remainingLots").as_array().is_some_and(|ls| {
                    ls.iter().any(|l| get(l, "lotId").as_str() == Some(first_lot))
                }) && get(&counterpart.clone().unwrap(), "remainingLots")
                    .as_array()
                    .is_some_and(|ls| ls.iter().any(|l| get(l, "lotId").as_str() == Some(other_lot))),
                "MATCHED_ORIGINAL_LOTS",
            )?;
            let q = js_int(get(m, "quantity")).unwrap();
            let mf = matched_first
                .get(first_lot)
                .and_then(js_int)
                .unwrap_or(0);
            matched_first.insert(first_lot.to_string(), json!(mf + q));
            let mo = matched_other
                .get(other_lot)
                .and_then(js_int)
                .unwrap_or(0);
            matched_other.insert(other_lot.to_string(), json!(mo + q));
            matched += q;
        }
        for l in get(&p, "remainingLots").as_array().unwrap() {
            let lq = js_int(get(l, "quantity")).unwrap_or(0);
            let mf = matched_first
                .get(get(l, "lotId").as_str().unwrap_or(""))
                .and_then(js_int)
                .unwrap_or(0);
            need(mf <= lq, "FIRST_LOT_OVERPAIRED")?;
        }
        if let Some(cp) = &counterpart {
            for l in get(cp, "remainingLots").as_array().cloned().unwrap_or_default() {
                let lq = js_int(get(&l, "quantity")).unwrap_or(0);
                let mo = matched_other
                    .get(get(&l, "lotId").as_str().unwrap_or(""))
                    .and_then(js_int)
                    .unwrap_or(0);
                need(mo <= lq, "COUNTERPART_LOT_OVERPAIRED")?;
            }
            need(
                js_int(get(cp, "remainingOwnedQuantity")) == Some(matched),
                "EXISTING_UNMATCHED_COUNTERPART_REQUIRES_PAIR_RECONCILIATION",
            )?;
        }
        let unpaired: Vec<V> = get(&p, "remainingLots")
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                let mut o = l.as_object().cloned().unwrap_or_default();
                let lq = js_int(get(l, "quantity")).unwrap_or(0);
                let mf = matched_first
                    .get(get(l, "lotId").as_str().unwrap_or(""))
                    .and_then(js_int)
                    .unwrap_or(0);
                o.insert("quantity".into(), json!(lq - mf));
                V::Object(o)
            })
            .filter(|l| js_int(get(l, "quantity")).unwrap_or(0) > 0)
            .collect();
        let unpaired_q = js_int(get(&p, "remainingOwnedQuantity")).unwrap() - matched;
        need(unpaired_q > 0, "NO_UNPAIRED_SHARES")?;

        let f = get(funding, "record");
        let reservation_hash = get(f, "reservationHash").clone();
        let mut fund_body = f.as_object().cloned().unwrap_or_default();
        fund_body.remove("reservationHash");
        let receipt = get(funding, "receipt");
        need(
            js_int(get(f, "schemaVersion")) == Some(1)
                && get(f, "kind").as_str() == Some("baal_scaled_no_full_nominal_reservation_v1")
                && get(f, "ownerId") == get(scope, "ownerId")
                && get(f, "runId") == get(scope, "runId")
                && get(f, "tournamentId") == get(scope, "tournamentId")
                && get(f, "allocationHash") == get(g, "allocationHash")
                && hex(&reservation_hash)
                && payload_hash(&V::Object(fund_body))? == reservation_hash.as_str().unwrap_or("")
                && get(g, "controllerReservationHash") == &reservation_hash
                && get(f, "reservations").is_array()
                && get(f, "owned").is_array()
                && get(f, "unresolvedIntentIds").as_array().is_some_and(|a| a.is_empty()),
            "INSTALLED_ACTUAL_RESERVE",
        )?;
        need(
            get(receipt, "allocationHash") == get(g, "allocationHash")
                && get(receipt, "reservationHash") == &reservation_hash
                && get(receipt, "globalLedgerHash") == get(g, "globalLedgerHash")
                && hex(get(receipt, "globalLedgerHash"))
                && instant(get(receipt, "confirmedAt").as_str().unwrap_or(""))? <= at
                && at - instant(get(receipt, "confirmedAt").as_str().unwrap_or(""))? <= 20000,
            "FRESH_RESERVE_RECEIPT",
        )?;
        let exposure = format!(
            "scaled:{}:{}:hedge-for-{}",
            get(scope, "runId").as_str().unwrap_or(""),
            get(&other, "exchangeId").as_str().unwrap_or(""),
            get(&p, "exchangeId").as_str().unwrap_or("")
        );
        let reserve: Vec<V> = get(f, "reservations")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| {
                get(r, "marketId") == get(&other, "marketId")
                    && get(r, "eventId") == &event_id
                    && get(r, "exposureId").as_str() == Some(exposure.as_str())
            })
            .collect();
        need(
            reserve.len() == 1
                && amount(get(&reserve[0], "maximumNominalCost").as_str().unwrap_or(""))? >= 0
                && get(f, "owned").as_array().is_some_and(|rows| {
                    rows.iter().any(|r| {
                        get(r, "marketId") == get(&p, "marketId")
                            && get(r, "eventId") == &event_id
                            && get(r, "nominalCost") == get(&p, "remainingOwnedCost")
                    })
                }),
            "SOURCE_BOUND_HEDGE_RESERVATION",
        )?;
        let reserved = amount(get(&reserve[0], "maximumNominalCost").as_str().unwrap_or(""))?;
        let cash = minimum(&[
            amount(get(&first_current, "cashLower").as_str().unwrap_or(""))?,
            amount(get(&other_current, "cashLower").as_str().unwrap_or(""))?,
            amount(get(g, "cashLower").as_str().unwrap_or(""))?,
        ]);
        {
            let g_res = get(g, "reservations").as_array().cloned().unwrap_or_default();
            let mut exposure_ids = std::collections::BTreeSet::new();
            let mut all_funded = true;
            for r in &g_res {
                all_funded &= get(r, "exposureId").is_string()
                    && amount(get(r, "maximumNominalCost").as_str().unwrap_or(""))? >= 0
                    && get(r, "unknownAcceptance").as_bool() != Some(true);
                exposure_ids.insert(get(r, "exposureId").as_str().unwrap_or("").to_string());
            }
            let f_res = get(f, "reservations").as_array().cloned().unwrap_or_default();
            let mut f_in_g = true;
            for r in &f_res {
                let rh = payload_hash(r)?;
                f_in_g &= g_res.iter().try_fold(false, |acc, x| -> Result<bool, String> {
                    Ok(acc || payload_hash(x)? == rh)
                })?;
            }
            let f_sum: i128 = f_res
                .iter()
                .try_fold(0i128, |n, r| -> Result<i128, String> {
                    Ok(n + amount(get(r, "maximumNominalCost").as_str().unwrap_or(""))?)
                })?;
            let g_sum: i128 = g_res
                .iter()
                .try_fold(0i128, |n, r| -> Result<i128, String> {
                    Ok(n + amount(get(r, "maximumNominalCost").as_str().unwrap_or(""))?)
                })?;
            need(
                get(g, "reservations").is_array()
                    && all_funded
                    && exposure_ids.len() == g_res.len()
                    && f_in_g
                    && get(f, "cashReserved").as_str() == Some(money(f_sum).as_str())
                    && g_sum <= cash,
                "ALL_CURRENT_RESERVATIONS_CASH_FUNDED",
            )?;
        }
        need(
            instant(get(g, "at").as_str().unwrap_or(""))?
                >= instant(get(&first_current, "completedAt").as_str().unwrap_or(""))?
                    .max(instant(get(&other_current, "completedAt").as_str().unwrap_or(""))?),
            "RISK_AFTER_ACCOUNT",
        )?;
        let book_endpoint = format!(
            "/markets/{}/orderbook?tournamentId={}&depth=20",
            get(&other, "marketId").as_str().unwrap_or(""),
            get(scope, "tournamentId").as_str().unwrap_or("")
        );
        let b = raw(other_book_receipt, &book_endpoint, at, 20000)?;
        let d = &b.data;
        let mut causal_max = instant(get(&first_current, "completedAt").as_str().unwrap_or(""))?
            .max(instant(get(&other_current, "completedAt").as_str().unwrap_or(""))?)
            .max(instant(get(c, "at").as_str().unwrap_or(""))?)
            .max(instant(get(receipt, "confirmedAt").as_str().unwrap_or(""))?)
            .max(instant(get(g, "at").as_str().unwrap_or(""))?);
        for r in &roots {
            causal_max = causal_max.max(instant(get(r, "receivedAt").as_str().unwrap_or(""))?);
        }
        need(
            causal_max <= b.start
                && get(d, "contexts").as_array().is_some_and(|a| a.len() == 1)
                && get(&get(d, "contexts")[0], "type").as_str() == Some("tournament")
                && get(&get(&get(d, "contexts")[0], "tournament"), "id") == get(scope, "tournamentId"),
            "CAUSAL_DEPTH_SOURCE",
        )?;
        let book = normalize_book(
            d,
            get(&other, "marketId").as_str().unwrap_or(""),
            get(scope, "tournamentId").as_str().unwrap_or(""),
            get(other_book_receipt, "receivedAt").as_str().unwrap_or(""),
            get(other_book_receipt, "requestStartedAt").as_str().unwrap_or(""),
            20000,
            &std::collections::BTreeSet::new(),
            "authoritative_receipt_v1",
        )?;
        need(
            get(&book, "exchangeId") == get(&other, "exchangeId")
                && get(&book, "bids").as_array().is_some_and(|a| a.len() <= 20)
                && get(&book, "asks").as_array().is_some_and(|a| a.len() <= 20),
            "EXACT_COUNTERPART_DEPTH20",
        )?;
        let raw_bids = get(&get(&get(&get(d, "contexts")[0], "orderbook"), "exchanges")[0], "bids")
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut best: Option<V> = None;
        let mut best_qty: i128 = 0;
        let mut best_first_principal: i128 = 0;
        let mut best_limit: i128 = 0;
        let mut best_raw_no_ask: i128 = 0;
        let mut best_depth: i64 = 0;
        for level in get(&book, "bids").as_array().cloned().unwrap_or_default() {
            let level_price = price(get(&level, "price").as_str().unwrap_or(""))?;
            let idx = raw_bids.iter().position(|l| {
                // JS String(l.price): strings pass through, numbers get JS ToString.
                let raw_p = match get(l, "price") {
                    V::String(s) => s.clone(),
                    other_v => js_stringify(other_v),
                };
                price(&raw_p).map(|x| x == level_price).unwrap_or(false)
            });
            let idx = match idx {
                Some(i) => i,
                None => {
                    // findIndex -> -1 -> path miss -> lexeme undefined -> need fails
                    need(false, "EXACT_BOOK_PRICE_AND_QUANTITY")?;
                    unreachable!()
                }
            };
            let qkey = serde_json::to_string(&json!([
                "contexts", 0, "orderbook", "exchanges", 0, "bids", idx, "quantity"
            ]))
            .unwrap();
            let pkey = serde_json::to_string(&json!([
                "contexts", 0, "orderbook", "exchanges", 0, "bids", idx, "price"
            ]))
            .unwrap();
            let qlex = b.numbers.get(&qkey).cloned();
            let plex = b.numbers.get(&pkey).cloned();
            let level_qty = bigint_v(get(&level, "quantity"))?;
            let q_ok = match &qlex {
                Some(s) if s.len() <= 128 => {
                    let int_part = s.split('.').next().unwrap_or("");
                    let frac_ok = match s.split_once('.') {
                        Some((_, f)) => !f.is_empty() && f.bytes().all(|b2| b2 == b'0'),
                        None => true,
                    };
                    !int_part.is_empty()
                        && int_part.bytes().all(|b2| b2.is_ascii_digit())
                        && (int_part == "0" || !int_part.starts_with('0'))
                        && frac_ok
                        && bigint_str(int_part).map(|n| n == level_qty).unwrap_or(false)
                }
                _ => false,
            };
            need(q_ok, "EXACT_BOOK_PRICE_AND_QUANTITY")?;
            need(
                price(plex.as_deref().unwrap_or(""))? == level_price,
                "EXACT_BOOK_PRICE_AND_QUANTITY",
            )?;
            let raw_no_ask = SCALE - level_price;
            let limit = raw_no_ask + TICK;
            if limit > SCALE - TICK {
                continue;
            }
            let cap = minimum(&[
                unpaired_q as i128,
                100,
                (js_int(get(&level, "quantity")).unwrap_or(0) as i128) / 100,
                25000000 / limit,
                reserved / limit,
                cash / limit,
            ]);
            let mut quantity: i128 = 0;
            let mut first_principal: i128 = 0;
            let mut assigned = Vec::new();
            for l in &unpaired {
                if quantity >= cap {
                    break;
                }
                let basis = price(get(l, "entryPrice").as_str().unwrap_or(""))?;
                if basis + limit > 990000 {
                    break;
                }
                let n = (js_int(get(l, "quantity")).unwrap_or(0) as i128).min(cap - quantity);
                quantity += n;
                first_principal += basis * n;
                assigned.push(json!({
                    "firstLotId": get(l, "lotId"),
                    "quantity": n,
                    "entryPrice": get(l, "entryPrice"),
                    "firstPrincipal": money(basis * n),
                    "counterpartLimit": money(limit),
                    "protectedPairUnitCost": money(basis + limit),
                }));
            }
            if quantity > 0 {
                let qty_i = quantity;
                if best.is_none() || qty_i > best_qty {
                    best = Some(json!({
                        "quantity": qty_i,
                        "limit": limit,
                        "rawNoAsk": raw_no_ask,
                        "firstPrincipal": first_principal,
                        "assigned": assigned,
                        "level": level,
                        "depth": (js_int(get(&level, "quantity")).unwrap_or(0)) / 100,
                    }));
                    best_qty = qty_i;
                    best_first_principal = first_principal;
                    best_limit = limit;
                    best_raw_no_ask = raw_no_ask;
                    best_depth = js_int(get(&level, "quantity")).unwrap_or(0) / 100;
                }
            }
        }
        let _ = (&best_first_principal, &best_limit, &best_raw_no_ask, &best_depth);
        if best.is_none() {
            let mut o = base.as_object().cloned().unwrap_or_default();
            o.insert("reason".into(), json!("NO_FUNDED_INDIVIDUAL_DEPTH_AND_FIFO_PAIR_MARGIN"));
            o.insert("ownedProofHash".into(), get(&p, "verificationHash").clone());
            o.insert("unpairedQuantity".into(), json!(unpaired_q));
            o.insert("existingMatchedQuantity".into(), json!(matched));
            return Ok(V::Object(o));
        }
        let best = best.unwrap();
        let b_qty = js_int(get(&best, "quantity")).unwrap_or(0) as i128;
        let b_limit = bigint_v(get(&best, "limit"))?;
        let hedge_cost = b_limit * b_qty;
        let pair_cost = bigint_v(get(&best, "firstPrincipal"))? + hedge_cost;
        let floor = SCALE * b_qty;
        let mut expires_min = (b.start + 20000)
            .min(instant(get(&first_current, "startedAt").as_str().unwrap_or(""))? + 20000)
            .min(instant(get(&other_current, "startedAt").as_str().unwrap_or(""))? + 20000)
            .min(instant(get(receipt, "confirmedAt").as_str().unwrap_or(""))? + 20000);
        for r in &roots {
            expires_min = expires_min.min(instant(get(r, "receivedAt").as_str().unwrap_or(""))? + 300000);
        }
        let expires_at = iso(expires_min)?;
        need(instant(&expires_at)? > at, "PROPOSAL_EXPIRED")?;
        let mut proposal = base.as_object().cloned().unwrap_or_default();
        // {...base, proposalReady:true, ...} — insertion order follows JS literal.
        proposal.insert("proposalReady".into(), json!(true));
        proposal.insert("quantity".into(), get(&best, "quantity").clone());
        proposal.insert("reason".into(), json!("ATTRIBUTED_FIFO_CONDITIONAL_COUNTERPART_PROPOSAL"));
        proposal.insert("ownerId".into(), get(scope, "ownerId").clone());
        proposal.insert("runId".into(), get(scope, "runId").clone());
        proposal.insert("tournamentId".into(), get(scope, "tournamentId").clone());
        proposal.insert("eventId".into(), event_id);
        proposal.insert("firstMarketId".into(), get(&p, "marketId").clone());
        proposal.insert("firstExchangeId".into(), get(&p, "exchangeId").clone());
        proposal.insert("firstProofHash".into(), get(&p, "verificationHash").clone());
        proposal.insert("firstRemainingQuantity".into(), get(&p, "remainingOwnedQuantity").clone());
        proposal.insert("existingMatchedQuantity".into(), json!(matched));
        proposal.insert("unpairedQuantity".into(), json!(unpaired_q));
        proposal.insert("marketId".into(), get(&other, "marketId").clone());
        proposal.insert("exchangeId".into(), get(&other, "exchangeId").clone());
        proposal.insert("side".into(), json!("no"));
        proposal.insert("action".into(), json!("buy"));
        proposal.insert("protectedLimit".into(), json!(money(b_limit)));
        proposal.insert("rawObservedNoAsk".into(), json!(money(best_raw_no_ask)));
        proposal.insert("maximumHedgeNotional".into(), json!(money(hedge_cost)));
        proposal.insert("firstMatchedActualPrincipal".into(), json!(money(best_first_principal)));
        proposal.insert("protectedMatchedPairCost".into(), json!(money(pair_cost)));
        proposal.insert("conditionalOrdinaryMinimumPayout".into(), json!(money(floor)));
        proposal.insert("conditionalThirdPartyPayout".into(), json!(money(2 * floor)));
        proposal.insert("conditionalOrdinarySurplus".into(), json!(money(floor - pair_cost)));
        proposal.insert("administrativeZeroPayoutLoss".into(), json!(money(-pair_cost)));
        proposal.insert("unconditionalFloor".into(), V::Null);
        proposal.insert("fees".into(), json!("0.000000"));
        proposal.insert("guaranteedProfit".into(), json!(false));
        proposal.insert("matchedAssignmentsIfFilled".into(), get(&best, "assigned").clone());
        proposal.insert("originalPairCensusHash".into(), json!(payload_hash(c)?));
        proposal.insert("fundingReservationHash".into(), reservation_hash);
        proposal.insert("globalLedgerHash".into(), get(receipt, "globalLedgerHash").clone());
        proposal.insert("availableAttributedCounterpartReserve".into(), json!(money(reserved)));
        proposal.insert("onePercentIndividualLevelQuantity".into(), json!(best_depth));
        proposal.insert(
            "sourceBook".into(),
            json!({
                "sourceBodyHash": b.hash,
                "requestStartedAt": get(other_book_receipt, "requestStartedAt"),
                "receivedAt": get(other_book_receipt, "receivedAt"),
                "versionKey": get(&book, "versionKey"),
                "yesBidLevel": get(&best, "level"),
            }),
        );
        proposal.insert(
            "sourceRoots".into(),
            json!(roots
                .iter()
                .map(|r| json!({
                    "marketId": get(r, "marketId"),
                    "exchangeId": get(r, "exchangeId"),
                    "rootHash": get(r, "rootHash"),
                    "sourceHash": get(r, "sourceHash"),
                    "receivedAt": get(r, "receivedAt"),
                }))
                .collect::<Vec<_>>()),
        );
        proposal.insert(
            "currentAccountSourceHash".into(),
            json!(payload_hash(&json!([
                get(&first_current, "sourceReceipts"),
                get(&other_current, "sourceReceipts"),
            ]))?),
        );
        proposal.insert("globalRiskSourceHash".into(), get(g, "sourceHash").clone());
        proposal.insert("expiresAt".into(), json!(expires_at));
        proposal.insert("conditionalCarryNotRealizedProfit".into(), json!(true));
        proposal.insert("nativeNoBookObserved".into(), json!(false));
        proposal.insert("noLoanAdvanceDiscount".into(), json!(true));
        proposal.insert(
            "assumptions".into(),
            json!([
                "Ordinary same-race opposing Party Winner outcomes are mutually exclusive; a third-party win can pay both NO contracts.",
                "Void/refund/administrative or inconsistent resolution can pay zero; ordinary floor is not unconditional.",
                "New counterpart fills must be terminally verified and attributed before any pairing/reserve release; source inventory remains unchanged.",
                "A separately reviewed controller/journal revision and bounded activation are required; this pure proposal authorizes no order.",
            ]),
        );
        let proposal = V::Object(proposal);
        let hash = payload_hash(&proposal)?;
        let mut o = proposal.as_object().cloned().unwrap_or_default();
        o.insert("proposalHash".into(), json!(hash));
        Ok(V::Object(o))
    })();
    match out {
        Ok(v) => v,
        Err(e) => {
            let mut o = base_c.as_object().cloned().unwrap_or_default();
            let is_ours = e.starts_with("SCALED_OWNED_HEDGE_")
                && e["SCALED_OWNED_HEDGE_".len()..]
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b == b'_')
                && e.len() > "SCALED_OWNED_HEDGE_".len();
            o.insert("reason".into(), json!(if is_ours { e } else { "INVALID_OR_INCOMPLETE_OWNED_HEDGE_SOURCE".to_string() }));
            V::Object(o)
        }
    }
}
