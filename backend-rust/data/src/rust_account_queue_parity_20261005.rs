// data/rust-account-queue-parity-20261005.mjs — offline Node ReadClient / Rust core parity.
use baal_service_runtime::client::{Clocks, DispatchSpec, Fetch, FetchOutcome, FetchRequest,
    ReadClient, ReadClientOptions};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::{js_stringify, sha256};
use baal_service_runtime::wait::retry_after_ms;
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

fn sha(b: &[u8]) -> String { sha256(b) }
fn now_iso() -> String {
    ms_to_iso(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64).unwrap_or(0.0))
}
fn write_pretty(f: &Path, v: &V) -> Result<(), String> {
    std::fs::write(f, format!("{}\n", serde_json::to_string_pretty(v).map_err(|e| e.to_string())?))
        .map_err(|e| e.to_string())
}

struct Harness {
    now: f64,
    index: usize,
    persisted: Option<V>,          // last onAdmission argument (None => null)
    commands: Vec<V>,
    recorded: Vec<V>,
    statuses: Vec<i64>,
    delays: Vec<f64>,
    writes: Vec<bool>,
    retry: Option<String>,
    guard_delay: Option<f64>,
    scope: String,
    cooldown: f64,                 // replicated `client.cooldown` (Rust client keeps it private)
}

struct FixtureFetch {
    h: Arc<Mutex<Harness>>,
}
impl Fetch for FixtureFetch {
    fn fetch(&self, _url: &str, options: &FetchRequest) -> Result<FetchOutcome, String> {
        let mut h = self.h.lock().map_err(|e| e.to_string())?;
        let i = h.index;
        h.index += 1;
        let at = h.now;
        let status = *h.statuses.get(i).ok_or("fixture index")?;
        let request = format!("fixture-{}", i);
        let write = options.method != "GET";
        let binding = if write {
            json!({"intent": format!("intent-{}", i), "key": format!("key-{}", i),
                "payload_hash": "a".repeat(64)})
        } else { V::Null };
        let has_binding = !binding.is_null();
        let admit_now = at - if h.guard_delay.is_some() && i == 0 { h.guard_delay.unwrap() } else { 0.0 };
        for (op, at_v) in [("admit", admit_now), ("network", at)] {
            let mut c = serde_json::Map::new();
            c.insert("op".into(), json!(op));
            c.insert("scope".into(), json!(h.scope));
            c.insert("now".into(), json!(at_v));
            c.insert("request".into(), json!(request));
            c.insert("write".into(), json!(write));
            if has_binding { c.insert("binding".into(), binding.clone()); }
            c.insert("authorized".into(), json!(true));
            c.insert("permit_valid".into(), json!(true));
            c.insert("lease_valid".into(), json!(true));
            h.commands.push(V::Object(c));
        }
        h.now += h.delays[i];
        let scope_v = h.scope.clone();
        let now_v = h.now;
        let retry_v = h.retry.clone();
        h.commands.push(json!({"op": "complete", "scope": scope_v, "now": now_v,
            "request": request, "write": write, "status": status,
            "outcome": if status == 200 { "success" } else { "rejected" },
            "retry_after": if status == 200 { "" } else { retry_v.as_deref().unwrap_or("") }}));
        h.recorded.push(json!({"networkAt": at, "finishedAt": now_v, "status": status, "write": write}));
        // Replicate the client's cooldown arithmetic so `state.cooldown === client.cooldown`
        // can be asserted without a private getter on the Rust port.
        if status == 429 || status == 503 {
            let delay = (if status == 429 { 60000.0f64 } else { 10000.0f64 })
                .max(retry_after_ms(Some(h.retry.as_deref().unwrap_or("")), h.now));
            h.cooldown = h.now + delay;
        }
        Ok(FetchOutcome { status: status as u16,
            retry_after: Some(if status == 200 { String::new() } else { h.retry.clone().unwrap_or_default() }),
            body: "{}".to_string() })
    }
}

fn scenario(name: &str, statuses: &[i64], delays: &[f64], writes: &[bool],
    retry: Option<&str>, guard_delay: Option<f64>) -> V {
    let mut s = json!({"name": name, "statuses": statuses, "delays": delays, "writes": writes});
    if let Some(r) = retry { s["retry"] = json!(r); }
    if let Some(g) = guard_delay { s["guardDelay"] = json!(g); }
    s
}

pub fn run(root: &Path) -> Result<V, String> {
    let executable = root.join("backend-rust/account-queue/target/release/baal-account-queue.exe");
    let budget = json!({"schemaVersion": 2, "ownerId": "Codex", "enabled": true, "intervalMs": 1000,
        "writeIntervalMs": 2500, "soleAccountClient": true, "formerWriterStopCommentId": 5947535773i64});
    let scope = "offline-fixture-only";
    let scenarios = vec![
        scenario("mixed_completion_pacing", &[200, 200, 200, 200, 200, 200],
            &[359.0, 7000.0, 0.0, 11.0, 25.0, 100.0],
            &[false, true, false, true, false, false], None, None),
        scenario("slow_durable_network_fence", &[200, 200], &[400.0, 0.0],
            &[true, false], None, Some(5000.0)),
        scenario("429_retry_after_90s", &[429, 200], &[300.0, 50.0],
            &[false, false], Some("90"), None),
        scenario("503_retry_after_15s", &[503, 200], &[0.0, 50.0],
            &[false, false], Some("15"), None),
    ];
    let mut evidence: Vec<V> = Vec::new();
    for scenario_v in &scenarios {
        let statuses: Vec<i64> = scenario_v["statuses"].as_array().unwrap().iter()
            .map(|x| x.as_i64().unwrap()).collect();
        let delays: Vec<f64> = scenario_v["delays"].as_array().unwrap().iter()
            .map(|x| x.as_f64().unwrap()).collect();
        let writes: Vec<bool> = scenario_v["writes"].as_array().unwrap().iter()
            .map(|x| x.as_bool().unwrap()).collect();
        let retry = scenario_v["retry"].as_str().map(String::from);
        let guard_delay = scenario_v["guardDelay"].as_f64();
        let name = scenario_v["name"].as_str().unwrap().to_string();
        let expected_statuses = statuses.clone();
        let write_flags = writes.clone();
        let h = Arc::new(Mutex::new(Harness {
            now: 0.0, index: 0, persisted: None, commands: Vec::new(), recorded: Vec::new(),
            statuses, delays, writes, retry, guard_delay,
            scope: scope.to_string(), cooldown: 0.0,
        }));
        let now_handle = h.clone();
        let persisted_handle = h.clone();
        let sleep_handle = h.clone();
        let client = ReadClient::new("OFFLINE_PLACEHOLDER", ReadClientOptions {
            network_budget: Some(budget.clone()),
            clocks: Clocks {
                now: Arc::new(move || now_handle.lock().map(|g| g.now).unwrap_or(0.0)),
                sleep: Arc::new(move |ms| {
                    if let Ok(mut g) = sleep_handle.lock() { g.now += ms; }
                }),
            },
            on_admission: Some(Arc::new(move |n: Option<f64>| {
                if let Ok(mut g) = persisted_handle.lock() {
                    g.persisted = Some(n.map(|v| json!(v)).unwrap_or(V::Null));
                }
            })),
            authorize_dispatch: Some(Arc::new(|_spec: &V| true)),
            fetch: Some(Arc::new(FixtureFetch { h: h.clone() })),
            injected_clock: true,
            monitor_event_loop: false,
            ..Default::default()
        }).map_err(|e| e)?;
        for i in 0..write_flags.len() {
            let outcome = if write_flags[i] {
                let guard_handle = h.clone();
                let guard_delay2 = guard_delay;
                client.dispatch(DispatchSpec {
                    method: "POST".into(), endpoint: "/orders".into(),
                    body: None, body_json: None,
                    before_dispatch: Some(Arc::new(|| Ok(()))),
                    before_network: Some(Arc::new(move || {
                        if i == 0 {
                            if let Ok(mut g) = guard_handle.lock() {
                                g.now += guard_delay2.unwrap_or(0.0);
                            }
                        }
                        Ok(())
                    })),
                }).map(|_| ()).map_err(|e| e.status)
            } else {
                client.request("/tournaments/midterm-elections").map(|_| ()).map_err(|e| e.status)
            };
            if let Err(status) = outcome {
                if status.map(|s| s as i64) != Some(expected_statuses[i]) {
                    return Err(format!("assert.equal err.status {:?} != {:?}", status,
                        expected_statuses[i]));
                }
            }
        }
        client.close();
        {
            let mut g = h.lock().map_err(|e| e.to_string())?;
            let now_v2 = g.now;
            g.commands.push(json!({"op": "status", "scope": scope, "now": now_v2}));
        }
        let dir = std::env::temp_dir().join(format!("baal-offline-rust-queue-{}-{}",
            std::process::id(), std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let result = (|| -> Result<V, String> {
            let g = h.lock().map_err(|e| e.to_string())?;
            let input = g.commands.iter().map(|c| js_stringify(c)).collect::<Vec<_>>().join("\n") + "\n";
            let n_commands = g.commands.len();
            drop(g);
            let mut child = Command::new(&executable)
                .args(["--offline-jsonl"]).arg(&dir).arg(scope).arg("offline-fixture-lock")
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
                .spawn().map_err(|e| e.to_string())?;
            child.stdin.take().unwrap().write_all(input.as_bytes()).map_err(|e| e.to_string())?;
            let r = child.wait_with_output().map_err(|e| e.to_string())?;
            if r.status.code() != Some(0) {
                return Err(format!("assert.equal status 0 != {:?}: {}", r.status.code(),
                    String::from_utf8_lossy(&r.stderr)));
            }
            let responses: Vec<V> = String::from_utf8_lossy(&r.stdout).trim().split('\n')
                .map(|l| serde_json::from_str(l).map_err(|e| e.to_string()))
                .collect::<Result<_, _>>()?;
            if responses.len() != n_commands {
                return Err(format!("assert.equal responses {} != commands {}", responses.len(), n_commands));
            }
            let g = h.lock().map_err(|e| e.to_string())?;
            for (i, resp) in responses.iter().enumerate() {
                if resp["ok"] != json!(true) {
                    return Err(format!("{} command {}: {}", name, i, js_stringify(resp)));
                }
                if g.commands[i]["op"].as_str() == Some("admit") && resp["result"]["admitted"] != json!(true) {
                    return Err("admission rejected".into());
                }
            }
            let state = &responses.last().unwrap()["result"]["state"];
            if state["next"] != g.persisted.clone().unwrap_or(V::Null) {
                return Err(format!("assert.equal state.next {} != persisted {}",
                    js_stringify(&state["next"]), js_stringify(&g.persisted.clone().unwrap_or(V::Null))));
            }
            if state["cooldown"].as_f64().unwrap_or(f64::NAN) != g.cooldown {
                return Err(format!("assert.equal state.cooldown {} != {}", state["cooldown"], g.cooldown));
            }
            if state["active"] != V::Null { return Err("assert.equal active".into()); }
            let expected_reads = g.recorded.iter().enumerate()
                .filter(|(idx, r)| {
                    !r["write"].as_bool().unwrap_or(false)
                        && g.now - (r["networkAt"].as_f64().unwrap_or(0.0)
                            - if g.guard_delay.is_some() && *idx == 0 { g.guard_delay.unwrap() } else { 0.0 }) <= 60000.0
                }).count();
            if state["reads"].as_array().map(|a| a.len()).unwrap_or(0) != expected_reads {
                return Err("assert.equal reads".into());
            }
            Ok(json!({"scenario": name, "requests": g.recorded.len(),
                "nodeRustFinalReservationParity": true, "next": state["next"],
                "cooldown": state["cooldown"], "networkSequence": g.recorded}))
        })();
        let _ = std::fs::remove_dir_all(&dir);
        evidence.push(result?);
    }
    let result = json!({"schemaVersion": 1, "recordedAt": now_iso(),
        "mode": "offline_actual_Node_ReadClient_Rust_core_parity",
        "binarySha256": sha(&std::fs::read(&executable).map_err(|e| e.to_string())?),
        "scenarios": evidence.clone(),
        "limitations": ["No API, credentials, runtime bank or production client was accessed.",
            "Rust explicitly enforces 60 reads/24 writes in an inclusive rolling minute; Node pacing alone permits boundary counts of 61/25 in existing tests. That conservative extra boundary differs intentionally.",
            "This measures correctness, not profitability or faster account dispatch. Existing production HTTP/authority validators remain Node."]});
    write_pretty(&root.join("data/rust-account-queue-parity-20261005.json"), &result)?;
    println!("{}", js_stringify(&json!({"passedScenarios": evidence.len(),
        "binarySha256": result["binarySha256"]})));
    Ok(result)
}
