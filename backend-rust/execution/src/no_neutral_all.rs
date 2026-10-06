// execution/no-neutral-all.mjs — read-only proof of the ACK's disclosed
// economics. The original raw body and its hash are never rewritten.
use crate::core::payload_hash;
use crate::receipts::parse_number_lexemes;
use baal_paper::fixed::{big_amount, money};
use baal_service_runtime::json_text::sha256;
use num_bigint::BigInt;
use serde_json::{json, Value as V};

const EQUAL_PRINCIPAL: [&str; 3] =
    ["fullNotionalCost", "effectiveEntryCost", "netBuyingPowerImpact"];
const ZERO_EFFECTS: [&str; 5] = [
    "collateralSavings",
    "guaranteedPayoutFloorAfter",
    "outstandingAdvanceAfter",
    "collateralRepayment",
    "redemptionCredit",
];

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("NO_NEUTRAL_ALL_{code}"))
    }
}

pub fn inspect_no_ack_all_economics(receipt: &V) -> Result<V, String> {
    need(
        receipt["method"].as_str() == Some("POST")
            && receipt["endpoint"].as_str() == Some("/orders")
            && receipt["status"].as_i64() == Some(200)
            && receipt["bodyText"].as_str().is_some(),
        "RECEIPT",
    )?;
    let body_text = receipt["bodyText"].as_str().unwrap();
    let source_body_hash = sha256(body_text.as_bytes());
    need(
        receipt["sourceBodyHash"].is_null()
            || receipt["sourceBodyHash"].as_str() == Some(source_body_hash.as_str()),
        "SOURCE_HASH",
    )?;
    let (data, numbers) = parse_number_lexemes(body_text)?;
    need(data.is_object() && data.get("all").is_some(), "MISSING_ALL")?;
    let all_is_null = data["all"].is_null();
    let proof = json!({
        "schemaVersion": 1,
        "mode": "strict_neutral_no_ack_economics_v1",
        "sourceBodyHash": source_body_hash,
        "allState": if all_is_null { "null" } else { "neutral" },
        "executionAuthority": false,
        "newSubmissionAuthorized": false,
        "globalActivityVerified": false,
    });
    if all_is_null {
        let mut out = proof.clone();
        out["verificationHash"] = json!(payload_hash(&proof)?);
        return Ok(out);
    }
    let all = &data["all"];
    let mut fields: Vec<String> = EQUAL_PRINCIPAL
        .iter()
        .chain(ZERO_EFFECTS.iter())
        .map(|s| s.to_string())
        .collect();
    fields.push("relationshipIds".into());
    fields.push("componentId".into());
    fields.sort();
    need(
        all.is_object()
            && all
                .as_object()
                .map(|m| {
                    let mut k: Vec<&String> = m.keys().collect();
                    k.sort();
                    k.into_iter().cloned().collect::<Vec<_>>().join("|")
                })
                .map(|got| got == fields.join("|"))
                .unwrap_or(false),
        "FIELDS",
    )?;
    need(
        data["side"].as_str() == Some("no")
            && data["action"].as_str() == Some("buy")
            && data["quantity"].as_i64() == Some(1)
            && [0, 1].contains(&data["quantityTraded"].as_i64().unwrap_or(-1)),
        "CANONICAL_NO_BUY",
    )?;
    need(
        all["relationshipIds"].as_array().is_some_and(|a| a.is_empty())
            && all["componentId"].is_null(),
        "RELATIONSHIP",
    )?;
    // /^(0|[1-9]\d*)(\.\d{1,6})?$/
    let lexeme_ok = |r: &str| -> bool {
        let (int_part, frac) = match r.split_once('.') {
            Some((i, f)) => (i, Some(f)),
            None => (r, None),
        };
        let int_ok = !int_part.is_empty()
            && int_part.chars().all(|c| c.is_ascii_digit())
            && (int_part == "0" || !int_part.starts_with('0'));
        let frac_ok = match frac {
            None => true,
            Some(f) => !f.is_empty() && f.len() <= 6 && f.chars().all(|c| c.is_ascii_digit()),
        };
        int_ok && frac_ok
    };
    let exact = |path: &[V]| -> Result<BigInt, String> {
        let raw = numbers.get(&serde_json::to_string(path).unwrap_or_default());
        need(raw.is_some_and(|r| lexeme_ok(r)), "EXACT_NUMERIC_SOURCE")?;
        big_amount(raw.unwrap())
    };
    let principal = exact(&[json!("totalCost")])?;
    let zero = BigInt::from(0);
    need(
        principal >= zero
            && principal <= BigInt::from(995000)
            && principal.clone() % BigInt::from(5000) == zero
            && if data["quantityTraded"].as_i64() == Some(0) {
                principal == zero
            } else {
                principal > zero
            },
        "PRINCIPAL",
    )?;
    for field in EQUAL_PRINCIPAL {
        need(
            exact(&[json!("all"), json!(field)])? == principal,
            "PRINCIPAL_MISMATCH",
        )?;
    }
    for field in ZERO_EFFECTS {
        need(exact(&[json!("all"), json!(field)])? == zero, "NONZERO_EFFECT")?;
    }
    let mut result = proof.clone();
    result["ackNotional"] = json!(money(&principal));
    result["fullNotionalCost"] = json!(money(&principal));
    result["effectiveEntryCost"] = json!(money(&principal));
    result["netBuyingPowerImpact"] = json!(money(&principal));
    result["relationshipIds"] = json!([]);
    result["componentId"] = V::Null;
    result["collateralSavings"] = json!("0.000000");
    result["guaranteedPayoutFloorAfter"] = json!("0.000000");
    result["outstandingAdvanceAfter"] = json!("0.000000");
    result["collateralRepayment"] = json!("0.000000");
    result["redemptionCredit"] = json!("0.000000");
    result["verificationHash"] = json!(payload_hash(&result)?);
    Ok(result)
}
