use std::time::Duration;

pub const STOPPED: &str = "STOPPED";
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug)]
pub struct Stopped;
impl Stopped {
    // feed/runtime.mjs Stopped: name 'AbortError'.
    pub const NAME: &'static str = "AbortError";
}
impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(STOPPED)
    }
}
impl std::error::Error for Stopped {}

#[derive(Debug)]
pub enum WaitError {
    Stopped,
    InvalidDelay,
}
impl std::fmt::Display for WaitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WaitError::Stopped => f.write_str("STOPPED"),
            WaitError::InvalidDelay => f.write_str("INVALID_DELAY"),
        }
    }
}
impl std::error::Error for WaitError {}

pub fn check_stop(aborted: bool, should_stop: &dyn Fn() -> bool) -> Result<(), WaitError> {
    if aborted || should_stop() { Err(WaitError::Stopped) } else { Ok(()) }
}

// feed/runtime.mjs retryAfterMs: only a pure-decimal header is treated as
// seconds; GMT-suffixed dates subtract `now`; everything else is zero — EXCEPT
// a valid decimal that overflows, which saturates to MAX_SAFE_INTEGER so the
// caller flags a manual review instead of retrying early.
pub fn retry_after_ms(header: Option<&str>, now_ms: f64) -> f64 {
    let Some(raw) = header else { return 0.0 };
    let text = raw.trim();
    if text.is_empty() {
        return 0.0;
    }
    let numeric = {
        let (whole, frac) = match text.split_once('.') {
            Some((w, f)) => (w, Some(f)),
            None => (text, None),
        };
        !whole.is_empty()
            && whole.bytes().all(|b| b.is_ascii_digit())
            && frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
    };
    let n: f64 = if numeric {
        text.parse::<f64>().unwrap_or(f64::NAN) * 1000.0
    } else if text.len() >= 3 && text[text.len() - 3..].eq_ignore_ascii_case("gmt") {
        httpdate::parse_http_date(text)
            .ok()
            .and_then(|d| d.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|x| x.as_millis() as f64 - now_ms)
            .unwrap_or(f64::NAN)
    } else {
        f64::NAN
    };
    if numeric && (!n.is_finite() || n >= MAX_SAFE_INTEGER as f64) {
        return MAX_SAFE_INTEGER as f64;
    }
    if n.is_finite() { n.max(0.0) } else { 0.0 }
}

// backoff(attempt, {baseMs, maxMs, random}): argument validation throws
// (requireThat), never a silent clamp.
pub fn backoff(attempt: i64, base_ms: f64, max_ms: f64, jitter: f64) -> Result<f64, WaitError> {
    let valid = attempt >= 0
        && base_ms.is_finite()
        && base_ms > 0.0
        && max_ms.is_finite()
        && max_ms >= base_ms
        && jitter.is_finite()
        && (0.0..=1.0).contains(&jitter);
    if !valid {
        return Err(WaitError::InvalidDelay);
    }
    let scale = base_ms * 2f64.powi(attempt.min(30) as i32);
    Ok(scale.min(max_ms) * (0.5 + jitter * 0.5))
}

// interruptibleWait: 50ms slices with a stop check before each slice and after.
pub fn interruptible_wait(
    ms: f64,
    aborted: &dyn Fn() -> bool,
    should_stop: &dyn Fn() -> bool,
    sleep: &dyn Fn(f64),
) -> Result<(), WaitError> {
    if !(ms.is_finite() && ms >= 0.0) {
        return Err(WaitError::InvalidDelay);
    }
    let mut remaining = ms;
    loop {
        check_stop(aborted(), should_stop)?;
        let chunk = remaining.min(50.0);
        if chunk > 0.0 {
            sleep(chunk);
        }
        remaining -= chunk;
        if remaining <= 0.0 {
            break;
        }
    }
    check_stop(aborted(), should_stop)
}

pub fn thread_sleep(ms: f64) {
    std::thread::sleep(Duration::from_millis(ms.ceil().max(0.0) as u64));
}
