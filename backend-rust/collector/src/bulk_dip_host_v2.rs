// live-paper/bulk-dip-host-v2.mjs — bulk-dip cohort host v2: reference barriers
// bind source+consumer boundaries; corrected pending-map projection.
use serde_json::{json, Value as V};
use std::fs::OpenOptions;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

fn need(ok: bool, why: &str) -> Result<(), String> {
    if !ok { return Err(format!("BULK_HOST_V2_{}", why)); }
    Ok(())
}
fn sha(b: &[u8]) -> String { baal_service_runtime::json_text::sha256(b) }
fn digest(v: &V) -> String { baal_service_runtime::json_text::digest(v) }
fn parse_at(v: &V) -> f64 {
    baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).unwrap_or(f64::NAN)
}
fn complete(file: &Path) -> Result<Vec<u8>, String> {
    let b = std::fs::read(file).map_err(|e| e.to_string())?;
    let end = b.iter().rposition(|x| *x == b'\n').map(|p| p + 1).unwrap_or(0);
    Ok(b[..end].to_vec())
}
fn rows(b: &[u8]) -> Result<Vec<V>, String> {
    if b.is_empty() { return Ok(vec![]); }
    let text = String::from_utf8(b.to_vec()).map_err(|e| e.to_string())?;
    let mut parts: Vec<&str> = text.split('\n').collect();
    parts.pop();
    parts.iter().map(|x| serde_json::from_str(x).map_err(|e| e.to_string())).collect()
}
fn write(file: &Path, bytes: &[u8], append: bool, create_new: bool) -> Result<(), String> {
    let mut o = OpenOptions::new();
    if create_new { o.write(true).create_new(true); } else if append { o.append(true); } else { o.write(true).truncate(true); }
    #[cfg(unix)]
    o.mode(0o600);
    let mut fd = o.open(file).map_err(|e| e.to_string())?;
    fd.write_all(bytes).map_err(|_| "BULK_HOST_V2_WRITE".to_string())?;
    fd.sync_all().map_err(|e| e.to_string())
}
fn read_json(file: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let r = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos()
        ^ ((std::process::id() as u128) << 64);
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (r & 0xffffffff) as u32, (r >> 32 & 0xffff) as u16, (r >> 48 & 0xffff) as u16,
        (r >> 64 & 0xffff) as u16, (r >> 80 & 0xffffffffffff) as u64)
}
fn identity(workspace_dir: &Path) -> Result<V, String> {
    let keys = ["./bulk-dip-host-v2.mjs", "./bulk-dip.mjs", "./bid-anchor.mjs", "./bid-anchor-live.mjs",
        "./market-price-publication.mjs", "./breadth.mjs", "./runtime.mjs", "./edge-depth-audit.mjs",
        "./model.mjs", "./book-publication.mjs", "../paper/fixed.mjs", "../feed/contract.mjs",
        "../execution/ledger.mjs"];
    let files = ["live-paper/bulk-dip-host-v2.mjs", "live-paper/bulk-dip.mjs", "live-paper/bid-anchor.mjs",
        "live-paper/bid-anchor-live.mjs", "live-paper/market-price-publication.mjs",
        "live-paper/breadth.mjs", "live-paper/runtime.mjs", "live-paper/edge-depth-audit.mjs",
        "live-paper/model.mjs", "live-paper/book-publication.mjs", "paper/fixed.mjs",
        "feed/contract.mjs", "execution/ledger.mjs"];
    let mut m = serde_json::Map::new();
    for (k, p) in keys.iter().zip(files.iter()) {
        m.insert(k.to_string(), json!(sha(&std::fs::read(workspace_dir.join(p)).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(m))
}
fn merge(books: &[V], prices: &[V]) -> Vec<(bool, V)> {
    let mut all: Vec<(bool, V)> = books.iter().cloned().map(|r| (true, r))
        .chain(prices.iter().cloned().map(|r| (false, r))).collect();
    all.sort_by(|a, b| {
        parse_at(&a.1["at"]).partial_cmp(&parse_at(&b.1["at"])).unwrap_or(std::cmp::Ordering::Equal)
            .then((!a.0).cmp(&!b.0))
            .then(a.1["sequence"].as_i64().unwrap_or(0).cmp(&b.1["sequence"].as_i64().unwrap_or(0)))
    });
    all
}
fn stat_of(file: &Path) -> Result<(u64, f64, u64), String> {
    let m = std::fs::metadata(file).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    return Ok((m.ino(), m.created().map_err(|e| e.to_string())?
        .duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs_f64() * 1000.0, m.len()));
    #[cfg(not(unix))]
    return Ok((0, 0.0, m.len()));
}

/// openBulkDip v2 host.
pub struct BulkDipHostV2 {
    pub dir: PathBuf,
    pub bank: V,
    state: V,
    runtime_dir: PathBuf,
    workspace_dir: PathBuf,
    cohort: String,
    code_identity: V,
    book_state: V,
    price_state: V,
    universe: V,
    market_by_exchange: std::collections::BTreeMap<String, String>,
    sequence: i64,
    previous: String,
    journal_bytes: u64,
    journal_bytes_hash: V,
    offset_book: usize,
    offset_price: usize,
    prefix_hash_book: String,
    prefix_hash_price: String,
    bank_bytes_hash: String,
    source_stat_book: (u64, f64, u64),
    source_stat_price: (u64, f64, u64),
    lock_file: PathBuf,
    token: String,
    closed: bool,
    failed: bool,
    now: Box<dyn Fn() -> f64>,
}
impl BulkDipHostV2 {
    fn bank_file(&self) -> PathBuf { self.dir.join("bank.json") }
    fn journal_file(&self) -> PathBuf { self.dir.join("journal.jsonl") }
    fn source_file(&self, kind: &str) -> PathBuf {
        self.runtime_dir.join(if kind == "book" { "public-books.jsonl" } else { "market-prices.jsonl" })
    }
    pub fn stopped(&self) -> bool {
        [self.workspace_dir.join("STOP"), self.runtime_dir.join("STOP"),
            self.runtime_dir.join("BULK_DIP_STOP"), self.dir.join("STOP")]
            .iter().any(|p| p.exists())
    }
    fn live(&self) -> Result<(), String> {
        need(!self.closed && !self.failed && !self.stopped()
            && std::fs::read_to_string(&self.lock_file).map(|s| s == self.token).unwrap_or(false),
            "OWNERSHIP")
    }
    fn expected(&self) -> V {
        json!({"bookBytes": self.offset_book, "bookSequence": self.book_state["sequence"],
            "bookHash": self.book_state["hash"], "priceBytes": self.offset_price,
            "priceSequence": self.price_state["sequence"], "priceHash": self.price_state["hash"],
            "stateHash": digest(&self.state)})
    }
    fn validate(&mut self, kind: &str, row: &V) -> Result<(), String> {
        if kind == "book" {
            self.book_state = crate::book_publication_v2::apply_book_publication(&self.book_state, row)?;
            return Ok(());
        }
        let mut body = row.clone();
        let hash = body.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
        let at_ok = baal_service_runtime::contract::parse_iso_ms(body["at"].as_str().unwrap_or(""));
        let recv = baal_service_runtime::contract::parse_iso_ms(
            body["event"]["snapshot"]["receivedAt"].as_str().unwrap_or(""));
        let prior_at = baal_service_runtime::contract::parse_iso_ms(
            self.price_state["at"].as_str().unwrap_or(""));
        need(body["sequence"].as_i64() == Some(self.price_state["sequence"].as_i64().unwrap_or(0) + 1)
            && body["previous"].as_str() == self.price_state["hash"].as_str()
            && hash.as_str() == Some(digest(&body).as_str())
            && body["state"].is_null()
            && body["event"]["type"].as_str() == Some("market_price_observation")
            && at_ok.is_some()
            && recv.is_some_and(|r| at_ok.unwrap() >= r)
            && (prior_at.is_none() || at_ok.unwrap() >= prior_at.unwrap()),
            "PRICE_CHAIN")?;
        crate::market_price_publication::validate_price_observation_pub(&body["event"]["snapshot"], &self.universe)?;
        self.price_state = json!({"sequence": body["sequence"], "hash": hash, "at": body["at"]});
        Ok(())
    }
    /// filteredPrices(snapshot) — drop quotes/ids predating the reference barriers.
    fn filtered_prices(&mut self, snapshot: &V) -> Option<V> {
        let permitted = |id: &str| -> bool {
            let global = &self.state["referenceBarriers"]["global"];
            let market = &self.state["referenceBarriers"]["markets"][id];
            [global, market].iter().all(|at| at.is_null()
                || parse_at(&snapshot["requestStartedAt"]) > parse_at(at))
        };
        let quotes: Vec<V> = snapshot["quotes"].as_array().cloned().unwrap_or_default().into_iter()
            .filter(|q| permitted(q["marketId"].as_str().unwrap_or(""))).collect();
        let missing_ids: Vec<V> = snapshot["missingIds"].as_array().cloned().unwrap_or_default().into_iter()
            .filter(|id| permitted(self.market_by_exchange
                .get(id.as_str().unwrap_or("")).map(|s| s.as_str()).unwrap_or("\u{0}"))).collect();
        let excluded = snapshot["quotes"].as_array().map(|a| a.len()).unwrap_or(0)
            + snapshot["missingIds"].as_array().map(|a| a.len()).unwrap_or(0)
            - quotes.len() - missing_ids.len();
        if excluded > 0 {
            let k = "price_request_before_source_barrier";
            self.state["bulk"]["counts"][k] = json!(self.state["bulk"]["counts"][k].as_i64().unwrap_or(0) + excluded as i64);
        }
        if quotes.is_empty() && missing_ids.is_empty() { return None; }
        let mut out = snapshot.clone();
        out["quotes"] = json!(quotes);
        out["missingIds"] = json!(missing_ids);
        Some(out)
    }
    fn reduce(&mut self, batch: &[(bool, V)], at: &str) -> Result<(), String> {
        for (is_book, row) in batch {
            need(parse_at(&row["at"]) <= parse_at(&json!(at)), "FUTURE_ROW")?;
            self.validate(if *is_book { "book" } else { "price" }, row)?;
            if !*is_book {
                if let Some(filtered) = self.filtered_prices(&row["event"]["snapshot"]) {
                    crate::bulk_dip::advance_bulk_prices(&mut self.state, &filtered, &self.bank, at, false)?;
                }
                continue;
            }
            let kind = row["kind"].as_str().unwrap_or("");
            if ["session_start", "session_stop"].contains(&kind)
                || (kind == "invalidation" && row["payload"]["marketId"].is_null()) {
                self.state["referenceBarriers"]["global"] = json!(at);
            } else if kind == "invalidation" {
                let m = row["payload"]["marketId"].as_str().unwrap_or("").to_string();
                self.state["referenceBarriers"]["markets"][&m] = json!(at);
            } else if kind == "book"
                && parse_at(&json!(at)) - parse_at(&row["payload"]["book"]["receivedAt"])
                    > self.bank["config"]["maxDataAgeMs"].as_i64().unwrap_or(0) as f64 {
                let id = row["payload"]["book"]["marketId"].as_str().unwrap_or("").to_string();
                self.state["referenceBarriers"]["markets"][&id] = json!(at);
                self.state["bulk"]["history"].as_object_mut().map(|m| m.remove(&id));
                self.state["bulk"]["candidates"].as_object_mut().map(|m| m.remove(&id));
            }
            crate::bulk_dip::advance_bulk_book(&mut self.state, row, &self.bank, at)?;
        }
        Ok(())
    }
    fn verify_journal(&self) -> Result<(), String> {
        let bytes = std::fs::read(self.journal_file()).map_err(|e| e.to_string())?;
        need(bytes.len() as u64 == self.journal_bytes
            && sha(&bytes) == self.journal_bytes_hash.as_str().unwrap_or(""), "JOURNAL_CHANGED")
    }
    fn append(&mut self, body: V) -> Result<(), String> {
        self.live()?;
        self.verify_journal()?;
        let mut item = json!({"sequence": self.sequence + 1, "previous": self.previous,
            "bankHash": digest(&self.bank)});
        for (k, v) in body.as_object().unwrap() { item[k] = v.clone(); }
        let hash = digest(&item);
        item["hash"] = json!(hash);
        let b = format!("{}\n", baal_service_runtime::json_text::js_stringify(&item));
        write(&self.journal_file(), b.as_bytes(), true, false)?;
        self.sequence += 1;
        self.previous = hash;
        self.journal_bytes += b.len() as u64;
        self.journal_bytes_hash = json!(sha(&std::fs::read(self.journal_file()).map_err(|e| e.to_string())?));
        Ok(())
    }
    /// consume() — merged book+price records consumed this pass.
    pub fn consume(&mut self) -> Result<usize, String> {
        self.live()?;
        let r = (|h: &mut Self| -> Result<usize, String> {
            need(sha(&std::fs::read(h.bank_file()).map_err(|e| e.to_string())?) == h.bank_bytes_hash
                && digest(&identity(&h.workspace_dir)?) == digest(&h.code_identity), "IDENTITY_CHANGED")?;
            h.verify_journal()?;
            let mut batch_book = Vec::new();
            let mut batch_price = Vec::new();
            let mut next_offset = (h.offset_book, h.offset_price);
            let mut next_hashes = (h.prefix_hash_book.clone(), h.prefix_hash_price.clone());
            // Verify both original consumed prefixes before parsing or applying
            // either source. Equal size is not evidence that bytes are unchanged.
            for kind in ["book", "price"] {
                let file = h.source_file(kind);
                let stat = stat_of(&file)?;
                let (ino, btime, _size) = if kind == "book" { h.source_stat_book } else { h.source_stat_price };
                let offset = if kind == "book" { h.offset_book } else { h.offset_price };
                let prefix_hash = if kind == "book" { &h.prefix_hash_book } else { &h.prefix_hash_price };
                need(stat.0 == ino && stat.1 == btime && stat.2 >= offset as u64, "SOURCE_REPLACED")?;
                let bytes = complete(&file)?;
                need(bytes.len() >= offset && sha(&bytes[..offset]) == *prefix_hash, "SOURCE_CHANGED")?;
                let parsed = rows(&bytes[offset..])?;
                if kind == "book" { batch_book = parsed; next_offset.0 = bytes.len(); next_hashes.0 = sha(&bytes); }
                else { batch_price = parsed; next_offset.1 = bytes.len(); next_hashes.1 = sha(&bytes); }
            }
            let count = batch_book.len() + batch_price.len();
            if count == 0 { return Ok(0); }
            let at = baal_service_runtime::contract::ms_to_iso((h.now)());
            h.reduce(&merge(&batch_book, &batch_price), &at)?;
            h.offset_book = next_offset.0;
            h.offset_price = next_offset.1;
            h.prefix_hash_book = next_hashes.0;
            h.prefix_hash_price = next_hashes.1;
            let mut body = json!({"kind": "batch", "availableAt": at});
            for (k, v) in h.expected().as_object().unwrap() { body[k] = v.clone(); }
            h.append(body)?;
            Ok(count)
        })(self);
        if r.is_err() { self.failed = true; }
        r
    }
    /// status() — bind every original pending-map key directly to its scoped
    /// instrument. An expired intent can reprepare without a fresh signal.
    pub fn status(&self) -> Result<V, String> {
        let mut report = crate::bulk_dip::bulk_dip_report(&self.state, &self.bank,
            &baal_service_runtime::contract::ms_to_iso((self.now)()))?;
        let mut wallets = serde_json::Map::new();
        for (latency, wallet) in report["wallets"].as_object().cloned().unwrap_or_default() {
            let mut w = wallet.clone();
            let mut pending: Vec<V> = Vec::new();
            for (instrument, mut intent) in self.state["wallets"][&latency]["state"]["strategies"]["mean_reversion"]["pending"]
                .as_object().cloned().unwrap_or_default().into_iter() {
                let spec = self.universe["exchanges"].as_array().unwrap_or(&vec![]).iter()
                    .find(|x| x["exchangeId"].as_str().map(|e| format!("{}:YES", e)) == Some(instrument.clone()))
                    .cloned();
                need(spec.is_some(), "PENDING_IDENTITY")?;
                intent["instrument"] = json!(instrument);
                intent["marketId"] = spec.unwrap()["marketId"].clone();
                pending.push(intent);
            }
            w["pending"] = json!(pending);
            wallets.insert(latency, w);
        }
        report["wallets"] = V::Object(wallets);
        report["hostVersion"] = json!(2);
        report["referenceBarriers"] = self.state["referenceBarriers"].clone();
        report["cohort"] = json!(self.cohort);
        for (k, v) in self.expected().as_object().unwrap() { report[k] = v.clone(); }
        report["journalSequence"] = json!(self.sequence);
        report["journalHash"] = json!(self.previous);
        report["sourceClosed"] = self.book_state["closed"].clone();
        report["failed"] = json!(self.failed);
        Ok(report)
    }
    pub fn close(&mut self) -> Result<(), String> {
        if self.closed { return Ok(()); }
        self.closed = true;
        need(std::fs::read_to_string(&self.lock_file).map(|s| s == self.token).unwrap_or(false),
            "BULK_HOST_V2_LOCK_CHANGED")?;
        std::fs::remove_file(&self.lock_file).map_err(|e| e.to_string())
    }
}
/// openBulkDip({runtimeDir, cohort, initialize, now, workspaceDir}) — v2.
pub fn open_bulk_dip_v2(runtime_dir: &Path, cohort: &str, initialize: bool,
    now: impl Fn() -> f64 + 'static, workspace_dir: &Path) -> Result<BulkDipHostV2, String> {
    let cohort_ok = cohort.len() >= 2 && cohort.len() <= 64
        && cohort.as_bytes()[0].is_ascii_lowercase()
        && cohort.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    need(runtime_dir.is_absolute() && cohort_ok, "DIRECTORY")?;
    let dir = runtime_dir.join("bulk-dip-cohorts").join(cohort);
    let bank_file = dir.join("bank.json");
    let journal_file = dir.join("journal.jsonl");
    let lock_file = dir.join("worker.lock");
    let source_book = runtime_dir.join("public-books.jsonl");
    let source_price = runtime_dir.join("market-prices.jsonl");
    let stopped = |workspace_dir: &Path| -> bool {
        [workspace_dir.join("STOP"), runtime_dir.join("STOP"),
            runtime_dir.join("BULK_DIP_STOP"), dir.join("STOP")].iter().any(|p| p.exists())
    };
    need(!stopped(workspace_dir), "STOPPED")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let token = format!("{}", json!({"pid": std::process::id(), "id": uuid_like()}));
    let r = (|| -> Result<BulkDipHostV2, String> {
        write(&lock_file, token.as_bytes(), false, true)?;
        let code_identity = identity(workspace_dir)?;
        let source_stat_book = stat_of(&source_book)?;
        let source_stat_price = stat_of(&source_price)?;
        let raw_book = complete(&source_book)?;
        let raw_price = complete(&source_price)?;
        let all_book = rows(&raw_book)?;
        let all_price = rows(&raw_price)?;
        need(!all_book.is_empty() && !all_price.is_empty(), "EMPTY")?;
        let bank: V;
        if initialize {
            need(!bank_file.exists() && !journal_file.exists(), "EXISTS")?;
            let cutoff = baal_service_runtime::contract::ms_to_iso(now());
            let template = read_json(&workspace_dir.join("live-paper/config.json"))?;
            let universe = read_json(&workspace_dir.join("live-paper/price-universe.json"))?;
            let cutoff_ms = parse_at(&json!(cutoff));
            need([all_book.last().unwrap(), all_price.last().unwrap()].iter().all(|a|
                parse_at(&a["at"]) <= cutoff_ms), "FUTURE")?;
            let mut config = template.clone();
            config["strategyUniverse"] = json!("diversified-dip-v1");
            config["marketIds"] = json!(universe["exchanges"].as_array().cloned().unwrap_or_default()
                .iter().map(|x| x["marketId"].clone()).collect::<Vec<V>>());
            config["initialCash"] = json!("100.000000");
            config["cashReserve"] = json!("90.000000");
            config["maxExposure"] = json!("10.000000");
            config["feeBps"] = json!(0);
            config["slippageTicks"] = json!(1);
            config["depthFractionBps"] = json!(100);
            config["orderShares"] = json!(1);
            config["maxPositionShares"] = json!(1);
            config["stopLoss"] = json!("0.020000");
            config["exitAfterMs"] = json!(300000);
            bank = json!({"schemaVersion":2,"hostVersion":2,"mode":"market_wide_bulk_reference_paper",
                "name":cohort,"cutoff":cutoff,"universe":universe,
                "policy":crate::bid_anchor::bid_anchor_policy(),
                "bulkPolicy":crate::bulk_dip::bulk_dip_policy(),"config":config,
                "prefix":{"book":{"bytes":raw_book.len() as i64,"hash":sha(&raw_book),
                        "sequence":all_book.last().unwrap()["sequence"],
                        "journalHash":all_book.last().unwrap()["hash"]},
                    "price":{"bytes":raw_price.len() as i64,"hash":sha(&raw_price),
                        "sequence":all_price.last().unwrap()["sequence"],
                        "journalHash":all_price.last().unwrap()["hash"]}},
                "codeIdentity":code_identity});
        } else {
            bank = read_json(&bank_file)?;
            need(bank["schemaVersion"].as_i64() == Some(2) && bank["hostVersion"].as_i64() == Some(2)
                && bank["name"].as_str() == Some(cohort), "BANK_VERSION")?;
            need(digest(&bank["codeIdentity"]) == digest(&code_identity), "FROZEN_CODE")?;
        }
        let universe = crate::breadth::validate_universe(&bank["universe"],
            bank["config"]["tournamentId"].as_str().unwrap_or(""))?;
        let market_by_exchange: std::collections::BTreeMap<String, String> =
            universe["exchanges"].as_array().cloned().unwrap_or_default().iter()
                .filter_map(|x| Some((x["exchangeId"].as_str()?.to_string(),
                    x["marketId"].as_str()?.to_string()))).collect();
        let mut host = BulkDipHostV2 {
            dir: dir.clone(), bank: bank.clone(),
            state: crate::bulk_dip::create_bulk_dip_state(&bank)?,
            runtime_dir: runtime_dir.to_path_buf(), workspace_dir: workspace_dir.to_path_buf(),
            cohort: cohort.to_string(), code_identity,
            book_state: crate::book_publication_v2::create_book_publication_state(&bank["universe"])?,
            price_state: json!({"sequence": 0, "hash": "", "at": V::Null}),
            universe, market_by_exchange,
            sequence: 0, previous: digest(&bank), journal_bytes: 0, journal_bytes_hash: V::Null,
            offset_book: 0, offset_price: 0, prefix_hash_book: String::new(),
            prefix_hash_price: String::new(), bank_bytes_hash: String::new(),
            source_stat_book, source_stat_price, lock_file: lock_file.clone(),
            token: token.clone(), closed: false, failed: false, now: Box::new(now),
        };
        let mut index_book = 0usize;
        let mut index_price = 0usize;
        for kind in ["book", "price"] {
            let target = bank["prefix"][kind]["sequence"].as_i64().unwrap_or(0);
            while (if kind == "book" { host.book_state["sequence"].as_i64().unwrap_or(0) }
                else { host.price_state["sequence"].as_i64().unwrap_or(0) }) < target {
                host.live()?;
                let (all, raw, index) = if kind == "book" {
                    (&all_book, &raw_book, &mut index_book)
                } else {
                    (&all_price, &raw_price, &mut index_price)
                };
                need(*index < all.len(), "PREFIX_TRUNCATED")?;
                let row = all[*index].clone();
                *index += 1;
                host.validate(kind, &row)?;
                let off = if kind == "book" { host.offset_book } else { host.offset_price };
                let next = raw[off..].iter().position(|x| *x == b'\n').map(|p| p + off + 1).unwrap_or(off);
                if kind == "book" { host.offset_book = next; } else { host.offset_price = next; }
            }
            let (offset, seq_hash) = if kind == "book" {
                (host.offset_book, host.book_state["hash"].as_str().unwrap_or("").to_string())
            } else {
                (host.offset_price, host.price_state["hash"].as_str().unwrap_or("").to_string())
            };
            let raw = if kind == "book" { &raw_book } else { &raw_price };
            need(offset == bank["prefix"][kind]["bytes"].as_i64().unwrap_or(-1) as usize
                && sha(&raw[..offset]) == bank["prefix"][kind]["hash"].as_str().unwrap_or("")
                && seq_hash == bank["prefix"][kind]["journalHash"].as_str().unwrap_or(""), "PREFIX")?;
        }
        // Include both source and consumer boundaries.
        let global = host.book_state["invalidations"]["global"].clone();
        host.state["referenceBarriers"] = json!({
            "global": if global.is_null() { V::Null } else { global["at"].clone() },
            "markets": host.book_state["invalidations"]["markets"].as_object().cloned()
                .unwrap_or_default().iter()
                .map(|(id, b)| (id.clone(), b["at"].clone())).collect::<serde_json::Map<String, V>>()});
        let start = parse_at(&bank["cutoff"]) - bank["policy"]["historyMs"].as_i64().unwrap_or(0) as f64;
        if host.book_state["closed"] != V::Bool(true) {
            for row in all_price.iter().take(index_price) {
                let snapshot = &row["event"]["snapshot"];
                let recv = parse_at(&snapshot["receivedAt"]);
                if recv >= start && recv <= parse_at(&bank["cutoff"]) {
                    if let Some(filtered) = host.filtered_prices(snapshot) {
                        crate::bulk_dip::advance_bulk_prices(&mut host.state, &filtered, &bank,
                            bank["cutoff"].as_str().unwrap_or(""), true)?;
                    }
                }
            }
        }
        host.state["live"] = json!({"lastAvailableAt": bank["cutoff"],
            "barriers": host.book_state["invalidations"]["markets"].as_object().cloned()
                .unwrap_or_default().keys()
                .map(|id| (id.clone(), bank["cutoff"].clone())).collect::<serde_json::Map<String, V>>(),
            "counts": {}});
        let mut sequence = 0i64;
        let mut previous = digest(&bank);
        if !initialize {
            let raw = std::fs::read(&journal_file).map_err(|e| e.to_string())?;
            need(!raw.is_empty() && *raw.last().unwrap() == b'\n', "PARTIAL_JOURNAL")?;
            for record in rows(&raw)? {
                let mut body = record.clone();
                let hash = body.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
                sequence += 1;
                need(body["sequence"].as_i64() == Some(sequence)
                    && body["previous"].as_str() == Some(previous.as_str())
                    && body["bankHash"].as_str() == Some(digest(&bank).as_str())
                    && hash.as_str() == Some(digest(&body).as_str()), "JOURNAL")?;
                if body["kind"].as_str() == Some("genesis") {
                    need(sequence == 1 && body["stateHash"].as_str() == Some(digest(&host.state).as_str()),
                        "GENESIS")?;
                } else {
                    need(body["kind"].as_str() == Some("batch"), "CHECKPOINT")?;
                    let mut batch_book: Vec<V> = Vec::new();
                    let mut batch_price: Vec<V> = Vec::new();
                    for kind in ["book", "price"] {
                        let target = body[&format!("{}Sequence", kind)].as_i64().unwrap_or(0);
                        need(body[&format!("{}Sequence", kind)].as_i64().is_some(), "SEQUENCE")?;
                        let current = if kind == "book" { host.book_state["sequence"].as_i64().unwrap_or(0) }
                            else { host.price_state["sequence"].as_i64().unwrap_or(0) };
                        need(target >= current, "SEQUENCE")?;
                        let (all, raw, index) = if kind == "book" {
                            (&all_book, &raw_book, &mut index_book)
                        } else {
                            (&all_price, &raw_price, &mut index_price)
                        };
                        while current + (if kind == "book" { batch_book.len() } else { batch_price.len() } as i64)
                            < target {
                            need(*index < all.len(), "SOURCE_TRUNCATED")?;
                            if kind == "book" { batch_book.push(all[*index].clone()); }
                            else { batch_price.push(all[*index].clone()); }
                            *index += 1;
                            let off = if kind == "book" { host.offset_book } else { host.offset_price };
                            let next = raw[off..].iter().position(|x| *x == b'\n').map(|p| p + off + 1).unwrap_or(off);
                            if kind == "book" { host.offset_book = next; } else { host.offset_price = next; }
                        }
                    }
                    need(!batch_book.is_empty() || !batch_price.is_empty(), "EMPTY_BATCH")?;
                    host.reduce(&merge(&batch_book, &batch_price), body["availableAt"].as_str().unwrap_or(""))?;
                    need(host.expected().as_object().unwrap().iter()
                        .all(|(k, v)| &body[k] == v), "REPLAY")?;
                }
                previous = hash.as_str().unwrap_or("").to_string();
            }
            need(sequence > 0, "GENESIS_MISSING")?;
            host.journal_bytes = raw.len() as u64;
            host.journal_bytes_hash = json!(sha(&raw));
        }
        host.sequence = sequence;
        host.previous = previous;
        if initialize {
            write(&bank_file, format!("{}\n", serde_json::to_string_pretty(&bank).map_err(|e| e.to_string())?).as_bytes(), false, true)?;
            write(&journal_file, &[], false, true)?;
            host.journal_bytes_hash = json!(sha(&[]));
            host.append(json!({"kind": "genesis", "stateHash": digest(&host.state)}))?;
        }
        host.bank_bytes_hash = sha(&std::fs::read(&bank_file).map_err(|e| e.to_string())?);
        host.prefix_hash_book = sha(&raw_book[..host.offset_book]);
        host.prefix_hash_price = sha(&raw_price[..host.offset_price]);
        Ok(host)
    })();
    if r.is_err() {
        if std::fs::read_to_string(&lock_file).map(|s| s == token).unwrap_or(false) {
            let _ = std::fs::remove_file(&lock_file);
        }
    }
    r
}
