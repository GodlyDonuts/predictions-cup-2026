// execution/cohort-profit-locked-depth-sweep-v2.mjs — pure economic selector:
// sweep cumulative ask depth for the best cohort-profit-locked sell candidate.
// No receipt validation, account adoption, activation or order authority.
//
// Port notes:
// - {yesAsks,requestedQuantity=null,...args}: the rest spread feeds every
//   other input key through verbatim; requestedQuantity defaults to 100.
// - candidates.sort: quantity desc, then removedPrincipalIfFilled ASC, then
//   protectedLimit ASC (numeric comparator faithfully reproduced).
// - price()/sizeCohortProfitLockedNoExitV2 errors propagate raw like the JS
//   throws; only the two level/input guards are bare COHORT_DEPTH_* codes.
use crate::cohort_profit_locked_no_exit_v2::size_cohort_profit_locked_no_exit_v2;
use baal_service_runtime::fixed::{amount, price};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Map, Value as V};

fn js_num(v: &V) -> Option<f64> {
    if v.is_i64() || v.is_u64() {
        v.as_i64().map(|x| x as f64)
    } else {
        v.as_f64()
    }
}

/// selectCohortProfitLockedDepthV2({yesAsks,requestedQuantity=null,...args})
pub fn select_cohort_profit_locked_depth_v2(input: &V) -> Result<V, String> {
    let yes_asks = match input.get("yesAsks") {
        Some(v) if v.is_array() => v.as_array().unwrap().clone(),
        _ => return Err("COHORT_DEPTH_INPUT".into()),
    };
    if yes_asks.is_empty() || yes_asks.len() > 20 {
        return Err("COHORT_DEPTH_INPUT".into());
    }
    let mut args: Map<String, V> = match input {
        V::Object(m) => m.clone(),
        _ => Map::new(),
    };
    args.shift_remove("yesAsks");
    args.shift_remove("requestedQuantity");
    let requested = js_num(input.get("requestedQuantity").unwrap_or(&V::Null)).unwrap_or(100.0);
    let ms_args = js_num(args.get("maximumSharesPerOrder").unwrap_or(&V::Null)).unwrap_or(100.0);
    let mut depth: f64 = 0.0;
    let mut previous: i128 = -1;
    let mut candidates: Vec<V> = Vec::new();
    for (through_level, l) in yes_asks.iter().enumerate() {
        let p = price(l.get("price").and_then(V::as_str).unwrap_or(""))?;
        let q = js_num(l.get("quantity").unwrap_or(&V::Null))
            .filter(|x| x.fract() == 0.0 && *x >= 1.0 && *x <= 9007199254740991.0);
        if p <= previous || q.is_none() {
            return Err("COHORT_DEPTH_LEVEL".into());
        }
        previous = p;
        depth = (depth + q.unwrap()).min(100.0);
        let mut call_args = args.clone();
        call_args.insert(
            "maximumSharesPerOrder".into(),
            json!(ms_args.min(requested) as i64),
        );
        call_args.insert(
            "yesAskLevel".into(),
            json!({"price": l.get("price"), "quantity": depth as i64}),
        );
        let proposal = size_cohort_profit_locked_no_exit_v2(&V::Object(call_args))?;
        let proposal_qty = js_num(proposal.get("quantity").unwrap_or(&V::Null)).unwrap_or(0.0);
        if proposal_qty != 0.0 && !proposal_qty.is_nan() {
            let mut c = proposal.as_object().cloned().unwrap_or_default();
            c.insert("throughLevel".into(), json!(through_level as i64));
            candidates.push(V::Object(c));
        }
    }
    candidates.sort_by(|a, b| {
        let qa = js_num(a.get("quantity").unwrap_or(&V::Null)).unwrap_or(f64::NAN);
        let qb = js_num(b.get("quantity").unwrap_or(&V::Null)).unwrap_or(f64::NAN);
        let dq = qb - qa;
        if dq < 0.0 {
            std::cmp::Ordering::Less
        } else if dq > 0.0 {
            std::cmp::Ordering::Greater
        } else {
            let ra = amount(
                a.get("removedPrincipalIfFilled")
                    .and_then(V::as_str)
                    .unwrap_or(""),
            )
            .unwrap_or(i128::MIN);
            let rb = amount(
                b.get("removedPrincipalIfFilled")
                    .and_then(V::as_str)
                    .unwrap_or(""),
            )
            .unwrap_or(i128::MIN);
            if rb > ra {
                return std::cmp::Ordering::Greater;
            }
            if rb < ra {
                return std::cmp::Ordering::Less;
            }
            let la = price(a.get("protectedLimit").and_then(V::as_str).unwrap_or(""))
                .unwrap_or(i128::MIN);
            let lb = price(b.get("protectedLimit").and_then(V::as_str).unwrap_or(""))
                .unwrap_or(i128::MIN);
            let dl = lb - la;
            if dl < 0 {
                std::cmp::Ordering::Less
            } else if dl > 0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        }
    });
    Ok(json!({
        "selected": candidates.first().cloned().unwrap_or(V::Null),
        "candidates": candidates,
        "executionAuthority": false,
    }))
}
