// Economic accounting and execution are included byte-for-byte, never forked.
include!("../../median-prefix-replay/src/frozen_kernel.rs");

use baal_portfolio_engine::{digest, publication::Publication};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
mod source;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pin { pub path: PathBuf, pub bytes: u64, pub sha256: String }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub public_source: Pin,
    pub account_source: Pin,
    pub price_source: Pin,
    pub universe_bank: PathBuf,
    pub output: PathBuf,
    #[serde(default)] pub experiment_cutoff: Option<String>,
}

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(code.into()) }
}

#[derive(Clone, Copy)]
enum Case { Baseline, OriginalReferenceCap }

impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::Baseline => "one_tick_native_control",
            Self::OriginalReferenceCap => "one_tick_original_reference_entry_cap",
        }
    }
}

// Exactly the frozen strict bulk reference: known before original request,
// within the hour ending at publication, then remove newest eligible point.
fn reference(wallet: &MedianWallet, quote: &Quote) -> Vec<(i64, i128)> {
    let mut prior = wallet.bulk_history.get(&quote.market).map(|history| {
        history.iter().filter(|(known, _)| quote.at - *known <= 3_600_000 && *known < quote.request)
            .copied().collect::<Vec<_>>()
    }).unwrap_or_default();
    prior.pop();
    prior
}

struct Arm { case: Case, wallet: MedianWallet, proposals: Vec<V> }

impl Arm {
    fn new(case: Case) -> Self {
        let mut wallet = MedianWallet::new(1);
        wallet.enable_bulk_history();
        Self { case, wallet, proposals: Vec::new() }
    }

    fn observe(&mut self, quote: Quote, eligible: bool, cutoff: Option<i64>, row_hash: &str) -> Result<(), String> {
        if cutoff.is_some_and(|cut| quote.at < cut) {
            self.wallet.prime_history(quote, cutoff.unwrap())?;
            return Ok(());
        }
        let allowed = cutoff.is_none_or(|cut| quote.at > cut && quote.request > cut);
        // Capture before observing; a later quote never changes a carried cap.
        let prior = reference(&self.wallet, &quote);
        let before = self.wallet.pending.get(&quote.market).map(|intent| (intent.created, intent.sequence));
        self.wallet.observe_with_entry_permission(quote.clone(), eligible, allowed)?;
        let newly_created = self.wallet.pending.get(&quote.market).is_some_and(|intent| {
            intent.buy && intent.created == quote.at && intent.sequence == quote.sequence
                && before != Some((intent.created, intent.sequence))
        });
        if !newly_created { return Ok(()); }

        need(prior.len() >= 20 && prior.last().unwrap().0 - prior[0].0 >= 1_800_000,
            "NEW_PROPOSAL_REFERENCE_MISMATCH")?;
        let mut values = prior.iter().map(|(_, bid)| *bid).collect::<Vec<_>>();
        values.sort();
        let median = values[values.len() / 2];
        let ask = quote.asks.first().ok_or("NEW_PROPOSAL_ASK_MISSING")?.0;
        let intent = self.wallet.pending.get_mut(&quote.market).unwrap();
        let original_limit = intent.limit;
        need(median - ask >= TICK && original_limit == (ask + TICK).min(SCALE - TICK),
            "NEW_PROPOSAL_NATIVE_GATE_MISMATCH")?;
        if matches!(self.case, Case::OriginalReferenceCap) {
            intent.limit = original_limit.min(median - TICK);
        }
        let cap = intent.limit;
        need(cap >= ask && cap >= TICK && cap % TICK == 0 && cap <= original_limit,
            "NEW_PROPOSAL_CAP_INVALID")?;
        self.wallet.assert_funded()?;
        self.proposals.push(json!({
            "marketId": quote.market, "exchangeId": quote.exchange,
            "sourceSequence": quote.sequence, "sourceRowHash": row_hash,
            "sourceVersion": quote.version, "availableAtMs": quote.at,
            "requestAtMs": quote.request, "receivedAtMs": quote.received,
            "originalAsk": money(ask), "originalCausalMedianBid": money(median),
            "referencePoints": prior.len(), "firstReferenceKnownAtMs": prior[0].0,
            "lastReferenceKnownAtMs": prior.last().unwrap().0,
            "originalProtectiveLimit": money(original_limit), "fixedBuyLimit": money(cap),
            "lowered": cap < original_limit, "quantity": 1
        }));
        Ok(())
    }

    fn report(&self, at: i64) -> Result<V, String> {
        self.wallet.assert_funded()?;
        let wallet = self.wallet.report(at);
        let mut lots = BTreeMap::new();
        let mut losses = 0;
        for fill in wallet["fills"].as_array().unwrap() {
            let market = fill["marketId"].as_str().unwrap();
            let cost = price(fill["price"].as_str().unwrap())?;
            if fill["action"] == "buy" { lots.insert(market, cost); }
            else if cost < lots.remove(market).ok_or("REPORT_UNCOVERED_SALE")? { losses += 1; }
        }
        Ok(json!({
            "case": self.case.name(), "proposalCount": self.proposals.len(),
            "loweredCapCount": self.proposals.iter().filter(|p| p["lowered"] == true).count(),
            "proposals": self.proposals, "closedLosingTrades": losses, "wallet": wallet,
            "finalWalletStateHash": digest(&self.wallet.checkpoint()?)
        }))
    }
}

pub fn run(cfg: &Config) -> Result<V, String> {
    let started = std::time::Instant::now();
    need(cfg.universe_bank.is_absolute() && cfg.output.is_absolute() && !cfg.output.exists(),
        "FRESH_OFFLINE_OUTPUT_REQUIRED")?;
    let bank_bytes = std::fs::read(&cfg.universe_bank).map_err(|_| "BANK_READ")?;
    let bank: V = serde_json::from_slice(&bank_bytes).map_err(|_| "BANK_JSON")?;
    source::validate_bank_scope(&bank)?;
    let universe = &bank["universe"];
    let cutoff = cfg.experiment_cutoff.as_ref().map(|s| clock(&json!(s))).transpose()?;
    let accounts = source::rows(&cfg.account_source)?;
    source::validate_accounts(&accounts, universe)?;
    let price_rows = source::rows(&cfg.price_source)?;
    source::validate_prices(&price_rows, universe)?;
    let public = source::rows(&cfg.public_source)?;
    let mut publication = Publication::new(universe)?;
    let mut arms = [Arm::new(Case::Baseline), Arm::new(Case::OriginalReferenceCap)];
    let mut price_index = 0;
    let mut first = None;
    let mut last = 0;
    let mut books = 0;
    for row in &public {
        publication.apply(row)?;
        let at = clock(&row["at"])?;
        first.get_or_insert(at);
        last = at;
        while price_index < price_rows.len() && clock(&price_rows[price_index]["at"])? <= at {
            let price_row = &price_rows[price_index];
            let known = clock(&price_row["at"])?;
            for quote in price_row["event"]["snapshot"]["quotes"].as_array().unwrap() {
                if quote["option"] == "YES" && quote["crossed"] == false {
                    if let Some(bid) = source::bid_tick(&quote["bestBid"]) {
                        for arm in &mut arms {
                            arm.wallet.feature_bid(quote["marketId"].as_str().unwrap(), known, bid)?;
                        }
                    }
                }
            }
            price_index += 1;
        }
        if row["kind"] != "book" {
            let market = if row["kind"] == "invalidation" { row["payload"]["marketId"].as_str() } else { None };
            for arm in &mut arms { arm.wallet.boundary(market); }
            continue;
        }
        let quote = Quote::parse(row)?;
        let eligible = source::eligible(&accounts, &quote);
        for arm in &mut arms {
            arm.observe(quote.clone(), eligible, cutoff, row["hash"].as_str().ok_or("PUBLIC_HASH")?)?;
        }
        books += 1;
    }
    Ok(json!({
        "schemaVersion": 1, "mode": "offline_fixed_median_entry_cap_v2",
        "elapsedMs": started.elapsed().as_secs_f64() * 1000., "eventsProcessed": public.len(),
        "originalKernelSha256": format!("{:x}", Sha256::digest(include_bytes!("../../median-prefix-replay/src/frozen_kernel.rs"))),
        "comparatorSha256": format!("{:x}", Sha256::digest(include_bytes!("lib.rs"))),
        "sourceReaderSha256": format!("{:x}", Sha256::digest(include_bytes!("source.rs"))),
        "universeBankSha256": format!("{:x}", Sha256::digest(&bank_bytes)),
        "experimentCutoff": cfg.experiment_cutoff, "freshIndependentWallets": true,
        "referencePolicy": feature_policy(), "executionPolicy": fixed_policy(),
        "entryCapPolicy": {
            "formula": "min(original_protective_limit, original_causal_median_bid - 0.005000)",
            "applies": "new_buy_proposal_only", "recomputedAtFollowup": false,
            "nativeFundingAdmissionUnchanged": true, "reservationUsesActualCap": true,
            "trendFilter": false
        },
        "source": {
            "publicRecords": public.len(), "accountRecords": accounts.len(), "priceRecords": price_rows.len(),
            "priceRecordsCausallyConsumed": price_index,
            "publicBytes": cfg.public_source.bytes, "publicSha256": cfg.public_source.sha256,
            "accountBytes": cfg.account_source.bytes, "accountSha256": cfg.account_source.sha256,
            "priceBytes": cfg.price_source.bytes, "priceSha256": cfg.price_source.sha256,
            "publicSequence": publication.sequence(), "publicTailHash": publication.tail_hash(),
            "hours": (last - first.unwrap_or(last)) as f64 / 3_600_000., "books": books
        },
        "results": arms.iter().map(|arm| arm.report(last)).collect::<Result<Vec<_>, _>>()?,
        "fees": 0, "networkRequests": 0, "competitionOrders": 0, "profitabilityEstablished": false,
        "limitations": [
            "Offline counterfactual comparison, not observed competition fills or an active deployment.",
            "The two wallets are independent controls and must not be added together.",
            "New proposals retain the original conservative ask-plus-tick funding admission before lowering the reservation; no extra funding-only admissions are introduced.",
            "Only the newly created BUY limit changes. Existing pending orders, original reference, latency, depth, expiry, sizing, stop, target, holding time and accounting remain frozen.",
            "A missing later executable receipt does not establish that a real resting order would or would not fill; open inventory and pending intents remain reported.",
            "Future-cutoff mode primes only history before the cutoff and imports no earlier orders, fills, cash changes or inventory. Historical development results do not establish profitability."
        ]
    }))
}

#[cfg(test)]
mod entry_cap_tests;
#[cfg(test)]
mod source_tests;
