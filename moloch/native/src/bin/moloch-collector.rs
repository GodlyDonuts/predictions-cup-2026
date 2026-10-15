//! Serialized, read-only SIG transport. No order submission routes exist here.
use serde_json::{Value, json};
use std::{
    env,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const TOURNAMENT: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis() as u64
}
fn read(p: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
fn save(p: &Path, v: &Value) -> Result<()> {
    let tmp = p.with_extension("pending");
    let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    f.write_all(&serde_json::to_vec(v)?)?;
    f.sync_all()?;
    fs::rename(tmp, p)?;
    Ok(())
}
fn admission(b: &Value, t: u64) -> Result<u64> {
    let times = b["times"].as_array().ok_or("BUDGET_TIMES")?;
    let mut last = 0;
    let mut recent = Vec::new();
    for v in times {
        let x = v.as_u64().ok_or("BUDGET_TIME")?;
        if x < last || x > t {
            return Err("BUDGET_CLOCK".into());
        }
        last = x;
        if x > t.saturating_sub(60000) {
            recent.push(x)
        }
    }
    let spacing = if times.is_empty() { 0 } else { last + 600 };
    Ok(t.max(spacing)
        .max(b["nextAt"].as_u64().ok_or("BUDGET_NEXT")?)
        .max(if recent.len() >= 100 {
            recent[recent.len() - 100] + 60001
        } else {
            0
        }))
}
fn guard(state: &Path) -> Result<Value> {
    let c = read(&state.join("deployment.json"))?;
    if c["networkEnabled"] != true
        || c["tradingEnabled"] != false
        || c["readsPerMinute"] != 100
        || env::var("MOLOCH_NETWORK_ENABLED").as_deref() != Ok("1")
        || state.join("NETWORK_DISABLED").exists()
    {
        return Err("MOLOCH_NETWORK_DISABLED".into());
    }
    Ok(c)
}
fn key(p: &Path) -> Result<String> {
    let m = fs::symlink_metadata(p)?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err("CREDENTIAL_PERMISSIONS".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let owner = fs::metadata(env::var("HOME")?)?.uid();
        if m.mode() & 0o077 != 0 || m.uid() != owner {
            return Err("CREDENTIAL_PERMISSIONS".into());
        }
    }
    let s = fs::read_to_string(p)?.trim().to_owned();
    if !(16..=256).contains(&s.len())
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("CREDENTIAL_FORMAT".into());
    }
    Ok(s)
}
struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn capture(state: &Path, pairs_file: &Path, out: &Path, cycles: usize) -> Result<()> {
    guard(state)?;
    let pairs = read(pairs_file)?;
    let pairs = pairs.as_array().ok_or("SCOPES")?;
    if pairs.is_empty() || pairs.len() > 4 || cycles == 0 || cycles > 60 / pairs.len() {
        return Err("SCOPES".into());
    }
    let mut markets = std::collections::BTreeSet::new();
    let mut exchanges = std::collections::BTreeSet::new();
    for p in pairs {
        for k in ["marketId", "exchangeId"] {
            let id = p[k].as_str().ok_or("SCOPE_ID")?;
            if id.starts_with('0') || id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
                return Err("SCOPE_ID".into());
            }
        }
        if !markets.insert(p["marketId"].as_str().unwrap())
            || !exchanges.insert(p["exchangeId"].as_str().unwrap())
        {
            return Err("DUPLICATE_SCOPE".into());
        }
    }
    let lock = state.join("capture.lock");
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)?;
    let _lock = Lock(lock);
    write!(f, "{}", std::process::id())?;
    f.sync_all()?;
    let bp = state.join("sig-read-budget.json");
    let mut b = if bp.exists() {
        read(&bp)?
    } else {
        json!({"times":[],"nextAt":0,"blocked":false,"endpointNextAt":{}})
    };
    if b["blocked"] != false {
        return Err("CAPTURE_BUDGET_BLOCKED".into());
    }
    let credential = key(&state.join("credentials/sig-api.key"))?;
    fs::create_dir(out)?;
    let mut log = OpenOptions::new()
        .append(true)
        .create_new(true)
        .open(out.join("receipts.jsonl"))?;
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()?;
    let deadline = now() + cycles as u64 * pairs.len() as u64 * 24000;
    for i in 0..cycles * 20 * pairs.len() * 2 {
        if now() >= deadline || out.join("STOP").exists() {
            break;
        }
        let p = &pairs[(i / 2) % pairs.len()];
        let book = i % 2 == 0;
        let route = if book {
            format!("/markets/{}/orderbook", p["marketId"].as_str().unwrap())
        } else {
            format!("/exchanges/{}/trades", p["exchangeId"].as_str().unwrap())
        };
        loop {
            let t = now();
            let at = admission(&b, t)?.max(b["endpointNextAt"][&route].as_u64().unwrap_or(0));
            if at <= t {
                break;
            }
            if out.join("STOP").exists() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis((at - t).min(250)))
        }
        let started = now();
        let mut times = b["times"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|x| x.as_u64().unwrap() > started.saturating_sub(60000))
            .cloned()
            .collect::<Vec<_>>();
        times.push(json!(started));
        b["times"] = json!(times);
        b["nextAt"] = json!(started + 600);
        b["endpointNextAt"][&route] = json!(started + if book { 1000 } else { 2000 });
        save(&bp, &b)?;
        let mut url = reqwest::Url::parse(&format!("https://sig.thesuper.market/api/v1{route}"))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("tournamentId", TOURNAMENT);
            if book {
                q.append_pair("depth", "200");
            } else {
                q.append_pair("limit", "200");
                q.append_pair("from", &iso(started.saturating_sub(900000)));
                q.append_pair("to", &iso(started));
            }
        }
        let result = (|| -> Result<Value> {
            let r = client.get(url.clone()).bearer_auth(&credential).send()?;
            if r.status() != 200 {
                return Err("HTTP_STATUS".into());
            }
            let mut bytes = Vec::new();
            r.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 16 * 1024 * 1024 {
                return Err("BODY_LIMIT".into());
            }
            Ok(serde_json::from_slice(&bytes)?)
        })();
        match result {
            Ok(data) => {
                let receipt = json!({"schemaVersion":1,"source":"official_sig_api_v1","kind":if book{"book"}else{"trades"},"endpoint":format!("{}?{}",route,url.query().unwrap()),"tournamentId":TOURNAMENT,"marketId":p["marketId"],"exchangeId":p["exchangeId"],"requestStartedAt":iso(started),"receivedAt":iso(now()),"status":200,"data":data});
                writeln!(log, "{}", receipt)?;
                log.sync_data()?;
            }
            Err(_) => {
                b["blocked"] = json!(true);
                b["nextAt"] = json!(now() + 60000);
                save(&bp, &b)?;
                return Err("CAPTURE_READ_FAILED_LATCHED".into());
            }
        }
    }
    Ok(())
}
fn iso(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .expect("timestamp")
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn run() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let state = PathBuf::from(env::var("MOLOCH_STATE_DIR").unwrap_or_else(|_| {
        format!(
            "{}/apps/moloch/shared",
            env::var("HOME").unwrap_or_default()
        )
    }));
    match args.first().map(String::as_str) {
        Some("check") => {
            let c = read(&state.join("deployment.json"))?;
            println!(
                "{}",
                json!({"runtime":"rust","networkEnabled":c["networkEnabled"],"tradingEnabled":c["tradingEnabled"],"configuredReadsPerMinute":c["readsPerMinute"],"networkStopMarker":state.join("NETWORK_DISABLED").exists(),"executionStopMarker":state.join("EXECUTION_STOP").exists()})
            );
            Ok(())
        }
        Some("capture") if args.len() == 4 => capture(
            &state,
            Path::new(&args[1]),
            Path::new(&args[2]),
            args[3].parse()?,
        ),
        _ => Err("USAGE: moloch-collector check | capture PAIRS NEW_OUTPUT CYCLES".into()),
    }
}
fn main() {
    if run().is_err() {
        eprintln!("MOLOCH_COLLECTOR_FAILED (no credentials or response bodies logged)");
        std::process::exit(1)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rolling_budget() {
        let mut b = json!({"times":[],"nextAt":0});
        let mut t = 1;
        for _ in 0..1000 {
            t = admission(&b, t).unwrap();
            b["times"].as_array_mut().unwrap().push(json!(t));
            let v = b["times"].as_array().unwrap();
            assert!(
                v.iter()
                    .filter(|x| x.as_u64().unwrap() > t.saturating_sub(60000))
                    .count()
                    <= 100
            )
        }
        assert_eq!(b["times"][99], 59401);
        assert_eq!(b["times"][100], 60002)
    }
    #[test]
    fn clock_reversal_rejected() {
        assert!(admission(&json!({"times":[5,4],"nextAt":0}), 6).is_err());
        assert!(admission(&json!({"times":[7],"nextAt":0}), 6).is_err())
    }
}
