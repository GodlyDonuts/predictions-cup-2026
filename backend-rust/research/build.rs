// Hash the crate's source into BAAL_RESEARCH_HASH for journal receipts.
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
fn main() {
    let mut files: Vec<String> = Vec::new();
    for e in std::fs::read_dir("src").unwrap() {
        files.push(e.unwrap().path().to_string_lossy().into_owned());
    }
    files.sort();
    let mut h = DefaultHasher::new();
    for f in files {
        f.hash(&mut h);
        std::fs::read(&f).unwrap().hash(&mut h);
    }
    println!("cargo:rustc-env=BAAL_RESEARCH_HASH={:016x}", h.finish());
    println!("cargo:rerun-if-changed=src");
}
