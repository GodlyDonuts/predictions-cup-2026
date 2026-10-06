// execution/baal-scaled-entry-signal-v1.mjs — generic conditional
// relative-value package gap; no fair-probability estimate, reversion promise,
// hedge dispatch, client, timer or execution authority.
//
// Port notes:
// - The whole plan runs inside a try/catch: coded BAAL_SCALED_ENTRY_* errors
//   surface verbatim in `reason`; anything else collapses to
//   'INVALID_OR_INCOMPLETE_SCALED_ENTRY_EVIDENCE'.
// - JS BigInt micro arithmetic -> num_bigint::BigInt; BigInt division
//   truncates toward zero (matches num-bigint for i128-compatible ranges).
use crate::baal_capital_sizing_v1::baal_capital_ceilings;
use crate::core::payload_hash;
use crate::model::normalize_book;
use crate::no_rest_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::party_winner_no_screen_v1::screen_party_winner_no_pairs;
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("BAAL_SCALED_ENTRY_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty()
            && !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<u64>().is_ok_and(|n| n <= 9_007_199_254_740_991)
    })
}
fn slug(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|seg| {
            !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}
fn minimum(v: &[BigInt]) -> BigInt {
    v.iter().cloned().reduce(|a, b| if a < b { a } else { b }).unwrap_or_default()
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn path_key(path: &[V]) -> String {
    js_stringify(&V::Array(path.to_vec()))
}
// String(v) — JS primitive-to-string coercion.
fn js_string_repr(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(_) | V::Bool(_) => js_stringify(v),
        V::Null => "null".to_string(),
        _ => "undefined".to_string(),
    }
}
// BigInt(v) — JS throws RangeError on non-integral numbers, SyntaxError on
// non-integer strings, TypeError elsewhere; BigInt("") is 0n.
fn js_bigint(v: &V) -> Result<BigInt, String> {
    match v {
        V::Number(_) => js_int(v)
            .map(BigInt::from)
            .ok_or_else(|| "Cannot convert to a BigInt".to_string()),
        V::String(s) => {
            if s.is_empty() {
                return Ok(BigInt::from(0));
            }
            let body = s.strip_prefix('-').unwrap_or(s);
            if !body.is_empty() && body.bytes().all(|b| b.is_ascii_digit()) {
                Ok(s.parse::<BigInt>().unwrap_or_default())
            } else {
                Err("Cannot convert to a BigInt".to_string())
            }
        }
        V::Bool(b) => Ok(BigInt::from(*b as i64)),
        _ => Err("Cannot convert to a BigInt".to_string()),
    }
}

struct Raw {
    data: V,
    numbers: std::collections::BTreeMap<String, String>,
    start: i64,
    end: i64,
    source_body_hash: String,
    request_started_at: V,
    received_at: V,
}
// raw(receipt, endpoint, at, maxAge)
fn raw(receipt: &V, endpoint: &str, at: i64, max_age: i64) -> Result<Raw, String> {
    need(
        get(receipt, "method").as_str() == Some("GET")
            && get(receipt, "endpoint").as_str() == Some(endpoint)
            && get(receipt, "status") == &json!(200)
            && get(receipt, "bodyText").is_string()
            && get(receipt, "bodyText").as_str().unwrap_or("").len() <= 8 * 1024 * 1024,
        "RAW_RECEIPT",
    )?;
    let start = instant_v(get(receipt, "requestStartedAt"))?;
    let end = instant_v(get(receipt, "receivedAt"))?;
    need(
        start <= end && end <= at && at - start <= max_age && end - start <= 20000,
        "RECEIPT_TIME",
    )?;
    let source_body_hash = sha256(get(receipt, "bodyText").as_str().unwrap().as_bytes());
    // JS `r.sourceBodyHash===undefined || ===computed`: absent key skips.
    if let Some(h) = receipt.get("sourceBodyHash") {
        need(h.as_str() == Some(source_body_hash.as_str()), "RECEIPT_HASH")?;
    }
    let (data, numbers) = parse_number_lexemes(get(receipt, "bodyText").as_str().unwrap())?;
    Ok(Raw {
        data,
        numbers,
        start,
        end,
        source_body_hash,
        request_started_at: get(receipt, "requestStartedAt").clone(),
        received_at: get(receipt, "receivedAt").clone(),
    })
}

/// planBaalScaledEntry({allocation, scope, account, accountRisk,
/// rootReceipts, bookReceipts, now})
pub fn plan_baal_scaled_entry(args: &V) -> V {
    let scope = get(args, "scope");
    let allocation = get(args, "allocation");
    let account = get(args, "account");
    let account_risk = get(args, "accountRisk");
    let root_receipts = get(args, "rootReceipts");
    let book_receipts = get(args, "bookReceipts");
    let now = get(args, "now");
    let base = json!({
        "schemaVersion": 1,
        "mode": "baal_scaled_conditional_party_winner_entry_v1",
        "at": now,
        "proposalReady": false,
        "orderEligible": false,
        "executionAuthority": false,
        "networkRequests": 0,
        "competitionOrders": 0,
        "profitabilityEstablished": false,
        "priorOneShareTradeProvesScale": false,
    });
    let attempt: Result<V, String> = (|| {
        let at = instant_v(now)?;
        let a = allocation;
        let g = account_risk;
        need(
            get(scope, "ownerId").as_str() == Some("Codex")
                && get(scope, "runId").as_str().is_some_and(|s| !s.is_empty())
                && get(scope, "tournamentId").as_str().is_some_and(|s| !s.is_empty())
                && get(scope, "tournamentSlug").as_str().is_some_and(slug),
            "SCOPE",
        )?;
        let ceilings = baal_capital_ceilings();
        need(
            get(a, "schemaVersion") == &json!(1)
                && get(a, "ownerId") == get(scope, "ownerId")
                && get(a, "runId") == get(scope, "runId")
                && get(a, "tournamentId") == get(scope, "tournamentId")
                && get(a, "capital") == get(&ceilings, "capital")
                && hex(get(a, "humanAllocationSourceHash"))
                && instant_v(get(a, "recordedAt"))? <= at,
            "HUMAN_ALLOCATION",
        )?;
        let order_cap = amount(
            a.get("maximumOrderCost")
                .filter(|v| !v.is_null())
                .unwrap_or(&json!("25.000000"))
                .as_str()
                .unwrap_or(""),
        )
        .map(BigInt::from)?;
        let market_cap = amount(
            a.get("maximumMarketCost")
                .filter(|v| !v.is_null())
                .unwrap_or(&json!("100.000000"))
                .as_str()
                .unwrap_or(""),
        )
        .map(BigInt::from)?;
        let event_cap = amount(
            a.get("maximumEventCost")
                .filter(|v| !v.is_null())
                .unwrap_or(&json!("200.000000"))
                .as_str()
                .unwrap_or(""),
        )
        .map(BigInt::from)?;
        let share_cap = a
            .get("maximumSharesPerOrder")
            .filter(|v| !v.is_null())
            .cloned()
            .unwrap_or(json!(100));
        need(
            order_cap > BigInt::from(0)
                && order_cap <= BigInt::from(25_000_000i64)
                && market_cap > BigInt::from(0)
                && market_cap <= BigInt::from(100_000_000i64)
                && event_cap > BigInt::from(0)
                && event_cap <= BigInt::from(200_000_000i64)
                && js_int(&share_cap).is_some_and(|n| n >= 1 && n <= 100),
            "CAPITAL_CEILINGS",
        )?;
        need(
            get(g, "ownerId") == get(scope, "ownerId")
                && get(g, "runId") == get(scope, "runId")
                && get(g, "tournamentId") == get(scope, "tournamentId")
                && get(g, "complete") == &json!(true)
                && get(g, "eventScopeComplete") == &json!(true)
                && get(g, "unknownAcceptance") == &json!(false)
                && get(g, "unresolvedIntentIds").as_array().is_some_and(|x| x.is_empty())
                && get(g, "stopped") == &json!(false)
                && get(g, "soleCollector") == &json!(true)
                && hex(get(g, "sourceHash"))
                && instant_v(get(g, "at"))? <= at
                && at - instant_v(get(g, "at"))? <= 20000
                && ["owned", "reservations", "loans"]
                    .iter()
                    .all(|k| get(g, k).is_array()),
            "FRESH_RISK_OR_UNKNOWN",
        )?;
        need(
            root_receipts.is_array()
                && root_receipts.as_array().unwrap().len() == 2
                && book_receipts.is_array()
                && book_receipts.as_array().unwrap().len() == 2,
            "TWO_ROOTS_AND_BOOKS",
        )?;
        let mut roots: Vec<V> = Vec::new();
        let mut root_raws: Vec<Raw> = Vec::new();
        for c in root_receipts.as_array().unwrap() {
            need(id(get(c, "marketId")) && id(get(c, "exchangeId")), "ROOT_MEMBER")?;
            let r = raw(
                get(c, "receipt"),
                &format!(
                    "/markets/{}/nodes?tournamentId={}",
                    get(c, "marketId").as_str().unwrap_or(""),
                    get(scope, "tournamentId").as_str().unwrap_or("")
                ),
                at,
                300000,
            )?;
            let d = r.data.clone();
            need(
                get(&d, "market_id") == get(c, "marketId")
                    && get(&d, "contexts").as_array().map(|x| x.len()) == Some(1)
                    && get(&get(&d, "contexts")[0], "type").as_str() == Some("tournament")
                    && get(&get(&get(&d, "contexts")[0], "tournament"), "id") == get(scope, "tournamentId"),
                "ROOT_SCOPE",
            )?;
            let root = json!({
                "marketId": get(c, "marketId"),
                "exchangeId": get(c, "exchangeId"),
                "tournamentId": get(scope, "tournamentId"),
                "option": "YES",
                "root": get(&d, "root"),
                "rootHash": payload_hash(get(&d, "root"))?,
                "sourceHash": r.source_body_hash,
                "requestStartedAt": r.request_started_at,
                "receivedAt": r.received_at,
                "knownAt": r.received_at,
            });
            roots.push(root);
            root_raws.push(r);
        }
        need(
            get(&roots[0], "marketId") != get(&roots[1], "marketId")
                && get(&roots[0], "exchangeId") != get(&roots[1], "exchangeId"),
            "DISTINCT_CONTRACTS",
        )?;
        let semantic = screen_party_winner_no_pairs(&json!({
            "universe": {
                "tournamentId": get(scope, "tournamentId"),
                "exchanges": roots.iter().map(|r| json!({
                    "marketId": get(r, "marketId"),
                    "exchangeId": get(r, "exchangeId"),
                    "option": get(r, "option"),
                })).collect::<Vec<V>>(),
            },
            "rootCaptures": roots,
            "quoteCaptures": [],
            "account": V::Null,
            "now": now,
        }))?;
        need(
            get(&semantic, "semanticPairCount") == &json!(1)
                && get(&get(&semantic, "pairs")[0], "rootsCurrent") == &json!(true),
            "OPPOSING_SAME_SINGLE_WINNER_RACE",
        )?;
        let event_id = get(&get(&semantic, "pairs")[0], "eventKey").clone();

        struct Leg {
            market_id: String,
            exchange_id: String,
            raw_no_ask: i128,
            limit: i128,
            depth: i64,
            source: Raw,
            book: V,
            yes_bid_level: V,
        }
        let mut legs: Vec<Leg> = Vec::new();
        for c in book_receipts.as_array().unwrap() {
            need(
                roots.iter().any(|r| {
                    get(r, "marketId") == get(c, "marketId") && get(r, "exchangeId") == get(c, "exchangeId")
                }),
                "BOOK_MEMBER",
            )?;
            let r = raw(
                get(c, "receipt"),
                &format!(
                    "/markets/{}/orderbook?tournamentId={}&depth=20",
                    get(c, "marketId").as_str().unwrap_or(""),
                    get(scope, "tournamentId").as_str().unwrap_or("")
                ),
                at,
                20000,
            )?;
            let d = r.data.clone();
            need(
                get(&d, "contexts").as_array().map(|x| x.len()) == Some(1)
                    && get(&get(&d, "contexts")[0], "type").as_str() == Some("tournament")
                    && get(&get(&get(&d, "contexts")[0], "tournament"), "id") == get(scope, "tournamentId"),
                "RULES_BEFORE_EXACT_BOOK",
            )?;
            for v in &roots {
                need(
                    instant_v(get(v, "receivedAt"))? <= r.start,
                    "RULES_BEFORE_EXACT_BOOK",
                )?;
            }
            let b = normalize_book(
                &d,
                get(c, "marketId").as_str().unwrap_or(""),
                get(scope, "tournamentId").as_str().unwrap_or(""),
                r.received_at.as_str().unwrap_or(""),
                r.request_started_at.as_str().unwrap_or(""),
                20000,
                &BTreeSet::new(),
                "authoritative_receipt_v1",
            )?;
            need(
                get(&b, "exchangeId") == get(c, "exchangeId")
                    && get(&b, "bids").as_array().map(|x| x.len() <= 20).unwrap_or(false)
                    && get(&b, "asks").as_array().map(|x| x.len() <= 20).unwrap_or(false),
                "BOOK_DEPTH20",
            )?;
            let bids = get(&get(&get(&d, "contexts")[0], "orderbook"), "exchanges")[0]
                .get("bids")
                .cloned()
                .unwrap_or(V::Null);
            for (i, l) in bids.as_array().cloned().unwrap_or_default().iter().enumerate() {
                let q = r.numbers.get(&path_key(&[
                    json!("contexts"), json!(0), json!("orderbook"), json!("exchanges"),
                    json!(0), json!("bids"), json!(i), json!("quantity"),
                ]));
                let p = r.numbers.get(&path_key(&[
                    json!("contexts"), json!(0), json!("orderbook"), json!("exchanges"),
                    json!(0), json!("bids"), json!(i), json!("price"),
                ]));
                let q_ok = q.is_some_and(|s| {
                    s.len() <= 128 && {
                        let mut parts = s.splitn(2, '.');
                        let whole = parts.next().unwrap_or("");
                        let frac = parts.next();
                        (whole == "0" || (!whole.is_empty() && !whole.starts_with('0')))
                            && whole.bytes().all(|x| x.is_ascii_digit())
                            && match frac {
                                None => true,
                                Some(f) => !f.is_empty() && f.bytes().all(|x| x == b'0'),
                            }
                    }
                }) && Some(
                    q.and_then(|s| s.split('.').next())
                        .and_then(|s| s.parse::<BigInt>().ok())
                        .unwrap_or_else(|| BigInt::from(i64::MIN)),
                ) == Some(js_bigint(get(l, "quantity"))?);
                // price(p) === price(String(l.price)) — JS BigInt()/price()
                // throws propagate uncoded into the catch below.
                let p_ok = match p {
                    Some(s) => {
                        let lhs = price(s)?;
                        let rhs = price(&js_string_repr(get(l, "price")))?;
                        lhs == rhs
                    }
                    None => {
                        // JS price(undefined) throws — an uncoded failure.
                        return Err("Cannot read price".to_string());
                    }
                };
                need(q_ok && p_ok, "EXACT_DEPTH_AND_PRICE_LEXEMES")?;
            }
            let level = get(&b, "bids")
                .as_array()
                .and_then(|ls| {
                    ls.iter()
                        .find(|l| js_int(get(l, "quantity")).map(|q| q / 100 >= 1).unwrap_or(false))
                })
                .cloned();
            need(level.is_some(), "NO_INDIVIDUAL_ONE_PERCENT_DEPTH")?;
            let level = level.unwrap();
            let raw_no_ask = SCALE - price(get(&level, "price").as_str().unwrap_or(""))?;
            let limit = raw_no_ask + TICK;
            price(&money(limit))?;
            legs.push(Leg {
                market_id: get(c, "marketId").as_str().unwrap_or("").to_string(),
                exchange_id: get(c, "exchangeId").as_str().unwrap_or("").to_string(),
                raw_no_ask,
                limit,
                depth: js_int(get(&level, "quantity")).unwrap_or(0) / 100,
                source: r,
                book: b,
                yes_bid_level: level,
            });
        }
        need(
            legs[0].exchange_id != legs[1].exchange_id
                && legs.iter().map(|l| l.source.end).max().unwrap_or(0)
                    - legs.iter().map(|l| l.source.start).min().unwrap_or(0)
                    <= 10000,
            "COFRESH_DISTINCT_BOOKS",
        )?;
        legs.sort_by(|x, y| {
            x.limit
                .cmp(&y.limit)
                .then(
                    x.exchange_id
                        .parse::<i64>()
                        .unwrap_or(0)
                        .cmp(&y.exchange_id.parse::<i64>().unwrap_or(0)),
                )
        });
        let (first, second) = (&legs[0], &legs[1]);
        let total = first.limit + second.limit;
        need(total <= 990000, "MINIMUM_PROTECTIVE_PACKAGE_GAP")?;

        let scoped = get(account, "scopedOrdersPagesByExchange")
            .get(&first.exchange_id)
            .filter(|v| !v.is_null())
            .cloned()
            .unwrap_or_else(|| get(account, "scopedOrdersPages").clone());
        let full = json!({
            "cash": get(account, "cash"),
            "positions": get(account, "positions"),
            "openOrdersPages": get(account, "openOrdersPages"),
            "scopedOrdersPages": scoped,
        });
        let mut binding = scope.clone();
        binding
            .as_object_mut()
            .map(|m| m.insert("exchangeId".into(), json!(first.exchange_id)));
        let current = inspect_scoped_rest_no_account(&json!({
            "account": full,
            "binding": binding,
            "readAt": now,
        }))?;
        let (raw_positions, pos_numbers) =
            parse_number_lexemes(get(get(account, "positions"), "bodyText").as_str().unwrap_or(""))?;
        for (i, p) in get(&raw_positions, "positions")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let q = pos_numbers.get(&path_key(&[json!("positions"), json!(i), json!("quantity")]));
            let q_ok = q.is_some_and(|s| {
                s.len() <= 128 && {
                    let body = s.strip_prefix('-').unwrap_or(s);
                    let mut parts = body.splitn(2, '.');
                    let whole = parts.next().unwrap_or("");
                    let frac = parts.next();
                    (whole == "0" || (!whole.is_empty() && !whole.starts_with('0')))
                        && whole.bytes().all(|x| x.is_ascii_digit())
                        && match frac {
                            None => true,
                            Some(f) => !f.is_empty() && f.bytes().all(|x| x == b'0'),
                        }
                }
            }) && q
                .and_then(|s| s.split('.').next())
                .and_then(|s| s.parse::<BigInt>().ok())
                == js_int(get(p, "quantity")).map(BigInt::from);
            need(q_ok, "EXACT_ACCOUNT_POSITION_QUANTITY")?;
        }
        let min_book_start = legs.iter().map(|l| l.source.start).min().unwrap_or(0);
        need(
            !get(&current, "positions")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .any(|p| {
                    legs.iter().any(|l| get(p, "exchangeId").as_str() == Some(l.exchange_id.as_str()))
                        && get(p, "quantity") != &json!(0)
                })
                && instant_v(get(&current, "completedAt"))? <= min_book_start
                && instant_v(get(g, "at"))? >= instant_v(get(&current, "completedAt"))?
                && instant_v(get(g, "at"))? <= min_book_start,
            "WHOLE_ACCOUNT_TARGET_FLAT_AND_CAUSAL",
        )?;
        let mut nominal = BigInt::from(0);
        let mut event_exposure = BigInt::from(0);
        let mut markets: std::collections::BTreeMap<String, BigInt> = legs
            .iter()
            .map(|l| (l.market_id.clone(), BigInt::from(0)))
            .collect();
        for (k, field) in [
            ("owned", "nominalCost"),
            ("reservations", "maximumNominalCost"),
            ("loans", "fullNominalPrincipal"),
        ] {
            for e in get(g, k).as_array().cloned().unwrap_or_default() {
                need(
                    e.is_object()
                        && id(get(&e, "marketId"))
                        && get(&e, "eventId").as_str().is_some_and(|s| !s.is_empty())
                        && get(&e, "exposureId").as_str().is_some_and(|s| !s.is_empty())
                        && get(&e, "unknownAcceptance") != &json!(true),
                    "EXACT_NOMINAL_EXPOSURE",
                )?;
                let n = amount(get(&e, field).as_str().unwrap_or("")).map(BigInt::from)?;
                let market_id = get(&e, "marketId").as_str().unwrap_or("").to_string();
                need(
                    n >= BigInt::from(0)
                        && (!markets.contains_key(&market_id) || get(&e, "eventId") == &event_id),
                    "EVENT_SCOPE",
                )?;
                nominal += &n;
                if get(&e, "eventId") == &event_id {
                    event_exposure += &n;
                }
                if let Some(m) = markets.get_mut(&market_id) {
                    *m += &n;
                }
            }
        }
        let cash = minimum(&[
            amount(get(&current, "cashLower").as_str().unwrap_or("")).map(BigInt::from)?,
            match g.get("cashLower") {
                None => amount(get(&current, "cashLower").as_str().unwrap_or("")).map(BigInt::from)?,
                Some(v) => amount(v.as_str().unwrap_or("")).map(BigInt::from)?,
            },
        ]);
        let capital_remaining = BigInt::from(1_000_000_000i64) - &nominal;
        let event_remaining = &event_cap - &event_exposure;
        need(cash >= BigInt::from(0), "CASH_LOWER")?;
        let first_remaining = &market_cap - markets.get(&first.market_id).cloned().unwrap_or_default();
        let second_remaining = &market_cap - markets.get(&second.market_id).cloned().unwrap_or_default();
        let q990 = BigInt::from(990_000i64);
        let capacities = vec![
            BigInt::from(js_int(&share_cap).unwrap_or(0)),
            BigInt::from(first.depth),
            BigInt::from(second.depth),
            &capital_remaining / &q990,
            &event_remaining / &q990,
            &cash / &q990,
            &order_cap / BigInt::from(first.limit),
            &order_cap / BigInt::from(second.limit),
            &first_remaining / BigInt::from(first.limit),
            &second_remaining / BigInt::from(second.limit),
        ];
        let quantity: i64 = minimum(&capacities).try_into().unwrap_or(0);
        let risk = json!({
            "nominalExposure": money_big(&nominal),
            "eventNominalExposure": money_big(&event_exposure),
            "capitalRemaining": money_big(&capital_remaining),
            "eventRemaining": money_big(&event_remaining),
            "firstMarketRemaining": money_big(&first_remaining),
            "secondMarketRemaining": money_big(&second_remaining),
            "cashLower": money_big(&cash),
            "fullNominalLoansAndReservationsCounted": true,
            "loanAdvanceDiscount": false,
        });
        if quantity < 1 {
            let mut out = base.clone();
            let m = out.as_object_mut().unwrap();
            m.insert("reason".into(), json!("NO_FULL_PACKAGE_FUNDED_INTEGER_QUANTITY"));
            m.insert("risk".into(), risk);
            return Ok(out);
        }
        let expires_at = ms_to_iso(
            (roots
                .iter()
                .map(|r| instant_v(get(r, "receivedAt")).unwrap_or(i64::MAX) + 300000)
                .chain(legs.iter().map(|l| l.source.start + 20000))
                .chain(std::iter::once(instant_v(get(&current, "startedAt"))? + 20000))
                .min()
                .unwrap_or(0)) as f64,
        );
        need(instant(&expires_at)? > at, "EXPIRED_ENTRY_SOURCE")?;
        let public_leg = |l: &Leg| -> V {
            json!({
                "marketId": l.market_id,
                "exchangeId": l.exchange_id,
                "side": "no",
                "limit": money(l.limit),
                "rawObservedNoAsk": money(l.raw_no_ask),
                "maximumQuantity": quantity,
                "onePercentIndividualLevelQuantity": l.depth,
                "yesBidSourceLevel": l.yes_bid_level,
                "maximumObservedNotional": money_big(&(BigInt::from(l.limit) * BigInt::from(quantity))),
                "bookVersion": get(&l.book, "versionKey"),
            })
        };
        let residual = json!({
            "kind": "conditional_two_no_relative_value_gap",
            "eventId": event_id,
            "protectiveUnitCost": money_big(&BigInt::from(total)),
            "ordinaryPayoutFloor": "1.000000",
            "conditionalGap": money_big(&BigInt::from(SCALE - total)),
            "rootHashes": roots.iter().map(|r| get(r, "rootHash").clone()).collect::<Vec<V>>(),
            "bookHashes": legs.iter().map(|l| json!(l.source.source_body_hash)).collect::<Vec<V>>(),
        });
        let mut proposal = base.clone();
        {
            let m = proposal.as_object_mut().unwrap();
            m.insert("proposalReady".into(), json!(true));
            m.insert(
                "reason".into(),
                json!("FUNDED_CONDITIONAL_PACKAGE_GAP_CHEAP_FIRST_PROPOSAL"),
            );
            m.insert("ownerId".into(), get(scope, "ownerId").clone());
            m.insert("runId".into(), get(scope, "runId").clone());
            m.insert("tournamentId".into(), get(scope, "tournamentId").clone());
            m.insert("tournamentSlug".into(), get(scope, "tournamentSlug").clone());
            m.insert("eventId".into(), event_id.clone());
            m.insert("allocationHash".into(), json!(payload_hash(a)?));
            m.insert("riskHash".into(), json!(payload_hash(g)?));
            m.insert(
                "legs".into(),
                V::Array(legs.iter().map(public_leg).collect()),
            );
            m.insert(
                "chosenLeg".into(),
                json!({
                    "marketId": first.market_id,
                    "exchangeId": first.exchange_id,
                    "side": "no",
                    "action": "buy",
                    "quantity": quantity,
                    "limit": money(first.limit),
                }),
            );
            m.insert(
                "fullPackageReservation".into(),
                json!(money_big(&(BigInt::from(990_000i64) * BigInt::from(quantity)))),
            );
            m.insert("unitPackageCeiling".into(), json!("0.990000"));
            m.insert("minimumOrdinaryMargin".into(), json!("0.010000"));
            m.insert(
                "observedProtectivePackageUnitCost".into(),
                json!(money_big(&BigInt::from(total))),
            );
            m.insert(
                "conditionalOrdinaryGap".into(),
                json!(money_big(&BigInt::from(SCALE - total))),
            );
            m.insert("conditionalOrdinaryFloor".into(), json!("1.000000"));
            m.insert("unconditionalFloor".into(), V::Null);
            m.insert(
                "exceptionLossAtFullReservation".into(),
                json!(money_big(&(BigInt::from(-990_000i64) * BigInt::from(quantity)))),
            );
            m.insert(
                "maximumFirstOrderNotional".into(),
                json!(money_big(&(BigInt::from(first.limit) * BigInt::from(quantity)))),
            );
            m.insert(
                "maximumFutureHedgeOrderNotional".into(),
                json!(money_big(&minimum(&[order_cap.clone(), second_remaining.clone()]))),
            );
            m.insert(
                "futureHedgeLimitRule".into(),
                json!("min(0.990-actual_first_unit_cost, tick_floor(max_order_notional/remaining_quantity), tick_floor(fresh_market_remaining/remaining_quantity))"),
            );
            m.insert("futureHedgeAutomaticallyAuthorized".into(), json!(false));
            m.insert(
                "sellTargetRule".into(),
                json!("actual_verified_unit_cost + 0.005 with fresh protected net exit/depth and separate authority"),
            );
            m.insert("valuationKind".into(), get(&residual, "kind").clone());
            m.insert("valuationHash".into(), json!(payload_hash(&residual)?));
            m.insert("risk".into(), risk.clone());
            m.insert("expiresAt".into(), json!(expires_at));
            m.insert(
                "sourceReceipts".into(),
                json!({
                    "roots": roots.iter().map(|r| json!({
                        "marketId": get(r, "marketId"),
                        "exchangeId": get(r, "exchangeId"),
                        "rootHash": get(r, "rootHash"),
                        "sourceBodyHash": get(r, "sourceHash"),
                        "requestStartedAt": get(r, "requestStartedAt"),
                        "receivedAt": get(r, "receivedAt"),
                    })).collect::<Vec<V>>(),
                    "books": legs.iter().map(|l| json!({
                        "marketId": l.market_id,
                        "exchangeId": l.exchange_id,
                        "sourceBodyHash": l.source.source_body_hash,
                        "requestStartedAt": l.source.request_started_at,
                        "receivedAt": l.source.received_at,
                        "versionKey": get(&l.book, "versionKey"),
                    })).collect::<Vec<V>>(),
                    "account": get(&current, "sourceReceipts"),
                }),
            );
            m.insert("fees".into(), json!("0.000000"));
            m.insert("depthCombinedAcrossLevels".into(), json!(false));
            m.insert("minimumFirstLotCorpusProfitabilityProven".into(), json!(false));
            m.insert(
                "assumptions".into(),
                json!([
                    "Opposing same-race Party Winner NO/NO has only a conditional ordinary-resolution floor; separate contracts are not exhaustive complements.",
                    "The first leg is directional until a independently funded/verified hedge fills; cheap is relative to the package, not a fair point probability.",
                    "Administrative, void/refund, affiliation or inconsistent-resolution exceptions can lose the full package cost.",
                    "Observed gaps need not revert on a short horizon; one original profitable share does not prove scaled profitability.",
                    "No entry window, order, automatic hedge or guaranteed sale is authorized by this proposal.",
                ]),
            );
        }
        let hash = payload_hash(&proposal)?;
        proposal
            .as_object_mut()
            .map(|m: &mut Map<String, V>| m.insert("proposalHash".into(), json!(hash)));
        Ok(proposal)
    })();
    match attempt {
        Ok(v) => v,
        Err(e) => {
            // /^BAAL_SCALED_ENTRY_[A-Z_]+$/.test(e.message) ? e.message : fallback
            let prefixed = e.starts_with("BAAL_SCALED_ENTRY_")
                && e.len() > "BAAL_SCALED_ENTRY_".len()
                && e["BAAL_SCALED_ENTRY_".len()..]
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b == b'_');
            let reason = if prefixed {
                e
            } else {
                "INVALID_OR_INCOMPLETE_SCALED_ENTRY_EVIDENCE".to_string()
            };
            let mut out = base;
            out.as_object_mut()
                .map(|m| m.insert("reason".into(), json!(reason)));
            out
        }
    }
}

fn money_big(x: &BigInt) -> String {
    baal_paper::fixed::money(x)
}
