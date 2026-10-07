// live-paper/coordinated-account-client-v2.mjs — a faster completion gap
// inside the SAME serialized account client. Durable rolling admission caps
// retain headroom below published 100READ/30WRITE limits.
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::client::{
    Fetch, FetchOutcome, FetchRequest, ReadClient, ReadClientOptions, ReqwestFetch,
};
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Value as V};
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const READ_CAP: usize = 60;
const WRITE_CAP: usize = 24;
const WINDOW_MS: f64 = 60000.0;

fn need(ok: bool, c: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("COORDINATED_CLIENT_{}", c));
    }
    Ok(())
}

/// The members destructured ahead of `...options` in the JS constructor:
/// `{admissionFile, coordinatedReadGapMs=100, coordination, ...options}`.
pub struct CoordinatedAccountOptions {
    /// admissionFile — must be absolute.
    pub admission_file: PathBuf,
    /// coordinatedReadGapMs — the JS default is 100 and every other value
    /// fails COORDINATED_CLIENT_EXPLICIT_COORDINATION.
    pub coordinated_read_gap_ms: i64,
    /// coordination — the baal_single_client_rolling_policy_v1 record.
    pub coordination: Option<V>,
    /// `...options` forwarded to `super(key, options)` (ReadClient).
    pub client: ReadClientOptions,
}
impl Default for CoordinatedAccountOptions {
    fn default() -> Self {
        CoordinatedAccountOptions {
            admission_file: PathBuf::new(),
            coordinated_read_gap_ms: 100,
            coordination: None,
            client: ReadClientOptions::default(),
        }
    }
}

#[derive(Default)]
struct AdmissionData {
    reads: Vec<f64>,
    writes: Vec<f64>,
}

/// Durable rolling-admission state shared by the client and its fetch
/// wrapper, exactly like the `this.readAdmissions`/`this.writeAdmissions`
/// arrays the JS wrapper closure captures through `this`.
struct Admissions {
    data: Mutex<AdmissionData>,
    file: PathBuf,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    sleep: Arc<dyn Fn(f64) + Send + Sync>,
    signal: Arc<AtomicBool>,
}
impl Admissions {
    // ReadClient.pause: sleep slices of at most 200ms, honoring the abort
    // signal mid-wait.
    fn pause(&self, ms: f64) -> Result<(), String> {
        let until = (self.now)() + ms;
        while (self.now)() < until {
            if self.signal.load(Ordering::SeqCst) {
                return Err("Stopped".to_string());
            }
            (self.sleep)((until - (self.now)()).min(200.0));
        }
        Ok(())
    }
    // async admit(method): rolling window; when full, pause until the oldest
    // admission leaves the window. The JS binds `list` to the live
    // this.readAdmissions/this.writeAdmissions array, pushes the new instant,
    // then reassigns both fields through .filter — the result equals the
    // push-then-retain sequence below.
    fn admit(&self, method: &str) -> Result<(), String> {
        let mut st = self.data.lock().map_err(|_| "Stopped".to_string())?;
        let maximum = if method == "GET" { READ_CAP } else { WRITE_CAP };
        loop {
            let at = (self.now)();
            {
                let list = if method == "GET" { &mut st.reads } else { &mut st.writes };
                while let Some(&first) = list.first() {
                    if at - first >= WINDOW_MS {
                        list.remove(0);
                    } else {
                        break;
                    }
                }
                if list.len() < maximum {
                    break;
                }
            }
            let wait = if method == "GET" { &st.reads } else { &st.writes }[0] + WINDOW_MS - at;
            self.pause(wait)?;
        }
        let at = (self.now)();
        if method == "GET" {
            &mut st.reads
        } else {
            &mut st.writes
        }
        .push(at);
        st.reads.retain(|x| at - *x < WINDOW_MS);
        st.writes.retain(|x| at - *x < WINDOW_MS);
        // ../execution/ledger.mjs atomicJson — the canonical-bytes, fsynced
        // variant, not the pretty transport-runtime one.
        atomic_json_canonical(
            &self.file,
            &json!({"schemaVersion": 1, "reads": st.reads, "writes": st.writes}),
        )
    }
}

/// `this.fetch = async (url, request) => { await this.admit(...); return
/// suppliedFetch(url, request); }` — the wrapper installed over the supplied
/// fetch after `super(key, options)` ran.
struct AdmittingFetch {
    supplied: Arc<dyn Fetch>,
    admissions: Arc<Admissions>,
}
impl Fetch for AdmittingFetch {
    fn fetch(&self, url: &str, req: &FetchRequest) -> Result<FetchOutcome, String> {
        self.admissions.admit(&req.method.to_uppercase())?;
        self.supplied.fetch(url, req)
    }
}

/// `class CoordinatedAccountClientV2 extends ReadClient` — composition plus
/// Deref, so `request`/`dispatch`/`orders`/`abort`/`requests`/`close` and the
/// public `interval`/`write_interval`/`read_only` fields resolve on the
/// wrapped client.
pub struct CoordinatedAccountClientV2 {
    client: ReadClient,
    /// admissionFile
    pub admission_file: PathBuf,
    admissions: Arc<Admissions>,
}
impl Deref for CoordinatedAccountClientV2 {
    type Target = ReadClient;
    fn deref(&self) -> &ReadClient {
        &self.client
    }
}
impl CoordinatedAccountClientV2 {
    pub fn new(key: &str, coord: CoordinatedAccountOptions) -> Result<Self, String> {
        let budget = coord.client.network_budget.as_ref();
        let c = coord.coordination.as_ref();
        need(
            coord.admission_file.is_absolute()
                && coord.coordinated_read_gap_ms == 100
                && js_int(budget.and_then(|b| b.get("schemaVersion")).unwrap_or(&V::Null))
                    == Some(4)
                && budget.and_then(|b| b.get("soleAccountClient")) == Some(&V::Bool(true))
                && c.and_then(|v| v.get("kind")).and_then(V::as_str)
                    == Some("baal_single_client_rolling_policy_v1")
                && js_int(c.and_then(|v| v.get("readCompletionGapMs")).unwrap_or(&V::Null))
                    == Some(100)
                && js_int(c.and_then(|v| v.get("writeCompletionGapMs")).unwrap_or(&V::Null))
                    == Some(2500)
                && js_int(c.and_then(|v| v.get("maxReadsPerMinute")).unwrap_or(&V::Null))
                    == Some(60)
                && js_int(c.and_then(|v| v.get("maxWritesPerMinute")).unwrap_or(&V::Null))
                    == Some(24),
            "EXPLICIT_COORDINATION",
        )?;
        let mut options = coord.client;
        // const suppliedFetch = options.fetchFn ?? fetch
        let supplied: Arc<dyn Fetch> = match options.fetch.take() {
            Some(f) => f,
            None => Arc::new(ReqwestFetch::new()?),
        };
        // The wrapper's this.now/this.sleep/this.signal are the parent's —
        // clone the same Arcs before moving the options into ReadClient::new.
        let signal = options
            .signal
            .clone()
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        options.signal = Some(signal.clone());
        let admissions = Arc::new(Admissions {
            data: Mutex::new(AdmissionData::default()),
            file: coord.admission_file.clone(),
            now: options.clocks.now.clone(),
            sleep: options.clocks.sleep.clone(),
            signal,
        });
        options.fetch = Some(Arc::new(AdmittingFetch {
            supplied,
            admissions: admissions.clone(),
        }));
        let mut client = ReadClient::new(key, options)?; // super(key, options)
        client.interval = coord.coordinated_read_gap_ms as u64; // this.interval=100
        if coord.admission_file.exists() {
            // JSON.parse(fs.readFileSync(file)) — parse/read errors propagate
            // unwrapped, exactly like the JS.
            let raw = std::fs::read(&coord.admission_file).map_err(|e| e.to_string())?;
            let r: V = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
            need(
                js_int(&r["schemaVersion"]) == Some(1)
                    && r["reads"].is_array()
                    && r["writes"].is_array(),
                "PERSISTENT_STATE",
            )?;
            for (key, maximum) in [("reads", READ_CAP), ("writes", WRITE_CAP)] {
                let list = r[key].as_array().unwrap();
                // Array.every short-circuits, so the previous element was
                // already validated when the ordering check runs.
                let mut ok = list.len() <= maximum;
                let mut prev = 0i64;
                for (i, el) in list.iter().enumerate() {
                    match js_int(el) {
                        Some(x)
                            if x >= 0
                                && (x as f64) <= (admissions.now)()
                                && (i == 0 || x >= prev) =>
                        {
                            prev = x;
                        }
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                }
                need(ok, "ADMISSION_CLOCK_OR_CAP")?;
            }
            let mut st = admissions.data.lock().map_err(|_| "Stopped".to_string())?;
            st.reads = r["reads"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap_or(0.0))
                .collect();
            st.writes = r["writes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap_or(0.0))
                .collect();
        }
        Ok(CoordinatedAccountClientV2 {
            client,
            admission_file: coord.admission_file,
            admissions,
        })
    }

    /// async admit(method) — also reachable directly on the JS instance.
    pub fn admit(&self, method: &str) -> Result<(), String> {
        self.admissions.admit(method)
    }
    /// this.readAdmissions snapshot.
    pub fn read_admissions(&self) -> Vec<f64> {
        self.admissions
            .data
            .lock()
            .map(|s| s.reads.clone())
            .unwrap_or_default()
    }
    /// this.writeAdmissions snapshot.
    pub fn write_admissions(&self) -> Vec<f64> {
        self.admissions
            .data
            .lock()
            .map(|s| s.writes.clone())
            .unwrap_or_default()
    }
}
