//! Offline only: link against one pinned optimized moloch-replay build.
//! No networking, account, worker, authority or source-event admission calls.
use moloch_replay::session::{Session, engine_identity, propose_live_engine_upgrade};
use serde_json::{Value, json};
use std::{env, fs, path::{Path, PathBuf}, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len()==2 && args[1]=="identity" {println!("{}",engine_identity());return Ok(())}
    if args.len()<4 {return Err("OFFLINE_PROBE_ARGUMENTS".into())}
    let command=&args[1];let root=PathBuf::from(&args[2]);
    if !root.is_absolute() || root.canonicalize()?!=root || !root.to_string_lossy().starts_with("/tmp/moloch-native-flow-benchmark-") {return Err("OFFLINE_PROBE_COPY_PATH".into())}
    // Marker is created only in a create-new detached trial directory by the
    // guarded driver. This is not a production activation or bridge approval.
    let marker:Value=serde_json::from_slice(&fs::read(root.join("OFFLINE_ONLY.json"))?)?;
    if marker["offlineOnly"]!=true || marker["productionAuthorization"]!=false || marker["productionStopped"]!=false || args[3]!=engine_identity() {return Err("OFFLINE_PROBE_IDENTITY".into())}
    match command.as_str() {
        "propose" if args.len()==6 => {
            // Existing API verifies original hash chain/head/report references;
            // it only returns a proposal. The driver pins original12 entries.
            println!("{}",propose_live_engine_upgrade(&root,&args[4],&args[5])?);
        },
        "replay" if args.len()==4 => {
            let manifest:Value=serde_json::from_slice(&fs::read(root.join("manifest.json"))?)?;
            if manifest["mode"]!="rust_live_session" || manifest["seed"]["kind"]!="actual_account_allocation" {return Err("OFFLINE_PROBE_ORIGINAL_SEED".into())}
            let started=Instant::now();
            let bank=Session::open_live(Path::new(&root),&manifest["policy"],&manifest["seed"]["evidence"])?;
            let elapsed=started.elapsed().as_nanos();let head=bank.durable_head();
            let engine=bank.engine();
            let comparison=json!({"report":engine.report()?,"orders":engine.order_records(),"fills":engine.fills(),"books":engine.live_package_books(),"bindings":engine.live_order_bindings(),"required":engine.required_exchanges(),"packages":engine.package_records(),"policy":engine.policy()});
            let mut state=fs::OpenOptions::new().write(true).create_new(true).open(root.join("comparison-state.json"))?;
            serde_json::to_writer(&mut state,&comparison)?;
            use std::io::Write;state.write_all(b"\n")?;state.sync_all()?;drop(bank);
            println!("{}",json!({"schemaVersion":1,"offlineOnly":true,"engineIdentity":engine_identity(),"openLiveElapsedNs":elapsed,"durableHead":head,"networkRequests":0,"sourceAdmissionCalls":0,"productionAuthorization":false}));
        },
        _ => return Err("OFFLINE_PROBE_COMMAND".into()),
    }
    Ok(())
}
