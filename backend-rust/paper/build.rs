use sha2::{Digest, Sha256};
use std::fs;
fn main() {
    let files = [
        "Cargo.toml",
        "build.rs",
        "src/lib.rs",
        "src/schema.rs",
        "src/split.rs",
        "src/engine.rs",
        "src/cli.rs",
    ];
    let mut hash = Sha256::new();
    for f in files {
        println!("cargo:rerun-if-changed={f}");
        hash.update(f.as_bytes());
        hash.update([0]);
        hash.update(fs::read(f).expect("Bound runtime source must exist"));
        hash.update([0]);
    }
    println!("cargo:rustc-env=BAAL_PAPER_HASH={:x}", hash.finalize());
}
