// execution/proven-network-guard-v1.mjs — install when creating a spec,
// before registering its callback identity. On guard failure it appends one
// blocked-network proof line to proofFile, then rethrows.
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use baal_service_runtime::contract::ms_to_iso;
use serde_json::{json, Value as V};

use crate::blocked_network_proof_v1::blocked_network_proof_v1;

/// The inner guard JS resolves per call: `await getGuard()()`.
/// getGuard may itself throw (Result), and the resolved guard may throw.
pub type GuardFactory = Box<dyn Fn() -> Result<Box<dyn Fn() -> Result<V, String>>, String>>;

pub struct ProvenNetworkGuardV1 {
    spec: V,
    get_guard: GuardFactory,
    proof_file: PathBuf,
    now: Box<dyn Fn() -> f64>,
}

/// provenNetworkGuardV1({spec,getGuard,proofFile,now})
pub fn proven_network_guard_v1(
    spec: &V,
    get_guard: GuardFactory,
    proof_file: &Path,
    now: Box<dyn Fn() -> f64>,
) -> Result<ProvenNetworkGuardV1, String> {
    if !proof_file.is_absolute() {
        return Err("PROVEN_GUARD_INPUT".into());
    }
    Ok(ProvenNetworkGuardV1 {
        spec: spec.clone(),
        get_guard,
        proof_file: proof_file.to_path_buf(),
        now,
    })
}

impl ProvenNetworkGuardV1 {
    /// The returned guard closure: try the resolved guard; on any error record
    /// the proof line (append, 0o600, fsync) and rethrow.
    pub fn run(&self) -> Result<V, String> {
        match (self.get_guard)().and_then(|g| g()) {
            Ok(v) => Ok(v),
            Err(message) => {
                let error = json!({"message": message});
                let proof = blocked_network_proof_v1(&json!({
                    "spec": self.spec,
                    "error": error,
                    "at": ms_to_iso((self.now)()),
                }))?;
                let line = format!("{}\n", serde_json::to_string(&proof).map_err(|e| e.to_string())?);
                let mut fd = fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .mode(0o600)
                    .open(&self.proof_file)
                    .map_err(|e| e.to_string())?;
                let write_result = fd
                    .write_all(line.as_bytes())
                    .and_then(|_| fd.sync_all())
                    .map_err(|e| e.to_string());
                drop(fd);
                write_result?;
                Err(message)
            }
        }
    }
}
