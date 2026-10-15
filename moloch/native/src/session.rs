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
        include_bytes!("engine_live.rs").as_slice(),
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
/// Private, bounded approvals bridge exact source identities without rewriting
/// the immutable manifest or accepting different historical financial results.
struct UpgradeChain { entries: Vec<Value>, checked: usize }
fn digest_text(value:&Value)->bool{value.as_str().is_some_and(|s|s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit()))}
fn verify_value(mut value:Value)->Result<Value>{
    let claimed=value.as_object_mut().ok_or("UPGRADE_OBJECT")?.remove("hash").ok_or("UPGRADE_HASH")?;
    if claimed.as_str()!=Some(hash(&value).as_str()){return Err("UPGRADE_HASH_MISMATCH".into())}
    value["hash"]=claimed;Ok(value)
}
fn upgrade_document(root:&Path)->Result<Value>{
    let path=root.join("engine-upgrade-v1.json");let metadata=fs::symlink_metadata(&path)?;
    if !metadata.is_file()||metadata.len()>8*1024*1024{return Err("UPGRADE_FILE_BOUND".into())}
    verify_value(serde_json::from_slice(&fs::read(path)?)?)
}
impl UpgradeChain {
    fn load(root:&Path,manifest:&Value,target:&str,head_sequence:u64)->Result<Self>{
        let document=upgrade_document(root)?;
        if document["schemaVersion"]!=1||document["kind"]!="moloch_live_engine_upgrade_chain_v1"||document["manifestHash"]!=manifest["hash"]{return Err("UPGRADE_SCOPE".into())}
        let entries=document["upgrades"].as_array().filter(|xs|!xs.is_empty()&&xs.len()<=16).ok_or("UPGRADE_CHAIN_BOUND")?.clone();
        if !digest_text(&manifest["engineIdentity"]){return Err("UPGRADE_IDENTITY".into())}
        let mut identity=manifest["engineIdentity"].as_str().ok_or("UPGRADE_IDENTITY")?.to_owned();let mut previous=Value::String(String::new());let mut bytes=0;let mut sequence=0;
        for entry in &entries {
            verify_value(entry.clone())?;
            let head=verify_value(entry["oldHead"].clone())?;let report=verify_value(entry["oldReportEnvelope"].clone())?;
            let n=entry["journalPrefix"]["bytes"].as_u64().ok_or("UPGRADE_PREFIX_BYTES")?;let seq=head["sequence"].as_u64().ok_or("UPGRADE_PREFIX_SEQUENCE")?;
            if entry["schemaVersion"]!=1||entry["manifestHash"]!=manifest["hash"]||entry["fromEngineIdentity"]!=identity||!digest_text(&entry["toEngineIdentity"])||entry["fromEngineIdentity"]==entry["toEngineIdentity"]||entry["previousUpgradeHash"]!=previous||head["manifestHash"]!=manifest["hash"]||report["manifestHash"]!=manifest["hash"]||report["sequence"]!=head["sequence"]||report["previous"]!=head["previous"]||!digest_text(&entry["journalPrefix"]["sha256"])||n<bytes||seq<sequence||seq>head_sequence||entry["approval"]["role"]!="human_authorized_execution_owner"||entry["approval"]["previousProcessStopped"]!=true||!digest_text(&entry["approval"]["oldBinarySha256"]){return Err("UPGRADE_BINDING".into())}
            chrono::DateTime::parse_from_rfc3339(entry["approval"]["approvedAt"].as_str().ok_or("UPGRADE_APPROVAL_CLOCK")?)?;
            identity=entry["toEngineIdentity"].as_str().ok_or("UPGRADE_IDENTITY")?.into();previous=entry["hash"].clone();bytes=n;sequence=seq;
        }
        if identity!=target{return Err("UPGRADE_TARGET_IDENTITY".into())}
        Ok(Self{entries,checked:0})
    }
    fn check_prefix(&mut self,bytes:u64,digest:&Sha256,chain:&Journal,engine:&Engine,cursors:&BTreeMap<String,u64>)->Result<()>{
        while let Some(entry)=self.entries.get(self.checked){
            let end=entry["journalPrefix"]["bytes"].as_u64().ok_or("UPGRADE_PREFIX_BYTES")?;
            if bytes<end{return Ok(())}if bytes>end{return Err("UPGRADE_PREFIX_RECORD_BOUNDARY".into())}
            let head=&entry["oldHead"];let report=&entry["oldReportEnvelope"];
            if head["sequence"].as_u64()!=Some(chain.sequence)||head["previous"].as_str()!=Some(chain.previous.as_str())||entry["journalPrefix"]["sha256"]!=format!("{:x}",digest.clone().finalize())||report["report"]!=engine.report()?||report["orders"]!=json!(engine.order_records())||report["fills"]!=json!(engine.fills())||report["sourceCursors"]!=json!(cursors)||report["contestOrders"].as_u64()!=Some(engine.live_order_bindings().len() as u64){return Err("UPGRADE_PREFIX_PARITY".into())}
            self.checked+=1;
        }Ok(())
    }
    fn complete(&self)->Result<()>{if self.checked!=self.entries.len(){return Err("UPGRADE_PREFIX_NOT_FOUND".into())}Ok(())}
}
/// Prepare, but do not approve or write, the next explicit identity bridge.
/// Caller must verify the pinned old binary/process and execution dispatch state.
/// The bank's cooperative writer lock is acquired for this immutable snapshot.
pub fn propose_live_engine_upgrade(root:&Path,old_binary_sha256:&str,approved_at:&str)->Result<Value>{
    if !digest_text(&json!(old_binary_sha256)){return Err("UPGRADE_BINARY_HASH".into())}
    chrono::DateTime::parse_from_rfc3339(approved_at)?;
    if fs::symlink_metadata(root)?.file_type().is_symlink(){return Err("PAPER_DIRECTORY_SYMLINK".into())}
    let _lock=ExclusiveLock::acquire(&root.join("writer.lock"))?;
    let manifest=verified(&root.join("manifest.json"))?;let head=verified(&root.join("head.json"))?;let report=verified(&root.join("report.json"))?;
    if manifest["mode"]!="rust_live_session"||head["manifestHash"]!=manifest["hash"]||report["manifestHash"]!=manifest["hash"]||head["sequence"]!=report["sequence"]||head["previous"]!=report["previous"]{return Err("UPGRADE_SOURCE_SNAPSHOT".into())}
    let mut entries=Vec::new();let mut from=manifest["engineIdentity"].as_str().ok_or("UPGRADE_SOURCE_IDENTITY")?.to_owned();let mut previous=json!("");
    if root.join("engine-upgrade-v1.json").exists(){let document=upgrade_document(root)?;from=document["upgrades"].as_array().and_then(|xs|xs.last()).and_then(|last|last["toEngineIdentity"].as_str()).ok_or("UPGRADE_CHAIN_BOUND")?.into();let prior=UpgradeChain::load(root,&manifest,&from,head["sequence"].as_u64().ok_or("UPGRADE_PREFIX_SEQUENCE")?)?;entries=prior.entries;previous=entries.last().ok_or("UPGRADE_CHAIN_BOUND")?["hash"].clone();}
    if entries.len()>=16||from==engine_identity(){return Err("UPGRADE_CHAIN_BOUND_OR_NO_CHANGE".into())}
    let mut input=BufReader::new(File::open(root.join("events.jsonl"))?);let mut line=Vec::new();let mut bytes=0u64;let mut digest=Sha256::new();let mut chain=Journal::default();
    while input.read_until(b'\n',&mut line)?>0{if !line.ends_with(b"\n")||line.len()>16*1024*1024{return Err("UPGRADE_SOURCE_TAIL".into())}bytes=bytes.checked_add(line.len() as u64).ok_or("UPGRADE_PREFIX_BYTES")?;digest.update(&line);chain.accept(&serde_json::from_slice(&line)?)?;line.clear();}
    if head["sequence"].as_u64()!=Some(chain.sequence)||head["previous"].as_str()!=Some(chain.previous.as_str()){return Err("UPGRADE_SOURCE_HEAD".into())}
    entries.push(sealed(json!({"schemaVersion":1,"manifestHash":manifest["hash"],"fromEngineIdentity":from,"toEngineIdentity":engine_identity(),"previousUpgradeHash":previous,"approval":{"role":"human_authorized_execution_owner","previousProcessStopped":true,"oldBinarySha256":old_binary_sha256,"approvedAt":approved_at},"journalPrefix":{"bytes":bytes,"sha256":format!("{:x}",digest.finalize())},"oldHead":head,"oldReportEnvelope":report})));
    Ok(sealed(json!({"schemaVersion":1,"kind":"moloch_live_engine_upgrade_chain_v1","manifestHash":manifest["hash"],"upgrades":entries})))
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
/// Admission bounds include duplicates. Bytes bound serialized raw inputs and,
/// separately, the final journal append; existing engine/history memory is not
/// claimed to fit in this budget.
pub const SESSION_BATCH_MAX_EVENTS:usize=32;
pub const SESSION_BATCH_MAX_BYTES:usize=8*1024*1024;
pub struct BatchEntry<'a>{pub source_id:&'a str,pub sequence:u64,pub event:&'a Value}
#[derive(Debug)]
pub struct BatchOutcome{pub accepted:Vec<bool>,pub appended_rows:usize,pub encoded_journal_bytes:usize}
struct BatchJson{bytes:Vec<u8>,maximum:usize}
impl Write for BatchJson{
    fn write(&mut self,bytes:&[u8])->std::io::Result<usize>{
        if bytes.len()>self.maximum.saturating_sub(self.bytes.len()){return Err(std::io::Error::other("PAPER_BATCH_BYTE_LIMIT"))}
        self.bytes.extend_from_slice(bytes);Ok(bytes.len())
    }
    fn flush(&mut self)->std::io::Result<()>{Ok(())}
}
fn batch_shape(event:&Value)->Result<()>{
    let mut stack=vec![(event,0usize)];let mut nodes=0usize;
    while let Some((value,depth))=stack.pop(){
        nodes+=1;if nodes>262144||depth>64{return Err("PAPER_BATCH_SHAPE_LIMIT".into())}
        match value{
            Value::Array(xs)=>{if xs.len()>262144usize.saturating_sub(nodes+stack.len()){return Err("PAPER_BATCH_SHAPE_LIMIT".into())}stack.extend(xs.iter().map(|v|(v,depth+1)));},
            Value::Object(xs)=>{if xs.len()>262144usize.saturating_sub(nodes+stack.len()){return Err("PAPER_BATCH_SHAPE_LIMIT".into())}stack.extend(xs.values().map(|v|(v,depth+1)));},
            _=>{},
        }
    }Ok(())
}
impl Session {
    pub fn open(root: &Path, seed: Option<&Path>, policy: &Value) -> Result<Self> {
        Self::open_mode(root,seed,policy,None)
    }
    /// A live bank starts from an actual-account allocation, never a paper seed.
    pub fn open_live(root:&Path,policy:&Value,account_seed:&Value)->Result<Self>{
        let engine=Engine::new_live(policy)?;
        let available=crate::amount(account_seed["availableCash"].as_str().ok_or("LIVE_SEED_CASH")?)?;
        if available<engine.wallet.initial || account_seed["runId"].as_str().is_none_or(|s|s.is_empty()||s.len()>128) || account_seed["accountSnapshotHash"].as_str().is_none_or(|s|s.len()!=64||!s.bytes().all(|b|b.is_ascii_hexdigit())) {return Err("LIVE_SEED_EVIDENCE".into())}
        chrono::DateTime::parse_from_rfc3339(account_seed["observedAt"].as_str().ok_or("LIVE_SEED_CLOCK")?)?;
        Self::open_mode(root,None,policy,Some(account_seed))
    }
    fn open_mode(root:&Path,seed:Option<&Path>,policy:&Value,live_seed:Option<&Value>)->Result<Self>{
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
        let (mut engine, seed_identity) = if let Some(account_seed)=live_seed {
            (Engine::new_live(policy)?,json!({"kind":"actual_account_allocation","evidence":account_seed}))
        } else if let Some(path) = seed {
            let (engine, snapshot, digest) = crate::legacy::load_snapshot(path)?;
            (
                engine,
                json!({"kind":"legacy","snapshotHash":snapshot["hash"],"fileSha256":digest}),
            )
        } else {
            (Engine::new(policy)?, json!({"kind":"independent_paper"}))
        };
        engine.update_policy(policy)?;
        let expected = json!({"schemaVersion":1,"mode":if live_seed.is_some(){"rust_live_session"}else{"rust_paper_session"},"seed":seed_identity,"policy":engine.policy(),"engineIdentity":engine_identity()});
        let manifest_path = root.join("manifest.json");
        let journal_path = root.join("events.jsonl");
        let head_path = root.join("head.json");
        let manifest = if existed {
            let m = verified(&manifest_path)?;
            for k in ["schemaVersion", "mode", "seed", "policy"] {
                if m[k] != expected[k] {
                    return Err("PAPER_IDENTITY_CHANGED".into());
                }
            }
            m
        } else {
            let m = sealed(expected.clone());
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
        let mut upgrade = if manifest["engineIdentity"] != expected["engineIdentity"] {
            if live_seed.is_none(){return Err("PAPER_IDENTITY_CHANGED".into())}
            Some(UpgradeChain::load(root,&manifest,&engine_identity(),head_sequence)?)
        } else { None };
        let mut chain = Journal::default();
        let mut prefix_bytes=0u64;let mut prefix_digest=Sha256::new();
        let mut refs = BTreeMap::new();
        let mut cursors = BTreeMap::new();
        let mut facts = BTreeMap::new();
        let mut input = BufReader::new(File::open(&journal_path)?);
        let mut line = Vec::new();
        if let Some(proof)=&mut upgrade{proof.check_prefix(prefix_bytes,&prefix_digest,&chain,&engine,&cursors)?;}
        while input.read_until(b'\n', &mut line)? > 0 {
            if !line.ends_with(b"\n") {
                return Err("PAPER_PARTIAL_JOURNAL_TAIL".into());
            }
            if line.len() > 16 * 1024 * 1024 {
                return Err("PAPER_ROW_LIMIT".into());
            }
            prefix_bytes=prefix_bytes.checked_add(line.len() as u64).ok_or("UPGRADE_PREFIX_BYTES")?;prefix_digest.update(&line);
            let row: Value = serde_json::from_slice(&line)?;
            let event = chain.accept(&row)?;
            Self::restore_ref(&event, &mut refs, &mut cursors)?;
            engine.advance(&event)?;
            Self::remember(&event, &mut facts);
            if let Some(proof)=&mut upgrade{proof.check_prefix(prefix_bytes,&prefix_digest,&chain,&engine,&cursors)?;}
            if chain.sequence == head_sequence && chain.previous != head_previous {
                return Err("PAPER_HEAD_CHAIN".into());
            }
            line.clear();
        }
        if let Some(proof)=&upgrade{proof.complete()?;}
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
                json!({"manifestHash":self.manifest["hash"],"sequence":self.chain.sequence,"previous":self.chain.previous,"report":self.engine.report()?,"orders":self.engine.order_records(),"fills":self.engine.fills(),"sourceCursors":self.cursors,"contestOrders":self.engine.live_order_bindings().len()}),
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
    /// Validate the whole bounded candidate before I/O. Every accepted event
    /// keeps its original individual row/hash/cursor semantics; no batch marker.
    /// Success follows journal+head+report durability. Any I/O failure poisons.
    /// A crash can recover a COMPLETE SERIAL PREFIX, not an atomic whole batch.
    pub fn accept_batch(&mut self,entries:&[BatchEntry<'_>])->Result<BatchOutcome>{
        if self.poisoned{return Err("PAPER_SESSION_POISONED".into())}
        if entries.len()>SESSION_BATCH_MAX_EVENTS{return Err("PAPER_BATCH_EVENT_LIMIT".into())}
        let mut accepted=Vec::with_capacity(entries.len());let mut rows=Vec::new();
        let mut staged_refs:BTreeMap<(String,u64),String>=BTreeMap::new();
        let mut staged_cursors:BTreeMap<String,u64>=BTreeMap::new();
        let mut staged_events=Vec::new();let mut next=None;let mut chain=self.chain.clone();let mut input_bytes=0usize;
        for input in entries{
            let(source_id,sequence,event)=(input.source_id,input.sequence,input.event);
            if !event.is_object(){return Err("PAPER_EVENT".into())}
            if event.get("sourceRef").is_some(){return Err("PAPER_SOURCE_REF_RESERVED".into())}
            if source_id.is_empty()||source_id.len()>256||source_id.contains(['\r','\n'])||sequence==0{return Err("PAPER_SOURCE_ID".into())}
            batch_shape(event)?;
            let mut raw=BatchJson{bytes:Vec::new(),maximum:SESSION_BATCH_MAX_BYTES-input_bytes};
            serde_json::to_writer(&mut raw,event)?;input_bytes+=raw.bytes.len();
            let digest=format!("{:x}",Sha256::digest(&raw.bytes));drop(raw);
            let key=(source_id.to_owned(),sequence);
            if let Some(old)=staged_refs.get(&key).or_else(||self.refs.get(&key)){
                if old!=&digest{return Err("PAPER_SOURCE_PREFIX_CHANGED".into())}
                accepted.push(false);continue;
            }
            let previous=staged_cursors.get(source_id).or_else(||self.cursors.get(source_id)).copied().unwrap_or(0);
            if previous.checked_add(1)!=Some(sequence){return Err("PAPER_SOURCE_SEQUENCE_GAP".into())}
            let mut event=event.clone();event["sourceRef"]=json!({"id":source_id,"sequence":sequence,"hash":digest});
            next.get_or_insert_with(||self.engine.clone()).advance(&event)?;
            let number=chain.sequence.checked_add(1).ok_or("PAPER_BATCH_SEQUENCE_RANGE")?;
            let row=sealed(json!({"sequence":number,"previous":chain.previous,"event":event}));
            chain.accept(&row)?;
            let mut encoded=BatchJson{bytes:rows,maximum:SESSION_BATCH_MAX_BYTES};
            serde_json::to_writer(&mut encoded,&row)?;encoded.write_all(b"\n")?;rows=encoded.bytes;
            staged_refs.insert(key,digest);staged_cursors.insert(source_id.to_owned(),sequence);staged_events.push(event);accepted.push(true);
        }
        let Some(next)=next else{return Ok(BatchOutcome{accepted,appended_rows:0,encoded_journal_bytes:0})};
        // Render/hash all fallible candidate projections BEFORE the first write.
        let head=sealed(json!({"manifestHash":self.manifest["hash"],"sequence":chain.sequence,"previous":chain.previous}));
        let mut cursors=self.cursors.clone();cursors.extend(staged_cursors.clone());
        let report=sealed(json!({"manifestHash":self.manifest["hash"],"sequence":chain.sequence,"previous":chain.previous,"report":next.report()?,"orders":next.order_records(),"fills":next.fills(),"sourceCursors":cursors,"contestOrders":next.live_order_bindings().len()}));
        serde_json::to_writer(std::io::sink(),&head)?;serde_json::to_writer(std::io::sink(),&report)?;
        self.poisoned=true;
        self.file.write_all(&rows)?;self.file.sync_all()?;
        atomic_json(&self.root.join("head.json"),&head)?;
        atomic_json(&self.root.join("report.json"),&report)?;
        self.engine=next;self.chain=chain;self.refs.extend(staged_refs);self.cursors.extend(staged_cursors);
        for event in &staged_events{Self::remember(event,&mut self.facts)}
        self.poisoned=false;
        Ok(BatchOutcome{accepted,appended_rows:staged_events.len(),encoded_journal_bytes:rows.len()})
    }
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
    pub fn live_account_seed(&self)->Option<&Value>{
        if self.manifest["mode"]=="rust_live_session"{Some(&self.manifest["seed"]["evidence"])}else{None}
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
