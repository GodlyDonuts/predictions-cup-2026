// data/measure-portfolio-fill-markouts-20261006.mjs — later-observed depth
// markouts for each distinct purchase in the sealed portfolio journal.
use baal_collector::book_publication_v2::{apply_book_publication, create_book_publication_state};
use baal_collector::settlement_portfolio_paper_v2::assert_settlement_portfolio_state;
use baal_service_runtime::journal::{journal_lines, JournalReadOptions};
use baal_service_runtime::json_text::{digest, js_stringify, sha256};
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn sealed_end(path: &Path) -> Result<u64, String> {
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    let mut fd = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let back = std::cmp::min(len, 1024 * 1024);
    fd.seek(SeekFrom::Start(len - back)).map_err(|e| e.to_string())?;
    let mut tail = vec![0u8; back as usize];
    fd.read_exact(&mut tail).map_err(|e| e.to_string())?;
    match tail.iter().rposition(|c| *c == b'\n') {
        Some(i) => Ok(len - back + i as u64 + 1),
        None => Err("DUAL_TIMELAPSE_EMPTY".into()),
    }
}
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn perf_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0
}

pub fn run() -> Result<V, String> {
    let rt = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let cohort = "kansas-horizon-portfolio-20261005-v2";
    let dir = rt.join("settlement-portfolio-v2-cohorts").join(cohort);
    let bank = read_json(&dir.join("bank.json"))?;
    let wallet_file = dir.join("journal.jsonl");
    let wallet_end = sealed_end(&wallet_file)?;
    let mut wallet_hash = Sha256::new();
    let mut seq = 0i64;
    let mut prev = String::new();
    let mut last = V::Null;
    for line in journal_lines(&wallet_file,
            &JournalReadOptions { end_byte: Some(wallet_end), ..Default::default() })? {
        wallet_hash.update(line.as_bytes());
        wallet_hash.update(b"\n");
        let r: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let mut body = r.clone();
        let h = r["hash"].as_str().unwrap_or("").to_string();
        if let V::Object(m) = &mut body { m.shift_remove("hash"); }
        seq += 1;
        if r["sequence"].as_i64() != Some(seq)
            || r["previous"].as_str().unwrap_or("") != prev
            || sha(js_stringify(&body).as_bytes()) != h {
            return Err("WALLET_CHAIN".into());
        }
        assert_settlement_portfolio_state(&r["state"]["paper"], &bank)?;
        prev = h;
        last = r;
    }
    let mut fills: indexmap::IndexMap<String, (V, Vec<V>)> = indexmap::IndexMap::new();
    if let V::Object(wallets) = &last["state"]["paper"]["wallets"] {
        for (latency, w) in wallets {
            for f in w["fills"].as_array().cloned().unwrap_or_default() {
                if f["action"].as_str() != Some("buy") { continue; }
                let key = format!("{}:{}:{}:{}",
                    f["marketId"].as_str().unwrap_or(""),
                    f["intentId"].as_str().unwrap_or(""),
                    f["at"].as_str().unwrap_or(""),
                    f["price"].as_str().unwrap_or(""));
                fills.entry(key).or_insert_with(|| (f.clone(), Vec::new()))
                    .1.push(json!(latency));
            }
        }
    }
    let source_file = rt.join("public-books.jsonl");
    let end = sealed_end(&source_file)?;
    let mut raw = Sha256::new();
    let started = perf_ms();
    let mut publ = create_book_publication_state(&bank["universe"])?;
    let mut entries: indexmap::IndexMap<i64, V> = indexmap::IndexMap::new();
    let mut later: Vec<V> = Vec::new();
    let market_ids: std::collections::HashSet<String> = fills.values()
        .map(|(f, _)| f["marketId"].as_str().unwrap_or("").to_string()).collect();
    for line in journal_lines(&source_file,
            &JournalReadOptions { end_byte: Some(end), ..Default::default() })? {
        raw.update(line.as_bytes());
        raw.update(b"\n");
        let r: V = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        publ = apply_book_publication(&publ, &r)?;
        if r["kind"].as_str() == Some("book")
                && market_ids.contains(
                    r["payload"]["book"]["marketId"].as_str().unwrap_or("")) {
            later.push(r.clone());
            for (f, _) in fills.values() {
                if r["sequence"].as_i64() == f["sourceSequence"].as_i64() {
                    entries.insert(f["sourceSequence"].as_i64().unwrap_or(0), r.clone());
                }
            }
        }
    }
    let results: Vec<V> = fills.values().map(|(f, latencies)| {
        let entry = entries.get(&f["sourceSequence"].as_i64().unwrap_or(0))
            .cloned().unwrap_or(V::Null);
        let mut m = match baal_collector::fill_markout::fill_markout(
            f, &entry, &later, None, None, publ["lastAt"].as_str().unwrap_or("")) {
            Ok(v) => v,
            Err(e) => return Err::<V, String>(e),
        };
        m["correlatedLatencyCases"] = json!(latencies);
        Ok(m)
    }).collect::<Result<Vec<V>, String>>()?;
    let out = json!({"schemaVersion": 1, "at": now_iso(), "cohort": cohort,
        "bankHash": digest(&bank),
        "wallet": {"prefixBytes": wallet_end,
            "sha256": format!("{:x}", wallet_hash.finalize()),
            "sequence": seq, "tailHash": prev},
        "source": {"prefixBytes": end, "sha256": format!("{:x}", raw.finalize()),
            "sequence": publ["sequence"], "tailHash": publ["hash"],
            "throughAt": publ["lastAt"]},
        "wallMs": perf_ms() - started,
        "distinctPurchaseOpportunities": results.len(), "results": results,
        "fees": "0.000000", "sourceQuotesVerified": true,
        "originalJournalUnchanged": true, "accountRequests": 0,
        "simulatedSales": 0, "realizedProfitFromDiagnostic": V::Null,
        "profitabilityEstablished": false,
        "limitations": ["Markouts are later observed depth-aware quote proceeds, not accepted sale orders.",
            "Source gaps and unknown later liquidity are censored, never forward-filled.",
            "One-tick assumptions are explicit modeling stress, not platform fees.",
            "Latency variants of the same opportunity are deduplicated; a tiny sample does not establish an edge."]});
    std::fs::write("data/portfolio-fill-markouts-20261006.json",
        format!("{}\n", serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())?;
    Ok(json!({"opportunities": out["results"].as_array().map(|a| a.len()).unwrap_or(0),
        "wallMs": out["wallMs"], "results": out["results"]}))
}
