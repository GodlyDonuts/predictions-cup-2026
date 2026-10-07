// execution/baal-neutral-owned-counterpart-pilot-v2.mjs — versioned one-share
// neutral-only owned counterpart pilot. Never enables a prior window.
//
// Port notes:
// - `need` throws 'HEDGE_COORDINATOR_'+code (this module's own prefix).
// - The pilot seeds from a verified legacy scaled-controller journal replay
//   (crate::baal_scaled_controller_legacy_replay_v1).
// - sync(fn) -> direct Result propagation (undefined cannot occur).
// - `issued` WeakMap -> pointer-keyed HashMap, same as
//   baal_scaled_no_controller_v1.rs.
// - JS `f.fatal=true;f.durableRecordingFailed=true` on journal-append failures
//   is proxied by `inner.failed` (the only code paths that set those flags also
//   set failed).
// - fetchDeadline: f64 Option (None == JS null); Number.isSafeInteger check on
//   it is an integral-finite check.
use crate::core::{canonical_payload, payload_hash};
use crate::frozen::collect_frozen_paper_files;
use crate::receipts::parse_number_lexemes;
use crate::transport::{Guard, OrderTransport, OrderTransportError, OrderTransportOptions};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::{amount, instant, money, price};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use baal_service_runtime::ledger::OwnerLease;
use serde_json::{json, Map, Value as V};
use std::cell::RefCell;
use std::collections::HashMap;
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
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
fn hex(x: &V) -> bool {
    x.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
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
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().ok_or_else(|| "INVALID_INSTANT".to_string())?)
}
fn num_str(v: &V) -> String {
    js_stringify(v)
}
// JS Number(x) on numbers or numeric strings (money strings like "0.995000").
fn js_number(v: &V) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
}
fn first_non_null<'a>(v: &'a V, keys: &[&str]) -> &'a V {
    for k in keys {
        if let Some(x) = v.get(k) {
            if !x.is_null() {
                return x;
            }
        }
    }
    &V::Null
}

/// liveCoordinatorIdentity(workspace)
pub fn live_coordinator_identity(workspace: &Path) -> Result<V, String> {
    collect_frozen_paper_files(workspace, &["execution/baal-neutral-owned-counterpart-pilot-v2.mjs"])
}

fn active(s: &V) -> Option<V> {
    get(s, "orders").as_array().and_then(|orders| {
        orders
            .iter()
            .find(|o| get(o, "proof").is_null() && get(o, "phase").as_str() != Some("abandoned"))
            .cloned()
    })
}
fn active_pair_quantity(s: &V, a: &V) -> i64 {
    let retired: i64 = get(s, "pairRetirements")
        .as_array()
        .map(|rs| {
            rs.iter()
                .filter(|r| get(r, "assignmentId") == get(a, "assignmentId"))
                .map(|r| js_int(get(r, "quantity")).unwrap_or(0))
                .sum()
        })
        .unwrap_or(0);
    js_int(get(a, "quantity")).unwrap_or(0) - retired
}
fn matched(s: &V) -> i64 {
    get(s, "assignments")
        .as_array()
        .map(|xs| xs.iter().map(|a| active_pair_quantity(s, a)).sum())
        .unwrap_or(0)
}
fn scope_account(account: &V, ex: &str) -> V {
    let scoped = get(account, "scopedOrdersPagesByExchange")
        .get(ex)
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or_else(|| get(account, "scopedOrdersPages").clone());
    let mut out = Map::new();
    out.insert("cash".into(), get(account, "cash").clone());
    out.insert("positions".into(), get(account, "positions").clone());
    out.insert("openOrdersPages".into(), get(account, "openOrdersPages").clone());
    out.insert("scopedOrdersPages".into(), scoped);
    V::Object(out)
}
fn pair_census(s: &V, first_ex: &str, at: &str) -> V {
    let first = get(get(s, "currentInventory"), first_ex);
    let other_ex = get(get(first, "package"), "legs")
        .as_array()
        .and_then(|ls| {
            ls.iter()
                .find(|l| sget(l, "exchangeId") != Some(first_ex))
                .and_then(|l| l.get("exchangeId"))
                .and_then(V::as_str)
        })
        .unwrap_or("");
    let counterpart = get(get(s, "currentInventory"), other_ex);
    let mut out = V::Object(s.get("scope").and_then(|x| x.as_object()).cloned().unwrap_or_default());
    if let Some(m) = out.as_object_mut() {
        m.insert("complete".into(), json!(true));
        m.insert(
            "firstProofHash".into(),
            get(get(first, "proof"), "verificationHash").clone(),
        );
        m.insert(
            "sourceHash".into(),
            json!(payload_hash(&json!({
                "seed": get(s, "seedHash"),
                "assignments": get(s, "assignments"),
                "counterpart": get(get(counterpart, "proof"), "verificationHash"),
            }))
            .unwrap_or_default()),
        );
        m.insert("at".into(), json!(at));
        m.insert(
            "matchedLots".into(),
            V::Array(
                get(s, "assignments")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|a| {
                        sget(a, "firstExchangeId") == Some(first_ex) && active_pair_quantity(s, a) > 0
                    })
                    .map(|a| {
                        json!({
                            "assignmentId": get(&a, "assignmentId"),
                            "firstLotId": get(&a, "firstLotId"),
                            "counterpartLotId": get(&a, "counterpartLotId"),
                            "quantity": active_pair_quantity(s, &a),
                        })
                    })
                    .collect(),
            ),
        );
        m.insert(
            "counterpartOwned".into(),
            if js_int(get(get(counterpart, "proof"), "remainingOwnedQuantity")).unwrap_or(0) > 0 {
                counterpart.clone()
            } else {
                V::Null
            },
        );
    }
    out
}

/// projectRisk(state) — also `funding`.
pub fn project_risk(s: &V) -> Result<V, String> {
    let mut rows: Vec<V> = Vec::new();
    let mut owned: Vec<V> = Vec::new();
    if let Some(inv) = get(s, "currentInventory").as_object() {
        for (ex, slot) in inv {
            if js_int(get(get(slot, "proof"), "remainingOwnedQuantity")).unwrap_or(0) == 0 {
                continue;
            }
            let mut allocated: HashMap<String, i64> = HashMap::new();
            for a in get(s, "assignments").as_array().cloned().unwrap_or_default() {
                let n = active_pair_quantity(s, &a);
                need(n >= 0, "PAIR_OVERRETIRE")?;
                let lot = if sget(&a, "firstExchangeId") == Some(ex.as_str()) {
                    sget(&a, "firstLotId")
                } else if sget(&a, "counterpartExchangeId") == Some(ex.as_str()) {
                    sget(&a, "counterpartLotId")
                } else {
                    None
                };
                if let Some(lot) = lot {
                    *allocated.entry(lot.to_string()).or_insert(0) += n;
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
                        .get(sget(&l, "lotId").unwrap_or(""))
                        .copied()
                        .unwrap_or(0);
                need(n >= 0, "PAIR_EXCEEDS_CURRENT_LOT")?;
                q += n;
                cost += price(sget(&l, "entryPrice").unwrap_or(""))? * n as i128;
            }
            let other = get(get(slot, "package"), "legs")
                .as_array()
                .and_then(|ls| {
                    ls.iter()
                        .find(|l| sget(l, "exchangeId") != Some(ex.as_str()))
                        .cloned()
                })
                .unwrap_or(V::Null);
            owned.push(json!({
                "exposureId": format!("scaled:{}:{}:owned", sget(get(s,"scope"),"runId").unwrap_or(""), ex),
                "marketId": get(get(slot,"proof"),"marketId"),
                "eventId": get(get(slot,"package"),"eventId"),
                "nominalCost": get(get(slot,"proof"),"remainingOwnedCost"),
            }));
            if q != 0 {
                rows.push(json!({
                    "exposureId": format!("scaled:{}:{}:hedge-for-{}", sget(get(s,"scope"),"runId").unwrap_or(""), sget(&other,"exchangeId").unwrap_or(""), ex),
                    "marketId": get(&other,"marketId"),
                    "eventId": get(get(slot,"package"),"eventId"),
                    "maximumNominalCost": money(990000i128 * q as i128 - cost),
                }));
            }
        }
    }
    let o = active(s);
    let cash_reserved: i128 = rows
        .iter()
        .map(|r| amount(sget(r, "maximumNominalCost").unwrap_or("")).unwrap_or(0))
        .sum();
    let owned_principal: i128 = owned
        .iter()
        .map(|r| amount(sget(r, "nominalCost").unwrap_or("")).unwrap_or(0))
        .sum();
    let mut body = Map::new();
    body.insert("schemaVersion".into(), json!(1));
    body.insert("kind".into(), json!("baal_scaled_no_full_nominal_reservation_v1"));
    body.insert("ownerId".into(), get(get(s, "scope"), "ownerId").clone());
    body.insert("runId".into(), get(get(s, "scope"), "runId").clone());
    body.insert("tournamentId".into(), get(get(s, "scope"), "tournamentId").clone());
    body.insert("allocationHash".into(), json!(payload_hash(get(s, "allocation"))?));
    body.insert("seedHash".into(), get(s, "seedHash").clone());
    body.insert(
        "implementationEpoch".into(),
        get(get(s, "migration"), "implementationEpoch").clone(),
    );
    body.insert("owned".into(), V::Array(owned.clone()));
    body.insert("reservations".into(), V::Array(rows));
    body.insert(
        "unresolvedIntentIds".into(),
        o.map(|o| json!([get(&o, "intentId")]))
            .unwrap_or_else(|| json!([])),
    );
    body.insert("cashReserved".into(), json!(money(cash_reserved)));
    body.insert("ownedPrincipal".into(), json!(money(owned_principal)));
    let mut out = V::Object(body);
    let h = payload_hash(&out)?;
    out.as_object_mut().unwrap().insert("reservationHash".into(), json!(h));
    Ok(out)
}
fn funding(s: &V) -> Result<V, String> {
    project_risk(s)
}
/// pilotUnknown
pub fn pilot_unknown(s: &V) -> bool {
    match active(s) {
        Some(o) => {
            sget(&o, "phase") == Some("uncertain")
                || sget(&o, "phase") == Some("semantic_mismatch")
                || (get(&o, "orderId").is_null()
                    && get(&o, "fetchStarts")
                        .as_array()
                        .map(|x| !x.is_empty())
                        .unwrap_or(false))
        }
        None => false,
    }
}
fn empty_all(e: &V, s: &V, at: i64) -> Result<(), String> {
    let first_ex = sget(e, "firstExchangeId").unwrap_or("");
    let market_id = sget(
        get(get(get(s, "currentInventory"), first_ex), "proof"),
        "marketId",
    )
    .unwrap_or("");
    for (key, endpoint) in [
        (
            "graphReceipt",
            format!(
                "/relationships/graph?marketId={}&depth=4&tournamentId={}",
                market_id,
                sget(get(s, "scope"), "tournamentId").unwrap_or("")
            ),
        ),
        (
            "collateralReceipt",
            format!(
                "/portfolio/collateral?tournamentId={}",
                sget(get(s, "scope"), "tournamentId").unwrap_or("")
            ),
        ),
    ] {
        let r = get(e, key);
        need(
            sget(r, "method") == Some("GET")
                && js_int(get(r, "status")) == Some(200)
                && sget(r, "endpoint") == Some(endpoint.as_str())
                && get(r, "bodyText").as_str().is_some()
                && sget(r, "sourceBodyHash") == Some(sha(sget(r, "bodyText").unwrap_or("")).as_str())
                && instant_v(get(r, "requestStartedAt"))? <= instant_v(get(r, "receivedAt"))?
                && instant_v(get(r, "receivedAt"))? <= at
                && at - instant_v(get(r, "requestStartedAt"))? <= 5000,
            &format!("FRESH_NONE_{key}"),
        )?;
        let d = parse_number_lexemes(sget(r, "bodyText").unwrap_or(""))?.0;
        need(
            if key == "graphReceipt" {
                get(&d, "nodes").as_array().map(|x| x.is_empty()).unwrap_or(false)
                    && get(&d, "relationships")
                        .as_array()
                        .map(|x| x.is_empty())
                        .unwrap_or(false)
            } else {
                get(&d, "data").as_array().map(|x| x.is_empty()).unwrap_or(false)
            },
            &format!("NON_NEUTRAL_{key}"),
        )?;
    }
    let book = first_non_null(e, &["otherBookReceipt", "bookReceipt", "book"]);
    let after = get(e, "after");
    let last_account: Option<i64> = if after.is_null() {
        None
    } else {
        let mut v = instant_v(get(get(after, "cash"), "receivedAt"))?.max(instant_v(get(
            get(after, "positions"),
            "receivedAt",
        ))?);
        for r in get(after, "openOrdersPages")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .chain(
                get(after, "scopedOrdersPages")
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            )
        {
            v = v.max(instant_v(get(&r, "receivedAt"))?);
        }
        Some(v)
    };
    let floor = if !book.is_null() {
        instant_v(get(book, "receivedAt"))?
    } else {
        last_account.unwrap_or(0)
    };
    need(
        instant_v(get(get(e, "graphReceipt"), "receivedAt"))?
            <= instant_v(get(get(e, "collateralReceipt"), "requestStartedAt"))?
            && instant_v(get(get(e, "graphReceipt"), "requestStartedAt"))? >= floor,
        "COLLATERAL_LAST",
    )?;
    Ok(())
}
fn guarded(s: &V, e: &V, g: &V, at: i64) -> Result<V, String> {
    need(
        at >= instant_v(get(get(s, "activation"), "startedAt"))?
            && at < instant_v(get(get(s, "activation"), "hedgeExpiresAt"))?,
        "HEDGE_WINDOW",
    )?;
    need(
        sget(g, "ownerId") == sget(get(s, "scope"), "ownerId")
            && sget(g, "runId") == sget(get(s, "scope"), "runId")
            && sget(g, "tournamentId") == sget(get(s, "scope"), "tournamentId")
            && get(g, "complete") == &V::Bool(true)
            && get(g, "eventScopeComplete") == &V::Bool(true)
            && get(g, "soleCollector") == &V::Bool(true)
            && get(g, "stopped") == &V::Bool(false)
            && hex(get(g, "sourceHash"))
            && get(g, "unknownAcceptance") == &V::Bool(false)
            && get(g, "unresolvedIntentIds").as_array().is_some_and(|ids| {
                ids.iter()
                    .all(|id| get(&active(s).unwrap_or(V::Null), "intentId") == id)
            })
            && instant_v(get(g, "at"))? <= at
            && at - instant_v(get(g, "at"))? <= 20000,
        "GLOBAL_UNKNOWN",
    )?;
    {
        let reservations = get(g, "reservations").as_array();
        let shape_ok = reservations
            .map(|rs| {
                rs.iter().all(|r| {
                    amount(sget(r, "maximumNominalCost").unwrap_or(""))
                        .map(|x| x >= 0)
                        .unwrap_or(false)
                        && get(r, "unknownAcceptance") != &V::Bool(true)
                }) && {
                    let mut seen = std::collections::HashSet::new();
                    rs.iter()
                        .all(|r| seen.insert(sget(r, "exposureId").unwrap_or("").to_string()))
                }
            })
            .unwrap_or(false);
        let fits = if get(g, "fundingSatisfied") != &V::Bool(false)
            && reservations.is_some()
            && shape_ok
        {
            let sum = reservations
                .unwrap_or(&Vec::new())
                .iter()
                .try_fold(0i128, |n, r| {
                    amount(sget(r, "maximumNominalCost").unwrap_or("")).map(|x| n + x)
                })?;
            sum <= amount(sget(g, "cashLower").unwrap_or(""))?
        } else {
            false
        };
        need(
            get(g, "fundingSatisfied") != &V::Bool(false)
                && reservations.is_some()
                && shape_ok
                && fits,
            "CURRENT_CASH_FUNDS_FULL_RESERVE",
        )?;
    }
    empty_all(e, s, at)?;
    need(
        !get(e, "pairCensus").is_null()
            && same(
                get(e, "pairCensus"),
                &pair_census(s, sget(e, "firstExchangeId").unwrap_or(""), sget(get(e, "pairCensus"), "at").unwrap_or("")),
            )?,
        "CAPTURED_PAIR_CENSUS",
    )?;
    need(
        !get(e, "globalRisk").is_null()
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
            .map(|x| x.is_empty())
            .unwrap_or(false),
        "SOURCE_RISK_PENDING",
    )?;
    let mut entry = e.clone();
    entry
        .as_object_mut()
        .map(|m| {
            m.insert("globalRisk".into(), source_risk.clone());
            m.insert("scope".into(), get(s, "scope").clone());
            m.insert(
                "owned".into(),
                get(get(s, "currentInventory"), sget(e, "firstExchangeId").unwrap_or("")).clone(),
            );
            m.insert("now".into(), json!(iso(at as f64)));
        });
    let p = crate::baal_scaled_owned_no_hedge_proposal_v1::plan_baal_owned_no_hedge(&entry);
    need(
        get(&p, "proposalReady") == &V::Bool(true),
        &format!(
            "PROPOSAL_{}",
            sget(&p, "reason").unwrap_or("")
        ),
    )?;
    let first = get(&p, "matchedAssignmentsIfFilled")
        .as_array()
        .and_then(|x| x.first())
        .cloned()
        .unwrap_or(V::Null);
    need(
        !first.is_null() && price(sget(&p, "protectedLimit").unwrap_or(""))? <= 995000,
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
            price(sget(&first, "entryPrice").unwrap_or(""))?
                + price(sget(&p, "protectedLimit").unwrap_or(""))?,
        )),
    );
    q.insert("conditionalOrdinaryMinimumPayout".into(), json!("1.000000"));
    q.insert("conditionalThirdPartyPayout".into(), json!("2.000000"));
    q.insert(
        "conditionalOrdinarySurplus".into(),
        json!(money(
            1000000i128
                - price(sget(&first, "entryPrice").unwrap_or(""))?
                - price(sget(&p, "protectedLimit").unwrap_or(""))?,
        )),
    );
    q.insert(
        "administrativeZeroPayoutLoss".into(),
        json!(money(
            -price(sget(&first, "entryPrice").unwrap_or(""))?
                - price(sget(&p, "protectedLimit").unwrap_or(""))?,
        )),
    );
    let mut first_one = first.as_object().cloned().unwrap_or_default();
    first_one.insert("quantity".into(), json!(1));
    first_one.insert("firstPrincipal".into(), get(&first, "entryPrice").clone());
    q.insert("matchedAssignmentsIfFilled".into(), json!([V::Object(first_one)]));
    q.shift_remove("proposalHash");
    let qv = V::Object(q);
    let h = payload_hash(&qv)?;
    let mut out = qv.as_object().cloned().unwrap_or_default();
    out.insert("proposalHash".into(), json!(h));
    Ok(V::Object(out))
}

fn sell_proposal(s: &V, ex: &str, e: &V, g: &V, at: i64) -> Result<V, String> {
    need(
        at >= instant_v(get(get(s, "activation"), "startedAt"))?
            && at < instant_v(get(get(s, "activation"), "exitExpiresAt"))?,
        "OWNED_EXIT_WINDOW",
    )?;
    need(
        sget(g, "ownerId") == sget(get(s, "scope"), "ownerId")
            && sget(g, "runId") == sget(get(s, "scope"), "runId")
            && sget(g, "tournamentId") == sget(get(s, "scope"), "tournamentId")
            && get(g, "complete") == &V::Bool(true)
            && get(g, "eventScopeComplete") == &V::Bool(true)
            && get(g, "soleCollector") == &V::Bool(true)
            && get(g, "stopped") == &V::Bool(false)
            && hex(get(g, "sourceHash"))
            && get(g, "unknownAcceptance") == &V::Bool(false)
            && get(g, "unresolvedIntentIds").as_array().is_some_and(|ids| {
                ids.iter()
                    .all(|id| get(&active(s).unwrap_or(V::Null), "intentId") == id)
            })
            && instant_v(get(g, "at"))? <= at
            && at - instant_v(get(g, "at"))? <= 20000,
        "GLOBAL_UNKNOWN",
    )?;
    need(
        get(g, "controllerReservationHash") == get(&funding(s)?, "reservationHash")
            && get(g, "globalLedgerHash") == get(get(s, "reservation"), "globalLedgerHash"),
        "CURRENT_RESERVE_BINDING",
    )?;
    {
        let mut e2 = e.clone();
        e2.as_object_mut()
            .map(|m| m.insert("firstExchangeId".into(), json!(ex)));
        empty_all(&e2, s, at)?;
    }
    let slot = get(get(s, "currentInventory"), ex);
    need(
        js_int(get(get(slot, "proof"), "remainingOwnedQuantity")).unwrap_or(0) > 0,
        "OWNED_POSITIVE",
    )?;
    let book_receipt = first_non_null(e, &["bookReceipt", "book"]).clone();
    let requested = e.get("requestedQuantity").cloned().unwrap_or(V::Null);
    let p = crate::baal_scaled_profitable_exit_signal_v2::plan_baal_scaled_profitable_exit_v2(
        &json!({
            "scope": get(s, "scope"),
            "owned": slot,
            "account": scope_account(get(e, "account"), ex),
            "bookReceipt": book_receipt,
            "globalRisk": get(e, "globalRisk"),
            "now": iso(at as f64),
            "protectiveTicks": 0,
            "maximumSharesPerOrder": 100,
            "maximumOrderCost": "25.000000",
            "feesPerShare": "0.000000",
            "requestedQuantity": requested,
        }),
    );
    need(
        get(&p, "proposalReady") == &V::Bool(true),
        &format!("PROFIT_EXIT_{}", sget(&p, "reason").unwrap_or("")),
    )?;
    Ok(p)
}
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
            let first = sget(&a, "firstExchangeId") == sget(get(o, "body"), "exchangeId")
                && get(&a, "firstLotId") == get(&old, "lotId");
            let counter = sget(&a, "counterpartExchangeId") == sget(get(o, "body"), "exchangeId")
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
                offset.is_some() && js_int(get(&a, "quantity")) == Some(1),
                "ONE_SHARE_PAIR_INTERVAL",
            )?;
            let offset = offset.unwrap_or(0);
            let n = (end.min(offset + js_int(get(&a, "quantity")).unwrap_or(0))
                - begin.max(offset))
            .max(0);
            if n != 0 {
                out.push(json!({
                    "retirementId": format!("{}:{}", sget(o,"intentId").unwrap_or(""), sget(&a,"assignmentId").unwrap_or("")),
                    "assignmentId": get(&a, "assignmentId"),
                    "quantity": n,
                    "consumedExchangeId": get(get(o,"body"),"exchangeId"),
                    "consumedLotId": get(&old,"lotId"),
                    "consumedUnitStart": begin.max(offset),
                    "sellProofHash": get(proof,"verificationHash"),
                    "reason": "actual_fifo_owned_sell",
                }));
            }
        }
    }
    need(
        {
            let mut seen = std::collections::HashSet::new();
            out.iter()
                .all(|r| seen.insert(sget(r, "assignmentId").unwrap_or("").to_string()))
        },
        "PAIR_DOUBLE_RETIREMENT",
    )?;
    Ok(out)
}
fn assign_fill(s: &V, o: &V, p: &V) -> Result<Vec<V>, String> {
    let mut result: Vec<V> = Vec::new();
    let mut target: usize = 0;
    let proposals = get(get(o, "proposal"), "matchedAssignmentsIfFilled")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut left = proposals
        .first()
        .and_then(|f| js_int(get(f, "quantity")))
        .unwrap_or(0);
    for lot in get(p, "remainingLots").as_array().cloned().unwrap_or_default() {
        let mut remaining = js_int(get(&lot, "quantity")).unwrap_or(0);
        if get(get(o, "priorProof"), "remainingLots")
            .as_array()
            .map(|olds| olds.iter().any(|old| get(old, "lotId") == get(&lot, "lotId")))
            .unwrap_or(false)
        {
            continue;
        }
        while remaining > 0 {
            let first = proposals.get(target).cloned().unwrap_or(V::Null);
            need(!first.is_null() && left > 0, "MATCHED_FILL_EXCEEDS_PROPOSAL")?;
            let n = left.min(remaining);
            need(
                price(sget(&first, "entryPrice").unwrap_or(""))?
                    + price(sget(&lot, "entryPrice").unwrap_or(""))?
                    <= 990000,
                "ACTUAL_PAIR_MARGIN",
            )?;
            let first_lot = || {
                get(get(get(s, "currentInventory"), sget(o, "firstExchangeId").unwrap_or("")), "proof")
                    .get("remainingLots")
                    .and_then(|ls| ls.as_array())
                    .and_then(|ls| {
                        ls.iter()
                            .find(|l| get(l, "lotId") == get(&first, "firstLotId"))
                            .cloned()
                    })
                    .unwrap_or(V::Null)
            };
            let fl = first_lot();
            result.push(json!({
                "assignmentId": format!("{}:{}:{}", sget(o,"intentId").unwrap_or(""), sget(&lot,"lotId").unwrap_or(""), target),
                "firstExchangeId": get(o,"firstExchangeId"),
                "counterpartExchangeId": get(get(o,"body"),"exchangeId"),
                "firstLotId": get(&first,"firstLotId"),
                "counterpartLotId": get(&lot,"lotId"),
                "quantity": n,
                "firstUnitCost": get(&first,"entryPrice"),
                "counterpartUnitCost": get(&lot,"entryPrice"),
                "firstProofHash": get(get(o,"proposal"),"firstProofHash"),
                "counterpartProofHash": get(p,"verificationHash"),
                "firstUnitStart": js_int(get(&fl,"originalQuantity")).unwrap_or(0) - js_int(get(&fl,"quantity")).unwrap_or(0),
                "counterpartUnitStart": js_int(get(&lot,"originalQuantity")).unwrap_or(0) - js_int(get(&lot,"quantity")).unwrap_or(0),
            }));
            remaining -= n;
            left -= n;
            if left == 0 {
                target += 1;
                left = proposals
                    .get(target)
                    .and_then(|f| js_int(get(f, "quantity")))
                    .unwrap_or(0);
            }
        }
    }
    need(
        result
            .iter()
            .map(|a| js_int(get(a, "quantity")).unwrap_or(0))
            .sum::<i64>()
            == js_int(get(p, "filledQuantity")).unwrap_or(-1),
        "EXACT_PARTIAL_PAIR_COUNT",
    )?;
    Ok(result)
}
fn reduce(s: Option<V>, e: &V) -> Result<V, String> {
    let kind = sget(e, "kind").unwrap_or("");
    if kind == "seed" {
        need(
            s.is_none()
                && get(get(e, "seed"), "economicReplayPerformed") == &V::Bool(true)
                && !get(get(get(e, "seed"), "state"), "orders")
                    .as_array()
                    .map(|orders| {
                        orders.iter().any(|o| {
                            get(o, "proof").is_null()
                                && get(o, "phase").as_str() != Some("abandoned")
                        })
                    })
                    .unwrap_or(false),
            "VERIFIED_SEED",
        )?;
        let seed_state = get(get(e, "seed"), "state");
        return Ok(json!({
            "schemaVersion": 1,
            "mode": "baal_neutral_owned_counterpart_pilot_v2",
            "scope": get(seed_state, "scope"),
            "allocation": get(seed_state, "allocation"),
            "originalActivation": get(seed_state, "activation"),
            "originalOrders": get(seed_state, "orders"),
            "originalInventory": get(seed_state, "inventory"),
            "currentInventory": get(seed_state, "inventory"),
            "pairRetirements": [],
            "seedHash": get(e, "seedHash"),
            "seedAnchor": get(get(e, "seed"), "anchor"),
            "activation": get(e, "activation"),
            "migration": get(e, "migration"),
            "orders": [],
            "assignments": [],
            "counterInventory": {},
            "reservation": V::Null,
        }));
    }
    let s = s.ok_or_else(|| "HEDGE_COORDINATOR_NOT_SEEDED".to_string())?;
    let mut n = s.clone();
    let oi = get(&n, "orders")
        .as_array()
        .and_then(|orders| {
            orders
                .iter()
                .position(|o| get(o, "proof").is_null() && get(o, "phase").as_str() != Some("abandoned"))
        });
    let o = oi.map(|i| get(&n, "orders").as_array().unwrap()[i].clone());
    if kind == "reservation" {
        need(
            get(get(e, "receipt"), "reservationHash") == get(&funding(&s)?, "reservationHash")
                && get(get(e, "receipt"), "seedHash") == get(&s, "seedHash")
                && hex(get(get(e, "receipt"), "globalLedgerHash")),
            "RESERVATION",
        )?;
        n.as_object_mut()
            .unwrap()
            .insert("reservation".into(), get(e, "receipt").clone());
        return Ok(n);
    }
    if kind == "prepared_sell" {
        need(o.is_none(), "UNKNOWN_OR_ACTIVE")?;
        let ex = sget(e, "exchangeId").unwrap_or("");
        let p = sell_proposal(
            &s,
            ex,
            get(e, "evidence"),
            get(e, "globalRisk"),
            instant_v(get(e, "at"))?,
        )?;
        let slot = get(get(&s, "currentInventory"), ex);
        let expires = iso(
            (instant_v(get(e, "at"))? + 30000)
                .min(instant_v(get(&p, "expiresAt"))?)
                .min(instant_v(get(get(&s, "activation"), "exitExpiresAt"))?) as f64,
        );
        let body = json!({
            "idempotencyKey": get(e, "key"),
            "exchangeId": ex,
            "side": "no",
            "action": "sell",
            "quantity": get(&p, "quantity"),
            "price": js_number(get(&p, "limit")),
            "expirationDate": expires,
            "tournamentId": sget(get(&s,"scope"),"tournamentId"),
        });
        need(
            !get(&s, "originalOrders")
                .as_array()
                .map(|os| os.iter().any(|o| get(get(o, "body"), "idempotencyKey") == get(e, "key")))
                .unwrap_or(false)
                && !get(&s, "orders")
                    .as_array()
                    .map(|os| {
                        os.iter().any(|o| {
                            get(get(o, "body"), "idempotencyKey") == get(e, "key")
                                || get(o, "intentId") == get(e, "intentId")
                        })
                    })
                    .unwrap_or(false),
            "KEY_REUSE",
        )?;
        let candidate = json!({
            "intentId": get(e, "intentId"),
            "firstExchangeId": ex,
            "marketId": get(&p, "marketId"),
            "createdAt": get(e, "at"),
            "body": body,
            "bodyJson": canonical_payload(&body)?,
            "bodyHash": payload_hash(&body)?,
            "proposal": p,
            "before": scope_account(get(get(e,"evidence"),"account"), ex),
            "priorProof": get(slot, "proof"),
            "priorAfter": get(slot, "after"),
            "package": get(slot, "package"),
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
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(candidate);
        return Ok(n);
    }
    if kind == "prepared" {
        need(
            o.is_none()
                && get(&s, "orders")
                    .as_array()
                    .map(|os| {
                        os.iter()
                            .filter(|o| sget(get(o, "body"), "action") == Some("buy"))
                            .count()
                            == 0
                    })
                    .unwrap_or(false)
                && get(get(&s, "activation"), "allowedFirstExchangeIds")
                    .as_array()
                    .map(|xs| {
                        xs.iter()
                            .any(|x| x == get(get(e, "evidence"), "firstExchangeId"))
                    })
                    .unwrap_or(false)
                && js_int(get(
                    get(get(get(&s, "originalInventory"), sget(get(e, "evidence"), "firstExchangeId").unwrap_or("")), "proof"),
                    "remainingOwnedQuantity",
                ))
                .unwrap_or(0)
                    > 0,
            "ORIGINAL_UNPAIRED_ONLY",
        )?;
        let p = guarded(
            &s,
            get(e, "evidence"),
            get(e, "globalRisk"),
            instant_v(get(e, "at"))?,
        )?;
        let expires = iso(
            (instant_v(get(e, "at"))? + 30000)
                .min(instant_v(get(&p, "expiresAt"))?)
                .min(instant_v(get(get(&s, "activation"), "hedgeExpiresAt"))?) as f64,
        );
        let body = json!({
            "idempotencyKey": get(e, "key"),
            "exchangeId": get(&p, "exchangeId"),
            "side": "no",
            "action": "buy",
            "quantity": get(&p, "quantity"),
            "price": js_number(get(&p, "protectedLimit")),
            "expirationDate": expires,
            "tournamentId": sget(get(&s,"scope"),"tournamentId"),
        });
        need(
            !get(&s, "orders")
                .as_array()
                .map(|os| {
                    os.iter().any(|o| {
                        get(get(o, "body"), "idempotencyKey") == get(e, "key")
                            || get(o, "intentId") == get(e, "intentId")
                    })
                })
                .unwrap_or(false),
            "KEY_REUSE",
        )?;
        let bought_ex = sget(&p, "exchangeId").unwrap_or("");
        let first_ex = sget(get(e, "evidence"), "firstExchangeId").unwrap_or("");
        let candidate = json!({
            "intentId": get(e, "intentId"),
            "firstExchangeId": first_ex,
            "marketId": get(&p, "marketId"),
            "createdAt": get(e, "at"),
            "body": body,
            "bodyJson": canonical_payload(&body)?,
            "bodyHash": payload_hash(&body)?,
            "proposal": p,
            "before": scope_account(get(get(e,"evidence"),"account"), bought_ex),
            "priorProof": get(get(get(&s,"currentInventory"), bought_ex), "proof"),
            "priorAfter": get(get(get(&s,"currentInventory"), bought_ex), "after"),
            "package": get(get(get(&s,"originalInventory"), first_ex), "package"),
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
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(candidate);
        return Ok(n);
    }
    if kind == "abandoned_prepared" {
        let o = o.ok_or_else(|| "HEDGE_COORDINATOR_ABANDON_REQUIRES_EXPIRED_UNADMITTED_UNFETCHED".to_string())?;
        need(
            sget(&o, "phase") == Some("prepared")
                && get(&o, "orderId").is_null()
                && get(&o, "ack").is_null()
                && get(&o, "proof").is_null()
                && get(&o, "semanticMismatch") == &V::Bool(false)
                && get(&o, "mayHaveExecuted") == &V::Bool(false)
                && get(&o, "fences").as_array().map(|x| x.is_empty()).unwrap_or(false)
                && get(&o, "fetchStarts").as_array().map(|x| x.is_empty()).unwrap_or(false)
                && get(&o, "receipts").as_array().map(|x| x.is_empty()).unwrap_or(false)
                && instant_v(get(e, "at"))? >= instant_v(get(get(&o, "body"), "expirationDate"))?,
            "ABANDON_REQUIRES_EXPIRED_UNADMITTED_UNFETCHED",
        )?;
        let m = n
            .as_object_mut()
            .unwrap()
            .get_mut("orders")
            .unwrap()
            .as_array_mut()
            .unwrap()[oi.unwrap()]
            .as_object_mut()
            .unwrap();
        m.insert("phase".into(), json!("abandoned"));
        m.insert("abandonedAt".into(), get(e, "at").clone());
        m.insert("abandonedReason".into(), json!("expired_before_any_admission"));
        return Ok(n);
    }
    let o = o.ok_or_else(|| "HEDGE_COORDINATOR_NO_PENDING".to_string())?;
    let oi = oi.unwrap();
    need(get(&o, "proof").is_null(), "NO_PENDING")?;
    if kind == "admitted" || kind == "network_fence" {
        need(
            if kind == "admitted" {
                sget(&o, "phase") == Some("prepared")
            } else {
                sget(&o, "phase") == Some("dispatching")
            },
            "SINGLE_ATTEMPT",
        )?;
        let p = if sget(get(&o, "body"), "action") == Some("buy") {
            guarded(&s, get(e, "evidence"), get(e, "globalRisk"), instant_v(get(e, "at"))?)?
        } else {
            sell_proposal(
                &s,
                sget(get(&o, "body"), "exchangeId").unwrap_or(""),
                get(e, "evidence"),
                get(e, "globalRisk"),
                instant_v(get(e, "at"))?,
            )?
        };
        need(
            get(&p, "exchangeId") == get(get(&o, "body"), "exchangeId")
                && js_int(get(&p, "quantity")).unwrap_or(-1)
                    >= js_int(get(get(&o, "body"), "quantity")).unwrap_or(0)
                && if sget(get(&o, "body"), "action") == Some("buy") {
                    get(&p, "firstProofHash") == get(get(&o, "proposal"), "firstProofHash")
                        && price(sget(&p, "protectedLimit").unwrap_or(""))?
                            <= price(&num_str(get(get(&o, "body"), "price")))?
                } else {
                    get(&p, "ownedProofHash") == get(get(&o, "priorProof"), "verificationHash")
                        && price(sget(&p, "limit").unwrap_or(""))?
                            >= price(&num_str(get(get(&o, "body"), "price")))?
                }
                && instant_v(get(e, "at"))? < instant_v(get(get(&o, "body"), "expirationDate"))?,
            "FIXED_LIMIT_SOURCE",
        )?;
        if kind == "admitted" {
            n.as_object_mut()
                .unwrap()
                .get_mut("orders")
                .unwrap()
                .as_array_mut()
                .unwrap()[oi]
                .as_object_mut()
                .unwrap()
                .insert("phase".into(), json!("dispatching"));
        } else {
            need(
                payload_hash(get(e, "lease"))? == get(e, "leaseHash").as_str().unwrap_or("\u{0}"),
                "ACTUAL_LEASE",
            )?;
            n.as_object_mut()
                .unwrap()
                .get_mut("orders")
                .unwrap()
                .as_array_mut()
                .unwrap()[oi]
                .as_object_mut()
                .unwrap()
                .get_mut("fences")
                .unwrap()
                .as_array_mut()
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
        let f = get(&o, "fences")
            .as_array()
            .and_then(|x| x.last())
            .cloned()
            .unwrap_or(V::Null);
        let w = get(e, "meta");
        need(
            !f.is_null()
                && sget(w, "method") == Some("POST")
                && sget(w, "endpoint") == Some("/orders")
                && sget(w, "requestBodyHash") == sget(&o, "bodyHash")
                && instant_v(get(w, "requestStartedAt"))? >= instant_v(get(&f, "at"))?
                && get(&o, "fetchStarts")
                    .as_array()
                    .map(|x| x.is_empty())
                    .unwrap_or(false),
            "ACTUAL_INVOCATION",
        )?;
        {
            let m = n
                .as_object_mut()
                .unwrap()
                .get_mut("orders")
                .unwrap()
                .as_array_mut()
                .unwrap()[oi]
                .as_object_mut()
                .unwrap();
            m.get_mut("fetchStarts")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(w.clone());
            m.insert("mayHaveExecuted".into(), json!(true));
        }
        return Ok(n);
    }
    if kind == "wire" {
        let w = get(&o, "fetchStarts")
            .as_array()
            .and_then(|x| x.first())
            .cloned()
            .unwrap_or(V::Null);
        let r = get(e, "receipt");
        need(
            !w.is_null()
                && sget(r, "method") == Some("POST")
                && sget(r, "endpoint") == Some("/orders")
                && sget(r, "sourceBodyHash") == Some(sha(sget(r, "bodyText").unwrap_or("")).as_str())
                && get(r, "fetchInvocationId") == get(&w, "fetchInvocationId")
                && get(r, "requestStartedAt") == get(&w, "requestStartedAt"),
            "ORIGINAL_WIRE",
        )?;
        let mut ack = r.clone();
        ack.as_object_mut()
            .map(|m| m.insert("requestBodyHash".into(), get(&o, "bodyHash").clone()));
        let parsed = parse_number_lexemes(sget(r, "bodyText").unwrap_or(""))?;
        let d = parsed.0;
        let order_id = if js_int(get(&d, "orderId")).map(|x| x > 0).unwrap_or(false) {
            json!(js_int(get(&d, "orderId")).unwrap().to_string())
        } else {
            get(&o, "orderId").clone()
        };
        let mismatch = !crate::baal_scaled_controller_legacy_replay_v1::validate_legacy_scaled_neutral_ack(&o, &ack);
        {
            let m = n
                .as_object_mut()
                .unwrap()
                .get_mut("orders")
                .unwrap()
                .as_array_mut()
                .unwrap()[oi]
                .as_object_mut()
                .unwrap();
            m.get_mut("receipts")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(r.clone());
            m.insert("orderId".into(), order_id);
            m.insert("ack".into(), ack);
            m.insert("semanticMismatch".into(), json!(mismatch));
            m.insert(
                "phase".into(),
                json!(if mismatch { "semantic_mismatch" } else { "acknowledged" }),
            );
        }
        return Ok(n);
    }
    if kind == "error" {
        let (ph, may) = if sget(&o, "phase") == Some("dispatching")
            || get(e, "mayHaveExecuted") != &V::Bool(false)
        {
            (json!("uncertain"), json!(true))
        } else {
            need(
                sget(&o, "phase") == Some("prepared")
                    && get(&o, "fences").as_array().map(|x| x.is_empty()).unwrap_or(false)
                    && get(&o, "fetchStarts").as_array().map(|x| x.is_empty()).unwrap_or(false)
                    && get(&o, "receipts").as_array().map(|x| x.is_empty()).unwrap_or(false)
                    && get(&o, "orderId").is_null()
                    && get(&o, "ack").is_null()
                    && get(&o, "mayHaveExecuted") == &V::Bool(false),
                "KNOWN_UNADMITTED_ERROR_ONLY",
            )?;
            (get(&o, "phase").clone(), get(&o, "mayHaveExecuted").clone())
        };
        let m = n
            .as_object_mut()
            .unwrap()
            .get_mut("orders")
            .unwrap()
            .as_array_mut()
            .unwrap()[oi]
            .as_object_mut()
            .unwrap();
        m.insert("phase".into(), ph);
        m.insert("mayHaveExecuted".into(), may);
        return Ok(n);
    }
    if kind == "financial_mismatch" {
        need(
            sget(e, "reason") == Some("NON_NEUTRAL_POST_ACCOUNT"),
            "FINANCIAL_MISMATCH_REASON",
        )?;
        let m = n
            .as_object_mut()
            .unwrap()
            .get_mut("orders")
            .unwrap()
            .as_array_mut()
            .unwrap()[oi]
            .as_object_mut()
            .unwrap();
        m.insert("semanticMismatch".into(), json!(true));
        m.insert("phase".into(), json!("semantic_mismatch"));
        return Ok(n);
    }
    if kind == "reconciled" {
        {
            let mut e2 = get(e, "evidence").clone();
            e2.as_object_mut()
                .map(|m| m.insert("firstExchangeId".into(), get(&o, "firstExchangeId").clone()));
            empty_all(&e2, &s, instant_v(get(e, "at"))?)?;
        }
        need(
            !get(&o, "orderId").is_null()
                && !get(&o, "ack").is_null()
                && get(&o, "semanticMismatch") != &V::Bool(true)
                && sget(&o, "phase") == Some("acknowledged"),
            "NON_NEUTRAL_OR_UNKNOWN_BLOCKS",
        )?;
        let mut proof_input = get(e, "evidence").clone();
        {
            let mut bm = get(&s, "scope").as_object().cloned().unwrap_or_default();
            bm.insert("intentId".into(), get(&o, "intentId").clone());
            bm.insert(
                "idempotencyKey".into(),
                get(get(&o, "body"), "idempotencyKey").clone(),
            );
            bm.insert("orderId".into(), get(&o, "orderId").clone());
            bm.insert("marketId".into(), get(&o, "marketId").clone());
            bm.insert(
                "exchangeId".into(),
                get(get(&o, "body"), "exchangeId").clone(),
            );
            bm.insert("side".into(), json!("no"));
            bm.insert("action".into(), get(get(&o, "body"), "action").clone());
            bm.insert("quantity".into(), get(get(&o, "body"), "quantity").clone());
            bm.insert(
                "limit".into(),
                json!(money(price(&num_str(get(get(&o, "body"), "price"))).unwrap_or(0))),
            );
            bm.insert("createdAt".into(), get(&o, "createdAt").clone());
            bm.insert(
                "expirationDate".into(),
                get(get(&o, "body"), "expirationDate").clone(),
            );
            let binding = V::Object(bm);
            proof_input.as_object_mut().map(|m| {
                m.insert("binding".into(), binding);
                m.insert("before".into(), get(&o, "before").clone());
                m.insert("ack".into(), get(&o, "ack").clone());
                m.insert(
                    "dispatchReceipt".into(),
                    get(&o, "fetchStarts")
                        .as_array()
                        .and_then(|x| x.first())
                        .cloned()
                        .unwrap_or(V::Null),
                );
                m.insert("readAt".into(), get(e, "at").clone());
                m.insert(
                    "reservedFunding".into(),
                    json!(if sget(get(&o, "body"), "action") == Some("buy") {
                        money(990000i128 * js_int(get(get(&o, "body"), "quantity")).unwrap_or(0) as i128)
                    } else {
                        "0.000000".to_string()
                    }),
                );
                m.insert("priorProof".into(), get(&o, "priorProof").clone());
                m.insert("priorAfter".into(), get(&o, "priorAfter").clone());
            });
        }
        let proof = crate::no_rest_multishare_reconciliation_v1::verify_scoped_rest_no_multishare(&proof_input)?;
        let mut n2 = n.clone();
        if sget(get(&o, "body"), "action") == Some("sell") {
            need(
                amount(sget(&proof, "realizedPnl").unwrap_or(""))?
                    >= 5000i128 * js_int(get(&proof, "filledQuantity")).unwrap_or(0) as i128,
                "ACTUAL_NET_PROFIT",
            )?;
            let retires = retire_sold_pairs(&s, &o, &proof)?;
            n2.as_object_mut()
                .unwrap()
                .get_mut("pairRetirements")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .extend(retires);
        } else {
            let pairs = assign_fill(&s, &o, &proof)?;
            n2.as_object_mut()
                .unwrap()
                .get_mut("assignments")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .extend(pairs);
        }
        need(
            {
                let mut seen = std::collections::HashSet::new();
                get(&n2, "assignments")
                    .as_array()
                    .map(|xs| {
                        xs.iter()
                            .all(|a| seen.insert(sget(a, "assignmentId").unwrap_or("").to_string()))
                    })
                    .unwrap_or(false)
            },
            "DUPLICATE_PAIR",
        )?;
        {
            let m = n2
                .as_object_mut()
                .unwrap()
                .get_mut("orders")
                .unwrap()
                .as_array_mut()
                .unwrap()[oi]
                .as_object_mut()
                .unwrap();
            m.insert("proof".into(), proof.clone());
            m.insert("phase".into(), json!("reconciled"));
        }
        let ex_key = sget(get(&o, "body"), "exchangeId").unwrap_or("").to_string();
        n2.as_object_mut()
            .unwrap()
            .get_mut("currentInventory")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(
                ex_key.clone(),
                json!({
                    "proof": proof,
                    "after": get(get(e,"evidence"),"after"),
                    "package": get(&o,"package"),
                }),
            );
        if sget(get(&o, "body"), "action") == Some("buy")
            || get(&s, "counterInventory").as_object().map(|m| m.contains_key(&ex_key)).unwrap_or(false)
        {
            let ci = get(get(&n2, "currentInventory"), ex_key.as_str()).clone();
            n2.as_object_mut()
                .unwrap()
                .get_mut("counterInventory")
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(ex_key, ci);
        }
        return Ok(n2);
    }
    need(false, "EVENT").map(|_| n)
}

fn configuration(seed: &V, activation: &V, migration: &V, code: &V) -> Result<(), String> {
    need(
        get(seed, "economicReplayPerformed") == &V::Bool(true)
            && get(seed, "stateHash").as_str()
                == Some(payload_hash(get(seed, "state"))?.as_str())
            && !get(seed, "legacyCodeIdentityHash").is_null()
            && js_int(get(migration, "schemaVersion")) == Some(1)
            && get(migration, "seedHash").as_str()
                == Some(payload_hash(seed)?.as_str())
            && same(get(migration, "legacyAnchor"), get(seed, "anchor"))?
            && get(migration, "legacyCodeIdentityHash") == get(seed, "legacyCodeIdentityHash")
            && sget(migration, "implementationEpoch").is_some_and(|s| !s.is_empty())
            && hex(get(migration, "authorityHash")),
        "VERIFIED_MIGRATION",
    )?;
    need(
        js_int(get(activation, "schemaVersion")) == Some(1)
            && sget(activation, "mode") == Some("baal_neutral_owned_counterpart_pilot_v2")
            && sget(activation, "ownerId") == sget(get(get(seed, "state"), "scope"), "ownerId")
            && sget(activation, "runId") == sget(get(get(seed, "state"), "scope"), "runId")
            && sget(activation, "tournamentId")
                == sget(get(get(seed, "state"), "scope"), "tournamentId")
            && get(activation, "seedHash").as_str() == Some(payload_hash(seed)?.as_str())
            && get(activation, "implementationEpoch") == get(migration, "implementationEpoch")
            && get(activation, "codeIdentityHash").as_str()
                == Some(payload_hash(code)?.as_str())
            && hex(get(activation, "authorityHash"))
            && get(activation, "authorityHash") == get(migration, "authorityHash")
            && js_int(get(activation, "maximumHedgeOrders")) == Some(1)
            && js_int(get(activation, "quantity")) == Some(1)
            && sget(activation, "maximumOrderCost") == Some("0.995000")
            && sget(activation, "maximumPairUnitCost") == Some("0.990000")
            && instant_v(get(activation, "exitExpiresAt"))?
                > instant_v(get(activation, "startedAt"))?
            && instant_v(get(activation, "exitExpiresAt"))?
                - instant_v(get(activation, "startedAt"))?
                <= 86400000
            && get(activation, "allowedFirstExchangeIds")
                .as_array()
                .is_some_and(|xs| xs.len() == 1)
            && js_int(get(
                get(
                    get(
                        get(get(seed, "state"), "inventory"),
                        get(activation, "allowedFirstExchangeIds")
                            .as_array()
                            .and_then(|xs| xs.first())
                            .and_then(V::as_str)
                            .unwrap_or(""),
                    ),
                    "proof",
                ),
                "remainingOwnedQuantity",
            ))
            .unwrap_or(0)
                > 0
            && instant_v(get(activation, "hedgeExpiresAt"))?
                > instant_v(get(activation, "startedAt"))?
            && instant_v(get(activation, "hedgeExpiresAt"))?
                - instant_v(get(activation, "startedAt"))?
                <= 3600000,
        "BOUNDED_NEW_HEDGE_ONLY",
    )?;
    Ok(())
}

/// replayLiveHedgeJournal({bodyText,seed,activation,migration,expectedCodeIdentity})
pub fn replay_live_hedge_journal(input: &V) -> Result<V, String> {
    let seed = get(input, "seed");
    let activation = get(input, "activation");
    let migration = get(input, "migration");
    let expected_code_identity = get(input, "expectedCodeIdentity");
    configuration(seed, activation, migration, expected_code_identity)?;
    let body_text = get(input, "bodyText");
    need(
        body_text
            .as_str()
            .is_some_and(|s| s.ends_with('\n') && s.len() <= 128 * 1024 * 1024),
        "REPLAY_INPUT",
    )?;
    let body_text = body_text.as_str().unwrap_or("");
    let mut state: Option<V> = None;
    let mut sequence: i64 = 0;
    let mut previous = String::new();
    let mut root_prefix: Option<V> = None;
    for line in body_text.trim_end().split('\n') {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let mut body = row.clone();
        let hash = match body.as_object_mut() {
            Some(m) => m.shift_remove("hash").unwrap_or(V::Null),
            None => V::Null,
        };
        sequence += 1;
        need(
            js_stringify(&row) == line
                && js_int(get(&body, "sequence")) == Some(sequence)
                && get(&body, "previous").as_str() == Some(previous.as_str())
                && sha(&js_stringify(&body)) == hash.as_str().unwrap_or("\u{0}")
                && same(
                    get(get(&body, "event"), "codeIdentity"),
                    expected_code_identity,
                )?,
            "CHAIN_CODE",
        )?;
        if sequence == 1 {
            need(
                sget(get(get(&body, "event"), "input"), "kind") == Some("seed")
                    && same(
                        get(get(get(&body, "event"), "input"), "seed"),
                        seed,
                    )?
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
        state = Some(reduce(state, get(get(&body, "event"), "input"))?);
        need(
            same(state.as_ref().unwrap(), get(&body, "state"))?,
            "ECONOMIC_STATE_REPLAY",
        )?;
        previous = hash.as_str().unwrap_or("").to_string();
        if sequence == 1 {
            root_prefix = Some(json!({
                "bytes": line.len() + 1,
                "sha256": sha(&(line.to_string() + "\n")),
                "sequence": sequence,
                "hash": hash,
            }));
        }
    }
    let state = state.unwrap_or(V::Null);
    Ok(json!({
        "state": state,
        "anchor": {
            "bytes": body_text.len(),
            "sha256": sha(body_text),
            "sequence": sequence,
            "hash": previous,
        },
        "rootPrefix": root_prefix,
        "economicReplayPerformed": true,
        "unknownAcceptance": pilot_unknown(&state),
    }))
}

// ---------------------------------------------------------------------------
// openBaalNeutralOwnedCounterpartPilotV2 — the live pilot controller.

struct InFlight {
    spec: V,
    bd_ptr: usize,
    #[allow(dead_code)]
    bn_ptr: usize,
}

pub struct NeutralPilotDeps {
    pub dispatch: Rc<crate::paired_rest_controller::WireDispatch>,
    pub raw_receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub assert_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub assert_financial_owner_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub install_funding_reservation: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub get_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub get_evidence: Rc<dyn Fn(&V) -> Result<V, String>>,
    pub on_financial_halt: Rc<dyn Fn(&V) -> Result<(), String>>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    /// Defaults to can_operate (JS canCheckMechanical=canOperate).
    pub can_check_mechanical: Option<Rc<dyn Fn() -> bool>>,
    /// Defaults to assert_account_lease (JS assertMechanicalAccountLease).
    pub assert_mechanical_account_lease: Option<Rc<dyn Fn() -> Result<V, String>>>,
    /// Defaults to assert_financial_owner_lease (JS assertMechanicalFinancialOwnerLease).
    pub assert_mechanical_financial_owner_lease: Option<Rc<dyn Fn() -> Result<V, String>>>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct Inner {
    seed: V,
    seed_hash: String,
    original_inventory_hash: String,
    original_orders_hash: String,
    activation: V,
    migration: V,
    file: PathBuf,
    code: V,
    journal: Journal,
    state: Option<V>,
    closed: bool,
    failed: bool,
    busy: bool,
    size: u64,
    bytes_hash: String,
    ino: u64,
    birthtime_ms: f64,
    in_flight: Option<InFlight>,
    issued: HashMap<usize, V>,
    lease: OwnerLease,
    last_guard_failure: Option<V>,
    fetch_deadline: Option<f64>,
    local_timings: RefCell<Vec<V>>,
    can_check_mechanical: Rc<dyn Fn() -> bool>,
    assert_mechanical_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    assert_mechanical_financial_owner_lease: Rc<dyn Fn() -> Result<V, String>>,
    legacy_file: PathBuf,
    legacy_bytes: Vec<u8>,
    workspace: PathBuf,
    deps: NeutralPilotDeps,
}

pub struct BaalNeutralOwnedCounterpartPilotV2 {
    inner: Rc<RefCell<Inner>>,
    transport: OrderTransport,
}

fn diagnostic_code(message: &str) -> String {
    let ok = |s: &str| {
        let b = s.as_bytes();
        !b.is_empty()
            && b.len() >= 2
            && b.len() <= 141
            && b[0].is_ascii_alphabetic()
            && b[1..].iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
    };
    if ok(message) {
        message.to_uppercase()
    } else {
        "OPERATION_FAILED".to_string()
    }
}

fn measure<T>(
    inner: &Rc<RefCell<Inner>>,
    name: &str,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let start = Instant::now();
    let mut failure: Option<(String, String)> = None;
    let r = f();
    if let Err(e) = &r {
        failure = Some((
            diagnostic_code(e),
            iso((inner.borrow().deps.now)()),
        ));
    }
    let dur = (start.elapsed().as_secs_f64() * 1000.0 * 1000.0).round() / 1000.0;
    let mut row = Map::new();
    row.insert("name".into(), json!(name));
    row.insert("durationMs".into(), json!(dur));
    if let Some((code, at)) = failure {
        row.insert("errorCode".into(), json!(code));
        row.insert("failedAt".into(), json!(at));
    }
    {
        let g = inner.borrow();
        let mut t = g.local_timings.borrow_mut();
        t.push(V::Object(row));
        if t.len() > 96 {
            t.remove(0);
        }
    }
    r
}

fn measure_into<T>(
    timings: &RefCell<Vec<V>>,
    now: &dyn Fn() -> f64,
    name: &str,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let start = Instant::now();
    let mut failure: Option<(String, String)> = None;
    let r = f();
    if let Err(e) = &r {
        failure = Some((diagnostic_code(e), iso(now())));
    }
    let dur = (start.elapsed().as_secs_f64() * 1000.0 * 1000.0).round() / 1000.0;
    let mut row = Map::new();
    row.insert("name".into(), json!(name));
    row.insert("durationMs".into(), json!(dur));
    if let Some((code, at)) = failure {
        row.insert("errorCode".into(), json!(code));
        row.insert("failedAt".into(), json!(at));
    }
    let mut t = timings.borrow_mut();
    t.push(V::Object(row));
    if t.len() > 96 {
        t.remove(0);
    }
    r
}

#[allow(clippy::too_many_arguments)]
fn mechanical_at(
    stopped: bool,
    can_check_mechanical: &dyn Fn() -> bool,
    lease: &OwnerLease,
    assert_mechanical_account_lease: &dyn Fn() -> Result<V, String>,
    assert_mechanical_financial_owner_lease: &dyn Fn() -> Result<V, String>,
    activation: &V,
    migration: &V,
    timings: &RefCell<Vec<V>>,
    now: &dyn Fn() -> f64,
) -> Result<V, String> {
    need(!stopped && can_check_mechanical(), "STOPPED")?;
    lease.assert_ownership().map_err(|e| e.message)?;
    let l = measure_into(timings, now, "basic_account_lease", || {
        assert_mechanical_account_lease()
    })?;
    let f = measure_into(timings, now, "basic_financial_lease", || {
        assert_mechanical_financial_owner_lease()
    })?;
    for x in [&l, &f] {
        need(
            sget(x, "ownerId") == sget(activation, "ownerId")
                && sget(x, "runId") == sget(activation, "runId")
                && sget(x, "tournamentId") == sget(activation, "tournamentId")
                && sget(x, "leaseId").is_some_and(|s| !s.is_empty()),
            "ACTUAL_ACCOUNT_LEASE",
        )?;
    }
    need(
        get(&f, "implementationEpoch") == get(migration, "implementationEpoch"),
        "FINANCIAL_OWNER_EPOCH",
    )?;
    need(
        l.as_object()
            .map(|m| {
                m.keys()
                    .all(|k| ["ownerId", "runId", "tournamentId", "leaseId"].contains(&k.as_str()))
            })
            .unwrap_or(false),
        "LEASE_RECEIPT_SCHEMA",
    )?;
    Ok(l)
}

#[allow(clippy::too_many_arguments)]
fn basic_at(
    can_operate: &dyn Fn() -> bool,
    can_check_mechanical: &dyn Fn() -> bool,
    lease: &OwnerLease,
    assert_mechanical_account_lease: &dyn Fn() -> Result<V, String>,
    assert_mechanical_financial_owner_lease: &dyn Fn() -> Result<V, String>,
    activation: &V,
    migration: &V,
    legacy_file: &Path,
    legacy_bytes: &[u8],
    seed: &V,
    workspace: &Path,
    code: &V,
    timings: &RefCell<Vec<V>>,
    now: &dyn Fn() -> f64,
) -> Result<V, String> {
    need(can_operate(), "STOPPED")?;
    let l = mechanical_at(
        false,
        can_check_mechanical,
        lease,
        assert_mechanical_account_lease,
        assert_mechanical_financial_owner_lease,
        activation,
        migration,
        timings,
        now,
    )?;
    let actual = measure_into(timings, now, "basic_legacy_fresh_read", || {
        fs::read(legacy_file).map_err(|e| e.to_string())
    })?;
    let unchanged = measure_into(timings, now, "basic_legacy_exact_compare", || {
        Ok(actual == legacy_bytes)
    })?;
    let code_same = measure_into(timings, now, "basic_current_code_graph", || {
        same(&live_coordinator_identity(workspace)?, code)
    })?;
    need(
        actual.len()
            == get(get(seed, "anchor"), "bytes")
                .as_u64()
                .unwrap_or(0) as usize
            && unchanged
            && code_same,
        "LEGACY_NOT_SEALED_OR_CODE_CHANGED",
    )?;
    need(
        l.as_object()
            .map(|m| {
                m.keys()
                    .all(|k| ["ownerId", "runId", "tournamentId", "leaseId"].contains(&k.as_str()))
            })
            .unwrap_or(false),
        "LEASE_RECEIPT_SCHEMA",
    )?;
    Ok(l)
}

impl Inner {
    fn mechanical(&self) -> Result<V, String> {
        let stopped = self.closed || self.failed;
        mechanical_at(
            stopped,
            &*self.can_check_mechanical,
            &self.lease,
            &*self.assert_mechanical_account_lease,
            &*self.assert_mechanical_financial_owner_lease,
            &self.activation,
            &self.migration,
            &self.local_timings,
            &*self.deps.now,
        )
    }
    fn basic(&self) -> Result<V, String> {
        need((self.deps.can_operate)(), "STOPPED")?;
        let l = self.mechanical()?;
        let timings = &self.local_timings;
        let now_fn = &*self.deps.now;
        let actual = measure_into(timings, now_fn, "basic_legacy_fresh_read", || {
            fs::read(&self.legacy_file).map_err(|e| e.to_string())
        })?;
        let unchanged = measure_into(timings, now_fn, "basic_legacy_exact_compare", || {
            Ok(actual == self.legacy_bytes)
        })?;
        let code_same = measure_into(timings, now_fn, "basic_current_code_graph", || {
            same(&live_coordinator_identity(&self.workspace)?, &self.code)
        })?;
        need(
            actual.len()
                == get(get(&self.seed, "anchor"), "bytes")
                    .as_u64()
                    .unwrap_or(0) as usize
                && unchanged
                && code_same,
            "LEGACY_NOT_SEALED_OR_CODE_CHANGED",
        )?;
        need(
            l.as_object()
                .map(|m| {
                    m.keys().all(|k| {
                        ["ownerId", "runId", "tournamentId", "leaseId"].contains(&k.as_str())
                    })
                })
                .unwrap_or(false),
            "LEASE_RECEIPT_SCHEMA",
        )?;
        Ok(l)
    }
}

fn append(inner: &Rc<RefCell<Inner>>, input: &V) -> Result<(), String> {
    inner.borrow().basic()?;
    let (before, size, bytes_hash, ino, birthtime) = {
        let g = inner.borrow();
        let before = fs::read(&g.file).map_err(|e| e.to_string())?;
        let stat = fs::metadata(&g.file).map_err(|e| e.to_string())?;
        use std::os::unix::fs::MetadataExt;
        (
            before,
            g.size,
            g.bytes_hash.clone(),
            stat.ino(),
            stat.created()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0),
        )
    };
    need(
        ino == inner.borrow().ino
            && birthtime == inner.borrow().birthtime_ms
            && before.len() as u64 == size
            && sha_bytes(&before) == bytes_hash,
        "JOURNAL_CHANGED",
    )?;
    let next = reduce(inner.borrow().state.clone(), input)?;
    need(
        (size + js_stringify(&next).len() as u64 + js_stringify(input).len() as u64)
            < 128 * 1024 * 1024,
        "JOURNAL_BOUND",
    )?;
    let event = {
        let g = inner.borrow();
        json!({"codeIdentity": g.code, "input": input})
    };
    match inner.borrow_mut().journal.commit(&next, &event) {
        Ok(_) => {
            inner.borrow().basic()?;
            let all = fs::read(&inner.borrow().file).map_err(|e| e.to_string())?;
            need(
                all[..size as usize] == before[..] && all.len() as u64 > size,
                "PREFIX_CHANGED",
            )?;
            let mut g = inner.borrow_mut();
            g.state = Some(next);
            g.size = all.len() as u64;
            g.bytes_hash = sha_bytes(&all);
            Ok(())
        }
        Err(e) => {
            inner.borrow_mut().failed = true;
            Err(e)
        }
    }
}

fn reserve(inner: &Rc<RefCell<Inner>>) -> Result<V, String> {
    // JS: the entire body is wrapped in try/catch marking failed+fatal.
    let result = (|| -> Result<V, String> {
        let res = {
            let g = inner.borrow();
            funding(g.state.as_ref().unwrap_or(&V::Null))?
        };
        let receipt = (inner.borrow().deps.install_funding_reservation)(&res)?;
        append(
            inner,
            &json!({"kind": "reservation", "receipt": receipt, "at": iso((inner.borrow().deps.now)())}),
        )?;
        Ok(receipt)
    })();
    match result {
        Ok(v) => Ok(v),
        Err(e) => {
            inner.borrow_mut().failed = true;
            Err(e)
        }
    }
}

fn ensure_installed(inner: &Rc<RefCell<Inner>>) -> Result<V, String> {
    inner.borrow().basic()?;
    let (record, r, state, seed_hash, epoch) = {
        let g = inner.borrow();
        let st = g.state.clone().unwrap_or(V::Null);
        (
            funding(&st)?,
            get(&st, "reservation").clone(),
            st,
            g.seed_hash.clone(),
            get(&g.migration, "implementationEpoch").clone(),
        )
    };
    let now = (inner.borrow().deps.now)() as i64;
    let mut valid = get(&r, "reservationHash") == get(&record, "reservationHash")
        && get(&r, "allocationHash") == get(&record, "allocationHash")
        && get(&r, "seedHash").as_str() == Some(seed_hash.as_str())
        && hex(get(&r, "globalLedgerHash"))
        && hex(get(&r, "budgetInstalledHash"))
        && instant_v(get(&r, "confirmedAt"))? <= now
        && now - instant_v(get(&r, "confirmedAt"))? <= 20000;
    if valid {
        let g = (inner.borrow().deps.get_global_risk)()?;
        let o = active(&state);
        valid = sget(&g, "ownerId") == sget(&state, "ownerId")
            && sget(&g, "runId") == sget(&state, "runId")
            && sget(&g, "tournamentId") == sget(&state, "tournamentId")
            && get(&g, "complete") == &V::Bool(true)
            && get(&g, "eventScopeComplete") == &V::Bool(true)
            && get(&g, "soleCollector") == &V::Bool(true)
            && get(&g, "stopped") == &V::Bool(false)
            && get(&g, "unknownAcceptance") == &V::Bool(false)
            && hex(get(&g, "sourceHash"))
            && get(&g, "unresolvedIntentIds").as_array().is_some_and(|ids| {
                ids.iter()
                    .all(|id| get(&o.clone().unwrap_or(V::Null), "intentId") == id)
            })
            && get(&g, "controllerReservationHash") == get(&record, "reservationHash")
            && get(&g, "globalLedgerHash") == get(&r, "globalLedgerHash")
            && get(&g, "budgetInstalledHash") == get(&r, "budgetInstalledHash")
            && get(&g, "allocationHash") == get(&record, "allocationHash")
            && get(&g, "seedHash").as_str() == Some(seed_hash.as_str())
            && get(&g, "implementationEpoch") == &epoch
            && get(&g, "fundingSatisfied") == &V::Bool(true)
            && get(&g, "installedCashLower").as_str().is_some()
            && get(&g, "cashLower") == get(&g, "installedCashLower")
            && instant_v(get(&g, "at"))? <= now
            && now - instant_v(get(&g, "at"))? <= 20000;
    }
    if !valid {
        return reserve(inner);
    }
    Ok(r)
}

fn evidence(
    inner: &Rc<RefCell<Inner>>,
    operation: &str,
    first_ex: Option<&str>,
    fence_risk: Option<&V>,
) -> Result<V, String> {
    let (o, exchanges) = {
        let g = inner.borrow();
        let st = g.state.clone().unwrap_or(V::Null);
        (
            active(&st).unwrap_or(V::Null),
            get(get(&st, "scope"), "exchanges").clone(),
        )
    };
    let first = first_ex
        .map(|x| x.to_string())
        .or_else(|| sget(&o, "firstExchangeId").map(|x| x.to_string()));
    let exchange_id = sget(get(&o, "body"), "exchangeId")
        .map(|x| x.to_string())
        .or_else(|| first.clone());
    // JS: o?.marketId ?? scope.exchanges.find(x=>x.exchangeId===firstEx)?.marketId ?? null
    // — the find() uses the firstEx PARAMETER only (no o.firstExchangeId fallback).
    let market_id = if !get(&o, "marketId").is_null() {
        get(&o, "marketId").clone()
    } else {
        first_ex
            .and_then(|fx| {
                exchanges.as_array().and_then(|xs| {
                    xs.iter()
                        .find(|x| sget(x, "exchangeId") == Some(fx))
                        .and_then(|x| x.get("marketId"))
                        .cloned()
                })
            })
            .unwrap_or(V::Null)
    };
    (inner.borrow().deps.get_evidence)(&json!({
        "operation": operation,
        "fenceRisk": fence_risk.map(|x| x.clone()).unwrap_or(V::Null),
        "intentId": get(&o, "intentId"),
        "orderId": get(&o, "orderId"),
        "firstExchangeId": first,
        "exchangeId": exchange_id,
        "marketId": market_id,
    }))
}

fn validate(inner: &Rc<RefCell<Inner>>, p: &V, spec: &V, network: bool) -> Result<bool, String> {
    inner.borrow().mechanical()?;
    let (state, o) = {
        let g = inner.borrow();
        let st = g.state.clone().unwrap_or(V::Null);
        (st.clone(), active(&st))
    };
    let o = o.ok_or_else(|| "HEDGE_COORDINATOR_HEDGE_ONLY_FIXED_PERMIT".to_string())?;
    need(
        get(p, "intentId") == get(&o, "intentId")
            && get(p, "bodyHash") == get(&o, "bodyHash")
            && sget(spec, "method") == Some("POST")
            && sget(spec, "endpoint") == Some("/orders")
            && get(spec, "bodyJson") == get(&o, "bodyJson")
            && !pilot_unknown(&state),
        "HEDGE_ONLY_FIXED_PERMIT",
    )?;
    let r = get(&state, "reservation");
    let now = (inner.borrow().deps.now)();
    need(
        get(r, "reservationHash") == get(&funding(&state)?, "reservationHash")
            && get(r, "allocationHash").as_str()
                == Some(payload_hash(get(&state, "allocation"))?.as_str())
            && hex(get(r, "globalLedgerHash"))
            && instant_v(get(r, "confirmedAt"))? <= now as i64
            && now as i64 - instant_v(get(r, "confirmedAt"))? <= 20000,
        "FULL_RESERVE_INSTALLED",
    )?;
    let before_fence = json!({
        "recordHash": get(&funding(&state)?, "reservationHash"),
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
    let l = measure(inner, "guard_current_lease", || inner.borrow().basic())?;
    let g = measure(
        inner,
        if network {
            "network_current_budget_risk"
        } else {
            "admission_current_budget_risk"
        },
        || (inner.borrow().deps.get_global_risk)(),
    )?;
    let op = if sget(get(&o, "body"), "action") == Some("buy") {
        "place_hedge"
    } else {
        "place_sell"
    };
    let e = measure(
        inner,
        if network {
            "network_cached_evidence"
        } else {
            "admission_cached_evidence"
        },
        || evidence(inner, op, None, Some(&g)),
    )?;
    let lease_hash = payload_hash(&l)?;
    let at_iso = iso((inner.borrow().deps.now)());
    measure(
        inner,
        if network {
            "network_durable_fence_append"
        } else {
            "admission_durable_event_append"
        },
        || {
            append(
                inner,
                &json!({
                    "kind": if network { "network_fence" } else { "admitted" },
                    "evidence": e,
                    "globalRisk": g,
                    "lease": l,
                    "leaseHash": lease_hash,
                    "at": at_iso,
                }),
            )
        },
    )?;
    if network {
        let o = {
            let g2 = inner.borrow();
            active(&g2.state.clone().unwrap_or(V::Null)).unwrap_or(V::Null)
        };
        let l2 = measure(inner, "network_postappend_current_lease", || {
            inner.borrow().basic()
        })?;
        need(same(&l2, &l)?, "NETWORK_LEASE_CHANGED_AFTER_COMMIT")?;
        measure(inner, "network_postappend_current_journal", || {
            let (file, size, bytes_hash, ino, birthtime, seq, prev) = {
                let g = inner.borrow();
                (
                    g.file.clone(),
                    g.size,
                    g.bytes_hash.clone(),
                    g.ino,
                    g.birthtime_ms,
                    g.journal.sequence,
                    g.journal.previous.clone(),
                )
            };
            let stat = fs::metadata(&file).map_err(|e| e.to_string())?;
            let fresh = fs::read(&file).map_err(|e| e.to_string())?;
            let nl = {
                let mut idx = fresh.len().saturating_sub(2);
                while idx > 0 && fresh[idx] != b'\n' {
                    idx -= 1;
                }
                if fresh.len() >= 2 && idx == 0 && fresh[0] != b'\n' {
                    0
                } else {
                    idx + 1
                }
            };
            let last: V = serde_json::from_str(&String::from_utf8_lossy(&fresh[nl.min(fresh.len())..]))
                .map_err(|e| e.to_string())?;
            use std::os::unix::fs::MetadataExt;
            let st_ino = stat.ino();
            let st_birth = stat
                .created()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0);
            need(
                st_ino == ino
                    && st_birth == birthtime
                    && fresh.len() as u64 == size
                    && sha_bytes(&fresh) == bytes_hash
                    && js_int(get(&last, "sequence")).map(|x| x as u64) == Some(seq)
                    && get(&last, "hash").as_str() == Some(prev.as_str()),
                "CURRENT_JOURNAL_CHANGED",
            )?;
            Ok(())
        })?;
        need(
            get(&before_fence, "recordHash") == get(&funding(&state)?, "reservationHash")
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
        let at_now = (inner.borrow().deps.now)() as i64;
        let state2 = inner.borrow().state.clone().unwrap_or(V::Null);
        measure(inner, "network_postappend_policy_validation", || {
            if sget(get(&o, "body"), "action") == Some("buy") {
                guarded(&state2, &e, &g, at_now).map(|_| ())
            } else {
                sell_proposal(
                    &state2,
                    sget(get(&o, "body"), "exchangeId").unwrap_or(""),
                    &e,
                    &g,
                    at_now,
                )
                .map(|_| ())
            }
        })?;
        let deadline = instant_v(get(get(&o, "body"), "expirationDate"))?
            .min(instant_v(get(get(&e, "graphReceipt"), "requestStartedAt"))? + 5000)
            .min(instant_v(get(get(&e, "collateralReceipt"), "requestStartedAt"))? + 5000);
        inner.borrow_mut().fetch_deadline = Some(deadline as f64);
    }
    Ok(true)
}

fn guarded_permit(
    inner: &Rc<RefCell<Inner>>,
    p: &V,
    s: &V,
    network: bool,
) -> Result<bool, OrderTransportError> {
    match validate(inner, p, s, network) {
        Ok(v) => Ok(v),
        Err(e) => {
            {
                let mut g = inner.borrow_mut();
                g.last_guard_failure = Some(json!({
                    "stage": if network { "network" } else { "admission" },
                    "errorCode": diagnostic_code(&e),
                    "failedAt": iso((g.deps.now)()),
                }));
            }
            if inner.borrow().failed {
                // JS: if(error.fatal||error.durableRecordingFailed)throw error;
                return Err(OrderTransportError {
                    code: e,
                    outcome: "unknown".into(),
                    may_have_executed: true,
                    retry_same_request_only: false,
                    retry_after_ms: 0.0,
                    status: None,
                    api_code: None,
                    semantic_mismatch: false,
                });
            }
            Err(OrderTransportError {
                code: "OWNED_CUSTODY_DISPATCH_GUARD".into(),
                outcome: "blocked".into(),
                may_have_executed: false,
                retry_same_request_only: false,
                retry_after_ms: 0.0,
                status: None,
                api_code: None,
                semantic_mismatch: false,
            })
        }
    }
}

fn wrap(
    inner: &Rc<RefCell<Inner>>,
    spec: &V,
    bd: Option<&Guard<'_>>,
    bn: Option<&Guard<'_>>,
) -> Result<V, crate::paired_rest_controller::WireDispatchError> {
    let out = (|| -> Result<V, String> {
        let bd_ptr = bd
            .map(|g| g as *const Guard as *const () as usize)
            .unwrap_or(0);
        let bn_ptr = bn
            .map(|g| g as *const Guard as *const () as usize)
            .unwrap_or(0);
        {
            let mut g = inner.borrow_mut();
            g.issued.insert(
                bd_ptr,
                json!({
                    "method": get(spec, "method"),
                    "endpoint": get(spec, "endpoint"),
                    "bodyJson": get(spec, "bodyJson"),
                    "beforeNetwork": bn_ptr,
                }),
            );
            g.in_flight = Some(InFlight {
                spec: spec.clone(),
                bd_ptr,
                bn_ptr,
            });
        }
        let capture = |response: &V| -> Result<(), String> {
            let o = {
                let g = inner.borrow();
                active(&g.state.clone().unwrap_or(V::Null)).unwrap_or(V::Null)
            };
            let r = (inner.borrow().deps.raw_receipt_of)(
                response,
                &json!({
                    "method": "POST",
                    "requestBodyHash": get(&o, "bodyHash"),
                }),
            )?;
            need(
                get(&r, "status") == get(response, "status")
                    && sget(&r, "endpoint") == Some("/orders")
                    && instant_v(get(&r, "requestStartedAt"))?
                        >= instant_v(get(response, "requestStartedAt"))?
                    && instant_v(get(&r, "receivedAt"))?
                        <= instant_v(get(response, "receivedAt"))?
                    && same(
                        &parse_number_lexemes(sget(&r, "bodyText").unwrap_or(""))?.0,
                        get(response, "data"),
                    )?,
                "RAW_RESPONSE_BINDING",
            )?;
            append(
                inner,
                &json!({"kind": "wire", "receipt": r, "at": iso((inner.borrow().deps.now)())}),
            )?;
            {
                let g = inner.borrow();
                if pilot_unknown(&g.state.clone().unwrap_or(V::Null)) {
                    let o = active(&g.state.clone().unwrap_or(V::Null)).unwrap_or(V::Null);
                    (g.deps.on_financial_halt)(&json!({
                        "kind": "non_neutral_or_unknown_hedge",
                        "intentId": get(&o, "intentId"),
                        "sourceHash": get(&r, "sourceBodyHash"),
                        "allWritersStop": true,
                    }))?;
                }
            }
            Ok(())
        };
        let result = (inner.borrow().deps.dispatch)(spec, bd, bn);
        let r = match result {
            Ok(response) => {
                capture(&response)?;
                Ok(response)
            }
            Err(e) => {
                if let Some(wr) = e.wire_receipt.clone() {
                    capture(&wr)?;
                }
                Err(e.message)
            }
        };
        {
            let mut g = inner.borrow_mut();
            g.issued.remove(&bd_ptr);
            g.in_flight = None;
        }
        r
    })();
    out.map_err(|e| crate::paired_rest_controller::WireDispatchError {
        message: e,
        wire_receipt: None,
    })
}

fn status_of(inner: &Rc<RefCell<Inner>>) -> Result<V, String> {
    let g = inner.borrow();
    let state = g.state.clone().unwrap_or(V::Null);
    let o = active(&state);
    let orders = get(&state, "orders").as_array().cloned().unwrap_or_default();
    let mut inv = Map::new();
    if let Some(inventory) = get(&state, "currentInventory").as_object() {
        for (ex, x) in inventory {
            inv.insert(
                ex.clone(),
                json!({
                    "quantity": get(get(x,"proof"),"remainingOwnedQuantity"),
                    "cost": get(get(x,"proof"),"remainingOwnedCost"),
                    "realizedPnl": get(get(x,"proof"),"cumulativeRealizedPnl"),
                }),
            );
        }
    }
    let realized: i128 = get(&state, "currentInventory")
        .as_object()
        .map(|inventory| {
            inventory
                .values()
                .map(|x| {
                    amount(sget(get(x, "proof"), "cumulativeRealizedPnl").unwrap_or(""))
                        .unwrap_or(0)
                })
                .sum()
        })
        .unwrap_or(0);
    let intent = o.as_ref().map(|o| {
        json!({
            "intentId": get(o,"intentId"),
            "firstExchangeId": get(o,"firstExchangeId"),
            "orderId": get(o,"orderId"),
            "action": get(get(o,"body"),"action"),
            "marketId": get(o,"marketId"),
            "exchangeId": get(get(o,"body"),"exchangeId"),
            "body": get(o,"body"),
            "bodyHash": get(o,"bodyHash"),
            "phase": get(o,"phase"),
            "createdAt": get(o,"createdAt"),
            "packageLegs": get(get(o,"package"),"legs"),
            "actualFetchCount": get(o,"fetchStarts").as_array().map(|x|x.len()).unwrap_or(0),
            "mayHaveExecuted": get(o,"mayHaveExecuted"),
            "cancelPending": false,
        })
    });
    let phase = o
        .as_ref()
        .and_then(|o| sget(o, "phase").map(|x| x.to_string()))
        .unwrap_or_else(|| {
            if orders.is_empty() {
                "ready".to_string()
            } else {
                "terminal".to_string()
            }
        });
    let mut out = Map::new();
    out.insert("mode".into(), get(&state, "mode").clone());
    // {...state.scope} spread after mode — scope keys in scope order.
    if let Some(m) = get(&state, "scope").as_object() {
        for (k, v) in m {
            out.insert(k.clone(), v.clone());
        }
    }
    out.insert("seedHash".into(), json!(g.seed_hash));
    out.insert(
        "implementationEpoch".into(),
        get(&g.migration, "implementationEpoch").clone(),
    );
    out.insert("sequence".into(), json!(g.journal.sequence));
    out.insert("hash".into(), json!(g.journal.previous));
    out.insert("phase".into(), json!(phase));
    out.insert("intent".into(), intent.unwrap_or(V::Null));
    out.insert(
        "unknownAcceptance".into(),
        json!(pilot_unknown(&state)),
    );
    out.insert("assignments".into(), get(&state, "assignments").clone());
    out.insert(
        "pairRetirements".into(),
        get(&state, "pairRetirements").clone(),
    );
    out.insert("inventory".into(), V::Object(inv));
    out.insert("realizedPnl".into(), json!(money(realized)));
    out.insert("matchedQuantity".into(), json!(matched(&state)));
    out.insert(
        "originalInventoryHash".into(),
        json!(g.original_inventory_hash),
    );
    out.insert("originalOrdersHash".into(), json!(g.original_orders_hash));
    out.insert("funding".into(), funding(&state)?);
    out.insert(
        "competitionOrders".into(),
        json!(orders
            .iter()
            .filter(|o| {
                get(o, "fetchStarts")
                    .as_array()
                    .map(|x| !x.is_empty())
                    .unwrap_or(false)
            })
            .count()),
    );
    out.insert(
        "hedgeIntentCount".into(),
        json!(orders
            .iter()
            .filter(|o| sget(get(o, "body"), "action") == Some("buy"))
            .count()),
    );
    out.insert(
        "abandonedIntentCount".into(),
        json!(orders
            .iter()
            .filter(|o| sget(o, "phase") == Some("abandoned"))
            .count()),
    );
    out.insert(
        "executionOwnerEpoch".into(),
        get(&g.migration, "implementationEpoch").clone(),
    );
    out.insert("ownedSellSupported".into(), json!(true));
    out.insert(
        "handbackRequiresExplicitImplementationMigration".into(),
        json!(true),
    );
    out.insert("cashProfitEstablished".into(), json!(false));
    out.insert("recentLocalTimings".into(), json!(g.local_timings));
    out.insert(
        "lastGuardFailure".into(),
        g.last_guard_failure.clone().unwrap_or(V::Null),
    );
    out.insert("closed".into(), json!(g.closed));
    out.insert("failed".into(), json!(g.failed));
    out.insert("busy".into(), json!(g.busy));
    Ok(V::Object(out))
}

fn run<T>(
    inner: &Rc<RefCell<Inner>>,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    need(!inner.borrow().busy, "BUSY")?;
    inner.borrow_mut().busy = true;
    let r = f();
    inner.borrow_mut().busy = false;
    r
}

/// openBaalNeutralOwnedCounterpartPilotV2({dir,workspace,legacyFile,
/// legacyCodeIdentity,legacyScope,legacyAllocation,activation,migration,...deps})
#[allow(clippy::too_many_arguments)]
pub fn open_baal_neutral_owned_counterpart_pilot_v2(
    dir: &Path,
    workspace: &Path,
    legacy_file: &Path,
    legacy_code_identity: &V,
    legacy_scope: &V,
    legacy_allocation: &V,
    activation: &V,
    migration: &V,
    deps: NeutralPilotDeps,
) -> Result<BaalNeutralOwnedCounterpartPilotV2, String> {
    need(
        dir.is_absolute() && workspace.is_absolute() && legacy_file.is_absolute(),
        "DEPENDENCIES",
    )?;
    let bytes = fs::read(legacy_file).map_err(|e| e.to_string())?;
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
    let mut lease = crate::ledger::acquire_owner_lease(
        dir,
        sget(activation, "ownerId").unwrap_or(""),
        sget(activation, "runId").unwrap_or(""),
        "neutral-owned-counterpart-pilot",
        (deps.now)(),
    )
    .map_err(|e| e.message)?;
    let file = dir.join("neutral-owned-counterpart-pilot-v2.jsonl");
    let can_check_mechanical = deps
        .can_check_mechanical
        .clone()
        .unwrap_or_else(|| deps.can_operate.clone());
    let assert_mechanical_account_lease = deps
        .assert_mechanical_account_lease
        .clone()
        .unwrap_or_else(|| deps.assert_account_lease.clone());
    let assert_mechanical_financial_owner_lease = deps
        .assert_mechanical_financial_owner_lease
        .clone()
        .unwrap_or_else(|| deps.assert_financial_owner_lease.clone());
    let open_timings: RefCell<Vec<V>> = RefCell::new(Vec::new());
    let open_result: Result<(Journal, Option<V>, u64, String, u64, f64), String> = (|| {
        basic_at(
            &*deps.can_operate,
            &*can_check_mechanical,
            &lease,
            &*assert_mechanical_account_lease,
            &*assert_mechanical_financial_owner_lease,
            activation,
            migration,
            legacy_file,
            &bytes,
            &seed,
            workspace,
            &code,
            &open_timings,
            &*deps.now,
        )?;
        if !file.exists() {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&file)
                .map_err(|e| e.to_string())?;
        }
        let data = fs::read(&file).map_err(|e| e.to_string())?;
        need(data.is_empty() || *data.last().unwrap() == 10, "PARTIAL_JOURNAL")?;
        let journal = Journal::open(&file)?;
        let mut state: Option<V> = None;
        if !data.is_empty() {
            state = Some(
                get(
                    &replay_live_hedge_journal(&json!({
                        "bodyText": String::from_utf8_lossy(&data),
                        "seed": seed,
                        "activation": activation,
                        "migration": migration,
                        "expectedCodeIdentity": code,
                    }))?,
                    "state",
                )
                .clone(),
            );
        }
        let size = data.len() as u64;
        let bytes_hash = sha_bytes(&data);
        let stat = fs::metadata(&file).map_err(|e| e.to_string())?;
        use std::os::unix::fs::MetadataExt;
        Ok((
            journal,
            state,
            size,
            bytes_hash,
            stat.ino(),
            stat.created()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0),
        ))
    })();
    let (journal, state, size, bytes_hash, ino, birthtime) = match open_result {
        Ok(v) => v,
        Err(e) => {
            let _ = lease.release();
            return Err(e);
        }
    };
    let inner = Rc::new(RefCell::new(Inner {
        seed,
        seed_hash,
        original_inventory_hash,
        original_orders_hash,
        activation: activation.clone(),
        migration: migration.clone(),
        file: file.clone(),
        code,
        journal,
        state,
        closed: false,
        failed: false,
        busy: false,
        size,
        bytes_hash,
        ino,
        birthtime_ms: birthtime,
        in_flight: None,
        issued: HashMap::new(),
        lease,
        last_guard_failure: None,
        fetch_deadline: None,
        local_timings: RefCell::new(open_timings.into_inner()),
        can_check_mechanical,
        assert_mechanical_account_lease,
        assert_mechanical_financial_owner_lease,
        legacy_file: legacy_file.to_path_buf(),
        legacy_bytes: bytes,
        workspace: workspace.to_path_buf(),
        deps,
    }));
    if inner.borrow().state.is_none() {
        let (sd, sh, ac, mg, at) = {
            let g = inner.borrow();
            (
                g.seed.clone(),
                g.seed_hash.clone(),
                g.activation.clone(),
                g.migration.clone(),
                iso((g.deps.now)()),
            )
        };
        append(
            &inner,
            &json!({
                "kind": "seed",
                "seed": sd,
                "seedHash": sh,
                "activation": ac,
                "migration": mg,
                "at": at,
            }),
        )?;
    }
    let transport = {
        let inner_w = inner.clone();
        let dispatch_wrap: Box<crate::transport::Dispatch> = Box::new(move |spec, bd, bn| {
            wrap(&inner_w, spec, bd, bn).map_err(|e| OrderTransportError {
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
        let scope = get(&inner.borrow().state.clone().unwrap_or(V::Null), "scope").clone();
        OrderTransport::new(OrderTransportOptions {
            dispatch: dispatch_wrap,
            scope: crate::transport::scope_of(&scope).map_err(|e| e.code)?,
            can_write: Box::new({
                let inner = inner.clone();
                move || {
                    let g = inner.borrow();
                    g.mechanical().is_ok()
                        && !pilot_unknown(&g.state.clone().unwrap_or(V::Null))
                }
            }),
            owns_order: Box::new({
                let inner = inner.clone();
                move |oid: &str| {
                    let g = inner.borrow();
                    active(&g.state.clone().unwrap_or(V::Null))
                        .map(|o| get(&o, "orderId").as_str() == Some(oid))
                        .unwrap_or(false)
                }
            }),
            validate_permit: Box::new({
                let inner = inner.clone();
                move |p: &V, spec: &V| guarded_permit(&inner, p, spec, false)
            }),
            validate_network_permit: Box::new({
                let inner = inner.clone();
                move |p: &V, spec: &V| guarded_permit(&inner, p, spec, true)
            }),
            validate_lease: Box::new({
                let inner = inner.clone();
                move |l: &V, _s: &V| {
                    inner
                        .borrow()
                        .mechanical()
                        .map_err(|e| OrderTransportError {
                            code: e,
                            outcome: "unknown".into(),
                            may_have_executed: true,
                            retry_same_request_only: false,
                            retry_after_ms: 0.0,
                            status: None,
                            api_code: None,
                            semantic_mismatch: false,
                        })
                        .and_then(|v| {
                            same(l, &v).map_err(|e| OrderTransportError {
                                code: e,
                                outcome: "unknown".into(),
                                may_have_executed: true,
                                retry_same_request_only: false,
                                retry_after_ms: 0.0,
                                status: None,
                                api_code: None,
                                semantic_mismatch: false,
                            })
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
    Ok(BaalNeutralOwnedCounterpartPilotV2 { inner, transport })
}

impl BaalNeutralOwnedCounterpartPilotV2 {
    pub fn active_intent_status(&self) -> V {
        let g = self.inner.borrow();
        let o = active(&g.state.clone().unwrap_or(V::Null));
        o.map(|o| {
            json!({
                "intentId": get(&o,"intentId"),
                "firstExchangeId": get(&o,"firstExchangeId"),
                "orderId": get(&o,"orderId"),
                "action": get(get(&o,"body"),"action"),
                "marketId": get(&o,"marketId"),
                "exchangeId": get(get(&o,"body"),"exchangeId"),
                "body": get(&o,"body"),
                "bodyHash": get(&o,"bodyHash"),
                "phase": get(&o,"phase"),
                "createdAt": get(&o,"createdAt"),
                "packageLegs": get(get(&o,"package"),"legs"),
                "actualFetchCount": get(&o,"fetchStarts").as_array().map(|x|x.len()).unwrap_or(0),
                "mayHaveExecuted": get(&o,"mayHaveExecuted"),
                "cancelPending": false,
            })
        })
        .unwrap_or(V::Null)
    }
    pub fn status(&self) -> Result<V, String> {
        status_of(&self.inner)
    }
    /// onBeforeFetch(meta) -> {notAfter}
    pub fn on_before_fetch(&self, meta: &V) -> Result<V, String> {
        self.inner.borrow().mechanical()?;
        let (o, in_flight, deadline, now) = {
            let g = self.inner.borrow();
            (
                active(&g.state.clone().unwrap_or(V::Null)),
                g.in_flight.is_some(),
                g.fetch_deadline,
                (g.deps.now)(),
            )
        };
        let d = deadline.unwrap_or(f64::NAN);
        need(
            in_flight
                && o.is_some()
                && sget(meta, "method") == Some("POST")
                && sget(meta, "endpoint") == Some("/orders")
                && sget(meta, "requestBodyHash")
                    == sget(&o.unwrap_or(V::Null), "bodyHash")
                && d.is_finite()
                && d.fract() == 0.0
                && d.abs() <= 9007199254740991.0
                && now < d,
            "FINAL_FETCH_CLOCK_OR_BINDING",
        )?;
        Ok(json!({"notAfter": d}))
    }
    /// onFetchStart(meta) -> bool; journal append failure marks failed and throws.
    pub fn on_fetch_start(&self, meta: &V) -> Result<bool, String> {
        if sget(meta, "method") == Some("GET") || self.inner.borrow().in_flight.is_none() {
            return Ok(false);
        }
        let r: Result<bool, String> = (|| {
            self.inner.borrow().basic()?;
            let (o, f, spec) = {
                let g = self.inner.borrow();
                let st = g.state.clone().unwrap_or(V::Null);
                (
                    active(&st).unwrap_or(V::Null),
                    get(&active(&st).unwrap_or(V::Null), "fences")
                        .as_array()
                        .and_then(|x| x.last())
                        .cloned()
                        .unwrap_or(V::Null),
                    g.in_flight.as_ref().map(|i| i.spec.clone()).unwrap_or(V::Null),
                )
            };
            need(
                !f.is_null()
                    && sget(meta, "method") == Some("POST")
                    && sget(meta, "endpoint") == Some("/orders")
                    && sget(meta, "requestBodyHash") == sget(&o, "bodyHash")
                    && get(meta, "fetchInvocationId").as_str().is_some()
                    && get(&spec, "bodyJson") == get(&o, "bodyJson"),
                "BOUND_ACTUAL_FETCH",
            )?;
            let scope = {
                let g = self.inner.borrow();
                get(&g.state.clone().unwrap_or(V::Null), "scope").clone()
            };
            let mut start = meta.as_object().cloned().unwrap_or_default();
            start.insert("kind".into(), json!("place"));
            start.insert("ownerId".into(), get(&scope, "ownerId").clone());
            start.insert("runId".into(), get(&scope, "runId").clone());
            start.insert(
                "tournamentId".into(),
                get(&scope, "tournamentId").clone(),
            );
            start.insert("intentId".into(), get(&o, "intentId").clone());
            start.insert(
                "idempotencyKey".into(),
                get(get(&o, "body"), "idempotencyKey").clone(),
            );
            start.insert("requestBodyJson".into(), get(&o, "bodyJson").clone());
            start.insert("requestBodyHash".into(), get(&o, "bodyHash").clone());
            start.insert("leaseIdentity".into(), get(&f, "lease").clone());
            start.insert("leaseHash".into(), get(&f, "leaseHash").clone());
            append(
                &self.inner,
                &json!({"kind": "fetch_start", "meta": V::Object(start), "at": iso((self.inner.borrow().deps.now)())}),
            )?;
            Ok(true)
        })();
        match r {
            Ok(v) => Ok(v),
            Err(e) => {
                self.inner.borrow_mut().failed = true;
                Err(e)
            }
        }
    }
    /// authorizes(spec, beforeDispatch, beforeNetwork) -> bool
    pub fn authorizes(
        &self,
        spec: &V,
        before_dispatch: Option<&Guard<'_>>,
        before_network: Option<&Guard<'_>>,
    ) -> bool {
        let r: Result<bool, String> = (|| {
            self.inner.borrow().mechanical()?;
            let bd_ptr = before_dispatch
                .map(|x| x as *const Guard as *const () as usize)
                .unwrap_or(0);
            let bn_ptr = before_network
                .map(|x| x as *const Guard as *const () as usize)
                .unwrap_or(0);
            let g = self.inner.borrow();
            let st = g.state.clone().unwrap_or(V::Null);
            let p = g.issued.get(&bd_ptr).cloned();
            Ok(!pilot_unknown(&st)
                && p.is_some()
                && get(p.as_ref().unwrap(), "method") == get(spec, "method")
                && get(p.as_ref().unwrap(), "endpoint") == get(spec, "endpoint")
                && get(p.as_ref().unwrap(), "bodyJson") == get(spec, "bodyJson")
                && get(p.as_ref().unwrap(), "beforeNetwork") == &json!(bn_ptr)
                && self
                    .transport
                    .authorize_request(spec)
                    .map(|x| x)
                    .unwrap_or(false))
        })();
        r.unwrap_or(false)
    }
    /// journalSource()
    pub fn journal_source(&self) -> Result<V, String> {
        let g = self.inner.borrow();
        let bytes = fs::read(&g.file).map_err(|e| e.to_string())?;
        let nl = bytes.iter().position(|b| *b == 10).unwrap_or(0);
        let first = &bytes[..nl + 1];
        let row: V = serde_json::from_str(&String::from_utf8_lossy(first))
            .map_err(|e| e.to_string())?;
        let st = g.state.clone().unwrap_or(V::Null);
        Ok(json!({
            "journalFile": g.file.to_string_lossy(),
            "record": funding(&st)?,
            "sequence": g.journal.sequence,
            "hash": g.journal.previous,
            "rootPrefix": {
                "bytes": first.len(),
                "sha256": sha_bytes(first),
                "sequence": get(&row,"sequence"),
                "hash": get(&row,"hash"),
            },
            "headPrefix": {
                "bytes": g.size,
                "sha256": g.bytes_hash,
                "sequence": g.journal.sequence,
                "hash": g.journal.previous,
            },
            "status": status_of(&self.inner)?,
        }))
    }
    /// abandonPrepared()
    pub fn abandon_prepared(&self) -> Result<V, String> {
        run(&self.inner, || {
            self.inner.borrow().basic()?;
            need(
                self.inner.borrow().in_flight.is_none(),
                "ABANDON_IN_FLIGHT",
            )?;
            append(
                &self.inner,
                &json!({"kind": "abandoned_prepared", "at": iso((self.inner.borrow().deps.now)())}),
            )?;
            reserve(&self.inner)?;
            self.status()
        })
    }
    /// inventory(ex)
    pub fn inventory(&self, ex: &str) -> V {
        let g = self.inner.borrow();
        get(get(&g.state.clone().unwrap_or(V::Null), "currentInventory"), ex).clone()
    }
    /// orderBinding(id)
    pub fn order_binding(&self, id: &str) -> V {
        let g = self.inner.borrow();
        let st = g.state.clone().unwrap_or(V::Null);
        let o = active(&st).unwrap_or(V::Null);
        if get(&o, "orderId").as_str() != Some(id) {
            return V::Null;
        }
        let mut out = self.active_intent_status().as_object().cloned().unwrap_or_default();
        if let Some(m) = get(&st, "scope").as_object() {
            for (k, v) in m {
                out.insert(k.clone(), v.clone());
            }
        }
        out.insert(
            "idempotencyKey".into(),
            get(get(&o, "body"), "idempotencyKey").clone(),
        );
        out.insert("side".into(), json!("no"));
        out.insert("quantity".into(), get(get(&o, "body"), "quantity").clone());
        out.insert(
            "limit".into(),
            json!(money(price(&num_str(get(get(&o, "body"), "price"))).unwrap_or(0))),
        );
        out.insert(
            "expirationDate".into(),
            get(get(&o, "body"), "expirationDate").clone(),
        );
        V::Object(out)
    }
    /// prepareSell(ex,{requestedQuantity=null})
    pub fn prepare_sell(&self, ex: &str, requested_quantity: Option<i64>) -> Result<V, String> {
        run(&self.inner, || {
            self.inner.borrow().basic()?;
            need(
                active(&self.inner.borrow().state.clone().unwrap_or(V::Null)).is_none(),
                "UNKNOWN_OR_ACTIVE",
            )?;
            let mut captured = evidence(&self.inner, "prepare_sell", Some(ex), None)?;
            captured.as_object_mut().map(|m| {
                m.insert(
                    "requestedQuantity".into(),
                    requested_quantity.map(|q| json!(q)).unwrap_or(V::Null),
                )
            });
            ensure_installed(&self.inner)?;
            let g = (self.inner.borrow().deps.get_global_risk)()?;
            append(
                &self.inner,
                &json!({
                    "kind": "prepared_sell",
                    "exchangeId": ex,
                    "evidence": captured,
                    "globalRisk": g,
                    "intentId": format!("neutral-owned-sell-{}", uuid::Uuid::new_v4()),
                    "key": uuid::Uuid::new_v4().to_string(),
                    "at": iso((self.inner.borrow().deps.now)()),
                }),
            )?;
            reserve(&self.inner)?;
            self.status()
        })
    }
    /// pairCensus(firstEx)
    pub fn pair_census(&self, first_ex: &str) -> Result<V, String> {
        self.inner.borrow().basic()?;
        let (st, now) = {
            let g = self.inner.borrow();
            (g.state.clone().unwrap_or(V::Null), (g.deps.now)())
        };
        Ok(pair_census(&st, first_ex, &iso(now)))
    }
    /// fundingReservation()
    pub fn funding_reservation(&self) -> V {
        funding(&self.inner.borrow().state.clone().unwrap_or(V::Null)).unwrap_or(V::Null)
    }
    /// fundingReservationStatus()
    pub fn funding_reservation_status(&self) -> V {
        let g = self.inner.borrow();
        let st = g.state.clone().unwrap_or(V::Null);
        json!({
            "record": funding(&st).unwrap_or(V::Null),
            "receipt": get(&st, "reservation"),
        })
    }
    /// refreshReservation()
    pub fn refresh_reservation(&self) -> Result<V, String> {
        run(&self.inner, || {
            self.inner.borrow().basic()?;
            reserve(&self.inner)?;
            self.status()
        })
    }
    /// prepareHedge(firstEx)
    pub fn prepare_hedge(&self, first_ex: &str) -> Result<V, String> {
        run(&self.inner, || {
            self.inner.borrow().basic()?;
            {
                let g = self.inner.borrow();
                let st = g.state.clone().unwrap_or(V::Null);
                need(
                    active(&st).is_none()
                        && get(&st, "orders")
                            .as_array()
                            .map(|os| {
                                os.iter()
                                    .filter(|o| sget(get(o, "body"), "action") == Some("buy"))
                                    .count()
                                    == 0
                            })
                            .unwrap_or(false),
                    "ONE_ORDER_PILOT",
                )?;
            }
            let captured = evidence(&self.inner, "prepare_hedge", Some(first_ex), None)?;
            ensure_installed(&self.inner)?;
            let g = (self.inner.borrow().deps.get_global_risk)()?;
            append(
                &self.inner,
                &json!({
                    "kind": "prepared",
                    "evidence": captured,
                    "globalRisk": g,
                    "intentId": format!("neutral-owned-hedge-{}", uuid::Uuid::new_v4()),
                    "key": uuid::Uuid::new_v4().to_string(),
                    "at": iso((self.inner.borrow().deps.now)()),
                }),
            )?;
            reserve(&self.inner)?;
            self.status()
        })
    }
    /// dispatchPrepared()
    pub fn dispatch_prepared(&self) -> Result<V, String> {
        run(&self.inner, || {
            self.inner.borrow().basic()?;
            let o = {
                let g = self.inner.borrow();
                active(&g.state.clone().unwrap_or(V::Null)).unwrap_or(V::Null)
            };
            need(
                sget(&o, "phase") == Some("prepared")
                    && get(&o, "fetchStarts")
                        .as_array()
                        .map(|x| x.is_empty())
                        .unwrap_or(false),
                "UNKNOWN_NEVER_RETRY",
            )?;
            ensure_installed(&self.inner)?;
            let l = self.inner.borrow().basic()?;
            let result = self.transport.place(
                get(&o, "body"),
                &json!({
                    "permit": {"intentId": get(&o,"intentId"), "bodyHash": get(&o,"bodyHash")},
                    "lease": l,
                }),
            );
            if let Err(e) = result {
                if !self.inner.borrow().failed {
                    append(
                        &self.inner,
                        &json!({
                            "kind": "error",
                            "mayHaveExecuted": e.may_have_executed,
                            "at": iso((self.inner.borrow().deps.now)()),
                        }),
                    )?;
                    let st = self.inner.borrow().state.clone().unwrap_or(V::Null);
                    let o2 = active(&st).unwrap_or(V::Null);
                    if pilot_unknown(&st) {
                        (self.inner.borrow().deps.on_financial_halt)(&json!({
                            "kind": "uncertain_hedge",
                            "intentId": get(&o2, "intentId"),
                            "allWritersStop": true,
                        }))?;
                    }
                }
                return Err(e.code);
            }
            self.status()
        })
    }
    /// reconcile(evidence)
    pub fn reconcile(&self, e: &V) -> Result<V, String> {
        run(&self.inner, || {
            self.inner.borrow().basic()?;
            let g = (self.inner.borrow().deps.get_global_risk)()?;
            let mut ev = e.as_object().cloned().unwrap_or_default();
            ev.insert("globalRisk".into(), g);
            let r = append(
                &self.inner,
                &json!({
                    "kind": "reconciled",
                    "evidence": V::Object(ev),
                    "at": iso((self.inner.borrow().deps.now)()),
                }),
            );
            if let Err(error) = r {
                if !self.inner.borrow().failed
                    && regex_non_neutral_post_account(&error)
                {
                    let _ = append(
                        &self.inner,
                        &json!({
                            "kind": "financial_mismatch",
                            "reason": "NON_NEUTRAL_POST_ACCOUNT",
                            "at": iso((self.inner.borrow().deps.now)()),
                        }),
                    );
                    let _ = (self.inner.borrow().deps.on_financial_halt)(&json!({
                        "kind": "non_neutral_post_account",
                        "allWritersStop": true,
                    }));
                }
                return Err(error);
            }
            reserve(&self.inner)?;
            self.status()
        })
    }
    /// exportHandbackEvidence()
    pub fn export_handback_evidence(&self) -> Result<V, String> {
        self.inner.borrow().basic()?;
        let g = self.inner.borrow();
        let st = g.state.clone().unwrap_or(V::Null);
        need(
            active(&st).is_none() && !pilot_unknown(&st),
            "HANDOFF_NOT_TERMINAL",
        )?;
        let head = self.journal_source()?;
        let mut body = Map::new();
        body.insert("schemaVersion".into(), json!(1));
        body.insert(
            "kind".into(),
            json!("neutral_owned_hedge_explicit_handback_evidence_v2"),
        );
        if let Some(m) = get(&st, "scope").as_object() {
            for (k, v) in m {
                body.insert(k.clone(), v.clone());
            }
        }
        body.insert("seedHash".into(), json!(g.seed_hash));
        body.insert("migration".into(), g.migration.clone());
        body.insert(
            "legacyAnchor".into(),
            get(&g.seed, "anchor").clone(),
        );
        body.insert(
            "coordinatorAnchor".into(),
            get(&head, "headPrefix").clone(),
        );
        body.insert(
            "originalInventory".into(),
            get(&st, "originalInventory").clone(),
        );
        body.insert(
            "currentInventory".into(),
            get(&st, "currentInventory").clone(),
        );
        body.insert(
            "pairRetirements".into(),
            get(&st, "pairRetirements").clone(),
        );
        body.insert(
            "counterInventory".into(),
            get(&st, "counterInventory").clone(),
        );
        body.insert("assignments".into(), get(&st, "assignments").clone());
        body.insert("funding".into(), funding(&st)?);
        body.insert(
            "requiresExplicitNewOwnerImplementation".into(),
            json!(true),
        );
        let mut out = V::Object(body);
        let h = payload_hash(&out)?;
        out.as_object_mut()
            .unwrap()
            .insert("proofHash".into(), json!(h));
        Ok(out)
    }
    /// close()
    pub fn close(&self) -> Result<(), String> {
        need(!self.inner.borrow().busy, "BUSY")?;
        {
            let mut g = self.inner.borrow_mut();
            if g.closed {
                return Ok(());
            }
            g.closed = true;
            g.lease.release().map_err(|e| e.message)?;
        }
        Ok(())
    }
}

fn regex_non_neutral_post_account(message: &str) -> bool {
    message == "HEDGE_COORDINATOR_NON_NEUTRAL_graphReceipt"
        || message == "HEDGE_COORDINATOR_NON_NEUTRAL_collateralReceipt"
}
