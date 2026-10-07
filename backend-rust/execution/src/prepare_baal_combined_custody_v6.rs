// execution/prepare-baal-combined-custody-v6.mjs — root-controlled descriptor
// preparation. No client, timer, restart or STOP removal.
use crate::baal_combined_custody_host_v6::{
    baal_combined_custody_v6_identity, create_baal_combined_custody_v6_descriptor,
};
use crate::baal_scaled_controller_legacy_replay_v1::{
    baal_scaled_no_controller_identity, replay_baal_scaled_controller_v1,
};
use crate::breadth::validate_universe;
use crate::core::payload_hash;
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::fixed::{amount, instant, money};
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Map, Value as V};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("PREPARE_CUSTODY_V6_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}
fn sha(b: &[u8]) -> String {
    sha256(b)
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn instant_v(v: &V) -> f64 {
    instant(v.as_str().unwrap_or("")).map(|m| m as f64).unwrap_or(f64::NAN)
}
fn iso(n: f64) -> String {
    ms_to_iso(n)
}
const FILES: [&str; 4] = [
    "baal-scaled-live-request.json",
    "baal-scaled-host-upgrade-v5.json",
    "paired-live-authority.json",
    "baal-capital-allocation.json",
];

fn created_ms(m: &fs::Metadata) -> f64 {
    m.created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(f64::NAN)
}

/// windowsProcessSnapshot() — execFileSync('pwsh.exe', ...) returning
/// [{pid,commandLine}].
pub fn windows_process_snapshot() -> Result<V, String> {
    let command = "Get-CimInstance Win32_Process | Select-Object ProcessId,CommandLine | ConvertTo-Json -Compress";
    let out = Command::new("pwsh.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", command])
        .output()
        .map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?;
    if !out.status.success() {
        return Err(format!("PREPARE_CUSTODY_V6_IO:pwsh exit {}", out.status));
    }
    let body = String::from_utf8_lossy(&out.stdout);
    let parsed: V = serde_json::from_str(body.trim())
        .map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?;
    let rows = match &parsed {
        V::Array(a) => a.clone(),
        other => vec![other.clone()],
    };
    Ok(V::Array(
        rows.into_iter()
            .filter(|r| r["ProcessId"].as_i64().unwrap_or(0) != 0)
            .map(|r| {
                json!({
                    "pid": r["ProcessId"],
                    "commandLine": r["CommandLine"].as_str().unwrap_or(""),
                })
            })
            .collect(),
    ))
}

/// /(?:collector|runner|paper-observe)/ or /(?:live-paper[\\/]runner\.mjs|
/// run-collector[^ ]*\.mjs)/ on the lowercase command line.
fn is_runtime_producer(cmd_lower: &str, base: &str, root: &str) -> bool {
    let normalized = cmd_lower.replace('\\', "/");
    let in_dir = cmd_lower.contains(base) || normalized.contains(root);
    let named = cmd_lower.contains("collector")
        || cmd_lower.contains("runner")
        || cmd_lower.contains("paper-observe")
        || normalized.contains("live-paper/runner.mjs")
        || cmd_lower.contains("live-paper\\runner.mjs")
        || run_collector_mjs(cmd_lower);
    in_dir && named && !cmd_lower.contains("prepare-baal-combined-custody-v6")
}
fn run_collector_mjs(cmd_lower: &str) -> bool {
    let mut idx = 0usize;
    while let Some(off) = cmd_lower[idx..].find("run-collector") {
        let start = idx + off;
        let tail = &cmd_lower[start..];
        let token = tail.split_whitespace().next().unwrap_or("");
        if token.starts_with("run-collector") && token.ends_with(".mjs") {
            return true;
        }
        idx = start + "run-collector".len();
    }
    false
}

/// assertRootControlledCustodyStop({runtimeDir,runId,expectedStopHash,
/// processSnapshot,coreLeaseFiles,now})
pub fn assert_root_controlled_custody_stop(
    runtime_dir: &Path,
    run_id: &str,
    expected_stop_hash: &str,
    process_snapshot: &V,
    core_lease_files: &[PathBuf],
    now: f64,
) -> Result<V, String> {
    need(
        hex(&json!(expected_stop_hash))
            && process_snapshot.is_array()
            && process_snapshot.as_array().unwrap().iter().all(|p| {
                p["pid"].as_i64().is_some_and(|n| n > 0) && p["commandLine"].is_string()
            }),
        "STOP_PROOF",
    )?;
    let file = runtime_dir.join("COLLECTOR_STOP");
    let bytes = fs::read(&file).map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?;
    let s: V =
        serde_json::from_slice(&bytes).map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?;
    let mut keys: Vec<String> = s
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    let mut expected = [
        "schemaVersion", "kind", "source", "ownerId", "reason", "runId", "oldPid", "at",
        "humanStop",
    ];
    expected.sort();
    need(
        keys.join("|") == expected.join("|")
            && sha(&bytes) == expected_stop_hash
            && s["schemaVersion"].as_i64() == Some(1)
            && sget(&s, "kind") == Some("baal_root_controlled_engineering_stop_v1")
            && sget(&s, "source")
                == Some("baal-combined-custody-v6-migration-20261006")
            && sget(&s, "ownerId") == Some("Codex")
            && sget(&s, "reason") == Some("financial_owner_custody_v6_migration")
            && sget(&s, "runId") == Some(run_id)
            && s["humanStop"].as_bool() == Some(false)
            && s["oldPid"].as_i64().is_some_and(|n| n > 0)
            && instant_v(get(&s, "at")) <= now
            && now - instant_v(get(&s, "at")) <= 900_000.0,
        "ENGINEERING_STOP_ONLY",
    )?;
    let old_pid = s["oldPid"].as_i64().unwrap_or(0);
    need(
        !process_snapshot
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["pid"].as_i64() == Some(old_pid)),
        "ORIGINAL_PID_PRESENT",
    )?;
    let base = runtime_dir
        .canonicalize()
        .unwrap_or_else(|_| runtime_dir.to_path_buf())
        .to_string_lossy()
        .to_lowercase();
    let root = base.replace('\\', "/");
    need(
        !process_snapshot.as_array().unwrap().iter().any(|p| {
            is_runtime_producer(
                &p["commandLine"].as_str().unwrap_or("").to_lowercase(),
                &base,
                &root,
            )
        }),
        "OTHER_RUNTIME_PRODUCER_PRESENT",
    )?;
    need(
        !runtime_dir.join("EXECUTION_STOP").exists(),
        "EXECUTION_OR_HUMAN_STOP_PRESENT",
    )?;
    let legacy = runtime_dir.join("baal-scaled-live-runs").join(run_id);
    let mut locks = vec![
        legacy.join("controller").join("execution-owner.lock"),
        legacy.join("budget").join("execution-owner.lock"),
        runtime_dir.join("baal-financial-owner").join("execution-owner.lock"),
        runtime_dir
            .join("competition-runs")
            .join("sig-pilot-median-20261006-human")
            .join("execution-owner.lock"),
    ];
    locks.extend(core_lease_files.iter().cloned());
    for lock in &locks {
        let absolute = lock
            .canonicalize()
            .unwrap_or_else(|_| {
                if lock.is_absolute() {
                    lock.clone()
                } else {
                    std::env::current_dir().unwrap_or_default().join(lock)
                }
            })
            .to_string_lossy()
            .to_lowercase();
        need(
            absolute.starts_with(&format!("{}{}", base, std::path::MAIN_SEPARATOR))
                && !lock.exists(),
            "OWNER_LEASE_PRESENT",
        )?;
    }
    let snap: Vec<V> = process_snapshot
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            json!({
                "pid": p["pid"],
                "commandHash": sha(p["commandLine"].as_str().unwrap_or("").as_bytes()),
            })
        })
        .collect();
    Ok(json!({
        "stopHash": sha(&bytes),
        "oldPid": old_pid,
        "processSnapshotHash": payload_hash(&V::Array(snap))?,
        "checkedAt": iso(now),
        "leaseFilesChecked": locks.len(),
    }))
}

/// prepareBaalCombinedCustodyV6({runtimeDir,workspace,universeFile,
/// firstExchangeId,implementationEpoch,startedAt,recordedAt,now})
pub fn prepare_baal_combined_custody_v6(
    runtime_dir: &Path,
    workspace: &Path,
    universe_file: Option<&Path>,
    first_exchange_id: &str,
    implementation_epoch: &str,
    started_at: &str,
    recorded_at: Option<&str>,
    now: f64,
) -> Result<V, String> {
    let universe_file = universe_file
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("live-paper/price-universe.json"));
    let recorded = recorded_at.unwrap_or(started_at);
    need(
        runtime_dir.is_absolute()
            && workspace.is_absolute()
            && universe_file.is_absolute()
            && !implementation_epoch.is_empty()
            && implementation_epoch.len() <= 100
            && implementation_epoch
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "ARGUMENTS",
    )?;
    let start = instant_v(&json!(started_at));
    // /^\d{4}-\d{2}-\d{2}T.*Z$/
    let b = started_at.as_bytes();
    let started_iso = b.len() >= 12
        && b[..4].iter().all(|c| c.is_ascii_digit())
        && b[4] == b'-'
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[7] == b'-'
        && b[8..10].iter().all(|c| c.is_ascii_digit())
        && b[10] == b'T'
        && started_at.ends_with('Z');
    need(
        started_iso && start <= now && now - start <= 300_000.0,
        "EXPLICIT_FRESH_ROOT_CLOCK",
    )?;
    need(
        instant_v(&json!(recorded)) == start,
        "EXPLICIT_FRESH_ROOT_CLOCK",
    )?;
    let mut raw = Map::new();
    let mut values = Map::new();
    for f in FILES {
        let b = fs::read(runtime_dir.join(f))
            .map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?;
        let v: V = serde_json::from_slice(&b)
            .map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?;
        raw.insert(f.to_string(), json!(sha(&b)));
        values.insert(f.to_string(), v);
    }
    let request = values[FILES[0]].clone();
    let upgrade = values[FILES[1]].clone();
    let authority = values[FILES[2]].clone();
    let capital = values[FILES[3]].clone();
    let universe_bytes =
        fs::read(&universe_file).map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?;
    let universe_parsed: V = serde_json::from_slice(&universe_bytes)
        .map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?;
    let universe = validate_universe(&universe_parsed, sget(&request, "tournamentId").unwrap_or(""))
        .map_err(|e| format!("PREPARE_CUSTODY_V6_{e}"))?;
    need(
        instant_v(get(&request, "entryExpiresAt")) < start
            && sget(&request, "ownerId") == Some("Codex")
            && sget(&request, "kind") == Some("baal_scaled_live_production_v1")
            && sget(&request, "authorityHash")
                == Some(payload_hash(&authority)?.as_str())
            && sget(&authority, "kind") == Some("direct_human_authorized_bot_update")
            && sget(&authority, "ownerId") == Some("Codex")
            && sget(&request, "humanAllocationSourceHash")
                == Some(payload_hash(&capital)?.as_str())
            && sget(&capital, "allocatedNominalCapital") == Some("1000.000000")
            && sget(&capital, "fee") == Some("0.000000"),
        "ROOT_AUTHORITY",
    )?;
    need(
        sget(&universe, "tournamentId") == sget(&request, "tournamentId")
            && sget(&universe, "hash") == sget(&request, "universeHash")
            && universe.get("exchanges").is_some_and(V::is_array),
        "UNIVERSE",
    )?;
    let allowed: Vec<&str> = get(&request, "allowedMarkets")
        .as_array()
        .map(|a| a.iter().filter_map(V::as_str).collect())
        .unwrap_or_default();
    let members: Vec<V> = get(&universe, "exchanges")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| allowed.contains(&sget(m, "marketId").unwrap_or("")))
        .collect();
    let scope = json!({
        "ownerId": "Codex",
        "runId": get(&request, "runId"),
        "tournamentId": get(&request, "tournamentId"),
        "tournamentSlug": get(&request, "tournamentSlug"),
        "exchanges": members.iter().map(|m| json!({
            "marketId": get(m, "marketId"),
            "exchangeId": get(m, "exchangeId"),
        })).collect::<Vec<V>>(),
    });
    let allocation = json!({
        "schemaVersion": 1,
        "ownerId": "Codex",
        "runId": get(&request, "runId"),
        "tournamentId": get(&request, "tournamentId"),
        "capital": "1000.000000",
        "maximumOrderCost": "25.000000",
        "maximumMarketCost": "100.000000",
        "maximumEventCost": "200.000000",
        "maximumSharesPerOrder": 100,
        "maximumActiveMarkets": 20,
        "humanAllocationSourceHash": get(&request, "humanAllocationSourceHash"),
        "recordedAt": get(&request, "startedAt"),
    });
    let legacy_file = runtime_dir
        .join("baal-scaled-live-runs")
        .join(sget(&request, "runId").unwrap_or(""))
        .join("controller")
        .join("baal-scaled-no-journal.jsonl");
    let before = fs::metadata(&legacy_file)
        .map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?;
    let source = fs::read(&legacy_file)
        .map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?;
    let old_code = baal_scaled_no_controller_identity()?;
    let seed = replay_baal_scaled_controller_v1(&json!({
        "bodyText": std::str::from_utf8(&source)
            .map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?,
        "expectedCodeIdentity": old_code,
        "expectedScope": scope,
        "expectedAllocation": allocation,
    }))?;
    let after = fs::metadata(&legacy_file)
        .map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?;
    need(
        before.ino() == after.ino()
            && created_ms(&before) == created_ms(&after)
            && before.len() == after.len()
            && source.len() as u64 == after.len()
            && sha(&fs::read(&legacy_file).map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?)
                == sget(get(&seed, "anchor"), "sha256").unwrap_or(""),
        "SOURCE_CHANGED",
    )?;
    need(
        baal_service_runtime::json_text::js_int(get(
            &get(
                &get(&seed, "state"),
                "inventory",
            )
            .get(first_exchange_id)
            .map(|x| get(x, "proof"))
            .unwrap_or(&V::Null),
            "remainingOwnedQuantity",
        ))
        .unwrap_or(0)
            > 0,
        "SELECTED_LOT_FLAT_OR_UNKNOWN",
    )?;
    let descriptor = create_baal_combined_custody_v6_descriptor(
        workspace,
        &universe,
        &request,
        &upgrade,
        &seed,
        first_exchange_id,
        implementation_epoch,
        started_at,
        &iso(start + 3_600_000.0),
        &iso(start + 86_400_000.0),
        recorded,
    )?;
    let inventory = get(&get(&seed, "state"), "inventory");
    let mut owned_qty = 0i64;
    let mut owned_cost = 0i128;
    let mut realized = 0i128;
    let mut positive = 0i64;
    if let V::Object(m) = inventory {
        for x in m.values() {
            let proof = get(x, "proof");
            let qty = baal_service_runtime::json_text::js_int(get(
                proof,
                "remainingOwnedQuantity",
            ))
            .unwrap_or(0);
            if qty > 0 {
                positive += 1;
            }
            owned_qty += qty;
            if let Ok(a) = amount(sget(proof, "remainingOwnedCost").unwrap_or("0")) {
                owned_cost += a;
            }
            if let Ok(a) = amount(sget(proof, "cumulativeRealizedPnl").unwrap_or("0")) {
                realized += a;
            }
        }
    }
    let evidence = json!({
        "schemaVersion": 1,
        "kind": "root_prepared_custody_v6_descriptor_v1",
        "descriptorHash": payload_hash(&descriptor)?,
        "legacyAnchor": get(&seed, "anchor"),
        "seedHash": payload_hash(&seed)?,
        "firstExchangeId": first_exchange_id,
        "originalBuyExpiry": get(&request, "entryExpiresAt"),
        "hedgeExpiresAt": get(get(&descriptor, "activation"), "hedgeExpiresAt"),
        "exitExpiresAt": get(get(&descriptor, "activation"), "exitExpiresAt"),
        "currentPositiveSlots": positive,
        "currentOwnedQuantity": owned_qty,
        "currentOwnedPrincipal": money(owned_cost),
        "originalRealizedPnl": money(realized),
        "sourceHashes": raw,
        "universeFileHash": sha(&universe_bytes),
        "codeIdentityHash": payload_hash(&baal_combined_custody_v6_identity(workspace)?)?,
        "preparedAt": iso(now),
        "executionAuthority": false,
        "networkRequests": 0,
    });
    Ok(json!({
        "descriptor": descriptor,
        "evidence": evidence,
        "legacyFile": legacy_file,
        "workspace": workspace,
        "universeFile": universe_file,
    }))
}

/// writePreparedCustodyV6({prepared,runtimeDir,targetFile,install,
/// expectedStopHash,processSnapshot,coreLeaseFiles,now})
pub fn write_prepared_custody_v6(
    prepared: &V,
    runtime_dir: &Path,
    target_file: &Path,
    install: bool,
    expected_stop_hash: Option<&str>,
    process_snapshot: Option<&V>,
    core_lease_files: &[PathBuf],
    now: f64,
) -> Result<V, String> {
    need(
        target_file.is_absolute() && !target_file.exists(),
        "NEW_OUTPUT_ONLY",
    )?;
    let actual = runtime_dir.join("baal-combined-custody-request-v6.json");
    let resolved_target = target_file
        .canonicalize()
        .unwrap_or_else(|_| target_file.to_path_buf());
    let resolved_actual = actual
        .canonicalize()
        .unwrap_or_else(|_| actual.clone());
    need(
        if install {
            resolved_target == resolved_actual
        } else {
            resolved_target != resolved_actual
        },
        "DRAFT_CANNOT_ACTIVATE",
    )?;
    let mut stop_evidence = V::Null;
    if install {
        stop_evidence = assert_root_controlled_custody_stop(
            runtime_dir,
            sget(get(prepared, "descriptor"), "runId").unwrap_or(""),
            expected_stop_hash.unwrap_or(""),
            process_snapshot.unwrap_or(&V::Null),
            core_lease_files,
            now,
        )?;
    }
    let workspace = PathBuf::from(sget(prepared, "workspace").unwrap_or(""));
    let universe_file = PathBuf::from(sget(prepared, "universeFile").unwrap_or(""));
    let legacy_file = PathBuf::from(sget(prepared, "legacyFile").unwrap_or(""));
    need(
        payload_hash(&baal_combined_custody_v6_identity(&workspace)?).ok()
            == Some(sget(get(prepared, "evidence"), "codeIdentityHash").unwrap_or("").to_string())
            && sha(&fs::read(&universe_file).map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?)
                == sget(get(prepared, "evidence"), "universeFileHash").unwrap_or(""),
        "CODE_OR_UNIVERSE_CHANGED_BEFORE_WRITE",
    )?;
    need(
        sha(&fs::read(&legacy_file).map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?)
            == sget(get(get(prepared, "evidence"), "legacyAnchor"), "sha256")
                .unwrap_or(""),
        "SEAL_CHANGED_BEFORE_WRITE",
    )?;
    if let V::Object(m) = get(get(prepared, "evidence"), "sourceHashes") {
        for (f, h) in m {
            need(
                sha(&fs::read(runtime_dir.join(f))
                    .map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?)
                    == h.as_str().unwrap_or(""),
                "SOURCE_CHANGED_BEFORE_WRITE",
            )?;
        }
    }
    let body = if install {
        get(prepared, "descriptor").clone()
    } else {
        let mut b = prepared.clone();
        if let V::Object(m) = &mut b {
            let mut ev = get(prepared, "evidence").clone();
            if let V::Object(em) = &mut ev {
                em.insert("stopEvidence".into(), stop_evidence.clone());
                em.insert("executionAuthority".into(), json!(false));
            }
            m.insert("evidence".into(), ev);
        }
        b
    };
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(target_file)
        .map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?;
    f.write_all(
        (serde_json::to_string_pretty(&body).unwrap_or_default() + "\n").as_bytes(),
    )
    .map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?;
    f.sync_all().map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?;
    drop(f);
    Ok(json!({
        "path": target_file,
        "sha256": sha(&fs::read(target_file).map_err(|e| format!("PREPARE_CUSTODY_V6_IO:{e}"))?),
        "installed": install,
        "stopEvidence": stop_evidence,
    }))
}
