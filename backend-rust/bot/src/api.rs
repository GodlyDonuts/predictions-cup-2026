// bot/api.mjs — read-only research API client with serialized admission pacing.
use baal_service_runtime::client::{Fetch, FetchOutcome, FetchRequest};
use serde_json::{json, Value as V};
use std::sync::Mutex;

/// Reads the API key from SIG_API_KEY, falling back to the Windows DPAPI
/// credential bound to the local account (pwsh.exe on Windows only).
pub fn load_key() -> Result<String, String> {
    if let Ok(k) = std::env::var("SIG_API_KEY") {
        return Ok(k.trim().to_string());
    }
    let script = "$ErrorActionPreference='Stop'; $s=(Get-Content -Raw -LiteralPath (Join-Path $env:LOCALAPPDATA 'SigTradingCup\\api-key.dpapi')).Trim()|ConvertTo-SecureString; [Console]::Write(([pscredential]::new('api',$s)).GetNetworkCredential().Password)";
    let out = std::process::Command::new("pwsh.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|_| "API key unavailable: SIG_API_KEY unset and DPAPI requires Windows".to_string())?;
    if !out.status.success() {
        return Err("API key unavailable: DPAPI credential read failed".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub retry_after: f64,
}
impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "API {}: {}", self.status, self.code)
    }
}
impl std::error::Error for ApiError {}

/// bot retryAfterMs(header, now, fallback=0): seconds or HTTP-date, else fallback.
pub fn retry_after_ms(header: Option<&str>, now: f64, fallback: f64) -> f64 {
    let Some(raw) = header else { return fallback };
    let text = raw.trim();
    if text.is_empty() {
        return fallback;
    }
    let seconds = {
        let (whole, frac) = match text.split_once('.') {
            Some((w, f)) => (w, Some(f)),
            None => (text, None),
        };
        if !whole.is_empty()
            && whole.bytes().all(|b| b.is_ascii_digit())
            && frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
        {
            text.parse::<f64>().unwrap_or(f64::NAN)
        } else {
            f64::NAN
        }
    };
    let delay = if seconds.is_finite() {
        seconds * 1000.0
    } else if text.len() >= 3 && text[text.len() - 3..].eq_ignore_ascii_case("gmt") {
        baal_service_runtime::contract::parse_iso_ms(text).map(|d| d - now).unwrap_or(f64::NAN)
    } else {
        f64::NAN
    };
    if delay.is_finite() { delay.max(0.0) } else { fallback }
}

struct QueueState {
    next: f64,
    paused_until: f64,
}

/// AdmissionQueue: serializes admission pacing. Api::request holds `tail` so
/// admits serialize exactly like the JS promise-chain tail.
pub struct AdmissionQueue<FNow, FSleep>
where
    FNow: Fn() -> f64,
    FSleep: Fn(f64),
{
    pub interval: f64,
    pub now: FNow,
    pub sleep: FSleep,
    state: Mutex<QueueState>,
}

impl<FNow: Fn() -> f64, FSleep: Fn(f64)> AdmissionQueue<FNow, FSleep> {
    pub fn new(interval: f64, now: FNow, sleep: FSleep) -> Result<Self, String> {
        if !interval.is_finite() || interval < 0.0 {
            return Err("Invalid admission interval".into());
        }
        Ok(AdmissionQueue {
            interval,
            now,
            sleep,
            state: Mutex::new(QueueState { next: 0.0, paused_until: 0.0 }),
        })
    }
    pub fn pause(&self, ms: f64) -> Result<(), String> {
        if !ms.is_finite() || ms < 0.0 {
            return Err("Invalid cooldown".into());
        }
        let mut s = self.state.lock().unwrap();
        s.paused_until = s.paused_until.max((self.now)() + ms);
        Ok(())
    }
    /// Waits out next/paused_until then advances next by the interval.
    pub fn admit(&self) {
        loop {
            let delay = {
                let s = self.state.lock().unwrap();
                s.next.max(s.paused_until) - (self.now)()
            };
            if delay <= 0.0 {
                break;
            }
            (self.sleep)(delay); // Recheck if another response extended the pause.
        }
        self.state.lock().unwrap().next = (self.now)() + self.interval;
    }
}

pub struct Api<FNow, FSleep>
where
    FNow: Fn() -> f64,
    FSleep: Fn(f64),
{
    pub base: String,
    pub key: String,
    pub now: FNow,
    pub fetch: std::sync::Arc<dyn Fetch>,
    pub queue: AdmissionQueue<FNow, FSleep>,
    /// Serializes admit() like the JS promise-chain tail.
    pub tail: Mutex<()>,
}

impl<FNow: Fn() -> f64 + Clone, FSleep: Fn(f64) + Clone> Api<FNow, FSleep> {
    pub fn new(
        base: &str,
        key: &str,
        interval: f64,
        now: FNow,
        sleep: FSleep,
        fetch: std::sync::Arc<dyn Fetch>,
    ) -> Result<Self, String> {
        if base != baal_service_runtime::client::API_BASE {
            return Err("API destination must be the authorized SIG host".into());
        }
        Ok(Api {
            base: base.to_string(),
            key: key.to_string(),
            queue: AdmissionQueue::new(interval, now.clone(), sleep.clone())?,
            now,
            fetch,
            tail: Mutex::new(()),
        })
    }

    /// GET only: research client is read-only — order placement and cancellation disabled.
    pub fn request(&self, path: &str) -> Result<V, ApiError> {
        for attempt in 0..4u32 {
            {
                let _guard = self.tail.lock().unwrap();
                self.queue.admit();
            }
            let outcome = self.fetch.fetch(
                &format!("{}{}", self.base, path),
                &FetchRequest {
                    method: "GET".into(),
                    headers: vec![
                        ("Authorization".into(), format!("Bearer {}", self.key)),
                        ("Content-Type".into(), "application/json".into()),
                    ],
                    body: None,
                    timeout_ms: 20000,
                },
            );
            let FetchOutcome { status, retry_after, body } = match outcome {
                Ok(o) => o,
                Err(_) => {
                    if attempt == 3 {
                        return Err(ApiError {
                            status: 0,
                            code: "API connection unavailable".into(),
                            retry_after: 0.0,
                        });
                    }
                    let _ = self.queue.pause(1000.0 * 2f64.powi(attempt as i32));
                    continue;
                }
            };
            let data: V = serde_json::from_str(&body).unwrap_or(V::Null);
            if (200..300).contains(&status) {
                return Ok(data);
            }
            let code = data["error"]["code"].as_str().unwrap_or("UNKNOWN_ERROR").to_string();
            let delay = retry_after_ms(retry_after.as_deref(), (self.now)(), 0.0);
            if status == 429 || status == 503 {
                let backoff = delay.max(if status == 429 { 60000.0 } else { 1000.0 * 2f64.powi(attempt as i32) });
                let _ = self.queue.pause(backoff);
                if attempt == 3 {
                    return Err(ApiError { status, code, retry_after: backoff });
                }
                continue;
            }
            return Err(ApiError { status, code, retry_after: delay });
        }
        unreachable!()
    }

    /// Follows cursor pagination: collects page.data until hasMore is false.
    pub fn pages(&self, path: &str) -> Result<Vec<V>, ApiError> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = self.request(&format!(
                "{}{}",
                path,
                cursor.as_ref().map(|c| format!("&cursor={}", urlencoding(c))).unwrap_or_default()
            ))?;
            if let Some(arr) = page["data"].as_array() {
                out.extend(arr.iter().cloned());
            }
            let has_more = page["pagination"]["hasMore"].as_bool().unwrap_or(false);
            cursor = if has_more { page["pagination"]["nextCursor"].as_str().map(str::to_string) } else { None };
            if has_more && cursor.is_none() {
                return Err(ApiError {
                    status: 0,
                    code: "Incomplete API pagination".into(),
                    retry_after: 0.0,
                });
            }
            if cursor.is_none() {
                break;
            }
        }
        Ok(out)
    }
}

fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{:02X}", b),
        })
        .collect()
}

/// Exposed for logging parity with the JS ApiError message.
pub fn api_error_json(e: &ApiError) -> V {
    json!({"status": e.status, "code": e.code, "retryAfter": e.retry_after})
}
