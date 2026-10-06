// live-paper/probability-receipt-observe-v4.mjs — registry-driven wrapper, v3 root + v1 companions.
use serde_json::{json, Value as V};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::probability_receipt_observe::{ObservedWorker, ProbabilityReceiptObserveOptions};
use crate::probability_receipt_host_v2::{cohort_ok, sha};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("PROBABILITY_WRAPPER_V4_{}", code));
    }
    Ok(())
}
const LEGACY_POINTERS: [&str; 2] = [
    "calibrated-poll-paper-active.json",
    "primary-breadth-paper-active.json",
];
fn safe_hash(s: &V) -> bool {
    s.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn read(file: &Path) -> Result<V, String> {
    serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn source_identity(source_dir: &Path) -> Result<V, String> {
    let mut out = serde_json::Map::new();
    for p in [
        "probability-receipt-observe-v4.mjs",
        "frozen-paper-runtime.mjs",
        "probability-read-focus.mjs",
        "executable-edge-observe.mjs",
        "runtime.mjs",
        "model.mjs",
        "../execution/ledger.mjs",
    ] {
        out.insert(
            p.to_string(),
            json!(sha(&fs::read(source_dir.join(p)).map_err(|e| e.to_string())?)),
        );
    }
    Ok(V::Object(out))
}
fn pointer(value: &V) -> Result<V, String> {
    need(
        js_truthy_value(value) && cohort_ok(value["cohort"].as_str().unwrap_or("")) && safe_hash(&value["bankHash"]),
        "POINTER",
    )?;
    Ok(json!({"cohort": value["cohort"], "bankHash": value["bankHash"]}))
}
fn js_truthy_value(v: &V) -> bool {
    baal_service_runtime::json_text::js_truthy(Some(v))
}

#[derive(Debug)]
#[allow(dead_code)] // `registry` flag mirrors the JS pointer objects.
struct RegistryEntry {
    cohort: String,
    bank_hash: String,
    pointer_file: Option<PathBuf>,
    root: bool,
    registry: bool,
    dir: PathBuf,
    bank: V,
}
#[derive(Debug)]
#[allow(dead_code)] // registry/registry_file returned for shape parity with JS.
struct RegistryV4 {
    entries: Vec<RegistryEntry>,
    registry_file: PathBuf,
    registry: V,
    identity: String,
}
/// probabilityRegistryV4(runtimeDir, cohort)
fn probability_registry_v4(runtime_dir: &Path, cohort: &str) -> Result<RegistryV4, String> {
    need(runtime_dir.is_absolute() && cohort_ok(cohort), "DIRECTORY")?;
    let root_file = runtime_dir.join("probability-receipt-active.json");
    let root = pointer(&read(&root_file)?)?;
    need(root["cohort"].as_str() == Some(cohort), "ROOT_POINTER")?;
    let registry_file = runtime_dir.join("probability-receipt-companions.json");
    let registry: V = if registry_file.exists() {
        read(&registry_file)?
    } else {
        json!({"schemaVersion": 1, "entries": []})
    };
    let registry_keys_ok = registry.as_object().map(|m| {
        let mut keys: Vec<&str> = m.keys().map(|k| k.as_str()).collect();
        keys.sort();
        keys.join(",") == "entries,schemaVersion"
    });
    need(
        registry["schemaVersion"].as_i64() == Some(1)
            && registry_keys_ok.unwrap_or(false)
            && registry["entries"].is_array()
            && registry["entries"].as_array().map(|a| a.len() <= 4).unwrap_or(false),
        "REGISTRY",
    )?;
    let mut entries: Vec<RegistryEntry> = vec![RegistryEntry {
        cohort: root["cohort"].as_str().unwrap_or("").to_string(),
        bank_hash: root["bankHash"].as_str().unwrap_or("").to_string(),
        pointer_file: Some(root_file.clone()),
        root: true,
        registry: false,
        dir: PathBuf::new(),
        bank: V::Null,
    }];
    for name in LEGACY_POINTERS {
        let file = runtime_dir.join(name);
        if file.exists() {
            let p = pointer(&read(&file)?)?;
            entries.push(RegistryEntry {
                cohort: p["cohort"].as_str().unwrap_or("").to_string(),
                bank_hash: p["bankHash"].as_str().unwrap_or("").to_string(),
                pointer_file: Some(file),
                root: false,
                registry: false,
                dir: PathBuf::new(),
                bank: V::Null,
            });
        }
    }
    for item in registry["entries"].as_array().unwrap() {
        let item_keys_ok = item.as_object().map(|m| {
            let mut keys: Vec<&str> = m.keys().map(|k| k.as_str()).collect();
            keys.sort();
            keys.join(",") == "bankHash,cohort"
        });
        need(js_truthy_value(item) && item_keys_ok.unwrap_or(false), "REGISTRY_ENTRY")?;
        let p = pointer(item)?;
        entries.push(RegistryEntry {
            cohort: p["cohort"].as_str().unwrap_or("").to_string(),
            bank_hash: p["bankHash"].as_str().unwrap_or("").to_string(),
            pointer_file: None,
            root: false,
            registry: true,
            dir: PathBuf::new(),
            bank: V::Null,
        });
    }
    let mut seen = BTreeSet::new();
    let index = read(&runtime_dir.join("frozen-code").join("index.json"))?;
    for item in entries.iter_mut() {
        need(!seen.contains(&item.cohort), "DUPLICATE_COHORT")?;
        seen.insert(item.cohort.clone());
        item.dir = runtime_dir
            .join("probability-receipt-cohorts")
            .join(&item.cohort);
        item.bank = read(&item.dir.join("bank.json"))?;
        need(
            item.bank["name"].as_str() == Some(item.cohort.as_str())
                && crate::model::digest(&item.bank) == item.bank_hash,
            "BANK_POINTER_BINDING",
        )?;
        need(
            if item.root {
                item.bank["schemaVersion"].as_i64() == Some(4)
            } else {
                item.bank["schemaVersion"].as_i64() == Some(2)
            },
            "BANK_SCHEMA",
        )?;
        let binding = &index["banks"][baal_execution::core::payload_hash(&item.bank).unwrap_or_default()]["snapshot"];
        need(safe_hash(binding), "FROZEN_BINDING_REQUIRED")?;
    }
    let context = entries[0].bank["universe"].clone();
    need(
        entries.iter().all(|e| {
            e.bank["universe"]["hash"] == context["hash"]
                && e.bank["universe"]["tournamentId"] == context["tournamentId"]
        }),
        "UNIVERSE_SCOPE",
    )?;
    let pointers: Vec<V> = entries
        .iter()
        .map(|e| {
            json!({"cohort": e.cohort, "bankHash": e.bank_hash,
                "pointerFile": e.pointer_file.as_ref().map(|p| p.to_string_lossy().into_owned()),
                "root": e.root})
        })
        .collect();
    let identity = baal_execution::core::payload_hash(&json!({
        "registry": registry,
        "pointers": pointers,
    }))?;
    Ok(RegistryV4 {
        entries,
        registry_file,
        registry,
        identity,
    })
}

/// openProbabilityWorkersV4(options) — one worker per registry entry (root uses v3 host,
/// companions v1 host; the frozen binding maps both to the crate functions).
fn open_probability_workers_v4(
    options: &ProbabilityReceiptObserveOptions,
) -> Result<(Vec<Box<dyn ObservedWorker>>, RegistryV4), String> {
    let registry = probability_registry_v4(options.runtime_dir, options.cohort)?;
    let mut workers: Vec<Box<dyn ObservedWorker>> = Vec::new();
    let run = (|workers: &mut Vec<Box<dyn ObservedWorker>>| -> Result<(), String> {
        for entry in &registry.entries {
            if !entry.root && entry.dir.join("OBSERVER_STOP").exists() {
                continue;
            }
            let module_path = if entry.root {
                "live-paper/probability-receipt-host-v3.mjs"
            } else {
                "live-paper/probability-receipt-host.mjs"
            };
            // frozenPaperModule({runtimeDir, bank, modulePath, fallback})
            let _frozen = crate::frozen_paper_runtime::frozen_paper_module(
                options.runtime_dir,
                &entry.bank,
                module_path,
            )?;
            let worker: Box<dyn ObservedWorker> = if entry.root {
                Box::new(crate::probability_receipt_host_v3::open_probability_receipt(
                    options.runtime_dir,
                    &entry.cohort,
                    false,
                    options.bundle,
                    options.now.clone(),
                    options.workspace_dir,
                    options.workspace_dir,
                )?)
            } else {
                Box::new({
                    let n = options.now.clone();
                    crate::probability_receipt_host::open_probability_receipt(
                        options.runtime_dir,
                        &entry.cohort,
                        false,
                        options.bundle,
                        move || n(),
                        options.workspace_dir,
                    )?
                })
            };
            workers.push(worker);
            let w = workers.last().unwrap();
            need(
                w.dir() == entry.dir && w.status()?["bankHash"].as_str() == Some(entry.bank_hash.as_str()),
                "OPENED_BANK_BINDING",
            )?;
        }
        Ok(())
    })(&mut workers);
    if let Err(e) = run {
        // JS catch: workers closed in reverse; a close error replaces it.
        for worker in workers.iter_mut().rev() {
            if let Err(x) = worker.close() {
                return Err(x);
            }
        }
        return Err(e);
    }
    Ok((workers, registry))
}

/// runProbabilityReceiptV4(options)
pub fn run_probability_receipt_v4(
    options: &ProbabilityReceiptObserveOptions,
) -> Result<(), String> {
    run_probability_receipt_v4_with(options, &|entries| {
        crate::executable_edge_observe::write_observed_projections(entries)
    })
}

fn run_probability_receipt_v4_with(
    options: &ProbabilityReceiptObserveOptions,
    project: &dyn Fn(&[(PathBuf, V)]) -> Result<V, String>,
) -> Result<(), String> {
    let (mut workers, registry) = open_probability_workers_v4(options)?;
    // root is always index 0 (root entry is never OBSERVER_STOP-skipped)
    let root_dir = workers[0].dir().to_path_buf();
    let mut retired: BTreeSet<usize> = BTreeSet::new();
    let identity = source_identity(options.source_dir)?;
    let revision_file = options
        .runtime_dir
        .join("probability-receipt-wrapper-revisions.jsonl");
    let stopped = || -> bool {
        options.runtime_dir.join("STOP").exists()
            || options.runtime_dir.join("EXECUTION_STOP").exists()
            || options.workspace_dir.join("STOP").exists()
            || root_dir.join("OBSERVER_STOP").exists()
    };
    let guard = || -> Result<(), String> {
        need(!stopped(), "STOPPED")?;
        need(
            baal_execution::core::payload_hash(&source_identity(options.source_dir)?).ok()
                == baal_execution::core::payload_hash(&identity).ok(),
            "WRAPPER_CODE_CHANGED",
        )?;
        need(
            probability_registry_v4(options.runtime_dir, options.cohort)?.identity == registry.identity,
            "REGISTRY_OR_POINTER_CHANGED",
        )?;
        Ok(())
    };
    let result = (|| -> Result<(), String> {
        guard()?;
        let mut revisions = baal_service_runtime::journal::Journal::open(&revision_file)?;
        let cohorts: Vec<V> = workers
            .iter()
            .map(|w| -> Result<V, String> {
                let s = w.status()?;
                Ok(json!({"cohort": s["cohort"], "bankHash": s["bankHash"], "stateHash": s["stateHash"],
                    "journalSequence": s["journalSequence"], "journalHash": s["journalHash"],
                    "bankFileHash": sha(&fs::read(w.dir().join("bank.json")).map_err(|e| e.to_string())?),
                    "journalFileHash": sha(&fs::read(w.dir().join("journal.jsonl")).map_err(|e| e.to_string())?)}))
            })
            .collect::<Result<_, _>>()?;
        revisions.commit(
            &V::Null,
            &json!({
                "kind": "probability_observer_wrapper_code_revision_v4",
                "wrapperIdentity": identity,
                "registryIdentity": registry.identity,
                "cohorts": cohorts,
                "accountingReset": false,
                "oldFillsImported": false,
                "networkRequests": 0,
                "competitionOrders": 0,
                "executionAuthority": false,
            }),
        )?;
        loop {
            guard()?;
            if (options.stopping)() {
                break;
            }
            for (i, worker) in workers.iter_mut().enumerate() {
                guard()?;
                if (options.stopping)() {
                    break;
                }
                if retired.contains(&i) {
                    continue;
                }
                if i != 0 && worker.dir().join("OBSERVER_STOP").exists() {
                    worker.close()?;
                    retired.insert(i);
                    continue;
                }
                worker.consume()?;
                let stats = worker.status()?;
                let lock = read(&worker.dir().join("worker.lock"))?;
                let mut projections = vec![
                    (worker.dir().join("stats.json"), stats.clone()),
                    (
                        worker.dir().join("read-focus.json"),
                        crate::probability_read_focus::project_probability_read_focus(
                            &stats,
                            &lock,
                            &stats["cohort"],
                            options.workspace_dir,
                        )?,
                    ),
                ];
                if i == 0 {
                    projections.push((
                        options.runtime_dir.join("probability-receipt-active.json"),
                        json!({"cohort": options.cohort, "bankHash": stats["bankHash"]}),
                    ));
                }
                project(&projections)?;
            }
            if options.once {
                break;
            }
            (options.delay)(1000);
            if (options.stopping)() {
                break;
            }
        }
        Ok(())
    })();
    // finally: close non-retired workers in reverse; a close error aborts the
    // loop and replaces the loop result.
    let mut outcome = result;
    for (i, worker) in workers.iter_mut().enumerate().rev() {
        if retired.contains(&i) {
            continue;
        }
        if let Err(e) = worker.close() {
            outcome = Err(e);
            break;
        }
    }
    outcome
}
