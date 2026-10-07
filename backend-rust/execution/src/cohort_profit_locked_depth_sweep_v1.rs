// execution/cohort-profit-locked-depth-sweep-v1.mjs — sweep the YES ask
// ladder: for each level, size a cohort-profit-locked NO exit against the
// cumulative reachable depth (clamped at 100 shares) and keep the best
// proposal by quantity, then removed principal, then protected limit.
//
// Port notes:
// - `{yesAsks,requestedQuantity=null,...args}`: args is the input minus both
//   destructured keys, passed through verbatim to the sizer.
// - Errors are raw throws: COHORT_DEPTH_INPUT for a bad ladder,
//   COHORT_DEPTH_LEVEL for a non-increasing price or bad quantity; the
//   price()/sizer errors propagate unprefixed exactly as in JS.
// - `depth` clamps the level quantity offered to the sizer at 100; the sizer
//   itself sees `{price: l.price, quantity: depth}`.
// - Candidate sort: quantity desc, removedPrincipalIfFilled (micro) desc,
//   then protectedLimit (micro) desc — stable, matching Array#sort.
use baal_service_runtime::fixed::{amount, price};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Map, Value as V};

use crate::cohort_profit_locked_no_exit_v1::size_cohort_profit_locked_no_exit_v1;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

/// Math.min coercion: JS ToNumber — strings parse, null/absent handled by the
/// caller's `?? default`, anything else NaN.
fn js_num(v: &V) -> f64 {
    match v {
        V::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        V::String(s) => s.trim().parse::<f64>().unwrap_or(f64::NAN),
        V::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        _ => f64::NAN,
    }
}

/// selectCohortProfitLockedDepthV1({yesAsks,requestedQuantity=null,...args})
pub fn select_cohort_profit_locked_depth_v1(input: &V) -> Result<V, String> {
    let yes_asks = get(input, "yesAsks");
    let requested_quantity = input.get("requestedQuantity");
    // {...args}: input minus the two destructured keys (insertion order kept).
    let mut args = input.as_object().cloned().unwrap_or_default();
    args.shift_remove("yesAsks");
    args.shift_remove("requestedQuantity");
    if !(yes_asks.is_array()
        && !yes_asks.as_array().unwrap().is_empty()
        && yes_asks.as_array().unwrap().len() <= 20)
    {
        return Err("COHORT_DEPTH_INPUT".to_string());
    }
    let mut depth: i64 = 0;
    let mut previous: i128 = -1;
    let mut candidates: Vec<V> = Vec::new();
    for (through_level, l) in yes_asks.as_array().unwrap().iter().enumerate() {
        // price() first: its own error propagates before the level check.
        let p = price(get(l, "price").as_str().unwrap_or("\u{0}"))?;
        let lq = js_int(get(l, "quantity"));
        if p <= previous || lq.map(|q| q < 1).unwrap_or(true) {
            return Err("COHORT_DEPTH_LEVEL".to_string());
        }
        previous = p;
        depth = depth.saturating_add(lq.unwrap()).min(100);
        // maximumSharesPerOrder = min(args.maximumSharesPerOrder ?? 100,
        //                             requestedQuantity ?? 100)
        let args_max = match args.get("maximumSharesPerOrder") {
            Some(x) if !x.is_null() => js_num(x),
            _ => 100.0,
        };
        let req = match requested_quantity {
            Some(x) if !x.is_null() => js_num(x),
            _ => 100.0,
        };
        let min_shares = args_max.min(req);
        let mut call = args.clone();
        call.insert(
            "maximumSharesPerOrder".into(),
            if min_shares.is_finite() && min_shares.fract() == 0.0 {
                json!(min_shares as i64)
            } else {
                json!(min_shares)
            },
        );
        call.insert(
            "yesAskLevel".into(),
            json!({"price": get(l, "price"), "quantity": depth}),
        );
        let proposal = size_cohort_profit_locked_no_exit_v1(&V::Object(call))?;
        if js_int(get(&proposal, "quantity")).map(|q| q != 0).unwrap_or(false) {
            let mut c = proposal.as_object().cloned().unwrap_or_default();
            c.insert("throughLevel".into(), json!(through_level as i64));
            candidates.push(V::Object(c));
        }
    }
    candidates.sort_by(|a, b| {
        let qa = js_int(get(a, "quantity")).unwrap_or(0);
        let qb = js_int(get(b, "quantity")).unwrap_or(0);
        let ra = amount(get(a, "removedPrincipalIfFilled").as_str().unwrap_or(""))
            .unwrap_or(i128::MIN);
        let rb = amount(get(b, "removedPrincipalIfFilled").as_str().unwrap_or(""))
            .unwrap_or(i128::MIN);
        let pa = price(get(a, "protectedLimit").as_str().unwrap_or("")).unwrap_or(i128::MIN);
        let pb = price(get(b, "protectedLimit").as_str().unwrap_or("")).unwrap_or(i128::MIN);
        qb.cmp(&qa)
            .then_with(|| {
                if rb > ra {
                    std::cmp::Ordering::Less
                } else if rb < ra {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| (pb - pa).cmp(&0))
    });
    let mut out = Map::new();
    out.insert(
        "selected".into(),
        candidates.first().cloned().unwrap_or(V::Null),
    );
    out.insert("candidates".into(), V::Array(candidates));
    out.insert("executionAuthority".into(), json!(false));
    Ok(V::Object(out))
}
