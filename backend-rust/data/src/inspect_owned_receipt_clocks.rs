// data/inspect-owned-receipt-clocks.mjs — read-only local timing diagnostic;
// emits no account bodies or credentials.
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Value as V};
use std::path::PathBuf;

fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
// Date.parse: NaN on failure; NaN differences serialize as null in JSON.
fn dp(v: &V) -> f64 {
    v.as_str().and_then(parse_iso_ms).unwrap_or(f64::NAN)
}
fn num(x: f64) -> V {
    serde_json::Number::from_f64(x).map(V::Number).unwrap_or(V::Null)
}

pub fn run() -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let file = runtime
        .join("baal-scaled-live-runs")
        .join("baal-scaled-no-20261006-human")
        .join("controller")
        .join("baal-scaled-no-journal.jsonl");
    let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let trimmed = text.trim_end();
    let last = trimmed[trimmed.rfind('\n').map(|i| i + 1).unwrap_or(0)..].to_string();
    let row: V = serde_json::from_str(&last).map_err(|e| e.to_string())?;
    let inventory = get(get(&row, "state"), "inventory")
        .as_object()
        .ok_or_else(|| "Cannot convert undefined or null to object".to_string())?;
    let scope = get(get(&row, "state"), "scope");
    let mut results = Vec::new();
    for (exchange_id, s) in inventory {
        let p = get(s, "proof");
        let quantity = get(p, "remainingOwnedQuantity");
        if !(quantity.as_i64().map(|q| q > 0).unwrap_or(false)
            || quantity.as_u64().map(|q| q > 0).unwrap_or(false)
            || quantity.as_f64().map(|q| q > 0.0).unwrap_or(false))
        {
            continue;
        }
        let a = get(s, "after");
        let at = dp(get(p, "readAt"));
        let mut binding = scope.as_object().cloned().unwrap_or_default();
        binding.insert("exchangeId".into(), json!(exchange_id));
        let account_validation =
            match crate::no_rest_sell_reconciliation_v2::inspect_scoped_rest_no_account(&json!({
                "account": a,
                "binding": V::Object(binding),
                "readAt": get(p, "readAt"),
            })) {
                Ok(_) => json!("passed"),
                Err(e) => json!(e),
            };
        let mut receipts = vec![get(a, "cash").clone(), get(a, "positions").clone()];
        for page in ["openOrdersPages", "scopedOrdersPages"] {
            if let Some(pages) = get(a, page).as_array() {
                receipts.extend(pages.iter().cloned());
            }
        }
        let receipts: Vec<V> = receipts
            .iter()
            .map(|r| {
                json!({
                    "endpoint": get(r, "endpoint"),
                    "durationMs": num(dp(get(r, "receivedAt")) - dp(get(r, "requestStartedAt"))),
                    "ageAtProofMs": num(at - dp(get(r, "receivedAt"))),
                    "startAgeAtProofMs": num(at - dp(get(r, "requestStartedAt"))),
                })
            })
            .collect();
        results.push(json!({
            "exchangeId": exchange_id,
            "quantity": get(p, "remainingOwnedQuantity"),
            "proofAt": get(p, "readAt"),
            "accountValidation": account_validation,
            "receipts": receipts,
        }));
    }
    Ok(json!({ "sourceSequence": get(&row, "sequence"), "results": results }))
}
