// data/prepare-canonical-capacity-20261003.mjs — fetch public sources, declare
// the NH canonical capacity review, and freeze the cohort bank.
use baal_collector::canonical_capacity_monitor::capacity_code_identity;
use baal_collector::canonical_no_capacity::{canonical_no_policy, validate_capacity_review};
use baal_service_runtime::json_text::{digest, sha256};
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
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
fn parse_iso_ms(s: &str) -> Option<f64> {
    baal_service_runtime::contract::parse_iso_ms(s)
}
fn write_wx(p: &Path, b: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600); }
    let mut f = o.open(p).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, b).map_err(|e| e.to_string())
}
fn mkdir_700(p: &Path) -> Result<(), String> {
    #[cfg(unix)] {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(p)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(unix))] { std::fs::create_dir_all(p).map_err(|e| e.to_string()) }
}

pub fn run() -> Result<V, String> {
    let rt = localappdata()?.join("SigTradingCup/paper-runtime");
    let cohort = "nh-canonical-capacity-20261003-v1";
    let dir = rt.join("canonical-capacity-cohorts").join(cohort);
    if dir.join("bank.json").exists()
        || Path::new("data/canonical-capacity-bank-20261003-v1.json").exists() {
        return Err("Existing frozen cohort must not be overwritten".into());
    }
    let input = read_json(Path::new("data/canonical-capacity-research-input.json"))?;
    let evidence: Vec<V> = input.as_array().cloned().unwrap_or_default().iter()
        .map(|s| json!({"commentId": s["id"], "url": s["url"],
            "contentHash": sha(s["body"].as_str().unwrap_or("").as_bytes()),
            "provenance": "Daenerys manual signed-in browser transcript summary, not authenticated API metadata"}))
        .collect();
    let mut captures = Vec::new();
    let sources = [("platform-complement.html",
            "https://sig.thesuper.market/docs/markets-and-trading"),
        ("NH-sample-ballot.pdf",
            "https://www.londonderrynh.gov/DocumentCenter/View/3063")];
    mkdir_700(&dir)?;
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::default())
        .build().map_err(|e| e.to_string())?;
    for (file, url) in sources {
        let mut response = client.get(url).send().map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err("Public source unavailable".into());
        }
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut response, &mut bytes).map_err(|e| e.to_string())?;
        write_wx(&dir.join(file), &bytes)?;
        captures.push(json!({"url": url, "file": file, "contentHash": sha(&bytes),
            "bytes": bytes.len(), "observedAt": now_iso()}));
    }
    let recorded_at = now_iso();
    let bindings: Vec<V> = [["381", "1070", "Democratic Party"],
            ["382", "1071", "Republican Party"]].iter()
        .map(|[market_id, exchange_id, winner_affiliation]| json!({
            "marketId": market_id, "exchangeId": exchange_id, "outcome": "NO",
            "event": "2026 New Hampshire U.S. Senate general election",
            "resolution": "final certified winner party affiliation",
            "winnerAffiliation": winner_affiliation,
            "fusion": "all parties listed for the winner count",
            "administrative": "sponsor may change rules or outcome; delays/void/refund not excluded",
            "manualInfoAt": "2026-10-03T22:11:30Z through 2026-10-03T22:11:43Z",
            "provenance": evidence[0]["url"]}))
        .collect();
    let review = json!({"schemaVersion": 1, "id": cohort,
        "kind": "conditional_no_pair_review",
        "tournamentId": "bda92870-621e-47b0-bc3c-3602c5c26f55",
        "recordedAt": recorded_at,
        "expiresAt": baal_service_runtime::contract::ms_to_iso(
            parse_iso_ms(&recorded_at).unwrap_or(f64::NAN) + 21600000.0),
        "evidenceHash": digest(&json!({"evidence": evidence,
            "captures": captures, "bindings": bindings})),
        "legs": bindings.iter().map(|b| json!({"marketId": b["marketId"],
            "exchangeId": b["exchangeId"], "outcome": "NO",
            "rulesHash": digest(b), "binding": b})).collect::<Vec<_>>(),
        "assumptions": [
            "Current Senate ballot maps Chris Pappas Democratic, John E. Sununu Republican, Edmond Laplante Constitution, plus write-in; ordinary single winner affiliations are exclusive.",
            "Same ballot demonstrates dual-party nomination for other offices, so fusion is a real contrary possibility and zero-payout stress is required.",
            "Exact rules are peer-supplied manual transcript evidence. Rules hashes bind that attestation, not live API authenticity or scenario completeness.",
            "Uncovered SELL YES is documented BUY NO at complement; YES bid rungs provide the canonical same-exchange NO acquisition depth, not independent native-NO observations.",
            "Fees are set to zero under the direct human instruction. Each acquired leg still includes one adverse 0.005 tick; displayed depth is discounted to one percent.",
            "Normal settlement surplus is not a forecast of a profitable resale or guaranteed completion of two legs."],
        "scenarios": [
            {"id": "Democratic_winner", "category": "ordinary", "payouts": [0, 1]},
            {"id": "Republican_winner", "category": "ordinary", "payouts": [1, 0]},
            {"id": "other_winner", "category": "ordinary", "payouts": [1, 1]},
            {"id": "dual_party_or_admin_zero_stress", "category": "exception",
                "payouts": [0, 0]}],
        "evidence": evidence, "captures": captures});
    validate_capacity_review(&review)?;
    let bytes = std::fs::read(rt.join("public-books.jsonl")).map_err(|e| e.to_string())?;
    let prefix_end = bytes.iter().rposition(|c| *c == b'\n').map(|i| i + 1).unwrap_or(0);
    let prefix = &bytes[..prefix_end];
    let text = String::from_utf8_lossy(prefix);
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.retain(|l| !l.is_empty());
    let last: V = serde_json::from_str(lines.last().copied().unwrap_or(""))
        .map_err(|e| e.to_string())?;
    let cutoff = now_iso();
    if parse_iso_ms(last["at"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
        > parse_iso_ms(&cutoff).unwrap_or(f64::NAN) {
        return Err("Future source".into());
    }
    let bank = json!({"schemaVersion": 1, "name": cohort,
        "tournamentId": review["tournamentId"], "cutoff": cutoff,
        "review": review, "policy": canonical_no_policy(),
        "codeIdentity": capacity_code_identity(Path::new("live-paper"))?,
        "sourcePrefixBytes": prefix.len(), "sourcePrefixHash": sha(prefix),
        "sourcePrefixSequence": last["sequence"],
        "sourcePrefixJournalHash": last["hash"]});
    for file in [dir.join("bank.json"),
            PathBuf::from("data/canonical-capacity-bank-20261003-v1.json")] {
        write_wx(&file, format!("{}\n", serde_json::to_string_pretty(&bank)
            .map_err(|e| e.to_string())?).as_bytes())?;
    }
    write_wx(&dir.join("journal.jsonl"), b"")?;
    // JS: compact JSON.stringify, no trailing newline.
    write_wx(&rt.join("canonical-capacity-active.json"),
        serde_json::to_string(&json!({"cohort": cohort,
            "bankHash": digest(&bank)})).map_err(|e| e.to_string())?.as_bytes())?;
    Ok(json!({"cohort": cohort, "bankHash": digest(&bank), "cutoff": cutoff,
        "sourcePrefixSequence": last["sequence"],
        "reviewExpiresAt": review["expiresAt"], "newApiClients": 0,
        "competitionOrders": 0}))
}
