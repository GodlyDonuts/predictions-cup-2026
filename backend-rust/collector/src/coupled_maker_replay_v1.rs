// live-paper/coupled-maker-replay-v1.mjs — replay over coupled-maker-paper-v1 using the
// shared stable-maker source loader (imported from stable-maker-replay-v2.mjs upstream).
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};
use crate::stable_maker_replay_v2 as shared;

/// replayCoupledMakerEvents(events, bank)
pub fn replay_coupled_maker_events(events: &[V], bank: &V) -> Result<V, String> {
    shared::replay_events_impl(events, bank,
        crate::coupled_maker_paper_v1::create_coupled_maker_state,
        crate::coupled_maker_paper_v1::advance_coupled_maker,
        crate::coupled_maker_paper_v1::coupled_maker_report)
}

/// runCoupledMakerReplay({runtimeDir, bank, executable, binaryHash, out, workingDir})
pub fn run_coupled_maker_replay(runtime_dir: &Path, bank: &V, executable: &Path, binary_hash: &str,
    out: &Path, working_dir: Option<&Path>) -> Result<V, String> {
    let owned_dir: PathBuf;
    let working_dir = match working_dir {
        Some(w) => w,
        None => {
            owned_dir = runtime_dir.join("coupled-maker-diagnostics").join(shared::uuid_v4());
            &owned_dir
        }
    };
    let started = std::time::Instant::now();
    let sources = shared::load_stable_maker_sources(runtime_dir, working_dir, executable, binary_hash)?;
    let events = shared::stable_maker_events(&sources, bank)?;
    let loaded = std::time::Instant::now();
    let replay = replay_coupled_maker_events(&events, bank)?;
    let finished = std::time::Instant::now();
    let mut record = shared::base_record("coupled_maker_actual_capture_development_replay_v1",
        bank, &sources, &events, &replay, started, loaded, finished);
    record.insert("fills".into(), replay["state"]["wallets"]["COUPLED"]["fills"].clone());
    record.insert("redemptions".into(), replay["state"]["wallets"]["COUPLED"]["redemptions"].clone());
    record.insert("networkRequests".into(), json!(0));
    record.insert("competitionOrders".into(), json!(0));
    record.insert("fees".into(), json!(0));
    record.insert("profitabilityEstablished".into(), json!(false));
    record.insert("limitation".into(), json!("Historical development diagnostic. The model was implemented after this capture; hypothetical fills are not actual orders or prospective proof."));
    shared::write_record(out, record)
}
