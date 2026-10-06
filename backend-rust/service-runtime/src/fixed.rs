// paper/fixed.mjs: integer micro-SUSQie money. Decimal strings only; never
// binary floating-point money.
pub const SCALE: i128 = 1_000_000;
pub const TICK: i128 = 5_000;

pub fn amount(value: &str) -> Result<i128, String> {
    let canonical = {
        let body = value.strip_prefix('-').unwrap_or(value);
        let mut parts = body.splitn(2, '.');
        let whole = parts.next().unwrap_or("");
        let frac = parts.next();
        (whole == "0" || !whole.is_empty() && !whole.starts_with('0'))
            && whole.bytes().all(|b| b.is_ascii_digit())
            && match frac {
                None => true,
                Some(f) => !f.is_empty() && f.len() <= 6 && f.bytes().all(|b| b.is_ascii_digit()),
            }
    };
    if !canonical {
        return Err("Money must be a canonical decimal string with at most six places".into());
    }
    let negative = value.starts_with('-');
    let body = value.strip_prefix('-').unwrap_or(value);
    let mut parts = body.splitn(2, '.');
    let whole: i128 = parts.next().unwrap_or("0").parse().map_err(|_| "INVALID_AMOUNT")?;
    let fraction = parts.next().unwrap_or("");
    let frac: i128 = fraction.parse::<i128>().unwrap_or(0) * 10_i128.pow((6 - fraction.len()) as u32);
    Ok(if negative { -1 } else { 1 } * (whole * SCALE + frac))
}
pub fn money(value: i128) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let x = value.abs();
    format!("{sign}{}.{:06}", x / SCALE, x % SCALE)
}
pub fn price(value: &str) -> Result<i128, String> {
    let result = amount(value)?;
    if result < TICK || result > SCALE - TICK || result % TICK != 0 {
        return Err("Price must be 0.005..0.995 on the 0.005 tick".into());
    }
    Ok(result)
}
pub fn uint(value: f64, name: &str, minimum: f64) -> Result<f64, String> {
    if !(value.is_finite() && value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_991.0 && value >= minimum) {
        return Err(format!("{name} must be a safe integer >= {minimum}"));
    }
    Ok(value)
}
pub fn instant(value: &str) -> Result<i64, String> {
    let ok = value.len() == 24
        && value.ends_with('Z')
        && value.as_bytes().get(23) == Some(&b'Z')
        && chrono::DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true) == value)
            .unwrap_or(false);
    if !ok {
        return Err("Timestamp must be canonical UTC ISO milliseconds".into());
    }
    Ok(chrono::DateTime::parse_from_rfc3339(value).unwrap().timestamp_millis())
}
pub fn sequence(value: &str) -> Result<i128, String> {
    if value.is_empty()
        || value.len() > 1 && value.starts_with('0')
        || !value.bytes().all(|b| b.is_ascii_digit())
    {
        return Err("Sequence must be an unsigned integer string".into());
    }
    value.parse().map_err(|_| "Sequence must be an unsigned integer string".to_string())
}
// fee: round each fill's proportional charge UP to one micro.
pub fn fee(notional: i128, quantity: i128, fee_bps: i128, fee_per_share: &str) -> Result<i128, String> {
    Ok((notional * fee_bps + 9_999) / 10_000 + quantity * amount(fee_per_share)?)
}
