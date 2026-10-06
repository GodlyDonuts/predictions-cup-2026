// execution/owned-no-hedge-plan-v1.mjs — source-bound proposal only. Never
// renews an original plan or grants a write. Errors collapse to a reason like
// the single-owned signal module.
use crate::core::payload_hash;
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy, sha256};
use baal_service_runtime::market::normalize_book;
use serde_json::{json, Map, Value as V};
use std::collections::BTreeMap;
use std::collections::BTreeSet;

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("OWNED_NO_HEDGE_{c}")) }
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
        !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0 && n <= 9_007_199_254_740_991)
    })
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn price_v(v: &V) -> Result<i128, String> {
    price(v.as_str().unwrap_or(""))
}
// String(x) for price-string coercion of numeric raw prices.
fn js_string(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        _ => js_stringify(v),
    }
}

struct Raw {
    data: V,
    numbers: BTreeMap<String, String>,
    source_hash: String,
    start: i64,
    end: i64,
}

fn raw(r: &V, endpoint: &str, at: i64, age: i64) -> Result<Raw, String> {
    need(
        get(r, "method").as_str() == Some("GET")
            && get(r, "status") == &json!(200)
            && get(r, "endpoint").as_str() == Some(endpoint)
            && get(r, "bodyText").as_str().is_some_and(|s| s.len() <= 8 * 1024 * 1024),
        "RAW_RECEIPT",
    )?;
    let start = instant_v(get(r, "requestStartedAt"))?;
    let end = instant_v(get(r, "receivedAt"))?;
    need(
        start <= end && end <= at && at - start <= age && end - start <= 20_000,
        "SOURCE_CLOCK",
    )?;
    let source_hash = sha256(get(r, "bodyText").as_str().unwrap().as_bytes());
    if let Some(h) = r.get("sourceBodyHash") {
        need(h.as_str() == Some(source_hash.as_str()), "SOURCE_HASH")?;
    }
    let (data, numbers) = parse_number_lexemes(get(r, "bodyText").as_str().unwrap())?;
    Ok(Raw { data, numbers, source_hash, start, end })
}

// planOwnedNoHedge({buyProof,originalAfter,originalPlan,account,rootReceipts,otherBookReceipt,globalRisk,reservedElsewhere,tournamentSlug,now})
pub fn plan_owned_no_hedge(input: &V) -> V {
    let buy_proof = get(input, "buyProof");
    let original_after = get(input, "originalAfter");
    let original_plan = get(input, "originalPlan");
    let account = get(input, "account");
    let root_receipts = get(input, "rootReceipts");
    let other_book_receipt = get(input, "otherBookReceipt");
    let global_risk = get(input, "globalRisk");
    let default_zero = json!("0.000000");
    let reserved_elsewhere = input.get("reservedElsewhere").unwrap_or(&default_zero);
    let default_slug = json!("midterm-elections");
    let tournament_slug = input.get("tournamentSlug").unwrap_or(&default_slug);
    let now = get(input, "now");
    let base = json!({
        "schemaVersion": 1,
        "mode": "fresh_owned_no_conditional_hedge_proposal_v1",
        "createdAt": now,
        "proposalReady": false,
        "orderEligible": false,
        "executionAuthority": false,
        "newBoundedAuthorizationRequired": true,
        "originalReviewRenewed": false,
        "networkRequests": 0,
        "competitionOrders": 0,
        "profitabilityEstablished": false,
    });
    let inner = (|| -> Result<V, String> {
        let at = instant_v(now)?;
        let p = buy_proof;
        let mut proof_body = p.clone();
        let verification_hash = proof_body
            .as_object_mut()
            .and_then(|m| m.remove("verificationHash"));
        need(
            verification_hash.as_ref().is_some_and(|h| hex(h))
                && payload_hash(&proof_body)?
                    == verification_hash.as_ref().and_then(|h| h.as_str()).unwrap_or("")
                && get(p, "mode") == &json!("scoped_rest_no_lot_with_rounded_cash_neutral_all_v2")
                && get(p, "scopedLotVerified") == &json!(true)
                && get(p, "filledQuantity") == &json!(1)
                && get(p, "signedPositionDelta") == &json!(-1)
                && get(p, "ownerId") == &json!("Codex")
                && id(get(p, "marketId"))
                && id(get(p, "exchangeId"))
                && id(get(p, "orderId")),
            "EXACT_TERMINAL_BUY",
        )?;
        let lot = get(p, "lot");
        let f = get(p, "fills")[0].clone();
        let cost = price_v(get(p, "scopedPrincipalCost"))?;
        need(
            get(lot, "quantity") == &json!(1)
                && get(lot, "side").as_str() == Some("no")
                && get(lot, "orderId") == get(p, "orderId")
                && get(lot, "ownerId") == get(p, "ownerId")
                && get(lot, "runId") == get(p, "runId")
                && get(lot, "intentId") == get(p, "intentId")
                && get(p, "fills").as_array().is_some_and(|a| a.len() == 1)
                && price_v(get(lot, "entryPrice"))? == cost
                && get(&f, "quantity") == &json!(1)
                && get(&f, "signedQuantity") == &json!(-1)
                && get(&f, "side").as_str() == Some("no")
                && get(&f, "priceBasis").as_str() == Some("side_relative")
                && get(&f, "source").as_str() == Some("rest_order_fills")
                && get(&f, "orderId") == get(p, "orderId")
                && get(&f, "marketId") == get(p, "marketId")
                && get(&f, "exchangeId") == get(p, "exchangeId")
                && get(&f, "tournamentId") == get(p, "tournamentId")
                && price_v(get(&f, "price"))? == cost
                && get(&f, "filledAt") == get(lot, "openedAt")
                && instant_v(get(lot, "openedAt"))? <= instant_v(get(p, "readAt"))?,
            "ORIGINAL_OWNED_BASIS",
        )?;
        let mut plan_body = original_plan.clone();
        let plan_hash = plan_body.as_object_mut().and_then(|m| m.remove("planHash"));
        need(
            plan_hash.as_ref().is_some_and(|h| hex(h))
                && payload_hash(&plan_body)? == plan_hash.as_ref().and_then(|h| h.as_str()).unwrap_or("")
                && matches!(
                    get(original_plan, "mode").as_str(),
                    Some("paired_rest_no_fixed_margin_v2" | "paired_no_dry_run_v1")
                )
                && get(get(original_plan, "scope"), "ownerId") == get(p, "ownerId")
                && get(get(original_plan, "scope"), "runId") == get(p, "runId")
                && get(get(original_plan, "scope"), "tournamentId") == get(p, "tournamentId")
                && get(get(original_plan, "scope"), "tournamentSlug") == tournament_slug
                && get(original_plan, "executionAuthority") == &json!(false)
                && get(original_plan, "legs").as_array().is_some_and(|a| a.len() == 2)
                && instant_v(get(original_plan, "createdAt"))? <= instant_v(get(p, "readAt"))?
                && instant_v(get(original_plan, "reviewExpiresAt"))? > instant_v(get(original_plan, "createdAt"))?,
            "ORIGINAL_PLAN",
        )?;
        let first = get(original_plan, "legs").as_array().unwrap().iter().find(|l| {
            get(l, "marketId") == get(p, "marketId") && get(l, "exchangeId") == get(p, "exchangeId")
        }).cloned().unwrap_or(V::Null);
        let other = get(original_plan, "legs").as_array().unwrap().iter().find(|l| {
            get(l, "marketId") != get(p, "marketId") && get(l, "exchangeId") != get(p, "exchangeId")
        }).cloned().unwrap_or(V::Null);
        need(
            !first.is_null()
                && !other.is_null()
                && id(get(&other, "marketId"))
                && id(get(&other, "exchangeId"))
                && price_v(get(&first, "originalLimit"))? >= cost
                && price_v(get(p, "requestedLimit"))? == price_v(get(&first, "originalLimit"))?,
            "ORIGINAL_FIRST_LEG_BINDING",
        )?;
        need(
            get(global_risk, "ownerId") == get(p, "ownerId")
                && get(global_risk, "runId") == get(p, "runId")
                && get(global_risk, "tournamentId") == get(p, "tournamentId")
                && get(global_risk, "complete") == &json!(true)
                && get(global_risk, "soleCollector") == &json!(true)
                && get(global_risk, "stopped") == &json!(false)
                && hex(get(global_risk, "sourceHash"))
                && get(global_risk, "unresolvedIntentIds").as_array().is_some_and(|a| a.is_empty())
                && instant_v(get(global_risk, "at"))? <= at
                && at - instant_v(get(global_risk, "at"))? <= 20_000,
            "UNKNOWN_OR_PENDING",
        )?;
        let mut binding = p.as_object().cloned().unwrap_or_default();
        binding.insert("tournamentSlug".into(), tournament_slug.clone());
        let binding = V::Object(binding);
        let old = crate::no_rest_reconciliation_v2::inspect_scoped_rest_no_account(&json!({
            "account": original_after,
            "binding": binding,
            "readAt": get(p, "readAt"),
        }))?;
        let receipt_triplet = |v: &V| -> V {
            json!({
                "endpoint": get(v, "endpoint"),
                "sourceBodyHash": get(v, "sourceBodyHash"),
                "receivedAt": get(v, "receivedAt"),
            })
        };
        need(
            payload_hash(&V::Array(
                get(&old, "sourceReceipts").as_array().cloned().unwrap_or_default().iter().map(&receipt_triplet).collect(),
            ))? == payload_hash(&V::Array(
                get(get(p, "sourceReceipts"), "after").as_array().cloned().unwrap_or_default().iter().map(&receipt_triplet).collect(),
            ))?,
            "ORIGINAL_AFTER_PROVENANCE",
        )?;
        let cap: i128 = 990_000 - cost;
        price(&money(cap))?;
        let current = crate::no_rest_reconciliation_v2::inspect_scoped_rest_no_account(&json!({
            "account": account,
            "binding": binding,
            "readAt": now,
            "requiredFunding": money(cap),
            "reservedElsewhere": reserved_elsewhere,
        }))?;
        let sorted = |v: &V| -> Vec<V> {
            let mut xs = v.as_array().cloned().unwrap_or_default();
            xs.sort_by(|a, b| {
                js_int(get(a, "id")).unwrap_or(0).cmp(&js_int(get(b, "id")).unwrap_or(0))
            });
            xs
        };
        need(
            get(&current, "funded") == &json!(true)
                && instant_v(get(&old, "completedAt"))? <= instant_v(get(&current, "startedAt"))?
                && instant_v(get(p, "readAt"))? <= instant_v(get(&current, "startedAt"))?
                && payload_hash(&V::Array(sorted(get(&current, "scopedOrders"))))?
                    == payload_hash(&V::Array(sorted(get(&old, "scopedOrders"))))?,
            "CURRENT_FUNDING_OR_SCOPED_ACTIVITY",
        )?;
        need(
            get(&current, "positions").as_array().is_some_and(|a| {
                a.iter().any(|v| {
                    get(v, "exchangeId") == get(p, "exchangeId")
                        && get(v, "marketId") == get(p, "marketId")
                        && get(v, "quantity") == &json!(-1)
                        && get(v, "settled") == &json!(false)
                }) && !a.iter().any(|v| {
                    get(v, "exchangeId") == get(&other, "exchangeId") && get(v, "quantity") != &json!(0)
                })
            }),
            "EXACT_OWNED_AND_UNOWNED_OTHER",
        )?;
        need(
            instant_v(get(global_risk, "at"))? >= instant_v(get(&current, "completedAt"))?
                && root_receipts.as_array().is_some_and(|a| a.len() == 2),
            "CURRENT_RISK_OR_ROOTS",
        )?;
        let members: Vec<V> = [&first, &other]
            .iter()
            .map(|l| {
                json!({
                    "marketId": get(l, "marketId"),
                    "exchangeId": get(l, "exchangeId"),
                    "option": "YES",
                })
            })
            .collect();
        let mut roots: Vec<V> = Vec::new();
        for c in root_receipts.as_array().unwrap() {
            let m = members.iter().find(|v| {
                get(v, "marketId") == get(c, "marketId") && get(v, "exchangeId") == get(c, "exchangeId")
            });
            need(m.is_some(), "ROOT_SCOPE")?;
            let m = m.unwrap();
            let v = raw(
                get(c, "receipt"),
                &format!(
                    "/markets/{}/nodes?tournamentId={}",
                    get(c, "marketId").as_str().unwrap_or(""),
                    get(p, "tournamentId").as_str().unwrap_or("")
                ),
                at,
                300_000,
            )?;
            let d = &v.data;
            need(
                get(d, "market_id") == get(c, "marketId")
                    && get(d, "contexts").as_array().is_some_and(|x| {
                        x.len() == 1
                            && get(&x[0], "type") == &json!("tournament")
                            && get(&get(&x[0], "tournament"), "id") == get(p, "tournamentId")
                    }),
                "ROOT_RESPONSE_SCOPE",
            )?;
            roots.push(json!({
                "marketId": get(m, "marketId"),
                "exchangeId": get(m, "exchangeId"),
                "option": "YES",
                "tournamentId": get(p, "tournamentId"),
                "root": get(d, "root"),
                "rootHash": payload_hash(get(d, "root"))?,
                "sourceHash": v.source_hash,
                "requestStartedAt": get(get(c, "receipt"), "requestStartedAt"),
                "receivedAt": get(get(c, "receipt"), "receivedAt"),
                "knownAt": get(get(c, "receipt"), "receivedAt"),
            }));
        }
        need(
            roots.iter().map(|r| get(r, "marketId").as_str().unwrap_or("")).collect::<BTreeSet<_>>().len() == 2,
            "DUPLICATE_ROOT",
        )?;
        let semantic = crate::party_winner_no_screen_v1::screen_party_winner_no_pairs(&json!({
            "universe": {"tournamentId": get(p, "tournamentId"), "exchanges": members},
            "rootCaptures": roots,
            "quoteCaptures": [],
            "account": V::Null,
            "now": now,
        }))?;
        need(
            get(&semantic, "semanticPairCount") == &json!(1)
                && get(&get(&semantic, "pairs")[0], "rootsCurrent") == &json!(true),
            "SAME_SINGLE_WINNER_RACE",
        )?;
        let b = raw(
            other_book_receipt,
            &format!(
                "/markets/{}/orderbook?tournamentId={}&depth=20",
                get(&other, "marketId").as_str().unwrap_or(""),
                get(p, "tournamentId").as_str().unwrap_or("")
            ),
            at,
            20_000,
        )?;
        let d = &b.data;
        need(
            instant_v(get(&current, "completedAt"))? <= b.start
                && roots.iter().all(|r| instant_v(get(r, "receivedAt")).is_ok_and(|x| x <= b.start))
                && get(d, "contexts").as_array().is_some_and(|x| {
                    x.len() == 1
                        && get(&x[0], "type") == &json!("tournament")
                        && get(&get(&x[0], "tournament"), "id") == get(p, "tournamentId")
                }),
            "CAUSAL_OTHER_BOOK",
        )?;
        let book = normalize_book(
            d,
            get(&other, "marketId").as_str().unwrap_or(""),
            get(p, "tournamentId").as_str().unwrap_or(""),
            get(other_book_receipt, "receivedAt").as_str().unwrap_or(""),
            get(other_book_receipt, "requestStartedAt").as_str().unwrap_or(""),
            20_000.0,
            &BTreeSet::new(),
            "authoritative_receipt_v1",
        )?;
        need(
            get(&book, "exchangeId") == get(&other, "exchangeId")
                && get(&book, "bids").as_array().is_some_and(|a| a.len() <= 20)
                && get(&book, "asks").as_array().is_some_and(|a| a.len() <= 20),
            "EXACT_OTHER_DEPTH20",
        )?;
        let level = get(&book, "bids").as_array().unwrap().iter().find(|l| {
            js_int(get(l, "quantity")).is_some()
                && (js_int(get(l, "quantity")).unwrap() as f64 / 100.0).floor() >= 1.0
                && price_v(get(l, "price")).is_ok_and(|p| SCALE - p + TICK <= cap)
        }).cloned();
        need(level.is_some(), "PROTECTED_DEPTH_OR_MARGIN")?;
        let level = level.unwrap();
        let e = get(&get(&get(d, "contexts")[0], "orderbook"), "exchanges")[0].clone();
        let idx = get(&e, "bids").as_array().unwrap().iter().position(|l| {
            price(&js_string(get(l, "price"))).unwrap_or(-1)
                == price_v(get(&level, "price")).unwrap_or(-2)
        });
        let idx = idx.unwrap_or(usize::MAX);
        let key = format!(
            "[\"contexts\",0,\"orderbook\",\"exchanges\",0,\"bids\",{},\"quantity\"]",
            idx
        );
        let lex = b.numbers.get(&key);
        need(
            lex.is_some()
                && {
                    let l = lex.unwrap();
                    let mut parts = l.splitn(2, '.');
                    let whole = parts.next().unwrap_or("");
                    let frac = parts.next();
                    (whole == "0" || (!whole.is_empty() && !whole.starts_with('0')))
                        && whole.bytes().all(|b| b.is_ascii_digit())
                        && frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|x| x == b'0'))
                        && l.split('.').next().unwrap_or("").parse::<i128>().is_ok_and(|x| x >= 100)
                },
            "EXACT_ONE_PERCENT_DEPTH",
        )?;
        let expires_at = baal_service_runtime::contract::ms_to_iso(
            [
                roots.iter().map(|r| instant_v(get(r, "receivedAt")).unwrap_or(i64::MAX) + 300_000).min().unwrap_or(i64::MAX),
                b.start + 20_000,
                instant_v(get(&current, "startedAt"))? + 20_000,
            ]
            .iter()
            .copied()
            .min()
            .unwrap() as f64,
        );
        need(instant(&expires_at)? > at, "FRESH_PROPOSAL_WINDOW")?;
        let mut proposal = base.as_object().unwrap().clone();
        let root_sources: Vec<V> = roots
            .iter()
            .map(|r| {
                let mut m = serde_json::Map::new();
                for k in ["marketId", "exchangeId", "rootHash", "sourceHash", "requestStartedAt", "receivedAt"] {
                    m.insert(k.to_string(), get(r, k).clone());
                }
                V::Object(m)
            })
            .collect();
        let extra: Vec<(&str, V)> = vec![
            ("proposalReady", json!(true)),
            ("reason", json!("FRESH_CONDITIONAL_OWNED_RISK_HEDGE")),
            ("ownerId", get(p, "ownerId").clone()),
            ("runId", get(p, "runId").clone()),
            ("tournamentId", get(p, "tournamentId").clone()),
            ("tournamentSlug", tournament_slug.clone()),
            ("originalPlanHash", plan_hash.clone().unwrap_or(V::Null)),
            ("originalReviewExpiresAt", get(original_plan, "reviewExpiresAt").clone()),
            ("originalWindowExpired", json!(at >= instant_v(get(original_plan, "reviewExpiresAt"))?)),
            ("originalBuyProofHash", verification_hash.clone().unwrap_or(V::Null)),
            ("originalLotId", get(lot, "lotId").clone()),
            ("originalOrderId", get(p, "orderId").clone()),
            ("ownedMarketId", get(p, "marketId").clone()),
            ("ownedExchangeId", get(p, "exchangeId").clone()),
            ("actualOwnedPrincipal", json!(money(cost))),
            ("marketId", get(&other, "marketId").clone()),
            ("exchangeId", get(&other, "exchangeId").clone()),
            ("side", json!("no")),
            ("action", json!("buy")),
            ("quantity", json!(1)),
            ("fixedSecondLimit", json!(money(cap))),
            ("conditionalPairCostCeiling", json!("0.990000")),
            ("conditionalOrdinaryMinimumPayout", json!("1.000000")),
            ("minimumOrdinaryMargin", json!("0.010000")),
            ("conditionalOrdinarySurplusAtLimit", json!("0.010000")),
            ("administrativeZeroPayoutLossAtLimit", json!("-0.990000")),
            ("unconditionalPayoutFloor", V::Null),
            ("guaranteedProfit", json!(false)),
            ("fees", json!("0.000000")),
            ("rawObservedNoAsk", json!(money(SCALE - price_v(get(&level, "price"))?))),
            ("observedProtectiveNoAsk", json!(money(SCALE - price_v(get(&level, "price"))? + TICK))),
            (
                "depthQualifiedQuantity",
                json!((js_int(get(&level, "quantity")).unwrap() as f64 / 100.0).floor()),
            ),
            ("yesBidSourceLevel", level.clone()),
            (
                "otherBook",
                V::Object(serde_json::Map::from_iter([
                    ("sourceBodyHash".into(), json!(b.source_hash)),
                    ("requestStartedAt".into(), get(other_book_receipt, "requestStartedAt").clone()),
                    ("receivedAt".into(), get(other_book_receipt, "receivedAt").clone()),
                    ("sequence".into(), get(&book, "sequence").clone()),
                    ("sourceAt".into(), get(&book, "sourceAt").clone()),
                    ("versionKey".into(), get(&book, "versionKey").clone()),
                    (
                        "depthHash".into(),
                        json!(payload_hash(&V::Array(vec![get(&book, "bids").clone(), get(&book, "asks").clone()]))?),
                    ),
                ])),
            ),
            ("rootSources", V::Array(root_sources)),
            ("semanticEventKey", get(&get(&semantic, "pairs")[0], "eventKey").clone()),
            ("currentAccountHash", json!(payload_hash(get(&current, "sourceReceipts"))?)),
            ("globalRiskSourceHash", get(global_risk, "sourceHash").clone()),
            ("expiresAt", json!(expires_at)),
            ("priceBasis", json!("same_exchange_yes_bid_complement_side_relative_no")),
            (
                "assumptions",
                V::Array(vec![
                    json!("Normal same-race opposing Party Winner outcomes are mutually exclusive; third-party wins may pay both NO shares."),
                    json!("Void/refund/administrative or inconsistent contract resolution has no unconditional payout floor."),
                    json!("The second purchase is non-atomic; original owned inventory remains exposed until its separate terminal proof."),
                    json!("A fresh bounded authorization and complete dispatch-time reconciliation are required; this proposal never renews the old window."),
                ]),
            ),
        ];
        for (k, v) in extra {
            proposal.insert(k.to_string(), v);
        }
        let h = payload_hash(&V::Object(proposal.clone()))?;
        proposal.insert("proposalHash".into(), json!(h));
        Ok(V::Object(proposal))
    })();
    match inner {
        Ok(v) => v,
        Err(e) => {
            let owned = e.len() > "OWNED_NO_HEDGE_".len()
                && e.starts_with("OWNED_NO_HEDGE_")
                && e["OWNED_NO_HEDGE_".len()..].bytes().all(|b| b.is_ascii_uppercase() || b == b'_');
            let mut out = base.as_object().unwrap().clone();
            out.insert("reason".into(), json!(if owned { e } else { "INVALID_OR_INCOMPLETE_HEDGE_EVIDENCE".to_string() }));
            out.insert("ownedInventoryRetained".into(), json!(true));
            V::Object(out)
        }
    }
}
