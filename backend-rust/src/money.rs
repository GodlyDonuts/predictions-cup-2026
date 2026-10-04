pub const SCALE: i128 = 1_000_000;
pub const TICK: i128 = 5_000;
pub fn amount(s: &str) -> Result<i128, String> {
    let (negative, s) = s.strip_prefix('-').map_or((false, s), |x| (true, x));
    let mut fields = s.split('.');
    let whole = fields.next().unwrap_or("");
    let fractional = fields.next();
    let fraction = fractional.unwrap_or("");
    if fields.next().is_some()
        || whole.is_empty()
        || whole.len() > 1 && whole.starts_with('0')
        || !whole.bytes().all(|x| x.is_ascii_digit())
        || fractional == Some("")
        || fraction.len() > 6
        || !fraction.bytes().all(|x| x.is_ascii_digit())
    {
        return Err("INVALID_MONEY".into());
    }
    let w = whole.parse::<i128>().map_err(|_| "INVALID_MONEY")?;
    let f = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<i128>().map_err(|_| "INVALID_MONEY")?
    };
    let value = w
        .checked_mul(SCALE)
        .and_then(|v| v.checked_add(f * 10_i128.pow((6 - fraction.len()) as u32)))
        .ok_or("MONEY_OVERFLOW")?;
    Ok(if negative { -value } else { value })
}
pub fn money(v: i128) -> String {
    let sign = if v < 0 { "-" } else { "" };
    let x = v.abs();
    format!("{sign}{}.{:06}", x / SCALE, x % SCALE)
}
pub fn fee(notional: i128, bps: i128) -> i128 {
    (notional * bps + 9_999) / 10_000
}
pub fn price(s: &str) -> Result<i128, String> {
    let v = amount(s)?;
    if !(TICK..=SCALE - TICK).contains(&v) || v % TICK != 0 {
        return Err("INVALID_PRICE_TICK".into());
    }
    Ok(v)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact() {
        assert_eq!(amount("-0.015").unwrap(), -15000);
        assert_eq!(money(-15000), "-0.015000");
        assert_eq!(fee(415000, 10), 415);
        assert_eq!(fee(1, 1), 1);
        for x in ["01", "0.0000001", "NaN", "1."] {
            assert!(amount(x).is_err());
        }
        assert!(price("0.006").is_err());
    }
}
