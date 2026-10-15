//! Serialized, read-only SIG transport. No order submission routes exist here.
#[path = "../capture_scope.rs"]
mod capture_scope;
use moloch_replay::realtime::{Auth, Incoming, Realtime};
use moloch_replay::session::{ExclusiveLock, Session};
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
fn auth_packet(client: &reqwest::blocking::Client, credential: &str) -> Packet {
    let Ok(r) = client
        .post("https://sig.thesuper.market/api/v1/realtime/token")
        .bearer_auth(credential)
        .send()
    else {
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
    let data = if r.take(65537).read_to_end(&mut bytes).is_ok() && bytes.len() <= 65536 {
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
struct EventLog {
    events: std::fs::File,
    journal: std::fs::File,
    sequence: u64,
    previous: String,
    last_processed: u64,
    paper: Option<Session>,
    source_id: String,
    #[cfg(test)]
    test_clock: Option<u64>,
}
impl EventLog {
    fn append(&mut self, events: Vec<Value>) -> Result<()> {
        use sha2::{Digest, Sha256};
        for mut event in events {
            let at = now();
            #[cfg(test)]
            let at = self.test_clock.unwrap_or(at);
            let received = chrono::DateTime::parse_from_rfc3339(
                event["receivedAt"].as_str().ok_or("EVENT_RECEIPT")?,
            )?
            .timestamp_millis();
            if at < self.last_processed || received < 0 || at < (received as u64) {
                return Err("CAPTURE_PROCESSING_CLOCK".into());
            }
            event["processedAt"] = json!(iso(at));
            if let Some(paper) = &mut self.paper {
                paper.accept(&self.source_id, self.sequence + 1, &event)?;
            }
            self.sequence += 1;
            let body = json!({"sequence":self.sequence,"previous":self.previous,"event":event});
            let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&body)?));
            let mut row = body;
            row["hash"] = json!(hash);
            writeln!(self.journal, "{}", row)?;
            self.journal.sync_data()?;
            writeln!(self.events, "{}", row["event"])?;
            self.events.sync_data()?;
            self.previous = hash;
            self.last_processed = at;
        }
        Ok(())
    }
}
fn drain_stream(
    stream: &Option<Realtime>,
    source: &mut RestSource,
    log: &mut EventLog,
) -> Result<()> {
    if let Some(stream) = stream {
        if now() >= stream.deadline_ms {
            invalidate_all(source, log, &iso(now()))?;
            return Err("CAPTURE_STREAM_DEADLINE".into());
        }
        if !stream.healthy() {
            invalidate_all(source, log, &iso(now()))?;
            return Err("CAPTURE_STREAM_STOPPED".into());
        }
        while let Ok(event) = stream.incoming.try_recv() {
            if !stream.healthy() {
                invalidate_all(source, log, &iso(now()))?;
                return Err("CAPTURE_STREAM_STOPPED".into());
            }
            match event {
                Incoming::Joined { received_at } => invalidate_all(source, log, &received_at)?,
                Incoming::Broadcast {
                    event,
                    payload,
                    received_at,
                } => log.append(source.broadcast(&event, &payload, &received_at)?)?,
                Incoming::Failed(code) => {
                    invalidate_all(source, log, &iso(now()))?;
                    return Err(code.into());
                }
            }
        }
        if !stream.healthy() {
            invalidate_all(source, log, &iso(now()))?;
            return Err("CAPTURE_STREAM_STOPPED".into());
        }
    }
    Ok(())
}
fn invalidate_all(source: &mut RestSource, log: &mut EventLog, received: &str) -> Result<()> {
    let mut ids = std::collections::BTreeSet::new();
    for gap in source.subscribed(received)? {
        ids.insert(gap["exchangeId"].as_str().ok_or("GAP_SCOPE")?.to_owned());
    }
    if let Some(paper) = &log.paper {
        ids.extend(paper.engine().known_exchanges());
    }
    if !ids.is_empty() {
        log.append(vec![
            json!({"type":"gap","exchangeIds":ids,"receivedAt":received}),
        ])?;
    }
    Ok(())
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
#[derive(Clone)]
struct PlannedRead {
    pair: usize,
    book: bool,
    url: Option<reqwest::Url>,
    page: usize,
}
fn prioritize_exit_book(
    queue: &mut std::collections::VecDeque<PlannedRead>,
    pairs: &[Value],
    windows: &[(String, i64, i64)],
    budget: &Value,
    at: u64,
) -> Result<bool> {
    let global = admission(budget, at)?;
    for (exchange, activation, end) in windows {
        let Some(pair) = pairs.iter().position(|p| p["exchangeId"] == *exchange) else {
            continue;
        };
        let route = format!(
            "/markets/{}/orderbook",
            pairs[pair]["marketId"].as_str().ok_or("PAIR_MARKET")?
        );
        let next = global.max(budget["endpointNextAt"][&route].as_u64().unwrap_or(0));
        if next < *activation as u64 || next >= *end as u64 {
            continue;
        }
        if let Some(index) = queue.iter().position(|q| q.pair == pair && q.book) {
            if index > 0 {
                let item = queue.remove(index).ok_or("CAPTURE_PLAN")?;
                queue.push_front(item);
                return Ok(true);
            }
            return Ok(false);
        }
    }
    Ok(false)
}
fn capture(
    state: &Path,
    pairs_file: &Path,
    out: &Path,
    cycles: usize,
    realtime: bool,
) -> Result<()> {
    let control = guard(state)?;
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
    let _lock = ExclusiveLock::acquire(&state.join("capture.lock"))?;
    let bp = state.join("sig-read-budget.json");
    let mut b = if bp.exists() {
        read(&bp)?
    } else {
        json!({"times":[],"nextAt":0,"blocked":false,"endpointNextAt":{}})
    };
    if b["blocked"] != false {
        return Err("CAPTURE_BUDGET_BLOCKED".into());
    }
    let paper = if let Some(config) = control.get("paperSession") {
        let directory = config["directory"]
            .as_str()
            .ok_or("PAPER_DIRECTORY_REQUIRED")?;
        let seed = config["seedSnapshotFile"]
            .as_str()
            .ok_or("PAPER_SEED_REQUIRED")?;
        let policy = read(Path::new(
            config["policyFile"]
                .as_str()
                .ok_or("PAPER_POLICY_REQUIRED")?,
        ))?;
        Some(Session::open(
            Path::new(directory),
            Some(Path::new(seed)),
            &policy,
        )?)
    } else {
        None
    };
    if let Some(paper) = &paper {
        capture_scope::validate(
            &Value::Array(pairs.clone()),
            paper.engine().policy(),
            &paper.engine().required_exchanges(),
        )?;
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
    if let Some(paper) = &paper {
        source.seed_trade_facts(paper.source_facts())?;
    }
    let events = OpenOptions::new()
        .append(true)
        .create_new(true)
        .open(out.join("events.jsonl"))?;
    let journal = OpenOptions::new()
        .append(true)
        .create_new(true)
        .open(out.join("source-journal.jsonl"))?;
    use sha2::{Digest, Sha256};
    let source_id = format!(
        "capture:{:x}",
        Sha256::digest(serde_json::to_vec(
            &json!({"directory":fs::canonicalize(out)?.to_str().ok_or("CAPTURE_DIRECTORY_ENCODING")?,"startedAt":started_at,"pairs":pairs,"realtime":realtime})
        )?)
    );
    let financial_integration = paper.is_some();
    let paper_manifest = paper.as_ref().map(|p| p.manifest_hash().clone());
    let mut event_log = EventLog {
        events,
        journal,
        sequence: 0,
        previous: String::new(),
        last_processed: 0,
        paper,
        source_id: source_id.clone(),
        #[cfg(test)]
        test_clock: None,
    };
    save(
        &out.join("manifest.json"),
        &json!({"runtime":"rust","mode":"official_sig_capture","sourceId":source_id,"realtime":realtime,"startedAt":started_at,"pairs":pairs,"tournamentId":TOURNAMENT,"readLimitPerMinute":100,"contestOrders":0,"financialIntegration":financial_integration,"paperManifestHash":paper_manifest,"authoritativeJournal":if financial_integration{"paper session events.jsonl"}else{"source-journal.jsonl"},"tokenPersistence":false}),
    )?;
    invalidate_all(&mut source, &mut event_log, &iso(now()))?;
    let mut stream = None;
    if realtime {
        loop {
            let t = now();
            let at = admission(&b, t)?.max(b["nextAuthAt"].as_u64().unwrap_or(0));
            if at <= t {
                break;
            }
            guard(state)?;
            if out.join("STOP").exists() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis((at - t).min(250)));
        }
        b["nextAuthAt"] = json!(now() + 60000);
        b["nextAt"] = json!(now() + 600);
        save(&bp, &b)?;
        let packet = auth_packet(&client, &credential);
        let Some(auth) = packet.data else {
            b["blocked"] = json!(true);
            b["nextAt"] = json!(now().saturating_add(packet.delay));
            save(&bp, &b)?;
            return Err("CAPTURE_AUTH_FAILED_LATCHED".into());
        };
        let connected = Realtime::connect(Auth::parse(auth)?, TOURNAMENT)?;
        match connected.incoming.recv_timeout(Duration::from_secs(12)) {
            Ok(Incoming::Joined { received_at }) => {
                invalidate_all(&mut source, &mut event_log, &received_at)?
            }
            _ => return Err("CAPTURE_STREAM_JOIN_FAILED".into()),
        }
        stream = Some(connected);
    }
    let deadline = (now() + cycles as u64 * pairs.len() as u64 * 24000)
        .min(stream.as_ref().map_or(u64::MAX, |s| s.deadline_ms));
    let mut queue = std::collections::VecDeque::new();
    for _ in 0..cycles * 20 {
        for pair in 0..pairs.len() {
            queue.push_back(PlannedRead {
                pair,
                book: true,
                url: None,
                page: 1,
            });
            queue.push_back(PlannedRead {
                pair,
                book: false,
                url: None,
                page: 1,
            });
        }
    }
    if let Some(paper) = &event_log.paper {
        let mut initial = std::collections::VecDeque::new();
        for (pair, p) in pairs.iter().enumerate() {
            if let Some((from, to)) = paper.engine().expired_recovery_window(
                p["exchangeId"].as_str().ok_or("PAIR_EXCHANGE")?,
                now() as i64,
            )? {
                let mut url = reqwest::Url::parse(&format!(
                    "https://sig.thesuper.market/api/v1/exchanges/{}/trades",
                    p["exchangeId"].as_str().ok_or("PAIR_EXCHANGE")?
                ))?;
                url.query_pairs_mut()
                    .append_pair("tournamentId", TOURNAMENT)
                    .append_pair("limit", "200")
                    .append_pair("from", &from)
                    .append_pair("to", &to);
                initial.push_back(PlannedRead {
                    pair,
                    book: true,
                    url: None,
                    page: 1,
                });
                initial.push_back(PlannedRead {
                    pair,
                    book: false,
                    url: Some(url),
                    page: 1,
                });
            }
        }
        initial.append(&mut queue);
        queue = initial;
    }
    while !queue.is_empty() {
        if now() >= deadline || out.join("STOP").exists() {
            break;
        }
        guard(state)?;
        drain_stream(&stream, &mut source, &mut event_log)?;
        if let Some(paper) = &event_log.paper {
            prioritize_exit_book(
                &mut queue,
                pairs,
                &paper.engine().exit_book_windows(),
                &b,
                now(),
            )?;
        }
        let planned = queue.pop_front().ok_or("CAPTURE_PLAN")?;
        let continuation = planned.url.map(|url| (planned.pair, url, planned.page));
        let pair_index = planned.pair;
        let p = &pairs[pair_index];
        let book = planned.book;
        let route = if book {
            format!("/markets/{}/orderbook", p["marketId"].as_str().unwrap())
        } else {
            format!("/exchanges/{}/trades", p["exchangeId"].as_str().unwrap())
        };
        loop {
            guard(state)?;
            drain_stream(&stream, &mut source, &mut event_log)?;
            let t = now();
            let at = admission(&b, t)?.max(b["endpointNextAt"][&route].as_u64().unwrap_or(0));
            if at <= t {
                break;
            }
            if out.join("STOP").exists() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis((at - t).min(100)))
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
                let from = if let Some(paper) = &event_log.paper {
                    paper.engine().recovery_from(
                        p["exchangeId"].as_str().ok_or("PAIR_EXCHANGE")?,
                        started as i64,
                    )?
                } else {
                    iso(started.saturating_sub(900000))
                };
                q.append_pair("from", &from);
                q.append_pair("to", &iso(started));
            }
        }
        let packet = get(&client, &url, &credential);
        match packet.data {
            Some(data) => {
                let receipt = json!({"schemaVersion":1,"source":"official_sig_api_v1","kind":if book{"book"}else{"trades"},"endpoint":format!("{}?{}",route,url.query().unwrap()),"tournamentId":TOURNAMENT,"marketId":p["marketId"],"exchangeId":p["exchangeId"],"requestStartedAt":iso(started),"receivedAt":iso(now()),"status":200,"data":data});
                writeln!(log, "{}", receipt)?;
                log.sync_data()?;
                drain_stream(&stream, &mut source, &mut event_log)?;
                event_log.append(source.accept(&receipt)?)?;
                save(
                    &out.join("stats.json"),
                    &json!({"runtime":"rust","phase":"capturing","at":iso(now()),"sequence":event_log.sequence,"previous":event_log.previous,"scopes":source.scopes(),"realtime":realtime,"contestOrders":0}),
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
                    queue.push_front(PlannedRead {
                        pair: pair_index,
                        url: Some(next_url),
                        book: false,
                        page: page + 1,
                    });
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
        &json!({"runtime":"rust","phase":"completed","at":iso(now()),"sequence":event_log.sequence,"previous":event_log.previous,"scopes":source.scopes(),"realtime":realtime,"contestOrders":0}),
    )?;
    Ok(())
}
fn iso(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .expect("timestamp")
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn run_paper(state: &Path) -> Result<()> {
    loop {
        let control = guard(state)?;
        if !control["paperSession"].is_object() {
            return Err("PAPER_SESSION_REQUIRED".into());
        }
        let pairs_file = control["pairsFile"]
            .as_str()
            .map(PathBuf::from)
            .unwrap_or_else(|| state.join("pairs.json"));
        let pairs = read(&pairs_file)?;
        let count = pairs
            .as_array()
            .filter(|p| !p.is_empty() && p.len() <= 4)
            .ok_or("SCOPES")?
            .len();
        let cycles = control["captureCycles"]
            .as_u64()
            .map(|v| v as usize)
            .unwrap_or(60 / count);
        if cycles == 0 || cycles > 60 / count {
            return Err("CAPTURE_CYCLES".into());
        }
        let captures = state.join("captures");
        fs::create_dir_all(&captures)?;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let out = captures.join(format!("{}-{nonce}", std::process::id()));
        let result = capture(state, &pairs_file, &out, cycles, true);
        let controlled_deadline = result
            .as_ref()
            .err()
            .is_some_and(|e| e.to_string() == "CAPTURE_STREAM_DEADLINE");
        if result.is_err() {
            if out.is_dir() {
                let mut stats = if out.join("stats.json").exists() {
                    read(&out.join("stats.json")).unwrap_or(json!({}))
                } else {
                    json!({})
                };
                stats["runtime"] = json!("rust");
                stats["phase"] = json!(if controlled_deadline {
                    "completed"
                } else {
                    "failed"
                });
                stats["at"] = json!(iso(now()));
                stats["code"] = json!(if controlled_deadline {
                    "TOKEN_WINDOW_CLOSED"
                } else {
                    "CAPTURE_STOPPED_FAIL_CLOSED"
                });
                let _ = save(&out.join("stats.json"), &stats);
            }
            if !controlled_deadline {
                return result;
            }
        }
        let after = guard(state)?;
        if after["continuousPaper"] != true {
            return Ok(());
        }
    }
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
        Some("run") if args.len() == 1 => run_paper(&state),
        Some("benchmark") if args.len() == 2 => benchmark(Path::new(&args[1])),
        Some("check") => {
            let c = read(&state.join("deployment.json"))?;
            println!(
                "{}",
                json!({"runtime":"rust","networkEnabled":c["networkEnabled"],"tradingEnabled":c["tradingEnabled"],"configuredReadsPerMinute":c["readsPerMinute"],"networkStopMarker":state.join("NETWORK_DISABLED").exists(),"executionStopMarker":state.join("EXECUTION_STOP").exists()})
            );
            Ok(())
        }
        Some("capture") if args.len() == 4 || args.len() == 5 && args[4] == "--realtime" => {
            capture(
                &state,
                Path::new(&args[1]),
                Path::new(&args[2]),
                args[3].parse()?,
                args.len() == 5,
            )
        }
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
    fn test_log(root: &Path, policy: &Value) -> EventLog {
        EventLog {
            events: OpenOptions::new()
                .append(true)
                .create_new(true)
                .open(root.join("normalized.jsonl"))
                .unwrap(),
            journal: OpenOptions::new()
                .append(true)
                .create_new(true)
                .open(root.join("source.jsonl"))
                .unwrap(),
            sequence: 0,
            previous: String::new(),
            last_processed: 0,
            paper: Some(Session::open(&root.join("bank"), None, policy).unwrap()),
            source_id: "fixture".into(),
            test_clock: None,
        }
    }
    #[test]
    fn complete_source_to_persistent_paper_path_matches_reference() {
        let root = std::env::temp_dir().join(format!(
            "moloch-collector-integration-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let corpus: Value =
            serde_json::from_str(include_str!("../../tests/reference/realtime-golden.json"))
                .unwrap();
        let c = &corpus["config"];
        let mut source = RestSource::new(
            &c["pairs"],
            c["tournamentId"].as_str().unwrap(),
            c["startedAt"].as_str().unwrap(),
        )
        .unwrap();
        let mut expected = moloch_replay::engine::Engine::new(&json!({})).unwrap();
        let mut log = test_log(&root, &json!({}));
        for step in corpus["steps"].as_array().unwrap() {
            let input = &step["input"];
            let received = if input["kind"] == "rest" {
                &input["receipt"]["receivedAt"]
            } else {
                &input["receivedAt"]
            };
            log.test_clock = Some(
                chrono::DateTime::parse_from_rfc3339(received.as_str().unwrap())
                    .unwrap()
                    .timestamp_millis() as u64,
            );
            match input["kind"].as_str().unwrap() {
                "subscribed" => {
                    invalidate_all(&mut source, &mut log, received.as_str().unwrap()).unwrap()
                }
                "rest" => log
                    .append(source.accept(&input["receipt"]).unwrap())
                    .unwrap(),
                "broadcast" => log
                    .append(
                        source
                            .broadcast(
                                input["event"].as_str().unwrap(),
                                &input["payload"],
                                received.as_str().unwrap(),
                            )
                            .unwrap(),
                    )
                    .unwrap(),
                _ => panic!("fixture"),
            };
            for event in step["expected"].as_array().unwrap() {
                expected.advance(event).unwrap();
            }
            let actual = log.paper.as_ref().unwrap().engine().report().unwrap();
            let reference = expected.report().unwrap();
            for field in [
                "events",
                "cash",
                "realized",
                "fees",
                "ownedCost",
                "reservedCash",
                "positions",
                "fills",
                "orders",
                "activeOrders",
                "unresolvedOrders",
                "counts",
            ] {
                assert_eq!(actual[field], reference[field], "pipeline field {field}");
            }
        }
        let before = log.paper.as_ref().unwrap().engine().report().unwrap();
        drop(log);
        let reopened = Session::open(&root.join("bank"), None, &json!({})).unwrap();
        assert_eq!(reopened.engine().report().unwrap(), before);
        drop(reopened);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn startup_invalidation_is_one_event_and_covers_unobserved_cached_books() {
        let root = std::env::temp_dir().join(format!(
            "moloch-atomic-startup-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let at = "2026-10-14T00:00:00.000Z";
        let mut log = test_log(&root, &json!({"entriesEnabled":false}));
        log.paper.as_mut().unwrap().accept("old",1,&json!({"type":"book","receivedAt":at,"book":{"tournamentId":"cup","marketId":"99","exchangeId":"99","sequence":"1","requestStartedAt":at,"sourceAt":at,"receivedAt":at,"bids":[{"price":"0.450000","quantity":100}],"asks":[{"price":"0.455000","quantity":100}]}})).unwrap();
        let mut source = RestSource::new(
            &json!([{"marketId":"11","exchangeId":"22"},{"marketId":"33","exchangeId":"44"}]),
            "cup",
            at,
        )
        .unwrap();
        let received = "2026-10-14T00:00:01.000Z";
        log.test_clock = Some(
            chrono::DateTime::parse_from_rfc3339(received)
                .unwrap()
                .timestamp_millis() as u64,
        );
        invalidate_all(&mut source, &mut log, received).unwrap();
        assert_eq!(log.sequence, 1);
        let raw = fs::read_to_string(root.join("source.jsonl")).unwrap();
        let row: Value = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(row["event"]["exchangeIds"], json!(["22", "44", "99"]));
        assert_eq!(
            log.paper.as_ref().unwrap().engine().report().unwrap()["events"],
            2
        );
        drop(log);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn exit_priority_moves_existing_reads_without_adding_or_dropping_tape() {
        use std::collections::VecDeque;
        let pairs = vec![
            json!({"marketId":"1","exchangeId":"2"}),
            json!({"marketId":"3","exchangeId":"4"}),
        ];
        let mut queue = VecDeque::from([
            PlannedRead {
                pair: 0,
                book: false,
                url: None,
                page: 1,
            },
            PlannedRead {
                pair: 1,
                book: true,
                url: None,
                page: 1,
            },
            PlannedRead {
                pair: 0,
                book: true,
                url: None,
                page: 1,
            },
        ]);
        let budget = json!({"times":[],"nextAt":0,"endpointNextAt":{}});
        assert!(
            prioritize_exit_book(
                &mut queue,
                &pairs,
                &[("2".into(), 1000, 2000)],
                &budget,
                1000
            )
            .unwrap()
        );
        assert_eq!(queue.len(), 3);
        assert!(queue[0].book);
        assert_eq!(queue[0].pair, 0);
        assert!(!queue[1].book);
        assert!(
            !prioritize_exit_book(
                &mut queue,
                &pairs,
                &[("4".into(), 1000, 1100)],
                &json!({"times":[],"nextAt":1200}),
                1000
            )
            .unwrap()
        );
    }
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
