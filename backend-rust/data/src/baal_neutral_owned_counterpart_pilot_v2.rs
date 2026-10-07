// execution/baal-neutral-owned-counterpart-pilot-v2.mjs — versioned one-share
// neutral-only owned counterpart pilot. Never enables a prior window.
//
// Port conventions: error strings carry the JS error's `.code` (or message);
// the JS markers `e.fatal`/`e.durableRecordingFailed` are encoded by appending
// ":fatal:"/"durableRecordingFailed" to the error string (the host maps fatal
// strings back to transport errors). Guard-pointer "WeakMap" semantics use raw
// pointer identity keys, matching the other ported hosts.
use baal_collector::frozen_paper_graph::collect_frozen_paper_files;
use baal_execution::core::{canonical_payload, payload_hash};
use baal_execution::receipts::parse_number_lexemes;
use baal_execution::transport::{Guard, OrderTransport, OrderTransportError, OrderTransportOptions};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::{amount, instant, money, price};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use baal_service_runtime::ledger::acquire_owner_lease;
use baal_service_runtime::ledger::OwnerLease;
use serde_json::{json, Map, Value as V};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("HEDGE_COORDINATOR_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.as_object().and_then(|m| m.get(k)).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    get(v, k).as_str()
}
fn copy(v: &V) -> V {
    v.clone()
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn sha(s: &str) -> String {
    sha256(s.as_bytes())
}
fn sha_bytes(b: &[u8]) -> String {
    sha256(b)
}
fn iso(n: f64) -> String {
    ms_to_iso(n)
}
fn truthy(v: &V) -> bool {
    js_truthy(Some(v))
}
fn num_eq(v: &V, n: f64) -> bool {
    v.is_number() && v.as_f64() == Some(n)
}
fn instant_s(v: &V) -> Result<i64, String> {
    instant(v.as_str().ok_or_else(|| "Timestamp must be canonical UTC ISO milliseconds".to_string())?)
}
fn js_num_string(v: &V) -> String {
    v.as_str()
        .map(String::from)
        .or_else(|| v.as_i64().map(|x| x.to_string()))
        .or_else(|| v.as_u64().map(|x| x.to_string()))
        .or_else(|| v.as_f64().map(|x| js_stringify(&json!(x))))
        .unwrap_or_default()
}
// JS `x??null` — null AND absent both yield Null.
fn opt_v<'a>(v: &'a V, k: &str) -> &'a V {
    get(v, k)
}

fn active(s: &V) -> Option<V> {
    get(s, "orders")
        .as_array()
        .and_then(|os| {
            os.iter()
                .find(|o| get(o, "proof").is_null() && get(o, "phase").as_str() != Some("abandoned"))
                .cloned()
        })
}
fn active_pair_quantity(s: &V, a: &V) -> i64 {
    js_int(get(a, "quantity")).unwrap_or(0)
        - get(s, "pairRetirements")
            .as_array()
            .map(|rs| {
                rs.iter()
                    .filter(|r| get(r, "assignmentId") == get(a, "assignmentId"))
                    .map(|r| js_int(get(r, "quantity")).unwrap_or(0))
                    .sum::<i64>()
            })
            .unwrap_or(0)
}
fn matched(s: &V) -> i64 {
    get(s, "assignments")
        .as_array()
        .map(|as_| as_.iter().map(|a| active_pair_quantity(s, a)).sum())
        .unwrap_or(0)
}
fn scope_account(account: &V, ex: &str) -> V {
    let scoped = get(get(account, "scopedOrdersPagesByExchange"), ex).clone();
    let scoped = if scoped.is_null() {
        get(account, "scopedOrdersPages").clone()
    } else {
        scoped
    };
    json!({
        "cash": get(account, "cash"),
        "positions": get(account, "positions"),
        "openOrdersPages": get(account, "openOrdersPages"),
        "scopedOrdersPages": scoped,
    })
}
fn pair_census(s: &V, first_ex: &str, at: &str) -> Result<V, String> {
    let first = get(get(s, "currentInventory"), first_ex);
    let other_ex = get(get(first, "package"), "legs")
        .as_array()
        .and_then(|legs| {
            legs.iter()
                .find(|l| get(l, "exchangeId").as_str() != Some(first_ex))
                .and_then(|l| get(l, "exchangeId").as_str().map(String::from))
        })
        .unwrap_or_default();
    let counterpart = get(get(s, "currentInventory"), &other_ex);
    let counterpart_owned = {
        let cp = counterpart;
        if js_int(get(get(cp, "proof"), "remainingOwnedQuantity")).is_some_and(|q| q > 0) {
            cp.clone()
        } else {
            V::Null
        }
    };
    let matched_lots: Vec<V> = get(s, "assignments")
        .as_array()
        .map(|as_| {
            as_.iter()
                .filter(|a| {
                    get(a, "firstExchangeId").as_str() == Some(first_ex)
                        && active_pair_quantity(s, a) > 0
                })
                .map(|a| {
                    json!({
                        "assignmentId": get(a, "assignmentId"),
                        "firstLotId": get(a, "firstLotId"),
                        "counterpartLotId": get(a, "counterpartLotId"),
                        "quantity": active_pair_quantity(s, a),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let mut m = s
        .get("scope")
        .and_then(|x| x.as_object().cloned())
        .unwrap_or_default();
    m.insert("complete".into(), V::Bool(true));
    m.insert(
        "firstProofHash".into(),
        get(get(first, "proof"), "verificationHash").clone(),
    );
    m.insert(
        "sourceHash".into(),
        json!(payload_hash(&json!({
            "seed": get(s, "seedHash"),
            "assignments": get(s, "assignments"),
            "counterpart": get(get(&counterpart, "proof"), "verificationHash").clone(),
        }))?),
    );
    m.insert("at".into(), json!(at));
    m.insert("matchedLots".into(), json!(matched_lots));
    m.insert("counterpartOwned".into(), counterpart_owned);
    Ok(V::Object(m))
}

// projectRisk(s) — the funding record.
pub fn project_risk(s: &V) -> Result<V, String> {
    let mut rows: Vec<V> = Vec::new();
    let mut owned: Vec<V> = Vec::new();
    if let V::Object(inv) = get(s, "currentInventory") {
        for (ex, slot) in inv {
            let remaining = js_int(get(get(slot, "proof"), "remainingOwnedQuantity")).unwrap_or(0);
            if remaining == 0 {
                continue;
            }
            let mut allocated: HashMap<String, i64> = HashMap::new();
            for a in get(s, "assignments").as_array().cloned().unwrap_or_default() {
                let n = active_pair_quantity(s, &a);
                need(n >= 0, "PAIR_OVERRETIRE")?;
                let lot: Option<String> = if get(&a, "firstExchangeId").as_str() == Some(ex.as_str())
                {
                    get(&a, "firstLotId").as_str().map(String::from)
                } else if get(&a, "counterpartExchangeId").as_str() == Some(ex.as_str()) {
                    get(&a, "counterpartLotId").as_str().map(String::from)
                } else {
                    None
                };
                if let Some(lot) = lot {
                    *allocated.entry(lot).or_insert(0) += n;
                }
            }
            let mut q: i64 = 0;
            let mut cost: i128 = 0;
            for l in get(get(slot, "proof"), "remainingLots")
                .as_array()
                .cloned()
                .unwrap_or_default()
            {
                let n = js_int(get(&l, "quantity")).unwrap_or(0)
                    - allocated
                        .get(get(&l, "lotId").as_str().unwrap_or(""))
                        .copied()
                        .unwrap_or(0);
                need(n >= 0, "PAIR_EXCEEDS_CURRENT_LOT")?;
                q += n;
                cost += price(get(&l, "entryPrice").as_str().unwrap_or(""))? * n as i128;
            }
            let other = get(get(slot, "package"), "legs")
                .as_array()
                .and_then(|legs| {
                    legs.iter()
                        .find(|l| get(l, "exchangeId").as_str() != Some(ex.as_str()))
                        .cloned()
                })
                .unwrap_or(V::Null);
            let run_id = sget(get(s, "scope"), "runId").unwrap_or("");
            owned.push(json!({
                "exposureId": format!("scaled:{run_id}:{ex}:owned"),
                "marketId": get(get(slot, "proof"), "marketId"),
                "eventId": get(get(slot, "package"), "eventId"),
                "nominalCost": get(get(slot, "proof"), "remainingOwnedCost"),
            }));
            if q > 0 {
                rows.push(json!({
                    "exposureId": format!(
                        "scaled:{run_id}:{}:hedge-for-{ex}",
                        get(&other, "exchangeId").as_str().unwrap_or("")
                    ),
                    "marketId": get(&other, "marketId"),
                    "eventId": get(get(slot, "package"), "eventId"),
                    "maximumNominalCost": money(990_000i128 * q as i128 - cost),
                }));
            }
        }
    }
    let o = active(s);
    let unresolved: Vec<V> = o
        .as_ref()
        .map(|x| vec![get(x, "intentId").clone()])
        .unwrap_or_default();
    let cash_reserved: i128 = rows
        .iter()
        .map(|r| amount(get(r, "maximumNominalCost").as_str().unwrap_or("")))
        .collect::<Result<Vec<i128>, _>>()?
        .iter()
        .sum();
    let owned_principal: i128 = owned
        .iter()
        .map(|r| amount(get(r, "nominalCost").as_str().unwrap_or("")))
        .collect::<Result<Vec<i128>, _>>()?
        .iter()
        .sum();
    let mut body = Map::new();
    body.insert("schemaVersion".into(), json!(1));
    body.insert("kind".into(), json!("baal_scaled_no_full_nominal_reservation_v1"));
    body.insert("ownerId".into(), get(get(s, "scope"), "ownerId").clone());
    body.insert("runId".into(), get(get(s, "scope"), "runId").clone());
    body.insert("tournamentId".into(), get(get(s, "scope"), "tournamentId").clone());
    body.insert(
        "allocationHash".into(),
        json!(payload_hash(get(s, "allocation"))?),
    );
    body.insert("seedHash".into(), get(s, "seedHash").clone());
    body.insert(
        "implementationEpoch".into(),
        get(get(s, "migration"), "implementationEpoch").clone(),
    );
    body.insert("owned".into(), json!(owned));
    body.insert("reservations".into(), json!(rows));
    body.insert("unresolvedIntentIds".into(), json!(unresolved));
    body.insert("cashReserved".into(), json!(money(cash_reserved)));
    body.insert("ownedPrincipal".into(), json!(money(owned_principal)));
    let body = V::Object(body);
    let mut out = body.as_object().cloned().unwrap_or_default();
    out.insert("reservationHash".into(), json!(payload_hash(&body)?));
    Ok(V::Object(out))
}
// funding(s) === projectRisk(s) in JS.
fn funding(s: &V) -> Result<V, String> {
    project_risk(s)
}

// pilotUnknown(s)
pub fn pilot_unknown(s: &V) -> bool {
    match active(s) {
        None => false,
        Some(o) => {
            let phase = get(&o, "phase").as_str().unwrap_or("");
            phase == "uncertain"
                || phase == "semantic_mismatch"
                || (get(&o, "orderId").is_null()
                    && get(&o, "fetchStarts").as_array().is_some_and(|f| !f.is_empty()))
        }
    }
}

// emptyAll(e,s,at)
fn empty_all(e: &V, s: &V, at: i64) -> Result<(), String> {
    let tid = sget(get(s, "scope"), "tournamentId").unwrap_or("");
    let market_id = {
        let first_ex = sget(e, "firstExchangeId").unwrap_or("");
        get(get(get(get(s, "currentInventory"), first_ex), "proof"), "marketId")
            .as_str()
            .unwrap_or("")
            .to_string()
    };
    for (key, endpoint) in [
        (
            "graphReceipt",
            format!("/relationships/graph?marketId={market_id}&depth=4&tournamentId={tid}"),
        ),
        (
            "collateralReceipt",
            format!("/portfolio/collateral?tournamentId={tid}"),
        ),
    ] {
        let r = get(e, key);
        need(
            get(r, "method").as_str() == Some("GET")
                && num_eq(get(r, "status"), 200.0)
                && get(r, "endpoint").as_str() == Some(endpoint.as_str())
                && get(r, "bodyText").is_string()
                && get(r, "sourceBodyHash").as_str()
                    == Some(sha(get(r, "bodyText").as_str().unwrap_or("")).as_str())
                && instant_s(get(r, "requestStartedAt"))? <= instant_s(get(r, "receivedAt"))?
                && instant_s(get(r, "receivedAt"))? <= at
                && at - instant_s(get(r, "requestStartedAt"))? <= 5000,
            &format!("FRESH_NONE_{key}"),
        )?;
        let (d, _nums) =
            parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        if key == "graphReceipt" {
            need(
                get(&d, "nodes").as_array().is_some_and(|a| a.is_empty())
                    && get(&d, "relationships")
                        .as_array()
                        .is_some_and(|a| a.is_empty()),
                "NON_NEUTRAL_graphReceipt",
            )?;
        } else {
            need(
                get(&d, "data").as_array().is_some_and(|a| a.is_empty()),
                "NON_NEUTRAL_collateralReceipt",
            )?;
        }
    }
    let book = {
        let b = opt_v(e, "otherBookReceipt");
        if b.is_null() {
            let b = opt_v(e, "bookReceipt");
            if b.is_null() {
                opt_v(e, "book").clone()
            } else {
                b.clone()
            }
        } else {
            b.clone()
        }
    };
    let last_account = if !opt_v(e, "after").is_null() {
        let after = opt_v(e, "after");
        let mut ts: Vec<i64> = vec![
            instant_s(get(get(after, "cash"), "receivedAt"))?,
            instant_s(get(get(after, "positions"), "receivedAt"))?,
        ];
        for r in get(after, "openOrdersPages").as_array().cloned().unwrap_or_default() {
            ts.push(instant_s(get(&r, "receivedAt"))?);
        }
        for r in get(after, "scopedOrdersPages").as_array().cloned().unwrap_or_default() {
            ts.push(instant_s(get(&r, "receivedAt"))?);
        }
        Some(ts.iter().copied().max().unwrap_or(0))
    } else {
        None
    };
    let floor = if !book.is_null() {
        instant_s(get(&book, "receivedAt"))?
    } else {
        last_account.unwrap_or(0)
    };
    need(
        instant_s(get(get(e, "graphReceipt"), "receivedAt"))?
            <= instant_s(get(get(e, "collateralReceipt"), "requestStartedAt"))?
            && instant_s(get(get(e, "graphReceipt"), "requestStartedAt"))? >= floor,
        "COLLATERAL_LAST",
    )?;
    Ok(())
}

// sellProposal(s,ex,e,g,at)
fn sell_proposal(s: &V, ex: &str, e: &V, g: &V, at: i64) -> Result<V, String> {
    need(
        at >= instant_s(get(get(s, "activation"), "startedAt"))?
            && at < instant_s(get(get(s, "activation"), "exitExpiresAt"))?,
        "OWNED_EXIT_WINDOW",
    )?;
    let active_id = active(s).map(|o| get(&o, "intentId").clone()).unwrap_or(V::Null);
    need(
        get(g, "ownerId") == get(get(s, "scope"), "ownerId")
            && get(g, "runId") == get(get(s, "scope"), "runId")
            && get(g, "tournamentId") == get(get(s, "scope"), "tournamentId")
            && get(g, "complete") == &V::Bool(true)
            && get(g, "eventScopeComplete") == &V::Bool(true)
            && get(g, "soleCollector") == &V::Bool(true)
            && get(g, "stopped") == &V::Bool(false)
            && hex(get(g, "sourceHash"))
            && get(g, "unknownAcceptance") == &V::Bool(false)
            && get(g, "unresolvedIntentIds").is_array()
            && get(g, "unresolvedIntentIds")
                .as_array()
                .unwrap()
                .iter()
                .all(|id| id == &active_id)
            && (instant_s(get(g, "at"))? as i128) <= at as i128
            && (at as i128 - instant_s(get(g, "at"))? as i128) <= 20000,
        "GLOBAL_UNKNOWN",
    )?;
    need(
        get(g, "controllerReservationHash") == get(&funding(s)?, "reservationHash")
            && get(g, "globalLedgerHash") == get(get(s, "reservation"), "globalLedgerHash"),
        "CURRENT_RESERVE_BINDING",
    )?;
    let mut ev = e.as_object().cloned().unwrap_or_default();
    ev.insert("firstExchangeId".into(), json!(ex));
    empty_all(&V::Object(ev), s, at)?;
    let slot = get(get(s, "currentInventory"), ex);
    need(
        js_int(get(get(slot, "proof"), "remainingOwnedQuantity")).is_some_and(|q| q > 0),
        "OWNED_POSITIVE",
    )?;
    let book = {
        let b = opt_v(e, "bookReceipt");
        if b.is_null() {
            opt_v(e, "book").clone()
        } else {
            b.clone()
        }
    };
    let p = crate::baal_scaled_profitable_exit_signal_v2::plan_baal_scaled_profitable_exit_v2(
        &json!({
            "scope": get(s, "scope"),
            "owned": slot,
            "account": scope_account(get(e, "account"), ex),
            "bookReceipt": book,
            "globalRisk": get(e, "globalRisk"),
            "now": iso(at as f64),
            "protectiveTicks": 0,
            "maximumSharesPerOrder": 100,
            "maximumOrderCost": "25.000000",
            "feesPerShare": "0.000000",
            "requestedQuantity": opt_v(e, "requestedQuantity"),
        }),
    );
    need(
        get(&p, "proposalReady") == &V::Bool(true),
        &format!("PROFIT_EXIT_{}", sget(&p, "reason").unwrap_or("")),
    )?;
    Ok(p)
}

// guarded(s,e,g,at)
fn guarded(s: &V, e: &V, g: &V, at: i64) -> Result<V, String> {
    need(
        at >= instant_s(get(get(s, "activation"), "startedAt"))?
            && at < instant_s(get(get(s, "activation"), "hedgeExpiresAt"))?,
        "HEDGE_WINDOW",
    )?;
    let active_id = active(s).map(|o| get(&o, "intentId").clone()).unwrap_or(V::Null);
    need(
        get(g, "ownerId") == get(get(s, "scope"), "ownerId")
            && get(g, "runId") == get(get(s, "scope"), "runId")
            && get(g, "tournamentId") == get(get(s, "scope"), "tournamentId")
            && get(g, "complete") == &V::Bool(true)
            && get(g, "eventScopeComplete") == &V::Bool(true)
            && get(g, "soleCollector") == &V::Bool(true)
            && get(g, "stopped") == &V::Bool(false)
            && hex(get(g, "sourceHash"))
            && get(g, "unknownAcceptance") == &V::Bool(false)
            && get(g, "unresolvedIntentIds").is_array()
            && get(g, "unresolvedIntentIds")
                .as_array()
                .unwrap()
                .iter()
                .all(|id| id == &active_id)
            && instant_s(get(g, "at"))? <= at
            && at - instant_s(get(g, "at"))? <= 20000,
        "GLOBAL_UNKNOWN",
    )?;
    need(
        get(g, "fundingSatisfied") != &V::Bool(false)
            && get(g, "reservations").is_array()
            && get(g, "reservations").as_array().unwrap().iter().all(|r| {
                amount(sget(r, "maximumNominalCost").unwrap_or(""))
                    .map(|x| x >= 0)
                    .unwrap_or(false)
                    && get(r, "unknownAcceptance") != &V::Bool(true)
            })
            && {
                let ids: HashSet<String> = get(g, "reservations")
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| js_stringify(get(r, "exposureId")))
                    .collect();
                ids.len() == get(g, "reservations").as_array().unwrap().len()
            }
            && get(g, "reservations").as_array().unwrap().iter().try_fold(
                0i128,
                |n, r| amount(sget(r, "maximumNominalCost").unwrap_or("")).map(|x| n + x),
            )? <= amount(sget(g, "cashLower").unwrap_or(""))?,
        "CURRENT_CASH_FUNDS_FULL_RESERVE",
    )?;
    empty_all(e, s, at)?;
    need(
        truthy(get(e, "pairCensus"))
            && same(
                get(e, "pairCensus"),
                &pair_census(
                    s,
                    sget(e, "firstExchangeId").unwrap_or(""),
                    sget(get(e, "pairCensus"), "at").unwrap_or(""),
                )?,
            )?,
        "CAPTURED_PAIR_CENSUS",
    )?;
    need(
        truthy(get(e, "globalRisk"))
            && get(get(e, "globalRisk"), "allocationHash") == get(g, "allocationHash")
            && same(get(get(e, "globalRisk"), "reservations"), get(g, "reservations"))?
            && get(g, "controllerReservationHash") == get(&funding(s)?, "reservationHash")
            && get(g, "globalLedgerHash") == get(get(s, "reservation"), "globalLedgerHash"),
        "CAPTURED_BUDGET_STILL_MATCHES",
    )?;
    let source_risk = get(e, "globalRisk");
    need(
        get(source_risk, "unresolvedIntentIds")
            .as_array()
            .is_some_and(|a| a.is_empty()),
        "SOURCE_RISK_PENDING",
    )?;
    let mut input = e.as_object().cloned().unwrap_or_default();
    input.insert("globalRisk".into(), source_risk.clone());
    input.insert("scope".into(), get(s, "scope").clone());
    input.insert(
        "owned".into(),
        get(get(s, "currentInventory"), sget(e, "firstExchangeId").unwrap_or("")).clone(),
    );
    input.insert("now".into(), json!(iso(at as f64)));
    let p = crate::baal_scaled_owned_no_hedge_proposal_v1::plan_baal_owned_no_hedge(&V::Object(input));
    need(
        get(&p, "proposalReady") == &V::Bool(true),
        &format!("PROPOSAL_{}", sget(&p, "reason").unwrap_or("")),
    )?;
    let first = get(&p, "matchedAssignmentsIfFilled")
        .as_array()
        .and_then(|a| a.first().cloned())
        .unwrap_or(V::Null);
    need(
        truthy(&first)
            && price(sget(&p, "protectedLimit").unwrap_or(""))? <= 995_000i128,
        "PILOT_COST",
    )?;
    let mut q = p.as_object().cloned().unwrap_or_default();
    // {...p, mode, originalProposalHash, quantity:1, maximumHedgeNotional,
    //  firstMatchedActualPrincipal, protectedMatchedPairCost,
    //  conditionalOrdinaryMinimumPayout, conditionalThirdPartyPayout,
    //  conditionalOrdinarySurplus, administrativeZeroPayoutLoss,
    //  matchedAssignmentsIfFilled:[{...first,quantity:1,firstPrincipal}],
    //  proposalHash:undefined} — then proposalHash deleted and recomputed.
    q.insert("mode".into(), json!("baal_neutral_owned_counterpart_fixed_one_v1"));
    q.insert("originalProposalHash".into(), get(&p, "proposalHash").clone());
    q.insert("quantity".into(), json!(1));
    q.insert("maximumHedgeNotional".into(), get(&p, "protectedLimit").clone());
    q.insert("firstMatchedActualPrincipal".into(), get(&first, "entryPrice").clone());
    q.insert(
        "protectedMatchedPairCost".into(),
        json!(money(
            price(sget(&first, "entryPrice").unwrap_or(""))?
                + price(sget(&p, "protectedLimit").unwrap_or(""))?
        )),
    );
    q.insert("conditionalOrdinaryMinimumPayout".into(), json!("1.000000"));
    q.insert("conditionalThirdPartyPayout".into(), json!("2.000000"));
    q.insert(
        "conditionalOrdinarySurplus".into(),
        json!(money(
            1_000_000i128
                - price(sget(&first, "entryPrice").unwrap_or(""))?
                - price(sget(&p, "protectedLimit").unwrap_or(""))?
        )),
    );
    q.insert(
        "administrativeZeroPayoutLoss".into(),
        json!(money(
            -price(sget(&first, "entryPrice").unwrap_or(""))?
                - price(sget(&p, "protectedLimit").unwrap_or(""))?
        )),
    );
    let mut first2 = first.as_object().cloned().unwrap_or_default();
    first2.insert("quantity".into(), json!(1));
    first2.insert("firstPrincipal".into(), get(&first, "entryPrice").clone());
    q.insert("matchedAssignmentsIfFilled".into(), json!([V::Object(first2)]));
    q.insert("proposalHash".into(), V::Null); // proposalHash:undefined
    q.remove("proposalHash"); // delete q.proposalHash
    let q = V::Object(q);
    let mut out = q.as_object().cloned().unwrap_or_default();
    out.insert("proposalHash".into(), json!(payload_hash(&q)?));
    Ok(V::Object(out))
}

// retireSoldPairs(s,o,proof)
fn retire_sold_pairs(s: &V, o: &V, proof: &V) -> Result<Vec<V>, String> {
    let mut out: Vec<V> = Vec::new();
    for old in get(get(o, "priorProof"), "remainingLots")
        .as_array()
        .cloned()
        .unwrap_or_default()
    {
        let next = get(proof, "remainingLots")
            .as_array()
            .and_then(|ls| {
                ls.iter()
                    .find(|l| get(l, "lotId") == get(&old, "lotId"))
                    .cloned()
            })
            .unwrap_or(V::Null);
        let removed = js_int(get(&old, "quantity")).unwrap_or(0)
            - js_int(get(&next, "quantity")).unwrap_or(0);
        need(removed >= 0, "SELL_FIFO_REGRESSION")?;
        if removed == 0 {
            continue;
        }
        let begin = js_int(get(&old, "originalQuantity")).unwrap_or(0)
            - js_int(get(&old, "quantity")).unwrap_or(0);
        let end = begin + removed;
        for a in get(s, "assignments").as_array().cloned().unwrap_or_default() {
            if active_pair_quantity(s, &a) <= 0 {
                continue;
            }
            let first = get(&a, "firstExchangeId").as_str() == sget(get(o, "body"), "exchangeId")
                && get(&a, "firstLotId") == get(&old, "lotId");
            let counter = get(&a, "counterpartExchangeId").as_str()
                == sget(get(o, "body"), "exchangeId")
                && get(&a, "counterpartLotId") == get(&old, "lotId");
            if !first && !counter {
                continue;
            }
            let offset = if first {
                js_int(get(&a, "firstUnitStart"))
            } else {
                js_int(get(&a, "counterpartUnitStart"))
            };
            need(
                offset.is_some() && num_eq(get(&a, "quantity"), 1.0),
                "ONE_SHARE_PAIR_INTERVAL",
            )?;
            let offset = offset.unwrap();
            let n = std::cmp::max(
                0,
                std::cmp::min(end, offset + js_int(get(&a, "quantity")).unwrap_or(0))
                    - std::cmp::max(begin, offset),
            );
            if n > 0 {
                out.push(json!({
                    "retirementId": format!("{}:{}", sget(o, "intentId").unwrap_or(""), sget(&a, "assignmentId").unwrap_or("")),
                    "assignmentId": get(&a, "assignmentId"),
                    "quantity": n,
                    "consumedExchangeId": get(get(o, "body"), "exchangeId"),
                    "consumedLotId": get(&old, "lotId"),
                    "consumedUnitStart": std::cmp::max(begin, offset),
                    "sellProofHash": get(proof, "verificationHash"),
                    "reason": "actual_fifo_owned_sell",
                }));
            }
        }
    }
    let ids: HashSet<String> = out
        .iter()
        .map(|r| js_stringify(get(r, "assignmentId")))
        .collect();
    need(ids.len() == out.len(), "PAIR_DOUBLE_RETIREMENT")?;
    Ok(out)
}

// assignFill(s,o,p)
fn assign_fill(s: &V, o: &V, p: &V) -> Result<Vec<V>, String> {
    let mut result: Vec<V> = Vec::new();
    let mut target: usize = 0;
    let matched_list = get(get(o, "proposal"), "matchedAssignmentsIfFilled")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut left = matched_list
        .first()
        .map(|m| js_int(get(m, "quantity")).unwrap_or(0))
        .unwrap_or(0);
    for lot in get(p, "remainingLots").as_array().cloned().unwrap_or_default() {
        let mut remaining = js_int(get(&lot, "quantity")).unwrap_or(0);
        // Skip lots already in priorProof.
        if get(get(o, "priorProof"), "remainingLots")
            .as_array()
            .is_some_and(|old| old.iter().any(|x| get(x, "lotId") == get(&lot, "lotId")))
        {
            continue;
        }
        while remaining > 0 {
            let first = matched_list.get(target).cloned().unwrap_or(V::Null);
            need(truthy(&first) && left > 0, "MATCHED_FILL_EXCEEDS_PROPOSAL")?;
            let n = std::cmp::min(left, remaining);
            need(
                price(sget(&first, "entryPrice").unwrap_or(""))?
                    + price(sget(&lot, "entryPrice").unwrap_or(""))?
                    <= 990_000i128,
                "ACTUAL_PAIR_MARGIN",
            )?;
            let first_ex = sget(o, "firstExchangeId").unwrap_or("").to_string();
            let first_lots = get(
                get(get(s, "currentInventory"), &first_ex),
                "proof",
            )
            .get("remainingLots")
            .cloned()
            .unwrap_or(V::Null);
            let flot = first_lots
                .as_array()
                .and_then(|ls| {
                    ls.iter()
                        .find(|l| get(l, "lotId") == get(&first, "firstLotId"))
                        .cloned()
                })
                .unwrap_or(V::Null);
            let first_unit_start = js_int(get(&flot, "originalQuantity")).unwrap_or(0)
                - js_int(get(&flot, "quantity")).unwrap_or(0);
            result.push(json!({
                "assignmentId": format!("{}:{}:{}", sget(o, "intentId").unwrap_or(""), sget(&lot, "lotId").unwrap_or(""), target),
                "firstExchangeId": get(o, "firstExchangeId"),
                "counterpartExchangeId": get(get(o, "body"), "exchangeId"),
                "firstLotId": get(&first, "firstLotId"),
                "counterpartLotId": get(&lot, "lotId"),
                "quantity": n,
                "firstUnitCost": get(&first, "entryPrice"),
                "counterpartUnitCost": get(&lot, "entryPrice"),
                "firstProofHash": get(get(o, "proposal"), "firstProofHash"),
                "counterpartProofHash": get(p, "verificationHash"),
                "firstUnitStart": first_unit_start,
                "counterpartUnitStart": js_int(get(&lot, "originalQuantity")).unwrap_or(0)
                    - js_int(get(&lot, "quantity")).unwrap_or(0),
            }));
            remaining -= n;
            left -= n;
            if left == 0 {
                target += 1;
                left = matched_list
                    .get(target)
                    .map(|m| js_int(get(m, "quantity")).unwrap_or(0))
                    .unwrap_or(0);
            }
        }
    }
    need(
        result.iter().map(|a| js_int(get(a, "quantity")).unwrap_or(0)).sum::<i64>()
            == js_int(get(p, "filledQuantity")).unwrap_or(0),
        "EXACT_PARTIAL_PAIR_COUNT",
    )?;
    Ok(result)
}

// reduce(s,e)
fn reduce(s: &V, e: &V) -> Result<V, String> {
    if sget(e, "kind") == Some("seed") {
        let seed = get(e, "seed");
        need(
            s.is_null()
                && get(seed, "economicReplayPerformed") == &V::Bool(true)
                && !get(get(seed, "state"), "orders")
                    .as_array()
                    .map(|os| {
                        os.iter().any(|o| {
                            get(o, "proof").is_null()
                                && get(o, "phase").as_str() != Some("abandoned")
                        })
                    })
                    .unwrap_or(false),
            "VERIFIED_SEED",
        )?;
        return Ok(json!({
            "schemaVersion": 1,
            "mode": "baal_neutral_owned_counterpart_pilot_v2",
            "scope": copy(get(get(seed, "state"), "scope")),
            "allocation": copy(get(get(seed, "state"), "allocation")),
            "originalActivation": copy(get(get(seed, "state"), "activation")),
            "originalOrders": copy(get(get(seed, "state"), "orders")),
            "originalInventory": copy(get(get(seed, "state"), "inventory")),
            "currentInventory": copy(get(get(seed, "state"), "inventory")),
            "pairRetirements": [],
            "seedHash": get(e, "seedHash"),
            "seedAnchor": copy(get(seed, "anchor")),
            "activation": copy(get(e, "activation")),
            "migration": copy(get(e, "migration")),
            "orders": [],
            "assignments": [],
            "counterInventory": {},
            "reservation": V::Null,
        }));
    }
    need(!s.is_null(), "NOT_SEEDED")?;
    let mut n = copy(s);
    let o = active(s);
    match sget(e, "kind").unwrap_or("") {
        "reservation" => {
            need(
                get(get(e, "receipt"), "reservationHash") == get(&funding(s)?, "reservationHash")
                    && get(get(e, "receipt"), "seedHash") == get(s, "seedHash")
                    && hex(get(get(e, "receipt"), "globalLedgerHash")),
                "RESERVATION",
            )?;
            n["reservation"] = get(e, "receipt").clone();
            Ok(n)
        }
        "prepared_sell" => {
            need(o.is_none(), "UNKNOWN_OR_ACTIVE")?;
            let ex = sget(e, "exchangeId").unwrap_or("");
            let p = sell_proposal(
                s,
                ex,
                get(e, "evidence"),
                get(e, "globalRisk"),
                instant_s(get(e, "at"))?,
            )?;
            let slot = get(get(s, "currentInventory"), ex);
            let body = json!({
                "idempotencyKey": get(e, "key"),
                "exchangeId": ex,
                "side": "no",
                "action": "sell",
                "quantity": get(&p, "quantity"),
                "price": get(&p, "limit").as_str().and_then(|x| x.parse::<f64>().ok()).unwrap_or(f64::NAN),
                "expirationDate": iso(
                    (instant_s(get(e, "at"))? + 30000)
                        .min(instant_s(get(&p, "expiresAt"))?)
                        .min(instant_s(get(get(s, "activation"), "exitExpiresAt"))?) as f64
                ),
                "tournamentId": get(get(s, "scope"), "tournamentId"),
            });
            let key = get(e, "key");
            let intent_id = get(e, "intentId");
            need(
                !get(s, "originalOrders")
                    .as_array()
                    .is_some_and(|os| {
                        os.iter()
                            .any(|x| get(get(x, "body"), "idempotencyKey") == key)
                    })
                    && !get(s, "orders").as_array().is_some_and(|os| {
                        os.iter().any(|x| {
                            get(get(x, "body"), "idempotencyKey") == key
                                || get(x, "intentId") == intent_id
                        })
                    }),
                "KEY_REUSE",
            )?;
            let order = json!({
                "intentId": intent_id,
                "firstExchangeId": ex,
                "marketId": get(&p, "marketId"),
                "createdAt": get(e, "at"),
                "body": body,
                "bodyJson": canonical_payload(&body)?,
                "bodyHash": payload_hash(&body)?,
                "proposal": p,
                "before": scope_account(get(get(e, "evidence"), "account"), ex),
                "priorProof": copy(get(slot, "proof")),
                "priorAfter": copy(get(slot, "after")),
                "package": copy(get(slot, "package")),
                "phase": "prepared",
                "orderId": V::Null,
                "ack": V::Null,
                "proof": V::Null,
                "fences": [],
                "fetchStarts": [],
                "receipts": [],
                "semanticMismatch": false,
                "mayHaveExecuted": false,
            });
            n["orders"].as_array_mut().unwrap().push(order);
            Ok(n)
        }
        "prepared" => {
            need(
                o.is_none()
                    && get(s, "orders")
                        .as_array()
                        .map(|os| {
                            !os.iter()
                                .any(|x| get(get(x, "body"), "action").as_str() == Some("buy"))
                        })
                        .unwrap_or(false)
                    && get(get(s, "activation"), "allowedFirstExchangeIds")
                        .as_array()
                        .is_some_and(|ids| {
                            ids.iter().any(|x| x == get(get(e, "evidence"), "firstExchangeId"))
                        })
                    && js_int(get(
                        get(
                            get(get(s, "originalInventory"),
                                sget(get(e, "evidence"), "firstExchangeId").unwrap_or("")),
                            "proof",
                        ),
                        "remainingOwnedQuantity",
                    ))
                    .is_some_and(|q| q > 0),
                "ORIGINAL_UNPAIRED_ONLY",
            )?;
            let p = guarded(
                s,
                get(e, "evidence"),
                get(e, "globalRisk"),
                instant_s(get(e, "at"))?,
            )?;
            let p_ex = sget(&p, "exchangeId").unwrap_or("");
            let body = json!({
                "idempotencyKey": get(e, "key"),
                "exchangeId": p_ex,
                "side": "no",
                "action": "buy",
                "quantity": get(&p, "quantity"),
                "price": get(&p, "protectedLimit").as_str().and_then(|x| x.parse::<f64>().ok()).unwrap_or(f64::NAN),
                "expirationDate": iso(
                    (instant_s(get(e, "at"))? + 30000)
                        .min(instant_s(get(&p, "expiresAt"))?)
                        .min(instant_s(get(get(s, "activation"), "hedgeExpiresAt"))?) as f64
                ),
                "tournamentId": get(get(s, "scope"), "tournamentId"),
            });
            let key = get(e, "key");
            let intent_id = get(e, "intentId");
            need(
                !get(s, "orders").as_array().is_some_and(|os| {
                    os.iter().any(|x| {
                        get(get(x, "body"), "idempotencyKey") == key
                            || get(x, "intentId") == intent_id
                    })
                }),
                "KEY_REUSE",
            )?;
            let first_ex = sget(get(e, "evidence"), "firstExchangeId").unwrap_or("");
            let order = json!({
                "intentId": intent_id,
                "firstExchangeId": first_ex,
                "marketId": get(&p, "marketId"),
                "createdAt": get(e, "at"),
                "body": body,
                "bodyJson": canonical_payload(&body)?,
                "bodyHash": payload_hash(&body)?,
                "proposal": p,
                "before": scope_account(get(get(e, "evidence"), "account"), p_ex),
                "priorProof": get(get(get(s, "currentInventory"), p_ex), "proof").clone(),
                "priorAfter": get(get(get(s, "currentInventory"), p_ex), "after").clone(),
                "package": copy(get(get(get(s, "originalInventory"), first_ex), "package")),
                "phase": "prepared",
                "orderId": V::Null,
                "ack": V::Null,
                "proof": V::Null,
                "fences": [],
                "fetchStarts": [],
                "receipts": [],
                "semanticMismatch": false,
                "mayHaveExecuted": false,
            });
            n["orders"].as_array_mut().unwrap().push(order);
            Ok(n)
        }
        "abandoned_prepared" => {
            let oo = o.clone().unwrap_or(V::Null);
            need(
                o.is_some()
                    && get(&oo, "phase").as_str() == Some("prepared")
                    && get(&oo, "orderId").is_null()
                    && get(&oo, "ack").is_null()
                    && get(&oo, "proof").is_null()
                    && get(&oo, "semanticMismatch") == &V::Bool(false)
                    && get(&oo, "mayHaveExecuted") == &V::Bool(false)
                    && get(&oo, "fences").as_array().is_some_and(|a| a.is_empty())
                    && get(&oo, "fetchStarts").as_array().is_some_and(|a| a.is_empty())
                    && get(&oo, "receipts").as_array().is_some_and(|a| a.is_empty())
                    && instant_s(get(e, "at"))?
                        >= instant_s(get(get(&oo, "body"), "expirationDate"))?,
                "ABANDON_REQUIRES_EXPIRED_UNADMITTED_UNFETCHED",
            )?;
            let intent_id = get(&oo, "intentId").clone();
            let arr = n["orders"].as_array_mut().unwrap();
            let target = arr
                .iter_mut()
                .find(|x| get(x, "intentId") == &intent_id)
                .unwrap();
            target["phase"] = json!("abandoned");
            target["abandonedAt"] = get(e, "at").clone();
            target["abandonedReason"] = json!("expired_before_any_admission");
            Ok(n)
        }
        "admitted" | "network_fence" => {
            need(
                o.as_ref().is_some() && get(o.as_ref().unwrap(), "proof").is_null(),
                "NO_PENDING",
            )?;
            let oo = o.clone().unwrap();
            need(
                if sget(e, "kind") == Some("admitted") {
                    get(&oo, "phase").as_str() == Some("prepared")
                } else {
                    get(&oo, "phase").as_str() == Some("dispatching")
                },
                "SINGLE_ATTEMPT",
            )?;
            let is_buy = get(get(&oo, "body"), "action").as_str() == Some("buy");
            let p = if is_buy {
                guarded(s, get(e, "evidence"), get(e, "globalRisk"), instant_s(get(e, "at"))?)?
            } else {
                sell_proposal(
                    s,
                    sget(get(&oo, "body"), "exchangeId").unwrap_or(""),
                    get(e, "evidence"),
                    get(e, "globalRisk"),
                    instant_s(get(e, "at"))?,
                )?
            };
            need(
                get(&p, "exchangeId") == get(get(&oo, "body"), "exchangeId")
                    && js_int(get(&p, "quantity")).unwrap_or(0)
                        >= js_int(get(get(&oo, "body"), "quantity")).unwrap_or(0)
                    && (if is_buy {
                        get(&p, "firstProofHash") == get(get(&oo, "proposal"), "firstProofHash")
                            && price(sget(&p, "protectedLimit").unwrap_or(""))?
                                <= price(&js_num_string(get(get(&oo, "body"), "price")))?
                    } else {
                        get(&p, "ownedProofHash") == get(get(&oo, "priorProof"), "verificationHash")
                            && price(sget(&p, "limit").unwrap_or(""))?
                                >= price(&js_num_string(get(get(&oo, "body"), "price")))?
                    })
                    && instant_s(get(e, "at"))?
                        < instant_s(get(get(&oo, "body"), "expirationDate"))?,
                "FIXED_LIMIT_SOURCE",
            )?;
            let intent_id = get(&oo, "intentId").clone();
            let arr = n["orders"].as_array_mut().unwrap();
            let target = arr
                .iter_mut()
                .find(|x| get(x, "intentId") == &intent_id)
                .unwrap();
            if sget(e, "kind") == Some("admitted") {
                target["phase"] = json!("dispatching");
            } else {
                need(
                    payload_hash(get(e, "lease"))? == sget(e, "leaseHash").unwrap_or(""),
                    "ACTUAL_LEASE",
                )?;
                target["fences"].as_array_mut().unwrap().push(json!({
                    "at": get(e, "at"),
                    "lease": get(e, "lease"),
                    "leaseHash": get(e, "leaseHash"),
                }));
            }
            Ok(n)
        }
        "fetch_start" => {
            need(
                o.as_ref().is_some() && get(o.as_ref().unwrap(), "proof").is_null(),
                "NO_PENDING",
            )?;
            let oo = o.clone().unwrap();
            let f = get(&oo, "fences").as_array().and_then(|a| a.last().cloned());
            let w = get(e, "meta");
            need(
                f.is_some()
                    && get(w, "method").as_str() == Some("POST")
                    && get(w, "endpoint").as_str() == Some("/orders")
                    && get(w, "requestBodyHash") == get(&oo, "bodyHash")
                    && instant_s(get(w, "requestStartedAt"))?
                        >= instant_s(get(&f.unwrap(), "at"))?
                    && get(&oo, "fetchStarts").as_array().is_some_and(|a| a.is_empty()),
                "ACTUAL_INVOCATION",
            )?;
            let intent_id = get(&oo, "intentId").clone();
            let arr = n["orders"].as_array_mut().unwrap();
            let target = arr
                .iter_mut()
                .find(|x| get(x, "intentId") == &intent_id)
                .unwrap();
            target["fetchStarts"].as_array_mut().unwrap().push(copy(w));
            target["mayHaveExecuted"] = V::Bool(true);
            Ok(n)
        }
        "wire" => {
            need(
                o.as_ref().is_some() && get(o.as_ref().unwrap(), "proof").is_null(),
                "NO_PENDING",
            )?;
            let oo = o.clone().unwrap();
            let w = get(&oo, "fetchStarts").as_array().and_then(|a| a.first().cloned());
            let r = get(e, "receipt");
            need(
                w.is_some()
                    && get(r, "method").as_str() == Some("POST")
                    && get(r, "endpoint").as_str() == Some("/orders")
                    && get(r, "sourceBodyHash").as_str()
                        == Some(sha(get(r, "bodyText").as_str().unwrap_or("")).as_str())
                    && get(r, "fetchInvocationId") == get(w.as_ref().unwrap(), "fetchInvocationId")
                    && get(r, "requestStartedAt") == get(w.as_ref().unwrap(), "requestStartedAt"),
                "ORIGINAL_WIRE",
            )?;
            let (d, _) =
                parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
            let order_id = js_int(get(&d, "orderId"))
                .filter(|x| *x > 0)
                .map(|x| x.to_string());
            let mut ack = r.clone();
            ack.as_object_mut()
                .map(|m| m.insert("requestBodyHash".into(), get(&oo, "bodyHash").clone()));
            let semantic_mismatch =
                !crate::baal_scaled_controller_legacy_replay_v1::validate_legacy_scaled_neutral_ack(
                    &oo, &ack,
                );
            let intent_id = get(&oo, "intentId").clone();
            let arr = n["orders"].as_array_mut().unwrap();
            let target = arr
                .iter_mut()
                .find(|x| get(x, "intentId") == &intent_id)
                .unwrap();
            target["receipts"].as_array_mut().unwrap().push(copy(r));
            if let Some(oid) = order_id {
                target["orderId"] = json!(oid);
            }
            target["ack"] = ack;
            target["semanticMismatch"] = V::Bool(semantic_mismatch);
            target["phase"] = json!(if semantic_mismatch {
                "semantic_mismatch"
            } else {
                "acknowledged"
            });
            Ok(n)
        }
        "error" => {
            need(
                o.as_ref().is_some() && get(o.as_ref().unwrap(), "proof").is_null(),
                "NO_PENDING",
            )?;
            let oo = o.clone().unwrap();
            let phase = get(&oo, "phase").as_str().unwrap_or("").to_string();
            if phase == "dispatching" || get(e, "mayHaveExecuted") != &V::Bool(false) {
                let intent_id = get(&oo, "intentId").clone();
                let arr = n["orders"].as_array_mut().unwrap();
                let target = arr
                    .iter_mut()
                    .find(|x| get(x, "intentId") == &intent_id)
                    .unwrap();
                target["phase"] = json!("uncertain");
                target["mayHaveExecuted"] = V::Bool(true);
            } else {
                need(
                    phase == "prepared"
                        && get(&oo, "fences").as_array().is_some_and(|a| a.is_empty())
                        && get(&oo, "fetchStarts").as_array().is_some_and(|a| a.is_empty())
                        && get(&oo, "receipts").as_array().is_some_and(|a| a.is_empty())
                        && get(&oo, "orderId").is_null()
                        && get(&oo, "ack").is_null()
                        && get(&oo, "mayHaveExecuted") == &V::Bool(false),
                    "KNOWN_UNADMITTED_ERROR_ONLY",
                )?;
            }
            Ok(n)
        }
        "financial_mismatch" => {
            need(
                o.as_ref().is_some() && get(o.as_ref().unwrap(), "proof").is_null(),
                "NO_PENDING",
            )?;
            need(
                sget(e, "reason") == Some("NON_NEUTRAL_POST_ACCOUNT"),
                "FINANCIAL_MISMATCH_REASON",
            )?;
            let oo = o.clone().unwrap();
            let intent_id = get(&oo, "intentId").clone();
            let arr = n["orders"].as_array_mut().unwrap();
            let target = arr
                .iter_mut()
                .find(|x| get(x, "intentId") == &intent_id)
                .unwrap();
            target["semanticMismatch"] = V::Bool(true);
            target["phase"] = json!("semantic_mismatch");
            Ok(n)
        }
        "reconciled" => {
            need(
                o.as_ref().is_some() && get(o.as_ref().unwrap(), "proof").is_null(),
                "NO_PENDING",
            )?;
            let oo = o.clone().unwrap();
            let mut ev = get(e, "evidence").as_object().cloned().unwrap_or_default();
            ev.insert("firstExchangeId".into(), get(&oo, "firstExchangeId").clone());
            empty_all(&V::Object(ev), s, instant_s(get(e, "at"))?)?;
            need(
                truthy(get(&oo, "orderId"))
                    && truthy(get(&oo, "ack"))
                    && get(&oo, "semanticMismatch") == &V::Bool(false)
                    && get(&oo, "phase").as_str() == Some("acknowledged"),
                "NON_NEUTRAL_OR_UNKNOWN_BLOCKS",
            )?;
            let mut binding = get(s, "scope").as_object().cloned().unwrap_or_default();
            binding.insert("intentId".into(), get(&oo, "intentId").clone());
            binding.insert(
                "idempotencyKey".into(),
                get(get(&oo, "body"), "idempotencyKey").clone(),
            );
            binding.insert("orderId".into(), get(&oo, "orderId").clone());
            binding.insert("marketId".into(), get(&oo, "marketId").clone());
            binding.insert("exchangeId".into(), get(get(&oo, "body"), "exchangeId").clone());
            binding.insert("side".into(), json!("no"));
            binding.insert("action".into(), get(get(&oo, "body"), "action").clone());
            binding.insert("quantity".into(), get(get(&oo, "body"), "quantity").clone());
            binding.insert(
                "limit".into(),
                json!(money(price(&js_num_string(get(get(&oo, "body"), "price")))?)),
            );
            binding.insert("createdAt".into(), get(&oo, "createdAt").clone());
            binding.insert(
                "expirationDate".into(),
                get(get(&oo, "body"), "expirationDate").clone(),
            );
            let is_buy = get(get(&oo, "body"), "action").as_str() == Some("buy");
            let mut input = get(e, "evidence").as_object().cloned().unwrap_or_default();
            input.insert("binding".into(), V::Object(binding));
            input.insert("before".into(), get(&oo, "before").clone());
            input.insert("ack".into(), get(&oo, "ack").clone());
            input.insert(
                "dispatchReceipt".into(),
                get(&oo, "fetchStarts")
                    .as_array()
                    .and_then(|a| a.first().cloned())
                    .unwrap_or(V::Null),
            );
            input.insert("readAt".into(), get(e, "at").clone());
            input.insert(
                "reservedFunding".into(),
                json!(if is_buy {
                    money(990_000i128 * js_int(get(get(&oo, "body"), "quantity")).unwrap_or(0) as i128)
                } else {
                    "0.000000".to_string()
                }),
            );
            input.insert("priorProof".into(), get(&oo, "priorProof").clone());
            input.insert("priorAfter".into(), get(&oo, "priorAfter").clone());
            let proof =
                baal_execution::no_rest_multishare_reconciliation_v1::verify_scoped_rest_no_multishare(
                    &V::Object(input),
                )?;
            let ex = get(get(&oo, "body"), "exchangeId").as_str().unwrap_or("").to_string();
            if is_buy {
                let pairs = assign_fill(s, &oo, &proof)?;
                let arr = n["assignments"].as_array_mut().unwrap();
                for pair in pairs {
                    arr.push(pair);
                }
            } else {
                need(
                    amount(sget(&proof, "realizedPnl").unwrap_or(""))?
                        >= 5_000i128 * js_int(get(&proof, "filledQuantity")).unwrap_or(0) as i128,
                    "ACTUAL_NET_PROFIT",
                )?;
                let retirees = retire_sold_pairs(s, &oo, &proof)?;
                let arr = n["pairRetirements"].as_array_mut().unwrap();
                for r in retirees {
                    arr.push(r);
                }
            }
            let seen: HashSet<String> = n["assignments"]
                .as_array()
                .map(|a| a.iter().map(|x| js_stringify(get(x, "assignmentId"))).collect())
                .unwrap_or_default();
            need(
                seen.len() == n["assignments"].as_array().unwrap().len(),
                "DUPLICATE_PAIR",
            )?;
            let intent_id = get(&oo, "intentId").clone();
            let inv_pkg = get(&oo, "package").clone();
            {
                let arr = n["orders"].as_array_mut().unwrap();
                let target = arr
                    .iter_mut()
                    .find(|x| get(x, "intentId") == &intent_id)
                    .unwrap();
                target["proof"] = proof.clone();
                target["phase"] = json!("reconciled");
            }
            let mut slot = Map::new();
            slot.insert("proof".into(), proof.clone());
            slot.insert("after".into(), copy(get(get(e, "evidence"), "after")));
            slot.insert("package".into(), inv_pkg);
            n["currentInventory"]
                .as_object_mut()
                .unwrap()
                .insert(ex.clone(), V::Object(slot));
            let counter_has = n["counterInventory"]
                .as_object()
                .map(|m| m.contains_key(&ex))
                .unwrap_or(false);
            if is_buy || counter_has {
                let cur = get(&n, "currentInventory").get(&ex).cloned().unwrap_or(V::Null);
                n["counterInventory"]
                    .as_object_mut()
                    .unwrap()
                    .insert(ex, copy(&cur));
            }
            Ok(n)
        }
        _ => need(false, "EVENT").map(|_| n),
    }
}

// configuration(seed,activation,migration,code)
fn configuration(seed: &V, activation: &V, migration: &V, code: &V) -> Result<(), String> {
    need(
        get(seed, "economicReplayPerformed") == &V::Bool(true)
            && get(seed, "stateHash").as_str()
                == Some(payload_hash(get(seed, "state"))?.as_str())
            && hex(get(seed, "legacyCodeIdentityHash"))
            && get(migration, "schemaVersion") == &json!(1)
            && get(migration, "seedHash").as_str() == Some(payload_hash(seed)?.as_str())
            && same(get(migration, "legacyAnchor"), get(seed, "anchor"))?
            && get(migration, "legacyCodeIdentityHash") == get(seed, "legacyCodeIdentityHash")
            && get(migration, "implementationEpoch").as_str().is_some_and(|s| !s.is_empty())
            && hex(get(migration, "authorityHash")),
        "VERIFIED_MIGRATION",
    )?;
    let first_ex_list = get(activation, "allowedFirstExchangeIds");
    need(
        get(activation, "schemaVersion") == &json!(1)
            && get(activation, "mode").as_str() == Some("baal_neutral_owned_counterpart_pilot_v2")
            && get(activation, "ownerId") == get(&get(get(seed, "state"), "scope").clone(), "ownerId")
            && get(activation, "runId") == get(&get(get(seed, "state"), "scope").clone(), "runId")
            && get(activation, "tournamentId") == get(&get(get(seed, "state"), "scope").clone(), "tournamentId")
            && get(activation, "seedHash").as_str() == Some(payload_hash(seed)?.as_str())
            && get(activation, "implementationEpoch") == get(migration, "implementationEpoch")
            && get(activation, "codeIdentityHash").as_str() == Some(payload_hash(code)?.as_str())
            && hex(get(activation, "authorityHash"))
            && get(activation, "authorityHash") == get(migration, "authorityHash")
            && num_eq(get(activation, "maximumHedgeOrders"), 1.0)
            && num_eq(get(activation, "quantity"), 1.0)
            && get(activation, "maximumOrderCost").as_str() == Some("0.995000")
            && get(activation, "maximumPairUnitCost").as_str() == Some("0.990000")
            && instant_s(get(activation, "exitExpiresAt"))?
                > instant_s(get(activation, "startedAt"))?
            && instant_s(get(activation, "exitExpiresAt"))?
                - instant_s(get(activation, "startedAt"))?
                <= 86_400_000
            && first_ex_list.is_array()
            && first_ex_list.as_array().unwrap().len() == 1
            && {
                let inv = get(get(seed, "state"), "inventory");
                let slot = get(
                    inv,
                    first_ex_list
                        .as_array()
                        .unwrap()
                        .first()
                        .and_then(|x| x.as_str())
                        .unwrap_or(""),
                );
                get(get(slot, "proof"), "remainingOwnedQuantity")
                    .as_f64()
                    .map_or(false, |q| q > 0.0)
            }
            && instant_s(get(activation, "hedgeExpiresAt"))?
                > instant_s(get(activation, "startedAt"))?
            && instant_s(get(activation, "hedgeExpiresAt"))?
                - instant_s(get(activation, "startedAt"))?
                <= 3_600_000,
        "BOUNDED_NEW_HEDGE_ONLY",
    )?;
    Ok(())
}

/// liveCoordinatorIdentity(workspace)
pub fn live_coordinator_identity(workspace: &Path) -> Result<V, String> {
    Ok(json!(collect_frozen_paper_files(
        workspace,
        &["execution/baal-neutral-owned-counterpart-pilot-v2.mjs".to_string()],
    )?))
}

/// replayLiveHedgeJournal({bodyText,seed,activation,migration,expectedCodeIdentity})
pub fn replay_live_hedge_journal(input: &V) -> Result<V, String> {
    let body_text = get(input, "bodyText").as_str().unwrap_or("").to_string();
    let seed = get(input, "seed");
    let activation = get(input, "activation");
    let migration = get(input, "migration");
    let expected_code_identity = get(input, "expectedCodeIdentity");
    configuration(seed, activation, migration, expected_code_identity)?;
    need(
        !body_text.is_empty()
            && body_text.ends_with('\n')
            && body_text.as_bytes().len() <= 128 * 1024 * 1024,
        "REPLAY_INPUT",
    )?;
    let mut state = V::Null;
    let mut sequence: i64 = 0;
    let mut previous = String::new();
    let mut root_prefix = V::Null;
    let lines: Vec<&str> = body_text.trim_end_matches('\n').split('\n').collect();
    for line in &lines {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let hash = get(&row, "hash").as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().map(|m| m.remove("hash"));
        need(
            js_stringify(&row) == *line
                && js_int(get(&body, "sequence")) == Some(sequence + 1)
                && sget(&body, "previous") == Some(previous.as_str())
                && sha(&js_stringify(&body)) == hash
                && same(
                    get(get(&body, "event"), "codeIdentity"),
                    expected_code_identity,
                )?,
            "CHAIN_CODE",
        )?;
        if sequence == 0 {
            need(
                get(get(&body, "event"), "input").get("kind").map(|k| k.as_str())
                    == Some(Some("seed"))
                    && same(get(get(get(&body, "event"), "input"), "seed"), seed)?
                    && same(get(get(get(&body, "event"), "input"), "activation"), activation)?
                    && same(get(get(get(&body, "event"), "input"), "migration"), migration)?,
                "EXACT_SEED",
            )?;
        }
        sequence += 1;
        state = reduce(&state, get(get(&body, "event"), "input"))?;
        need(same(&state, get(&body, "state"))?, "ECONOMIC_STATE_REPLAY")?;
        previous = hash.clone();
        if sequence == 1 {
            root_prefix = json!({
                "bytes": line.as_bytes().len() + 1,
                "sha256": sha(&format!("{line}\n")),
                "sequence": sequence,
                "hash": hash,
            });
        }
    }
    Ok(json!({
        "state": state,
        "anchor": {
            "bytes": body_text.as_bytes().len(),
            "sha256": sha(&body_text),
            "sequence": sequence,
            "hash": previous,
        },
        "rootPrefix": root_prefix,
        "economicReplayPerformed": true,
        "unknownAcceptance": pilot_unknown(&state),
    }))
}

pub struct NeutralOwnedPilotDeps {
    pub dispatch: Rc<baal_execution::paired_rest_controller::WireDispatch>,
    pub raw_receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub assert_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub assert_financial_owner_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub install_funding_reservation: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_evidence: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub on_financial_halt: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    /// canCheckMechanical=canOperate default in JS; explicit in Rust.
    pub can_check_mechanical: Rc<dyn Fn() -> bool>,
    /// assertMechanicalAccountLease=assertAccountLease default in JS.
    pub assert_mechanical_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    /// assertMechanicalFinancialOwnerLease=assertFinancialOwnerLease default.
    pub assert_mechanical_financial_owner_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct PilotInner {
    scope_dir: PathBuf,
    file: PathBuf,
    legacy_file: PathBuf,
    legacy_bytes: Vec<u8>,
    workspace: PathBuf,
    seed: V,
    seed_hash: String,
    original_inventory_hash: String,
    original_orders_hash: String,
    code: V,
    activation: V,
    migration: V,
    lease: OwnerLease,
    journal: Journal,
    state: V,
    size: usize,
    hash: String,
    stat_ino: u64,
    stat_birth_ms: f64,
    closed: bool,
    failed: bool,
    busy: bool,
    in_flight: Option<V>,
    issued: HashMap<usize, V>,
    last_guard_failure: V,
    fetch_deadline: Option<i64>,
    local_timings: VecDeque<V>,
    deps: NeutralOwnedPilotDeps,
    transport: Option<OrderTransport>,
}

type Shared = Rc<RefCell<PilotInner>>;

fn fatal(e: String) -> String {
    format!("{e}\u{0}fatal\u{0}durableRecordingFailed")
}
fn is_fatal(e: &str) -> bool {
    e.contains("\u{0}fatal\u{0}") || e.contains("durableRecordingFailed")
}

impl PilotInner {
    fn diagnostic_code(e: &str) -> String {
        let re_ok = |s: &str| -> bool {
            let b = s.as_bytes();
            !b.is_empty()
                && b[0].is_ascii_alphabetic()
                && b.len() >= 2
                && b.len() <= 141
                && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
        };
        let s = e.split('\u{0}').next().unwrap_or("");
        if re_ok(s) {
            s.to_uppercase()
        } else {
            "OPERATION_FAILED".to_string()
        }
    }

    fn measure<T>(
        &mut self,
        name: &str,
        f: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let start = Instant::now();
        let mut failure: Option<V> = None;
        let result = f(self);
        let dur = ((start.elapsed().as_secs_f64() * 1000.0) * 1000.0).round() / 1000.0;
        if let Err(e) = &result {
            failure = Some(json!({
                "errorCode": Self::diagnostic_code(e),
                "failedAt": iso((self.deps.now)()),
            }));
        }
        let mut entry = Map::new();
        entry.insert("name".into(), json!(name));
        entry.insert("durationMs".into(), json!(dur));
        if let Some(f) = failure {
            if let V::Object(fm) = f {
                for (k, v) in fm {
                    entry.insert(k, v);
                }
            }
        }
        self.local_timings.push_back(V::Object(entry));
        if self.local_timings.len() > 96 {
            self.local_timings.pop_front();
        }
        result
    }

    fn mechanical(&mut self) -> Result<V, String> {
        need(
            !self.closed && !self.failed && (self.deps.can_check_mechanical)(),
            "STOPPED",
        )?;
        self.lease.assert_ownership().map_err(|e| e.code.to_string())?;
        let l = {
            let f = self.deps.assert_mechanical_account_lease.clone();
            self.measure("basic_account_lease", move |_| f())
        }?;
        let f = {
            let d = self.deps.assert_mechanical_financial_owner_lease.clone();
            self.measure("basic_financial_lease", move |_| d())
        }?;
        for x in [&l, &f] {
            need(
                get(x, "ownerId") == get(&self.activation, "ownerId")
                    && get(x, "runId") == get(&self.activation, "runId")
                    && get(x, "tournamentId") == get(&self.activation, "tournamentId")
                    && get(x, "leaseId").as_str().is_some_and(|s| !s.is_empty()),
                "ACTUAL_ACCOUNT_LEASE",
            )?;
        }
        need(
            get(&f, "implementationEpoch") == get(&self.migration, "implementationEpoch"),
            "FINANCIAL_OWNER_EPOCH",
        )?;
        need(
            l.as_object().map(|m| {
                m.keys()
                    .all(|k| ["ownerId", "runId", "tournamentId", "leaseId"].contains(&k.as_str()))
            }) == Some(true),
            "LEASE_RECEIPT_SCHEMA",
        )?;
        Ok(l)
    }

    fn basic(&mut self) -> Result<V, String> {
        need((self.deps.can_operate)(), "STOPPED")?;
        let l = self.mechanical()?;
        let file = self.legacy_file.clone();
        let actual = self.measure("basic_legacy_fresh_read", move |_| std::fs::read(&file).map_err(|e| e.to_string()))?;
        need(
            actual.len()
                == js_int(get(get(&self.seed, "anchor"), "bytes")).map(|x| x as usize).unwrap_or(0)
                && {
                    let lb = self.legacy_bytes.clone();
                    self.measure("basic_legacy_exact_compare", move |_| Ok(actual == lb))?
                }
                && {
                    let ws = self.workspace.clone();
                    let code = self.code.clone();
                    self.measure("basic_current_code_graph", move |_| {
                        same(&live_coordinator_identity(&ws)?, &code)
                    })?
                },
            "LEGACY_NOT_SEALED_OR_CODE_CHANGED",
        )?;
        need(
            l.as_object().map(|m| {
                m.keys()
                    .all(|k| ["ownerId", "runId", "tournamentId", "leaseId"].contains(&k.as_str()))
            }) == Some(true),
            "LEASE_RECEIPT_SCHEMA",
        )?;
        Ok(l)
    }

    fn append(&mut self, e: &V) -> Result<(), String> {
        self.basic()?;
        let old = std::fs::read(&self.file).map_err(|x| x.to_string())?;
        let st = std::fs::metadata(&self.file).map_err(|x| x.to_string())?;
        need(
            st.ino() == self.stat_ino
                && st.created().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs_f64() * 1000.0)
                    .unwrap_or(0.0) == self.stat_birth_ms
                && old.len() == self.size
                && sha_bytes(&old) == self.hash,
            "JOURNAL_CHANGED",
        )?;
        let next = reduce(&self.state, e)?;
        need(
            self.size + js_stringify(&next).as_bytes().len() + js_stringify(e).as_bytes().len()
                < 128 * 1024 * 1024,
            "JOURNAL_BOUND",
        )?;
        // Only the commit/verify block is covered by the JS try/catch that
        // marks failures durable+fatal; reduce() errors above propagate plain.
        let post: Result<(), String> = (|| {
            self.journal.commit(&next, &json!({"codeIdentity": self.code, "input": e}))?;
            self.basic()?;
            let after = std::fs::read(&self.file).map_err(|x| x.to_string())?;
            need(after[..self.size] == old[..], "PREFIX_CHANGED")?;
            self.state = next;
            self.size = after.len();
            self.hash = sha_bytes(&after);
            Ok(())
        })();
        match post {
            Ok(()) => Ok(()),
            Err(m) => {
                self.failed = true;
                Err(fatal(m))
            }
        }
    }
}

impl PilotInner {
    fn evidence(&mut self, operation: &str, first_ex: Option<&str>, fence_risk: &V) -> Result<V, String> {
        let o = active(&self.state);
        let intent_id = o.as_ref().map(|x| get(x, "intentId").clone()).unwrap_or(V::Null);
        let order_id = o.as_ref().map(|x| get(x, "orderId").clone()).unwrap_or(V::Null);
        let o_first = o.as_ref().map(|x| get(x, "firstExchangeId").clone()).unwrap_or(V::Null);
        let first_exchange_id = match first_ex {
            Some(f) => json!(f),
            None => o_first,
        };
        let exchange_id = {
            let b = o.as_ref().map(|x| get(get(x, "body"), "exchangeId").clone()).unwrap_or(V::Null);
            if b.is_null() { first_exchange_id.clone() } else { b }
        };
        let market_id = {
            let m = o.as_ref().map(|x| get(x, "marketId").clone()).unwrap_or(V::Null);
            if m.is_null() {
                let ex_str = first_exchange_id.as_str().unwrap_or("").to_string();
                get(get(&self.state, "scope"), "exchanges")
                    .as_array()
                    .and_then(|xs| {
                        xs.iter()
                            .find(|x| get(x, "exchangeId").as_str() == Some(ex_str.as_str()))
                            .map(|x| get(x, "marketId").clone())
                    })
                    .unwrap_or(V::Null)
            } else {
                m
            }
        };
        (self.deps.get_evidence)(&json!({
            "operation": operation,
            "fenceRisk": fence_risk,
            "intentId": intent_id,
            "orderId": order_id,
            "firstExchangeId": first_exchange_id,
            "exchangeId": exchange_id,
            "marketId": market_id,
        }))
    }

    fn validate(&mut self, permit: &V, spec: &V, network: bool) -> Result<bool, String> {
        self.mechanical()?;
        let o = active(&self.state);
        let ov = o.clone().unwrap_or(V::Null);
        need(
            o.is_some()
                && get(permit, "intentId") == get(&ov, "intentId")
                && get(permit, "bodyHash") == get(&ov, "bodyHash")
                && get(spec, "method").as_str() == Some("POST")
                && get(spec, "endpoint").as_str() == Some("/orders")
                && get(spec, "bodyJson") == get(&ov, "bodyJson")
                && !pilot_unknown(&self.state),
            "HEDGE_ONLY_FIXED_PERMIT",
        )?;
        let r = get(&self.state, "reservation");
        need(
            get(r, "reservationHash") == get(&funding(&self.state)?, "reservationHash")
                && get(r, "allocationHash").as_str()
                    == Some(payload_hash(get(&self.state, "allocation"))?.as_str())
                && hex(get(r, "globalLedgerHash"))
                && instant_s(get(r, "confirmedAt"))? as f64 <= (self.deps.now)()
                && (self.deps.now)() - instant_s(get(r, "confirmedAt"))? as f64 <= 20000.0,
            "FULL_RESERVE_INSTALLED",
        )?;
        let before_fence = json!({
            "recordHash": get(&funding(&self.state)?, "reservationHash"),
            "bodyHash": get(&ov, "bodyHash"),
            "bodyJson": get(&ov, "bodyJson"),
            "custodyHash": payload_hash(get(&self.state, "currentInventory"))?,
            "policyHash": payload_hash(&json!({
                "scope": get(&self.state, "scope"),
                "allocation": get(&self.state, "allocation"),
                "activation": get(&self.state, "activation"),
                "migration": get(&self.state, "migration"),
            }))?,
            "unknown": pilot_unknown(&self.state),
        });
        let l = self.measure("guard_current_lease", |s_| s_.basic())?;
        let g = self.measure(
            if network { "network_current_budget_risk" } else { "admission_current_budget_risk" },
            |s_| (s_.deps.get_global_risk)(),
        )?;
        let action = get(get(&ov, "body"), "action").as_str().unwrap_or("").to_string();
        let e = self.measure(
            if network { "network_cached_evidence" } else { "admission_cached_evidence" },
            |s_| {
                let g2 = g.clone();
                s_.evidence(
                    if action == "buy" { "place_hedge" } else { "place_sell" },
                    None,
                    &g2,
                )
            },
        )?;
        self.measure(
            if network { "network_durable_fence_append" } else { "admission_durable_event_append" },
            |s_| {
                let lease_hash = payload_hash(&l)?;
                let e2 = e.clone();
                let g2 = g.clone();
                let l2 = l.clone();
                s_.append(&json!({
                    "kind": if network { "network_fence" } else { "admitted" },
                    "evidence": e2,
                    "globalRisk": g2,
                    "lease": l2,
                    "leaseHash": lease_hash,
                    "at": iso((s_.deps.now)()),
                }))
            },
        )?;
        if network {
            let o2 = active(&self.state).unwrap_or(V::Null);
            let l2 = self.measure("network_postappend_current_lease", |s_| s_.basic())?;
            need(same(&l2, &l)?, "NETWORK_LEASE_CHANGED_AFTER_COMMIT")?;
            self.measure("network_postappend_current_journal", |s_| {
                let st = std::fs::metadata(&s_.file).map_err(|x| x.to_string())?;
                let fresh = std::fs::read(&s_.file).map_err(|x| x.to_string())?;
                let last_nl = fresh[..fresh.len().saturating_sub(1)]
                    .iter()
                    .rposition(|&b| b == 10)
                    .map(|i| i + 1)
                    .unwrap_or(0);
                let last: V = serde_json::from_slice(&fresh[last_nl..]).map_err(|x| x.to_string())?;
                need(
                    st.ino() == s_.stat_ino
                        && st.created()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs_f64() * 1000.0)
                            .unwrap_or(0.0)
                            == s_.stat_birth_ms
                        && fresh.len() == s_.size
                        && sha_bytes(&fresh) == s_.hash
                        && num_eq(get(&last, "sequence"), s_.journal.sequence as f64)
                        && get(&last, "hash").as_str() == Some(s_.journal.previous.as_str()),
                    "CURRENT_JOURNAL_CHANGED",
                )?;
                Ok(())
            })?;
            need(
                get(&before_fence, "recordHash") == get(&funding(&self.state)?, "reservationHash")
                    && get(&before_fence, "bodyHash") == get(&o2, "bodyHash")
                    && get(&before_fence, "bodyJson") == get(&o2, "bodyJson")
                    && get(&o2, "bodyHash").as_str()
                        == Some(payload_hash(get(&o2, "body"))?.as_str())
                    && get(&before_fence, "custodyHash").as_str()
                        == Some(payload_hash(get(&self.state, "currentInventory"))?.as_str())
                    && get(&before_fence, "policyHash").as_str()
                        == Some(
                            payload_hash(&json!({
                                "scope": get(&self.state, "scope"),
                                "allocation": get(&self.state, "allocation"),
                                "activation": get(&self.state, "activation"),
                                "migration": get(&self.state, "migration"),
                            }))?
                            .as_str(),
                        )
                    && get(&before_fence, "unknown") == &V::Bool(pilot_unknown(&self.state)),
                "NON_ECONOMIC_FENCE_ONLY",
            )?;
            need(
                (self.deps.now)() < instant_s(get(get(&o2, "body"), "expirationDate"))? as f64,
                "ORDER_EXPIRED_AFTER_FENCE",
            )?;
            self.measure("network_postappend_policy_validation", |s_| {
                let g3 = g.clone();
                let e3 = e.clone();
                let now = (s_.deps.now)();
                if get(get(&o2, "body"), "action").as_str() == Some("buy") {
                    guarded(&s_.state.clone(), &e3, &g3, now as i64).map(|_| ())
                } else {
                    sell_proposal(
                        &s_.state.clone(),
                        sget(get(&o2, "body"), "exchangeId").unwrap_or(""),
                        &e3,
                        &g3,
                        now as i64,
                    )
                    .map(|_| ())
                }
            })?;
            self.fetch_deadline = Some(
                instant_s(get(get(&o2, "body"), "expirationDate"))?
                    .min(instant_s(get(get(&e, "graphReceipt"), "requestStartedAt"))? + 5000)
                    .min(instant_s(get(get(&e, "collateralReceipt"), "requestStartedAt"))? + 5000),
            );
        }
        Ok(true)
    }
}


impl PilotInner {
    // capture(response): raw receipt binding + wire event + unknown-halt hook.
    fn capture(&mut self, response: &V) -> Result<(), String> {
        let o = active(&self.state).unwrap_or(V::Null);
        let body_hash = get(&o, "bodyHash").clone();
        let r = (self.deps.raw_receipt_of)(
            response,
            &json!({"method": "POST", "requestBodyHash": body_hash}),
        )?;
        let status_eq = match (get(&r, "status").as_f64(), get(response, "status").as_f64()) {
            (Some(a), Some(b)) => a == b,
            _ => get(&r, "status") == get(response, "status"),
        };
        need(
            status_eq
                && get(&r, "endpoint").as_str() == Some("/orders")
                && instant_s(get(&r, "requestStartedAt"))?
                    >= instant_s(get(response, "requestStartedAt"))?
                && instant_s(get(&r, "receivedAt"))?
                    <= instant_s(get(response, "receivedAt"))?
                && same(
                    &parse_number_lexemes(sget(&r, "bodyText").unwrap_or(""))?.0,
                    get(response, "data"),
                )?,
            "RAW_RESPONSE_BINDING",
        )?;
        self.append(&json!({"kind": "wire", "receipt": r, "at": iso((self.deps.now)())}))?;
        if pilot_unknown(&self.state) {
            let arg = json!({
                "kind": "non_neutral_or_unknown_hedge",
                "intentId": get(&o, "intentId"),
                "sourceHash": get(&r, "sourceBodyHash"),
                "allWritersStop": true,
            });
            (self.deps.on_financial_halt)(&arg)?;
        }
        Ok(())
    }

    fn guarded_permit(&mut self, p: &V, s: &V, network: bool) -> Result<bool, OrderTransportError> {
        match self.validate(p, s, network) {
            Ok(b) => Ok(b),
            Err(e) => {
                let fatal_flag = is_fatal(&e);
                self.last_guard_failure = json!({
                    "stage": if network { "network" } else { "admission" },
                    "errorCode": Self::diagnostic_code(&e),
                    "failedAt": iso((self.deps.now)()),
                });
                if fatal_flag {
                    return Err(OrderTransportError {
                        code: e.split('\u{0}').next().unwrap_or("").to_string(),
                        status: None,
                        api_code: None,
                        outcome: "unknown".into(),
                        may_have_executed: true,
                        retry_same_request_only: false,
                        retry_after_ms: 0.0,
                        semantic_mismatch: false,
                    });
                }
                Err(OrderTransportError {
                    code: "OWNED_CUSTODY_DISPATCH_GUARD".to_string(),
                    status: None,
                    api_code: None,
                    outcome: "blocked".into(),
                    may_have_executed: false,
                    retry_same_request_only: false,
                    retry_after_ms: 0.0,
                    semantic_mismatch: false,
                })
            }
        }
    }
}

fn wrap_dispatch(
    inner: &Shared,
    spec: &V,
    bd: Option<&Guard<'_>>,
    bn: Option<&Guard<'_>>,
) -> Result<V, baal_execution::paired_rest_controller::WireDispatchError> {
    let bd_ptr = bd.map(|g| g as *const Guard as *const () as usize).unwrap_or(0);
    let bn_ptr = bn.map(|g| g as *const Guard as *const () as usize).unwrap_or(0);
    {
        let mut g = inner.borrow_mut();
        g.issued.insert(
            bd_ptr,
            json!({
                "method": get(spec, "method"),
                "endpoint": get(spec, "endpoint"),
                "bodyJson": get(spec, "bodyJson"),
                "beforeNetwork": get(spec, "beforeNetwork"),
            }),
        );
        g.in_flight = Some(spec.clone());
    }
    let dispatch = inner.borrow().deps.dispatch.clone();
    let result = dispatch(spec, bd, bn);
    let out: Result<V, String> = match result {
        Ok(r) => {
            inner.borrow_mut().capture(&r).map(|_| r)
        }
        Err(e) => {
            if let Some(wr) = e.wire_receipt.clone() {
                inner.borrow_mut().capture(&wr).map_err(|message| {
                    baal_execution::paired_rest_controller::WireDispatchError {
                        message,
                        wire_receipt: None,
                    }
                })?;
            }
            Err(e.message)
        }
    };
    {
        let mut g = inner.borrow_mut();
        g.issued.remove(&bd_ptr);
        g.in_flight = None;
    }
    out.map_err(|message| baal_execution::paired_rest_controller::WireDispatchError {
        message,
        wire_receipt: None,
    })
}

impl PilotInner {
    fn reserve(&mut self) -> Result<V, String> {
        let f = funding(&self.state)?;
        let step: Result<V, String> = (|| {
            let r = (self.deps.install_funding_reservation)(&f)?;
            self.append(&json!({"kind":"reservation","receipt":r,"at":iso((self.deps.now)())}))?;
            Ok(r)
        })();
        match step {
            Ok(r) => Ok(r),
            Err(m) => {
                self.failed = true;
                Err(fatal(m))
            }
        }
    }

    fn ensure_installed(&mut self) -> Result<V, String> {
        self.basic()?;
        let record = funding(&self.state)?;
        let r = get(&self.state, "reservation").clone();
        let mut valid = get(&r, "reservationHash") == get(&record, "reservationHash")
            && get(&r, "allocationHash") == get(&record, "allocationHash")
            && get(&r, "seedHash").as_str() == Some(self.seed_hash.as_str())
            && hex(get(&r, "globalLedgerHash"))
            && hex(get(&r, "budgetInstalledHash"))
            && instant_s(get(&r, "confirmedAt"))? as f64 <= (self.deps.now)()
            && (self.deps.now)() - instant_s(get(&r, "confirmedAt"))? as f64 <= 20000.0;
        if valid {
            let g = (self.deps.get_global_risk)()?;
            let o = active(&self.state);
            let intent = o.as_ref().map(|x| get(x, "intentId").clone()).unwrap_or(V::Null);
            valid = get(&g, "ownerId") == get(get(&self.state, "scope"), "ownerId")
                && get(&g, "runId") == get(get(&self.state, "scope"), "runId")
                && get(&g, "tournamentId") == get(get(&self.state, "scope"), "tournamentId")
                && get(&g, "complete") == &V::Bool(true)
                && get(&g, "eventScopeComplete") == &V::Bool(true)
                && get(&g, "soleCollector") == &V::Bool(true)
                && get(&g, "stopped") == &V::Bool(false)
                && get(&g, "unknownAcceptance") == &V::Bool(false)
                && hex(get(&g, "sourceHash"))
                && get(&g, "unresolvedIntentIds").is_array()
                && get(&g, "unresolvedIntentIds")
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|id| id == &intent)
                && get(&g, "controllerReservationHash") == get(&record, "reservationHash")
                && get(&g, "globalLedgerHash") == get(&r, "globalLedgerHash")
                && get(&g, "budgetInstalledHash") == get(&r, "budgetInstalledHash")
                && get(&g, "allocationHash") == get(&record, "allocationHash")
                && get(&g, "seedHash").as_str() == Some(self.seed_hash.as_str())
                && get(&g, "implementationEpoch") == get(&self.migration, "implementationEpoch")
                && get(&g, "fundingSatisfied") == &V::Bool(true)
                && get(&g, "installedCashLower").is_string()
                && get(&g, "cashLower") == get(&g, "installedCashLower")
                && instant_s(get(&g, "at"))? as f64 <= (self.deps.now)()
                && (self.deps.now)() - instant_s(get(&g, "at"))? as f64 <= 20000.0;
        }
        if !valid {
            return self.reserve();
        }
        Ok(copy(&r))
    }

    fn run<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        need(!self.busy, "BUSY")?;
        self.busy = true;
        let r = f(self);
        self.busy = false;
        r
    }

    fn active_intent_status(&self) -> V {
        match active(&self.state) {
            None => V::Null,
            Some(o) => json!({
                "intentId": get(&o, "intentId"),
                "firstExchangeId": get(&o, "firstExchangeId"),
                "orderId": get(&o, "orderId"),
                "action": get(get(&o, "body"), "action"),
                "marketId": get(&o, "marketId"),
                "exchangeId": get(get(&o, "body"), "exchangeId"),
                "body": copy(get(&o, "body")),
                "bodyHash": get(&o, "bodyHash"),
                "phase": get(&o, "phase"),
                "createdAt": get(&o, "createdAt"),
                "packageLegs": copy(get(get(&o, "package"), "legs")),
                "actualFetchCount": get(&o, "fetchStarts").as_array().map(|a| a.len() as i64).unwrap_or(0),
                "mayHaveExecuted": get(&o, "mayHaveExecuted"),
                "cancelPending": false,
            }),
        }
    }

    fn status(&self) -> Result<V, String> {
        let o = active(&self.state);
        let phase = match &o {
            Some(x) => get(x, "phase").clone(),
            None => {
                if get(&self.state, "orders").as_array().is_some_and(|a| !a.is_empty()) {
                    json!("terminal")
                } else {
                    json!("ready")
                }
            }
        };
        let mut inv = Map::new();
        if let V::Object(m) = get(&self.state, "currentInventory") {
            for (ex, x) in m {
                inv.insert(
                    ex.clone(),
                    json!({
                        "quantity": get(get(x, "proof"), "remainingOwnedQuantity"),
                        "cost": get(get(x, "proof"), "remainingOwnedCost"),
                        "realizedPnl": get(get(x, "proof"), "cumulativeRealizedPnl"),
                    }),
                );
            }
        }
        let realized: i128 = if let V::Object(m) = get(&self.state, "currentInventory") {
            m.values()
                .map(|x| {
                    amount(sget(get(x, "proof"), "cumulativeRealizedPnl").unwrap_or(""))
                })
                .collect::<Result<Vec<i128>, _>>()?
                .iter()
                .sum()
        } else {
            0
        };
        let mut out = Map::new();
        out.insert("mode".into(), get(&self.state, "mode").clone());
        if let V::Object(sc) = get(&self.state, "scope") {
            for (k, v) in sc {
                out.insert(k.clone(), v.clone());
            }
        }
        out.insert("seedHash".into(), json!(self.seed_hash));
        out.insert("implementationEpoch".into(), get(&self.migration, "implementationEpoch").clone());
        out.insert("sequence".into(), json!(self.journal.sequence));
        out.insert("hash".into(), json!(self.journal.previous));
        out.insert("phase".into(), phase);
        out.insert("intent".into(), self.active_intent_status());
        out.insert("unknownAcceptance".into(), V::Bool(pilot_unknown(&self.state)));
        out.insert("assignments".into(), copy(get(&self.state, "assignments")));
        out.insert("pairRetirements".into(), copy(get(&self.state, "pairRetirements")));
        out.insert("inventory".into(), V::Object(inv));
        out.insert("realizedPnl".into(), json!(money(realized)));
        out.insert("matchedQuantity".into(), json!(matched(&self.state)));
        out.insert("originalInventoryHash".into(), json!(self.original_inventory_hash));
        out.insert("originalOrdersHash".into(), json!(self.original_orders_hash));
        out.insert("funding".into(), funding(&self.state)?);
        out.insert(
            "competitionOrders".into(),
            json!(get(&self.state, "orders")
                .as_array()
                .map(|os| os.iter().filter(|o| !get(o, "fetchStarts").as_array().map(|a| a.is_empty()).unwrap_or(true)).count())
                .unwrap_or(0)),
        );
        out.insert(
            "hedgeIntentCount".into(),
            json!(get(&self.state, "orders")
                .as_array()
                .map(|os| os.iter().filter(|o| get(get(o, "body"), "action").as_str() == Some("buy")).count())
                .unwrap_or(0)),
        );
        out.insert(
            "abandonedIntentCount".into(),
            json!(get(&self.state, "orders")
                .as_array()
                .map(|os| os.iter().filter(|o| get(o, "phase").as_str() == Some("abandoned")).count())
                .unwrap_or(0)),
        );
        out.insert("executionOwnerEpoch".into(), get(&self.migration, "implementationEpoch").clone());
        out.insert("ownedSellSupported".into(), V::Bool(true));
        out.insert("handbackRequiresExplicitImplementationMigration".into(), V::Bool(true));
        out.insert("cashProfitEstablished".into(), V::Bool(false));
        out.insert("recentLocalTimings".into(), json!(self.local_timings.iter().cloned().collect::<Vec<V>>()));
        out.insert("lastGuardFailure".into(), copy(&self.last_guard_failure));
        out.insert("closed".into(), V::Bool(self.closed));
        out.insert("failed".into(), V::Bool(self.failed));
        out.insert("busy".into(), V::Bool(self.busy));
        Ok(V::Object(out))
    }

    fn journal_source(&self) -> Result<V, String> {
        let bytes = std::fs::read(&self.file).map_err(|e| e.to_string())?;
        let nl = bytes.iter().position(|&b| b == 10).unwrap_or(bytes.len() - 1) + 1;
        let first = &bytes[..nl];
        let row: V = serde_json::from_slice(first).map_err(|e| e.to_string())?;
        Ok(json!({
            "journalFile": self.file.to_string_lossy(),
            "record": funding(&self.state)?,
            "sequence": self.journal.sequence,
            "hash": self.journal.previous,
            "rootPrefix": {
                "bytes": first.len(),
                "sha256": sha_bytes(first),
                "sequence": get(&row, "sequence"),
                "hash": get(&row, "hash"),
            },
            "headPrefix": {
                "bytes": self.size,
                "sha256": self.hash,
                "sequence": self.journal.sequence,
                "hash": self.journal.previous,
            },
            "status": self.status()?,
        }))
    }
}

/// openBaalNeutralOwnedCounterpartPilotV2({dir,workspace,legacyFile,legacyCodeIdentity,
///   legacyScope,legacyAllocation,activation,migration,...deps})
pub fn open_baal_neutral_owned_counterpart_pilot_v2(
    dir: &Path,
    workspace: &Path,
    legacy_file: &Path,
    legacy_code_identity: &V,
    legacy_scope: &V,
    legacy_allocation: &V,
    activation: &V,
    migration: &V,
    deps: NeutralOwnedPilotDeps,
) -> Result<NeutralOwnedPilotV2, String> {
    need(dir.is_absolute() && workspace.is_absolute() && legacy_file.is_absolute(), "DEPENDENCIES")?;
    let bytes = std::fs::read(legacy_file).map_err(|e| e.to_string())?;
    let seed = crate::baal_scaled_controller_legacy_replay_v1::replay_baal_scaled_controller_v1(
        &json!({
            "bodyText": String::from_utf8_lossy(&bytes),
            "expectedCodeIdentity": legacy_code_identity,
            "expectedScope": legacy_scope,
            "expectedAllocation": legacy_allocation,
        }),
    )?;
    let seed_hash = payload_hash(&seed)?;
    let original_inventory_hash = payload_hash(get(get(&seed, "state"), "inventory"))?;
    let original_orders_hash = payload_hash(get(get(&seed, "state"), "orders"))?;
    let code = live_coordinator_identity(workspace)?;
    configuration(&seed, activation, migration, &code)?;
    let file = dir.join("neutral-owned-counterpart-pilot-v2.jsonl");
    // Lease + journal bootstrap.
    let owner_id = sget(activation, "ownerId").unwrap_or("").to_string();
    let run_id = sget(activation, "runId").unwrap_or("").to_string();
    let now_ms = (deps.now)();
    let lease = acquire_owner_lease(
        dir,
        &owner_id,
        &run_id,
        "neutral-owned-counterpart-pilot",
        now_ms,
    )
    .map_err(|e| e.code.to_string())?;
    let mut inner = PilotInner {
        scope_dir: dir.to_path_buf(),
        file: file.clone(),
        legacy_file: legacy_file.to_path_buf(),
        legacy_bytes: bytes,
        workspace: workspace.to_path_buf(),
        seed: seed.clone(),
        seed_hash,
        original_inventory_hash,
        original_orders_hash,
        code,
        activation: activation.clone(),
        migration: migration.clone(),
        lease,
        journal: Journal::open(&file).map_err(|e| e.to_string())?,
        state: V::Null,
        size: 0,
        hash: String::new(),
        stat_ino: 0,
        stat_birth_ms: 0.0,
        closed: false,
        failed: false,
        busy: false,
        in_flight: None,
        issued: HashMap::new(),
        last_guard_failure: V::Null,
        fetch_deadline: None,
        local_timings: VecDeque::new(),
        deps,
        transport: None,
    };
    // try{...} catch{release lease; rethrow}
    let boot: Result<(), String> = (|| {
        inner.basic()?;
        if !file.exists() {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&file)
                .map(|f| drop(f))
                .map_err(|e| e.to_string())?;
        }
        let data = std::fs::read(&file).map_err(|e| e.to_string())?;
        need(data.is_empty() || data.last() == Some(&10), "PARTIAL_JOURNAL")?;
        inner.journal = Journal::open(&file).map_err(|e| e.to_string())?;
        if !data.is_empty() {
            let act = inner.activation.clone();
            let mig = inner.migration.clone();
            let sd = inner.seed.clone();
            let code_id = inner.code.clone();
            inner.state = replay_live_hedge_journal(&json!({
                "bodyText": String::from_utf8_lossy(&data),
                "seed": sd,
                "activation": act,
                "migration": mig,
                "expectedCodeIdentity": code_id,
            }))?
            .get("state")
            .cloned()
            .unwrap_or(V::Null);
        }
        inner.size = data.len();
        inner.hash = sha_bytes(&data);
        let st = std::fs::metadata(&file).map_err(|e| e.to_string())?;
        inner.stat_ino = st.ino();
        inner.stat_birth_ms = st
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        Ok(())
    })();
    if let Err(e) = boot {
        inner.lease.release().map_err(|x| x.to_string())?;
        return Err(e);
    }
    if inner.state.is_null() {
        let seed = inner.seed.clone();
        let sh = inner.seed_hash.clone();
        let act = inner.activation.clone();
        let mig = inner.migration.clone();
        inner.append(&json!({
            "kind": "seed",
            "seed": seed,
            "seedHash": sh,
            "activation": act,
            "migration": mig,
            "at": iso((inner.deps.now)()),
        }))?;
    }
    let inner = Rc::new(RefCell::new(inner));
    let transport = {
        let inner_w = inner.clone();
        let wrap: Box<baal_execution::transport::Dispatch> = Box::new(move |spec, bd, bn| {
            wrap_dispatch(&inner_w, spec, bd, bn).map_err(|e| OrderTransportError {
                code: e.message,
                outcome: "unknown".into(),
                may_have_executed: true,
                retry_same_request_only: false,
                retry_after_ms: 0.0,
                status: None,
                api_code: None,
                semantic_mismatch: false,
            })
        });
        let scope_v = inner.borrow().state.get("scope").cloned().unwrap_or(V::Null);
        OrderTransport::new(OrderTransportOptions {
            dispatch: wrap,
            scope: baal_execution::transport::scope_of(&scope_v).map_err(|e| e.code)?,
            can_write: Box::new({
                let inner = inner.clone();
                move || {
                    let mut g = inner.borrow_mut();
                    if g.mechanical().is_err() {
                        return false;
                    }
                    !pilot_unknown(&g.state)
                }
            }),
            owns_order: Box::new({
                let inner = inner.clone();
                move |oid: &str| {
                    active(&inner.borrow().state)
                        .map(|o| get(&o, "orderId").as_str() == Some(oid))
                        .unwrap_or(false)
                }
            }),
            validate_permit: Box::new({
                let inner = inner.clone();
                move |p: &V, s: &V| inner.borrow_mut().guarded_permit(p, s, false)
            }),
            validate_network_permit: Box::new({
                let inner = inner.clone();
                move |p: &V, s: &V| inner.borrow_mut().guarded_permit(p, s, true)
            }),
            validate_lease: Box::new({
                let inner = inner.clone();
                move |l: &V, _s: &V| {
                    inner
                        .borrow_mut()
                        .mechanical()
                        .and_then(|b| same(l, &b))
                        .map_err(|e| OrderTransportError {
                            code: e.split('\u{0}').next().unwrap_or("").to_string(),
                            outcome: "unknown".into(),
                            may_have_executed: true,
                            retry_same_request_only: false,
                            retry_after_ms: 0.0,
                            status: None,
                            api_code: None,
                            semantic_mismatch: false,
                        })
                }
            }),
            now: Box::new({
                let inner = inner.clone();
                move || (inner.borrow().deps.now)() as i64
            }),
            max_pages: 10,
        })
        .map_err(|e| e.code)?
    };
    inner.borrow_mut().transport = Some(transport);
    Ok(NeutralOwnedPilotV2 { inner })
}

pub struct NeutralOwnedPilotV2 {
    inner: Shared,
}

impl NeutralOwnedPilotV2 {
    pub fn status(&self) -> Result<V, String> {
        self.inner.borrow().status()
    }

    pub fn active_intent_status(&self) -> V {
        self.inner.borrow().active_intent_status()
    }

    pub fn authorizes(
        &self,
        spec: &V,
        before_dispatch: Option<&Guard<'_>>,
        before_network: Option<&Guard<'_>>,
    ) -> bool {
        let bd_ptr =
            before_dispatch.map(|x| x as *const Guard as *const () as usize).unwrap_or(0);
        let bn_ptr =
            before_network.map(|x| x as *const Guard as *const () as usize).unwrap_or(0);
        self.authorizes_ptrs(spec, bd_ptr, bn_ptr)
    }

    /// authorizes resolved by guard-pointer values (usize data addresses).
    pub fn authorizes_ptrs(&self, spec: &V, bd_ptr: usize, bn_ptr: usize) -> bool {
        let r: Result<bool, String> = (|| {
            self.inner.borrow_mut().mechanical()?;
            let g = self.inner.borrow();
            let p = g.issued.get(&bd_ptr).cloned();
            Ok(!pilot_unknown(&g.state)
                && p.is_some()
                && get(p.as_ref().unwrap(), "method") == get(spec, "method")
                && get(p.as_ref().unwrap(), "endpoint") == get(spec, "endpoint")
                && get(p.as_ref().unwrap(), "bodyJson") == get(spec, "bodyJson")
                && get(p.as_ref().unwrap(), "beforeNetwork") == &json!(bn_ptr)
                && g.transport
                    .as_ref()
                    .map(|t| t.authorize_request(spec).unwrap_or(false))
                    .unwrap_or(false))
        })();
        r.unwrap_or(false)
    }

    pub fn on_before_fetch(&self, meta: &V) -> Result<V, String> {
        let mut g = self.inner.borrow_mut();
        g.mechanical()?;
        let o = active(&g.state).unwrap_or(V::Null);
        let in_flight_ok = g.in_flight.is_some()
            && get(g.in_flight.as_ref().unwrap(), "bodyJson") == get(&o, "bodyJson");
        need(
            in_flight_ok
                && !o.is_null()
                && get(meta, "method").as_str() == Some("POST")
                && get(meta, "endpoint").as_str() == Some("/orders")
                && get(meta, "requestBodyHash") == get(&o, "bodyHash")
                && g.fetch_deadline.is_some()
                && (g.deps.now)() < g.fetch_deadline.unwrap() as f64,
            "FINAL_FETCH_CLOCK_OR_BINDING",
        )?;
        Ok(json!({"notAfter": g.fetch_deadline.unwrap()}))
    }

    pub fn on_fetch_start(&self, meta: &V) -> Result<bool, String> {
        if get(meta, "method").as_str() == Some("GET") || self.inner.borrow().in_flight.is_none() {
            return Ok(false);
        }
        let r: Result<bool, String> = (|| {
            let mut g = self.inner.borrow_mut();
            g.basic()?;
            let o = active(&g.state).unwrap_or(V::Null);
            let f = get(&o, "fences").as_array().and_then(|a| a.last().cloned());
            let in_flight_ok = g.in_flight.is_some()
                && get(g.in_flight.as_ref().unwrap(), "bodyJson") == get(&o, "bodyJson");
            need(
                f.is_some()
                    && get(meta, "method").as_str() == Some("POST")
                    && get(meta, "endpoint").as_str() == Some("/orders")
                    && get(meta, "requestBodyHash") == get(&o, "bodyHash")
                    && get(meta, "fetchInvocationId").is_string()
                    && in_flight_ok,
                "BOUND_ACTUAL_FETCH",
            )?;
            let f = f.unwrap();
            let mut m = meta.as_object().cloned().unwrap_or_default();
            m.insert("kind".into(), json!("place"));
            m.insert("ownerId".into(), get(get(&g.state, "scope"), "ownerId").clone());
            m.insert("runId".into(), get(get(&g.state, "scope"), "runId").clone());
            m.insert("tournamentId".into(), get(get(&g.state, "scope"), "tournamentId").clone());
            m.insert("intentId".into(), get(&o, "intentId").clone());
            m.insert("idempotencyKey".into(), get(get(&o, "body"), "idempotencyKey").clone());
            m.insert("requestBodyJson".into(), get(&o, "bodyJson").clone());
            m.insert("requestBodyHash".into(), get(&o, "bodyHash").clone());
            m.insert("leaseIdentity".into(), get(&f, "lease").clone());
            m.insert("leaseHash".into(), get(&f, "leaseHash").clone());
            let at = iso((g.deps.now)());
            g.append(&json!({
                "kind": "fetch_start",
                "meta": V::Object(m),
                "at": at,
            }))?;
            Ok(true)
        })();
        match r {
            Ok(b) => Ok(b),
            Err(m) => {
                let mut g = self.inner.borrow_mut();
                g.failed = true;
                Err(fatal(m))
            }
        }
    }

    pub fn journal_source(&self) -> Result<V, String> {
        self.inner.borrow().journal_source()
    }

    pub fn abandon_prepared(&self) -> Result<V, String> {
        let r = self.inner.clone();
        let mut g = r.borrow_mut();
        g.run(|g| {
            g.basic()?;
            need(g.in_flight.is_none(), "ABANDON_IN_FLIGHT")?;
            g.append(&json!({"kind":"abandoned_prepared","at":iso((g.deps.now)())}))?;
            g.reserve()?;
            g.status()
        })
    }

    pub fn inventory(&self, ex: &str) -> V {
        copy(opt_v(get(&self.inner.borrow().state, "currentInventory"), ex))
    }

    pub fn order_binding(&self, id: &V) -> V {
        let g = self.inner.borrow();
        let id_s = js_num_string(id);
        match active(&g.state) {
            None => V::Null,
            Some(o) if get(&o, "orderId").as_str() != Some(id_s.as_str()) => V::Null,
            Some(o) => {
                let mut out = g
                    .active_intent_status()
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                if let V::Object(sc) = get(&g.state, "scope") {
                    for (k, v) in sc {
                        out.insert(k.clone(), v.clone());
                    }
                }
                out.insert("idempotencyKey".into(), get(get(&o, "body"), "idempotencyKey").clone());
                out.insert("side".into(), json!("no"));
                out.insert("quantity".into(), get(get(&o, "body"), "quantity").clone());
                out.insert(
                    "limit".into(),
                    json!(money(
                        price(&js_num_string(get(get(&o, "body"), "price"))).unwrap_or(0)
                    )),
                );
                out.insert(
                    "expirationDate".into(),
                    get(get(&o, "body"), "expirationDate").clone(),
                );
                V::Object(out)
            }
        }
    }

    pub fn prepare_sell(&self, ex: &str, requested_quantity: Option<&V>) -> Result<V, String> {
        let r = self.inner.clone();
        let mut g = r.borrow_mut();
        g.run(|g| {
            g.basic()?;
            need(active(&g.state).is_none(), "UNKNOWN_OR_ACTIVE")?;
            let mut captured = g.evidence("prepare_sell", Some(ex), &V::Null)?;
            captured
                .as_object_mut()
                .map(|m| m.insert("requestedQuantity".into(), copy(requested_quantity.unwrap_or(&V::Null))));
            g.ensure_installed()?;
            let g_risk = (g.deps.get_global_risk)()?;
            g.append(&json!({
                "kind": "prepared_sell",
                "exchangeId": ex,
                "evidence": captured,
                "globalRisk": g_risk,
                "intentId": format!("neutral-owned-sell-{}", uuid::Uuid::new_v4()),
                "key": uuid::Uuid::new_v4().to_string(),
                "at": iso((g.deps.now)()),
            }))?;
            g.reserve()?;
            g.status()
        })
    }

    pub fn pair_census(&self, first_ex: &str) -> Result<V, String> {
        let mut g = self.inner.borrow_mut();
        g.basic()?;
        let at = iso((g.deps.now)());
        pair_census(&g.state, first_ex, &at)
    }

    pub fn funding_reservation(&self) -> Result<V, String> {
        funding(&self.inner.borrow().state)
    }

    pub fn funding_reservation_status(&self) -> Result<V, String> {
        let g = self.inner.borrow();
        Ok(json!({
            "record": funding(&g.state)?,
            "receipt": copy(get(&g.state, "reservation")),
        }))
    }

    pub fn refresh_reservation(&self) -> Result<V, String> {
        let r = self.inner.clone();
        let mut g = r.borrow_mut();
        g.run(|g| {
            g.basic()?;
            g.reserve()?;
            g.status()
        })
    }

    pub fn prepare_hedge(&self, first_ex: &str) -> Result<V, String> {
        let r = self.inner.clone();
        let mut g = r.borrow_mut();
        g.run(|g| {
            g.basic()?;
            need(
                active(&g.state).is_none()
                    && get(&g.state, "orders")
                        .as_array()
                        .map(|os| {
                            !os.iter()
                                .any(|o| get(get(o, "body"), "action").as_str() == Some("buy"))
                        })
                        .unwrap_or(true),
                "ONE_ORDER_PILOT",
            )?;
            let captured = g.evidence("prepare_hedge", Some(first_ex), &V::Null)?;
            g.ensure_installed()?;
            let g_risk = (g.deps.get_global_risk)()?;
            g.append(&json!({
                "kind": "prepared",
                "evidence": captured,
                "globalRisk": g_risk,
                "intentId": format!("neutral-owned-hedge-{}", uuid::Uuid::new_v4()),
                "key": uuid::Uuid::new_v4().to_string(),
                "at": iso((g.deps.now)()),
            }))?;
            g.reserve()?;
            g.status()
        })
    }

    pub fn dispatch_prepared(&self) -> Result<V, String> {
        let r = self.inner.clone();
        let mut g = r.borrow_mut();
        g.run(|g| {
            g.basic()?;
            let o = active(&g.state).unwrap_or(V::Null);
            need(
                get(&o, "phase").as_str() == Some("prepared")
                    && get(&o, "fetchStarts").as_array().is_some_and(|a| a.is_empty()),
                "UNKNOWN_NEVER_RETRY",
            )?;
            g.ensure_installed()?;
            let lease = g.basic()?;
            let body = get(&o, "body").clone();
            let authority = json!({
                "permit": {
                    "intentId": get(&o, "intentId"),
                    "bodyHash": get(&o, "bodyHash"),
                },
                "lease": lease,
            });
            let place_result = {
                let transport = g.transport.take();
                let res = match &transport {
                    Some(t) => t.place(&body, &authority).map_err(|e| {
                        json!({"code": e.code, "mayHaveExecuted": e.may_have_executed})
                    }),
                    None => Err(json!({"code": "NO_TRANSPORT", "mayHaveExecuted": true})),
                };
                g.transport = transport;
                res
            };
            match place_result {
                Ok(_) => {}
                Err(e) => {
                    if !g.failed {
                        let mhe = get(&e, "mayHaveExecuted") != &V::Bool(false);
                        g.append(&json!({
                            "kind": "error",
                            "mayHaveExecuted": mhe,
                            "at": iso((g.deps.now)()),
                        }))?;
                        if pilot_unknown(&g.state) {
                            let arg = json!({
                                "kind": "uncertain_hedge",
                                "intentId": get(&o, "intentId"),
                                "allWritersStop": true,
                            });
                            (g.deps.on_financial_halt)(&arg)?;
                        }
                    }
                    return Err(sget(&e, "code").unwrap_or("DISPATCH_FAILED").to_string());
                }
            }
            g.status()
        })
    }

    pub fn reconcile(&self, e: &V) -> Result<V, String> {
        let r = self.inner.clone();
        let mut g = r.borrow_mut();
        g.run(|g| {
            g.basic()?;
            let g_risk = (g.deps.get_global_risk)()?;
            let mut ev = copy(e).as_object().cloned().unwrap_or_default();
            ev.insert("globalRisk".into(), g_risk);
            let res = g.append(&json!({
                "kind": "reconciled",
                "evidence": V::Object(ev),
                "at": iso((g.deps.now)()),
            }));
            if let Err(m) = &res {
                let base = m.split('\u{0}').next().unwrap_or("");
                if !g.failed
                    && (base == "HEDGE_COORDINATOR_NON_NEUTRAL_graphReceipt"
                        || base == "HEDGE_COORDINATOR_NON_NEUTRAL_collateralReceipt")
                {
                    g.append(&json!({
                        "kind": "financial_mismatch",
                        "reason": "NON_NEUTRAL_POST_ACCOUNT",
                        "at": iso((g.deps.now)()),
                    }))?;
                    (g.deps.on_financial_halt)(&json!({
                        "kind": "non_neutral_post_account",
                        "allWritersStop": true,
                    }))?;
                }
                return Err(m.clone());
            }
            g.reserve()?;
            g.status()
        })
    }

    pub fn export_handback_evidence(&self) -> Result<V, String> {
        let mut g = self.inner.borrow_mut();
        g.basic()?;
        need(
            active(&g.state).is_none() && !pilot_unknown(&g.state),
            "HANDOFF_NOT_TERMINAL",
        )?;
        let coordinator_anchor = get(&g.journal_source()?, "headPrefix").clone();
        let mut body = Map::new();
        body.insert("schemaVersion".into(), json!(1));
        body.insert("kind".into(), json!("neutral_owned_hedge_explicit_handback_evidence_v2"));
        if let V::Object(sc) = get(&g.state, "scope") {
            for (k, v) in sc {
                body.insert(k.clone(), v.clone());
            }
        }
        body.insert("seedHash".into(), json!(g.seed_hash.clone()));
        body.insert("migration".into(), copy(&g.migration));
        body.insert("legacyAnchor".into(), copy(get(&g.seed, "anchor")));
        body.insert("coordinatorAnchor".into(), coordinator_anchor);
        body.insert("originalInventory".into(), copy(get(&g.state, "originalInventory")));
        body.insert("currentInventory".into(), copy(get(&g.state, "currentInventory")));
        body.insert("pairRetirements".into(), copy(get(&g.state, "pairRetirements")));
        body.insert("counterInventory".into(), copy(get(&g.state, "counterInventory")));
        body.insert("assignments".into(), copy(get(&g.state, "assignments")));
        body.insert("funding".into(), funding(&g.state)?);
        body.insert("requiresExplicitNewOwnerImplementation".into(), V::Bool(true));
        let body = V::Object(body);
        let mut out = body.as_object().cloned().unwrap_or_default();
        out.insert("proofHash".into(), json!(payload_hash(&body)?));
        Ok(V::Object(out))
    }

    pub fn close(&self) -> Result<(), String> {
        let mut g = self.inner.borrow_mut();
        need(!g.busy, "BUSY")?;
        if g.closed {
            return Ok(());
        }
        g.closed = true;
        g.lease.release().map_err(|e| e.to_string())
    }
}
