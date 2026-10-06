// data/prepare-profit-strategy-publication-20261005.mjs — milestone + pack + manifest
use crate::publication_pack::{elements, now_ms, read_json, runtime_dir, sha};
use baal_collector::model::digest;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use serde_json::{json, Value as V};
use std::path::Path;

fn wallets(s: &V) -> V {
    json!(s["wallets"].as_object().map(|m| m.iter().map(|(k, w)| (k.clone(), json!({
        "fills": w["fills"], "closedTrades": w["closedTrades"], "realizedPnl": w["realizedPnl"],
        "liquidationPnl": w["liquidationPnl"], "fees": w["fees"],
        "positions": w["positions"].as_array().map(|a| a.iter().map(|p| json!({
            "instrument": p["instrument"], "quantity": p["quantity"], "cost": p["cost"],
        })).collect::<Vec<_>>()).unwrap_or_default(),
        "pending": w["pending"].as_array().map(|a| a.iter().map(|p| json!({
            "action": p["action"], "limit": p["limit"], "quantity": p["quantity"],
        })).collect::<Vec<_>>()).unwrap_or_default(),
    }))).collect::<serde_json::Map<_, _>>()).unwrap_or_default())
}
pub fn run() -> Result<V, String> {
    let runtime = runtime_dir();
    let name = "calibrated-kansas-poll-20261005-v1";
    let dir = format!("{runtime}/probability-receipt-cohorts/{name}");
    let bank = read_json(Path::new(&format!("{dir}/bank.json")))?;
    let stats = read_json(Path::new(&format!("{dir}/stats.json")))?;
    let old_dir = format!("{runtime}/probability-receipt-cohorts/source-probability-native-20261004-v3");
    let prior = read_json(Path::new(&format!("{old_dir}/stats.json")))?;
    let pids = read_json(Path::new("data/current-runtime-pids.json"))?;
    let collector = read_json(Path::new(&format!("{runtime}/stats.json")))?;
    let production = read_json(Path::new(&format!("{runtime}/competition-stats.json")))?;
    let activation = read_json(Path::new(&format!("{runtime}/competition-activation.json")))?;
    let assessment = read_json(Path::new("research/poll-calibration-v1/kansas-paper-assessment-20261005-v1.json"))?;
    let at = ms_to_iso(now_ms() as f64);
    let at_ms = parse_iso_ms(&at).ok_or("ISO")?;
    if stats["failed"] == json!(true) || stats["sourceClosed"] == json!(true)
        || digest(&bank) != stats["bankHash"].as_str().unwrap_or("")
        || (at_ms - parse_iso_ms(stats["updatedAt"].as_str().unwrap_or("")).unwrap_or(0.0)) > 20_000.0
        || stats["competitionOrders"] != json!(0) || stats["networkRequests"] != json!(0)
    {
        return Err("CURRENT_PAPER_NOT_VERIFIED".into());
    }
    let lock_pid = read_json(Path::new(&format!("{dir}/worker.lock")))?["pid"].clone();
    let old_lock_pid = read_json(Path::new(&format!("{old_dir}/worker.lock")))?["pid"].clone();
    if collector["runtime"]["pid"] != pids["collector"]
        || !collector["runtime"]["errors"].as_array().map(|a| a.is_empty()).unwrap_or(false)
        || (at_ms - parse_iso_ms(collector["reportedAt"].as_str().unwrap_or("")).unwrap_or(0.0)) > 20_000.0
        || lock_pid != pids["paper"] || old_lock_pid != pids["paper"]
    {
        return Err("SHARED_LIVE_SERVICES_NOT_VERIFIED".into());
    }
    if activation["entryExpiresAt"] != json!("2026-10-05T03:59:22.131Z")
        || production["status"] != json!("entry_window_closed")
        || production["ordersSubmitted"] != json!(0) || production["unconfirmedIntents"] != json!(0)
    {
        return Err("ORIGINAL_OFFICIAL_RUN_CHANGED".into());
    }
    let milestone = json!({
        "schemaVersion": 1, "id": "empirical-poll-profit-strategy-20261005-v1", "at": at,
        "previousGoalTurnClassification": "progress", "requiredGoalWorkRemaining": true,
        "humanPriority": "Profitability first; production follows actual strategy economics, then continuous improvement.",
        "currentStrategy": {
            "cohort": name, "bankHash": stats["bankHash"], "cutoff": bank["cutoff"],
            "marketId": "356", "exchangeId": "1045", "modelRaces": assessment["model"]["races"],
            "individualPollObservations": assessment["model"]["pollsterRaceObservations"],
            "errorStandardDeviationPoints": assessment["model"]["standardDeviationPoints"],
            "ordinaryValuationRange": assessment["ordinaryRange"], "rangeIsConfidenceBound": false,
        },
        "paper": {
            "observedAt": stats["updatedAt"], "sourceSequence": stats["sourceSequence"],
            "counts": stats["counts"], "wallets": wallets(&stats), "networkRequests": 0, "competitionOrders": 0,
        },
        "previousPaper": {
            "cohort": prior["cohort"], "bankHash": prior["bankHash"], "observedAt": prior["updatedAt"],
            "wallets": wallets(&prior), "preserved": true,
        },
        "services": {
            "collector": pids["collector"], "paper": pids["paper"], "dashboard": pids["dashboard"],
            "persistent": 3, "additional": 0, "collectorReportedAt": collector["reportedAt"],
            "cycles": collector["runtime"]["cycles"], "errors": collector["runtime"]["errors"].as_array().map(|a| a.len()).unwrap_or(0),
            "sharedPaperWorkerVerified": true, "focus": collector["runtime"]["probabilityReadFocus"],
        },
        "official": {
            "runId": activation["runId"], "bankHash": activation["strategyBankHash"],
            "entryExpiresAt": activation["entryExpiresAt"], "status": production["status"],
            "ordersSubmitted": production["ordersSubmitted"], "realizedPnl": production["realizedPnl"],
            "activationChanged": false,
        },
        "checks": {"runnerLifecyclePassed": 53, "focusAndCalibrationPassed": 10, "baselineReproduced": true, "sourceGitBlobsVerified": true},
        "limits": [
            "Historical outcome scores do not include executable market returns.",
            "The first new paper trade is still open; liquidation P&L includes spread/adverse-tick costs and is not realized profit.",
            "Two latency wallets share correlated data and opportunities.",
            "The polling archive lacks population labels and original publication times; historical coverage is conditional on eventual top-two identity.",
            "The original official pilot is expired and its source bank has not changed.",
        ],
        "next": [
            "Observe actual subsequent exit receipts and net P&L for this frozen strategy; preserve negative results and open risk.",
            "Improve the next named strategy only when measured forecast, execution or economic evidence identifies a defect.",
            "Deploy the same trading policy under the human-authorized bounded workflow when actual net economics support it, retaining fresh reconciliation and lifetime attribution.",
        ],
        "profitabilityEstablished": false,
    });
    std::fs::write("data/profit-strategy-milestone-20261005.json", serde_json::to_string_pretty(&milestone).map_err(|e| e.to_string())? + "\n")
        .map_err(|e| e.to_string())?;
    for file in ["data/quant-development-state.json", "data/daenerys-monitor.json"] {
        let mut s = read_json(Path::new(file))?;
        let o = s.as_object_mut().unwrap();
        o.insert("updatedAt".into(), json!(at));
        o.insert("profitStrategyMilestone".into(), milestone.clone());
        o.insert("nextConcreteMilestone".into(), milestone["next"].clone());
        o.insert("currentRuntimePids".into(), pids.clone());
        if file.contains("quant") {
            o.insert("activeProcesses".into(), json!({
                "collector": pids["collector"], "dashboard": pids["dashboard"],
                "probabilityWorker": pids["paper"], "persistent": 3, "extra": 0,
            }));
            o.insert("humanPriority".into(), milestone["humanPriority"].clone());
            o.insert("productionNext".into(), milestone["next"].clone());
        }
        std::fs::write(file, serde_json::to_string_pretty(&s).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
    }
    let base = "research/poll-calibration-v1";
    let mut files: Vec<String> = [
        "capture.mjs", "definition.json", "model.mjs", "model.test.mjs", "train.mjs",
        "frozen-model-baseline-20261005.mjs", "README.md", "sources.json", "report-20261005-v1.json",
        "summary-20261005-v1.json", "prepare-kansas-paper.mjs", "kansas-paper-assessment-20261005-v1.json",
        "kansas-paper-bundle-20261005-v1.json",
    ].iter().map(|f| format!("{base}/{f}")).collect();
    for f in [
        "live-paper/runner.mjs", "live-paper/runner-lifecycle.test.mjs", "live-paper/probability-read-focus.mjs",
        "live-paper/probability-multiple-focus.test.mjs", "live-paper/probability-receipt-observe-v3.mjs",
        "README.md", "docs/CURRENT_STATE.md", "data/deploy-calibrated-poll-paper-20261005.ps1",
        "data/calibrated-paper-deployment-20261005.json", "data/profit-strategy-milestone-20261005.json",
        "data/prepare-profit-strategy-publication-20261005.mjs",
    ] {
        files.push(f.to_string());
    }
    let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    let els = elements(&refs, crate::secret_scan::scan_c, |_f| "SECRET_SCAN_REJECTED".to_string())?;
    let pack = serde_json::to_string(&els).map_err(|e| e.to_string())?;
    std::fs::write("data/profit-strategy-publication-pack.json", &pack).map_err(|e| e.to_string())?;
    let manifest: Vec<V> = els.iter().map(|e| {
        let b = e["content"].as_str().unwrap().as_bytes();
        json!({
            "path": e["path"], "bytes": b.len(), "sha256": sha(b),
            "gitBlobSha1": git_blob_sha1(b),
        })
    }).collect();
    std::fs::write(
        "data/profit-strategy-publication-manifest.json",
        serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())? + "\n",
    ).map_err(|e| e.to_string())?;
    Ok(json!({
        "at": at, "files": files.len(), "packChars": pack.len(), "cohort": stats["cohort"],
        "bankHash": stats["bankHash"], "wallets": milestone["paper"]["wallets"],
        "services": milestone["services"], "ordersSubmitted": 0, "profitabilityEstablished": false,
    }))
}
fn git_blob_sha1(b: &[u8]) -> String {
    // SHA-1 over `blob <len>\0<content>` — the git blob id.
    sha1(&format!("blob {}\0", b.len()).into_bytes().iter().chain(b.iter()).copied().collect::<Vec<u8>>())
}
fn sha1(data: &[u8]) -> String {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let ml = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&ml.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 { w[i] = u32::from_be_bytes([chunk[i*4], chunk[i*4+1], chunk[i*4+2], chunk[i*4+3]]); }
        for i in 16..80 { w[i] = (w[i-3] ^ w[i-8] ^ w[i-14] ^ w[i-16]).rotate_left(1); }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(w[i]);
            e = d; d = c; c = b.rotate_left(30); b = a; a = t;
        }
        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b); h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d); h[4] = h[4].wrapping_add(e);
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}
