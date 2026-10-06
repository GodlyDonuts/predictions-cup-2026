use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
// Hash the crate's Rust sources so the binary can prove which build is live.
fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap().path();
        if entry.is_dir() {
            walk(&entry, out);
        } else if entry.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(entry);
        }
    }
}
fn main() {
    let mut files = Vec::new();
    walk(Path::new("src"), &mut files);
    walk(Path::new("tests"), &mut files);
    files.sort();
    let mut h = Sha256::new();
    for f in &files {
        h.update(f.to_string_lossy().as_bytes());
        h.update(fs::read(f).unwrap());
    }
    println!("cargo:rustc-env=BAAL_EXECUTION_HASH={:x}", h.finalize());
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
    }
}
