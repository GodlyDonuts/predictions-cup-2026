// live-paper/stable-maker-replay-v1.mjs — identical pipeline to v2, but replays over
// stable-maker-paper-v1 and stamps mode 'stable_maker_actual_capture_replay_v1'.
// The loader/event-builder bodies are textually identical in the .mjs sources, so they
// delegate to the v2 module here.
use serde_json::{json, Value as V};
use std::path::{Path, PathBuf};

pub use crate::stable_maker_replay_v2::{verify_chained_rows, load_stable_maker_sources,
    stable_maker_events};

/// replayStableMakerEvents(events, bank) — over stable-maker-paper-v1.
pub fn replay_stable_maker_events(events: &[V], bank: &V) -> Result<V, String> {
    crate::stable_maker_replay_v2::replay_events_impl(events, bank,
        crate::stable_maker_paper_v1::create_stable_maker_state,
        crate::stable_maker_paper_v1::advance_stable_maker,
        crate::stable_maker_paper_v1::stable_maker_report)
}

/// runStableMakerReplay({runtimeDir, bank, executable, binaryHash, out, workingDir})
pub fn run_stable_maker_replay(runtime_dir: &Path, bank: &V, executable: &Path, binary_hash: &str,
    out: &Path, working_dir: Option<&Path>) -> Result<V, String> {
    let owned_dir: PathBuf;
    let working_dir = match working_dir {
        Some(w) => w,
        None => {
            owned_dir = runtime_dir.join("stable-maker-diagnostics")
                .join(crate::stable_maker_replay_v2::uuid_v4());
            &owned_dir
        }
    };
    let started = std::time::Instant::now();
    let sources = load_stable_maker_sources(runtime_dir, working_dir, executable, binary_hash)?;
    let events = stable_maker_events(&sources, bank)?;
    let loaded = std::time::Instant::now();
    let replay = replay_stable_maker_events(&events, bank)?;
    let finished = std::time::Instant::now();
    let mut record = crate::stable_maker_replay_v2::base_record(
        "stable_maker_actual_capture_replay_v1", bank, &sources, &events,
        &replay, started, loaded, finished);
    let mut fills = serde_json::Map::new();
    for (side, w) in replay["state"]["wallets"].as_object().map(|m| m.iter()).into_iter().flatten() {
        fills.insert(side.clone(), w["fills"].clone());
    }
    record.insert("fills".into(), V::Object(fills));
    record.insert("networkRequests".into(), json!(0));
    record.insert("competitionOrders".into(), json!(0));
    record.insert("fees".into(), json!(0));
    record.insert("profitabilityEstablished".into(), json!(false));
    crate::stable_maker_replay_v2::write_record(out, record)
}
