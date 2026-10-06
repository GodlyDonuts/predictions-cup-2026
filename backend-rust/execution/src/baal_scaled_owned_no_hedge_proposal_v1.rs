// execution/baal-scaled-owned-no-hedge-proposal-v1.mjs — pure attributed
// counterpart proposal, never a hedge order/activation or a claim of cash
// profit. Each paired first-leg unit retains its actual basis.
//
// Port notes:
// - JS BigInt money -> i128 micro units via baal_service_runtime::fixed.
// - raw(r,endpoint,at,age): JS returns {...parseNumberLexemes(body), hash,
//   start, end}; here a RawOut struct.
// - `sourceBodyHash === undefined` means the key is ABSENT (explicit null does
//   not pass in JS) — checked via contains_key.
// - JS Map insertion order is never observed for matchedFirst/matchedOther
//   (lookups only) -> HashMap; assignmentIds -> BTreeSet.
// - Errors thrown inside .every/.map/.findIndex propagate raw -> the catch
//   fallback, mirroring JS; only SCALED_OWNED_HEDGE_[A-Z_]+ codes surface.
use crate::core::payload_hash;
use crate::exact_money_receipts::parse_number_lexemes;
use crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account;
use crate::party_winner_no_screen_v1::screen_party_winner_no_pairs;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use baal_service_runtime::market::normalize_book;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("SCALED_OWNED_HEDGE_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
// /^[1-9]\d*$/ and Number.isSafeInteger(Number(s)).
fn id(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty()
            && !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n <= 9_007_199_254_740_991)
    })
}
fn sha(s: &str) -> String {
    sha256(s.as_bytes())
}
fn minimum(v: &[i128]) -> i128 {
    *v.iter().min().unwrap_or(&0)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().ok_or_else(|| "INVALID_INSTANT".to_string())?)
}
fn sort_by_id(a: &[V]) -> Vec<V> {
    let mut x: Vec<V> = a.to_vec();
    x.sort_by(|p, q| {
        get(p, "id")
            .as_f64()
            .partial_cmp(&get(q, "id").as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    x
}
// String(value): JS ToString (numbers via shortest-roundtrip).
fn js_string(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Null => "null".to_string(),
        V::Bool(b) => b.to_string(),
        V::Number(_) => js_stringify(v),
        V::Object(_) => "[object Object]".to_string(),
        V::Array(a) => a
            .iter()
            .map(|x| match x {
                V::Null => String::new(),
                other => js_string(other),
            })
            .collect::<Vec<String>>()
            .join(","),
    }
}
// JS falsy: undefined/null/false/0/"" (V only holds JSON).
fn js_falsy(v: &V) -> bool {
    match v {
        V::Null => true,
        V::Bool(b) => !*b,
        V::Number(n) => n.as_f64() == Some(0.0),
        V::String(s) => s.is_empty(),
        _ => false,
    }
}
// /[a-z0-9]+(?:-[a-z0-9]+)*/ anchored slug check.
fn slug_ok(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    !parts.is_empty()
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}
// Lexeme regexes — two distinct forms appear in the JS: signed
// /^-?(0|[1-9]\d*)(\.0+)?$/ (positions) and unsigned /^(0|[1-9]\d*)(\.0+)?$/
// (book depth).
fn int_lexeme_ok(x: &str, allow_sign: bool) -> bool {
    let t = if allow_sign {
        x.strip_prefix('-').unwrap_or(x)
    } else {
        x
    };
    let (w, f) = match t.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (t, None),
    };
    !w.is_empty()
        && (w == "0" || !w.starts_with('0'))
        && w.bytes().all(|b| b.is_ascii_digit())
        && f.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b == b'0'))
}

struct RawOut {
    data: V,
    numbers: BTreeMap<String, String>,
    hash: String,
    start: i64,
    #[allow(dead_code)]
    end: i64,
}

fn raw(r: &V, endpoint: &str, at: i64, age: i64) -> Result<RawOut, String> {
    need(
        get(r, "method").as_str() == Some("GET")
            && js_int(get(r, "status")) == Some(200)
            && get(r, "endpoint").as_str() == Some(endpoint)
            && get(r, "bodyText").as_str().is_some_and(|s| s.len() <= 8 * 1024 * 1024),
        "RAW_RECEIPT",
    )?;
    let start = instant_v(get(r, "requestStartedAt"))?;
    let end = instant_v(get(r, "receivedAt"))?;
    need(
        start <= end && end <= at && at - start <= age && end - start <= 20000,
        "FRESH_SOURCE",
    )?;
    let hash = sha(get(r, "bodyText").as_str().unwrap_or(""));
    // r.sourceBodyHash === undefined || r.sourceBodyHash === hash — undefined
    // means the key is absent; explicit null does not satisfy it.
    need(
        !r.as_object().map(|m| m.contains_key("sourceBodyHash")).unwrap_or(false)
            || get(r, "sourceBodyHash").as_str() == Some(hash.as_str()),
        "SOURCE_HASH",
    )?;
    let (data, numbers) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
    Ok(RawOut {
        data,
        numbers,
        hash,
        start,
        end,
    })
}

fn owned_proof(owned: &V, scope: &V) -> Result<V, String> {
    let p = get(owned, "proof").clone();
    let verification_hash = get(&p, "verificationHash").clone();
    let mut body = p.as_object().cloned().unwrap_or_default();
    body.shift_remove("verificationHash");
    need(
        hex(&verification_hash)
            && payload_hash(&V::Object(body))? == verification_hash.as_str().unwrap_or("")
            && get(&p, "mode").as_str() == Some("scoped_rest_no_multishare_fifo_v1")
            && get(&p, "scopedTradeVerified") == &V::Bool(true)
            && get(&p, "side").as_str() == Some("no")
            && get(&p, "ownerId") == get(scope, "ownerId")
            && get(&p, "runId") == get(scope, "runId")
            && get(&p, "tournamentId") == get(scope, "tournamentId")
            && id(get(&p, "marketId"))
            && id(get(&p, "exchangeId"))
            && js_int(get(&p, "remainingOwnedQuantity")).is_some_and(|q| q > 0)
            && get(&p, "remainingLots").is_array(),
        "ACTUAL_OWNED_PROOF",
    )?;
    let mut q: i64 = 0;
    let mut cost: i128 = 0;
    let mut lots: BTreeSet<String> = BTreeSet::new();
    for l in get(&p, "remainingLots").as_array().cloned().unwrap_or_default() {
        let lot_id = sget(&l, "lotId").unwrap_or("").to_string();
        let qty = js_int(get(&l, "quantity")).unwrap_or(i64::MIN);
        need(
            sget(&l, "lotId").is_some()
                && !lot_id.is_empty()
                && !lots.contains(&lot_id)
                && qty > 0,
            "OWNED_FIFO",
        )?;
        lots.insert(lot_id);
        q += qty;
        let entry = price(sget(&l, "entryPrice").ok_or_else(|| "INVALID".to_string())?)?;
        cost += entry * qty as i128;
    }
    need(
        q == js_int(get(&p, "remainingOwnedQuantity")).unwrap_or(i64::MIN)
            && money(cost) == sget(&p, "remainingOwnedCost").unwrap_or(""),
        "EXACT_REMAINING_BASIS",
    )?;
    Ok(p)
}

fn full_account(a: &V, exchange_id: &V) -> V {
    let scoped = get(
        get(a, "scopedOrdersPagesByExchange"),
        exchange_id.as_str().unwrap_or(""),
    );
    json!({
        "cash": get(a, "cash"),
        "positions": get(a, "positions"),
        "openOrdersPages": get(a, "openOrdersPages"),
        "scopedOrdersPages": if scoped.is_null() { get(a, "scopedOrdersPages") } else { scoped },
    })
}


fn account_continuity(owned: &V, p: &V, current: &V, scope: &V) -> Result<(), String> {
    let mut binding = scope.as_object().cloned().unwrap_or_default();
    binding.insert("exchangeId".into(), get(p, "exchangeId").clone());
    let old = inspect_scoped_rest_no_account(&json!({
        "account": get(owned, "after"),
        "binding": V::Object(binding),
        "readAt": get(p, "readAt"),
    }))?;
    // s.map(({endpoint,sourceBodyHash,receivedAt}) => ({...}))
    let sources = |s: &V| -> Result<V, String> {
        let arr = s
            .as_array()
            .ok_or_else(|| "TypeError".to_string())?;
        Ok(V::Array(
            arr.iter()
                .map(|r| {
                    json!({
                        "endpoint": get(r, "endpoint"),
                        "sourceBodyHash": get(r, "sourceBodyHash"),
                        "receivedAt": get(r, "receivedAt"),
                    })
                })
                .collect(),
        ))
    };
    let after_raw = get(get(p, "sourceReceipts"), "after");
    // JS p.sourceReceipts?.after || [] — any falsy value falls back to [];
    // a truthy non-array later throws in .map (TypeError -> INVALID).
    let empty: Vec<V> = Vec::new();
    let after_receipts = if js_falsy(after_raw) {
        &V::Array(empty)
    } else {
        after_raw
    };
    let old_sorted = sort_by_id(
        get(&old, "scopedOrders")
            .as_array()
            .ok_or_else(|| "TypeError".to_string())?,
    );
    let cur_sorted = sort_by_id(
        get(current, "scopedOrders")
            .as_array()
            .ok_or_else(|| "TypeError".to_string())?,
    );
    need(
        payload_hash(&sources(get(&old, "sourceReceipts"))?)?
            == payload_hash(&sources(after_receipts)?)?
            && instant_v(get(p, "readAt"))? <= instant_v(get(current, "startedAt"))?
            && payload_hash(&V::Array(old_sorted))? == payload_hash(&V::Array(cur_sorted))?,
        "ORIGINAL_AFTER_OR_INTERVENING_ACTIVITY",
    )?;
    let position = get(current, "positions")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|r| get(r, "exchangeId") == get(p, "exchangeId"));
    need(
        position.as_ref().is_some_and(|pos| {
            get(pos, "marketId") == get(p, "marketId")
                && js_int(get(pos, "quantity"))
                    == Some(-js_int(get(p, "remainingOwnedQuantity")).unwrap_or(i64::MAX))
                && !js_truthy(pos.get("settled"))
        }),
        "EXACT_CURRENT_NO_HOLDING",
    )?;
    Ok(())
}

/// planBaalOwnedNoHedge({scope,owned,pairCensus,account,rootReceipts,
/// otherBookReceipt,funding,globalRisk,now})
pub fn plan_baal_owned_no_hedge(input: &V) -> V {
    let scope = get(input, "scope");
    let owned = get(input, "owned");
    let pair_census = get(input, "pairCensus");
    let account = get(input, "account");
    let root_receipts = get(input, "rootReceipts");
    let other_book_receipt = get(input, "otherBookReceipt");
    let funding = get(input, "funding");
    let global_risk = get(input, "globalRisk");
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
    match inner(
        scope,
        owned,
        pair_census,
        account,
        root_receipts,
        other_book_receipt,
        funding,
        global_risk,
        now,
        &base,
    ) {
        Ok(v) => v,
        Err(e) => {
            let mut out = base.as_object().cloned().unwrap_or_else(Map::new);
            out.insert(
                "reason".into(),
                json!(if e.len() > "SCALED_OWNED_HEDGE_".len()
                    && e.starts_with("SCALED_OWNED_HEDGE_")
                    && e["SCALED_OWNED_HEDGE_".len()..]
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b == b'_')
                {
                    e
                } else {
                    "INVALID_OR_INCOMPLETE_OWNED_HEDGE_SOURCE".to_string()
                }),
            );
            V::Object(out)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn inner(
    scope: &V,
    owned: &V,
    c: &V,
    account: &V,
    root_receipts: &V,
    other_book_receipt: &V,
    funding: &V,
    g: &V,
    now: &V,
    base: &V,
) -> Result<V, String> {
    let at = instant_v(now)?;
    need(
        get(scope, "ownerId").as_str() == Some("Codex")
            && sget(scope, "runId").is_some_and(|s| !s.is_empty())
            && sget(scope, "tournamentId").is_some()
            && sget(scope, "tournamentSlug").is_some_and(slug_ok),
        "SCOPE",
    )?;
    let p = owned_proof(owned, scope)?;
    need(
        get(g, "ownerId") == get(scope, "ownerId")
            && get(g, "runId") == get(scope, "runId")
            && get(g, "tournamentId") == get(scope, "tournamentId")
            && get(g, "complete") == &V::Bool(true)
            && get(g, "eventScopeComplete") == &V::Bool(true)
            && get(g, "soleCollector") == &V::Bool(true)
            && get(g, "stopped") == &V::Bool(false)
            && get(g, "unknownAcceptance") == &V::Bool(false)
            && get(g, "unresolvedIntentIds").as_array().is_some_and(|x| x.is_empty())
            && hex(get(g, "sourceHash"))
            && hex(get(g, "allocationHash"))
            && instant_v(get(g, "at"))? <= at
            && at - instant_v(get(g, "at"))? <= 20000,
        "UNKNOWN_OR_GLOBAL_SCOPE",
    )?;
    need(
        get(c, "complete") == &V::Bool(true)
            && get(c, "ownerId") == get(scope, "ownerId")
            && get(c, "runId") == get(scope, "runId")
            && get(c, "tournamentId") == get(scope, "tournamentId")
            && get(c, "firstProofHash") == get(&p, "verificationHash")
            && hex(get(c, "sourceHash"))
            && get(c, "matchedLots").is_array()
            && instant_v(get(c, "at"))? >= instant_v(get(&p, "readAt"))?
            && instant_v(get(c, "at"))? <= at
            && at - instant_v(get(c, "at"))? <= 20000,
        "EXACT_PAIR_CENSUS",
    )?;
    need(
        root_receipts
            .as_array()
            .is_some_and(|rs| rs.len() == 2),
        "TWO_ROOTS",
    )?;
    let mut roots: Vec<V> = Vec::new();
    for x in root_receipts.as_array().unwrap() {
        need(
            id(get(x, "marketId")) && id(get(x, "exchangeId")),
            "ROOT_IDS",
        )?;
        let r = raw(
            get(x, "receipt"),
            &format!(
                "/markets/{}/nodes?tournamentId={}",
                sget(x, "marketId").unwrap_or(""),
                sget(scope, "tournamentId").unwrap_or("")
            ),
            at,
            300000,
        )?;
        let d = &r.data;
        need(
            get(d, "market_id") == get(x, "marketId")
                && get(d, "contexts").as_array().is_some_and(|cs| cs.len() == 1)
                && get(&get(d, "contexts").as_array().unwrap()[0], "type").as_str()
                    == Some("tournament")
                && get(&get(d, "contexts").as_array().unwrap()[0], "tournament")
                    .get("id")
                    .and_then(V::as_str)
                    == sget(scope, "tournamentId"),
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
    need(
        roots
            .iter()
            .map(|r| sget(r, "marketId").unwrap_or(""))
            .collect::<BTreeSet<_>>()
            .len()
            == 2
            && roots.iter().any(|r| {
                get(r, "marketId") == get(&p, "marketId")
                    && get(r, "exchangeId") == get(&p, "exchangeId")
            }),
        "FIRST_ROOT_SCOPE",
    )?;
    let other = roots
        .iter()
        .find(|r| {
            get(r, "exchangeId") != get(&p, "exchangeId") && get(r, "marketId") != get(&p, "marketId")
        })
        .cloned();
    need(other.is_some(), "OPPOSING_ROOT")?;
    let other = other.unwrap();
    let universe_exchanges: Vec<V> = roots
        .iter()
        .map(|r| {
            json!({
                "marketId": get(r, "marketId"),
                "exchangeId": get(r, "exchangeId"),
                "option": get(r, "option"),
            })
        })
        .collect();
    let semantic = screen_party_winner_no_pairs(&json!({
        "universe": {"tournamentId": get(scope, "tournamentId"), "exchanges": universe_exchanges},
        "rootCaptures": roots,
        "quoteCaptures": [],
        "account": V::Null,
        "now": now,
    }))?;
    need(
        js_int(get(&semantic, "semanticPairCount")) == Some(1),
        "SAME_ORIGINAL_PARTY_WINNER_PACKAGE",
    )?;
    // JS semantic.pairs[0].rootsCurrent on a missing element throws TypeError
    // -> INVALID fallback.
    let pair0 = get(&semantic, "pairs")
        .as_array()
        .and_then(|ps| ps.get(0))
        .ok_or_else(|| "TypeError".to_string())?
        .clone();
    need(
        get(&pair0, "rootsCurrent") == &V::Bool(true)
            && get(get(owned, "package"), "eventId") == get(&pair0, "eventKey")
            && get(get(owned, "package"), "legs")
                .as_array()
                .is_some_and(|ls| {
                    ls.iter().any(|l| {
                        get(l, "marketId") == get(&other, "marketId")
                            && get(l, "exchangeId") == get(&other, "exchangeId")
                    })
                }),
        "SAME_ORIGINAL_PARTY_WINNER_PACKAGE",
    )?;
    let event_id = get(&pair0, "eventKey").clone();
    let mut binding_first = scope.as_object().cloned().unwrap_or_default();
    binding_first.insert("exchangeId".into(), get(&p, "exchangeId").clone());
    let first_current = inspect_scoped_rest_no_account(&json!({
        "account": full_account(account, get(&p, "exchangeId")),
        "binding": V::Object(binding_first),
        "readAt": now,
    }))?;
    let mut binding_other = scope.as_object().cloned().unwrap_or_default();
    binding_other.insert("exchangeId".into(), get(&other, "exchangeId").clone());
    let other_current = inspect_scoped_rest_no_account(&json!({
        "account": full_account(account, get(&other, "exchangeId")),
        "binding": V::Object(binding_other),
        "readAt": now,
    }))?;
    account_continuity(owned, &p, &first_current, scope)?;
    let (pos_data, pos_numbers) =
        parse_number_lexemes(sget(get(account, "positions"), "bodyText").unwrap_or(""))?;
    let positions = get(&pos_data, "positions")
        .as_array()
        .ok_or_else(|| "TypeError".to_string())?;
    for (i, v) in positions.iter().enumerate() {
        let raw_q = pos_numbers
            .get(&js_stringify(&json!(["positions", i, "quantity"])))
            .cloned();
        need(
            raw_q.as_ref().is_some_and(|raw_q| {
                raw_q.len() <= 128 && int_lexeme_ok(raw_q, true)
            }),
            "EXACT_POSITION_INTEGER",
        )?;
        // BigInt(v.quantity) is evaluated only after the lexeme checks pass;
        // a non-integer quantity throws RangeError -> INVALID fallback.
        let n = js_int(get(v, "quantity")).ok_or_else(|| "RangeError".to_string())?;
        need(
            BigInt::parse_bytes(
                raw_q.unwrap().split('.').next().unwrap_or("").as_bytes(),
                10,
            ) == Some(BigInt::from(n)),
            "EXACT_POSITION_INTEGER",
        )?;
    }
    let counterpart = match c.get("counterpartOwned") {
        Some(co) if !co.is_null() => {
            let cp = owned_proof(co, scope)?;
            need(
                get(&cp, "marketId") == get(&other, "marketId")
                    && get(&cp, "exchangeId") == get(&other, "exchangeId"),
                "ATTRIBUTED_COUNTERPART",
            )?;
            account_continuity(co, &cp, &other_current, scope)?;
            Some(cp)
        }
        _ => {
            need(
                !get(&other_current, "positions")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .any(|v| {
                        get(v, "exchangeId") == get(&other, "exchangeId")
                            && js_int(get(v, "quantity")) != Some(0)
                    }),
                "FOREIGN_COUNTERPART_HOLDING",
            )?;
            None
        }
    };
    let mut matched_first: HashMap<String, i64> = HashMap::new();
    let mut matched_other: HashMap<String, i64> = HashMap::new();
    let mut matched: i64 = 0;
    let mut assignment_ids: BTreeSet<String> = BTreeSet::new();
    for m in get(c, "matchedLots").as_array().cloned().unwrap_or_default() {
        let assignment_id = sget(&m, "assignmentId").unwrap_or("").to_string();
        need(
            sget(&m, "assignmentId").is_some()
                && !assignment_id.is_empty()
                && !assignment_ids.contains(&assignment_id)
                && sget(&m, "firstLotId").is_some()
                && sget(&m, "counterpartLotId").is_some()
                && js_int(get(&m, "quantity")).is_some_and(|q| q > 0)
                && counterpart.is_some(),
            "ATTRIBUTED_PAIR_ASSIGNMENT",
        )?;
        assignment_ids.insert(assignment_id);
        need(
            get(&p, "remainingLots")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .any(|l| get(l, "lotId") == get(&m, "firstLotId"))
                && get(&counterpart.clone().unwrap(), "remainingLots")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .any(|l| get(l, "lotId") == get(&m, "counterpartLotId")),
            "MATCHED_ORIGINAL_LOTS",
        )?;
        let q = js_int(get(&m, "quantity")).unwrap_or(0);
        *matched_first
            .entry(sget(&m, "firstLotId").unwrap_or("").to_string())
            .or_insert(0) += q;
        *matched_other
            .entry(sget(&m, "counterpartLotId").unwrap_or("").to_string())
            .or_insert(0) += q;
        matched += q;
    }
    for l in get(&p, "remainingLots").as_array().cloned().unwrap_or_default() {
        need(
            *matched_first
                .get(sget(&l, "lotId").unwrap_or(""))
                .unwrap_or(&0)
                <= js_int(get(&l, "quantity")).unwrap_or(i64::MIN),
            "FIRST_LOT_OVERPAIRED",
        )?;
    }
    if let Some(cp) = &counterpart {
        for l in get(cp, "remainingLots").as_array().cloned().unwrap_or_default() {
            need(
                *matched_other
                    .get(sget(&l, "lotId").unwrap_or(""))
                    .unwrap_or(&0)
                    <= js_int(get(&l, "quantity")).unwrap_or(i64::MIN),
                "COUNTERPART_LOT_OVERPAIRED",
            )?;
        }
        need(
            js_int(get(cp, "remainingOwnedQuantity")) == Some(matched),
            "EXISTING_UNMATCHED_COUNTERPART_REQUIRES_PAIR_RECONCILIATION",
        )?;
    }
    let unpaired: Vec<V> = get(&p, "remainingLots")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|l| {
            let mut lot = l.as_object().cloned().unwrap_or_default();
            let q = js_int(get(&l, "quantity")).unwrap_or(0)
                - *matched_first.get(sget(&l, "lotId").unwrap_or("")).unwrap_or(&0);
            lot.insert("quantity".into(), json!(q));
            V::Object(lot)
        })
        .filter(|l| js_int(get(l, "quantity")).unwrap_or(0) > 0)
        .collect();
    let unpaired_q = js_int(get(&p, "remainingOwnedQuantity")).unwrap_or(0) - matched;
    need(unpaired_q > 0, "NO_UNPAIRED_SHARES")?;
    let f = get(funding, "record").clone();
    let reservation_hash = get(&f, "reservationHash").clone();
    let mut fund_body = f.as_object().cloned().unwrap_or_default();
    fund_body.shift_remove("reservationHash");
    let receipt = get(funding, "receipt").clone();
    need(
        get(&f, "schemaVersion") == &json!(1)
            && get(&f, "kind").as_str() == Some("baal_scaled_no_full_nominal_reservation_v1")
            && get(&f, "ownerId") == get(scope, "ownerId")
            && get(&f, "runId") == get(scope, "runId")
            && get(&f, "tournamentId") == get(scope, "tournamentId")
            && get(&f, "allocationHash") == get(g, "allocationHash")
            && hex(&reservation_hash)
            && payload_hash(&V::Object(fund_body))? == reservation_hash.as_str().unwrap_or("")
            && *get(g, "controllerReservationHash") == reservation_hash
            && get(&f, "reservations").is_array()
            && get(&f, "owned").is_array()
            && get(&f, "unresolvedIntentIds").as_array().is_some_and(|x| x.is_empty()),
        "INSTALLED_ACTUAL_RESERVE",
    )?;
    need(
        get(&receipt, "allocationHash") == get(g, "allocationHash")
            && get(&receipt, "reservationHash") == &reservation_hash
            && get(&receipt, "globalLedgerHash") == get(g, "globalLedgerHash")
            && hex(get(&receipt, "globalLedgerHash"))
            && instant_v(get(&receipt, "confirmedAt"))? <= at
            && at - instant_v(get(&receipt, "confirmedAt"))? <= 20000,
        "FRESH_RESERVE_RECEIPT",
    )?;
    let exposure_id = format!(
        "scaled:{}:{}:hedge-for:{}",
        js_string(get(scope, "runId")),
        js_string(get(&other, "exchangeId")),
        js_string(get(&p, "exchangeId"))
    );
    let reserve: Vec<V> = get(&f, "reservations")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| {
            get(r, "marketId") == get(&other, "marketId")
                && get(r, "eventId") == &event_id
                && get(r, "exposureId").as_str() == Some(exposure_id.as_str())
        })
        .collect();
    need(
        reserve.len() == 1
            && amount(sget(&reserve[0], "maximumNominalCost").unwrap_or("")).unwrap_or(i128::MIN) >= 0
            && get(&f, "owned")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .any(|r| {
                    get(r, "marketId") == get(&p, "marketId")
                        && get(r, "eventId") == &event_id
                        && get(r, "nominalCost") == get(&p, "remainingOwnedCost")
                }),
        "SOURCE_BOUND_HEDGE_RESERVATION",
    )?;
    let reserved = amount(sget(&reserve[0], "maximumNominalCost").unwrap_or(""))?;
    let cash = minimum(&[
        amount(sget(&first_current, "cashLower").unwrap_or(""))?,
        amount(sget(&other_current, "cashLower").unwrap_or(""))?,
        amount(sget(g, "cashLower").unwrap_or(""))?,
    ]);
    // g.reservations checks — amount() errors propagate raw, matching JS
    // .every()/.some() callback throws surfacing as the catch fallback.
    need(
        get(g, "reservations").is_array(),
        "ALL_CURRENT_RESERVATIONS_CASH_FUNDED",
    )?;
    let g_reservations = get(g, "reservations")
        .as_array()
        .cloned()
        .unwrap_or_default();
    for r in &g_reservations {
        let a = amount(sget(r, "maximumNominalCost").unwrap_or(""))?;
        need(
            sget(r, "exposureId").is_some()
                && a >= 0
                && get(r, "unknownAcceptance") != &V::Bool(true),
            "ALL_CURRENT_RESERVATIONS_CASH_FUNDED",
        )?;
    }
    need(
        g_reservations
            .iter()
            .map(|r| sget(r, "exposureId").unwrap_or(""))
            .collect::<BTreeSet<&str>>()
            .len()
            == g_reservations.len(),
        "ALL_CURRENT_RESERVATIONS_CASH_FUNDED",
    )?;
    let g_hashes: Vec<String> = g_reservations
        .iter()
        .map(payload_hash)
        .collect::<Result<_, _>>()?;
    for r in get(&f, "reservations").as_array().cloned().unwrap_or_default() {
        need(
            g_hashes.contains(&payload_hash(&r)?),
            "ALL_CURRENT_RESERVATIONS_CASH_FUNDED",
        )?;
    }
    let mut f_sum: i128 = 0;
    for r in get(&f, "reservations").as_array().cloned().unwrap_or_default() {
        f_sum += amount(sget(&r, "maximumNominalCost").unwrap_or(""))?;
    }
    need(
        sget(&f, "cashReserved") == Some(money(f_sum).as_str()),
        "ALL_CURRENT_RESERVATIONS_CASH_FUNDED",
    )?;
    let mut g_sum: i128 = 0;
    for r in &g_reservations {
        g_sum += amount(sget(r, "maximumNominalCost").unwrap_or(""))?;
    }
    need(g_sum <= cash, "ALL_CURRENT_RESERVATIONS_CASH_FUNDED")?;
    need(
        instant_v(get(g, "at"))?
            >= std::cmp::max(
                instant_v(get(&first_current, "completedAt"))?,
                instant_v(get(&other_current, "completedAt"))?,
            ),
        "RISK_AFTER_ACCOUNT",
    )?;
    let b = raw(
        other_book_receipt,
        &format!(
            "/markets/{}/orderbook?tournamentId={}&depth=20",
            js_string(get(&other, "marketId")),
            sget(scope, "tournamentId").unwrap_or("")
        ),
        at,
        20000,
    )?;
    let d = &b.data;
    need(
        std::cmp::max(
            std::cmp::max(
                instant_v(get(&first_current, "completedAt"))?,
                instant_v(get(&other_current, "completedAt"))?,
            ),
            std::cmp::max(
                instant_v(get(c, "at"))?,
                std::cmp::max(
                    instant_v(get(&receipt, "confirmedAt"))?,
                    std::cmp::max(
                        instant_v(get(g, "at"))?,
                        roots
                            .iter()
                            .map(|r| instant_v(get(r, "receivedAt")))
                            .collect::<Result<Vec<i64>, String>>()?
                            .into_iter()
                            .max()
                            .unwrap_or(i64::MIN),
                    ),
                ),
            ),
        ) <= b.start
            && get(d, "contexts").as_array().is_some_and(|cs| cs.len() == 1)
            && get(&get(d, "contexts").as_array().unwrap()[0], "type").as_str()
                == Some("tournament")
            && get(&get(d, "contexts").as_array().unwrap()[0], "tournament")
                .get("id")
                .and_then(V::as_str)
                == sget(scope, "tournamentId"),
        "CAUSAL_DEPTH_SOURCE",
    )?;
    let book = normalize_book(
        d,
        sget(&other, "marketId").unwrap_or(""),
        sget(scope, "tournamentId").unwrap_or(""),
        sget(other_book_receipt, "receivedAt").unwrap_or(""),
        sget(other_book_receipt, "requestStartedAt").unwrap_or(""),
        20000.0,
        &BTreeSet::new(),
        "authoritative_receipt_v1",
    )?;
    need(
        get(&book, "exchangeId") == get(&other, "exchangeId")
            && get(&book, "bids").as_array().is_some_and(|x| x.len() <= 20)
            && get(&book, "asks").as_array().is_some_and(|x| x.len() <= 20),
        "EXACT_COUNTERPART_DEPTH20",
    )?;
    let raw_bids = get(&get(&get(d, "contexts").as_array().cloned().unwrap_or_default()[0], "orderbook"), "exchanges")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let raw_bids = if raw_bids.is_empty() {
        Vec::new()
    } else {
        get(&raw_bids[0], "bids")
            .as_array()
            .cloned()
            .unwrap_or_default()
    };
    struct Candidate {
        quantity: i64,
        limit: i128,
        raw_no_ask: i128,
        first_principal: i128,
        assigned: Vec<V>,
        level: V,
        depth: i64,
    }
    let mut best: Option<Candidate> = None;
    for level in get(&book, "bids").as_array().cloned().unwrap_or_default() {
        let idx = raw_bids.iter().position(|l| {
            price(&js_string(get(l, "price"))).unwrap_or(i128::MIN)
                == price(sget(&level, "price").unwrap_or("")).unwrap_or(i128::MAX)
        });
        let idx = match idx {
            Some(i) => i as i64,
            None => -1,
        };
        let qlex = b
            .numbers
            .get(&js_stringify(&json!([
                "contexts", 0, "orderbook", "exchanges", 0, "bids", idx, "quantity"
            ])))
            .cloned();
        let plex = b
            .numbers
            .get(&js_stringify(&json!([
                "contexts", 0, "orderbook", "exchanges", 0, "bids", idx, "price"
            ])))
            .cloned();
        need(
            qlex.as_ref().is_some_and(|qlex| {
                qlex.len() <= 128
                    && int_lexeme_ok(qlex, false)
                    && js_int(get(&level, "quantity")).is_some_and(|n| {
                        num_bigint::BigInt::parse_bytes(
                            qlex.split('.').next().unwrap_or("").as_bytes(),
                            10,
                        ) == Some(num_bigint::BigInt::from(n))
                    })
            }) && price(&plex.clone().unwrap_or_default())?
                == price(sget(&level, "price").unwrap_or(""))?,
            "EXACT_BOOK_PRICE_AND_QUANTITY",
        )?;
        let raw_no_ask = SCALE - price(sget(&level, "price").unwrap_or(""))?;
        let limit = raw_no_ask + TICK;
        if limit > SCALE - TICK {
            continue;
        }
        let cap = minimum(&[
            unpaired_q as i128,
            100,
            (js_int(get(&level, "quantity")).unwrap_or(0) / 100) as i128,
            25_000_000 / limit,
            reserved / limit,
            cash / limit,
        ]) as i64;
        let mut quantity: i64 = 0;
        let mut first_principal: i128 = 0;
        let mut assigned: Vec<V> = Vec::new();
        for l in &unpaired {
            if quantity >= cap {
                break;
            }
            let basis = price(sget(l, "entryPrice").ok_or_else(|| "INVALID".to_string())?)?;
            if basis + limit > 990_000 {
                break;
            }
            let n = std::cmp::min(
                js_int(get(l, "quantity")).unwrap_or(0),
                cap - quantity,
            );
            quantity += n;
            first_principal += basis * n as i128;
            assigned.push(json!({
                "firstLotId": get(l, "lotId"),
                "quantity": n,
                "entryPrice": get(l, "entryPrice"),
                "firstPrincipal": money(basis * n as i128),
                "counterpartLimit": money(limit),
                "protectedPairUnitCost": money(basis + limit),
            }));
        }
        if quantity > 0 {
            let candidate = Candidate {
                quantity,
                limit,
                raw_no_ask,
                first_principal,
                assigned,
                level: level.clone(),
                depth: js_int(get(&level, "quantity")).unwrap_or(0) / 100,
            };
            if best.as_ref().is_none_or(|b| quantity > b.quantity) {
                best = Some(candidate);
            }
        }
    }
    let Some(best) = best else {
        let mut out = base.as_object().cloned().unwrap_or_default();
        out.insert("reason".into(), json!("NO_FUNDED_INDIVIDUAL_DEPTH_AND_FIFO_PAIR_MARGIN"));
        out.insert("ownedProofHash".into(), get(&p, "verificationHash").clone());
        out.insert("unpairedQuantity".into(), json!(unpaired_q));
        out.insert("existingMatchedQuantity".into(), json!(matched));
        return Ok(V::Object(out));
    };
    let hedge_cost = best.limit * best.quantity as i128;
    let pair_cost = best.first_principal + hedge_cost;
    let floor = SCALE * best.quantity as i128;
    let expires_at = baal_service_runtime::contract::ms_to_iso(
        [
            b.start + 20000,
            instant_v(get(&first_current, "startedAt"))? + 20000,
            instant_v(get(&other_current, "startedAt"))? + 20000,
            instant_v(get(&receipt, "confirmedAt"))? + 20000,
        ]
        .into_iter()
        .chain(
            roots
                .iter()
                .map(|r| instant_v(get(r, "receivedAt")).map(|t| t + 300000))
                .collect::<Result<Vec<i64>, String>>()?,
        )
        .min()
        .unwrap_or(i64::MAX) as f64,
    );
    need(instant_v(&json!(expires_at))? > at, "PROPOSAL_EXPIRED")?;
    let source_roots: Vec<V> = roots
        .iter()
        .map(|r| {
            json!({
                "marketId": get(r, "marketId"),
                "exchangeId": get(r, "exchangeId"),
                "rootHash": get(r, "rootHash"),
                "sourceHash": get(r, "sourceHash"),
                "receivedAt": get(r, "receivedAt"),
            })
        })
        .collect();
    let mut proposal = base.as_object().cloned().unwrap_or_default();
    for (k, v) in [
        ("proposalReady", json!(true)),
        ("quantity", json!(best.quantity)),
        ("reason", json!("ATTRIBUTED_FIFO_CONDITIONAL_COUNTERPART_PROPOSAL")),
        ("ownerId", get(scope, "ownerId").clone()),
        ("runId", get(scope, "runId").clone()),
        ("tournamentId", get(scope, "tournamentId").clone()),
        ("eventId", event_id),
        ("firstMarketId", get(&p, "marketId").clone()),
        ("firstExchangeId", get(&p, "exchangeId").clone()),
        ("firstProofHash", get(&p, "verificationHash").clone()),
        ("firstRemainingQuantity", get(&p, "remainingOwnedQuantity").clone()),
        ("existingMatchedQuantity", json!(matched)),
        ("unpairedQuantity", json!(unpaired_q)),
        ("marketId", get(&other, "marketId").clone()),
        ("exchangeId", get(&other, "exchangeId").clone()),
        ("side", json!("no")),
        ("action", json!("buy")),
        ("protectedLimit", json!(money(best.limit))),
        ("rawObservedNoAsk", json!(money(best.raw_no_ask))),
        ("maximumHedgeNotional", json!(money(hedge_cost))),
        ("firstMatchedActualPrincipal", json!(money(best.first_principal))),
        ("protectedMatchedPairCost", json!(money(pair_cost))),
        ("conditionalOrdinaryMinimumPayout", json!(money(floor))),
        ("conditionalThirdPartyPayout", json!(money(2 * floor))),
        ("conditionalOrdinarySurplus", json!(money(floor - pair_cost))),
        ("administrativeZeroPayoutLoss", json!(money(-pair_cost))),
        ("unconditionalFloor", V::Null),
        ("fees", json!("0.000000")),
        ("guaranteedProfit", json!(false)),
        ("matchedAssignmentsIfFilled", V::Array(best.assigned)),
        ("originalPairCensusHash", json!(payload_hash(c)?)),
        ("fundingReservationHash", reservation_hash),
        ("globalLedgerHash", get(&receipt, "globalLedgerHash").clone()),
        (
            "availableAttributedCounterpartReserve",
            json!(money(reserved)),
        ),
        (
            "onePercentIndividualLevelQuantity",
            json!(best.depth),
        ),
        (
            "sourceBook",
            json!({
                "sourceBodyHash": b.hash,
                "requestStartedAt": get(other_book_receipt, "requestStartedAt"),
                "receivedAt": get(other_book_receipt, "receivedAt"),
                "versionKey": get(&book, "versionKey"),
                "yesBidLevel": best.level,
            }),
        ),
        ("sourceRoots", V::Array(source_roots)),
        (
            "currentAccountSourceHash",
            json!(payload_hash(&json!([
                get(&first_current, "sourceReceipts"),
                get(&other_current, "sourceReceipts")
            ]))?),
        ),
        ("globalRiskSourceHash", get(g, "sourceHash").clone()),
        ("expiresAt", json!(expires_at)),
        ("conditionalCarryNotRealizedProfit", json!(true)),
        ("nativeNoBookObserved", json!(false)),
        ("noLoanAdvanceDiscount", json!(true)),
        (
            "assumptions",
            json!([
                "Ordinary same-race opposing Party Winner outcomes are mutually exclusive; a third-party win can pay both NO contracts.",
                "Void/refund/administrative or inconsistent resolution can pay zero; ordinary floor is not unconditional.",
                "New counterpart fills must be terminally verified and attributed before any pairing/reserve release; source inventory remains unchanged.",
                "A separately reviewed controller/journal revision and bounded activation are required; this pure proposal authorizes no order."
            ]),
        ),
    ] {
        proposal.insert(k.to_string(), v);
    }
    let proposal = V::Object(proposal);
    let mut out = proposal.as_object().cloned().unwrap_or_default();
    out.insert("proposalHash".into(), json!(payload_hash(&proposal)?));
    Ok(V::Object(out))
}
