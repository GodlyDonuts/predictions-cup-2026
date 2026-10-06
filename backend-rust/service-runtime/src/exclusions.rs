use crate::atomic::need;
use crate::json_text::js_int;
use serde_json::Value as V;
use std::collections::BTreeSet;

// transport-runtime.mjs exclusionSet(positions, orders): every row must map to
// an exchange id (numeric string, safe integer) and a safe-integer quantity.
// Nonzero positions and every open order exclude that exchange id.
fn validate_row(row: &V) -> Result<(&str, i64), String> {
    let ok = row.is_object() && !row.is_array();
    need(ok, "Unmappable account exposure")?;
    let id = row.get("exchangeId").and_then(V::as_str).unwrap_or("");
    let id_ok = !id.is_empty()
        && !id.starts_with('0')
        && id.bytes().all(|b| b.is_ascii_digit())
        && id.parse::<i64>().is_ok_and(|x| x <= 9_007_199_254_740_991);
    let qty = js_int(row.get("quantity").unwrap_or(&V::Null));
    need(id_ok && qty.is_some(), "Unmappable account exposure")?;
    Ok((id, qty.unwrap()))
}
pub fn exclusion_set(positions: &V, orders: &V) -> Result<BTreeSet<String>, String> {
    need(
        positions.is_array() && orders.is_array(),
        "Incomplete account reconciliation",
    )?;
    let mut ids = BTreeSet::new();
    for row in positions.as_array().unwrap() {
        let (id, qty) = validate_row(row)?;
        if qty != 0 {
            ids.insert(id.to_string());
        }
    }
    for row in orders.as_array().unwrap() {
        let (id, qty) = validate_row(row)?;
        need(qty > 0, "Unmappable account exposure")?;
        ids.insert(id.to_string());
    }
    Ok(ids)
}
