//! Scope validation shared by the deployment audit and network admission path.
use serde_json::Value;
use std::collections::BTreeSet;
pub fn validate(
    pairs: &Value,
    policy: &Value,
    exposure: &BTreeSet<String>,
) -> Result<(), &'static str> {
    let rows = pairs.as_array().ok_or("CAPTURE_SCOPES")?;
    if rows.is_empty() || rows.len() > 4 {
        return Err("CAPTURE_SCOPE_COUNT");
    }
    let mut markets = BTreeSet::new();
    let mut exchanges = BTreeSet::new();
    for row in rows {
        for field in ["marketId", "exchangeId"] {
            let id = row[field].as_str().ok_or("CAPTURE_SCOPE_ID")?;
            if id.is_empty() || id.starts_with('0') || !id.bytes().all(|b| b.is_ascii_digit()) {
                return Err("CAPTURE_SCOPE_ID");
            }
        }
        if !markets.insert(row["marketId"].as_str().unwrap())
            || !exchanges.insert(row["exchangeId"].as_str().unwrap())
        {
            return Err("CAPTURE_SCOPE_DUPLICATE");
        }
    }
    if let Some(ids) = policy.get("entryExchangeIds") {
        for id in ids.as_array().ok_or("ENTRY_SCOPES")? {
            if !exchanges.contains(id.as_str().ok_or("ENTRY_SCOPE_ID")?) {
                return Err("PAPER_ENTRY_OUTSIDE_CAPTURE_SCOPE");
            }
        }
    }
    for id in exposure {
        if !exchanges.contains(id.as_str()) {
            return Err("PAPER_EXPOSURE_OUTSIDE_CAPTURE_SCOPE");
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn historical_example_cannot_silently_starve_selected_strategy() {
        let pairs = json!([{"marketId":"233","exchangeId":"922"}]);
        let policy = json!({"entryExchangeIds":["998","1044","1048","846"]});
        assert_eq!(
            validate(&pairs, &policy, &BTreeSet::new()),
            Err("PAPER_ENTRY_OUTSIDE_CAPTURE_SCOPE")
        );
    }
    #[test]
    fn funding_liabilities_must_remain_in_scope() {
        let pairs = json!([{"marketId":"309","exchangeId":"998"}]);
        let policy = json!({"entryExchangeIds":["998"]});
        assert!(validate(&pairs, &policy, &BTreeSet::new()).is_ok());
        assert_eq!(
            validate(&pairs, &policy, &BTreeSet::from(["922".into()])),
            Err("PAPER_EXPOSURE_OUTSIDE_CAPTURE_SCOPE")
        );
    }
    #[test]
    fn ambiguous_and_oversized_scope_rejected() {
        let row = json!({"marketId":"1","exchangeId":"2"});
        assert_eq!(
            validate(&json!([row, row]), &json!({}), &BTreeSet::new()),
            Err("CAPTURE_SCOPE_DUPLICATE")
        );
        assert_eq!(
            validate(&json!([]), &json!({}), &BTreeSet::new()),
            Err("CAPTURE_SCOPE_COUNT")
        );
        assert_eq!(
            validate(
                &json!([{"marketId":"01","exchangeId":"2"}]),
                &json!({}),
                &BTreeSet::new()
            ),
            Err("CAPTURE_SCOPE_ID")
        );
    }
}
