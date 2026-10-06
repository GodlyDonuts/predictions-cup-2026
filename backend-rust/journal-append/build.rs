use sha2::{Digest, Sha256};
use std::fs;
fn main() {
    let files = [
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        "src/lib.rs",
        "src/main.rs",
        "src/canonical.rs",
    ];
    let mut hash = Sha256::new();
    for f in files {
        println!("cargo:rerun-if-changed={f}");
        hash.update(f.as_bytes());
        hash.update([0]);
        hash.update(fs::read(f).expect("Bound engine source must exist"));
        hash.update([0]);
    }
    println!("cargo:rustc-env=APPEND_ENGINE_HASH={:x}", hash.finalize());
}
