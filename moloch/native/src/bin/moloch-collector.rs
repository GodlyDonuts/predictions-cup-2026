//! Serialized, read-only SIG transport. No order submission routes exist here.
use moloch_replay::source::RestSource;
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
    #[cfg(unix)]
    std::fs::File::open(p.parent().ok_or("STATE_PARENT")?)?.sync_all()?;
    Ok(())
}
fn retry_delay(header: Option<&str>, at: u64) -> u64 {
    let seconds = header
        .and_then(|s| s.parse::<u64>().ok())
        .map(|s| s.saturating_mul(1000));
    let date = header
        .and_then(|s| chrono::DateTime::parse_from_rfc2822(s).ok())
        .map(|d| (d.timestamp_millis().max(0) as u64).saturating_sub(at));
    seconds.or(date).unwrap_or(60000).max(60000)
}
struct Packet {
    data: Option<Value>,
    status: u16,
    delay: u64,
}
fn get(client: &reqwest::blocking::Client, url: &reqwest::Url, credential: &str) -> Packet {
    let Ok(r) = client.get(url.clone()).bearer_auth(credential).send() else {
        return Packet {
            data: None,
            status: 0,
            delay: 60000,
        };
    };
    let status = r.status().as_u16();
    let delay = retry_delay(
        r.headers().get("retry-after").and_then(|h| h.to_str().ok()),
        now(),
    );
    if status != 200 {
        return Packet {
            data: None,
            status,
            delay,
        };
    }
    let mut bytes = Vec::new();
    let data = if r.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes).is_ok()
        && bytes.len() <= 16 * 1024 * 1024
    {
        serde_json::from_slice(&bytes).ok()
    } else {
        None
    };
    Packet {
        data,
        status,
        delay,
    }
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
    let started_at = iso(now());
    let mut source = RestSource::new(&json!(pairs), TOURNAMENT, &started_at)?;
    let mut events = OpenOptions::new()
        .append(true)
        .create_new(true)
        .open(out.join("events.jsonl"))?;
    let mut sequence = 0u64;
    let mut previous = String::new();
    let mut journal = OpenOptions::new()
        .append(true)
        .create_new(true)
        .open(out.join("source-journal.jsonl"))?;
    save(
        &out.join("manifest.json"),
        &json!({"runtime":"rust","mode":"official_sig_rest_capture","startedAt":started_at,"pairs":pairs,"tournamentId":TOURNAMENT,"readLimitPerMinute":100,"contestOrders":0,"financialIntegration":false}),
    )?;
    let deadline = now() + cycles as u64 * pairs.len() as u64 * 24000;
    let mut cursor_page: Option<(usize, reqwest::Url, usize)> = None;
    for i in 0..cycles * 20 * pairs.len() * 2 {
        if now() >= deadline || out.join("STOP").exists() {
            break;
        }
        let continuation = cursor_page.take();
        let pair_index = continuation.as_ref().map_or((i / 2) % pairs.len(), |c| c.0);
        let p = &pairs[pair_index];
        let book = continuation.is_none() && i % 2 == 0;
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
        let mut url = if let Some((_, url, _)) = &continuation {
            url.clone()
        } else {
            reqwest::Url::parse(&format!("https://sig.thesuper.market/api/v1{route}"))?
        };
        if continuation.is_none() {
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
        let packet = get(&client, &url, &credential);
        match packet.data {
            Some(data) => {
                let receipt = json!({"schemaVersion":1,"source":"official_sig_api_v1","kind":if book{"book"}else{"trades"},"endpoint":format!("{}?{}",route,url.query().unwrap()),"tournamentId":TOURNAMENT,"marketId":p["marketId"],"exchangeId":p["exchangeId"],"requestStartedAt":iso(started),"receivedAt":iso(now()),"status":200,"data":data});
                writeln!(log, "{}", receipt)?;
                log.sync_data()?;
                for event in source.accept(&receipt)? {
                    use sha2::{Digest, Sha256};
                    sequence += 1;
                    let body = json!({"sequence":sequence,"previous":previous,"event":event});
                    let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&body)?));
                    let mut row = body;
                    row["hash"] = json!(hash);
                    writeln!(journal, "{}", row)?;
                    journal.sync_data()?;
                    writeln!(events, "{}", row["event"])?;
                    events.sync_data()?;
                    previous = hash;
                }
                save(
                    &out.join("stats.json"),
                    &json!({"runtime":"rust","phase":"capturing","at":iso(now()),"sequence":sequence,"previous":previous,"scopes":source.scopes(),"contestOrders":0}),
                )?;
                if !book && receipt["data"]["pagination"]["hasMore"] == true {
                    let page = continuation.as_ref().map_or(1, |c| c.2);
                    if page >= 5 {
                        return Err("CAPTURE_PAGE_BOUND".into());
                    }
                    let next = receipt["data"]["pagination"]["nextCursor"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .ok_or("CAPTURE_CURSOR_MISSING")?;
                    let mut next_url = url.clone();
                    let query = url
                        .query_pairs()
                        .filter(|(k, _)| k != "cursor")
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect::<Vec<_>>();
                    next_url.set_query(None);
                    next_url
                        .query_pairs_mut()
                        .extend_pairs(query)
                        .append_pair("cursor", next);
                    cursor_page = Some((pair_index, next_url, page + 1));
                }
            }
            None => {
                b["blocked"] = json!(true);
                b["nextAt"] = json!(now().saturating_add(packet.delay));
                save(&bp, &b)?;
                save(
                    &out.join("failure.json"),
                    &json!({"at":iso(now()),"status":packet.status,"endpoint":format!("{}?{}",route,url.query().unwrap()),"retryAfterMs":packet.delay,"contestOrders":0,"latched":true}),
                )?;
                return Err("CAPTURE_READ_FAILED_LATCHED".into());
            }
        }
    }
    save(
        &out.join("stats.json"),
        &json!({"runtime":"rust","phase":"completed","at":iso(now()),"sequence":sequence,"previous":previous,"scopes":source.scopes(),"contestOrders":0}),
    )?;
    Ok(())
}
fn iso(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .expect("timestamp")
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn benchmark(output: &Path) -> Result<()> {
    // The benchmark cannot select a remote host and never loads credentials.
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = thread::spawn(move || -> std::io::Result<usize> {
        let mut served = 0;
        for socket in listener.incoming().take(100) {
            let mut socket = socket?;
            socket.set_read_timeout(Some(Duration::from_secs(5)))?;
            let mut request = Vec::new();
            let mut buffer = [0u8; 512];
            while !request.windows(4).any(|s| s == b"\r\n\r\n") {
                let n = socket.read(&mut buffer)?;
                if n == 0 || request.len() + n > 4096 {
                    return Err(std::io::Error::other("BENCHMARK_REQUEST"));
                }
                request.extend_from_slice(&buffer[..n]);
            }
            if !request.starts_with(b"GET /benchmark ") {
                return Err(std::io::Error::other("BENCHMARK_ROUTE"));
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
            served += 1;
        }
        Ok(served)
    });
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()?;
    let mut budget = json!({"times":[],"nextAt":0});
    let mut starts = Vec::new();
    let began = std::time::Instant::now();
    for _ in 0..100 {
        loop {
            let t = now();
            let at = admission(&budget, t)?;
            if at <= t {
                break;
            }
            thread::sleep(Duration::from_millis((at - t).min(100)));
        }
        let at = now();
        budget["times"].as_array_mut().unwrap().push(json!(at));
        budget["nextAt"] = json!(at + 600);
        starts.push(at);
        let r = client.get(format!("http://{address}/benchmark")).send()?;
        if r.status() != 200 {
            return Err("BENCHMARK_STATUS".into());
        }
    }
    let served = server.join().map_err(|_| "BENCHMARK_SERVER")??;
    let minimum = starts
        .windows(2)
        .map(|p| p[1] - p[0])
        .min()
        .ok_or("BENCHMARK_TIMES")?;
    let report = json!({"runtime":"rust","mode":"loopback_only","requests":served,"elapsedMs":began.elapsed().as_millis(),"minimumStartSpacingMs":minimum,"maximumRequestsInRollingMinute":starts.iter().map(|t|starts.iter().filter(|x|**x>*t-60000&&**x<=*t).count()).max(),"sigRequests":0,"contestOrders":0,"credentialsLoaded":false});
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    serde_json::to_writer_pretty(&mut file, &report)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    println!("{}", report);
    Ok(())
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
        Some("benchmark") if args.len() == 2 => benchmark(Path::new(&args[1])),
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
    fn retry_after_is_preserved() {
        assert_eq!(retry_delay(Some("120"), 0), 120000);
        assert_eq!(retry_delay(Some("1"), 0), 60000);
        assert_eq!(retry_delay(Some("invalid"), 0), 60000);
        assert_eq!(
            retry_delay(Some("Thu, 01 Jan 1970 00:03:00 GMT"), 0),
            180000
        );
    }
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
