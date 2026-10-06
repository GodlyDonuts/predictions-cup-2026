use sha2::{Digest, Sha256};
use std::fs;
fn main() {
    let files = [
        "Cargo.toml",
        "build.rs",
        "src/lib.rs",
        "src/json_text.rs",
        "src/atomic.rs",
        "src/journal.rs",
        "src/ledger.rs",
        "src/wait.rs",
        "src/budget.rs",
        "src/timing.rs",
        "src/client.rs",
        "src/exclusions.rs",
        "src/history.rs",
        "src/tests.rs",
    ];
    let mut hash = Sha256::new();
    for f in files {
        println!("cargo:rerun-if-changed={f}");
        hash.update(f.as_bytes());
        hash.update([0]);
        hash.update(fs::read(f).expect("Bound runtime source must exist"));
        hash.update([0]);
    }
    println!("cargo:rustc-env=BAAL_SERVICE_RUNTIME_HASH={:x}", hash.finalize());
}
