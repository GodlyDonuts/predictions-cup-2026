// live-paper/funded-quote-replay.mjs — one local counterfactual wallet.
// No network, account or order endpoint exists.
use baal_paper::fixed::{big_amount, money, SCALE, TICK};
use baal_service_runtime::json_text::js_truthy;
use indexmap::IndexMap;
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use crate::stable_maker_replay_v2::{dp, js_string, js_strict_eq_opt, prop_read_err};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("FUNDED_QUOTE_{}", code)); }
    Ok(())
}
// key(q)=q.policyId+':'+q.marketId+':'+q.createdAt — missing fields stringify as 'undefined'.
fn key(q: &V) -> String {
    let part = |k: &str| q.get(k).map(js_string).unwrap_or_else(|| "undefined".to_string());
    format!("{}:{}:{}", part("policyId"), part("marketId"), part("createdAt"))
}
// JS Number.isSafeInteger: integral and |n| <= 2**53 - 1.
fn js_safe_int(v: &V) -> Option<i64> {
    if let Some(i) = v.as_i64() { return Some(i); }
    if let Some(u) = v.as_u64() { return i64::try_from(u).ok().filter(|i| *i <= 9_007_199_254_740_991); }
    match v.as_f64() {
        Some(f) if f.fract() == 0.0 && f.abs() <= 9_007_199_254_740_991.0 => Some(f as i64),
        _ => None,
    }
}
// Insert the property only when the JS side would have a defined value
// (JSON.stringify drops undefined-valued keys).
fn insert_defined(m: &mut serde_json::Map<String, V>, k: &str, v: Option<V>) {
    if let Some(v) = v { m.insert(k.to_string(), v); }
}

struct ActiveQuote {
    q: V,
    costs: [BigInt; 2],
    filled: [Option<V>; 2],
    censored: bool,
}

/// createFundedQuoteWallet({policyId, startAt, selector, initialCash, maximumCommitted,
/// maximumCumulativePurchases, maximumMarkets})
pub struct FundedQuoteWallet {
    policy_id: String,
    start_at: f64,
    start_at_raw: V,
    selector: Box<dyn Fn(&V) -> bool>,
    initial: BigInt,
    cap: BigInt,
    lifetime: BigInt,
    maximum_markets: i64,
    cash: BigInt,
    reserved: BigInt,
    basis: BigInt,
    cumulative: BigInt,
    realized: BigInt,
    closed: i64,
    buys: i64,
    sells: i64,
    paired: i64,
    active: IndexMap<String, ActiveQuote>,
    fills: Vec<V>,
    outcomes: Vec<V>,
    selected: i64,
    selector_rejected: i64,
    capital_rejected: i64,
    censored: i64,
}

/// createFundedQuoteWallet(options) — selector defaults to ()=>true.
pub fn create_funded_quote_wallet(options: &V) -> Result<FundedQuoteWallet, String> {
    create_funded_quote_wallet_with_selector(options, Box::new(|_| true))
}

pub fn create_funded_quote_wallet_with_selector(options: &V,
    selector: Box<dyn Fn(&V) -> bool>) -> Result<FundedQuoteWallet, String> {
    // JS destructuring throws on null; non-object primitives coerce to an empty view.
    if options.is_null() && !options.is_object() {
        return Err("Cannot destructure property 'policyId' of 'null' as it is null.".into());
    }
    let opt = |k: &str| options.get(k).cloned().unwrap_or(V::Null);
    // JS destructuring defaults apply only to absent/undefined; an explicit null
    // reaches amount() / the CONFIG checks and fails there.
    let money_arg = |k: &str, default: &str| -> Result<BigInt, String> {
        match options.get(k) {
            None => big_amount(default),
            Some(V::String(s)) => big_amount(s),
            Some(_) => Err("Money must be a canonical decimal string with at most six places".into()),
        }
    };
    // const initial=amount(initialCash),... — amount() errors propagate before CONFIG.
    let initial = money_arg("initialCash", "100.000000")?;
    let cap = money_arg("maximumCommitted", "5.000000")?;
    let lifetime = money_arg("maximumCumulativePurchases", "25.000000")?;
    let start = dp(&opt("startAt"));
    let policy_id = opt("policyId");
    let maximum_markets = options.get("maximumMarkets").cloned().unwrap_or(json!(3));
    need(policy_id.is_string() && start.is_finite() && initial > BigInt::from(0)
        && cap > BigInt::from(0) && lifetime > BigInt::from(0)
        && js_safe_int(&maximum_markets).is_some()
        && js_safe_int(&maximum_markets).unwrap_or(0) > 0, "CONFIG")?;
    Ok(FundedQuoteWallet {
        policy_id: policy_id.as_str().unwrap_or("").to_string(),
        start_at: start,
        start_at_raw: opt("startAt"), // verbatim constructor argument (JS echoes it)

        selector,
        initial: initial.clone(),
        cap,
        lifetime,
        maximum_markets: js_safe_int(&maximum_markets).unwrap_or(3),
        cash: initial,
        reserved: BigInt::from(0),
        basis: BigInt::from(0),
        cumulative: BigInt::from(0),
        realized: BigInt::from(0),
        closed: 0,
        buys: 0,
        sells: 0,
        paired: 0,
        active: IndexMap::new(),
        fills: Vec::new(),
        outcomes: Vec::new(),
        selected: 0,
        selector_rejected: 0,
        capital_rejected: 0,
        censored: 0,
    })
}

impl FundedQuoteWallet {
    /// decideQuote(q) -> bool (rejects silently) or propagates need() errors.
    pub fn decide_quote(&mut self, q: &V) -> Result<bool, String> {
        if q["policyId"].as_str() != Some(self.policy_id.as_str())
            || dp(&q["createdAt"]) < self.start_at {
            return Ok(false);
        }
        let limits = q["limitsTicks"].as_array();
        need(limits.is_some_and(|l| l.len() == 2
            && l.iter().all(|v| js_safe_int(v).is_some_and(|i| i > 0 && i < 200))), "LIMITS")?;
        need(dp(&q["createdAt"]) < dp(&q["activationAt"])
            && dp(&q["activationAt"]) < dp(&q["expiresAt"]), "CAUSALITY")?;
        if !(self.selector)(&q.clone()) {
            self.selector_rejected += 1;
            return Ok(false);
        }
        let ticks: Vec<i64> = limits.unwrap().iter()
            .map(|v| js_safe_int(v).unwrap_or(0)).collect();
        let tick = BigInt::from(TICK);
        let cost = BigInt::from(ticks[0]) * &tick + BigInt::from(ticks[1]) * &tick;
        let market_taken = self.active.values()
            .any(|a| js_strict_eq_opt(a.q.get("marketId"), q.get("marketId")));
        if self.active.len() as i64 >= self.maximum_markets || market_taken
            || self.cash.clone() - &self.reserved < cost
            || self.basis.clone() + &self.reserved + &cost > self.cap
            || self.cumulative.clone() + &self.reserved + &cost > self.lifetime {
            self.capital_rejected += 1;
            return Ok(false);
        }
        let id = key(q);
        need(!self.active.contains_key(&id), "DUPLICATE_INTENT")?;
        self.active.insert(id, ActiveQuote {
            q: q.clone(),
            costs: [BigInt::from(ticks[0]) * &tick, BigInt::from(ticks[1]) * &tick],
            filled: [None, None],
            censored: false,
        });
        self.reserved += &cost;
        self.selected += 1;
        Ok(true)
    }

    /// onFill(event) — policyId mismatch is a no-op.
    pub fn on_fill(&mut self, event: &V) -> Result<(), String> {
        if event["policyId"].as_str() != Some(self.policy_id.as_str()) { return Ok(()); }
        let id = key(event);
        let Some(q) = self.active.get_mut(&id) else {
            return Err("FUNDED_QUOTE_UNKNOWN_INTENT".into());
        };
        let side = match event["side"].as_str() {
            Some("YES") => 0usize,
            Some("NO") => 1usize,
            _ => usize::MAX,
        };
        need(side != usize::MAX, "SIDE")?;
        // if(q.filled[side]) is a JS truthy check — a stored null/false fill doesn't enter.
        if q.filled[side].as_ref().is_some_and(|v| js_truthy(Some(v))) {
            let prev = q.filled[side].as_ref().unwrap();
            // q.filled[side].source.id===event.source.id — a null/undefined .source throws
            // on the left side first, then the right.
            match prev.get("source") {
                v @ (None | Some(V::Null)) => return Err(prop_read_err(v, "id")),
                _ => {}
            }
            match event.get("source") {
                v @ (None | Some(V::Null)) => return Err(prop_read_err(v, "id")),
                _ => {}
            }
            need(js_strict_eq_opt(
                prev["source"].get("id"), event["source"].get("id")), "CONFLICTING_FILL")?;
            return Ok(());
        }
        need(!q.censored
            && js_strict_eq_opt(event.get("limitTicks"), q.q["limitsTicks"].get(side))
            && dp(&event["learnedAt"]) >= dp(&q.q["activationAt"])
            && dp(&event["learnedAt"]) < dp(&q.q["expiresAt"]), "FILL_WINDOW")?;
        let cost = q.costs[side].clone();
        need(self.reserved >= cost && self.cash >= cost, "INSUFFICIENT_CASH")?;
        self.reserved -= &cost;
        self.cash -= &cost;
        self.basis += &cost;
        self.cumulative += &cost;
        self.buys += 1;
        q.filled[side] = Some(event.clone());
        // {...event, action:'buy', price:money(cost), quantity:1, model:...}
        let mut fill = event.as_object().cloned().unwrap_or_default();
        fill.insert("action".into(), json!("buy"));
        fill.insert("price".into(), json!(money(&cost)));
        fill.insert("quantity".into(), json!(1));
        fill.insert("model".into(), json!("hypothetical_strict_trade_through_not_proven_execution"));
        self.fills.push(V::Object(fill));
        Ok(())
    }

    /// onOutcome(event) — resolves/censors an active intent.
    pub fn on_outcome(&mut self, event: &V) -> Result<(), String> {
        if event["policyId"].as_str() != Some(self.policy_id.as_str()) { return Ok(()); }
        let id = key(event);
        if !self.active.contains_key(&id) {
            return Err("FUNDED_QUOTE_UNKNOWN_OUTCOME".into());
        }
        // q.filled.filter(Boolean).length — counts truthy stored fills only.
        let filled = {
            let q = &self.active[&id];
            q.filled.iter().filter(|f| f.as_ref().is_some_and(|v| js_truthy(Some(v)))).count()
        };
        let kind = event["kind"].as_str().unwrap_or("");
        if kind == "censored" {
            let q = self.active.get_mut(&id).unwrap();
            if !q.censored {
                self.censored += 1;
                q.censored = true;
                let mut o = serde_json::Map::new();
                o.insert("key".into(), json!(id));
                insert_defined(&mut o, "at", event.get("resolvedAt").cloned());
                o.insert("kind".into(), json!("unknown_execution_path"));
                o.insert("knownFills".into(), json!(filled));
                self.outcomes.push(V::Object(o));
            }
            // Missing prints can hide fills on either leg. Keep both known basis and
            // every unresolved reservation; do not release cash or invent a zero result.
            return Ok(());
        }
        let (censored_flag, limits, costs, market_id) = {
            let q = &self.active[&id];
            (q.censored, q.q["limitsTicks"].clone(), q.costs.clone(), q.q.get("marketId").cloned())
        };
        need(!censored_flag && !js_truthy(event.get("gapExposed")), "UNKNOWN_PATH")?;
        if kind == "unfilled" {
            // filled===0 && event.tradeSources.length===0 — a missing tradeSources throws
            // in JS only when filled===0 (short-circuit).
            if filled == 0 {
                let n = match event.get("tradeSources") {
                    v @ (None | Some(V::Null)) => return Err(prop_read_err(v, "length")),
                    Some(v) => v.as_array().map(|a| a.len())
                        .ok_or_else(|| "Cannot read properties of non-array (reading 'length')".to_string())?,
                };
                need(n == 0, "UNFILLED_CONFLICT")?;
            } else {
                need(false, "UNFILLED_CONFLICT")?;
            }
        } else if kind == "paired" {
            let expected = 200i64
                - js_safe_int(&limits[0]).unwrap_or(0)
                - js_safe_int(&limits[1]).unwrap_or(0);
            need(filled == 2
                && js_strict_eq_opt(event.get("counterfactualNetTicks"), Some(&json!(expected))),
                "PAIR_ACCOUNTING")?;
            let scale = BigInt::from(SCALE);
            self.cash += &scale;
            self.basis -= &costs[0] + &costs[1];
            self.realized += &scale - &costs[0] - &costs[1];
            self.paired += 1;
            self.closed += 1;
        } else if kind == "marked" {
            let net = js_safe_int(&event["counterfactualNetTicks"]);
            need(filled == 1 && net.is_some(), "MARK_ACCOUNTING")?;
            let q = &self.active[&id];
            let side = if q.filled[0].as_ref().is_some_and(|v| js_truthy(Some(v))) { 0usize } else { 1usize };
            let proceeds = &costs[side] + BigInt::from(net.unwrap_or(0)) * BigInt::from(TICK);
            need(proceeds >= BigInt::from(0) && proceeds <= BigInt::from(SCALE), "EXIT_PRICE")?;
            self.cash += &proceeds;
            self.basis -= &costs[side];
            self.realized += &proceeds - &costs[side];
            self.sells += 1;
            self.closed += 1;
            let mut fill = serde_json::Map::new();
            fill.insert("action".into(), json!("sell"));
            insert_defined(&mut fill, "at", event.get("resolvedAt").cloned());
            insert_defined(&mut fill, "marketId", market_id);
            fill.insert("side".into(), json!(if side == 0 { "YES" } else { "NO" }));
            fill.insert("price".into(), json!(money(&proceeds)));
            fill.insert("quantity".into(), json!(1));
            fill.insert("model".into(), json!("hypothetical_later_depth_markout_not_proven_execution"));
            self.fills.push(V::Object(fill));
        } else {
            need(false, "OUTCOME_KIND")?;
        }
        // for(let side=0;side<2;side++) if(!q.filled[side]) reserved-=q.costs[side]
        {
            let q = self.active.get_mut(&id).unwrap();
            for side in 0..2 {
                if !q.filled[side].as_ref().is_some_and(|v| js_truthy(Some(v))) {
                    self.reserved -= &q.costs[side];
                }
            }
        }
        self.active.shift_remove(&id);
        need(self.cash >= self.reserved && self.basis >= BigInt::from(0)
            && self.reserved >= BigInt::from(0), "BALANCE")?;
        need(&self.cash + &self.basis == &self.initial + &self.realized, "CONSERVATION")?;
        let mut o = serde_json::Map::new();
        o.insert("key".into(), json!(id));
        insert_defined(&mut o, "at", event.get("resolvedAt").cloned());
        o.insert("kind".into(), event["kind"].clone());
        o.insert("realizedPnl".into(), json!(money(&self.realized)));
        self.outcomes.push(V::Object(o));
        Ok(())
    }

    /// report() — snapshot in the JS literal's key order.
    pub fn report(&self) -> V {
        let mut m = serde_json::Map::new();
        m.insert("schemaVersion".into(), json!(1));
        m.insert("mode".into(), json!("funded_strict_through_quote_counterfactual"));
        m.insert("policyId".into(), json!(self.policy_id));
        // startAt echoes the constructor argument verbatim.
        m.insert("startAt".into(), self.start_at_raw.clone());
        m.insert("initialCash".into(), json!(money(&self.initial)));
        m.insert("cash".into(), json!(money(&self.cash)));
        m.insert("reserved".into(), json!(money(&self.reserved)));
        m.insert("inventoryCost".into(), json!(money(&self.basis)));
        m.insert("realizedPnl".into(), json!(money(&self.realized)));
        m.insert("fees".into(), json!("0.000000"));
        m.insert("cumulativePurchases".into(), json!(money(&self.cumulative)));
        m.insert("buyFills".into(), json!(self.buys));
        m.insert("sellFills".into(), json!(self.sells));
        m.insert("pairedRedemptions".into(), json!(self.paired));
        m.insert("closedTrades".into(), json!(self.closed));
        m.insert("counts".into(), json!({"selected": self.selected,
            "selectorRejected": self.selector_rejected,
            "capitalRejected": self.capital_rejected,
            "censored": self.censored}));
        let mut markets = Vec::new();
        for a in self.active.values() {
            let mut e = serde_json::Map::new();
            insert_defined(&mut e, "marketId", a.q.get("marketId").cloned());
            insert_defined(&mut e, "createdAt", a.q.get("createdAt").cloned());
            // f?(i===0?'YES':'NO'):null then .filter(Boolean)
            let known: Vec<V> = a.filled.iter().enumerate()
                .filter(|(_, f)| f.as_ref().is_some_and(|v| js_truthy(Some(v))))
                .map(|(i, _)| json!(if i == 0 { "YES" } else { "NO" }))
                .collect();
            e.insert("knownSides".into(), V::Array(known));
            e.insert("uncertain".into(), json!(a.censored));
            markets.push(V::Object(e));
        }
        m.insert("activeMarkets".into(), V::Array(markets));
        m.insert("completeEconomicObservation".into(), json!(self.active.is_empty()));
        m.insert("fills".into(), V::Array(self.fills.clone()));
        m.insert("outcomes".into(), V::Array(self.outcomes.clone()));
        m.insert("networkRequests".into(), json!(0));
        m.insert("competitionOrders".into(), json!(0));
        m.insert("executionAuthority".into(), json!(false));
        m.insert("profitabilityEstablished".into(), json!(false));
        m.insert("limitations".into(), json!([
            "One funded wallet enforces shared cash, reservations and exact one-share basis; counterfactual fills still do not prove actual queue execution.",
            "Censored quote paths retain worst-case reservations and known holdings; their economic outcomes remain unknown.",
            "A final realized result does not include unresolved inventory value and is not prospective competition profitability."]));
        V::Object(m)
    }
}
