// ml/cli.mjs — offline experiment entrypoint: local-path arguments, exclusive
// artifact directory, sha256-manifested outputs. Byte/semantic port.
//
// `code_fingerprint` hashes the JavaScript runtime sources under `ml_dir`
// (the Rust port cannot hash itself through import.meta.url).
use crate::core::{canonical, hash, js_pretty, parse_dataset, Error, R};
use crate::experiment::{develop, final_evaluation, prepare};
use serde_json::{json, Value as V};
use std::io::Write;
use std::path::Path;

/// RUNTIME_FILES — the JS implementation surface covered by the fingerprint.
pub const RUNTIME_FILES: [&str; 4] = ["core.mjs", "model.mjs", "experiment.mjs", "cli.mjs"];
/// codeFingerprint() — sha256 each runtime file plus a canonical digest of the
/// manifest. `ml_dir` is the directory holding the .mjs sources.
pub fn code_fingerprint(ml_dir: &Path) -> R<V> {
    let mut files = Vec::with_capacity(RUNTIME_FILES.len());
    for name in RUNTIME_FILES {
        let bytes = std::fs::read(ml_dir.join(name)).map_err(io_error)?;
        files.push(json!({"path": format!("ml/{name}"), "sha256": hash(&bytes)}));
    }
    let files = V::Array(files);
    Ok(json!({"implementationSha256": hash(canonical(&files)), "files": files}))
}
fn local_path(value: &str) -> R<String> {
    let scheme = {
        // /[a-z][a-z0-9+.-]*:\/\//i
        let b = value.as_bytes();
        let mut hit = false;
        for i in 0..b.len().saturating_sub(2) {
            if b[i] == b':' && b[i + 1] == b'/' && b[i + 2] == b'/' {
                let head = &value[..i];
                if !head.is_empty()
                    && head.as_bytes()[0].is_ascii_alphabetic()
                    && head.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'.' | b'-'))
                {
                    hit = true;
                }
            }
        }
        hit
    };
    if value.is_empty() || value.contains('\0') || scheme || value.starts_with('-') {
        return Err(Error::code("LOCAL_PATH_REQUIRED"));
    }
    Ok(value.to_string())
}
/// Node-style I/O error message: the leading ERRNO code is what the CLI
/// failure summary extracts.
fn io_error(e: std::io::Error) -> Error {
    let code = match e.kind() {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::AlreadyExists => "EEXIST",
        std::io::ErrorKind::IsADirectory => "EISDIR",
        std::io::ErrorKind::NotADirectory => "ENOTDIR",
        _ => "EIO",
    };
    Error::code(format!("{code}: {e}"))
}
fn read_local(path: &str, max_bytes: u64) -> R<String> {
    let stat = std::fs::metadata(path).map_err(io_error)?;
    if !stat.is_file() { return Err(Error::code("REGULAR_FILE_REQUIRED")); }
    if stat.len() > max_bytes { return Err(Error::code("INPUT_TOO_LARGE")); }
    let bytes = std::fs::read(path).map_err(io_error)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if text.len() > max_bytes as usize { return Err(Error::code("INPUT_TOO_LARGE")); }
    Ok(text)
}
/// args(argv) — parsed CLI options, or None for `[]`/`--help`.
#[derive(Debug)]
pub struct CliOptions {
    pub command: String,
    pub input: String,
    pub config: String,
    pub out: String,
    /// `--selection`, present only for `final`.
    pub selection: Option<String>,
}
pub fn args(argv: &[String]) -> R<Option<CliOptions>> {
    if argv.is_empty() || (argv.len() == 1 && argv[0] == "--help") { return Ok(None); }
    let command = argv[0].as_str();
    if command != "develop" && command != "final" { return Err(Error::code("OFFLINE_COMMAND_ONLY")); }
    let allowed: Vec<&str> = if command == "final" {
        vec!["--input", "--config", "--out", "--selection"]
    } else {
        vec!["--input", "--config", "--out"]
    };
    let mut options: indexmap::IndexMap<String, String> = indexmap::IndexMap::new();
    let mut i = 1;
    while i < argv.len() {
        let flag = argv[i].as_str();
        if !allowed.contains(&flag) || options.contains_key(flag) || i + 1 >= argv.len() || argv[i + 1].is_empty() {
            return Err(Error::code("ARGUMENTS"));
        }
        options.insert(flag.to_string(), local_path(&argv[i + 1])?);
        i += 2;
    }
    if options.len() != allowed.len() { return Err(Error::code("ARGUMENTS")); }
    Ok(Some(CliOptions {
        command: command.to_string(),
        input: options["--input"].clone(),
        config: options["--config"].clone(),
        out: options["--out"].clone(),
        selection: options.get("--selection").cloned(),
    }))
}
fn write_exclusive(path: &Path, bytes: &[u8]) -> R<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(io_error)?;
    f.write_all(bytes).map_err(io_error)
}
/// main(argv) — the full develop/final pipeline plus artifact writes.
/// Returns the JSON summary on success; Err with .summary set on
/// INSUFFICIENT_DATA rejection.
pub fn main(argv: &[String], ml_dir: &Path) -> R<V> {
    let options = match args(argv)? {
        Some(o) => o,
        None => {
            return Ok(json!({
                "help": "Offline book baseline. node ml/cli.mjs develop --input books.jsonl --config config.json --out NEW_DIRECTORY. Final evaluation additionally requires command final and --selection development/selection.json. No network, credentials, orders or deployment."
            }))
        }
    };
    let input = read_local(&options.input, 100 * 1024 * 1024)?;
    let config_text = read_local(&options.config, 1024 * 1024)?;
    let config: V = serde_json::from_str(&config_text).map_err(|e| Error::code(e.to_string()))?;
    let fingerprint = code_fingerprint(ml_dir)?;
    let mut argv_v: Vec<V> = vec![json!("node"), json!("ml/cli.mjs")];
    argv_v.extend(argv.iter().map(|a| json!(a)));
    let runtime = json!({
        "implementationSha256": fingerprint["implementationSha256"].clone(),
        "files": fingerprint["files"].clone(),
        // process.version — a Rust process has none; null records "not node".
        "node": V::Null,
        "argv": V::Array(argv_v),
        "configFileSha256": hash(&config_text),
        "seed": config["seed"].clone(),
        "platformArithmetic": "IEEE-754 binary64; exact nanosecond timestamp comparisons; no monetary accounting",
    });
    let prepared = prepare(parse_dataset(&input)?, &config)?;
    let result = if options.command == "final" {
        let saved: V = serde_json::from_str(&read_local(options.selection.as_deref().unwrap_or_default(), 1024 * 1024)?)
            .map_err(|e| Error::code(e.to_string()))?;
        final_evaluation(&prepared, &runtime, &saved)?
    } else {
        develop(&prepared, &runtime)?
    };
    // Exclusive directory plus exclusive files. Report and completion manifest
    // are last. On an I/O failure a partial directory is not a successful run;
    // never overwrite it.
    let out_dir = Path::new(&options.out);
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(out_dir).map_err(io_error)?;
    }
    let mut artifacts: Vec<(&str, V)> = vec![
        ("selection.json", result.selection.clone()),
        ("split-manifest.json", result.split_manifest.clone()),
    ];
    if let Some(p) = &result.predictions { artifacts.push(("test-predictions.json", p.clone())); }
    artifacts.push(("report.json", result.report.clone()));
    let mut hashes = Vec::with_capacity(artifacts.len());
    for (name, value) in artifacts {
        let content = format!("{}\n", js_pretty(&value));
        write_exclusive(&out_dir.join(name), content.as_bytes())?;
        hashes.push(json!({"name": name, "sha256": hash(&content)}));
    }
    let complete = json!({
        "schemaVersion": 1, "complete": true,
        "fingerprint": result.report["fingerprint"].clone(),
        "artifacts": V::Array(hashes),
    });
    write_exclusive(&out_dir.join("complete.json"), format!("{}\n", js_pretty(&complete)).as_bytes())?;
    Ok(json!({
        "status": result.report["status"].clone(),
        "dataKind": result.report["dataKind"].clone(),
        "warning": result.report["warning"].clone(),
        "counts": result.report["counts"].clone(),
        "coverage": result.report["coverage"].clone(),
        "selectedAlpha": result.model["alpha"].clone(),
        "validation": result.report["validation"].clone(),
        "test": result.report["test"].clone(),
        "output": options.out,
    }))
}
/// /^[A-Z_]+(?:\b|:)/ — leading run of uppercase/underscores followed by a
/// word boundary or ':' identifies a coded error message.
fn extract_code(message: &str) -> &str {
    let b = message.as_bytes();
    let mut i = 0;
    while i < b.len() && (b[i].is_ascii_uppercase() || b[i] == b'_') { i += 1; }
    let ok = i > 0 && (i == b.len() || b[i] == b':' || !(b[i].is_ascii_alphanumeric() || b[i] == b'_'));
    if ok { message.split(':').next().unwrap_or("INVALID_INPUT_OR_IO") } else { "INVALID_INPUT_OR_IO" }
}
/// The process.argv entry gate: Ok(text) is the stdout payload (exit 0);
/// Err(text) is the stderr payload (exit 1).
pub fn cli_entry(argv: &[String], ml_dir: &Path) -> Result<String, String> {
    match main(argv, ml_dir) {
        Ok(v) => Ok(format!("{}\n", js_pretty(&v))),
        Err(e) => {
            let summary = match e.summary {
                Some(s) => s,
                None => json!({"status": "failed", "code": extract_code(&e.message)}),
            };
            Err(format!("{}\n", js_pretty(&summary)))
        }
    }
}
