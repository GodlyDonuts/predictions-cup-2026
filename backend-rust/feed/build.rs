use sha2::{Digest, Sha256};
use std::fs;
fn main() {
    let files = [
        "Cargo.toml",
        "build.rs",
        "src/lib.rs",
        "src/capture.rs",
        "src/collector.rs",
        "src/runtime.rs",
        "src/replay.rs",
    ];
    let mut hash = Sha256::new();
    for f in files {
        println!("cargo:rerun-if-changed={f}");
        hash.update(f.as_bytes());
        hash.update([0]);
        hash.update(fs::read(f).expect("Bound runtime source must exist"));
        hash.update([0]);
    }
    println!("cargo:rustc-env=BAAL_FEED_HASH={:x}", hash.finalize());
}
