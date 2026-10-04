use sha2::{Digest, Sha256};
use std::{fs, path::Path};
fn main() {
    let mut hash = Sha256::new();
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        ".cargo/config.toml",
        "src/main.rs",
        "src/money.rs",
        "src/types.rs",
        "src/capture.rs",
        "src/cache.rs",
        "src/engine.rs",
        "src/intent_observation.rs",
        "src/replay.rs",
        "src/trace.rs",
        "src/sweep.rs",
        "src/evaluation.rs",
        "src/publication.rs",
        "src/market_observations.rs",
        "src/quote_wallet.rs",
        "src/quote_replay.rs",
    ] {
        println!("cargo:rerun-if-changed={path}");
        let bytes = fs::read(Path::new(path)).expect("Missing native source graph");
        hash.update((path.len() as u64).to_le_bytes());
        hash.update(path.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    println!("cargo:rustc-env=BAAL_ENGINE_HASH={:x}", hash.finalize());
}
