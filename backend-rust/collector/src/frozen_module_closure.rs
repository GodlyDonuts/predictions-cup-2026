// live-paper/frozen-module-closure.mjs — sha256 manifest over the complete
// relative .mjs dependency closure. Dynamic entry points are supplied by the
// caller (entryFiles).
use std::path::{Path, PathBuf};

use indexmap::IndexMap;

fn is_word_byte(b: u8) -> bool { b.is_ascii_alphanumeric() || b == b'_' || b == b'$' }
fn word_boundary(b: &[u8], i: usize) -> bool {
    let before = i > 0 && is_word_byte(b[i - 1]);
    let after = i < b.len() && is_word_byte(b[i]);
    before != after
}
fn word_at(b: &[u8], i: usize, w: &[u8]) -> bool {
    i + w.len() <= b.len() && &b[i..i + w.len()] == w && word_boundary(b, i) && word_boundary(b, i + w.len())
}
// Skip whitespace going backwards from position i; returns index of first
// non-ws byte at or before i.
fn skip_ws_back(b: &[u8], mut i: usize) -> usize {
    while i > 0 && (b[i - 1] as char).is_whitespace() { i -= 1; }
    i
}

/// Collects relative './x.mjs' string literals that match one of the three JS
/// regex patterns:
///   A: /\b(?:import|export)\s*(?:[^;]*?\bfrom\s*)?['"](\.[^'"]+\.mjs)['"]/g
///   B: /\bimport\s*\(\s*['"](\.[^'"]+\.mjs)['"]\s*\)/g
///   C: /\bimport\s*\(\s*new\s+URL\s*\(\s*['"](\.[^'"]+\.mjs)['"]/g
fn relative_imports(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        let q = b[i];
        if q != b'\'' && q != b'"' { i += 1; continue; }
        let start = i + 1;
        let mut end = start;
        while end < b.len() && b[end] != q { end += 1; }
        if end >= b.len() { break; }
        let lit = &text[start..end];
        let captured = lit.starts_with('.') && lit.ends_with(".mjs") && lit.len() > 4;
        if !captured { i = end + 1; continue; }
        let qi = i; // index of the opening quote
        // Pattern B/C: the char before the quote (past whitespace) is '('.
        let p = skip_ws_back(b, qi);
        if p > 0 && b[p - 1] == b'(' {
            let k = skip_ws_back(b, p - 1); // word before '('
            if k >= 6 && word_at(b, k - 6, b"import") {
                out.push(lit.to_string());
            } else if k >= 3 && word_at(b, k - 3, b"URL") {
                let k2 = skip_ws_back(b, k - 3);
                if k2 >= 3 && word_at(b, k2 - 3, b"new") {
                    let k3 = skip_ws_back(b, k2 - 3);
                    if k3 > 0 && b[k3 - 1] == b'(' {
                        let k4 = skip_ws_back(b, k3 - 1);
                        if k4 >= 6 && word_at(b, k4 - 6, b"import") {
                            out.push(lit.to_string());
                        }
                    }
                }
            }
        } else {
            // Pattern A: within the same statement (since the previous ';'),
            // the last import/export keyword must reach the quote via
            // whitespace-only or any text ending in `from` + whitespace.
            let stmt = text[..qi].rfind(';').map(|x| x + 1).unwrap_or(0);
            let window = &b[stmt..qi];
            let mut last_kw: Option<usize> = None;
            let mut j = 0usize;
            while j < window.len() {
                if word_at(window, j, b"import") { last_kw = Some(j); j += 6; }
                else if word_at(window, j, b"export") { last_kw = Some(j); j += 6; }
                else { j += 1; }
            }
            if let Some(kw) = last_kw {
                let between = &text[stmt + kw + 6..qi];
                let t = between.trim_end();
                let ok = t.is_empty() || (t.len() >= 4 && t.ends_with("from")
                    && t[..t.len() - 4].chars().last().map_or(true, |c| !is_word_byte(c as u8)));
                if ok { out.push(lit.to_string()); }
            }
        }
        i = end + 1;
    }
    out
}

// Bank declarations remain immutable. Snapshot manifests include the complete
// relative dependency closure even when original declarations missed compact
// ESM syntax. Dynamic entry points must be supplied explicitly by the caller.
/// relativeMjsClosure({workspace, entryFiles}) — returns {name: sha256-hex} in
/// first-visit insertion order.
pub fn relative_mjs_closure(workspace: &Path, entry_files: &[&str])
    -> Result<IndexMap<String, String>, String> {
    // path.resolve: normalize . and .. lexically (no symlink resolution).
    fn normalize(p: &Path) -> PathBuf {
        let mut out = PathBuf::new();
        for c in p.components() {
            match c {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => { out.pop(); }
                other => out.push(other.as_os_str()),
            }
        }
        out
    }
    let base = normalize(workspace);
    let mut files: IndexMap<String, String> = IndexMap::new();
    fn visit(base: &Path, file: &Path, files: &mut IndexMap<String, String>,
             normalize: &dyn Fn(&Path) -> PathBuf) -> Result<(), String> {
        let resolved = normalize(&base.join(file));
        if !resolved.starts_with(base) || resolved.extension().and_then(|e| e.to_str()) != Some("mjs") {
            return Err("FROZEN_CLOSURE_SCOPE".into());
        }
        let name = resolved.strip_prefix(base).unwrap_or(&resolved)
            .to_string_lossy().replace('\\', "/");
        if files.contains_key(&name) { return Ok(()); }
        let bytes = std::fs::read(&resolved).map_err(|e| e.to_string())?;
        files.insert(name.clone(), baal_service_runtime::json_text::sha256(&bytes));
        let text = String::from_utf8_lossy(&bytes).to_string();
        let dir = resolved.parent().unwrap_or(base).to_path_buf();
        for rel in relative_imports(&text) {
            visit(base, &dir.join(rel), files, normalize)?;
        }
        Ok(())
    }
    for f in entry_files {
        visit(&base, Path::new(f), &mut files, &normalize)?;
    }
    Ok(files)
}
