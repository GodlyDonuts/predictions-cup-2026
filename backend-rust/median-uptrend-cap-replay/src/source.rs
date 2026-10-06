//! Sealed, read-only inputs. The causal lookup follows the corrected trend-v2
//! reader: account publication order does not imply book request order.
use super::*;
use std::{fs::File, io::{BufRead, BufReader, Read}};

fn exact(value: &V, keys: &[&str], code: &str) -> Result<(), String> {
    need(value.as_object().is_some_and(|object|
        object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))), code)
}

fn projection_clock(value: &V) -> Result<i64, String> {
    let at = clock(value)?;
    need(at >= 0 && chrono::DateTime::from_timestamp_millis(at).is_some_and(|date|
        value.as_str() == Some(date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true).as_str())),
        "SOURCE_CANONICAL_CLOCK")?;
    Ok(at)
}

fn scope_id(value: &V) -> bool {
    value.as_str().is_some_and(|id| !id.is_empty() && !id.starts_with('0')
        && id.bytes().all(|c| c.is_ascii_digit())
        && id.parse::<u64>().is_ok_and(|n| n <= 9_007_199_254_740_991))
}

pub(super) fn validate_bank_scope(bank: &V) -> Result<(), String> {
    need(bank["schemaVersion"] == 1 && bank["mode"] == "fixed_rolling_median_prospective_paper_v1"
        && bank["thresholds"] == json!([1, 2, 4]) && bank["policy"] == fixed_policy()
        && bank["featurePolicy"] == feature_policy(), "BANK_FROZEN_POLICY")?;
    let universe = &bank["universe"];
    need(universe["tournamentId"] == "bda92870-621e-47b0-bc3c-3602c5c26f55", "BANK_TOURNAMENT_SCOPE")?;
    let mut body = universe.clone();
    let object = body.as_object_mut().ok_or("BANK_UNIVERSE_SCOPE")?;
    object.shift_remove("hash");
    need(universe["hash"].as_str().is_some_and(|hash| hash.len() == 64
        && hash.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)))
        && universe["hash"] == digest(&body), "BANK_UNIVERSE_HASH")?;
    let entries = universe["exchanges"].as_array().ok_or("BANK_UNIVERSE_SCOPE")?;
    need(!entries.is_empty() && entries.len() <= 10_000, "BANK_UNIVERSE_SCOPE")?;
    let mut markets = std::collections::BTreeSet::new();
    let mut exchanges = std::collections::BTreeSet::new();
    for entry in entries {
        exact(entry, &["marketId", "exchangeId", "option"], "BANK_EXCHANGE_SCOPE")?;
        need(scope_id(&entry["marketId"]) && scope_id(&entry["exchangeId"]) && entry["option"] == "YES"
            && markets.insert(entry["marketId"].as_str().unwrap())
            && exchanges.insert(entry["exchangeId"].as_str().unwrap()), "BANK_EXCHANGE_SCOPE")?;
    }
    Ok(())
}

pub(super) fn rows(pin: &Pin) -> Result<Vec<V>, String> {
    need(pin.path.is_absolute() && pin.sha256.len() == 64
        && pin.sha256.bytes().all(|c| c.is_ascii_hexdigit()), "SEALED_ABSOLUTE_SOURCE_REQUIRED")?;
    let file = File::open(&pin.path).map_err(|_| "SOURCE_READ")?;
    need(file.metadata().map_err(|_| "SOURCE_METADATA")?.len() >= pin.bytes, "SOURCE_TRUNCATED")?;
    let mut reader = BufReader::new(file.take(pin.bytes));
    let mut hash = Sha256::new();
    let mut raw = Vec::new();
    let mut result = Vec::new();
    loop {
        raw.clear();
        let n = reader.read_until(b'\n', &mut raw).map_err(|_| "SOURCE_READ")?;
        if n == 0 { break; }
        need(n <= 16 * 1024 * 1024 && raw.last() == Some(&10), "SOURCE_PARTIAL_OR_OVERSIZE")?;
        hash.update(&raw);
        result.push(serde_json::from_slice(&raw).map_err(|_| "SOURCE_JSON")?);
    }
    need(format!("{:x}", hash.finalize()) == pin.sha256, "SOURCE_PREFIX_SHA")?;
    Ok(result)
}

pub(super) fn validate_accounts(rows: &[V], universe: &V) -> Result<(), String> {
    let mut previous = String::new();
    let mut known = 0;
    let mut last_positions = 0;
    let mut last_orders = 0;
    for (i, row) in rows.iter().enumerate() {
        exact(row, &["sequence", "previous", "at", "event", "state", "hash"], "ACCOUNT_ROW_SHAPE")?;
        exact(&row["event"], &["type", "snapshot"], "ACCOUNT_EVENT_SHAPE")?;
        let mut body = row.clone();
        body.as_object_mut().ok_or("ACCOUNT_BODY")?.shift_remove("hash");
        let snapshot = &row["event"]["snapshot"];
        exact(snapshot, &["tournamentId", "universeHash", "positionsAt", "ordersAt", "expiresAt",
            "excludedMarketIds", "complete"], "ACCOUNT_SNAPSHOT_SHAPE")?;
        let at = projection_clock(&row["at"])?;
        let positions = projection_clock(&snapshot["positionsAt"])?;
        let orders = projection_clock(&snapshot["ordersAt"])?;
        need(row["sequence"] == i + 1 && row["previous"] == previous
            && row["hash"] == digest(&body) && row["state"].is_null()
            && row["event"]["type"] == "market_account_eligibility"
            && snapshot["universeHash"] == universe["hash"]
            && snapshot["tournamentId"] == universe["tournamentId"]
            && snapshot["complete"] == true && positions <= orders && orders <= at
            && at >= known && positions >= last_positions && orders >= last_orders
            && projection_clock(&snapshot["expiresAt"])? == positions + 20_000,
            "ACCOUNT_CHAIN_SCOPE_CLOCK")?;
        let mut seen = std::collections::BTreeSet::new();
        for id in snapshot["excludedMarketIds"].as_array().ok_or("ACCOUNT_EXCLUSIONS")? {
            need(scope_id(id) && seen.insert(id.as_str().unwrap())
                && universe["exchanges"].as_array().ok_or("BANK_UNIVERSE_SCOPE")?
                    .iter().any(|entry| entry["marketId"] == *id), "ACCOUNT_EXCLUSIONS")?;
        }
        previous = row["hash"].as_str().ok_or("ACCOUNT_HASH")?.into();
        known = at;
        last_positions = positions;
        last_orders = orders;
    }
    Ok(())
}

pub(super) fn causal_account(accounts: &[V], request: i64, available: i64) -> Option<&V> {
    let count = accounts.partition_point(|row| clock(&row["at"]).unwrap() <= request.min(available));
    count.checked_sub(1).map(|i| &accounts[i]["event"]["snapshot"])
}

pub(super) fn eligible(accounts: &[V], quote: &Quote) -> bool {
    causal_account(accounts, quote.request, quote.at).is_some_and(|snapshot| {
        quote.at >= clock(&snapshot["positionsAt"]).unwrap()
            && quote.at >= clock(&snapshot["ordersAt"]).unwrap()
            && quote.at - clock(&snapshot["positionsAt"]).unwrap() <= 20_000
            && quote.at - clock(&snapshot["ordersAt"]).unwrap() <= 20_000
            && snapshot["excludedMarketIds"].as_array()
                .is_some_and(|ids| !ids.contains(&json!(quote.market)))
    })
}

pub(super) fn bid_tick(value: &V) -> Option<i128> {
    let n = value.as_f64()?;
    for tick in 1..200 {
        let p = tick as f64 / 200.;
        let other = (200 - tick) as f64 / 200.;
        if [p, 1. - other, 1. - (1. - p)].contains(&n) { return Some(tick * 5000); }
    }
    None
}

pub(super) fn validate_prices(rows: &[V], universe: &V) -> Result<(), String> {
    let mut previous = String::new();
    let mut last = 0;
    let mut last_received = 0;
    for (i, row) in rows.iter().enumerate() {
        exact(row, &["sequence", "previous", "at", "event", "state", "hash"], "PRICE_ROW_SHAPE")?;
        exact(&row["event"], &["type", "snapshot"], "PRICE_EVENT_SHAPE")?;
        let mut body = row.clone();
        body.as_object_mut().ok_or("PRICE_BODY")?.shift_remove("hash");
        let snapshot = &row["event"]["snapshot"];
        exact(snapshot, &["tournamentId", "universeHash", "requestStartedAt", "receivedAt", "sourceTimestamp",
            "depthAvailable", "executable", "quotes", "missingIds"], "PRICE_SNAPSHOT_SHAPE")?;
        let at = projection_clock(&row["at"])?;
        let request = projection_clock(&snapshot["requestStartedAt"])?;
        let received = projection_clock(&snapshot["receivedAt"])?;
        need(row["sequence"] == i + 1 && row["previous"] == previous
            && row["hash"] == digest(&body) && row["state"].is_null()
            && row["event"]["type"] == "market_price_observation" && at >= last
            && snapshot["tournamentId"] == universe["tournamentId"]
            && snapshot["universeHash"] == universe["hash"] && snapshot["sourceTimestamp"].is_null()
            && snapshot["depthAvailable"] == false && snapshot["executable"] == false
            && request <= received && received <= at && received >= last_received, "PRICE_CHAIN_SCOPE_CLOCK")?;
        let mut seen = std::collections::BTreeSet::new();
        for quote in snapshot["quotes"].as_array().ok_or("PRICE_QUOTES")? {
            exact(quote, &["exchangeId", "marketId", "option", "latestPrice", "bestBid", "bestAsk", "spread", "crossed"],
                "PRICE_QUOTE_SHAPE")?;
            need(seen.insert(quote["exchangeId"].as_str().ok_or("PRICE_EXCHANGE")?)
                && universe["exchanges"].as_array().ok_or("UNIVERSE")?.iter().any(|entry|
                    entry["marketId"] == quote["marketId"] && entry["exchangeId"] == quote["exchangeId"]
                        && entry["option"] == quote["option"]), "PRICE_QUOTE_SCOPE")?;
            for field in ["latestPrice", "bestBid", "bestAsk", "spread"] {
                need(quote[field].is_null() || quote[field].as_f64().is_some_and(|number|
                    number.is_finite() && number <= 1. && number >= if field == "spread" { -1. } else { 0. }),
                    "PRICE_QUOTE_VALUE")?;
            }
            let crossed = quote["bestBid"].as_f64().zip(quote["bestAsk"].as_f64())
                .is_some_and(|(bid, ask)| bid >= ask);
            need(quote["crossed"] == crossed, "PRICE_CROSS_FLAG")?;
        }
        for id in snapshot["missingIds"].as_array().ok_or("PRICE_MISSING_IDS")? {
            need(id.as_str().is_some_and(|id| seen.insert(id))
                && universe["exchanges"].as_array().ok_or("BANK_UNIVERSE_SCOPE")?
                    .iter().any(|entry| entry["exchangeId"] == *id), "PRICE_MISSING_IDS")?;
        }
        need(!seen.is_empty() && seen.len() <= 100, "PRICE_BATCH_SIZE")?;
        previous = row["hash"].as_str().ok_or("PRICE_HASH")?.into();
        last = at;
        last_received = received;
    }
    Ok(())
}
