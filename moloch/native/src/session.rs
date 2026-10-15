//! A single durable source/financial journal. Projections are replaceable;
//! committed input, immutable seed/policy, and the durable head are not.
use crate::{engine::Engine, journal::Journal};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn hash(v: &Value) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(v).expect("JSON")))
}
fn sealed(mut v: Value) -> Value {
    v["hash"] = json!(hash(&v));
    v
}
fn verified(path: &Path) -> Result<Value> {
    let mut v: Value = serde_json::from_slice(&fs::read(path)?)?;
    let claimed = v
        .as_object_mut()
        .ok_or("PAPER_OBJECT")?
        .remove("hash")
        .ok_or("PAPER_HASH")?;
    if claimed.as_str() != Some(hash(&v).as_str()) {
        return Err("PAPER_HASH_MISMATCH".into());
    }
    v["hash"] = claimed;
    Ok(v)
}
fn options() -> OpenOptions {
    let mut o = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o
}
fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    Ok(())
}
pub fn atomic_json(path: &Path, value: &Value) -> Result<()> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temp = path.with_extension(format!("{}.{}.pending", std::process::id(), nonce));
    let mut f = options().write(true).create_new(true).open(&temp)?;
    serde_json::to_writer(&mut f, value)?;
    f.write_all(b"\n")?;
    f.sync_all()?;
    fs::rename(&temp, path)?;
    sync_dir(path.parent().ok_or("PAPER_PARENT")?)?;
    Ok(())
}
pub struct ExclusiveLock {
    _file: File,
}
impl ExclusiveLock {
    pub fn acquire(path: &Path) -> Result<Self> {
        let (file, new) = match options().read(true).write(true).create_new(true).open(path) {
            Ok(f) => (f, true),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                (options().read(true).write(true).open(path)?, false)
            }
            Err(e) => return Err(e.into()),
        };
        file.try_lock().map_err(|_| "PAPER_WRITER_LOCKED")?;
        let mut file = file;
        if !new {
            let mut data = String::new();
            file.read_to_string(&mut data)?;
            let v: Value = serde_json::from_str(&data).map_err(|_| "LEGACY_OR_UNKNOWN_LOCK")?;
            if v["kind"] != "moloch_rust_os_lock" {
                return Err("LEGACY_OR_UNKNOWN_LOCK".into());
            }
        }
        file.set_len(0)?;
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(0))?;
        write!(
            file,
            "{}\n",
            json!({"kind":"moloch_rust_os_lock","pid":std::process::id()})
        )?;
        file.sync_all()?;
        sync_dir(path.parent().ok_or("PAPER_LOCK_PARENT")?)?;
        Ok(Self { _file: file })
    }
}
pub fn engine_identity() -> String {
    let mut h = Sha256::new();
    for bytes in [
        include_bytes!("engine.rs").as_slice(),
        include_bytes!("lib.rs").as_slice(),
        include_bytes!("legacy.rs").as_slice(),
        include_bytes!("journal.rs").as_slice(),
        include_bytes!("session.rs").as_slice(),
        include_bytes!("../Cargo.lock").as_slice(),
    ] {
        h.update(bytes);
        h.update(b"\0");
    }
    format!("{:x}", h.finalize())
}
pub struct Session {
    root: PathBuf,
    _lock: ExclusiveLock,
    file: File,
    chain: Journal,
    engine: Engine,
    manifest: Value,
    refs: BTreeMap<(String, u64), String>,
    cursors: BTreeMap<String, u64>,
    facts: BTreeMap<String, Value>,
    poisoned: bool,
}
impl Session {
    pub fn open(root: &Path, seed: Option<&Path>, policy: &Value) -> Result<Self> {
        let existed = root.exists();
        if !existed {
            fs::create_dir(root)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
            }
            sync_dir(root.parent().ok_or("PAPER_PARENT")?)?;
        }
        if fs::symlink_metadata(root)?.file_type().is_symlink() {
            return Err("PAPER_DIRECTORY_SYMLINK".into());
        }
        let lock = ExclusiveLock::acquire(&root.join("writer.lock"))?;
        let (mut engine, seed_identity) = if let Some(path) = seed {
            let (engine, snapshot, digest) = crate::legacy::load_snapshot(path)?;
            (
                engine,
                json!({"kind":"legacy","snapshotHash":snapshot["hash"],"fileSha256":digest}),
            )
        } else {
            (Engine::new(policy)?, json!({"kind":"independent_paper"}))
        };
        engine.update_policy(policy)?;
        let expected = json!({"schemaVersion":1,"mode":"rust_paper_session","seed":seed_identity,"policy":engine.policy(),"engineIdentity":engine_identity()});
        let manifest_path = root.join("manifest.json");
        let journal_path = root.join("events.jsonl");
        let head_path = root.join("head.json");
        let manifest = if existed {
            let m = verified(&manifest_path)?;
            for k in ["schemaVersion", "mode", "seed", "policy", "engineIdentity"] {
                if m[k] != expected[k] {
                    return Err("PAPER_IDENTITY_CHANGED".into());
                }
            }
            m
        } else {
            let m = sealed(expected);
            let mut f = options()
                .write(true)
                .create_new(true)
                .open(&manifest_path)?;
            serde_json::to_writer(&mut f, &m)?;
            f.write_all(b"\n")?;
            f.sync_all()?;
            let journal = options().write(true).create_new(true).open(&journal_path)?;
            journal.sync_all()?;
            atomic_json(
                &head_path,
                &sealed(json!({"manifestHash":m["hash"],"sequence":0,"previous":""})),
            )?;
            m
        };
        let head = verified(&head_path)?;
        if head["manifestHash"] != manifest["hash"] {
            return Err("PAPER_HEAD_MANIFEST".into());
        }
        let head_sequence = head["sequence"].as_u64().ok_or("PAPER_HEAD_SEQUENCE")?;
        let head_previous = head["previous"].as_str().ok_or("PAPER_HEAD_PREVIOUS")?;
        if head_sequence == 0 && !head_previous.is_empty() {
            return Err("PAPER_HEAD_PREVIOUS".into());
        }
        let mut chain = Journal::default();
        let mut refs = BTreeMap::new();
        let mut cursors = BTreeMap::new();
        let mut facts = BTreeMap::new();
        let mut input = BufReader::new(File::open(&journal_path)?);
        let mut line = Vec::new();
        while input.read_until(b'\n', &mut line)? > 0 {
            if !line.ends_with(b"\n") {
                return Err("PAPER_PARTIAL_JOURNAL_TAIL".into());
            }
            if line.len() > 16 * 1024 * 1024 {
                return Err("PAPER_ROW_LIMIT".into());
            }
            let row: Value = serde_json::from_slice(&line)?;
            let event = chain.accept(&row)?;
            Self::restore_ref(&event, &mut refs, &mut cursors)?;
            engine.advance(&event)?;
            Self::remember(&event, &mut facts);
            if chain.sequence == head_sequence && chain.previous != head_previous {
                return Err("PAPER_HEAD_CHAIN".into());
            }
            line.clear();
        }
        if chain.sequence < head_sequence {
            return Err("PAPER_COMMITTED_JOURNAL_TRUNCATED".into());
        }
        let file = options().append(true).open(&journal_path)?;
        file.sync_all()?;
        let session = Self {
            root: root.to_owned(),
            _lock: lock,
            file,
            chain,
            engine,
            manifest,
            refs,
            cursors,
            facts,
            poisoned: false,
        };
        session.publish_head()?;
        session.publish_report()?;
        Ok(session)
    }
    fn restore_ref(
        event: &Value,
        refs: &mut BTreeMap<(String, u64), String>,
        cursors: &mut BTreeMap<String, u64>,
    ) -> Result<()> {
        let r = &event["sourceRef"];
        let id = r["id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 256 && !s.contains(['\n', '\r']))
            .ok_or("PAPER_SOURCE_ID")?;
        let sequence = r["sequence"].as_u64().ok_or("PAPER_SOURCE_SEQUENCE")?;
        let mut raw = event.clone();
        raw.as_object_mut()
            .ok_or("PAPER_EVENT")?
            .remove("sourceRef");
        let digest = hash(&raw);
        if r["hash"].as_str() != Some(digest.as_str())
            || sequence != cursors.get(id).copied().unwrap_or(0) + 1
            || refs.insert((id.into(), sequence), digest).is_some()
        {
            return Err("PAPER_SOURCE_CHAIN".into());
        }
        cursors.insert(id.into(), sequence);
        Ok(())
    }
    fn remember(event: &Value, facts: &mut BTreeMap<String, Value>) {
        if event["type"] == "trade" {
            let key = format!("{}:{}", event["exchangeId"], event["id"]);
            facts.entry(key).or_insert_with(|| event.clone());
        }
    }
    fn publish_head(&self) -> Result<()> {
        atomic_json(
            &self.root.join("head.json"),
            &sealed(
                json!({"manifestHash":self.manifest["hash"],"sequence":self.chain.sequence,"previous":self.chain.previous}),
            ),
        )
    }
    fn publish_report(&self) -> Result<()> {
        atomic_json(
            &self.root.join("report.json"),
            &sealed(
                json!({"manifestHash":self.manifest["hash"],"sequence":self.chain.sequence,"previous":self.chain.previous,"report":self.engine.report()?,"orders":self.engine.order_records(),"fills":self.engine.fills(),"sourceCursors":self.cursors,"contestOrders":0}),
            ),
        )
    }
    pub fn accept(&mut self, source_id: &str, sequence: u64, event: &Value) -> Result<bool> {
        if self.poisoned {
            return Err("PAPER_SESSION_POISONED".into());
        }
        if event.get("sourceRef").is_some() {
            return Err("PAPER_SOURCE_REF_RESERVED".into());
        }
        if source_id.is_empty()
            || source_id.len() > 256
            || source_id.contains(['\r', '\n'])
            || sequence == 0
        {
            return Err("PAPER_SOURCE_ID".into());
        }
        let digest = hash(event);
        if let Some(old) = self.refs.get(&(source_id.to_owned(), sequence)) {
            if *old == digest {
                return Ok(false);
            }
            return Err("PAPER_SOURCE_PREFIX_CHANGED".into());
        }
        if sequence != self.cursors.get(source_id).copied().unwrap_or(0) + 1 {
            return Err("PAPER_SOURCE_SEQUENCE_GAP".into());
        }
        let mut event = event.clone();
        event["sourceRef"] = json!({"id":source_id,"sequence":sequence,"hash":digest});
        let mut next = self.engine.clone();
        next.advance(&event)?;
        let body =
            json!({"sequence":self.chain.sequence+1,"previous":self.chain.previous,"event":event});
        let row = sealed(body);
        let mut chain = self.chain.clone();
        chain.accept(&row)?;
        let mut bytes = serde_json::to_vec(&row)?;
        bytes.push(b'\n');
        self.poisoned = true;
        self.file.write_all(&bytes)?;
        self.file.sync_all()?;
        atomic_json(
            &self.root.join("head.json"),
            &sealed(
                json!({"manifestHash":self.manifest["hash"],"sequence":chain.sequence,"previous":chain.previous}),
            ),
        )?;
        self.engine = next;
        self.chain = chain;
        self.refs.insert((source_id.into(), sequence), digest);
        self.cursors.insert(source_id.into(), sequence);
        Self::remember(&event, &mut self.facts);
        self.publish_report()?;
        self.poisoned = false;
        Ok(true)
    }
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
    pub fn manifest_hash(&self) -> &Value {
        &self.manifest["hash"]
    }
    pub fn source_facts(&self) -> Vec<Value> {
        self.facts.values().cloned().collect()
    }
    pub fn last_source_sequence(&self, id: &str) -> u64 {
        self.cursors.get(id).copied().unwrap_or(0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "moloch-session-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn events() -> Vec<Value> {
        let c: Value =
            serde_json::from_str(include_str!("../tests/reference/golden.json")).unwrap();
        c[0]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["event"].clone())
            .collect()
    }
    #[test]
    fn restart_preserves_fill_and_repeated_input_is_exactly_once() {
        let t = Temp::new();
        let root = t.0.join("bank");
        let mut s =
            Session::open(&root, None, &json!({"quantity":1,"feePerShare":"0.000100"})).unwrap();
        for (i, e) in events().iter().enumerate() {
            assert!(s.accept("fixture", i as u64 + 1, e).unwrap());
        }
        let expected = s.engine.report().unwrap();
        assert_eq!(expected["realized"], "0.004800");
        drop(s);
        let mut s =
            Session::open(&root, None, &json!({"quantity":1,"feePerShare":"0.000100"})).unwrap();
        for (i, e) in events().iter().enumerate() {
            assert!(!s.accept("fixture", i as u64 + 1, e).unwrap());
        }
        assert_eq!(s.engine.report().unwrap(), expected);
    }
    #[test]
    fn competing_writer_cannot_open_and_released_kernel_lock_can() {
        let t = Temp::new();
        let root = t.0.join("bank");
        let s = Session::open(&root, None, &json!({})).unwrap();
        assert!(Session::open(&root, None, &json!({})).is_err());
        drop(s);
        assert!(Session::open(&root, None, &json!({})).is_ok());
    }
    #[test]
    fn committed_truncation_and_partial_tail_fail_closed() {
        for partial in [false, true] {
            let t = Temp::new();
            let root = t.0.join("bank");
            let mut s = Session::open(&root, None, &json!({})).unwrap();
            s.accept("fixture", 1, &events()[0]).unwrap();
            drop(s);
            let path = root.join("events.jsonl");
            if partial {
                OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .unwrap()
                    .write_all(b"{\"partial\":")
                    .unwrap();
            } else {
                OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .unwrap()
                    .set_len(0)
                    .unwrap();
            }
            assert!(Session::open(&root, None, &json!({})).is_err());
        }
    }
    #[test]
    fn lost_projection_is_rebuilt_and_changed_policy_rejected() {
        let t = Temp::new();
        let root = t.0.join("bank");
        let mut s = Session::open(&root, None, &json!({})).unwrap();
        s.accept("fixture", 1, &events()[0]).unwrap();
        let expected = s.engine.report().unwrap();
        drop(s);
        fs::write(root.join("report.json"), b"corrupt cache").unwrap();
        assert!(Session::open(&root, None, &json!({"quantity":7})).is_err());
        let s = Session::open(&root, None, &json!({})).unwrap();
        assert_eq!(s.engine.report().unwrap(), expected);
        assert_eq!(
            verified(&root.join("report.json")).unwrap()["report"],
            expected
        );
    }
    #[test]
    fn altered_source_prefix_and_invalid_event_do_not_change_finance() {
        let t = Temp::new();
        let root = t.0.join("bank");
        let mut s = Session::open(&root, None, &json!({})).unwrap();
        s.accept("fixture", 1, &events()[0]).unwrap();
        let before = s.engine.report().unwrap();
        let mut changed = events()[0].clone();
        changed["book"]["sequence"] = json!("100");
        assert!(s.accept("fixture", 1, &changed).is_err());
        assert!(s.accept("fixture", 3, &events()[1]).is_err());
        let mut invalid = events()[1].clone();
        invalid["quantity"] = json!(-1);
        assert!(s.accept("fixture", 2, &invalid).is_err());
        assert_eq!(before, s.engine.report().unwrap());
    }
    #[test]
    fn fully_written_event_before_head_update_recovers_once() {
        let t = Temp::new();
        let root = t.0.join("bank");
        let mut s = Session::open(&root, None, &json!({})).unwrap();
        let original = events()[0].clone();
        let mut event = original.clone();
        event["sourceRef"] = json!({"id":"fixture","sequence":1,"hash":hash(&original)});
        let row = sealed(json!({"sequence":1,"previous":"","event":event}));
        writeln!(s.file, "{}", row).unwrap();
        s.file.sync_all().unwrap();
        drop(s);
        let mut s = Session::open(&root, None, &json!({})).unwrap();
        assert_eq!(s.engine.report().unwrap()["orders"], 2);
        assert!(!s.accept("fixture", 1, &original).unwrap());
        assert_eq!(verified(&root.join("head.json")).unwrap()["sequence"], 1);
    }
}
