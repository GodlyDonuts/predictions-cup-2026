// execution/baal-neutral-owned-counterpart-pilot-v2.mjs — versioned one-share
// neutral-only owned counterpart pilot. Never enables a prior window.
//
// Port notes:
// - `need` in JS throws bare `Error('HEDGE_COORDINATOR_'+c)` (no .code) — kept
//   as bare Strings here; `diagnosticCode` uppercases them identically.
// - `randomUUID()` -> uuid::Uuid::new_v4(). `performance.now()` -> Instant.
// - `dispatch` is `paired_rest_controller::WireDispatch` (spec + guards +
//   WireDispatchError.wire_receipt), mirroring the JS spec-function wrappers.
// - `issued` WeakMap on spec.beforeDispatch -> HashMap<usize,V> keyed by guard
//   pointer (same convention as the evidence bridges).
// - `error.fatal`/`error.durableRecordingFailed` flags cannot ride on String
//   errors: guardedPermit wraps every guard failure in
//   OWNED_CUSTODY_DISPATCH_GUARD (noted divergence).
use crate::baal_scaled_controller_legacy_replay_v1::{
    replay_baal_scaled_controller_v1, validate_legacy_scaled_neutral_ack,
};
use crate::baal_scaled_owned_no_hedge_proposal_v1::plan_baal_owned_no_hedge;
use crate::baal_scaled_profitable_exit_signal_v2::plan_baal_scaled_profitable_exit_v2;
use crate::core::{canonical_payload, payload_hash};
use crate::frozen::collect_frozen_paper_files;
use crate::no_rest_multishare_reconciliation_v1::verify_scoped_rest_no_multishare;
use crate::paired_rest_controller::{WireDispatch, WireDispatchError};
use crate::receipts::parse_number_lexemes;
use crate::transport::{Guard, OrderTransport, OrderTransportError, OrderTransportOptions, TransportScope};
use baal_service_runtime::fixed::{amount, instant, money, price};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};
use serde_json::{json, Map, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
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
    v.get(k).unwrap_or(&V::Null)
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn hex(v: &V) -> bool {
    v.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}
fn sha(b: &[u8]) -> String {
    sha256(b)
}
fn iso(n: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(n)
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn sync(v: Result<V, String>) -> Result<V, String> {
    v.map(|x| x.clone())
}
fn js_str(v: &V) -> String {
    v.as_str()
        .map(|x| x.to_string())
        .or_else(|| v.as_i64().map(|x| x.to_string()))
        .or_else(|| v.as_u64().map(|x| x.to_string()))
        .or_else(|| v.as_f64().map(|x| x.to_string()))
        .unwrap_or_default()
}
fn diagnostic_code(error: &str) -> String {
    let stripped = error.strip_prefix('\u{0}').unwrap_or(error);
    let ok = !stripped.is_empty()
        && stripped.len() <= 141
        && stripped.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        && stripped.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if ok {
        stripped.to_uppercase()
    } else {
        "OPERATION_FAILED".to_string()
    }
}

/// liveCoordinatorIdentity(workspace)
pub fn live_coordinator_identity(workspace: &Path) -> Result<V, String> {
    collect_frozen_paper_files(
        workspace,
        &["execution/baal-neutral-owned-counterpart-pilot-v2.mjs"],
    )
}

fn find_active_index(s: &V) -> Option<usize> {
    get(s, "orders")
        .as_array()
        .and_then(|o| {
            o.iter()
                .position(|o| get(o, "proof").is_null() && get(o, "phase").as_str() != Some("abandoned"))
        })
}
fn active(s: &V) -> V {
    find_active_index(s)
        .map(|i| get(s, "orders")[i].clone())
        .unwrap_or(V::Null)
}
fn active_pair_quantity(s: &V, a: &V) -> i64 {
    let aq = js_int(get(a, "quantity")).unwrap_or(0);
    let retired: i64 = get(s, "pairRetirements")
        .as_array()
        .map(|rs| {
            rs.iter()
                .filter(|r| get(r, "assignmentId") == get(a, "assignmentId"))
                .map(|r| js_int(get(r, "quantity")).unwrap_or(0))
                .sum()
        })
        .unwrap_or(0);
    aq - retired
}
fn matched(s: &V) -> i64 {
    get(s, "assignments")
        .as_array()
        .map(|a| a.iter().map(|a| active_pair_quantity(s, a)).sum())
        .unwrap_or(0)
}
fn scope_account(account: &V, ex: &str) -> V {
    let scoped = get(account, "scopedOrdersPagesByExchange")
        .get(ex)
        .cloned()
        .or_else(|| {
            let d = get(account, "scopedOrdersPages").clone();
            if d.is_null() {
                None
            } else {
                Some(d)
            }
        })
        .unwrap_or(V::Null);
    json!({
        "cash": get(account, "cash"),
        "positions": get(account, "positions"),
        "openOrdersPages": get(account, "openOrdersPages"),
        "scopedOrdersPages": scoped,
    })
}
fn by_market_of(scope: &V) -> BTreeMap<String, Vec<String>> {
    let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for x in get(scope, "exchanges").as_array().cloned().unwrap_or_default() {
        m.entry(get(&x, "marketId").as_str().unwrap_or("").to_string())
            .or_default()
            .push(get(&x, "exchangeId").as_str().unwrap_or("").to_string());
    }
    m
}
/// pairCensus(s, firstEx, at)
fn pair_census_of(s: &V, first_ex: &str, at: &V) -> Result<V, String> {
    let first = get(&get(s, "currentInventory"), first_ex);
    let other_ex = get(get(first, "package"), "legs")
        .as_array()
        .and_then(|ls| {
            ls.iter()
                .find(|l| get(l, "exchangeId").as_str() != Some(first_ex))
        })
        .map(|l| get(l, "exchangeId").as_str().unwrap_or("").to_string())
        .unwrap_or_default();
    let mut out = get(s, "scope").as_object().cloned().unwrap_or_default();
    out.insert("complete".into(), json!(true));
    out.insert(
        "firstProofHash".into(),
        get(get(first, "proof"), "verificationHash").clone(),
    );
    out.insert(
        "sourceHash".into(),
        json!(payload_hash(&json!({
            "seed": get(s, "seedHash"),
            "assignments": get(s, "assignments"),
            "counterpart": get(&get(get(s, "currentInventory"), &other_ex), "proof")
                .get("verificationHash")
                .cloned()
                .unwrap_or(V::Null),
        }))?),
    );
    out.insert("at".into(), at.clone());
    let matched_lots: Vec<V> = get(s, "assignments")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| get(a, "firstExchangeId").as_str() == Some(first_ex))
        .filter(|a| active_pair_quantity(s, a) > 0)
        .map(|a| {
            json!({
                "assignmentId": get(&a, "assignmentId"),
                "firstLotId": get(&a, "firstLotId"),
                "counterpartLotId": get(&a, "counterpartLotId"),
                "quantity": active_pair_quantity(s, &a),
            })
        })
        .collect();
    out.insert("matchedLots".into(), V::Array(matched_lots));
    let cp = get(&get(s, "currentInventory"), &other_ex);
    let counterpart_owned = if js_int(get(get(cp, "proof"), "remainingOwnedQuantity"))
        .map(|q| q > 0)
        .unwrap_or(false)
    {
        cp.clone()
    } else {
        V::Null
    };
    out.insert("counterpartOwned".into(), counterpart_owned);
    Ok(V::Object(out))
}
/// projectRisk(s)
pub fn project_risk(s: &V) -> Result<V, String> {
    let mut rows: Vec<V> = Vec::new();
    let mut owned: Vec<V> = Vec::new();
    if let Some(m) = get(s, "currentInventory").as_object() {
        for (ex, slot) in m {
            if js_int(get(get(slot, "proof"), "remainingOwnedQuantity")).unwrap_or(0) == 0 {
                continue;
            }
            let mut allocated: Map<String, V> = Map::new();
            for a in get(s, "assignments").as_array().cloned().unwrap_or_default() {
                let n = active_pair_quantity(s, &a);
                need(n >= 0, "PAIR_OVERRETIRE")?;
                let lot = if get(&a, "firstExchangeId").as_str() == Some(ex.as_str()) {
                    get(&a, "firstLotId").clone()
                } else if get(&a, "counterpartExchangeId").as_str() == Some(ex.as_str()) {
                    get(&a, "counterpartLotId").clone()
                } else {
                    V::Null
                };
                if !lot.is_null() {
                    let key = get(&lot, "as_str").as_str();
                    let _ = key;
                    let prev = allocated
                        .get(&lot.as_str().unwrap_or("").to_string())
                        .and_then(V::as_i64)
                        .unwrap_or(0);
                    allocated.insert(
                        lot.as_str().unwrap_or("").to_string(),
                        json!(prev + n),
                    );
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
                        .and_then(V::as_i64)
                        .unwrap_or(0);
                need(n >= 0, "PAIR_EXCEEDS_CURRENT_LOT")?;
                q += n;
                cost += price(get(&l, "entryPrice").as_str().unwrap_or(""))? * n as i128;
            }
            let other = get(get(slot, "package"), "legs")
                .as_array()
                .and_then(|ls| {
                    ls.iter()
                        .find(|l| get(l, "exchangeId").as_str() != Some(ex.as_str()))
                        .cloned()
                })
                .unwrap_or(V::Null);
            owned.push(json!({
                "exposureId": format!("scaled:{}:{}:owned", get(get(s, "scope"), "runId").as_str().unwrap_or(""), ex),
                "marketId": get(get(slot, "proof"), "marketId"),
                "eventId": get(get(slot, "package"), "eventId"),
                "nominalCost": get(get(slot, "proof"), "remainingOwnedCost"),
            }));
            if q > 0 {
                rows.push(json!({
                    "exposureId": format!("scaled:{}:{}:hedge-for-{}", get(get(s, "scope"), "runId").as_str().unwrap_or(""), get(&other, "exchangeId").as_str().unwrap_or(""), ex),
                    "marketId": get(&other, "marketId"),
                    "eventId": get(get(slot, "package"), "eventId"),
                    "maximumNominalCost": money(990_000_i128 * q as i128 - cost),
                }));
            }
        }
    }
    let o = active(s);
    let mut body = Map::new();
    body.insert("schemaVersion".into(), json!(1));
    body.insert("kind".into(), json!("baal_scaled_no_full_nominal_reservation_v1"));
    body.insert("ownerId".into(), get(get(s, "scope"), "ownerId").clone());
    body.insert("runId".into(), get(get(s, "scope"), "runId").clone());
    body.insert(
        "tournamentId".into(),
        get(get(s, "scope"), "tournamentId").clone(),
    );
    body.insert(
        "allocationHash".into(),
        json!(payload_hash(get(s, "allocation"))?),
    );
    body.insert("seedHash".into(), get(s, "seedHash").clone());
    body.insert(
        "implementationEpoch".into(),
        get(get(s, "migration"), "implementationEpoch").clone(),
    );
    body.insert("owned".into(), V::Array(owned.clone()));
    body.insert("reservations".into(), V::Array(rows.clone()));
    body.insert(
        "unresolvedIntentIds".into(),
        if o.is_null() {
            json!([])
        } else {
            json!([get(&o, "intentId")])
        },
    );
    let cash: i128 = rows
        .iter()
        .map(|r| amount(get(r, "maximumNominalCost").as_str().unwrap_or("")).unwrap_or(0))
        .sum();
    body.insert("cashReserved".into(), json!(money(cash)));
    let owned_principal: i128 = owned
        .iter()
        .map(|r| amount(get(r, "nominalCost").as_str().unwrap_or("")).unwrap_or(0))
        .sum();
    body.insert("ownedPrincipal".into(), json!(money(owned_principal)));
    let body = V::Object(body);
    let mut out = body.as_object().cloned().unwrap();
    out.insert("reservationHash".into(), json!(payload_hash(&body)?));
    Ok(V::Object(out))
}
/// funding = projectRisk
fn funding(s: &V) -> Result<V, String> {
    project_risk(s)
}
/// pilotUnknown(s)
pub fn pilot_unknown(s: &V) -> bool {
    let o = active(s);
    !o.is_null()
        && (get(&o, "phase").as_str() == Some("uncertain")
            || get(&o, "phase").as_str() == Some("semantic_mismatch")
            || (get(&o, "orderId").is_null()
                && get(&o, "fetchStarts").as_array().is_some_and(|f| !f.is_empty())))
}
/// emptyAll(e, s, at)
fn empty_all(e: &V, s: &V, at: i64) -> Result<(), String> {
    let first_ex = get(e, "firstExchangeId").as_str().unwrap_or("");
    for (key, endpoint) in [
        (
            "graphReceipt",
            format!(
                "/relationships/graph?marketId={}&depth=4&tournamentId={}",
                get(&get(&get(s, "currentInventory"), first_ex), "proof")
                    .get("marketId")
                    .and_then(V::as_str)
                    .unwrap_or(""),
                get(get(s, "scope"), "tournamentId").as_str().unwrap_or("")
            ),
        ),
        (
            "collateralReceipt",
            format!(
                "/portfolio/collateral?tournamentId={}",
                get(get(s, "scope"), "tournamentId").as_str().unwrap_or("")
            ),
        ),
    ] {
        let r = get(e, key);
        need(
            !r.is_null()
                && get(r, "method").as_str() == Some("GET")
                && js_int(get(r, "status")) == Some(200)
                && get(r, "endpoint").as_str() == Some(endpoint.as_str())
                && get(r, "bodyText").is_string()
                && get(r, "sourceBodyHash").as_str()
                    == Some(sha(get(r, "bodyText").as_str().unwrap_or("").as_bytes()).as_str())
                && instant_v(get(r, "requestStartedAt"))? <= instant_v(get(r, "receivedAt"))?
                && instant_v(get(r, "receivedAt"))? <= at
                && at - instant_v(get(r, "requestStartedAt"))? <= 5_000,
            &format!("FRESH_NONE_{key}"),
        )?;
        let (d, _) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        need(
            if key == "graphReceipt" {
                get(&d, "nodes").as_array().is_some_and(|a| a.is_empty())
                    && get(&d, "relationships").as_array().is_some_and(|a| a.is_empty())
            } else {
                get(&d, "data").as_array().is_some_and(|a| a.is_empty())
            },
            &format!("NON_NEUTRAL_{key}"),
        )?;
    }
    let book = if !get(e, "otherBookReceipt").is_null() {
        get(e, "otherBookReceipt").clone()
    } else if !get(e, "bookReceipt").is_null() {
        get(e, "bookReceipt").clone()
    } else {
        get(e, "book").clone()
    };
    let last_account = if !get(e, "after").is_null() {
        let after = get(e, "after");
        let mut v = vec![
            instant_v(get(get(after, "cash"), "receivedAt"))?,
            instant_v(get(get(after, "positions"), "receivedAt"))?,
        ];
        for r in get(after, "openOrdersPages").as_array().cloned().unwrap_or_default() {
            v.push(instant_v(get(&r, "receivedAt"))?);
        }
        for r in get(after, "scopedOrdersPages").as_array().cloned().unwrap_or_default() {
            v.push(instant_v(get(&r, "receivedAt"))?);
        }
        Some(v.into_iter().max().unwrap_or(0))
    } else {
        None
    };
    need(
        instant_v(get(get(e, "graphReceipt"), "receivedAt"))?
            <= instant_v(get(get(e, "collateralReceipt"), "requestStartedAt"))?
            && instant_v(get(get(e, "graphReceipt"), "requestStartedAt"))?
                >= if !book.is_null() {
                    instant_v(get(&book, "receivedAt"))?
                } else {
                    last_account.unwrap_or(0)
                },
        "COLLATERAL_LAST",
    )?;
    Ok(())
}
/// guarded(s, e, g, at)
fn guarded(s: &V, e: &V, g: &V, at: i64) -> Result<V, String> {
    need(
        at >= instant_v(get(get(s, "activation"), "startedAt"))?
            && at < instant_v(get(get(s, "activation"), "hedgeExpiresAt"))?,
        "HEDGE_WINDOW",
    )?;
    let o = active(s);
    need(
        !g.is_null()
            && get(g, "ownerId") == get(get(s, "scope"), "ownerId")
            && get(g, "runId") == get(get(s, "scope"), "runId")
            && get(g, "tournamentId") == get(get(s, "scope"), "tournamentId")
            && get(g, "complete") == &json!(true)
            && get(g, "eventScopeComplete") == &json!(true)
            && get(g, "soleCollector") == &json!(true)
            && get(g, "stopped") == &json!(false)
            && hex(get(g, "sourceHash"))
            && get(g, "unknownAcceptance") == &json!(false)
            && get(g, "unresolvedIntentIds").is_array()
            && get(g, "unresolvedIntentIds")
                .as_array()
                .unwrap()
                .iter()
                .all(|id| id == &get(&o, "intentId").clone())
            && instant_v(get(g, "at"))? <= at
            && at - instant_v(get(g, "at"))? <= 20_000,
        "GLOBAL_UNKNOWN",
    )?;
    need(
        get(g, "fundingSatisfied") != &json!(false)
            && get(g, "reservations").is_array()
            && get(g, "reservations").as_array().unwrap().iter().all(|r| {
                amount(get(r, "maximumNominalCost").as_str().unwrap_or(""))
                    .map(|n| n >= 0)
                    .unwrap_or(false)
                    && get(r, "unknownAcceptance") != &json!(true)
            })
            && get(g, "reservations")
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|r| get(r, "exposureId").as_str().unwrap_or("").to_string())
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == a.len()
                })
                .unwrap_or(false)
            && get(g, "reservations")
                .as_array()
                .unwrap()
                .iter()
                .map(|r| amount(get(r, "maximumNominalCost").as_str().unwrap_or("")).unwrap_or(0))
                .sum::<i128>()
                <= amount(get(g, "cashLower").as_str().unwrap_or("")).unwrap_or(i128::MIN),
        "CURRENT_CASH_FUNDS_FULL_RESERVE",
    )?;
    empty_all(e, s, at)?;
    need(
        !get(e, "pairCensus").is_null()
            && same(
                get(e, "pairCensus"),
                &pair_census_of(
                    s,
                    get(e, "firstExchangeId").as_str().unwrap_or(""),
                    get(get(e, "pairCensus"), "at"),
                )?,
            )?,
        "CAPTURED_PAIR_CENSUS",
    )?;
    need(
        !get(e, "globalRisk").is_null()
            && get(get(e, "globalRisk"), "allocationHash") == get(g, "allocationHash")
            && same(get(get(e, "globalRisk"), "reservations"), get(g, "reservations"))?
            && get(g, "controllerReservationHash") == get(&funding(s)?, "reservationHash")
            && get(g, "globalLedgerHash")
                == get(&get(s, "reservation"), "globalLedgerHash"),
        "CAPTURED_BUDGET_STILL_MATCHES",
    )?;
    let source_risk = get(e, "globalRisk").clone();
    need(
        get(&source_risk, "unresolvedIntentIds")
            .as_array()
            .is_some_and(|a| a.is_empty()),
        "SOURCE_RISK_PENDING",
    )?;
    let mut p_in = e.as_object().cloned().unwrap_or_default();
    p_in.insert("globalRisk".into(), source_risk);
    p_in.insert("scope".into(), get(s, "scope").clone());
    p_in.insert(
        "owned".into(),
        get(&get(s, "currentInventory"), get(e, "firstExchangeId").as_str().unwrap_or(""))
            .clone(),
    );
    p_in.insert("now".into(), json!(iso(at as f64)));
    let p = plan_baal_owned_no_hedge(&V::Object(p_in));
    need(
        get(&p, "proposalReady") == &json!(true),
        &format!("PROPOSAL_{}", get(&p, "reason").as_str().unwrap_or("")),
    )?;
    let first = get(&p, "matchedAssignmentsIfFilled")
        .as_array()
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or(V::Null);
    need(
        !first.is_null() && price(get(&p, "protectedLimit").as_str().unwrap_or(""))? <= 995_000,
        "PILOT_COST",
    )?;
    let mut q = p.as_object().cloned().unwrap_or_default();
    q.insert("mode".into(), json!("baal_neutral_owned_counterpart_fixed_one_v1"));
    q.insert("originalProposalHash".into(), get(&p, "proposalHash").clone());
    q.insert("quantity".into(), json!(1));
    q.insert("maximumHedgeNotional".into(), get(&p, "protectedLimit").clone());
    q.insert(
        "firstMatchedActualPrincipal".into(),
        get(&first, "entryPrice").clone(),
    );
    q.insert(
        "protectedMatchedPairCost".into(),
        json!(money(
            price(get(&first, "entryPrice").as_str().unwrap_or(""))?
                + price(get(&p, "protectedLimit").as_str().unwrap_or(""))?
        )),
    );
    q.insert("conditionalOrdinaryMinimumPayout".into(), json!("1.000000"));
    q.insert("conditionalThirdPartyPayout".into(), json!("2.000000"));
    q.insert(
        "conditionalOrdinarySurplus".into(),
        json!(money(
            1_000_000_i128
                - price(get(&first, "entryPrice").as_str().unwrap_or(""))?
                - price(get(&p, "protectedLimit").as_str().unwrap_or(""))?
        )),
    );
    q.insert(
        "administrativeZeroPayoutLoss".into(),
        json!(money(
            -price(get(&first, "entryPrice").as_str().unwrap_or(""))?
                - price(get(&p, "protectedLimit").as_str().unwrap_or(""))?
        )),
    );
    let mut first_row = first.as_object().cloned().unwrap_or_default();
    first_row.insert("quantity".into(), json!(1));
    first_row.insert("firstPrincipal".into(), get(&first, "entryPrice").clone());
    q.insert(
        "matchedAssignmentsIfFilled".into(),
        V::Array(vec![V::Object(first_row)]),
    );
    q.shift_remove("proposalHash");
    let q = V::Object(q);
    let mut out = q.as_object().cloned().unwrap();
    out.insert("proposalHash".into(), json!(payload_hash(&q)?));
    Ok(V::Object(out))
}
/// sellProposal(s, ex, e, g, at)
fn sell_proposal(s: &V, ex: &str, e: &V, g: &V, at: i64) -> Result<V, String> {
    need(
        at >= instant_v(get(get(s, "activation"), "startedAt"))?
            && at < instant_v(get(get(s, "activation"), "exitExpiresAt"))?,
        "OWNED_EXIT_WINDOW",
    )?;
    let o = active(s);
    need(
        !g.is_null()
            && get(g, "ownerId") == get(get(s, "scope"), "ownerId")
            && get(g, "runId") == get(get(s, "scope"), "runId")
            && get(g, "tournamentId") == get(get(s, "scope"), "tournamentId")
            && get(g, "complete") == &json!(true)
            && get(g, "eventScopeComplete") == &json!(true)
            && get(g, "soleCollector") == &json!(true)
            && get(g, "stopped") == &json!(false)
            && hex(get(g, "sourceHash"))
            && get(g, "unknownAcceptance") == &json!(false)
            && get(g, "unresolvedIntentIds").is_array()
            && get(g, "unresolvedIntentIds")
                .as_array()
                .unwrap()
                .iter()
                .all(|id| id == &get(&o, "intentId").clone())
            && instant_v(get(g, "at"))? <= at
            && at - instant_v(get(g, "at"))? <= 20_000,
        "GLOBAL_UNKNOWN",
    )?;
    need(
        get(g, "controllerReservationHash") == get(&funding(s)?, "reservationHash")
            && get(g, "globalLedgerHash")
                == get(&get(s, "reservation"), "globalLedgerHash"),
        "CURRENT_RESERVE_BINDING",
    )?;
    let mut e2 = e.as_object().cloned().unwrap_or_default();
    e2.insert("firstExchangeId".into(), json!(ex));
    empty_all(&V::Object(e2.clone()), s, at)?;
    let slot = get(&get(s, "currentInventory"), ex);
    need(
        js_int(get(get(slot, "proof"), "remainingOwnedQuantity"))
            .map(|q| q > 0)
            .unwrap_or(false),
        "OWNED_POSITIVE",
    )?;
    let p = plan_baal_scaled_profitable_exit_v2(&json!({
        "scope": get(s, "scope"),
        "owned": slot,
        "account": scope_account(get(e, "account"), ex),
        "bookReceipt": if !get(e, "bookReceipt").is_null() {
            get(e, "bookReceipt").clone()
        } else {
            get(e, "book").clone()
        },
        "globalRisk": get(e, "globalRisk"),
        "now": iso(at as f64),
        "protectiveTicks": 0,
        "maximumSharesPerOrder": 100,
        "maximumOrderCost": "25.000000",
        "feesPerShare": "0.000000",
        "requestedQuantity": get(e, "requestedQuantity"),
    }));
    need(
        get(&p, "proposalReady") == &json!(true),
        &format!("PROFIT_EXIT_{}", get(&p, "reason").as_str().unwrap_or("")),
    )?;
    Ok(p)
}
/// retireSoldPairs(s, o, proof)
fn retire_sold_pairs(s: &V, o: &V, proof: &V) -> Result<Vec<V>, String> {
    let mut out: Vec<V> = Vec::new();
    for old in get(get(o, "priorProof"), "remainingLots")
        .as_array()
        .cloned()
        .unwrap_or_default()
    {
        let next = get(proof, "remainingLots")
            .as_array()
            .and_then(|a| {
                a.iter()
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
            let first = get(&a, "firstExchangeId") == get(get(o, "body"), "exchangeId")
                && get(&a, "firstLotId") == get(&old, "lotId");
            let counter = get(&a, "counterpartExchangeId") == get(get(o, "body"), "exchangeId")
                && get(&a, "counterpartLotId") == get(&old, "lotId");
            if !first && !counter {
                continue;
            }
            let offset = if first {
                get(&a, "firstUnitStart")
            } else {
                get(&a, "counterpartUnitStart")
            };
            let offset = js_int(offset);
            need(
                offset.is_some() && js_int(get(&a, "quantity")) == Some(1),
                "ONE_SHARE_PAIR_INTERVAL",
            )?;
            let offset = offset.unwrap();
            let n = std::cmp::min(end, offset + js_int(get(&a, "quantity")).unwrap())
                - std::cmp::max(begin, offset);
            let n = n.max(0);
            if n > 0 {
                out.push(json!({
                    "retirementId": format!("{}:{}", get(o, "intentId").as_str().unwrap_or(""), get(&a, "assignmentId").as_str().unwrap_or("")),
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
    need(
        out.iter()
            .map(|r| get(r, "assignmentId").as_str().unwrap_or("").to_string())
            .collect::<std::collections::HashSet<_>>()
            .len()
            == out.len(),
        "PAIR_DOUBLE_RETIREMENT",
    )?;
    Ok(out)
}
/// assignFill(s, o, p)
fn assign_fill(s: &V, o: &V, p: &V) -> Result<Vec<V>, String> {
    let mut result: Vec<V> = Vec::new();
    let mut target: usize = 0;
    let mut left = js_int(get(&get(&get(o, "proposal"), "matchedAssignmentsIfFilled")[0], "quantity"))
        .unwrap_or(0);
    for lot in get(p, "remainingLots").as_array().cloned().unwrap_or_default() {
        let mut remaining = js_int(get(&lot, "quantity")).unwrap_or(0);
        if get(get(o, "priorProof"), "remainingLots")
            .as_array()
            .is_some_and(|a| a.iter().any(|old| get(old, "lotId") == get(&lot, "lotId")))
        {
            continue;
        }
        while remaining > 0 {
            let first = get(&get(o, "proposal"), "matchedAssignmentsIfFilled")
                .as_array()
                .and_then(|a| a.get(target))
                .cloned()
                .unwrap_or(V::Null);
            need(!first.is_null() && left > 0, "MATCHED_FILL_EXCEEDS_PROPOSAL")?;
            let n = std::cmp::min(left, remaining);
            need(
                price(get(&first, "entryPrice").as_str().unwrap_or(""))?
                    + price(get(&lot, "entryPrice").as_str().unwrap_or(""))?
                    <= 990_000,
                "ACTUAL_PAIR_MARGIN",
            )?;
            let first_lots = get(&get(&get(s, "currentInventory"), get(o, "firstExchangeId").as_str().unwrap_or("")), "proof");
            let first_lots = get(first_lots, "remainingLots").as_array().cloned().unwrap_or_default();
            let flot = first_lots
                .iter()
                .find(|l| get(l, "lotId") == get(&first, "firstLotId"))
                .cloned()
                .unwrap_or(V::Null);
            result.push(json!({
                "assignmentId": format!("{}:{}:{}", get(o, "intentId").as_str().unwrap_or(""), get(&lot, "lotId").as_str().unwrap_or(""), target),
                "firstExchangeId": get(o, "firstExchangeId"),
                "counterpartExchangeId": get(get(o, "body"), "exchangeId"),
                "firstLotId": get(&first, "firstLotId"),
                "counterpartLotId": get(&lot, "lotId"),
                "quantity": n,
                "firstUnitCost": get(&first, "entryPrice"),
                "counterpartUnitCost": get(&lot, "entryPrice"),
                "firstProofHash": get(get(o, "proposal"), "firstProofHash"),
                "counterpartProofHash": get(p, "verificationHash"),
                "firstUnitStart": js_int(get(&flot, "originalQuantity")).unwrap_or(0) - js_int(get(&flot, "quantity")).unwrap_or(0),
                "counterpartUnitStart": js_int(get(&lot, "originalQuantity")).unwrap_or(0) - js_int(get(&lot, "quantity")).unwrap_or(0),
            }));
            remaining -= n;
            left -= n;
            if left == 0 {
                target += 1;
                left = js_int(
                    get(&get(&get(o, "proposal"), "matchedAssignmentsIfFilled")[target], "quantity"),
                )
                .unwrap_or(0);
            }
        }
    }
    need(
        result
            .iter()
            .map(|a| js_int(get(a, "quantity")).unwrap_or(0))
            .sum::<i64>()
            == js_int(get(p, "filledQuantity")).unwrap_or(i64::MIN),
        "EXACT_PARTIAL_PAIR_COUNT",
    )?;
    Ok(result)
}
/// reduce(s, e)
fn reduce(s: Option<&V>, e: &V) -> Result<V, String> {
    let kind = get(e, "kind").as_str().unwrap_or("").to_string();
    if kind == "seed" {
        need(
            s.is_none()
                && get(get(e, "seed"), "economicReplayPerformed") == &json!(true)
                && !get(&get(get(e, "seed"), "state"), "orders")
                    .as_array()
                    .is_some_and(|o| {
                        o.iter().any(|o| {
                            get(o, "proof").is_null()
                                && get(o, "phase").as_str() != Some("abandoned")
                        })
                    }),
            "VERIFIED_SEED",
        )?;
        let seed = get(e, "seed").clone();
        return Ok(json!({
            "schemaVersion": 1,
            "mode": "baal_neutral_owned_counterpart_pilot_v2",
            "scope": get(&get(&seed, "state"), "scope"),
            "allocation": get(&get(&seed, "state"), "allocation"),
            "originalActivation": get(&get(&seed, "state"), "activation"),
            "originalOrders": get(&get(&seed, "state"), "orders"),
            "originalInventory": get(&get(&seed, "state"), "inventory"),
            "currentInventory": get(&get(&seed, "state"), "inventory"),
            "pairRetirements": [],
            "seedHash": get(e, "seedHash"),
            "seedAnchor": get(&seed, "anchor"),
            "activation": get(e, "activation"),
            "migration": get(e, "migration"),
            "orders": [],
            "assignments": [],
            "counterInventory": {},
            "reservation": V::Null,
        }));
    }
    need(s.is_some(), "NOT_SEEDED")?;
    let s = s.unwrap();
    let mut n = s.clone();
    let o = active(&n);
    if kind == "reservation" {
        need(
            get(get(e, "receipt"), "reservationHash")
                == get(&funding(&n)?, "reservationHash")
            && get(get(e, "receipt"), "seedHash") == get(&n, "seedHash")
            && hex(get(get(e, "receipt"), "globalLedgerHash")),
            "RESERVATION",
        )?;
        n.as_object_mut()
            .unwrap()
            .insert("reservation".into(), get(e, "receipt").clone());
        return Ok(n);
    }
    if kind == "prepared_sell" {
        need(o.is_null(), "UNKNOWN_OR_ACTIVE")?;
        let ex = get(e, "exchangeId").as_str().unwrap_or("").to_string();
        let p = sell_proposal(
            &n,
            &ex,
            get(e, "evidence"),
            get(e, "globalRisk"),
            instant_v(get(e, "at"))?,
        )?;
        let slot = get(&get(&n, "currentInventory"), &ex).clone();
        let body = json!({
            "idempotencyKey": get(e, "key"),
            "exchangeId": ex,
            "side": "no",
            "action": "sell",
            "quantity": get(&p, "quantity"),
            "price": get(&p, "limit").as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN),
            "expirationDate": iso(std::cmp::min(
                instant_v(get(e, "at"))? + 30_000,
                std::cmp::min(
                    instant_v(get(&p, "expiresAt"))?,
                    instant_v(get(get(&n, "activation"), "exitExpiresAt"))?,
                ),
            ) as f64),
            "tournamentId": get(get(&n, "scope"), "tournamentId"),
        });
        need(
            !get(&n, "originalOrders").as_array().is_some_and(|os| {
                os.iter()
                    .any(|o| get(get(o, "body"), "idempotencyKey") == get(e, "key"))
            }) && !get(&n, "orders").as_array().is_some_and(|os| {
                os.iter().any(|o| {
                    get(get(o, "body"), "idempotencyKey") == get(e, "key")
                        || get(o, "intentId") == get(e, "intentId")
                })
            }),
            "KEY_REUSE",
        )?;
        let row = json!({
            "intentId": get(e, "intentId"),
            "firstExchangeId": ex,
            "marketId": get(&p, "marketId"),
            "createdAt": get(e, "at"),
            "body": body,
            "bodyJson": canonical_payload(&body),
            "bodyHash": payload_hash(&body)?,
            "proposal": p,
            "before": scope_account(get(get(e, "evidence"), "account"), &ex),
            "priorProof": get(&slot, "proof"),
            "priorAfter": get(&slot, "after"),
            "package": get(&slot, "package"),
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
        n.as_object_mut()
            .unwrap()
            .get_mut("orders")
            .and_then(|v| v.as_array_mut())
            .unwrap()
            .push(row);
        return Ok(n);
    }
    if kind == "prepared" {
        need(
            o.is_null()
                && get(&n, "orders")
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter(|o| get(get(o, "body"), "action").as_str() == Some("buy"))
                            .count()
                            == 0
                    })
                    .unwrap_or(false)
                && get(get(&n, "activation"), "allowedFirstExchangeIds")
                    .as_array()
                    .is_some_and(|a| {
                        a.iter()
                            .any(|x| x == get(get(e, "evidence"), "firstExchangeId"))
                    })
                && js_int(
                    get(&get(&get(&get(&n, "originalInventory"), get(get(e, "evidence"), "firstExchangeId").as_str().unwrap_or("")), "proof"), "remainingOwnedQuantity"),
                )
                .map(|q| q > 0)
                .unwrap_or(false),
            "ORIGINAL_UNPAIRED_ONLY",
        )?;
        let p = guarded(
            &n,
            get(e, "evidence"),
            get(e, "globalRisk"),
            instant_v(get(e, "at"))?,
        )?;
        let body = json!({
            "idempotencyKey": get(e, "key"),
            "exchangeId": get(&p, "exchangeId"),
            "side": "no",
            "action": "buy",
            "quantity": get(&p, "quantity"),
            "price": get(&p, "protectedLimit").as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN),
            "expirationDate": iso(std::cmp::min(
                instant_v(get(e, "at"))? + 30_000,
                std::cmp::min(
                    instant_v(get(&p, "expiresAt"))?,
                    instant_v(get(get(&n, "activation"), "hedgeExpiresAt"))?,
                ),
            ) as f64),
            "tournamentId": get(get(&n, "scope"), "tournamentId"),
        });
        need(
            !get(&n, "orders").as_array().is_some_and(|os| {
                os.iter().any(|o| {
                    get(get(o, "body"), "idempotencyKey") == get(e, "key")
                        || get(o, "intentId") == get(e, "intentId")
                })
            }),
            "KEY_REUSE",
        )?;
        let prior_slot = get(&get(&n, "currentInventory"), get(&p, "exchangeId").as_str().unwrap_or(""))
            .clone();
        let row = json!({
            "intentId": get(e, "intentId"),
            "firstExchangeId": get(get(e, "evidence"), "firstExchangeId"),
            "marketId": get(&p, "marketId"),
            "createdAt": get(e, "at"),
            "body": body,
            "bodyJson": canonical_payload(&body),
            "bodyHash": payload_hash(&body)?,
            "proposal": p,
            "before": scope_account(get(get(e, "evidence"), "account"), get(&p, "exchangeId").as_str().unwrap_or("")),
            "priorProof": get(&prior_slot, "proof"),
            "priorAfter": get(&prior_slot, "after"),
            "package": get(&get(&get(&n, "originalInventory"), get(get(e, "evidence"), "firstExchangeId").as_str().unwrap_or("")), "package"),
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
        n.as_object_mut()
            .unwrap()
            .get_mut("orders")
            .and_then(|v| v.as_array_mut())
            .unwrap()
            .push(row);
        return Ok(n);
    }
    if kind == "abandoned_prepared" {
        need(
            !o.is_null()
                && get(&o, "phase").as_str() == Some("prepared")
                && get(&o, "orderId").is_null()
                && get(&o, "ack").is_null()
                && get(&o, "proof").is_null()
                && get(&o, "semanticMismatch") == &json!(false)
                && get(&o, "mayHaveExecuted") == &json!(false)
                && get(&o, "fences").as_array().is_some_and(|a| a.is_empty())
                && get(&o, "fetchStarts").as_array().is_some_and(|a| a.is_empty())
                && get(&o, "receipts").as_array().is_some_and(|a| a.is_empty())
                && instant_v(get(e, "at"))? >= instant_v(get(get(&o, "body"), "expirationDate"))?,
            "ABANDON_REQUIRES_EXPIRED_UNADMITTED_UNFETCHED",
        )?;
        let idx = find_active_index(&n).unwrap();
        let orders = n
            .as_object_mut()
            .unwrap()
            .get_mut("orders")
            .and_then(|v| v.as_array_mut())
            .unwrap();
        orders[idx]
            .as_object_mut()
            .unwrap()
            .insert("phase".into(), json!("abandoned"));
        orders[idx]
            .as_object_mut()
            .unwrap()
            .insert("abandonedAt".into(), get(e, "at").clone());
        orders[idx].as_object_mut().unwrap().insert(
            "abandonedReason".into(),
            json!("expired_before_any_admission"),
        );
        return Ok(n);
    }
    need(!o.is_null() && get(&o, "proof").is_null(), "NO_PENDING")?;
    if kind == "admitted" || kind == "network_fence" {
        need(
            if kind == "admitted" {
                get(&o, "phase").as_str() == Some("prepared")
            } else {
                get(&o, "phase").as_str() == Some("dispatching")
            },
            "SINGLE_ATTEMPT",
        )?;
        let p = if get(get(&o, "body"), "action").as_str() == Some("buy") {
            guarded(&n, get(e, "evidence"), get(e, "globalRisk"), instant_v(get(e, "at"))?)?
        } else {
            sell_proposal(
                &n,
                get(get(&o, "body"), "exchangeId").as_str().unwrap_or(""),
                get(e, "evidence"),
                get(e, "globalRisk"),
                instant_v(get(e, "at"))?,
            )?
        };
        need(
            get(&p, "exchangeId") == get(get(&o, "body"), "exchangeId")
                && js_int(get(&p, "quantity")).unwrap_or(i64::MIN)
                    >= js_int(get(get(&o, "body"), "quantity")).unwrap_or(i64::MAX)
                && (if get(get(&o, "body"), "action").as_str() == Some("buy") {
                    get(&p, "firstProofHash") == get(get(&o, "proposal"), "firstProofHash")
                        && price(get(&p, "protectedLimit").as_str().unwrap_or(""))?
                            <= price(&js_str(get(get(&o, "body"), "price")))?
                } else {
                    get(&p, "ownedProofHash") == get(get(&o, "priorProof"), "verificationHash")
                        && price(get(&p, "limit").as_str().unwrap_or(""))?
                            >= price(&js_str(get(get(&o, "body"), "price")))?
                })
                && instant_v(get(e, "at"))? < instant_v(get(get(&o, "body"), "expirationDate"))?,
            "FIXED_LIMIT_SOURCE",
        )?;
        let idx = find_active_index(&n).unwrap();
        let orders = n
            .as_object_mut()
            .unwrap()
            .get_mut("orders")
            .and_then(|v| v.as_array_mut())
            .unwrap();
        if kind == "admitted" {
            orders[idx]
                .as_object_mut()
                .unwrap()
                .insert("phase".into(), json!("dispatching"));
        } else {
            need(
                payload_hash(get(e, "lease"))? == get(e, "leaseHash").as_str().unwrap_or(""),
                "ACTUAL_LEASE",
            )?;
            orders[idx]
                .as_object_mut()
                .unwrap()
                .get_mut("fences")
                .and_then(|v| v.as_array_mut())
                .unwrap()
                .push(json!({
                    "at": get(e, "at"),
                    "lease": get(e, "lease"),
                    "leaseHash": get(e, "leaseHash"),
                }));
        }
        return Ok(n);
    }
    if kind == "fetch_start" {
        let f = get(&o, "fences").as_array().and_then(|a| a.last()).cloned();
        let w = get(e, "meta");
        need(
            f.is_some()
                && get(w, "method").as_str() == Some("POST")
                && get(w, "endpoint").as_str() == Some("/orders")
                && get(w, "requestBodyHash") == get(&o, "bodyHash")
                && instant_v(get(w, "requestStartedAt"))?
                    >= instant_v(get(&f.unwrap(), "at"))?
                && get(&o, "fetchStarts").as_array().is_some_and(|a| a.is_empty()),
            "ACTUAL_INVOCATION",
        )?;
        let idx = find_active_index(&n).unwrap();
        let orders = n
            .as_object_mut()
            .unwrap()
            .get_mut("orders")
            .and_then(|v| v.as_array_mut())
            .unwrap();
        orders[idx]
            .as_object_mut()
            .unwrap()
            .get_mut("fetchStarts")
            .and_then(|v| v.as_array_mut())
            .unwrap()
            .push(w.clone());
        orders[idx]
            .as_object_mut()
            .unwrap()
            .insert("mayHaveExecuted".into(), json!(true));
        return Ok(n);
    }
    if kind == "wire" {
        let w = get(&o, "fetchStarts").as_array().and_then(|a| a.first()).cloned();
        let r = get(e, "receipt");
        need(
            w.is_some()
                && get(r, "method").as_str() == Some("POST")
                && get(r, "endpoint").as_str() == Some("/orders")
                && get(r, "sourceBodyHash").as_str()
                    == Some(sha(get(r, "bodyText").as_str().unwrap_or("").as_bytes()).as_str())
                && get(r, "fetchInvocationId") == get(&w.as_ref().unwrap(), "fetchInvocationId")
                && get(r, "requestStartedAt") == get(&w.as_ref().unwrap(), "requestStartedAt"),
            "ORIGINAL_WIRE",
        )?;
        let idx = find_active_index(&n).unwrap();
        let (d, _) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap_or(""))?;
        let orders = n
            .as_object_mut()
            .unwrap()
            .get_mut("orders")
            .and_then(|v| v.as_array_mut())
            .unwrap();
        orders[idx]
            .as_object_mut()
            .unwrap()
            .get_mut("receipts")
            .and_then(|v| v.as_array_mut())
            .unwrap()
            .push(r.clone());
        if let Some(id) = js_int(get(&d, "orderId")).filter(|id| *id > 0) {
            orders[idx]
                .as_object_mut()
                .unwrap()
                .insert("orderId".into(), json!(id.to_string()));
        }
        let mut ack = r.as_object().cloned().unwrap_or_default();
        ack.insert("requestBodyHash".into(), get(&orders[idx], "bodyHash").clone());
        orders[idx]
            .as_object_mut()
            .unwrap()
            .insert("ack".into(), V::Object(ack));
        let mismatch = !validate_legacy_scaled_neutral_ack(&orders[idx], get(&orders[idx], "ack"));
        orders[idx]
            .as_object_mut()
            .unwrap()
            .insert("semanticMismatch".into(), json!(mismatch));
        orders[idx].as_object_mut().unwrap().insert(
            "phase".into(),
            json!(if mismatch { "semantic_mismatch" } else { "acknowledged" }),
        );
        return Ok(n);
    }
    if kind == "error" {
        let idx = find_active_index(&n).unwrap();
        let orders = n
            .as_object_mut()
            .unwrap()
            .get_mut("orders")
            .and_then(|v| v.as_array_mut())
            .unwrap();
        if get(&orders[idx], "phase").as_str() == Some("dispatching")
            || get(e, "mayHaveExecuted") != &json!(false)
        {
            orders[idx]
                .as_object_mut()
                .unwrap()
                .insert("phase".into(), json!("uncertain"));
            orders[idx]
                .as_object_mut()
                .unwrap()
                .insert("mayHaveExecuted".into(), json!(true));
        } else {
            need(
                get(&orders[idx], "phase").as_str() == Some("prepared")
                    && get(&orders[idx], "fences").as_array().is_some_and(|a| a.is_empty())
                    && get(&orders[idx], "fetchStarts").as_array().is_some_and(|a| a.is_empty())
                    && get(&orders[idx], "receipts").as_array().is_some_and(|a| a.is_empty())
                    && get(&orders[idx], "orderId").is_null()
                    && get(&orders[idx], "ack").is_null()
                    && get(&orders[idx], "mayHaveExecuted") == &json!(false),
                "KNOWN_UNADMITTED_ERROR_ONLY",
            )?;
        }
        return Ok(n);
    }
    if kind == "financial_mismatch" {
        need(
            get(e, "reason").as_str() == Some("NON_NEUTRAL_POST_ACCOUNT"),
            "FINANCIAL_MISMATCH_REASON",
        )?;
        let idx = find_active_index(&n).unwrap();
        let orders = n
            .as_object_mut()
            .unwrap()
            .get_mut("orders")
            .and_then(|v| v.as_array_mut())
            .unwrap();
        orders[idx]
            .as_object_mut()
            .unwrap()
            .insert("semanticMismatch".into(), json!(true));
        orders[idx]
            .as_object_mut()
            .unwrap()
            .insert("phase".into(), json!("semantic_mismatch"));
        return Ok(n);
    }
    if kind == "reconciled" {
        let mut e2 = get(e, "evidence").as_object().cloned().unwrap_or_default();
        e2.insert("firstExchangeId".into(), get(&o, "firstExchangeId").clone());
        empty_all(&V::Object(e2), &n, instant_v(get(e, "at"))?)?;
        need(
            !get(&o, "orderId").is_null()
                && !get(&o, "ack").is_null()
                && get(&o, "semanticMismatch") == &json!(false)
                && get(&o, "phase").as_str() == Some("acknowledged"),
            "NON_NEUTRAL_OR_UNKNOWN_BLOCKS",
        )?;
        let mut binding = get(&n, "scope").as_object().cloned().unwrap_or_default();
        binding.insert("intentId".into(), get(&o, "intentId").clone());
        binding.insert(
            "idempotencyKey".into(),
            get(get(&o, "body"), "idempotencyKey").clone(),
        );
        binding.insert("orderId".into(), get(&o, "orderId").clone());
        binding.insert("marketId".into(), get(&o, "marketId").clone());
        binding.insert(
            "exchangeId".into(),
            get(get(&o, "body"), "exchangeId").clone(),
        );
        binding.insert("side".into(), json!("no"));
        binding.insert(
            "action".into(),
            get(get(&o, "body"), "action").clone(),
        );
        binding.insert("quantity".into(), get(get(&o, "body"), "quantity").clone());
        binding.insert(
            "limit".into(),
            json!(money(price(&js_str(get(get(&o, "body"), "price")))?)),
        );
        binding.insert("createdAt".into(), get(&o, "createdAt").clone());
        binding.insert(
            "expirationDate".into(),
            get(get(&o, "body"), "expirationDate").clone(),
        );
        let mut verify_input = get(e, "evidence").as_object().cloned().unwrap_or_default();
        verify_input.insert("binding".into(), V::Object(binding));
        verify_input.insert("before".into(), get(&o, "before").clone());
        verify_input.insert("ack".into(), get(&o, "ack").clone());
        verify_input.insert(
            "dispatchReceipt".into(),
            get(&o, "fetchStarts")
                .as_array()
                .and_then(|a| a.first())
                .cloned()
                .unwrap_or(V::Null),
        );
        verify_input.insert("readAt".into(), get(e, "at").clone());
        verify_input.insert(
            "reservedFunding".into(),
            json!(if get(get(&o, "body"), "action").as_str() == Some("buy") {
                money(990_000_i128 * js_int(get(get(&o, "body"), "quantity")).unwrap_or(0) as i128)
            } else {
                "0.000000".to_string()
            }),
        );
        verify_input.insert("priorProof".into(), get(&o, "priorProof").clone());
        verify_input.insert("priorAfter".into(), get(&o, "priorAfter").clone());
        let proof = verify_scoped_rest_no_multishare(&V::Object(verify_input))?;
        let idx = find_active_index(&n).unwrap();
        if get(get(&o, "body"), "action").as_str() == Some("sell") {
            need(
                amount(get(&proof, "realizedPnl").as_str().unwrap_or(""))?
                    >= 5_000 * js_int(get(&proof, "filledQuantity")).unwrap_or(0) as i128,
                "ACTUAL_NET_PROFIT",
            )?;
            let retires = retire_sold_pairs(&n, &o, &proof)?;
            n.as_object_mut()
                .unwrap()
                .get_mut("pairRetirements")
                .and_then(|v| v.as_array_mut())
                .unwrap()
                .extend(retires);
        } else {
            let pairs = assign_fill(&n, &o, &proof)?;
            n.as_object_mut()
                .unwrap()
                .get_mut("assignments")
                .and_then(|v| v.as_array_mut())
                .unwrap()
                .extend(pairs);
        }
        need(
            get(&n, "assignments")
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|x| get(x, "assignmentId").as_str().unwrap_or("").to_string())
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == a.len()
                })
                .unwrap_or(false),
            "DUPLICATE_PAIR",
        )?;
        {
            let orders = n
                .as_object_mut()
                .unwrap()
                .get_mut("orders")
                .and_then(|v| v.as_array_mut())
                .unwrap();
            orders[idx]
                .as_object_mut()
                .unwrap()
                .insert("proof".into(), proof.clone());
            orders[idx]
                .as_object_mut()
                .unwrap()
                .insert("phase".into(), json!("reconciled"));
        }
        n.as_object_mut()
            .unwrap()
            .get_mut("currentInventory")
            .and_then(|v| v.as_object_mut())
            .unwrap()
            .insert(
                get(get(&o, "body"), "exchangeId").as_str().unwrap_or("").to_string(),
                json!({
                    "proof": proof,
                    "after": get(get(e, "evidence"), "after"),
                    "package": get(&o, "package"),
                }),
            );
        if get(get(&o, "body"), "action").as_str() == Some("buy")
            || get(&n, "counterInventory")
                .as_object()
                .map(|m| m.contains_key(get(get(&o, "body"), "exchangeId").as_str().unwrap_or("")))
                .unwrap_or(false)
        {
            let entry = get(&get(&n, "currentInventory"), get(get(&o, "body"), "exchangeId").as_str().unwrap_or("")).clone();
            n.as_object_mut()
                .unwrap()
                .get_mut("counterInventory")
                .and_then(|v| v.as_object_mut())
                .unwrap()
                .insert(
                    get(get(&o, "body"), "exchangeId").as_str().unwrap_or("").to_string(),
                    entry,
                );
        }
        return Ok(n);
    }
    need(false, "EVENT")?;
    unreachable!()
}
/// configuration(seed, activation, migration, code)
fn configuration(seed: &V, activation: &V, migration: &V, code: &V) -> Result<(), String> {
    need(
        get(seed, "economicReplayPerformed") == &json!(true)
            && get(seed, "stateHash").as_str()
                == Some(payload_hash(get(seed, "state"))?.as_str())
            && !get(seed, "legacyCodeIdentityHash").is_null()
            && get(migration, "schemaVersion") == &json!(1)
            && get(migration, "seedHash").as_str() == Some(payload_hash(seed)?.as_str())
            && same(get(migration, "legacyAnchor"), get(seed, "anchor"))?
            && get(migration, "legacyCodeIdentityHash") == get(seed, "legacyCodeIdentityHash")
            && get(migration, "implementationEpoch")
                .as_str()
                .is_some_and(|s| !s.is_empty())
            && hex(get(migration, "authorityHash")),
        "VERIFIED_MIGRATION",
    )?;
    need(
        get(activation, "schemaVersion") == &json!(1)
            && get(activation, "mode").as_str()
                == Some("baal_neutral_owned_counterpart_pilot_v2")
            && get(activation, "ownerId") == get(&get(&get(seed, "state"), "scope"), "ownerId")
            && get(activation, "runId") == get(&get(&get(seed, "state"), "scope"), "runId")
            && get(activation, "tournamentId")
                == get(&get(&get(seed, "state"), "scope"), "tournamentId")
            && get(activation, "seedHash").as_str() == Some(payload_hash(seed)?.as_str())
            && get(activation, "implementationEpoch")
                == get(migration, "implementationEpoch")
            && get(activation, "codeIdentityHash").as_str()
                == Some(payload_hash(code)?.as_str())
            && hex(get(activation, "authorityHash"))
            && get(activation, "authorityHash") == get(migration, "authorityHash")
            && js_int(get(activation, "maximumHedgeOrders")) == Some(1)
            && js_int(get(activation, "quantity")) == Some(1)
            && get(activation, "maximumOrderCost").as_str() == Some("0.995000")
            && get(activation, "maximumPairUnitCost").as_str() == Some("0.990000")
            && instant_v(get(activation, "exitExpiresAt"))?
                > instant_v(get(activation, "startedAt"))?
            && instant_v(get(activation, "exitExpiresAt"))?
                - instant_v(get(activation, "startedAt"))?
                <= 86_400_000
            && get(activation, "allowedFirstExchangeIds")
                .as_array()
                .is_some_and(|a| a.len() == 1)
            && js_int(&get(
                &get(
                    &get(
                        &get(&get(seed, "state"), "inventory"),
                        get(activation, "allowedFirstExchangeIds")[0]
                            .as_str()
                            .unwrap_or(""),
                    ),
                    "proof",
                ),
                "remainingOwnedQuantity",
            ))
            .map(|q| q > 0)
            .unwrap_or(false)
            && instant_v(get(activation, "hedgeExpiresAt"))?
                > instant_v(get(activation, "startedAt"))?
            && instant_v(get(activation, "hedgeExpiresAt"))?
                - instant_v(get(activation, "startedAt"))?
                <= 3_600_000,
        "BOUNDED_NEW_HEDGE_ONLY",
    )?;
    Ok(())
}
/// replayLiveHedgeJournal({bodyText,seed,activation,migration,expectedCodeIdentity})
pub fn replay_live_hedge_journal(
    body_text: &str,
    seed: &V,
    activation: &V,
    migration: &V,
    expected_code_identity: &V,
) -> Result<V, String> {
    configuration(seed, activation, migration, expected_code_identity)?;
    need(
        !body_text.is_empty()
            && body_text.ends_with('\n')
            && body_text.len() <= 128 * 1024 * 1024,
        "REPLAY_INPUT",
    )?;
    let mut state: Option<V> = None;
    let mut sequence: u64 = 0;
    let mut previous = String::new();
    let mut root_prefix: Option<V> = None;
    for line in body_text.trim_end().split('\n') {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let hash = get(&row, "hash").as_str().unwrap_or("").to_string();
        let mut body = row.as_object().cloned().unwrap_or_default();
        body.shift_remove("hash");
        let body = V::Object(body);
        need(
            js_stringify(&row) == line
                && js_int(get(&body, "sequence")) == Some((sequence + 1) as i64)
                && get(&body, "previous").as_str() == Some(previous.as_str())
                && sha(js_stringify(&body).as_bytes()) == hash
                && same(get(get(&body, "event"), "codeIdentity"), expected_code_identity)?,
            "CHAIN_CODE",
        )?;
        sequence += 1;
        if sequence == 1 {
            need(
                get(get(get(&body, "event"), "input"), "kind").as_str() == Some("seed")
                    && same(get(get(get(&body, "event"), "input"), "seed"), seed)?
                    && same(
                        get(get(get(&body, "event"), "input"), "activation"),
                        activation,
                    )?
                    && same(
                        get(get(get(&body, "event"), "input"), "migration"),
                        migration,
                    )?,
                "EXACT_SEED",
            )?;
        }
        state = Some(reduce(state.as_ref(), get(&get(&body, "event"), "input"))?);
        need(
            same(state.as_ref().unwrap(), get(&body, "state"))?,
            "ECONOMIC_STATE_REPLAY",
        )?;
        previous = hash.clone();
        if sequence == 1 {
            root_prefix = Some(json!({
                "bytes": line.len() + 1,
                "sha256": sha(format!("{line}\n").as_bytes()),
                "sequence": sequence,
                "hash": previous,
            }));
        }
    }
    let anchor = json!({
        "bytes": body_text.len(),
        "sha256": sha(body_text.as_bytes()),
        "sequence": sequence,
        "hash": previous,
    });
    let unknown = pilot_unknown(state.as_ref().unwrap_or(&V::Null));
    Ok(json!({
        "state": state.unwrap_or(V::Null),
        "anchor": anchor,
        "rootPrefix": root_prefix.unwrap_or(V::Null),
        "economicReplayPerformed": true,
        "unknownAcceptance": unknown,
    }))
}

fn created_ms(st: &fs::Metadata) -> f64 {
    #[cfg(unix)]
    {
        st.created()
            .ok()
            .map(|c| {
                c.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64()
                    * 1000.0
            })
            .unwrap_or(0.0)
    }
    #[cfg(not(unix))]
    {
        let _ = st;
        0.0
    }
}
#[cfg(unix)]
fn stat_ino(st: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    st.ino()
}
#[cfg(not(unix))]
fn stat_ino(_st: &fs::Metadata) -> u64 {
    0
}

pub struct NeutralPilotDeps {
    pub dispatch: Rc<WireDispatch>,
    pub raw_receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub assert_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub assert_financial_owner_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub install_funding_reservation: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_evidence: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub on_financial_halt: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    /// canCheckMechanical — defaults to can_operate.
    pub can_check_mechanical: Option<Rc<dyn Fn() -> bool>>,
    /// assertMechanicalAccountLease — defaults to assert_account_lease.
    pub assert_mechanical_account_lease: Option<Rc<dyn Fn() -> Result<V, String>>>,
    /// assertMechanicalFinancialOwnerLease — defaults to
    /// assert_financial_owner_lease.
    pub assert_mechanical_financial_owner_lease: Option<Rc<dyn Fn() -> Result<V, String>>>,
    pub now: Rc<dyn Fn() -> f64>,
    pub workspace_dir: PathBuf,
}

struct PilotInner {
    deps: NeutralPilotDeps,
    seed: V,
    seed_hash: String,
    original_inventory_hash: String,
    original_orders_hash: String,
    code: V,
    activation: V,
    migration: V,
    legacy_file: PathBuf,
    bytes: Vec<u8>,
    state: Option<V>,
    closed: bool,
    failed: bool,
    busy: bool,
    in_flight: Option<V>,
    journal: Journal,
    file: PathBuf,
    size: u64,
    hash: String,
    stat_ino: u64,
    stat_birthtime_ms: f64,
    last_guard_failure: Option<V>,
    fetch_deadline: Option<f64>,
    issued: std::collections::HashMap<usize, V>,
    local_timings: Vec<V>,
    lease: OwnerLease,
}

pub struct BaalNeutralOwnedCounterpartPilot {
    inner: Rc<RefCell<PilotInner>>,
    transport: OrderTransport,
}

fn measure<F, T>(inner: &Rc<RefCell<PilotInner>>, name: &str, f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    let start = Instant::now();
    let result = f();
    let mut entry = Map::new();
    entry.insert("name".into(), json!(name));
    entry.insert(
        "durationMs".into(),
        json!((start.elapsed().as_secs_f64() * 1_000_000.0).round() / 1000.0),
    );
    if let Err(e) = &result {
        let at = (inner.borrow().deps.now)();
        entry.insert("errorCode".into(), json!(diagnostic_code(e)));
        entry.insert("failedAt".into(), json!(iso(at)));
    }
    let mut b = inner.borrow_mut();
    b.local_timings.push(V::Object(entry));
    if b.local_timings.len() > 96 {
        b.local_timings.remove(0);
    }
    drop(b);
    result
}

fn mechanical(inner: &Rc<RefCell<PilotInner>>) -> Result<V, String> {
    {
        let b = inner.borrow();
        let f = b
            .deps
            .can_check_mechanical
            .clone()
            .unwrap_or_else(|| b.deps.can_operate.clone());
        need(!b.closed && !b.failed && f(), "STOPPED")?;
    }
    inner.borrow().lease.assert_ownership().map_err(|e| e.message)?;
    let l = {
        let f = {
            let b = inner.borrow();
            b.deps
                .assert_mechanical_account_lease
                .clone()
                .unwrap_or_else(|| b.deps.assert_account_lease.clone())
        };
        measure(inner, "basic_account_lease", move || sync(f()))?
    };
    let f_lease = {
        let f = {
            let b = inner.borrow();
            b.deps
                .assert_mechanical_financial_owner_lease
                .clone()
                .unwrap_or_else(|| b.deps.assert_financial_owner_lease.clone())
        };
        measure(inner, "basic_financial_lease", move || sync(f()))?
    };
    let (activation, migration) = {
        let b = inner.borrow();
        (b.activation.clone(), b.migration.clone())
    };
    for x in [&l, &f_lease] {
        need(
            get(x, "ownerId") == get(&activation, "ownerId")
                && get(x, "runId") == get(&activation, "runId")
                && get(x, "tournamentId") == get(&activation, "tournamentId")
                && get(x, "leaseId").as_str().is_some_and(|s| !s.is_empty()),
            "ACTUAL_ACCOUNT_LEASE",
        )?;
    }
    need(
        get(&f_lease, "implementationEpoch") == get(&migration, "implementationEpoch"),
        "FINANCIAL_OWNER_EPOCH",
    )?;
    need(
        l.as_object()
            .map(|o| {
                o.keys()
                    .all(|k| ["ownerId", "runId", "tournamentId", "leaseId"].contains(&k.as_str()))
            })
            .unwrap_or(false),
        "LEASE_RECEIPT_SCHEMA",
    )?;
    Ok(l)
}

fn basic(inner: &Rc<RefCell<PilotInner>>) -> Result<V, String> {
    need((inner.borrow().deps.can_operate)(), "STOPPED")?;
    let l = mechanical(inner)?;
    let (legacy_file, seed_bytes, workspace, code, seed_anchor_bytes) = {
        let b = inner.borrow();
        (
            b.legacy_file.clone(),
            b.bytes.clone(),
            b.deps.workspace_dir.clone(),
            b.code.clone(),
            js_int(get(get(&b.seed, "anchor"), "bytes")).unwrap_or(0) as usize,
        )
    };
    let actual = measure(inner, "basic_legacy_fresh_read", || {
        fs::read(&legacy_file).map_err(|e| e.to_string())
    })?;
    let inner2 = inner.clone();
    let actual2 = actual.clone();
    need(
        actual.len() == seed_anchor_bytes
            && measure(inner, "basic_legacy_exact_compare", move || {
                let _ = &inner2;
                Ok(actual2 == seed_bytes)
            })?
            && measure(inner, "basic_current_code_graph", || {
                let ws = {
                    let _ = &workspace;
                    inner.borrow().deps.workspace_dir.clone()
                };
                same(&live_coordinator_identity(&ws)?, &code)
            })?,
        "LEGACY_NOT_SEALED_OR_CODE_CHANGED",
    )?;
    need(
        l.as_object()
            .map(|o| {
                o.keys()
                    .all(|k| ["ownerId", "runId", "tournamentId", "leaseId"].contains(&k.as_str()))
            })
            .unwrap_or(false),
        "LEASE_RECEIPT_SCHEMA",
    )?;
    Ok(l)
}

fn append(inner: &Rc<RefCell<PilotInner>>, e: &V) -> Result<(), String> {
    basic(inner)?;
    let (file, size, hash, stat_in, stat_birth, event) = {
        let b = inner.borrow();
        (
            b.file.clone(),
            b.size,
            b.hash.clone(),
            b.stat_ino,
            b.stat_birthtime_ms,
            json!({"codeIdentity": b.code, "input": e}),
        )
    };
    let old = fs::read(&file).map_err(|er| er.to_string())?;
    let st = fs::metadata(&file).map_err(|er| er.to_string())?;
    need(
        stat_ino(&st) == stat_in && created_ms(&st) == stat_birth,
        "JOURNAL_CHANGED",
    )?;
    need(old.len() as u64 == size && sha(&old) == hash, "JOURNAL_CHANGED")?;
    let state = inner.borrow().state.clone();
    let next = reduce(state.as_ref(), e)?;
    need(
        (size + js_stringify(&next).len() as u64 + js_stringify(e).len() as u64)
            < 134_217_728,
        "JOURNAL_BOUND",
    )?;
    let commit_result = {
        let mut b = inner.borrow_mut();
        b.journal.commit(&next, &event)
    };
    match commit_result {
        Ok(_) => {
            basic(inner)?;
            let after = fs::read(&file).map_err(|er| er.to_string())?;
            need(after[..size as usize] == old[..], "PREFIX_CHANGED")?;
            let mut b = inner.borrow_mut();
            b.state = Some(next);
            b.size = after.len() as u64;
            b.hash = sha(&after);
            Ok(())
        }
        Err(f) => {
            inner.borrow_mut().failed = true;
            Err(f)
        }
    }
}

fn state_of(inner: &Rc<RefCell<PilotInner>>) -> V {
    inner.borrow().state.clone().unwrap_or(V::Null)
}
fn funding_of(inner: &Rc<RefCell<PilotInner>>) -> Result<V, String> {
    funding(&state_of(inner))
}

fn reserve(inner: &Rc<RefCell<PilotInner>>) -> Result<V, String> {
    let record = funding_of(inner)?;
    let r = {
        let b = inner.borrow();
        (b.deps.install_funding_reservation)(&record)
    };
    let r = match r {
        Ok(v) => sync(Ok(v))?,
        Err(e) => {
            inner.borrow_mut().failed = true;
            return Err(e);
        }
    };
    if let Err(f) = append(
        inner,
        &json!({"kind": "reservation", "receipt": r, "at": iso((inner.borrow().deps.now)())}),
    ) {
        inner.borrow_mut().failed = true;
        return Err(f);
    }
    Ok(r)
}

fn ensure_installed(inner: &Rc<RefCell<PilotInner>>) -> Result<V, String> {
    basic(inner)?;
    let record = funding_of(inner)?;
    let state = state_of(inner);
    let r = get(&state, "reservation").clone();
    let seed_hash = inner.borrow().seed_hash.clone();
    let migration_epoch = get(&inner.borrow().migration, "implementationEpoch").clone();
    let n = (inner.borrow().deps.now)() as i64;
    let mut valid = get(&r, "reservationHash") == get(&record, "reservationHash")
        && get(&r, "allocationHash") == get(&record, "allocationHash")
        && get(&r, "seedHash").as_str() == Some(seed_hash.as_str())
        && hex(get(&r, "globalLedgerHash"))
        && hex(get(&r, "budgetInstalledHash"))
        && instant_v(get(&r, "confirmedAt"))
            .map(|t| t <= n && n - t <= 20_000)
            .unwrap_or(false);
    if valid {
        let g = {
            let b = inner.borrow();
            sync((b.deps.get_global_risk)())?
        };
        let o = active(&state);
        valid = get(&g, "ownerId") == get(get(&state, "scope"), "ownerId")
            && get(&g, "runId") == get(get(&state, "scope"), "runId")
            && get(&g, "tournamentId") == get(get(&state, "scope"), "tournamentId")
            && get(&g, "complete") == &json!(true)
            && get(&g, "eventScopeComplete") == &json!(true)
            && get(&g, "soleCollector") == &json!(true)
            && get(&g, "stopped") == &json!(false)
            && get(&g, "unknownAcceptance") == &json!(false)
            && hex(get(&g, "sourceHash"))
            && get(&g, "unresolvedIntentIds").is_array()
            && get(&g, "unresolvedIntentIds")
                .as_array()
                .unwrap()
                .iter()
                .all(|id| id == &get(&o, "intentId").clone())
            && get(&g, "controllerReservationHash") == get(&record, "reservationHash")
            && get(&g, "globalLedgerHash") == get(&r, "globalLedgerHash")
            && get(&g, "budgetInstalledHash") == get(&r, "budgetInstalledHash")
            && get(&g, "allocationHash") == get(&record, "allocationHash")
            && get(&g, "seedHash").as_str() == Some(seed_hash.as_str())
            && get(&g, "implementationEpoch") == &migration_epoch
            && get(&g, "fundingSatisfied") == &json!(true)
            && get(&g, "installedCashLower").is_string()
            && get(&g, "cashLower") == get(&g, "installedCashLower")
            && instant_v(get(&g, "at"))
                .map(|t| t <= n && n - t <= 20_000)
                .unwrap_or(false);
    }
    if !valid {
        return reserve(inner);
    }
    Ok(r)
}

/// evidence(operation, firstEx=null, fenceRisk=null)
fn evidence_of(
    inner: &Rc<RefCell<PilotInner>>,
    operation: &str,
    first_ex: Option<&str>,
    fence_risk: &V,
) -> Result<V, String> {
    let state = state_of(inner);
    let o = active(&state);
    let body_ex = get(get(&o, "body"), "exchangeId").clone();
    let market_id = if !get(&o, "marketId").is_null() {
        get(&o, "marketId").clone()
    } else {
        get(get(&state, "scope"), "exchanges")
            .as_array()
            .and_then(|xs| {
                xs.iter()
                    .find(|x| Some(get(x, "exchangeId").as_str().unwrap_or("")) == first_ex)
                    .map(|x| get(x, "marketId").clone())
            })
            .unwrap_or(V::Null)
    };
    let ctx = json!({
        "operation": operation,
        "fenceRisk": fence_risk,
        "intentId": get(&o, "intentId"),
        "orderId": get(&o, "orderId"),
        "firstExchangeId": first_ex.map(|x| json!(x)).unwrap_or_else(|| get(&o, "firstExchangeId").clone()),
        "exchangeId": if !body_ex.is_null() { body_ex } else { first_ex.map(|x| json!(x)).unwrap_or(V::Null) },
        "marketId": market_id,
    });
    let b = inner.borrow();
    sync((b.deps.get_evidence)(&ctx))
}

/// validate(permit, spec, network)
fn validate(
    inner: &Rc<RefCell<PilotInner>>,
    permit: &V,
    spec: &V,
    network: bool,
) -> Result<bool, String> {
    mechanical(inner)?;
    let state = state_of(inner);
    let o = active(&state);
    need(
        !o.is_null()
            && get(permit, "intentId") == get(&o, "intentId")
            && get(permit, "bodyHash") == get(&o, "bodyHash")
            && get(spec, "method").as_str() == Some("POST")
            && get(spec, "endpoint").as_str() == Some("/orders")
            && get(spec, "bodyJson") == get(&o, "bodyJson")
            && !pilot_unknown(&state),
        "HEDGE_ONLY_FIXED_PERMIT",
    )?;
    let r = get(&state, "reservation").clone();
    let n = (inner.borrow().deps.now)() as i64;
    need(
        get(&r, "reservationHash") == get(&funding_of(inner)?, "reservationHash")
            && get(&r, "allocationHash").as_str()
                == Some(payload_hash(get(&state, "allocation"))?.as_str())
            && hex(get(&r, "globalLedgerHash"))
            && instant_v(get(&r, "confirmedAt"))
                .map(|t| t <= n && n - t <= 20_000)
                .unwrap_or(false),
        "FULL_RESERVE_INSTALLED",
    )?;
    let before_fence = json!({
        "recordHash": get(&funding_of(inner)?, "reservationHash"),
        "bodyHash": get(&o, "bodyHash"),
        "bodyJson": get(&o, "bodyJson"),
        "custodyHash": payload_hash(get(&state, "currentInventory"))?,
        "policyHash": payload_hash(&json!({
            "scope": get(&state, "scope"),
            "allocation": get(&state, "allocation"),
            "activation": get(&state, "activation"),
            "migration": get(&state, "migration"),
        }))?,
        "unknown": pilot_unknown(&state),
    });
    let l = measure(inner, "guard_current_lease", || basic(inner))?;
    let name = if network {
        "network_current_budget_risk"
    } else {
        "admission_current_budget_risk"
    };
    let g = {
        let f = inner.borrow().deps.get_global_risk.clone();
        measure(inner, name, move || sync(f()))?
    };
    let name2 = if network {
        "network_cached_evidence"
    } else {
        "admission_cached_evidence"
    };
    let o_action_str = get(get(&o, "body"), "action").as_str().unwrap_or("").to_string();
    let o_action = o_action_str.clone();
    let g_c = g.clone();
    let e = measure(inner, name2, move || {
        evidence_of(
            inner,
            if o_action == "buy" { "place_hedge" } else { "place_sell" },
            None,
            &g_c,
        )
    })?;
    let name3 = if network {
        "network_durable_fence_append"
    } else {
        "admission_durable_event_append"
    };
    {
        let inner2 = inner.clone();
        let e2 = e.clone();
        let g2 = g.clone();
        let l2 = l.clone();
        measure(inner, name3, move || {
            let lease_hash = payload_hash(&l2)?;
            append(
                &inner2,
                &json!({
                    "kind": if network { "network_fence" } else { "admitted" },
                    "evidence": e2,
                    "globalRisk": g2,
                    "lease": l2,
                    "leaseHash": lease_hash,
                    "at": iso((inner2.borrow().deps.now)()),
                }),
            )
        })?;
    }
    if network {
        let state = state_of(inner);
        let o = active(&state);
        let post_l = measure(inner, "network_postappend_current_lease", || basic(inner))?;
        need(same(&post_l, &l)?, "NETWORK_LEASE_CHANGED_AFTER_COMMIT")?;
        {
            let inner2 = inner.clone();
            measure(inner, "network_postappend_current_journal", || {
                let (file, size, hash, stat_in, stat_birth, jseq, jprev) = {
                    let b = inner2.borrow();
                    (
                        b.file.clone(),
                        b.size,
                        b.hash.clone(),
                        b.stat_ino,
                        b.stat_birthtime_ms,
                        b.journal.sequence,
                        b.journal.previous.clone(),
                    )
                };
                let st = fs::metadata(&file).map_err(|er| er.to_string())?;
                need(
                    stat_ino(&st) == stat_in && created_ms(&st) == stat_birth,
                    "CURRENT_JOURNAL_CHANGED",
                )?;
                let fresh = fs::read(&file).map_err(|er| er.to_string())?;
                let last_start = fresh[..fresh.len().saturating_sub(1)]
                    .iter()
                    .rposition(|b| *b == 10)
                    .map(|i| i + 1)
                    .unwrap_or(0);
                let last: V = serde_json::from_str(
                    std::str::from_utf8(&fresh[last_start..]).map_err(|er| er.to_string())?,
                )
                .map_err(|er| er.to_string())?;
                need(
                    fresh.len() as u64 == size
                        && sha(&fresh) == hash
                        && js_int(get(&last, "sequence")) == Some(jseq as i64)
                        && get(&last, "hash").as_str() == Some(jprev.as_str()),
                    "CURRENT_JOURNAL_CHANGED",
                )?;
                Ok(())
            })?;
        }
        need(
            get(&before_fence, "recordHash") == get(&funding_of(inner)?, "reservationHash")
                && get(&before_fence, "bodyHash") == get(&o, "bodyHash")
                && get(&before_fence, "bodyJson") == get(&o, "bodyJson")
                && get(&o, "bodyHash").as_str()
                    == Some(payload_hash(get(&o, "body"))?.as_str())
                && get(&before_fence, "custodyHash").as_str()
                    == Some(payload_hash(get(&state, "currentInventory"))?.as_str())
                && get(&before_fence, "policyHash").as_str()
                    == Some(
                        payload_hash(&json!({
                            "scope": get(&state, "scope"),
                            "allocation": get(&state, "allocation"),
                            "activation": get(&state, "activation"),
                            "migration": get(&state, "migration"),
                        }))?
                        .as_str(),
                    )
                && get(&before_fence, "unknown") == &json!(pilot_unknown(&state)),
            "NON_ECONOMIC_FENCE_ONLY",
        )?;
        need(
            (inner.borrow().deps.now)() < instant_v(get(get(&o, "body"), "expirationDate"))? as f64,
            "ORDER_EXPIRED_AFTER_FENCE",
        )?;
        let o_exchange_id = get(get(&o, "body"), "exchangeId").as_str().unwrap_or("").to_string();
        let o_expiration = get(get(&o, "body"), "expirationDate").clone();
        {
            let inner2 = inner.clone();
            let e2 = e.clone();
            let g2 = g.clone();
            measure(inner, "network_postappend_policy_validation", move || {
                if o_action_str == "buy" {
                    guarded(
                        &inner2.borrow().state.clone().unwrap_or(V::Null),
                        &e2,
                        &g2,
                        (inner2.borrow().deps.now)() as i64,
                    )
                    .map(|_| ())
                } else {
                    sell_proposal(
                        &inner2.borrow().state.clone().unwrap_or(V::Null),
                        &o_exchange_id,
                        &e2,
                        &g2,
                        (inner2.borrow().deps.now)() as i64,
                    )
                    .map(|_| ())
                }
            })?;
        }
        {
            let mut b = inner.borrow_mut();
            b.fetch_deadline = Some(std::cmp::min(
                instant_v(&o_expiration)?,
                std::cmp::min(
                    instant_v(get(get(&e, "graphReceipt"), "requestStartedAt"))? + 5_000,
                    instant_v(get(get(&e, "collateralReceipt"), "requestStartedAt"))? + 5_000,
                ),
            ) as f64);
        }
    }
    Ok(true)
}

fn capture(inner: &Rc<RefCell<PilotInner>>, response: &V) -> Result<(), String> {
    let state = state_of(inner);
    let o = active(&state);
    let r = {
        let b = inner.borrow();
        sync((b.deps.raw_receipt_of)(
            response,
            &json!({"method": "POST", "requestBodyHash": get(&o, "bodyHash")}),
        ))?
    };
    need(
        js_int(get(&r, "status")) == js_int(get(response, "status"))
            && get(&r, "endpoint").as_str() == Some("/orders")
            && instant_v(get(&r, "requestStartedAt"))?
                >= instant_v(get(response, "requestStartedAt"))?
            && instant_v(get(&r, "receivedAt"))? <= instant_v(get(response, "receivedAt"))?
            && same(
                &parse_number_lexemes(get(&r, "bodyText").as_str().unwrap_or(""))?.0,
                get(response, "data"),
            )?,
        "RAW_RESPONSE_BINDING",
    )?;
    append(inner, &json!({"kind": "wire", "receipt": r, "at": iso((inner.borrow().deps.now)())}))?;
    if pilot_unknown(&state_of(inner)) {
        let b = inner.borrow();
        sync((b.deps.on_financial_halt)(&json!({
            "kind": "non_neutral_or_unknown_hedge",
            "intentId": get(&o, "intentId"),
            "sourceHash": get(&r, "sourceBodyHash"),
            "allWritersStop": true,
        })))?;
    }
    Ok(())
}

impl BaalNeutralOwnedCounterpartPilot {
    /// openBaalNeutralOwnedCounterpartPilotV2(options)
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        dir: &Path,
        workspace: &Path,
        legacy_file: &Path,
        legacy_code_identity: &V,
        legacy_scope: &V,
        legacy_allocation: &V,
        activation: &V,
        migration: &V,
        deps: NeutralPilotDeps,
    ) -> Result<Self, String> {
        need(
            dir.is_absolute() && workspace.is_absolute() && legacy_file.is_absolute(),
            "DEPENDENCIES",
        )?;
        let bytes = fs::read(legacy_file).map_err(|e| e.to_string())?;
        let seed = replay_baal_scaled_controller_v1(&json!({
            "bodyText": std::str::from_utf8(&bytes).map_err(|e| e.to_string())?,
            "expectedCodeIdentity": legacy_code_identity,
            "expectedScope": legacy_scope,
            "expectedAllocation": legacy_allocation,
        }))?;
        let seed_hash = payload_hash(&seed)?;
        let original_inventory_hash = payload_hash(get(&get(&seed, "state"), "inventory"))?;
        let original_orders_hash = payload_hash(get(&get(&seed, "state"), "orders"))?;
        let code = live_coordinator_identity(workspace)?;
        configuration(&seed, activation, migration, &code)?;
        let mut lease = acquire_owner_lease(
            dir,
            get(activation, "ownerId").as_str().unwrap_or(""),
            get(activation, "runId").as_str().unwrap_or(""),
            "neutral-owned-counterpart-pilot",
            (deps.now)(),
        )
        .map_err(|e| e.message)?;
        let file = dir.join("neutral-owned-counterpart-pilot-v2.jsonl");
        let inner = Rc::new(RefCell::new(PilotInner {
                deps: NeutralPilotDeps {
                    dispatch: deps.dispatch.clone(),
                    raw_receipt_of: deps.raw_receipt_of.clone(),
                    assert_account_lease: deps.assert_account_lease.clone(),
                    assert_financial_owner_lease: deps.assert_financial_owner_lease.clone(),
                    install_funding_reservation: deps.install_funding_reservation.clone(),
                    get_global_risk: deps.get_global_risk.clone(),
                    get_evidence: deps.get_evidence.clone(),
                    on_financial_halt: deps.on_financial_halt.clone(),
                    can_operate: deps.can_operate.clone(),
                    can_check_mechanical: deps.can_check_mechanical.clone(),
                    assert_mechanical_account_lease: deps.assert_mechanical_account_lease.clone(),
                    assert_mechanical_financial_owner_lease: deps
                        .assert_mechanical_financial_owner_lease
                        .clone(),
                    now: deps.now.clone(),
                    workspace_dir: workspace.to_path_buf(),
                },
                seed: seed.clone(),
                seed_hash: seed_hash.clone(),
                original_inventory_hash,
                original_orders_hash,
                code: code.clone(),
                activation: activation.clone(),
                migration: migration.clone(),
                legacy_file: legacy_file.to_path_buf(),
                bytes,
                state: None,
                closed: false,
                failed: false,
                busy: false,
                in_flight: None,
                journal: Journal {
                    file: file.clone(),
                    sequence: 0,
                    previous: String::new(),
                    last: None,
                },
                file: file.clone(),
                size: 0,
                hash: String::new(),
                stat_ino: 0,
                stat_birthtime_ms: 0.0,
                last_guard_failure: None,
                fetch_deadline: None,
                issued: std::collections::HashMap::new(),
                local_timings: Vec::new(),
                lease,
            }));
        if let Err(e) = (|| -> Result<(), String> {
            basic(&inner)?;
            if !file.exists() {
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&file)
                    .map_err(|e| e.to_string())?;
            }
            let data = fs::read(&file).map_err(|e| e.to_string())?;
            need(data.is_empty() || data.last() == Some(&10), "PARTIAL_JOURNAL")?;
            inner.borrow_mut().journal = Journal::open(&file)?;
            if !data.is_empty() {
                let replayed = replay_live_hedge_journal(
                    std::str::from_utf8(&data).map_err(|e| e.to_string())?,
                    &seed,
                    activation,
                    migration,
                    &code,
                )?;
                inner.borrow_mut().state = Some(get(&replayed, "state").clone());
            }
            {
                let mut b = inner.borrow_mut();
                b.size = data.len() as u64;
                b.hash = sha(&data);
                let st = fs::metadata(&file).map_err(|e| e.to_string())?;
                b.stat_ino = stat_ino(&st);
                b.stat_birthtime_ms = created_ms(&st);
            }
            Ok(())
        })() {
            inner.borrow_mut().lease.release().ok();
            return Err(e);
        }
        if inner.borrow().state.is_none() {
            if let Err(e) = append(
                &inner,
                &json!({
                    "kind": "seed",
                    "seed": seed,
                    "seedHash": seed_hash,
                    "activation": activation,
                    "migration": migration,
                    "at": iso((inner.borrow().deps.now)()),
                }),
            ) {
                inner.borrow_mut().lease.release().ok();
                return Err(e);
            }
        }
        let transport = {
            let inner_c = inner.clone();
            let scope = get(&state_of(&inner), "scope").clone();
            let mut by_exchange = BTreeMap::new();
            for x in get(&scope, "exchanges").as_array().cloned().unwrap_or_default() {
                by_exchange.insert(
                    get(&x, "exchangeId").as_str().unwrap_or("").to_string(),
                    get(&x, "marketId").as_str().unwrap_or("").to_string(),
                );
            }
            let tscope = TransportScope {
                tournament_id: get(&scope, "tournamentId").as_str().unwrap_or("").to_string(),
                tournament_slug: get(&scope, "tournamentSlug").as_str().unwrap_or("").to_string(),
                by_exchange,
                by_market: by_market_of(&scope),
            };
            let wrap_inner = inner.clone();
            let wrap_dispatch = inner.borrow().deps.dispatch.clone();
            let wrap: Box<crate::transport::Dispatch> = Box::new(
                move |spec: &V, bd: Option<&Guard>, bn: Option<&Guard>| {
                    let bd_ptr = bd.map(|g| g as *const Guard as *const () as usize).unwrap_or(0);
                    let bn_ptr = bn.map(|g| g as *const Guard as *const () as usize).unwrap_or(0);
                    wrap_inner.borrow_mut().issued.insert(
                        bd_ptr,
                        json!({
                            "method": get(spec, "method"),
                            "endpoint": get(spec, "endpoint"),
                            "bodyJson": get(spec, "bodyJson"),
                            "beforeNetworkPtr": bn_ptr as u64,
                        }),
                    );
                    wrap_inner.borrow_mut().in_flight = Some(spec.clone());
                    let result = (|| -> Result<V, String> {
                        match (wrap_dispatch)(spec, bd, bn) {
                            Ok(r) => {
                                capture(&wrap_inner, &r)?;
                                Ok(r)
                            }
                            Err(e) => {
                                if let Some(wr) = e.wire_receipt.as_ref() {
                                    capture(&wrap_inner, wr)?;
                                }
                                Err(e.message)
                            }
                        }
                    })();
                    wrap_inner.borrow_mut().issued.remove(&bd_ptr);
                    wrap_inner.borrow_mut().in_flight = None;
                    result.map_err(|code| OrderTransportError {
                        code,
                        status: None,
                        api_code: None,
                        outcome: "blocked".into(),
                        may_have_executed: false,
                        retry_same_request_only: false,
                        retry_after_ms: 0.0,
                        semantic_mismatch: false,
                    })
                },
            );
            let inner_cw = inner.clone();
            let inner_ow = inner.clone();
            let inner_vp = inner.clone();
            let inner_vn = inner.clone();
            let inner_vl = inner.clone();
            let now_fn = inner.borrow().deps.now.clone();
            let t_result = OrderTransport::new(OrderTransportOptions {
                dispatch: wrap,
                scope: tscope,
                can_write: Box::new(move || {
                    let _ = &inner_cw;
                    mechanical(&inner_cw).is_ok() && !pilot_unknown(&state_of(&inner_cw))
                }),
                owns_order: Box::new(move |id: &str| {
                    let o = active(&state_of(&inner_ow));
                    get(&o, "orderId").as_str() == Some(id)
                }),
                validate_permit: Box::new(move |p: &V, s: &V| {
                    guarded_permit(&inner_vp, p, s, false)
                }),
                validate_network_permit: Box::new(move |p: &V, s: &V| {
                    guarded_permit(&inner_vn, p, s, true)
                }),
                validate_lease: Box::new(move |l: &V, _s: &V| {
                    match (|| -> Result<bool, String> {
                        Ok(same(l, &mechanical(&inner_vl)?)?)
                    })() {
                        Ok(v) => Ok(v),
                        Err(code) => Err(OrderTransportError {
                            code,
                            status: None,
                            api_code: None,
                            outcome: "blocked".into(),
                            may_have_executed: false,
                            retry_same_request_only: false,
                            retry_after_ms: 0.0,
                            semantic_mismatch: false,
                        }),
                    }
                }),
                now: Box::new(move || now_fn() as i64),
                max_pages: 10,
            });
            let _ = inner_c;
            match t_result {
                Ok(t) => t,
                Err(e) => {
                    inner.borrow_mut().lease.release().ok();
                    return Err(e.code);
                }
            }
        };
        Ok(Self { inner, transport })
    }

    fn run_guard<F, T>(&self, f: F) -> Result<T, String>
    where
        F: FnOnce() -> Result<T, String>,
    {
        need(!self.inner.borrow().busy, "BUSY")?;
        self.inner.borrow_mut().busy = true;
        let r = f();
        self.inner.borrow_mut().busy = false;
        r
    }

    fn active_intent_status_of(state: &V) -> V {
        let o = active(state);
        if o.is_null() {
            return V::Null;
        }
        json!({
            "intentId": get(&o, "intentId"),
            "firstExchangeId": get(&o, "firstExchangeId"),
            "orderId": get(&o, "orderId"),
            "action": get(get(&o, "body"), "action"),
            "marketId": get(&o, "marketId"),
            "exchangeId": get(get(&o, "body"), "exchangeId"),
            "body": get(&o, "body"),
            "bodyHash": get(&o, "bodyHash"),
            "phase": get(&o, "phase"),
            "createdAt": get(&o, "createdAt"),
            "packageLegs": get(get(&o, "package"), "legs"),
            "actualFetchCount": get(&o, "fetchStarts").as_array().map(|a| a.len()).unwrap_or(0),
            "mayHaveExecuted": get(&o, "mayHaveExecuted"),
            "cancelPending": false,
        })
    }
    /// activeIntentStatus()
    pub fn active_intent_status(&self) -> V {
        Self::active_intent_status_of(&state_of(&self.inner))
    }
    /// status()
    pub fn status(&self) -> V {
        let b = self.inner.borrow();
        let state = b.state.clone().unwrap_or(V::Null);
        let o = active(&state);
        let mut inventory = Map::new();
        if let Some(m) = get(&state, "currentInventory").as_object() {
            for (ex, x) in m {
                inventory.insert(
                    ex.clone(),
                    json!({
                        "quantity": get(get(x, "proof"), "remainingOwnedQuantity"),
                        "cost": get(get(x, "proof"), "remainingOwnedCost"),
                        "realizedPnl": get(get(x, "proof"), "cumulativeRealizedPnl"),
                    }),
                );
            }
        }
        let realized: i128 = get(&state, "currentInventory")
            .as_object()
            .map(|m| {
                m.values()
                    .map(|x| {
                        amount(get(get(x, "proof"), "cumulativeRealizedPnl").as_str().unwrap_or(""))
                            .unwrap_or(0)
                    })
                    .sum()
            })
            .unwrap_or(0);
        let phase = if !o.is_null() {
            get(&o, "phase").clone()
        } else if get(&state, "orders").as_array().is_some_and(|a| !a.is_empty()) {
            json!("terminal")
        } else {
            json!("ready")
        };
        let mut out = Map::new();
        out.insert("mode".into(), get(&state, "mode").clone());
        for (k, v) in get(&state, "scope").as_object().cloned().unwrap_or_default() {
            out.insert(k, v);
        }
        out.insert("seedHash".into(), json!(b.seed_hash));
        out.insert(
            "implementationEpoch".into(),
            get(&b.migration, "implementationEpoch").clone(),
        );
        out.insert("sequence".into(), json!(b.journal.sequence));
        out.insert("hash".into(), json!(b.journal.previous));
        out.insert("phase".into(), phase);
        out.insert("intent".into(), Self::active_intent_status_of(&state));
        out.insert("unknownAcceptance".into(), json!(pilot_unknown(&state)));
        out.insert("assignments".into(), get(&state, "assignments").clone());
        out.insert("pairRetirements".into(), get(&state, "pairRetirements").clone());
        out.insert("inventory".into(), V::Object(inventory));
        out.insert("realizedPnl".into(), json!(money(realized)));
        out.insert("matchedQuantity".into(), json!(matched(&state)));
        out.insert(
            "originalInventoryHash".into(),
            json!(b.original_inventory_hash),
        );
        out.insert("originalOrdersHash".into(), json!(b.original_orders_hash));
        out.insert(
            "funding".into(),
            funding(&state).unwrap_or(V::Null),
        );
        out.insert(
            "competitionOrders".into(),
            json!(get(&state, "orders")
                .as_array()
                .map(|a| a
                    .iter()
                    .filter(|o| {
                        get(o, "fetchStarts").as_array().is_some_and(|f| !f.is_empty())
                    })
                    .count())
                .unwrap_or(0)),
        );
        out.insert(
            "hedgeIntentCount".into(),
            json!(get(&state, "orders")
                .as_array()
                .map(|a| a
                    .iter()
                    .filter(|o| get(get(o, "body"), "action").as_str() == Some("buy"))
                    .count())
                .unwrap_or(0)),
        );
        out.insert(
            "abandonedIntentCount".into(),
            json!(get(&state, "orders")
                .as_array()
                .map(|a| a
                    .iter()
                    .filter(|o| get(o, "phase").as_str() == Some("abandoned"))
                    .count())
                .unwrap_or(0)),
        );
        out.insert(
            "executionOwnerEpoch".into(),
            get(&b.migration, "implementationEpoch").clone(),
        );
        out.insert("ownedSellSupported".into(), json!(true));
        out.insert(
            "handbackRequiresExplicitImplementationMigration".into(),
            json!(true),
        );
        out.insert("cashProfitEstablished".into(), json!(false));
        out.insert(
            "recentLocalTimings".into(),
            V::Array(b.local_timings.clone()),
        );
        out.insert(
            "lastGuardFailure".into(),
            b.last_guard_failure.clone().unwrap_or(V::Null),
        );
        out.insert("closed".into(), json!(b.closed));
        out.insert("failed".into(), json!(b.failed));
        out.insert("busy".into(), json!(b.busy));
        V::Object(out)
    }

    /// authorizes(spec)
    pub fn authorizes(&self, spec: &V, before_dispatch: &Guard, before_network: &Guard) -> bool {
        let r: Result<bool, String> = (|| {
            mechanical(&self.inner)?;
            let bd_ptr = before_dispatch as *const Guard as *const () as usize;
            let p = self.inner.borrow().issued.get(&bd_ptr).cloned();
            let p = match p {
                Some(p) => p,
                None => return Ok(false),
            };
            Ok(!pilot_unknown(&state_of(&self.inner))
                && get(&p, "method") == get(spec, "method")
                && get(&p, "endpoint") == get(spec, "endpoint")
                && get(&p, "bodyJson") == get(spec, "bodyJson")
                && (before_network as *const Guard as *const () as usize)
                    == get(&p, "beforeNetworkPtr").as_u64().unwrap_or(0) as usize
                && self
                    .transport
                    .authorize_request(spec)
                    .map(|v| v == true)
                    .unwrap_or(false))
        })();
        r.unwrap_or(false)
    }

    /// onBeforeFetch(meta)
    pub fn on_before_fetch(&self, meta: &V) -> Result<V, String> {
        mechanical(&self.inner)?;
        let state = state_of(&self.inner);
        let o = active(&state);
        let deadline = self.inner.borrow().fetch_deadline;
        need(
            self.inner.borrow().in_flight.is_some()
                && !o.is_null()
                && get(meta, "method").as_str() == Some("POST")
                && get(meta, "endpoint").as_str() == Some("/orders")
                && get(meta, "requestBodyHash") == get(&o, "bodyHash")
                && deadline.is_some()
                && (self.inner.borrow().deps.now)() < deadline.unwrap(),
            "FINAL_FETCH_CLOCK_OR_BINDING",
        )?;
        Ok(json!({"notAfter": deadline.unwrap()}))
    }

    /// onFetchStart(meta)
    pub fn on_fetch_start(&self, meta: &V) -> Result<bool, String> {
        if get(meta, "method").as_str() == Some("GET")
            || self.inner.borrow().in_flight.is_none()
        {
            return Ok(false);
        }
        let result = (|| -> Result<bool, String> {
            basic(&self.inner)?;
            let state = state_of(&self.inner);
            let o = active(&state);
            let f = get(&o, "fences").as_array().and_then(|a| a.last()).cloned();
            need(
                f.is_some()
                    && get(meta, "method").as_str() == Some("POST")
                    && get(meta, "endpoint").as_str() == Some("/orders")
                    && get(meta, "requestBodyHash") == get(&o, "bodyHash")
                    && get(meta, "fetchInvocationId").is_string()
                    && get(&self.inner.borrow().in_flight.clone().unwrap_or(V::Null), "bodyJson")
                        == get(&o, "bodyJson"),
                "BOUND_ACTUAL_FETCH",
            )?;
            let f = f.unwrap();
            let mut m = meta.as_object().cloned().unwrap_or_default();
            m.insert("kind".into(), json!("place"));
            m.insert("ownerId".into(), get(get(&state, "scope"), "ownerId").clone());
            m.insert("runId".into(), get(get(&state, "scope"), "runId").clone());
            m.insert(
                "tournamentId".into(),
                get(get(&state, "scope"), "tournamentId").clone(),
            );
            m.insert("intentId".into(), get(&o, "intentId").clone());
            m.insert(
                "idempotencyKey".into(),
                get(get(&o, "body"), "idempotencyKey").clone(),
            );
            m.insert("requestBodyJson".into(), get(&o, "bodyJson").clone());
            m.insert("requestBodyHash".into(), get(&o, "bodyHash").clone());
            m.insert("leaseIdentity".into(), get(&f, "lease").clone());
            m.insert("leaseHash".into(), get(&f, "leaseHash").clone());
            append(
                &self.inner,
                &json!({
                    "kind": "fetch_start",
                    "meta": V::Object(m),
                    "at": iso((self.inner.borrow().deps.now)()),
                }),
            )?;
            Ok(true)
        })();
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                self.inner.borrow_mut().failed = true;
                Err(e)
            }
        }
    }

    /// journalSource()
    pub fn journal_source(&self) -> Result<V, String> {
        let (file, size, hash, jseq, jprev) = {
            let b = self.inner.borrow();
            (
                b.file.clone(),
                b.size,
                b.hash.clone(),
                b.journal.sequence,
                b.journal.previous.clone(),
            )
        };
        let bytes = fs::read(&file).map_err(|e| e.to_string())?;
        let first_nl = bytes.iter().position(|b| *b == 10).unwrap_or(bytes.len() - 1);
        let first = &bytes[..first_nl + 1];
        let row: V = serde_json::from_str(std::str::from_utf8(first).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let state = state_of(&self.inner);
        Ok(json!({
            "journalFile": file.to_string_lossy(),
            "record": funding(&state)?,
            "sequence": jseq,
            "hash": jprev,
            "rootPrefix": {
                "bytes": first.len(),
                "sha256": sha(first),
                "sequence": get(&row, "sequence"),
                "hash": get(&row, "hash"),
            },
            "headPrefix": {
                "bytes": size,
                "sha256": hash,
                "sequence": jseq,
                "hash": jprev,
            },
            "status": self.status(),
        }))
    }

    /// abandonPrepared()
    pub fn abandon_prepared(&self) -> Result<V, String> {
        self.run_guard(|| {
            basic(&self.inner)?;
            need(self.inner.borrow().in_flight.is_none(), "ABANDON_IN_FLIGHT")?;
            append(
                &self.inner,
                &json!({"kind": "abandoned_prepared", "at": iso((self.inner.borrow().deps.now)())}),
            )?;
            reserve(&self.inner)?;
            Ok(self.status())
        })
    }

    /// inventory(ex)
    pub fn inventory(&self, ex: &str) -> V {
        get(&get(&state_of(&self.inner), "currentInventory"), ex).clone()
    }

    /// orderBinding(id)
    pub fn order_binding(&self, id: &V) -> V {
        let state = state_of(&self.inner);
        let o = active(&state);
        if get(&o, "orderId").as_str() != Some(js_str(id).as_str()) || o.is_null() {
            return V::Null;
        }
        let mut out = Self::active_intent_status_of(&state)
            .as_object()
            .cloned()
            .unwrap_or_default();
        for (k, v) in get(&state, "scope").as_object().cloned().unwrap_or_default() {
            out.insert(k, v);
        }
        out.insert(
            "idempotencyKey".into(),
            get(get(&o, "body"), "idempotencyKey").clone(),
        );
        out.insert("side".into(), json!("no"));
        out.insert("quantity".into(), get(get(&o, "body"), "quantity").clone());
        out.insert(
            "limit".into(),
            json!(money(
                price(&js_str(get(get(&o, "body"), "price"))).unwrap_or(0)
            )),
        );
        out.insert(
            "expirationDate".into(),
            get(get(&o, "body"), "expirationDate").clone(),
        );
        V::Object(out)
    }

    /// prepareSell(ex, {requestedQuantity=null}={})
    pub fn prepare_sell(&self, ex: &str, requested_quantity: Option<&V>) -> Result<V, String> {
        self.run_guard(|| {
            basic(&self.inner)?;
            let state = state_of(&self.inner);
            need(active(&state).is_null(), "UNKNOWN_OR_ACTIVE")?;
            let mut captured = evidence_of(&self.inner, "prepare_sell", Some(ex), &V::Null)?;
            captured.as_object_mut().unwrap().insert(
                "requestedQuantity".into(),
                requested_quantity.cloned().unwrap_or(V::Null),
            );
            let g_risk = {
                let b = self.inner.borrow();
                sync((b.deps.get_global_risk)())?
            };
            ensure_installed(&self.inner)?;
            append(
                &self.inner,
                &json!({
                    "kind": "prepared_sell",
                    "exchangeId": ex,
                    "evidence": captured,
                    "globalRisk": g_risk,
                    "intentId": format!("neutral-owned-sell-{}", uuid::Uuid::new_v4()),
                    "key": uuid::Uuid::new_v4().to_string(),
                    "at": iso((self.inner.borrow().deps.now)()),
                }),
            )?;
            reserve(&self.inner)?;
            Ok(self.status())
        })
    }

    /// pairCensus(firstEx)
    pub fn pair_census(&self, first_ex: &str) -> Result<V, String> {
        basic(&self.inner)?;
        pair_census_of(
            &state_of(&self.inner),
            first_ex,
            &json!(iso((self.inner.borrow().deps.now)())),
        )
    }

    /// fundingReservation()
    pub fn funding_reservation(&self) -> Result<V, String> {
        funding(&state_of(&self.inner))
    }

    /// fundingReservationStatus()
    pub fn funding_reservation_status(&self) -> Result<V, String> {
        Ok(json!({
            "record": funding(&state_of(&self.inner))?,
            "receipt": get(&state_of(&self.inner), "reservation"),
        }))
    }

    /// refreshReservation()
    pub fn refresh_reservation(&self) -> Result<V, String> {
        self.run_guard(|| {
            basic(&self.inner)?;
            reserve(&self.inner)?;
            Ok(self.status())
        })
    }

    /// prepareHedge(firstEx)
    pub fn prepare_hedge(&self, first_ex: &str) -> Result<V, String> {
        self.run_guard(|| {
            basic(&self.inner)?;
            let state = state_of(&self.inner);
            need(
                active(&state).is_null()
                    && get(&state, "orders")
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter(|o| get(get(o, "body"), "action").as_str() == Some("buy"))
                                .count()
                                == 0
                        })
                        .unwrap_or(false),
                "ONE_ORDER_PILOT",
            )?;
            let captured = evidence_of(&self.inner, "prepare_hedge", Some(first_ex), &V::Null)?;
            let g_risk = {
                let b = self.inner.borrow();
                sync((b.deps.get_global_risk)())?
            };
            ensure_installed(&self.inner)?;
            append(
                &self.inner,
                &json!({
                    "kind": "prepared",
                    "evidence": captured,
                    "globalRisk": g_risk,
                    "intentId": format!("neutral-owned-hedge-{}", uuid::Uuid::new_v4()),
                    "key": uuid::Uuid::new_v4().to_string(),
                    "at": iso((self.inner.borrow().deps.now)()),
                }),
            )?;
            reserve(&self.inner)?;
            Ok(self.status())
        })
    }

    /// dispatchPrepared()
    pub fn dispatch_prepared(&self) -> Result<V, String> {
        self.run_guard(|| {
            basic(&self.inner)?;
            let state = state_of(&self.inner);
            let o = active(&state);
            need(
                get(&o, "phase").as_str() == Some("prepared")
                    && get(&o, "fetchStarts").as_array().is_some_and(|a| a.is_empty()),
                "UNKNOWN_NEVER_RETRY",
            )?;
            ensure_installed(&self.inner)?;
            let intent_id = get(&o, "intentId").clone();
            let body = get(&o, "body").clone();
            let authority = json!({
                "permit": {
                    "intentId": get(&o, "intentId"),
                    "bodyHash": get(&o, "bodyHash"),
                },
                "lease": basic(&self.inner)?,
            });
            let result = self.transport.place(&body, &authority);
            if let Err(e) = result {
                if !self.inner.borrow().failed {
                    let _ = append(
                        &self.inner,
                        &json!({
                            "kind": "error",
                            "mayHaveExecuted": e.may_have_executed,
                            "at": iso((self.inner.borrow().deps.now)()),
                        }),
                    );
                    if pilot_unknown(&state_of(&self.inner)) {
                        let b = self.inner.borrow();
                        let _ = (b.deps.on_financial_halt)(&json!({
                            "kind": "uncertain_hedge",
                            "intentId": intent_id,
                            "allWritersStop": true,
                        }));
                    }
                }
                return Err(e.code);
            }
            Ok(self.status())
        })
    }

    /// reconcile(e)
    pub fn reconcile(&self, e: &V) -> Result<V, String> {
        self.run_guard(|| {
            basic(&self.inner)?;
            let mut evidence = e.as_object().cloned().unwrap_or_default();
            evidence.insert(
                "globalRisk".into(),
                {
                    let b = self.inner.borrow();
                    sync((b.deps.get_global_risk)())?
                },
            );
            let event = json!({
                "kind": "reconciled",
                "evidence": V::Object(evidence),
                "at": iso((self.inner.borrow().deps.now)()),
            });
            if let Err(error) = append(&self.inner, &event) {
                if !self.inner.borrow().failed
                    && (error == "HEDGE_COORDINATOR_NON_NEUTRAL_graphReceipt"
                        || error == "HEDGE_COORDINATOR_NON_NEUTRAL_collateralReceipt")
                {
                    append(
                        &self.inner,
                        &json!({
                            "kind": "financial_mismatch",
                            "reason": "NON_NEUTRAL_POST_ACCOUNT",
                            "at": iso((self.inner.borrow().deps.now)()),
                        }),
                    )?;
                    let b = self.inner.borrow();
                    sync((b.deps.on_financial_halt)(&json!({
                        "kind": "non_neutral_post_account",
                        "allWritersStop": true,
                    })))?;
                }
                return Err(error);
            }
            reserve(&self.inner)?;
            Ok(self.status())
        })
    }

    /// exportHandbackEvidence()
    pub fn export_handback_evidence(&self) -> Result<V, String> {
        basic(&self.inner)?;
        let state = state_of(&self.inner);
        need(
            active(&state).is_null() && !pilot_unknown(&state),
            "HANDOFF_NOT_TERMINAL",
        )?;
        let mut body = Map::new();
        body.insert("schemaVersion".into(), json!(1));
        body.insert(
            "kind".into(),
            json!("neutral_owned_hedge_explicit_handback_evidence_v2"),
        );
        for (k, v) in get(&state, "scope").as_object().cloned().unwrap_or_default() {
            body.insert(k, v);
        }
        body.insert("seedHash".into(), json!(self.inner.borrow().seed_hash));
        body.insert(
            "migration".into(),
            self.inner.borrow().migration.clone(),
        );
        body.insert(
            "legacyAnchor".into(),
            get(&self.inner.borrow().seed, "anchor").clone(),
        );
        body.insert(
            "coordinatorAnchor".into(),
            get(&self.journal_source()?, "headPrefix").clone(),
        );
        body.insert(
            "originalInventory".into(),
            get(&state, "originalInventory").clone(),
        );
        body.insert(
            "currentInventory".into(),
            get(&state, "currentInventory").clone(),
        );
        body.insert(
            "pairRetirements".into(),
            get(&state, "pairRetirements").clone(),
        );
        body.insert(
            "counterInventory".into(),
            get(&state, "counterInventory").clone(),
        );
        body.insert("assignments".into(), get(&state, "assignments").clone());
        body.insert("funding".into(), funding(&state)?);
        body.insert(
            "requiresExplicitNewOwnerImplementation".into(),
            json!(true),
        );
        let body = V::Object(body);
        let mut out = body.as_object().cloned().unwrap();
        out.insert("proofHash".into(), json!(payload_hash(&body)?));
        Ok(V::Object(out))
    }

    /// close()
    pub fn close(&self) -> Result<(), String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        let mut b = self.inner.borrow_mut();
        if b.closed {
            return Ok(());
        }
        b.closed = true;
        b.lease.release().map_err(|e| e.message)?;
        Ok(())
    }
}

/// guardedPermit(p, s, network) — wraps validate; non-fatal guard failures
/// become OWNED_CUSTODY_DISPATCH_GUARD transport errors.
fn guarded_permit(
    inner: &Rc<RefCell<PilotInner>>,
    p: &V,
    s: &V,
    network: bool,
) -> Result<bool, OrderTransportError> {
    match validate(inner, p, s, network) {
        Ok(v) => Ok(v),
        Err(error) => {
            let at = (inner.borrow().deps.now)();
            inner.borrow_mut().last_guard_failure = Some(json!({
                "stage": if network { "network" } else { "admission" },
                "errorCode": diagnostic_code(&error),
                "failedAt": iso(at),
            }));
            Err(OrderTransportError {
                code: "OWNED_CUSTODY_DISPATCH_GUARD".into(),
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
