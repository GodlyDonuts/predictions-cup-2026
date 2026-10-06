use sha2::{Digest, Sha256};
use std::fs;
fn main() {
    let files = [
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        "src/lib.rs",
        "src/dual_v1.rs",
        "src/publication.rs",
        "src/history.rs",
        "src/main.rs",
        "src/settlement_value.rs",
        "src/money.rs",
        "../dual-no-engine/src/lib.rs",
        "../dual-no-engine/Cargo.toml",
        "../dual-no-engine/Cargo.lock",
    ];
    let mut h = Sha256::new();
    for f in files {
        println!("cargo:rerun-if-changed={f}");
        h.update(f.as_bytes());
        h.update([0]);
        h.update(fs::read(f).expect("Pinned source exists"));
        h.update([0]);
    }
    let hash = format!("{:x}", h.finalize());
    println!("cargo:rustc-env=BAAL_ENGINE_HASH={hash}");
    println!("cargo:rustc-env=PORTFOLIO_ENGINE_HASH={hash}");
}
