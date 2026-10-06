// live-paper/canonical-entry-exit-study.mjs — conditional capacity entry/exit diagnostic.
use baal_execution::core::payload_hash;
use baal_execution::gates::normalize_official_fill_tick;
use baal_paper::fixed::{big_amount, money, SCALE, TICK};
use baal_service_runtime::contract::instant_ns;
use num_bigint::BigInt;
use serde_json::{json, Value as V};

fn check(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("CAPACITY_PATH_{}", code)); } Ok(()) }
fn instant(s: &V) -> i64 {
    baal_service_runtime::contract::parse_iso_ms(s.as_str().unwrap_or("")).map(|m| m as i64).unwrap_or(i64::MIN)
}
pub fn capacity_path_policy() -> V {
    json!({"schemaVersion":1,"maximumFrameAgeMs":20000,"maximumWindowMs":10000,"depthFractionBps":100,
        "exitAdverseTicks":1,"latencyMs":3000,"restLifetimeMs":120000,"strictThroughTicks":1,
        "makerQuantity":1,"diagnosticLookaheadMs":3600000,"feePerShare":"0.000000"})
}
/// tapeTick(trade) — documented tick/complement encodings only.
pub fn tape_tick(trade: &V) -> Result<Option<V>, String> {
    if trade["yesPrice"].is_null() { return Ok(None); }
    check(trade["yesPrice"].as_str().is_some_and(|s| {
        s.parse::<f64>().map(|n| format!("{}", n) == s).unwrap_or(false)
    }), "RAW_TRADE_PRICE")?;
    let n: f64 = trade["yesPrice"].as_str().unwrap().parse().map_err(|_| "CAPACITY_PATH_RAW_TRADE_PRICE".to_string())?;
    match normalize_official_fill_tick(&json!(n)) { Ok(v) => Ok(Some(v)), Err(_) => Ok(None) }
}
fn usable(f: &V, p: &V) -> bool {
    let now = instant(&f["at"]);
    let books = f["books"].as_array().cloned().unwrap_or_default();
    if books.is_empty() { return false; }
    let starts: Vec<i64> = books.iter().map(|b| instant(&b["requestStartedAt"])).collect();
    let receipts: Vec<i64> = books.iter().map(|b| instant(&b["receivedAt"])).collect();
    books.iter().all(|b| now >= instant(&b["publishedAt"])
        && now - instant(&b["requestStartedAt"]) <= p["maximumFrameAgeMs"].as_i64().unwrap_or(0)
        && crate::probability_receipt::probability_account_gate(&f["accountEligibility"], b, f["at"].as_str().unwrap_or("")).is_none())
        && receipts.iter().max().unwrap() - starts.iter().min().unwrap() <= p["maximumWindowMs"].as_i64().unwrap_or(0)
}
fn no_exit(book: &V, quantity: &BigInt, p: &V) -> V {
    let mut remaining = quantity.clone();
    let mut proceeds = BigInt::from(0);
    let mut shares = BigInt::from(0);
    for l in book["asks"].as_array().cloned().unwrap_or_default() {
        let n = BigInt::from(l["quantity"].as_i64().unwrap_or(0)) * BigInt::from(p["depthFractionBps"].as_i64().unwrap_or(0)) / BigInt::from(10000);
        let take = n.clone().min(remaining.clone());
        let px = BigInt::from(SCALE) - big_amount(l["price"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0))
            - BigInt::from(p["exitAdverseTicks"].as_i64().unwrap_or(0)) * BigInt::from(TICK)
            - big_amount(p["feePerShare"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0));
        if px <= BigInt::from(0) { break; }
        proceeds += &take * &px;
        remaining -= &take;
        shares += &take;
        if remaining == BigInt::from(0) { break; }
    }
    json!({"quantity": shares.to_string(), "proceeds": money(&proceeds), "complete": remaining == BigInt::from(0)})
}
/// studyCapacityPaths({frames, trades, policy})
pub fn study_capacity_paths(frames: &[V], trades: &[V], policy: Option<&V>) -> Result<V, String> {
    let default_policy = capacity_path_policy();
    let policy = policy.unwrap_or(&default_policy);
    check(payload_hash(policy).ok() == payload_hash(&default_policy).ok(), "POLICY")?;
    let mut previous = i64::MIN;
    for f in frames {
        check(instant(&f["at"]) >= previous && f["books"].as_array().map(|b| b.len() == 2).unwrap_or(false), "FRAME_ORDER")?;
        previous = instant(&f["at"]);
        check(f["books"].as_array().unwrap().iter().enumerate()
            .all(|(i, b)| b["versionKey"] == f["versions"][i] && b["publicationSequence"].as_i64().unwrap_or(0) <= f["sourceSequence"].as_i64().unwrap_or(0)), "FRAME_BINDING")?;
    }
    let usable_frames: Vec<&V> = frames.iter().filter(|f| usable(f, policy)).collect();
    let anchors: Vec<&V> = usable_frames.iter().filter(|f| f["result"]["positiveConditionalCapacity"].as_bool().unwrap_or(false)).cloned().collect();
    let mut cases = Vec::new();
    for f in &anchors {
        let known = instant(&f["at"]);
        let quantity: BigInt = f["result"]["matchedQuantity"].as_str().unwrap_or("0").parse().unwrap_or(BigInt::from(0));
        let cost = big_amount(f["result"]["acquisitionCost"].as_str().unwrap_or(""))?;
        check(f["result"]["matchedQuantity"].as_str().is_some_and(|s| !s.is_empty() && s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit())), "QUANTITY")?;
        let fbooks = f["books"].as_array().unwrap();
        let later: Vec<&&V> = usable_frames.iter().filter(|x| {
            let x_at = instant(&x["at"]);
            x_at > known && x_at - known <= policy["diagnosticLookaheadMs"].as_i64().unwrap_or(0)
                && x["books"].as_array().unwrap().iter().enumerate().all(|(i, b)| {
                    b["marketId"] == fbooks[i]["marketId"] && b["exchangeId"] == fbooks[i]["exchangeId"]
                        && b["versionKey"] != f["versions"][i]
                        && instant(&b["requestStartedAt"]) >= known + policy["latencyMs"].as_i64().unwrap_or(0)
                })
        }).collect();
        let marks: Vec<V> = later.iter().map(|x| {
            let legs: Vec<V> = x["books"].as_array().unwrap().iter().map(|b| no_exit(b, &quantity, policy)).collect();
            let net: BigInt = legs.iter().map(|l| big_amount(l["proceeds"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0))).sum();
            let complete = legs.iter().all(|l| l["complete"].as_bool().unwrap_or(false));
            json!({"at": x["at"], "ageMs": instant(&x["at"]) - known, "complete": complete, "legs": legs,
                "netProceeds": money(&net),
                "hypotheticalPnl": if complete { json!(money(&(net.clone() - &cost))) } else { V::Null },
                "versions": x["versions"], "sourceSequence": x["sourceSequence"], "sourceHash": x["sourceHash"]})
        }).collect();
        let complete: Vec<&V> = marks.iter().filter(|m| m["complete"].as_bool().unwrap_or(false)).collect();
        let best: Option<V> = complete.iter().fold(None, |acc: Option<V>, m| {
            match acc {
                None => Some((*m).clone()),
                Some(a) => {
                    let am = big_amount(a["hypotheticalPnl"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0));
                    let bm = big_amount(m["hypotheticalPnl"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0));
                    if bm > am { Some((*m).clone()) } else { Some(a) }
                }
            }
        });
        let mut legs = Vec::new();
        for b in fbooks {
            let limit = big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))?;
            let activation_ns = (BigInt::from(known + policy["latencyMs"].as_i64().unwrap_or(0)) * BigInt::from(1_000_000))
                .max(instant_ns(b["sourceAt"].as_str().unwrap_or("")).map_err(|e| e.0)? + BigInt::from(2_000_000_000));
            let expiry_ns = BigInt::from(known + policy["restLifetimeMs"].as_i64().unwrap_or(0)) * BigInt::from(1_000_000);
            let eligible: Vec<&V> = trades.iter().filter(|t| {
                t["tournamentId"] == b["tournamentId"] && t["marketId"] == b["marketId"] && t["exchangeId"] == b["exchangeId"]
                    && instant(&t["firstAvailableAt"]) >= known
                    && instant_ns(t["createdAt"].as_str().unwrap_or("")).map(|t| BigInt::from(t) >= activation_ns && BigInt::from(t) < expiry_ns).unwrap_or(false)
            }).collect();
            let mut through: Vec<&&V> = eligible.iter().filter(|t| {
                tape_tick(t).ok().flatten().is_some_and(|p| {
                    big_amount(p["price"].as_str().unwrap_or("")).map(|x| x >= &limit + BigInt::from(policy["strictThroughTicks"].as_i64().unwrap_or(0)) * BigInt::from(TICK)).unwrap_or(false)
                })
            }).collect();
            through.sort_by(|a, b| {
                let an = instant_ns(a["createdAt"].as_str().unwrap_or("")).unwrap_or(0);
                let bn = instant_ns(b["createdAt"].as_str().unwrap_or("")).unwrap_or(0);
                an.cmp(&bn)
            });
            legs.push(json!({"marketId": b["marketId"], "exchangeId": b["exchangeId"],
                "yesSellLimit": money(&limit), "canonicalNoAcquisitionPrice": money(&(BigInt::from(SCALE) - &limit)),
                "activationAt": baal_service_runtime::contract::ms_to_iso((activation_ns.clone() / BigInt::from(1_000_000)).to_string().parse::<f64>().unwrap_or(0.0)),
                "expiresAt": baal_service_runtime::contract::ms_to_iso((known + policy["restLifetimeMs"].as_i64().unwrap_or(0)) as f64),
                "eligibleObservedTrades": eligible.len(), "strictThroughEvents": through.len(),
                "strictThroughShares": through.iter().map(|t| t["size"].as_i64().unwrap_or(0)).sum::<i64>(),
                "firstThrough": through.first().map(|t| json!({"id": t["id"], "key": t["key"], "createdAt": t["createdAt"],
                    "firstAvailableAt": t["firstAvailableAt"], "rawYesPrice": t["yesPrice"],
                    "normalizedPrice": tape_tick(t).ok().flatten().map(|p| p["price"].clone()).unwrap_or(V::Null)})).unwrap_or(V::Null),
                "aggressorSide": V::Null, "queuePositionProven": false, "fillProven": false}));
        }
        let leg_refs: Vec<&V> = legs.iter().collect();
        cases.push(json!({"anchorAt": f["at"], "sourceSequence": f["sourceSequence"], "sourceHash": f["sourceHash"],
            "versions": f["versions"], "quantity": f["result"]["matchedQuantity"],
            "acquisitionCost": money(&cost),
            "ordinaryConditionalSettlementSurplus": f["result"]["conditionalOrdinaryMinimumProfit"],
            "zeroPayoutStress": f["result"]["stressedMinimumProfit"],
            "subsequentCompleteExitObservations": complete.len(),
            "partialExitObservations": marks.len() - complete.len(),
            "bestLaterDepthMark": best.unwrap_or(V::Null),
            "lastLaterDepthMark": complete.last().map(|x| (*x).clone()).unwrap_or(V::Null),
            "rightCensored": later.is_empty(),
            "maker": {"quantity": policy["makerQuantity"],
                "canonicalNoQuotedPackageCost": money(&(leg_refs.iter().map(|l| big_amount(l["canonicalNoAcquisitionPrice"].as_str().unwrap_or("")).unwrap_or(BigInt::from(0))).sum::<BigInt>() * BigInt::from(policy["makerQuantity"].as_i64().unwrap_or(0)))),
                "bothLegsHaveStrictThrough": legs.iter().all(|l| l["strictThroughEvents"].as_i64().unwrap_or(0) > 0),
                "exactlyOneLegHasStrictThrough": legs.iter().filter(|l| l["strictThroughEvents"].as_i64().unwrap_or(0) > 0).count() == 1,
                "legs": legs}}));
    }
    Ok(json!({"schemaVersion":1,"mode":"conditional_capacity_entry_exit_development_diagnostic",
        "policy": policy, "policyHash": payload_hash(policy).unwrap_or_default(),
        "frames": frames.len(), "usableFrames": usable_frames.len(), "positiveAnchors": cases.len(),
        "anchorsWithLaterCompleteDepth": cases.iter().filter(|c| c["subsequentCompleteExitObservations"].as_i64().unwrap_or(0) > 0).count(),
        "anchorsWithAnyPositiveLaterDepthMark": cases.iter().filter(|c| !c["bestLaterDepthMark"].is_null()
            && big_amount(c["bestLaterDepthMark"]["hypotheticalPnl"].as_str().unwrap_or("")).map(|x| x > BigInt::from(0)).unwrap_or(false)).count(),
        "makerBothLegsThrough": cases.iter().filter(|c| c["maker"]["bothLegsHaveStrictThrough"].as_bool().unwrap_or(false)).count(),
        "makerOneLegThrough": cases.iter().filter(|c| c["maker"]["exactlyOneLegHasStrictThrough"].as_bool().unwrap_or(false)).count(),
        "cases": cases, "simulatedFills": V::Null, "realizedPnl": V::Null, "executionAuthority": false,
        "profitabilityEstablished": false,
        "limitations": ["Overlapping historical anchors are dependent; best-later marks use hindsight and are not a strategy return.",
            "Later co-observed depth is not proof of entry, exit, simultaneous matching or atomic execution.",
            "Strict trade-through prints are potential counterparties, not queue priority, aggressor identification or maker fills.",
            "Missing reporting windows, null/unrecognized legacy prices and unobserved paths cannot prove no trades or no future profit.",
            "Ordinary settlement scenarios can fail; zero-payout stress and partial-leg risk remain."]}))
}
