// execution/no-neutral-sell-all-v1.mjs — observed canonical one-share NO SELL
// ACK economics. ACK disclosure evidence ONLY; terminal fills/holdings/cash
// still need proof.
use crate::core::payload_hash;
use crate::receipts::parse_number_lexemes;
use baal_service_runtime::fixed::{amount, money};
use baal_service_runtime::json_text::{js_int, sha256};
use serde_json::{json, Value as V};

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("NO_NEUTRAL_SELL_ALL_{c}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

const SAME_PROCEEDS: [&str; 2] = ["fullNotionalCost", "effectiveEntryCost"];
const ZERO_EFFECTS: [&str; 5] = [
    "collateralSavings",
    "guaranteedPayoutFloorAfter",
    "outstandingAdvanceAfter",
    "collateralRepayment",
    "redemptionCredit",
];

// /^-?(0|[1-9]\d*)(\.\d{1,6})?$/
fn exact_lexeme(s: &str) -> bool {
    let t = s.strip_prefix('-').unwrap_or(s);
    let (w, f) = match t.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (t, None),
    };
    let whole_ok = !w.is_empty()
        && w.bytes().all(|b| b.is_ascii_digit())
        && (w == "0" || !w.starts_with('0'));
    let frac_ok = match f {
        None => true,
        Some(f) => !f.is_empty() && f.len() <= 6 && f.bytes().all(|b| b.is_ascii_digit()),
    };
    whole_ok && frac_ok
}

fn numbers_key(path: &[&str]) -> String {
    let arr: Vec<V> = path.iter().map(|s| json!(s)).collect();
    serde_json::to_string(&V::Array(arr)).unwrap()
}

pub fn inspect_no_sell_ack_all_economics(receipt: &V) -> Result<V, String> {
    need(
        get(receipt, "method").as_str() == Some("POST")
            && get(receipt, "endpoint").as_str() == Some("/orders")
            && js_int(get(receipt, "status")) == Some(200)
            && get(receipt, "bodyText").is_string(),
        "RECEIPT",
    )?;
    let body_text = get(receipt, "bodyText").as_str().unwrap();
    let source_body_hash = sha256(body_text.as_bytes());
    need(
        receipt
            .get("sourceBodyHash")
            .map(|v| v.as_str() == Some(source_body_hash.as_str()))
            .unwrap_or(true),
        "SOURCE_HASH",
    )?;
    let (data, numbers) = parse_number_lexemes(body_text)?;
    need(data.is_object() && data.get("all").is_some(), "MISSING_ALL")?;
    let base = json!({
        "schemaVersion": 1,
        "mode": "observed_neutral_no_sell_ack_economics_v1",
        "sourceBodyHash": source_body_hash,
        "allState": if get(&data, "all").is_null() { "null" } else { "observed_neutral_sell" },
        "executionAuthority": false,
        "newSubmissionAuthorized": false,
        "globalActivityVerified": false,
    });
    let all = get(&data, "all");
    if all.is_null() {
        let mut out = base.clone();
        out["verificationHash"] = json!(payload_hash(&base)?);
        return Ok(out);
    }
    let mut fields: Vec<&str> = SAME_PROCEEDS
        .iter()
        .chain(ZERO_EFFECTS.iter())
        .copied()
        .collect();
    fields.extend(["netBuyingPowerImpact", "relationshipIds", "componentId"]);
    fields.sort();
    let mut keys: Vec<&str> = all
        .as_object()
        .map(|m| m.keys().map(|k| k.as_str()).collect())
        .unwrap_or_default();
    keys.sort();
    need(all.is_object() && !all.is_array() && keys == fields, "FIELDS")?;
    need(
        get(&data, "side").as_str() == Some("no")
            && get(&data, "action").as_str() == Some("sell")
            && js_int(get(&data, "quantity")) == Some(1)
            && js_int(get(&data, "quantityTraded")) == Some(1)
            && js_int(get(&data, "remainingQuantity")) == Some(0)
            && get(&data, "open") == &V::Bool(false),
        "OBSERVED_CANONICAL_FILLED_NO_SELL",
    )?;
    need(
        get(all, "relationshipIds").as_array().map(|a| a.is_empty()) == Some(true)
            && get(all, "componentId").is_null(),
        "RELATIONSHIP",
    )?;
    let exact = |path: &[&str]| -> Result<i128, String> {
        let raw = numbers
            .get(&numbers_key(path))
            .ok_or_else(|| "NO_NEUTRAL_SELL_ALL_EXACT_NUMERIC_SOURCE".to_string())?;
        need(exact_lexeme(raw), "EXACT_NUMERIC_SOURCE")?;
        amount(raw)
    };
    for k in ["quantity", "quantityTraded", "remainingQuantity"] {
        need(
            exact(&[k])? == js_int(get(&data, k)).unwrap_or(i64::MIN) as i128 * 1_000_000,
            "EXACT_QUANTITY",
        )?;
    }
    let proceeds = exact(&["totalCost"])?;
    need(proceeds > 0 && proceeds <= 995_000 && proceeds % 5_000 == 0, "PROCEEDS")?;
    for k in SAME_PROCEEDS {
        need(exact(&["all", k])? == proceeds, "PROCEEDS_MISMATCH")?;
    }
    need(exact(&["all", "netBuyingPowerImpact"])? == -proceeds, "RELEASE_SIGN_OR_PROCEEDS")?;
    for k in ZERO_EFFECTS {
        need(exact(&["all", k])? == 0, "NONZERO_EFFECT")?;
    }
    let result = json!({
        "schemaVersion": 1,
        "mode": "observed_neutral_no_sell_ack_economics_v1",
        "sourceBodyHash": source_body_hash,
        "allState": "observed_neutral_sell",
        "executionAuthority": false,
        "newSubmissionAuthorized": false,
        "globalActivityVerified": false,
        "ackNotional": money(proceeds),
        "fullNotionalCost": money(proceeds),
        "effectiveEntryCost": money(proceeds),
        "netBuyingPowerImpact": money(-proceeds),
        "relationshipIds": [],
        "componentId": V::Null,
        "collateralSavings": "0.000000",
        "guaranteedPayoutFloorAfter": "0.000000",
        "outstandingAdvanceAfter": "0.000000",
        "collateralRepayment": "0.000000",
        "redemptionCredit": "0.000000",
        "cashDeltaInterpretation": "requires_separate_complete_scoped_no_sell_reconciliation",
    });
    let mut out = result.clone();
    out["verificationHash"] = json!(payload_hash(&result)?);
    Ok(out)
}
