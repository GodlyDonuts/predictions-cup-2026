// bot/bot.mjs — read-only scan loop: verify contract metadata, pair opposing
// parties, plan packages, log research candidates. Never places orders.
use crate::api::{load_key, Api};
use crate::strategy::{metadata_pairs, plan_pair};
use serde_json::{json, Value as V};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct Bot<FNow, FSleep>
where
    FNow: Fn() -> f64 + Clone,
    FSleep: Fn(f64) + Clone,
{
    pub cfg: V,
    pub api: Api<FNow, FSleep>,
    pub dir: PathBuf,
    stopping: Arc<AtomicBool>,
    snapshot_sequence: u64,
}

/// Process-wide single-instance lock (LOCALAPPDATA/SigTradingCup/bot.lock).
pub struct BotLock {
    path: PathBuf,
}
impl BotLock {
    pub fn acquire() -> Result<BotLock, String> {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
        let path = Path::new(&base).join("SigTradingCup").join("bot.lock");
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                use std::io::Write;
                let _ = write!(f, "{}", std::process::id());
                Ok(BotLock { path })
            }
            Err(_) => Err("Another local scanner may be running. Verify its PID before removing LocalAppData/SigTradingCup/bot.lock.".into()),
        }
    }
}
impl Drop for BotLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl<FNow: Fn() -> f64 + Clone, FSleep: Fn(f64) + Clone> Bot<FNow, FSleep> {
    /// Equivalent of bot.mjs main(): STOP short-circuits to read_only exit.
    pub fn open(dir: &Path, cfg_path: &Path, now: FNow, sleep: FSleep, fetch: Arc<dyn baal_service_runtime::client::Fetch>, stopping: Arc<AtomicBool>) -> Result<Option<Bot<FNow, FSleep>>, String> {
        if dir.join("STOP").exists() {
            println!("{}", json!({"event":"stopped","reason":"STOP_present","mode":"read_only"}));
            return Ok(None);
        }
        let cfg: V = serde_json::from_str(&fs::read_to_string(cfg_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        fs::create_dir_all(dir.join("data/sources")).map_err(|e| e.to_string())?;
        fs::create_dir_all(dir.join("logs")).map_err(|e| e.to_string())?;
        let key = load_key()?;
        let api = Api::new(
            cfg["baseUrl"].as_str().unwrap_or(""),
            &key,
            cfg["readIntervalMs"].as_f64().unwrap_or(1500.0),
            now,
            sleep,
            fetch,
        )?;
        Ok(Some(Bot { cfg, api, dir: dir.to_path_buf(), stopping, snapshot_sequence: 0 }))
    }

    fn log(&self, event: &str, fields: V) {
        let mut row = json!({"at": iso_now(), "event": event});
        if let V::Object(m) = fields {
            for (k, v) in m {
                row[k] = v;
            }
        }
        let line = serde_json::to_string(&row).unwrap();
        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("logs/bot.jsonl"))
            .and_then(|mut f| std::io::Write::write_all(&mut f, format!("{line}\n").as_bytes()));
        println!("{line}");
    }

    fn save(&self, name: &str, data: &V) {
        let tmp = self.dir.join(format!("data/{name}.tmp"));
        let dst = self.dir.join(format!("data/{name}.json"));
        let _ = fs::write(&tmp, serde_json::to_string_pretty(data).unwrap());
        let _ = fs::rename(&tmp, &dst);
    }

    fn read(&self, name: &str, fallback: V) -> V {
        self.dir
            .join(format!("data/{name}.json"))
            .exists()
            .then(|| fs::read_to_string(self.dir.join(format!("data/{name}.json"))).ok())
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(fallback)
    }

    fn should_stop(&self) -> bool {
        self.stopping.load(Ordering::SeqCst) || self.dir.join("STOP").exists()
    }

    fn scope(&self) -> String {
        format!("tournamentId={}", self.cfg["tournamentId"].as_str().unwrap_or(""))
    }

    /// Source read: records endpoint + requestStartedAt/receivedAt + raw
    /// response into data/sources/ with a unique sequence-tagged filename.
    fn source(&mut self, endpoint: &str) -> Result<V, String> {
        let request_started_at = iso_now();
        let response = self.api.request(endpoint).map_err(|e| e.to_string())?;
        let received_at = iso_now();
        let file = self.dir.join(format!(
            "data/sources/{}-{}.json",
            received_at.replace(':', "-"),
            self.snapshot_sequence
        ));
        self.snapshot_sequence += 1;
        let record = json!({"endpoint": endpoint, "requestStartedAt": request_started_at,
            "receivedAt": received_at, "response": response});
        match fs::OpenOptions::new().write(true).create_new(true).open(&file) {
            Ok(mut f) => {
                let _ = std::io::Write::write_all(&mut f, serde_json::to_string_pretty(&record).unwrap().as_bytes());
            }
            Err(e) => return Err(e.to_string()),
        }
        Ok(json!({"response": response, "requestStartedAt": request_started_at, "receivedAt": received_at}))
    }

    /// One scan cycle. Returns early when STOP appears mid-scan.
    pub fn cycle(&mut self, once: bool, sample: bool) -> Result<(), String> {
        let slug = self.cfg["tournamentSlug"].as_str().unwrap_or("").to_string();
        let tournament = self.source(&format!("/tournaments/{slug}"))?["response"].clone();
        if tournament["id"].as_str() != self.cfg["tournamentId"].as_str().map(Some).unwrap_or(None)
            || tournament["currencyName"].as_str() != Some("SUSQies")
        {
            return Err("Tournament identity mismatch".into());
        }
        let portfolio_read = self.source(&format!("/tournaments/{slug}/portfolio/positions"))?;
        let portfolio = portfolio_read["response"].clone();
        let mut pf = portfolio.clone();
        pf["observedAt"] = portfolio_read["receivedAt"].clone();
        pf["cash"] = tournament["myBalance"].clone();
        self.save("portfolio", &pf);
        let orders = self
            .api
            .pages(&format!("/orders?status=open&limit=200&{}", self.scope()))
            .map_err(|e| e.to_string())?;
        self.save("orders", &json!({"observedAt": iso_now(), "orders": orders}));
        let markets = self
            .api
            .pages(&format!("/markets?status=open&limit=100&{}", self.scope()))
            .map_err(|e| e.to_string())?;
        self.save("markets", &V::Array(markets.clone()));
        let mut nodes = self.read("nodes", json!({}));
        let mut candidates: Vec<V> = markets
            .iter()
            .filter(|m| {
                m["title"].as_str().map(|t| {
                    (t.starts_with("Will the Democratic Party win ")
                        || t.starts_with("Will the Republican Party win "))
                }) == Some(true)
                    && !m["isComposite"].as_bool().unwrap_or(false)
                    && m["exchanges"].as_array().map(|e| e.len()) == Some(1)
            })
            .cloned()
            .collect();
        if sample {
            candidates.truncate(20);
        }
        // Sequential verification (JS used 4 paced Promise.allSettled workers;
        // the admission queue serializes pacing identically in one thread).
        let mut loaded = 0u64;
        for m in &candidates {
            if self.should_stop() {
                return Ok(());
            }
            let id = m["id"].as_str().unwrap_or("");
            if let Some(at) = nodes[id]["observedAt"].as_str() {
                if let Some(t) = baal_service_runtime::contract::parse_iso_ms(at) {
                    if now_ms((self.api.now)()) - t < 12.0 * 3600000.0 {
                        continue;
                    }
                }
            }
            let s = self.source(&format!("/markets/{id}/nodes?{}", self.scope()))?;
            let mut entry = s["response"].clone();
            entry["observedAt"] = s["receivedAt"].clone();
            nodes[id] = entry;
            self.save("nodes", &nodes);
            loaded += 1;
            if loaded % 25 == 0 {
                self.log("contract_verification", json!({"verified": loaded, "total": candidates.len()}));
            }
        }
        if self.should_stop() {
            return Ok(());
        }
        let pairs = metadata_pairs(if sample { &candidates } else { &markets }, &nodes);
        self.save("pairs", &V::Array(pairs.iter().map(|p| V::Array(p.clone())).collect::<Vec<_>>().into()));
        self.log(
            "scan_start",
            json!({"mode":"read_only","balance":tournament["myBalance"],"markets":markets.len(),"metadataPairs":pairs.len()}),
        );
        let mut opportunities = Vec::new();
        let mut scanned = 0u64;
        for pair in &pairs {
            if self.should_stop() {
                return Ok(());
            }
            let mut books = json!({});
            let mut observation_window = Vec::new();
            for leg in pair {
                let ex = leg["exchangeId"].as_str().unwrap_or("");
                let s = self.source(&format!("/exchanges/{ex}/orderbook?depth=5&{}", self.scope()))?;
                books[ex] = s["response"].clone();
                observation_window.push(json!({"exchangeId": ex, "requestStartedAt": s["requestStartedAt"],
                    "receivedAt": s["receivedAt"], "asOf": s["response"]["asOf"]}));
            }
            let positions = portfolio["positions"].as_array().cloned().unwrap_or_default();
            if let Some(plan) = plan_pair(pair, &books, &self.cfg, &tournament["myBalance"], &positions, &orders) {
                let mut candidate = plan.clone();
                candidate["observationWindow"] = V::Array(observation_window);
                candidate["feesVerified"] = json!(false);
                candidate["eligibleForExecution"] = json!(false);
                candidate["limitations"] = json!([
                    "Exact settlement rules and exclusive outcomes are unverified",
                    "Sequential snapshots can be stale",
                    "Partial fills can create directional exposure",
                    "Fee schedule and exceptional settlement cases need verification"
                ]);
                opportunities.push(candidate.clone());
                self.log(
                    "research_candidate",
                    json!({"titles": pair.iter().map(|p| p["title"].clone()).collect::<Vec<_>>(),
                        "classification": plan["classification"], "quantity": plan["quantity"],
                        "cost": plan["worstCaseCost"], "quoteDifference": plan["quoteDifference"]}),
                );
            }
            scanned += 1;
            if scanned % 20 == 0 {
                self.log("book_scan_progress", json!({"scanned": scanned, "total": pairs.len()}));
            }
        }
        self.save("opportunities", &V::Array(opportunities.clone()));
        self.save(
            "status",
            &json!({"at": iso_now(), "mode":"read_only","sample":sample,"balance":tournament["myBalance"],
                "markets":markets.len(),"contractsScanned":candidates.len(),"pairs":pairs.len(),
                "opportunities":opportunities.len(),"openOrders":orders.len()}),
        );
        self.log("scan_complete", json!({"opportunities": opportunities.len(), "ordersSubmitted": 0}));
        let _ = once;
        Ok(())
    }

    /// do{cycle(); if(once||shouldStop())break; sleep(cycleIntervalMs);}while(!shouldStop())
    pub fn run(&mut self, once: bool, sample: bool) -> i32 {
        self.log("started", json!({"mode":"read_only","pid": std::process::id()}));
        loop {
            match self.cycle(once, sample) {
                Ok(()) => {}
                Err(e) => {
                    self.log("stopped_error", json!({"message": e}));
                    return 1;
                }
            }
            if once || self.should_stop() {
                break;
            }
            (self.api.queue.sleep)(self.cfg["cycleIntervalMs"].as_f64().unwrap_or(0.0));
        }
        self.log("stopped", json!({}));
        0
    }
}

fn iso_now() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0);
    baal_service_runtime::contract::ms_to_iso(ms)
}

fn now_ms(now: f64) -> f64 {
    now
}
