//! Read scheduling only. Native risk, account authority, freshness and the
//! sole Transport's shared admission budget remain unchanged.
use baal_transport_service::Scope;
use moloch_replay::session::Session;
use std::collections::BTreeSet;
use crate::feedback::Result;

/// Derived from the currently replayed native policy, never the original
/// Control's pre-adoption maximumMarkets. Not an exposure or write permit.
pub struct ReadBudget {
    allowed: BTreeSet<String>,
    native_required: BTreeSet<String>,
    maximum_managed_markets: usize,
    maximum_discovery_reads: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ReadOrder {
    pub scopes: Vec<String>,
    /// Invalid inputs retain the complete original order. The worker may log
    /// this diagnostic; it must not replace a native ownership/reconcile gate.
    pub diagnostic: Option<&'static str>,
}

fn valid_id(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
}

impl ReadBudget {
    pub fn from_bank(bank: &Session, scope: &Scope) -> Result<Self> {
        if scope.tournament_id.is_empty() || scope.exchanges.is_empty()
            || scope.exchanges.len() > 4096
            || scope.exchanges.iter().any(|(ex, market)| !valid_id(ex) || !valid_id(market)) {
            return Err("THROUGHPUT_DISCOVERY_SCOPE".into());
        }
        let maximum_managed_markets = bank.engine().policy()["maximumMarkets"]
            .as_u64().filter(|n| *n > 0 && *n <= 4096)
            .ok_or("THROUGHPUT_DISCOVERY_CAPACITY")? as usize;
        let maximum_discovery_reads = bank.engine().fresh_market_limit();
        if maximum_discovery_reads == 0 || maximum_discovery_reads > maximum_managed_markets
            || maximum_managed_markets > scope.exchanges.len() {
            return Err("THROUGHPUT_DISCOVERY_CAPACITY".into());
        }
        if bank.live_account_seed().is_some_and(|seed| seed.get("tournamentId")
            .is_some_and(|t| t != scope.tournament_id.as_str()))
            || bank.engine().live_market_capacity_record().is_some_and(|record|
                record["bankScope"]["tournamentId"] != scope.tournament_id.as_str()) {
            return Err("THROUGHPUT_DISCOVERY_TOURNAMENT".into());
        }
        let allowed: BTreeSet<_> = scope.exchanges.keys().cloned().collect();
        let native_required = bank.engine().required_exchanges();
        if !native_required.is_subset(&allowed) {
            return Err("THROUGHPUT_DISCOVERY_OWNED_SCOPE".into());
        }
        // Existing native records establish exchange→market binding without
        // cloning every full-depth book or inferring inherited ancestry.
        if bank.engine().order_records().iter().any(|q|
            q["retired"] != true && q["exchangeId"].as_str().is_none_or(|ex|
                scope.exchanges.get(ex).is_none_or(|market| q["marketId"] != market.as_str()))) {
            return Err("THROUGHPUT_DISCOVERY_ORDER_SCOPE".into());
        }
        Ok(Self { allowed, native_required, maximum_managed_markets, maximum_discovery_reads })
    }
    pub fn maximum_managed_markets(&self) -> usize { self.maximum_managed_markets }
    pub fn maximum_discovery_reads(&self) -> usize { self.maximum_discovery_reads }

    /// Reorder only already selected scopes: selected urgent owned first, then
    /// fresh/owned alternation. Relative order within all three partitions is
    /// unchanged. No extra read, dropped selected owned scope or silent trim.
    ///
    /// Gaps between nonurgent owned reads gain at most one fresh packet, but a
    /// later owned read's absolute position can move by multiple fresh packets.
    /// The caller must still reconcile before the loop and at its existing
    /// deadline, and check STOP/stream/source/account authority between scopes.
    pub fn order(&self, selected: &[String], required: &BTreeSet<String>,
        urgent_owned: &BTreeSet<String>) -> ReadOrder {
        let original = |reason| ReadOrder { scopes: selected.to_vec(), diagnostic: Some(reason) };
        let selected_set: BTreeSet<_> = selected.iter().cloned().collect();
        if selected_set.len() != selected.len() || !selected_set.is_subset(&self.allowed)
            || !required.is_subset(&self.allowed) || !urgent_owned.is_subset(required)
            || !self.native_required.is_subset(required) {
            return original("THROUGHPUT_DISCOVERY_SELECTION_SCOPE");
        }
        if selected.len() > self.maximum_managed_markets
            || selected.iter().filter(|ex| !required.contains(*ex)).count() > self.maximum_discovery_reads {
            return original("THROUGHPUT_DISCOVERY_SELECTION_CAPACITY");
        }
        let mut out: Vec<_> = selected.iter().filter(|ex| urgent_owned.contains(*ex)).cloned().collect();
        let mut owned = selected.iter().filter(|ex| required.contains(*ex) && !urgent_owned.contains(*ex));
        let mut fresh = selected.iter().filter(|ex| !required.contains(*ex));
        loop {
            let next_fresh = fresh.next();
            let next_owned = owned.next();
            if let Some(ex) = next_fresh { out.push(ex.clone()); }
            if let Some(ex) = next_owned { out.push(ex.clone()); }
            if next_fresh.is_none() && next_owned.is_none() { break; }
        }
        debug_assert_eq!(out.len(), selected.len());
        ReadOrder { scopes: out, diagnostic: None }
    }
}
