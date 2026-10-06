// execution/no-neutral-sell-all-v1.mjs — observed canonical one-share NO SELL
// ACK 2026-10-06T15:22:50.772Z, original raw
// SHA92110a984282511f5ef57ad7068aacc081157fbebb7c1a4f386433b97890d976.
// Its positive effectiveEntryCost is preserved, never interpreted as a debit.
// Negative netBuyingPowerImpact reports released buying power. This helper is
// ACK disclosure evidence ONLY; terminal fills/holdings/cash still need proof.
use baal_execution::core::payload_hash;
use baal_execution::receipts::parse_number_lexemes;
use baal_paper::fixed::{big_amount, money};
use baal_service_runtime::json_text::sha256;
use num_bigint::BigInt;
use serde_json::{json, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("NO_NEUTRAL_SELL_ALL_{code}")); }
    Ok(())
}

const SAME_PROCEEDS: [&str; 2] = ["fullNotionalCost", "effectiveEntryCost"];
const ZERO_EFFECTS: [&str; 5] = [
    "collateralSavings",
    "guaranteedPayoutFloorAfter",
    "outstandingAdvanceAfter",
    "collateralRepayment",
    "redemptionCredit",
];

fn fields_sorted() -> Vec<String> {
    let mut f: Vec<String> = SAME_PROCEEDS
        .iter()
        .chain(ZERO_EFFECTS.iter())
        .chain(["netBuyingPowerImpact", "relationshipIds", "componentId"].iter())
        .map(|s| s.to_string())
        .collect();
    f.sort();
    f
}

pub fn inspect_no_sell_ack_all_economics(receipt: &V) -> Result<V, String> {
    need(
        receipt["method"] == json!("POST") && receipt["endpoint"] == json!("/orders") && receipt["status"] == json!(200) && receipt["bodyText"].is_string(),
        "RECEIPT",
    )?;
    let body_text = receipt["bodyText"].as_str().unwrap();
    let source_body_hash = sha256(body_text.as_bytes());
    need(
        receipt["sourceBodyHash"].is_null() || receipt["sourceBodyHash"] == json!(source_body_hash),
        "SOURCE_HASH",
    )?;
    let (data, numbers) = parse_number_lexemes(body_text).map_err(|e| e)?;
    need(data.is_object() && data.as_object().unwrap().contains_key("all"), "MISSING_ALL")?;
    let mut base = serde_json::Map::new();
    base.insert("schemaVersion".into(), json!(1));
    base.insert("mode".into(), json!("observed_neutral_no_sell_ack_economics_v1"));
    base.insert("sourceBodyHash".into(), json!(source_body_hash));
    base.insert("allState".into(), json!(if data["all"].is_null() { "null" } else { "observed_neutral_sell" }));
    base.insert("executionAuthority".into(), json!(false));
    base.insert("newSubmissionAuthorized".into(), json!(false));
    base.insert("globalActivityVerified".into(), json!(false));
    if data["all"].is_null() {
        let mut out = V::Object(base.clone());
        out["verificationHash"] = json!(payload_hash(&V::Object(base))?);
        return Ok(out);
    }
    let all = &data["all"];
    let mut keys: Vec<String> = all.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    keys.sort();
    need(
        all.is_object() && !all.is_array() && keys.join("|") == fields_sorted().join("|"),
        "FIELDS",
    )?;
    need(
        data["side"] == json!("no")
            && data["action"] == json!("sell")
            && data["quantity"] == json!(1)
            && data["quantityTraded"] == json!(1)
            && data["remainingQuantity"] == json!(0)
            && data["open"] == json!(false),
        "OBSERVED_CANONICAL_FILLED_NO_SELL",
    )?;
    need(
        all["relationshipIds"].as_array().is_some_and(|a| a.is_empty()) && all["componentId"].is_null(),
        "RELATIONSHIP",
    )?;
    let exact = |path: &[&str]| -> Result<BigInt, String> {
        let key = baal_service_runtime::json_text::js_stringify(&json!(path));
        let raw = numbers.get(&key).cloned().unwrap_or_default();
        // /^-?(0|[1-9]\d*)(\.\d{1,6})?$/
        let ok = !raw.is_empty()
            && {
                let (neg, rest) = raw.strip_prefix('-').map(|r| (true, r)).unwrap_or((false, raw.as_str()));
                let (int_part, frac) = rest.split_once('.').map(|(a, b)| (a, Some(b))).unwrap_or((rest, None));
                !int_part.is_empty()
                    && int_part.bytes().all(|b| b.is_ascii_digit())
                    && (int_part == "0" || !int_part.starts_with('0'))
                    && frac.map(|f| !f.is_empty() && f.len() <= 6 && f.bytes().all(|b| b.is_ascii_digit())).unwrap_or(true)
                    && !(neg && int_part.is_empty())
            };
        need(ok, "EXACT_NUMERIC_SOURCE")?;
        big_amount(&raw).map_err(|e| e)
    };
    for k in ["quantity", "quantityTraded", "remainingQuantity"] {
        need(exact(&[k])? == BigInt::from(data[k].as_i64().unwrap_or(i64::MIN)) * BigInt::from(1_000_000), "EXACT_QUANTITY")?;
    }
    let proceeds = exact(&["totalCost"])?;
    need(
        proceeds > BigInt::from(0) && proceeds <= BigInt::from(995_000) && &proceeds % BigInt::from(5_000) == BigInt::from(0),
        "PROCEEDS",
    )?;
    for k in SAME_PROCEEDS {
        need(exact(&["all", k])? == proceeds, "PROCEEDS_MISMATCH")?;
    }
    need(exact(&["all", "netBuyingPowerImpact"])? == -&proceeds, "RELEASE_SIGN_OR_PROCEEDS")?;
    for k in ZERO_EFFECTS {
        need(exact(&["all", k])? == BigInt::from(0), "NONZERO_EFFECT")?;
    }
    let mut result = V::Object(base);
    result["ackNotional"] = json!(money(&proceeds));
    result["fullNotionalCost"] = json!(money(&proceeds));
    result["effectiveEntryCost"] = json!(money(&proceeds));
    result["netBuyingPowerImpact"] = json!(money(&-&proceeds));
    result["relationshipIds"] = json!([]);
    result["componentId"] = V::Null;
    result["collateralSavings"] = json!("0.000000");
    result["guaranteedPayoutFloorAfter"] = json!("0.000000");
    result["outstandingAdvanceAfter"] = json!("0.000000");
    result["collateralRepayment"] = json!("0.000000");
    result["redemptionCredit"] = json!("0.000000");
    result["cashDeltaInterpretation"] = json!("requires_separate_complete_scoped_no_sell_reconciliation");
    result["verificationHash"] = json!(payload_hash(&result)?);
    Ok(result)
}
