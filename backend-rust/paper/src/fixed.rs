// paper/fixed.mjs — integer micro units, BigInt like the JS source.
use num_bigint::BigInt;
use serde_json::Value as V;

pub const SCALE: i64 = 1_000_000;
pub const TICK: i64 = 5_000;

pub fn big_amount(value: &str) -> Result<BigInt, String> {
    let valid = {
        let (neg, body) = match value.strip_prefix('-') {
            Some(b) => (true, b),
            None => (false, value),
        };
        let _ = neg;
        match body.split_once('.') {
            None => !body.is_empty() && body.bytes().all(|b| b.is_ascii_digit()) && canon(body),
            Some((w, f)) => {
                !w.is_empty()
                    && canon(w)
                    && !f.is_empty()
                    && f.len() <= 6
                    && f.bytes().all(|b| b.is_ascii_digit())
            }
        }
    };
    if !valid {
        return Err("Money must be a canonical decimal string with at most six places".into());
    }
    let negative = value.starts_with('-');
    let body = if negative { &value[1..] } else { value };
    let (whole, frac) = match body.split_once('.') {
        Some((w, f)) => (w, f),
        None => (body, ""),
    };
    let w: BigInt = whole.parse().map_err(|_| "money".to_string())?;
    let f: BigInt = format!("{frac:0<6}").parse().map_err(|_| "money".to_string())?;
    let x = w * BigInt::from(SCALE) + f;
    Ok(if negative { -x } else { x })
}
fn canon(w: &str) -> bool {
    w.bytes().all(|b| b.is_ascii_digit()) && (w == "0" || !w.starts_with('0'))
}

pub fn money(value: &BigInt) -> String {
    let scale = BigInt::from(SCALE);
    let (sign, x) = if *value < BigInt::from(0) { ("-", -value) } else { ("", value.clone()) };
    format!("{}{}.{:0>6}", sign, &x / &scale, (&x % &scale).to_string())
}

pub fn big_price(value: &str) -> Result<BigInt, String> {
    let r = big_amount(value)?;
    let tick = BigInt::from(TICK);
    let scale = BigInt::from(SCALE);
    if r < tick || r > &scale - &tick || &r % &tick != BigInt::from(0) {
        return Err("Price must be 0.005..0.995 on the 0.005 tick".into());
    }
    Ok(r)
}

pub fn big_sequence(value: &str) -> Result<BigInt, String> {
    if value.is_empty()
        || !value.bytes().all(|b| b.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err("Sequence must be an unsigned integer string".into());
    }
    value.parse().map_err(|_| "Sequence must be an unsigned integer string".into())
}

// fee(notional, quantity, costs): per-fill proportional charge rounded UP to
// one micro plus a flat per-share amount; never assume rebates.
pub fn big_fee(notional: &BigInt, quantity: &BigInt, costs: &V) -> Result<BigInt, String> {
    let fee_bps = BigInt::from(costs["feeBps"].as_i64().unwrap_or(0));
    let per_share = big_amount(costs["feePerShare"].as_str().unwrap_or(""))?;
    Ok((notional * fee_bps + BigInt::from(9_999)) / BigInt::from(10_000) + quantity * per_share)
}

pub fn instant(value: &str) -> Result<i64, String> {
    // canonical UTC ISO milliseconds, exactly as paper/fixed.mjs.
    let re_ok = value.len() == 24
        && value.as_bytes()[4] == b'-' && value.as_bytes()[7] == b'-'
        && value.as_bytes()[10] == b'T' && value.as_bytes()[13] == b':'
        && value.as_bytes()[16] == b':' && value.as_bytes()[19] == b'.'
        && value.as_bytes()[23] == b'Z';
    if !re_ok { return Err("Timestamp must be canonical UTC ISO milliseconds".into()); }
    let ms = baal_parse_iso(value).ok_or("Invalid timestamp".to_string())?;
    if baal_iso_from_ms(ms) != value { return Err("Invalid timestamp".into()); }
    Ok(ms)
}
fn baal_parse_iso(v: &str) -> Option<i64> {
    let ms = baal_service_runtime::contract::parse_iso_ms(v)?;
    Some(ms as i64)
}
fn baal_iso_from_ms(ms: i64) -> String {
    baal_service_runtime::contract::ms_to_iso(ms as f64)
}
