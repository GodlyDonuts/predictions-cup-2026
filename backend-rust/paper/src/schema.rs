// paper/schema.mjs
use crate::fixed::{big_amount, big_price, big_sequence};
use baal_service_runtime::fixed::{instant, uint};
use num_bigint::BigInt;
use serde_json::Value as V;
use std::collections::HashSet;

const KINDS: &[&str] = &["snapshot", "trade", "gap", "submit", "cancel", "mark", "clock"];

fn id(value: &V, name: &str) -> Result<(), String> {
    let ok = match value.as_str() {
        Some(s) if !s.is_empty() && s.len() <= 128 => {
            s.bytes().next().map(|b| b.is_ascii_alphanumeric()).unwrap_or(false)
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b':' || b == b'.' || b == b'_' || b == b'-')
        }
        _ => false,
    };
    if ok { Ok(()) } else { Err(format!("Invalid {name}")) }
}

fn jsobj(v: &V) -> bool {
    v.is_object()
}


pub fn validate_config(c: &V) -> Result<V, String> {
    if !jsobj(c) {
        return Err("Unsupported config schema".into());
    }
    if c["schemaVersion"] != 1 {
        return Err("Unsupported config schema".into());
    }
    if big_amount(c["initialCash"].as_str().unwrap_or(""))? <= BigInt::from(0) {
        return Err("Initial cash must be positive".into());
    }
    uint(c["maxDataAgeMs"].as_f64().unwrap_or(f64::NAN), "maxDataAgeMs", 1.0)?;
    uint(c["orderLatencyMs"].as_f64().unwrap_or(f64::NAN), "orderLatencyMs", 0.0)?;
    uint(c["cancelLatencyMs"].as_f64().unwrap_or(f64::NAN), "cancelLatencyMs", 0.0)?;
    uint(c["maxPositionShares"].as_f64().unwrap_or(f64::NAN), "maxPositionShares", 1.0)?;
    let costs = &c["costs"];
    if !jsobj(costs) || costs["assumption"] != "unverified_scenario" {
        return Err("Costs must be labeled unverified_scenario".into());
    }
    let fee_bps = uint(costs["feeBps"].as_f64().unwrap_or(f64::NAN), "feeBps", 0.0)?;
    if fee_bps > 10_000.0 || big_amount(costs["feePerShare"].as_str().unwrap_or(""))? < BigInt::from(0) {
        return Err("Invalid fee scenario".into());
    }
    Ok(c.clone())
}

pub fn validate_event(e: &V) -> Result<(), String> {
    if !jsobj(e) || e["schemaVersion"] != 1 || !KINDS.contains(&e["type"].as_str().unwrap_or("")) {
        return Err("Unsupported event schema/type".into());
    }
    id(&e["eventId"], "eventId")?;
    let received = instant(e["receivedAt"].as_str().unwrap_or(""))?;
    let occurred = instant(e["occurredAt"].as_str().unwrap_or(""))?;
    if occurred > received {
        return Err("Event occurred after receipt".into());
    }
    let ty = e["type"].as_str().unwrap_or("");
    if ["snapshot", "trade", "gap", "submit", "mark"].contains(&ty) {
        id(&e["instrument"], "instrument")?;
    }
    match ty {
        "snapshot" => {
            big_sequence(e["sequence"].as_str().unwrap_or(""))?;
            if !e["authoritative"].is_boolean() {
                return Err("Snapshot authoritative flag required".into());
            }
            if !e["nextExpiryAt"].is_null() && instant(e["nextExpiryAt"].as_str().unwrap_or(""))? <= occurred {
                return Err("Snapshot has already-expired depth".into());
            }
            for side in ["bids", "asks"] {
                let levels = e[side].as_array().ok_or("Both depth arrays required")?;
                let mut prices = HashSet::new();
                for level in levels {
                    let p = big_price(level["price"].as_str().unwrap_or(""))?;
                    uint(level["quantity"].as_f64().unwrap_or(f64::NAN), "depth quantity", 1.0)?;
                    let own = uint(level["ownQuantity"].as_f64().unwrap_or(f64::NAN), "own quantity", 0.0)?;
                    if own > level["quantity"].as_f64().unwrap_or(-1.0) || !prices.insert(p.to_string()) {
                        return Err("Duplicate price or own quantity exceeds total".into());
                    }
                }
            }
            let bids = e["bids"].as_array().unwrap();
            let asks = e["asks"].as_array().unwrap();
            if !bids.is_empty() && !asks.is_empty() {
                let max_bid = bids
                    .iter()
                    .filter_map(|l| big_price(l["price"].as_str().unwrap_or("")).ok())
                    .max()
                    .unwrap_or_default();
                let min_ask = asks
                    .iter()
                    .filter_map(|l| big_price(l["price"].as_str().unwrap_or("")).ok())
                    .min()
                    .unwrap_or_default();
                if max_bid >= min_ask {
                    return Err("Crossed or locked snapshot".into());
                }
            }
        }
        "trade" => {
            big_sequence(e["sequence"].as_str().unwrap_or(""))?;
            big_sequence(e["bookSequence"].as_str().unwrap_or(""))?;
            big_price(e["price"].as_str().unwrap_or(""))?;
            uint(e["quantity"].as_f64().unwrap_or(f64::NAN), "trade quantity", 1.0)?;
            if !["buy", "sell"].contains(&e["aggressor"].as_str().unwrap_or("")) || e["evidence"] != "explicit_trade" {
                return Err("Explicit trade and aggressor evidence required".into());
            }
            if !e["externalAggressor"].is_boolean() {
                return Err("Trade externalAggressor classification required".into());
            }
        }
        "submit" => {
            id(&e["orderId"], "orderId")?;
            big_price(e["limitPrice"].as_str().unwrap_or(""))?;
            uint(e["quantity"].as_f64().unwrap_or(f64::NAN), "order quantity", 1.0)?;
            if !["buy", "sell"].contains(&e["action"].as_str().unwrap_or("")) {
                return Err("Invalid action".into());
            }
            if e["occurredAt"] != e["receivedAt"] {
                return Err("Paper actions use availability time".into());
            }
            if !e["expiresAt"].is_null() && instant(e["expiresAt"].as_str().unwrap_or(""))? <= received {
                return Err("Order expiry must follow submission".into());
            }
        }
        "cancel" => {
            id(&e["orderId"], "orderId")?;
            if occurred != received {
                return Err("Paper actions use availability time".into());
            }
        }
        "mark" => {
            let p = big_amount(e["price"].as_str().unwrap_or(""))?;
            if p < BigInt::from(0)
                || p > BigInt::from(1_000_000)
                || !["reference", "last", "tournament"].contains(&e["source"].as_str().unwrap_or(""))
            {
                return Err("Invalid mark".into());
            }
        }
        "gap" => {
            if e["reason"].as_str().map(|s| s.trim().is_empty()).unwrap_or(true) {
                return Err("Gap reason required".into());
            }
        }
        _ => {}
    }
    Ok(())
}
