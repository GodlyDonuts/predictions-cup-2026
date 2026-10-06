use sha2::{Digest, Sha256};
use std::fs;
fn main() {
    let mut files: Vec<String> = std::fs::read_dir("src")
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| format!("src/{}", e.file_name().to_string_lossy()))
        .filter(|f| f.ends_with(".rs"))
        .collect();
    files.sort();
    let mut all = vec!["Cargo.toml".to_string(), "build.rs".to_string()];
    all.extend(files);
    let mut hash = Sha256::new();
    for f in &all {
        println!("cargo:rerun-if-changed={f}");
        hash.update(f.as_bytes());
        hash.update([0]);
        hash.update(fs::read(f).expect("Bound runtime source must exist"));
        hash.update([0]);
    }
    println!("cargo:rustc-env=BAAL_CAPTURE_LIVE_HASH={:x}", hash.finalize());
}
