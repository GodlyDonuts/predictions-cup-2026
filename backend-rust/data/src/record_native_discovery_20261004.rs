// data/record-native-discovery-20261004.mjs — native discovery milestone record.
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical as atomic_json;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::{json, Value as V};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

fn read(f: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn now_iso() -> String {
    ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0))
}
fn fail(msg: &str) -> String { msg.to_string() }
fn eq(a: &V, b: &V) -> Result<(), String> {
    if a == b { Ok(()) } else { Err(fail(&format!("assert.equal: {} !== {}", js_stringify(a), js_stringify(b)))) }
}
fn a_true(ok: bool) -> Result<(), String> {
    if ok { Ok(()) } else { Err(fail("assertion failed")) }
}
fn pnull(x: &V, key: &str) -> Result<(), String> {
    if x.as_object().is_some_and(|m| m.get(key).is_some_and(|v| v.is_null())) { Ok(()) } else { Err(fail("assert.equal: not null")) }
}
fn kill0(pid: &V) -> Result<(), String> {
    let p = pid.as_i64().unwrap_or(-1);
    let ok = std::process::Command::new("/bin/kill").arg("-0").arg(p.to_string()).status()
        .map(|s| s.success()).unwrap_or(false);
    if ok { Ok(()) } else { Err(format!("kill ESRCH {}", p)) }
}
fn re_word(text: &str, pat: &str) -> bool {
    match text.find(pat) {
        Some(i) => {
            let next = text.as_bytes().get(i + pat.len());
            next.is_none_or(|c| !c.is_ascii_alphanumeric() && *c != b'_')
        }
        None => false,
    }
}
fn rt() -> PathBuf {
    Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("SigTradingCup").join("paper-runtime")
}
fn arr_len(x: &V) -> i64 { x.as_array().map(|a| a.len() as i64).unwrap_or(0) }

pub fn run(root: &Path) -> Result<V, String> {
    let r = rt();
    let pids = read(&root.join("data/current-runtime-pids.json"))?;
    let s = read(&r.join("stats.json"))?;
    let screen = read(&r.join("collector-market-screen-v1.json"))?;
    let n = read(&r.join("native-dip-cache-status.json"))?;
    let budget = read(&r.join("coordinated-network-budget.json"))?;
    let benchmark = read(&root.join("data/native-discovery-benchmark-20261004.json"))?;
    let preserved = read(&root.join("data/native-discovery-preservation-after-20261004.json"))?;
    let scheduler = read(&r.join("book-scheduler.json"))?;
    for pid in ["collector", "paper", "dashboard"] { kill0(&pids[pid])?; }
    eq(&s["runtime"]["pid"], &pids["collector"])?;
    eq(&s["runtime"]["status"], &json!("running"))?;
    a_true(s["runtime"]["cycles"].as_i64().unwrap_or(0) >= 2)?;
    a_true(arr_len(&s["runtime"]["errors"]) == 0 && s["runtime"]["errors"].is_array())?;
    eq(&screen["available"], &json!(true))?;
    eq(&screen["status"], &json!("running"))?;
    eq(&screen["counts"]["observed"], &json!(237))?;
    eq(&screen["executionAuthority"], &json!(false))?;
    eq(&json!(arr_len(&s["runtime"]["scheduling"]["reads"])), &json!(4))?;
    let reads = s["runtime"]["scheduling"]["reads"].as_array().cloned().unwrap_or_default();
    for market in ["381", "382", "297"] {
        a_true(reads.iter().any(|x| x["marketId"].as_str() == Some(market) && x["reason"].as_str() == Some("held_or_pending")))?;
    }
    a_true(reads.iter().any(|x| x["nativeDiscovery"].as_bool() == Some(true) && x["reason"].as_str() == Some("discovery")))?;
    a_true(scheduler["nativeDiscoveryAttempts"].as_i64().unwrap_or(0) >= 2)?;
    pnull(&n, "lastError")?;
    a_true(n["verifiedAt"].as_str().is_some())?;
    eq(&n["fees"], &json!(0))?;
    eq(&n["competitionOrders"], &json!(0))?;
    eq(&budget["maxReadsPerMinute"], &json!(20))?;
    eq(&budget["writesEnabled"], &json!(false))?;
    eq(&budget["executionOwner"], &json!("Daenerys"))?;
    eq(&read(&r.join("competition-activation.json"))?["enabled"], &json!(false))?;
    eq(&preserved["frozenBanksPreserved"], &json!(true))?;
    for (f, expected) in benchmark["codeHashes"].as_object().cloned().unwrap_or_default() {
        let hash = sha256(&std::fs::read(root.join(&f)).or_else(|_| std::fs::read(&f)).map_err(|e| e.to_string())?);
        eq(&json!(hash), &expected)?;
    }
    let checks = std::fs::read_to_string(root.join("data/native-discovery-all-checks-20261004.tap")).map_err(|e| e.to_string())?;
    a_true(re_word(&checks, "# pass 111") && re_word(&checks, "# fail 0") && re_word(&checks, "# skipped 0"))?;
    let books = r.join("public-books.jsonl");
    let size = std::fs::metadata(&books).map_err(|e| e.to_string())?.len();
    let amount = std::cmp::min(size, 262144) as usize;
    let mut b = vec![0u8; amount];
    let mut fd = std::fs::File::open(&books).map_err(|e| e.to_string())?;
    let got = { fd.seek(std::io::SeekFrom::Start(size - amount as u64)).map_err(|e| e.to_string())?; fd.read(&mut b).map_err(|e| e.to_string())? };
    drop(fd);
    eq(&json!(got), &json!(amount))?;
    eq(&json!(b.last().copied().unwrap_or(0)), &json!(10))?;
    let text = String::from_utf8_lossy(&b);
    let parts: Vec<&str> = text.split('\n').collect();
    // JS .slice(1,-1): drop the leading partial line and the trailing empty entry.
    let rows: Vec<V> = if parts.len() >= 2 {
        parts[1..parts.len() - 1].iter()
            .map(|l| serde_json::from_str(l).map_err(|e| e.to_string())).collect::<Result<_, _>>()?
    } else { Vec::new() };
    let discovery_read = reads.iter().find(|x| x["nativeDiscovery"].as_bool() == Some(true)).cloned().unwrap_or(V::Null);
    let receipt = rows.iter().rev().find(|x| x["kind"].as_str() == Some("book")
        && x["payload"]["book"]["marketId"] == discovery_read["marketId"]).cloned();
    a_true(receipt.is_some())?;
    let receipt = receipt.unwrap();
    let mut body = receipt.clone();
    let hash = body.as_object_mut().and_then(|m| m.shift_remove("hash")).unwrap_or(V::Null);
    eq(&json!(payload_hash(&body)?), &hash)?;
    let book = &receipt["payload"]["book"];
    eq(&book["freshnessPolicy"], &json!("authoritative_receipt_v1"))?;
    a_true(parse_iso_ms(book["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
        - parse_iso_ms(book["requestStartedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) <= 20000.0)?;
    a_true(parse_iso_ms(receipt["at"].as_str().unwrap_or("")).unwrap_or(f64::NAN)
        - parse_iso_ms(book["receivedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN) <= 20000.0)?;
    let e = json!({"schemaVersion": 1, "at": now_iso(), "claim": 5981528650i64,
        "status": "verified_global_discovery_deployed",
        "source": screen["source"], "sourceObservations": screen["sourceObservations"],
        "screen": {"counts": screen["counts"],
            "screenedCandidates": arr_len(&screen["candidateMarketIds"]),
            "bootstrapMs": screen["bootstrapMs"], "executionAuthority": false},
        "benchmark": {"wallMs": benchmark["wallMs"], "baselineWallMs": benchmark["baselineWallMs"],
            "speedup": benchmark["speedup"], "samePrefixCountsAndLeadsExact": benchmark["samePrefixCountsAndLeadsExact"],
            "source": benchmark["source"]},
        "scheduling": {"reads": reads.iter().map(|x| json!({"marketId": x["marketId"], "reason": x["reason"],
                "nativeDiscovery": x["nativeDiscovery"].as_bool() == Some(true)})).collect::<Vec<V>>(),
            "discovery": s["runtime"]["scheduling"]["nativeDiscovery"],
            "attempts": scheduler["nativeDiscoveryAttempts"],
            "entryMarketIds": s["runtime"]["scheduling"]["entryMarketIds"]},
        "actualDiscoveryReceipt": {"sequence": receipt["sequence"], "hash": hash, "marketId": book["marketId"],
            "requestStartedAt": book["requestStartedAt"], "receivedAt": book["receivedAt"],
            "publishedAt": receipt["at"], "freshnessPolicy": book["freshnessPolicy"]},
        "native": {"verifiedAt": n["verifiedAt"], "lastWallMs": n["lastWallMs"], "sourceBytes": n["sourceBytes"],
            "uniqueFillEvents": n["uniqueFillEvents"], "fees": 0},
        "checks": {"passed": 111, "failed": 0, "skipped": 0},
        "services": {"collector": pids["collector"], "paper": pids["paper"], "dashboard": pids["dashboard"],
            "persistent": 3, "extra": 0, "status": s["runtime"]["status"], "cycles": s["runtime"]["cycles"]},
        "readAllocation": 20, "executionOwner": "Daenerys", "competitionOrders": 0, "contestActivation": false,
        "frozenBanksPreserved": true, "originalJournalsPreserved": true, "walletsReset": false,
        "parametersTuned": false, "pairedReviewRenewed": false, "profitabilityEstablished": false,
        "limitations": ["Quote discounts are screening evidence only, not executable fills or profitable forecasts",
            "Only one discovery book remains while three held-risk obligations are active",
            "Historical passive buy-hold comparison marks can be stale without fixed-core reservations",
            "Frozen native per-instrument pending-intent observability remains follow-up work"]});
    atomic_json(&root.join("data/native-discovery-milestone-20261004.json"), &e)?;
    let mut q = read(&root.join("data/quant-development-state.json"))?;
    q["updatedAt"] = e["at"].clone();
    q["nativeDiscoveryMilestone"] = e.clone();
    q["activeProcesses"] = json!({"collector": pids["collector"], "probabilityWorker": pids["paper"],
        "dashboard": pids["dashboard"], "persistent": 3, "extra": 0});
    q["goalTurn"] = json!({"at": e["at"], "classification": "progress", "blockedAuditCount": 0,
        "materialChange": "Verified collector-local 237-market minute screen deployed with fixed-core comparison removed in native mode. Held obligations381/382/297 protected before screening; live discovery obtains fresh public depth under the same four-book/20-read budgets. Same-source bootstrap2.48s versus34.49s; exact oracle parity and111checks. No wallet reset or contest writes."});
    q["next"] = json!(["Expose independently verifiable native per-instrument pending intents in a versioned output so fresh confirmation reads can occur before expiry; preserve all original frozen wallet outputs/fills.",
        "Measure subsequent screened depth and realized spread/slippage gates; do not equate historical discount to a forecast or copied configurations to independent trades.",
        "Maintain cached Rust continuation, collector-local screen and existing frozen banks with no uncoordinated API clients.",
        "NH paired review expired15:36:07.104UTC; keep held monitoring and require genuinely new source-backed review for any successor entry cohort."]);
    atomic_json(&root.join("data/quant-development-state.json"), &q)?;
    let out = json!({"status": e["status"], "screen": e["screen"], "benchmark": e["benchmark"],
        "scheduling": e["scheduling"], "actualDiscoveryReceipt": e["actualDiscoveryReceipt"],
        "checks": e["checks"], "services": e["services"], "competitionOrders": 0});
    println!("{}", serde_json::to_string(&out).unwrap_or_default());
    Ok(e)
}
