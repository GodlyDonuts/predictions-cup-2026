// live-paper/paired-candidate-capture.mjs — auxiliary dual-NO pair read capture.
use baal_execution::core::payload_hash;
use baal_paper::fixed::{big_amount, money, SCALE, TICK};
use baal_service_runtime::client::ServiceError;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_truthy, js_stringify};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

fn svc(message: impl Into<String>) -> ServiceError {
    ServiceError { message: message.into(), status: None, api_code: None, retry_after_ms: 0.0, recoverable: false, stopped: false }
}
fn need(ok: bool, code: &str) -> Result<(), ServiceError> { if !ok { return Err(svc(format!("PAIRED_CAPTURE_{}", code))); } Ok(()) }
fn safe_id(v: &V) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty() && s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit())
        && s.parse::<i64>().is_ok_and(|n| n.unsigned_abs() <= 9_007_199_254_740_991))
}
fn uuid_id(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        let b = s.as_bytes();
        b.len() == 36 && b[8] == b'-' && b[13] == b'-' && b[18] == b'-' && b[23] == b'-'
            && b.iter().enumerate().all(|(i, c)| matches!(i, 8 | 13 | 18 | 23) || c.is_ascii_digit() || (b'a'..=b'f').contains(c))
    })
}
fn cohort_ok(name: &V) -> bool {
    name.as_str().is_some_and(|s| {
        let b = s.as_bytes();
        b.len() >= 2 && b.len() <= 64 && b[0].is_ascii_lowercase() && b[1..].iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
    })
}
fn instant(v: &V) -> Result<i64, ServiceError> { crate::model::instant(v.as_str().unwrap_or("")).map_err(svc) }
fn res_id_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 120 && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
}
fn paired_code(message: &str) -> bool {
    message.len() > 15 && message.starts_with("PAIRED_CAPTURE_")
        && message[15..].bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
}
fn js_num(v: f64) -> V { if v.fract() == 0.0 && v.is_finite() && v.abs() <= 9_007_199_254_740_991.0 { json!(v as i64) } else { json!(v) } }
/// POLICY (frozen)
fn policy() -> V {
    json!({"rootMaximumAgeMs":300000,"bookMaximumAgeMs":20000,"maximumSkewMs":10000,"accountMaximumAgeMs":20000,
        "depthFractionBps":100,"quantity":1,"minimumMargin":"0.010000","maximumReservationAgeMs":10000})
}

/// The accountView() eligibility check on a supplied eligibility object.
fn account_view(get_account_eligibility: &Arc<dyn Fn() -> V + Send + Sync>) -> Result<V, ServiceError> {
    let e = get_account_eligibility();
    need(e["complete"].as_bool() == Some(true) && e["snapshot"]["schemaVersion"].as_i64() == Some(1)
        && e["availableAt"].is_string()
        && e["snapshot"]["excludedExchangeIds"].as_array().is_some_and(|a| a.iter().all(safe_id)), "ACCOUNT_COMPLETE")?;
    let keys: String = e["snapshot"].as_object().map(|m| { let mut k: Vec<_> = m.keys().cloned().collect(); k.sort(); k })
        .unwrap_or_default().join("|");
    need(keys == "excludedExchangeIds|ordersAt|positionsAt|schemaVersion", "ACCOUNT_SCHEMA")?;
    instant(&e["availableAt"])?; instant(&e["snapshot"]["positionsAt"])?; instant(&e["snapshot"]["ordersAt"])?;
    Ok(e.clone())
}
fn account_gate(legs: &V, e: &V, at: i64) -> Result<(), ServiceError> {
    need(instant(&e["availableAt"])? <= at
        && [e["snapshot"]["positionsAt"].clone(), e["snapshot"]["ordersAt"].clone()].iter()
            .map(|t| instant(t)).collect::<Result<Vec<i64>, _>>()?
            .iter().all(|t| *t <= at && at - *t <= 20000), "ACCOUNT_NOT_CAUSAL_OR_FRESH")?;
    let excluded = e["snapshot"]["excludedExchangeIds"].as_array().cloned().unwrap_or_default();
    need(!legs.as_array().cloned().unwrap_or_default().iter()
        .any(|l| excluded.iter().any(|x| x == &l["exchangeId"])), "SCOPED_ACCOUNT_EXPOSURE")?;
    Ok(())
}

/// createPairedCandidateCapture({universe,dispatch,sharedReadHeadroom,getAccountEligibility,canOperate,signal,now}).
/// `signal` is a closure returning the aborted flag; `dispatch` is the shared
/// synchronous spec dispatcher returning ServiceError so `e.status` survives.
// The caller supplies its EXISTING serialized client and enforced account-budget
// view. This helper owns no client, credentials, limiter, publisher, timer or
// execution authority. Roots are finished before a separately granted two-read
// book packet. Neither grant increases the ordinary four-book allocation.
pub struct PairedCandidateCapture {
    universe: V,
    dispatch: Arc<dyn Fn(&V) -> Result<V, ServiceError> + Send + Sync>,
    shared_read_headroom: Arc<dyn Fn(&V) -> bool + Send + Sync>,
    get_account_eligibility: Arc<dyn Fn() -> V + Send + Sync>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
    signal: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    permits: HashMap<usize, String>,
    next_fence: usize,
    consumed: HashSet<String>,
    candidate: V,
    busy: bool,
}
impl PairedCandidateCapture {
    pub fn new(universe: &V,
        dispatch: Arc<dyn Fn(&V) -> Result<V, ServiceError> + Send + Sync>,
        shared_read_headroom: Arc<dyn Fn(&V) -> bool + Send + Sync>,
        get_account_eligibility: Arc<dyn Fn() -> V + Send + Sync>,
        can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
        signal: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
        now: Arc<dyn Fn() -> i64 + Send + Sync>) -> Result<Self, ServiceError> {
        need(uuid_id(&universe["tournamentId"]) && universe["exchanges"].is_array(), "DEPENDENCIES")?;
        Ok(PairedCandidateCapture { universe: universe.clone(), dispatch, shared_read_headroom,
            get_account_eligibility, can_operate, signal, now,
            permits: HashMap::new(), next_fence: 0, consumed: HashSet::new(), candidate: V::Null, busy: false })
    }
    fn aborted(&self) -> bool { self.signal.as_ref().map(|s| s()).unwrap_or(false) }
    fn operate(&self) -> Result<(), ServiceError> { need((self.can_operate)() && !self.aborted(), "STOPPED") }
    fn budget(&self, reads: f64) -> Result<(), ServiceError> {
        need((self.shared_read_headroom)(&json!({"reads":js_num(reads),"at":ms_to_iso((self.now)() as f64)})), "SHARED_BUDGET_HEADROOM")
    }
    /// authorizes(spec) — the fence id embedded as spec.beforeDispatch / spec.beforeNetwork.
    pub fn authorizes(&self, spec: &V) -> bool {
        (self.can_operate)() && !self.aborted() && spec["method"].as_str() == Some("GET")
            && spec["body"].is_null() && spec["bodyJson"].is_null()
            && spec["beforeDispatch"].as_u64().and_then(|id| self.permits.get(&(id as usize))).map(|s| s.as_str()) == spec["endpoint"].as_str()
            && spec["beforeNetwork"].as_u64().and_then(|id| self.permits.get(&(id as usize))).map(|s| s.as_str()) == spec["endpoint"].as_str()
    }
    fn grant(&mut self, g: &V, reads: f64) -> Result<V, ServiceError> {
        self.operate()?;
        need(g["kind"].as_str() == Some("auxiliary_candidate_reads") && g["sharedAccountBudget"].as_bool() == Some(true)
            && g["reservedReads"].as_f64() == Some(reads)
            && g["reservationId"].as_str().is_some_and(res_id_ok)
            && !self.consumed.contains(g["reservationId"].as_str().unwrap_or("")), "EXPLICIT_READ_RESERVATION")?;
        let issued = instant(&g["issuedAt"])?;
        let expires = instant(&g["expiresAt"])?;
        need(issued <= (self.now)() && expires > (self.now)() && expires - issued <= 10000, "RESERVATION_CLOCK")?;
        self.budget(reads)?;
        self.consumed.insert(g["reservationId"].as_str().unwrap().to_string());
        Ok(g.clone())
    }
    fn valid_grant(&self, g: &V) -> Result<(), ServiceError> {
        self.operate()?;
        need((self.now)() < instant(&g["expiresAt"])?, "RESERVATION_EXPIRED")
    }
    fn read(&mut self, endpoint: &str, g: &V, remaining: f64, guard: &dyn Fn() -> Result<(), ServiceError>) -> Result<V, ServiceError> {
        self.next_fence += 1;
        let fence_id = self.next_fence;
        self.permits.insert(fence_id, endpoint.to_string());
        let result = (|| -> Result<V, ServiceError> {
            // The existing client durably admits this read BETWEEN these callbacks. Its
            // reservation must not be double-counted against the remaining shared quota.
            // beforeDispatch: admitted from the serialized queue.
            self.valid_grant(g)?; self.budget(remaining)?; guard()?;
            // beforeNetwork: immediately before the network call.
            self.valid_grant(g)?; self.budget(remaining - 1.0)?; guard()?;
            let r = (self.dispatch)(&json!({"method":"GET","endpoint":endpoint,"beforeDispatch":fence_id,"beforeNetwork":fence_id}))?;
            need(r["status"].as_i64() == Some(200) && r["endpoint"].as_str() == Some(endpoint)
                && {
                    let started = instant(&r["requestStartedAt"])?;
                    let received = instant(&r["receivedAt"])?;
                    started <= received && received <= (self.now)() && (self.now)() - received <= 20000
                }, "RECEIPT")?;
            need(r["data"]["contexts"].as_array().is_some_and(|c| c.len() == 1
                && c[0]["type"].as_str() == Some("tournament")
                && c[0]["tournament"]["id"] == self.universe["tournamentId"]), "RESPONSE_SCOPE")?;
            Ok(r)
        })();
        self.permits.remove(&fence_id);
        result
    }
    fn review_roots(&self) -> Result<V, ServiceError> {
        let captured = self.candidate["roots"].as_array().cloned().unwrap_or_default();
        need(captured.len() == 2, "ROOT_CACHE_HASH_OR_AGE")?;
        for c in &captured {
            let h = payload_hash(&c["root"]).map_err(svc)?;
            need(c["rootHash"].as_str() == Some(h.as_str())
                && (self.now)() - instant(&c["receivedAt"])? <= 300000, "ROOT_CACHE_HASH_OR_AGE")?;
        }
        let known_at = ms_to_iso((self.now)() as f64);
        let min_received = captured.iter().map(|c| instant(&c["receivedAt"])).collect::<Result<Vec<i64>, _>>()?
            .into_iter().min().unwrap_or(0);
        let legs: Vec<V> = self.candidate["legs"].as_array().cloned().unwrap_or_default().iter().enumerate().map(|(i, l)| {
            let mut leg = l.clone();
            leg["captureMarketId"] = captured[i]["marketId"].clone();
            leg["outcome"] = json!("NO");
            leg["rulesHash"] = captured[i]["rootHash"].clone();
            leg["root"] = captured[i]["root"].clone();
            leg
        }).collect();
        let review = json!({"schemaVersion":1,"kind":"conditional_no_pair_review","id":self.candidate["name"].clone(),
            "tournamentId":self.universe["tournamentId"].clone(),"evidenceHash":payload_hash(&V::Array(captured)).map_err(svc)?,
            "recordedAt":known_at,"expiresAt":ms_to_iso((min_received + 300000) as f64),
            "normalResolutionOnly":true,"administrativeRefundGapAcknowledged":true,"legs":legs,
            "assumptions":["Same General election race/stage/office/date and different party winners.",
                "Normal mutual exclusion is conditional; voids, refunds, affiliation disputes or inconsistent administrative resolution can lose the whole cost.",
                "Sequential observations and purchases are not atomic; a hedge may fail."],
            "scenarios":[{"id":"democratic","category":"ordinary","payouts":[0,1]},
                {"id":"republican","category":"ordinary","payouts":[1,0]},
                {"id":"other","category":"ordinary","payouts":[1,1]},
                {"id":"exception","category":"exception","payouts":[0,0]}]});
        crate::dual_no_review_v1::validate_dual_no_review(&review).map_err(svc)?;
        Ok(review)
    }
    fn screen(&self, account: &V) -> Result<V, ServiceError> {
        let books: Vec<V> = self.candidate["books"].as_array().cloned().unwrap_or_default().iter().map(|c| c["book"].clone()).collect();
        let at = (self.now)();
        let starts: Vec<i64> = books.iter().map(|b| instant(&b["requestStartedAt"])).collect::<Result<_,_>>()?;
        let receipts: Vec<i64> = books.iter().map(|b| instant(&b["receivedAt"])).collect::<Result<_,_>>()?;
        account_gate(&self.candidate["legs"], account, starts.iter().min().copied().unwrap_or(i64::MAX))?;
        need([instant(&account["snapshot"]["positionsAt"])?, instant(&account["snapshot"]["ordersAt"])?].iter()
            .all(|t| at - *t <= 20000), "ACCOUNT_STALE_AT_PACKET")?;
        let recorded = instant(&self.candidate["review"]["recordedAt"])?;
        need(starts.iter().all(|t| *t >= recorded), "RULES_NOT_KNOWN_AT_REQUEST")?;
        need(at < instant(&self.candidate["review"]["expiresAt"])?
            && self.candidate["roots"].as_array().cloned().unwrap_or_default().iter()
                .map(|r| instant(&r["receivedAt"])).collect::<Result<Vec<i64>,_>>()?
                .iter().all(|t| at - *t <= 300000), "ROOTS_EXPIRED")?;
        need(starts.iter().all(|t| at - *t <= 20000)
            && receipts.iter().max().copied().unwrap_or(i64::MIN).saturating_sub(starts.iter().min().copied().unwrap_or(i64::MAX)) <= 10000,
            "BOOK_AGE_OR_SKEW")?;
        let prices: Vec<V> = books.iter().map(|b| {
            let level = b["bids"].as_array().cloned().unwrap_or_default().iter().cloned()
                .find(|l| (l["quantity"].as_f64().unwrap_or(f64::NAN) * 100.0 / 10000.0).floor() >= 1.0);
            match level {
                Some(l) => {
                    let depth = (l["quantity"].as_f64().unwrap_or(f64::NAN) * 100.0 / 10000.0).floor();
                    let price = big_amount(l["price"].as_str().unwrap_or("")).map(|p| money(&(BigInt::from(SCALE) - p + BigInt::from(TICK))));
                    match price { Ok(p) => Ok(json!({"price":p,"depth":depth as i64})), Err(e) => Err(svc(e)) }
                }
                None => Ok(V::Null),
            }
        }).collect::<Result<_,_>>()?;
        let cost: Option<BigInt> = if prices.iter().all(|p| js_truthy(Some(p))) {
            Some(prices.iter().map(|p| big_amount(p["price"].as_str().unwrap_or(""))).collect::<Result<Vec<BigInt>, String>>().map_err(svc)?
                .into_iter().fold(BigInt::from(0), |n, x| n + x))
        } else { None };
        let minimum_margin = big_amount("0.010000").map_err(svc)?;
        let positive = cost.is_some()
            && cost.as_ref().unwrap() <= &(BigInt::from(SCALE) - &minimum_margin)
            && prices.iter().map(|p| big_amount(p["price"].as_str().unwrap_or(""))).collect::<Result<Vec<BigInt>, String>>().map_err(svc)?
                .iter().all(|p| *p >= BigInt::from(TICK) && *p <= BigInt::from(SCALE - TICK));
        Ok(json!({"positiveConditionalScreen":positive,"quantity":1,
            "cushionedCost":cost.as_ref().map(|c| json!(money(c))).unwrap_or(V::Null),
            "conditionalOrdinarySurplus":cost.as_ref().map(|c| json!(money(&(BigInt::from(SCALE) - c)))).unwrap_or(V::Null),
            "zeroPayoutExceptionLoss":cost.as_ref().map(|c| json!(money(&(-c)))).unwrap_or(V::Null),
            "eligibleDepth":prices.iter().map(|p| if p.is_null() { json!(0) } else { p["depth"].clone() }).collect::<Vec<_>>(),
            "wholeWindowSkewMs":receipts.iter().max().copied().unwrap_or(i64::MIN).saturating_sub(starts.iter().min().copied().unwrap_or(i64::MAX)),
            "maximumBookAgeMs":at.saturating_sub(starts.iter().min().copied().unwrap_or(i64::MAX)),
            "accountReceiptKnownBeforeBothRequests":true,"accountSnapshotHash":payload_hash(&account["snapshot"]).map_err(svc)?,
            "representation":"documented_same_exchange_yes_bid_complement","ordinaryResolutionOnly":true,
            "publicationCreated":false,"orderEligible":false,"executionAuthority":false,"fills":0,"orders":0,
            "profitabilityEstablished":false}))
    }
    /// status()
    pub fn status(&self) -> V {
        if self.candidate.is_null() { json!({"phase":"idle","executionAuthority":false}) } else {
            let mut c = self.candidate.clone();
            c["policy"] = policy();
            c["executionAuthority"] = json!(false); c["additionalAccountClients"] = json!(0);
            c["fills"] = json!(0); c["orders"] = json!(0);
            c
        }
    }
    /// begin({name,marketIds})
    pub fn begin(&mut self, spec: &V) -> Result<V, ServiceError> {
        self.operate()?;
        let name = spec["name"].clone();
        let market_ids = spec["marketIds"].as_array().cloned().unwrap_or_default();
        let distinct = market_ids.iter().map(js_stringify).collect::<BTreeSet<_>>().len();
        need(self.candidate.is_null() && cohort_ok(&name) && spec["marketIds"].is_array()
            && market_ids.len() == 2 && distinct == 2, "SINGLE_PAIR_REQUEST")?;
        let exchanges = self.universe["exchanges"].as_array().cloned().unwrap_or_default();
        let legs: Vec<V> = market_ids.iter().map(|mid| exchanges.iter()
            .find(|l| l["marketId"] == *mid && l["option"].as_str() == Some("YES")).cloned().unwrap_or(V::Null)).collect();
        need(legs.iter().all(|l| js_truthy(Some(l)) && safe_id(&l["marketId"]) && safe_id(&l["exchangeId"])), "PAIR_SCOPE")?;
        let legs_min: Vec<V> = legs.iter().map(|l| json!({"marketId":l["marketId"].clone(),"exchangeId":l["exchangeId"].clone()})).collect();
        self.candidate = json!({"name":name,"legs":legs_min,"phase":"roots","roots":[],"books":[],"review":V::Null,"screen":V::Null});
        Ok(self.status())
    }
    /// advance({reservation})
    pub fn advance(&mut self, spec: &V) -> Result<V, ServiceError> {
        self.operate()?;
        need(!self.candidate.is_null() && !self.busy
            && !["complete", "failed"].contains(&self.candidate["phase"].as_str().unwrap_or("")), "SINGLE_PAIR_OPERATION")?;
        self.busy = true;
        let result = self.advance_inner(spec);
        if let Err(e) = &result {
            self.candidate["phase"] = json!("failed");
            self.candidate["failureCode"] = json!(if paired_code(&e.message) { e.message.clone() }
                else if e.status.is_some_and(|s| s != 0) { format!("SHARED_CLIENT_HTTP_{}", e.status.unwrap()) }
                else { "SHARED_CLIENT_OR_VALIDATION_FAILURE".to_string() });
        }
        self.busy = false;
        result
    }
    fn advance_inner(&mut self, spec: &V) -> Result<V, ServiceError> {
        let reservation = spec["reservation"].clone();
        if self.candidate["phase"].as_str() == Some("roots") {
            let roots_len = self.candidate["roots"].as_array().map(|a| a.len()).unwrap_or(0) as f64;
            let remaining = (2.0 - roots_len).min(reservation["reservedReads"].as_f64().unwrap_or(0.0));
            need(remaining > 0.0 && remaining <= 2.0, "ROOT_READ_GRANT")?;
            let g = self.grant(&reservation, remaining)?;
            let mut i = 0.0;
            while i < remaining {
                let idx = self.candidate["roots"].as_array().map(|a| a.len()).unwrap_or(0);
                let l = self.candidate["legs"][idx].clone();
                let endpoint = format!("/markets/{}/nodes?tournamentId={}",
                    l["marketId"].as_str().unwrap_or(""), self.universe["tournamentId"].as_str().unwrap_or(""));
                let r = self.read(&endpoint, &g, remaining - i, &|| Ok(()))?;
                need(r["data"]["market_id"] == l["marketId"]
                    && r["data"]["root"]["node_type"].as_str() == Some("contract")
                    && r["data"]["root"]["contract_type"].as_str() == Some("Election Outcome"), "ROOT_SCOPE")?;
                let entry = json!({"marketId":l["marketId"].clone(),"tournamentId":self.universe["tournamentId"].clone(),
                    "requestStartedAt":r["requestStartedAt"].clone(),"receivedAt":r["receivedAt"].clone(),
                    "root":r["data"]["root"].clone(),"rootHash":payload_hash(&r["data"]["root"]).map_err(svc)?,
                    "responseHash":payload_hash(&r["data"]).map_err(svc)?});
                self.candidate["roots"].as_array_mut().map(|a| a.push(entry));
                self.operate()?;
                i += 1.0;
            }
            if self.candidate["roots"].as_array().map(|a| a.len()).unwrap_or(0) == 2 {
                let review = self.review_roots()?;
                self.candidate["review"] = review;
                self.candidate["phase"] = json!("books");
            }
            return Ok(self.status());
        }
        need(self.candidate["phase"].as_str() == Some("books"), "PHASE")?;
        let g = self.grant(&reservation, 2.0)?;
        let review = self.review_roots()?;
        self.candidate["review"] = review;
        let account = account_view(&self.get_account_eligibility)?;
        let account_hash = payload_hash(&account["snapshot"]).map_err(svc)?;
        account_gate(&self.candidate["legs"], &account, (self.now)())?;
        for i in 0..2usize {
            let l = self.candidate["legs"][i].clone();
            let endpoint = format!("/markets/{}/orderbook?tournamentId={}&depth=20",
                l["marketId"].as_str().unwrap_or(""), self.universe["tournamentId"].as_str().unwrap_or(""));
            let guard = {
                let now = self.now.clone();
                let elig = self.get_account_eligibility.clone();
                let legs = self.candidate["legs"].clone();
                let account = account.clone();
                let account_hash = account_hash.clone();
                let expires_at = self.candidate["review"]["expiresAt"].clone();
                move || -> Result<(), ServiceError> {
                    account_gate(&legs, &account, (now)())?;
                    let fresh = account_view(&elig)?;
                    need(payload_hash(&fresh["snapshot"]).map_err(svc)? == account_hash, "ACCOUNT_CHANGED_BETWEEN_LEGS")?;
                    need((now)() < instant(&expires_at)?, "ROOTS_EXPIRED")?;
                    Ok(())
                }
            };
            let r = self.read(&endpoint, &g, (2 - i) as f64, &guard)?;
            let started = instant(&r["requestStartedAt"])?;
            account_gate(&self.candidate["legs"], &account, started)?;
            let b = crate::model::normalize_book(&r["data"], l["marketId"].as_str().unwrap_or(""),
                self.universe["tournamentId"].as_str().unwrap_or(""), r["receivedAt"].as_str().unwrap_or(""),
                r["requestStartedAt"].as_str().unwrap_or(""), 20000, &BTreeSet::new(), "authoritative_receipt_v1").map_err(svc)?;
            need(b["exchangeId"] == l["exchangeId"]
                && b["bids"].as_array().is_some_and(|a| a.len() <= 20)
                && b["asks"].as_array().is_some_and(|a| a.len() <= 20), "BOOK_EXCHANGE_OR_DEPTH")?;
            let entry = json!({"marketId":l["marketId"].clone(),"requestStartedAt":r["requestStartedAt"].clone(),
                "receivedAt":r["receivedAt"].clone(),"book":b,"responseHash":payload_hash(&r["data"]).map_err(svc)?});
            self.candidate["books"].as_array_mut().map(|a| a.push(entry));
            self.operate()?;
        }
        let screen = self.screen(&account)?;
        self.candidate["screen"] = screen;
        self.candidate["phase"] = json!("complete");
        Ok(self.status())
    }
}
