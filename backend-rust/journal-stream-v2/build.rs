use sha2::{Digest,Sha256};
use std::{fs,path::Path};
fn main(){
 let mut h=Sha256::new();
 for p in ["Cargo.toml","Cargo.lock","build.rs","src/main.rs","src/canonical.rs"]{
  println!("cargo:rerun-if-changed={p}");let b=fs::read(Path::new(p)).expect("Missing journal restore source");
  h.update((p.len() as u64).to_le_bytes());h.update(p.as_bytes());h.update((b.len() as u64).to_le_bytes());h.update(b);
 }
 println!("cargo:rustc-env=JOURNAL_ENGINE_HASH={:x}",h.finalize());
}
