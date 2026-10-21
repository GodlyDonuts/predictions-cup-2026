//! Supabase Phoenix v1 transport. Credentials remain in memory; error messages
//! never include URLs, request headers, join payloads or server error bodies.
use crate::Result;
use serde_json::{Value, json};
use sha2::Digest;
use std::{
    net::{TcpStream, ToSocketAddrs},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::TrySendError,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};
#[path="realtime_queue.rs"]mod ingress;
use ingress::{Receiver,Sender as SyncSender};
const INGRESS_MESSAGES:usize=1024;
const INGRESS_ENCODED_BYTES:usize=64*1024*1024;
fn ingress_channel(count:usize)->(SyncSender<Incoming>,Receiver<Incoming>){ingress::channel(count,INGRESS_ENCODED_BYTES,incoming_weight)}
fn incoming_weight(value:&Incoming)->usize{
    struct Count(usize);
    impl std::io::Write for Count{
        fn write(&mut self,bytes:&[u8])->std::io::Result<usize>{self.0=self.0.checked_add(bytes.len()).ok_or_else(||std::io::Error::other("encoded size overflow"))?;Ok(bytes.len())}
        fn flush(&mut self)->std::io::Result<()>{Ok(())}
    }
    match value{
        Incoming::Joined{received_at}=>256+received_at.len(),Incoming::Failed(code)=>256+code.len(),
        Incoming::Broadcast{event,payload,received_at}=>{let mut count=Count(256+event.len()+received_at.len());if serde_json::to_writer(&mut count,payload).is_err(){usize::MAX}else{count.0}},
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis() as u64
}
fn iso() -> String {
    chrono::DateTime::from_timestamp_millis(now() as i64)
        .expect("clock")
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
pub struct Auth {
    url: String,
    key: String,
    token: String,
    pub deadline_ms: u64,
    user_channel: Option<String>,
}
impl Auth {
    pub fn parse(v: Value) -> Result<Self> {
        Self::parse_response(v,true)
    }
    /// Only for an authenticated response from the fixed official API endpoint.
    /// SIG also supplies custom Realtime origins; do not use for user input.
    pub fn from_official_response(v:Value)->Result<Self>{
        Self::parse_response(v,false)
    }
    fn parse_response(v: Value,supabase_only:bool) -> Result<Self> {
        let text = |k: &str| {
            v[k].as_str()
                .filter(|s| !s.is_empty() && s.len() <= 32768 && !s.contains(['\r', '\n']))
                .ok_or("STREAM_AUTH")
        };
        let u = reqwest::Url::parse(text("supabaseUrl")?).map_err(|_| "STREAM_AUTH_URL")?;
        let host = u.host_str().ok_or("STREAM_AUTH_URL")?;
        if u.scheme() != "https"
            || supabase_only&&(!host.ends_with(".supabase.co")||host.trim_end_matches(".supabase.co").contains('.'))
            || !host.contains('.')||host.parse::<std::net::IpAddr>().is_ok()
            || ["localhost",".localhost",".local",".internal"].iter().any(|s|host==*s||host.ends_with(s))
            || u.port().is_some()
            || !u.username().is_empty()
            || u.password().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
            || supabase_only&&u.path() != "/"
        {
            return Err("STREAM_AUTH_URL");
        }
        let expiry = chrono::DateTime::parse_from_rfc3339(text("expiresAt")?)
            .map_err(|_| "STREAM_AUTH_EXPIRY")?
            .timestamp_millis();
        if expiry <= now() as i64 + 60000 {
            return Err("STREAM_AUTH_EXPIRY");
        }
        Ok(Self {
            user_channel: v["channels"]["user"].as_str().map(str::to_owned),
            url: u.to_string(),
            key: text("anonKey")?.into(),
            token: text("token")?.into(),
            deadline_ms: (expiry as u64 - 60000).min(now() + 30 * 60000),
        })
    }
}
/// Coalesced invalidation only. No account quantities are credited or adopted.
#[derive(Default)]
pub struct AccountSignal {
    generation:AtomicU64, last_received_ms:AtomicU64, reconciled:AtomicU64,
    dirty:Mutex<std::collections::BTreeSet<String>>,
}
impl AccountSignal {
    /// A local observation failure can only invalidate, never validate, facts.
    pub fn invalidate_for_resync(&self)->Result<()>{self.invalidate(&Default::default(),now())}
    pub fn generation(&self)->u64{self.generation.load(Ordering::SeqCst)}
    pub fn last_received_ms(&self)->u64{self.last_received_ms.load(Ordering::SeqCst)}
    pub fn requires_reconciliation(&self)->bool{self.generation()!=self.reconciled.load(Ordering::SeqCst)}
    /// A change arriving DURING the REST cycle remains dirty for the next cycle.
    pub fn reconciled_through(&self,generation:u64){let mut dirty=self.dirty.lock().unwrap();let current=self.generation();self.reconciled.fetch_max(generation.min(current),Ordering::SeqCst);if generation==current{dirty.clear();}}
    pub fn dirty_exchanges(&self)->Vec<String>{self.dirty.lock().unwrap().iter().cloned().collect()}
    fn invalidate(&self,exchanges:&std::collections::BTreeSet<String>,received:u64)->Result<()>{
        let mut dirty=self.dirty.lock().map_err(|_|"STREAM_ACCOUNT_LOCK")?;dirty.extend(exchanges.iter().cloned());
        self.last_received_ms.fetch_max(received,Ordering::SeqCst);
        self.generation.fetch_update(Ordering::SeqCst,Ordering::SeqCst,|n|n.checked_add(1)).map_err(|_|"STREAM_ACCOUNT_GENERATION")?;Ok(())
    }
}
/// Caller supplies the existing scope and a PRIVATE original-broadcast sink.
/// No credential/join/error frame is ever sent to this sink.
pub struct AccountOptions {
    pub exchanges:std::collections::BTreeMap<String,String>,
    pub capture:Box<dyn FnMut(&str,&str,&Value,&str)->Result<()>+Send>,
}
fn validate_account_options(channel:&str,options:&AccountOptions)->Result<()>{
    let profile=channel.strip_prefix("user:").ok_or("STREAM_ACCOUNT_AUTH")?;
    // The official response supplies this exact authorized profile topic.
    // Its schema says string, not UUID; do not invent a UUID-only requirement.
    if profile.is_empty()||profile.len()>128||!profile.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_')
        ||options.exchanges.len()>1000||options.exchanges.iter().any(|(ex,m)|[ex,m].iter().any(|s|s.is_empty()||s.starts_with('0')||!s.bytes().all(|b|b.is_ascii_digit()))){return Err("STREAM_ACCOUNT_AUTH")}
    Ok(())
}
struct AccountTracker {
    tournament:String, topic:String, options:AccountOptions, signal:Arc<AccountSignal>,
    joined:bool, revision:Option<u64>, revisions:std::collections::BTreeMap<u64,String>, standalone:std::collections::BTreeSet<String>,
}
fn account_integer(v:&Value)->Result<u64>{v.as_u64().or_else(||v.as_str().and_then(|s|s.parse().ok())).ok_or("STREAM_ACCOUNT_REVISION")}
impl AccountTracker {
    fn observe(&mut self,event:&str,payload:&Value,raw:&str,received:&str)->Result<()>{
        let received_ms=chrono::DateTime::parse_from_rfc3339(received).map_err(|_|"STREAM_ACCOUNT_CLOCK")?.timestamp_millis();
        let received_ms=u64::try_from(received_ms).map_err(|_|"STREAM_ACCOUNT_CLOCK")?;
        let mut scopes=std::collections::BTreeSet::new();let mut global=false;let mut duplicate=false;
        let mut rows=Vec::new();
        if event=="account_batch"{
            let d=&payload["delivery"];let revision=account_integer(&d["revision"])?;let previous=account_integer(&d["previousRevision"])?;
            if d["model"]!="best-effort-authoritative-resync"||revision==0||revision<=previous
                ||d["correlationId"].as_str().is_none_or(|s|s.len()>256)
                ||payload.get("resyncRequired").is_some_and(|v|!v.is_boolean()){return Err("STREAM_ACCOUNT_DELIVERY")}
            if d["sourceSequenceFrom"].is_null()!=d["sourceSequenceThrough"].is_null(){return Err("STREAM_ACCOUNT_SEQUENCE")}
            if !d["sourceSequenceFrom"].is_null()&&account_integer(&d["sourceSequenceFrom"])? >account_integer(&d["sourceSequenceThrough"])?{return Err("STREAM_ACCOUNT_SEQUENCE")}
            let digest=format!("{:x}",sha2::Sha256::digest(serde_json::to_vec(payload).map_err(|_|"STREAM_ACCOUNT_DELIVERY")?));
            duplicate=self.revision.is_some_and(|last|revision<=last);
            if duplicate{
                // A revision number alone cannot authenticate an economic duplicate.
                // Capture the original conflicting frame, then fail closed. An old
                // revision outside bounded retained identity is equally unverified.
                if self.revisions.get(&revision)!=Some(&digest){
                    // Invalidate BEFORE potentially blocking capture IO so a
                    // concurrent physical guard cannot retain its old stamp.
                    self.signal.invalidate(&self.options.exchanges.keys().cloned().collect(),received_ms)?;
                    (self.options.capture)(event,raw,payload,received)?;
                    return Err(if self.revisions.contains_key(&revision){"STREAM_ACCOUNT_REVISION_CONFLICT"}else{"STREAM_ACCOUNT_REVISION_UNVERIFIABLE"});
                }
            }else{
                self.revisions.insert(revision,digest);
                if self.revisions.len()>4096{self.revisions.pop_first();}
            }
            global=!duplicate&&(payload["resyncRequired"]==true||self.revision.is_none_or(|last|previous>last));
            if !duplicate{self.revision=Some(revision);}
            if payload["resyncRequired"]!=true{
                for field in ["fills","orderUpdates","settlements","refunds","collateralChanges"]{
                    let array=payload[field].as_array().filter(|a|a.len()<=10000).ok_or("STREAM_ACCOUNT_ARRAY")?;
                    rows.extend(array.iter().map(|row|(field,row)));
                }
            }
        }else if matches!(event,"position_settled"|"refund"){
            let digest=format!("{:x}",sha2::Sha256::digest(format!("{event}:{}",payload).as_bytes()));
            duplicate=self.standalone.contains(&digest);
            if !duplicate{if self.standalone.len()>=4096{return Err("STREAM_ACCOUNT_STANDALONE_BOUND")};self.standalone.insert(digest);}
            rows.push((event,payload));
        }else{return Err("STREAM_ACCOUNT_EVENT")}
        for(field,row)in rows{
            if !row.is_object(){return Err("STREAM_ACCOUNT_ROW")}
            // A user topic spans tournaments. Never adopt foreign account rows.
            if row["tournamentId"].as_str()!=Some(self.tournament.as_str()){
                if row["tournamentId"].is_null(){global=true;}continue
            }
            if field=="collateralChanges"{global=true;continue}
            if let Some(ex)=row["exchangeId"].as_str(){
                if let Some(market)=self.options.exchanges.get(ex){
                    if row["marketId"]!=market.as_str(){return Err("STREAM_ACCOUNT_SCOPE")}
                    scopes.insert(ex.into());
                }else{global=true;}
            }else{global=true;}
        }
        if !duplicate&&(global||!scopes.is_empty()){
            if global{scopes.extend(self.options.exchanges.keys().cloned());}
            self.signal.invalidate(&scopes,received_ms)?;
        }
        (self.options.capture)(event,raw,payload,received)?;Ok(())
    }
}
pub enum Incoming {
    Joined {
        received_at: String,
    },
    Broadcast {
        event: String,
        payload: Value,
        received_at: String,
    },
    Failed(&'static str),
}
/// Only these fixed internal codes may cross the diagnostic boundary. Never
/// retain a socket error, URL, credential, server payload, or panic payload.
fn sanitized_exit(code: &'static str) -> &'static str {
    match code {
        "STREAM_ACCOUNT_REVISION_CONFLICT"|"STREAM_ACCOUNT_REVISION_UNVERIFIABLE"|"STREAM_ACCOUNT_AUTH"|"STREAM_ACCOUNT_REVISION"|"STREAM_ACCOUNT_DELIVERY"|"STREAM_ACCOUNT_SEQUENCE"|"STREAM_ACCOUNT_ARRAY"|"STREAM_ACCOUNT_ROW"|"STREAM_ACCOUNT_SCOPE"|"STREAM_ACCOUNT_EVENT"|"STREAM_ACCOUNT_CAPTURE"|"STREAM_ACCOUNT_GENERATION"|"STREAM_ACCOUNT_LOCK"|"STREAM_ACCOUNT_CLOCK"|"STREAM_ACCOUNT_STANDALONE_BOUND"|"STREAM_FRAME" | "STREAM_JOIN_REJECTED" | "STREAM_HEARTBEAT_REJECTED"
        | "STREAM_REPLY_REF" | "STREAM_TOPIC" | "STREAM_BEFORE_JOIN"
        | "STREAM_BROADCAST" | "STREAM_SYSTEM_REJECTED" | "STREAM_CHANNEL_CLOSED"
        | "STREAM_EVENT" | "STREAM_HEARTBEAT_PENDING" | "STREAM_JOIN_TIMEOUT"
        | "STREAM_HEARTBEAT_TIMEOUT" | "STREAM_HEARTBEAT_SEND" | "STREAM_JSON"
        | "STREAM_QUEUE_FULL" | "STREAM_CONSUMER_DISCONNECTED" | "STREAM_SOCKET_CLOSED"
        | "STREAM_NON_TEXT" | "STREAM_READ_FAILED" | "STREAM_FLUSH"
        | "STREAM_DEADLINE_REACHED" | "STREAM_STOP_REQUESTED"
        | "STREAM_WORKER_PANICKED" | "STREAM_WORKER_EXIT_UNEXPLAINED" => code,
        _ => "STREAM_WORKER_EXIT_UNEXPLAINED",
    }
}
struct ExitReporter {
    reason: Arc<Mutex<Option<&'static str>>>,
    failed: Arc<AtomicBool>,
    published: bool,
}
impl ExitReporter {
    fn publish(&mut self, code: &'static str) {
        {
            // A poisoned diagnostic mutex must never suppress the failed gate.
            let mut reason=self.reason.lock().unwrap_or_else(|p|p.into_inner());
            if reason.is_none(){*reason=Some(sanitized_exit(code));}
        }
        self.failed.store(true, Ordering::SeqCst);
        self.published=true;
    }
}
impl Drop for ExitReporter {
    fn drop(&mut self) {
        if !self.published {
            self.publish(if thread::panicking(){"STREAM_WORKER_PANICKED"}else{"STREAM_WORKER_EXIT_UNEXPLAINED"});
        }
    }
}
fn deliver(tx:&SyncSender<Incoming>, incoming:Incoming)->Result<()> {
    tx.try_send(incoming).map_err(|e|match e {
        TrySendError::Full(_)=>"STREAM_QUEUE_FULL",
        TrySendError::Disconnected(_)=>"STREAM_CONSUMER_DISCONNECTED",
    })
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum PlannedExit { RequestedStop, Deadline }
impl PlannedExit {
    fn code(self)->&'static str{match self{Self::RequestedStop=>"STREAM_STOP_REQUESTED",Self::Deadline=>"STREAM_DEADLINE_REACHED"}}
}
struct Protocol {
    topic: String,
    joined: bool,
    heartbeat: Option<(String, Instant)>,
    next_ref: u64,
    account:Option<AccountTracker>,
}
impl Protocol {
    fn new(tournament: &str) -> Result<Self> {
        if tournament.len() != 36
            || !tournament
                .bytes()
                .all(|c| c.is_ascii_hexdigit() || c == b'-')
        {
            return Err("STREAM_TOURNAMENT");
        }
        Ok(Self {
            topic: format!("realtime:tournament:{tournament}:markets"),
            joined: false,
            heartbeat: None,
            next_ref: 2,
            account:None,
        })
    }
    fn join(&self, token: &str) -> Value {
        json!({"topic":self.topic,"event":"phx_join","payload":{"config":{"broadcast":{"ack":false,"self":false},"presence":{"enabled":false},"postgres_changes":[],"private":true},"access_token":token},"ref":"1","join_ref":"1"})
    }
    fn accept(&mut self, v: &Value) -> Result<Option<Incoming>> {self.accept_original(v,&v.to_string())}
    fn accept_original(&mut self, v:&Value, raw:&str)->Result<Option<Incoming>> {
        let event = v["event"].as_str().ok_or("STREAM_FRAME")?;
        let topic = v["topic"].as_str().ok_or("STREAM_FRAME")?;
        if event == "phx_reply" {
            if topic == self.topic && v["ref"] == "1" {
                if self.joined || v["payload"]["status"] != "ok" {
                    return Err("STREAM_JOIN_REJECTED");
                }
                self.joined = true;
                if self.account.as_ref().is_none_or(|a|a.joined){return Ok(Some(Incoming::Joined { received_at: iso() }));}
                return Ok(None);
            }
            if let Some(account)=self.account.as_mut(){if topic==account.topic&&v["ref"]=="2"{
                if account.joined||v["payload"]["status"]!="ok"{return Err("STREAM_JOIN_REJECTED")}
                account.joined=true;account.signal.invalidate(&account.options.exchanges.keys().cloned().collect(),now())?;
                return Ok(self.joined.then(||Incoming::Joined{received_at:iso()}));
            }}
            if topic == "phoenix" {
                if let Some((reference, _)) = &self.heartbeat {
                    if v["ref"] == *reference {
                        if v["payload"]["status"] != "ok" {
                            return Err("STREAM_HEARTBEAT_REJECTED");
                        }
                        self.heartbeat = None;
                    }
                }
                return Ok(None);
            }
            return Err("STREAM_REPLY_REF");
        }
        if let Some(account)=self.account.as_mut(){if topic==account.topic{
            return match event{
                "broadcast"=>{if !account.joined{return Err("STREAM_BEFORE_JOIN")}
                    let kind=v["payload"]["event"].as_str().ok_or("STREAM_ACCOUNT_EVENT")?;
                    if kind=="notification"{return Ok(None)}
                    let payload=&v["payload"]["payload"];if !payload.is_object(){return Err("STREAM_ACCOUNT_ROW")}
                    account.observe(kind,payload,raw,&iso())?;Ok(None)},
                "system" if v["payload"]["status"]=="ok"=>Ok(None),
                "phx_close"|"phx_error"=>Err("STREAM_CHANNEL_CLOSED"),_=>Err("STREAM_ACCOUNT_EVENT")
            };
        }}
        if topic != self.topic {
            return Err("STREAM_TOPIC");
        }
        match event {
            "broadcast" => {
                if !self.joined {
                    return Err("STREAM_BEFORE_JOIN");
                }
                let event = v["payload"]["event"].as_str().ok_or("STREAM_BROADCAST")?;
                if !["markets_batch", "book_dirty", "market_settled"].contains(&event) {
                    return Err("STREAM_BROADCAST");
                }
                let payload = v["payload"]["payload"].clone();
                if !payload.is_object() {
                    return Err("STREAM_BROADCAST");
                }
                Ok(Some(Incoming::Broadcast {
                    event: event.into(),
                    payload,
                    received_at: iso(),
                }))
            }
            "system" => {
                if v["payload"]["status"] != "ok" {
                    Err("STREAM_SYSTEM_REJECTED")
                } else {
                    Ok(None)
                }
            }
            "phx_close" | "phx_error" => Err("STREAM_CHANNEL_CLOSED"),
            _ => Err("STREAM_EVENT"),
        }
    }
    fn beat(&mut self) -> Result<Value> {
        if self.heartbeat.is_some() {
            return Err("STREAM_HEARTBEAT_PENDING");
        }
        let reference = self.next_ref.to_string();
        self.next_ref += 1;
        self.heartbeat = Some((reference.clone(), Instant::now()));
        Ok(json!({"topic":"phoenix","event":"heartbeat","payload":{},"ref":reference}))
    }
}
fn tcp(socket: &MaybeTlsStream<TcpStream>) -> Result<&TcpStream> {
    match socket {
        MaybeTlsStream::Plain(s) => Ok(s),
        MaybeTlsStream::Rustls(s) => Ok(&s.sock),
        _ => Err("STREAM_TLS"),
    }
}
pub struct Realtime {
    pub incoming: Receiver<Incoming>,
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    exit_reason: Arc<Mutex<Option<&'static str>>>,
    worker: Option<JoinHandle<()>>,
    pub deadline_ms: u64,
    account:Option<Arc<AccountSignal>>,
}
impl Realtime {
    pub fn connect(auth: Auth, tournament: &str) -> Result<Self> {Self::connect_configured(auth,tournament,None)}
    pub fn connect_with_account(auth:Auth,tournament:&str,options:AccountOptions)->Result<Self>{Self::connect_configured(auth,tournament,Some(options))}
    fn connect_configured(auth:Auth,tournament:&str,options:Option<AccountOptions>)->Result<Self>{
        if let Some(options)=&options{validate_account_options(auth.user_channel.as_deref().unwrap_or_default(),options)?;}
        let mut u = reqwest::Url::parse(&auth.url).map_err(|_| "STREAM_AUTH_URL")?;
        u.set_scheme("wss").map_err(|_| "STREAM_AUTH_URL")?;
        let socket_path=format!("{}/realtime/v1/websocket",u.path().trim_end_matches('/'));
        u.set_path(&socket_path);
        u.query_pairs_mut()
            .append_pair("apikey", &auth.key)
            .append_pair("vsn", "1.0.0");
        let host = u.host_str().ok_or("STREAM_HOST")?;
        let addresses = (host, 443).to_socket_addrs().map_err(|_| "STREAM_DNS")?;
        let mut stream = None;
        for address in addresses.take(4) {
            let public=match address.ip(){
                std::net::IpAddr::V4(ip)=>{let b=ip.octets();!ip.is_private()&&!ip.is_loopback()&&!ip.is_link_local()&&!ip.is_unspecified()&&!ip.is_broadcast()&&!ip.is_multicast()&&!(b[0]==100&&(64..=127).contains(&b[1]))&&b[0]!=0},
                std::net::IpAddr::V6(ip)=>!ip.is_loopback()&&!ip.is_unspecified()&&!ip.is_unique_local()&&!ip.is_unicast_link_local()&&!ip.is_multicast()&&ip.to_ipv4_mapped().is_none()
            };
            if !public{continue}
            if let Ok(s) = TcpStream::connect_timeout(&address, Duration::from_secs(5)) {
                stream = Some(s);
                break;
            }
        }
        let stream = stream.ok_or("STREAM_CONNECT")?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|_| "STREAM_TIMEOUT")?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|_| "STREAM_TIMEOUT")?;
        let config = tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(16 * 1024 * 1024))
            .max_frame_size(Some(16 * 1024 * 1024));
        let (socket, _) =
            tungstenite::client_tls_with_config(u.as_str(), stream, Some(config), None)
                .map_err(|_| "STREAM_HANDSHAKE")?;
        let account=options.map(|options|(auth.user_channel.unwrap_or_default(),options));
        Self::start_configured(socket, auth.token, tournament, auth.deadline_ms,account)
    }
    fn start(
        mut socket: WebSocket<MaybeTlsStream<TcpStream>>,
        token: String,
        tournament: &str,
        deadline_ms: u64,
    ) -> Result<Self> {
        Self::start_configured(socket,token,tournament,deadline_ms,None)
    }
    fn start_configured(mut socket:WebSocket<MaybeTlsStream<TcpStream>>,token:String,tournament:&str,deadline_ms:u64,options:Option<(String,AccountOptions)>)->Result<Self>{
        let mut protocol=Protocol::new(tournament)?;
        let signal=if let Some((channel,options))=options{
            validate_account_options(&channel,&options)?;
            let signal=Arc::new(AccountSignal::default());protocol.next_ref=3;
            protocol.account=Some(AccountTracker{tournament:tournament.into(),topic:format!("realtime:{channel}"),options,signal:signal.clone(),joined:false,revision:None,revisions:Default::default(),standalone:Default::default()});Some(signal)
        }else{None};
        socket
            .send(Message::Text(protocol.join(&token).to_string().into()))
            .map_err(|_| "STREAM_JOIN_SEND")?;
        if let Some(account)=&protocol.account{
            socket.send(Message::Text(json!({"topic":account.topic,"event":"phx_join","payload":{"config":{"broadcast":{"ack":false,"self":false},"presence":{"enabled":false},"postgres_changes":[],"private":true},"access_token":token},"ref":"2","join_ref":"2"}).to_string().into())).map_err(|_|"STREAM_JOIN_SEND")?;
        }
        drop(token);
        tcp(socket.get_ref())?
            .set_read_timeout(Some(Duration::from_millis(100)))
            .map_err(|_| "STREAM_TIMEOUT")?;
        let (tx, incoming) = ingress_channel(INGRESS_MESSAGES);
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let exit_reason=Arc::new(Mutex::new(None));
        let worker_stop = stop.clone();
        let mut reporter=ExitReporter{reason:exit_reason.clone(),failed:failed.clone(),published:false};
        let worker = thread::spawn(move || {
            match run_socket(&mut socket, &mut protocol, &tx, &worker_stop, deadline_ms) {
                Err(code)=>{
                    let code=sanitized_exit(code);
                    reporter.publish(code); // Out-of-band reason precedes the failed flag.
                    let _ = tx.try_send(Incoming::Failed(code)); // Compatibility, never authority.
                },
                Ok(exit)=>reporter.publish(exit.code()),
            }
            let _ = socket.close(None);
        });
        Ok(Self {
            incoming,
            stop,
            failed,
            exit_reason,
            worker: Some(worker),
            deadline_ms,
            account:signal,
        })
    }
    pub fn account_signal(&self)->Option<Arc<AccountSignal>>{self.account.clone()}
    pub fn healthy(&self) -> bool {
        !self.failed.load(Ordering::SeqCst)
            && self.worker.as_ref().is_some_and(|t| !t.is_finished())
    }
    pub fn failure_signal(&self)->Arc<AtomicBool>{self.failed.clone()}
    /// Out-of-band diagnostic remains readable when the incoming queue is full.
    /// None means no exit has been published, not proof of healthy operation.
    pub fn exit_reason(&self)->Option<&'static str>{
        let reason=*self.exit_reason.lock().unwrap_or_else(|p|p.into_inner());
        reason.or_else(||self.worker.as_ref().is_none_or(|w|w.is_finished()).then_some("STREAM_WORKER_EXIT_UNEXPLAINED"))
    }
}
impl Drop for Realtime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn run_socket(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    protocol: &mut Protocol,
    tx: &SyncSender<Incoming>,
    stop: &AtomicBool,
    deadline: u64,
) -> Result<PlannedExit> {
    let began = Instant::now();
    let mut heartbeat = Instant::now();
    while !stop.load(Ordering::SeqCst) && now() < deadline {
        if (!protocol.joined||protocol.account.as_ref().is_some_and(|a|!a.joined)) && began.elapsed() > Duration::from_secs(10) {
            return Err("STREAM_JOIN_TIMEOUT");
        }
        if protocol
            .heartbeat
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(10))
        {
            return Err("STREAM_HEARTBEAT_TIMEOUT");
        }
        if protocol.joined && heartbeat.elapsed() >= Duration::from_secs(25) {
            socket
                .send(Message::Text(protocol.beat()?.to_string().into()))
                .map_err(|_| "STREAM_HEARTBEAT_SEND")?;
            heartbeat = Instant::now();
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                let v: Value = serde_json::from_str(text.as_str()).map_err(|_| "STREAM_JSON")?;
                if let Some(incoming) = protocol.accept_original(&v,text.as_str())? {
                    deliver(tx,incoming)?;
                }
            }
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
            Ok(Message::Close(_)) => return Err("STREAM_SOCKET_CLOSED"),
            Ok(_) => return Err("STREAM_NON_TEXT"),
            Err(tungstenite::Error::Io(e))
                if [std::io::ErrorKind::WouldBlock, std::io::ErrorKind::TimedOut]
                    .contains(&e.kind()) => {}
            Err(_) => return Err("STREAM_READ_FAILED"),
        };
        socket.flush().map_err(|_| "STREAM_FLUSH")?;
    }
    Ok(if stop.load(Ordering::SeqCst){PlannedExit::RequestedStop}else{PlannedExit::Deadline})
}
#[cfg(test)]
mod tests {
    use super::*;
    const CUP: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
    #[test]
    fn complete_universe_burst_preserves_every_frame_and_order(){
        let(tx,rx)=ingress_channel(INGRESS_MESSAGES);
        for sequence in 0..1024 {deliver(&tx,Incoming::Broadcast{event:"orderbook".into(),payload:json!({"sequence":sequence}),received_at:"2026-10-21T00:00:00Z".into()}).unwrap();}
        assert_eq!(deliver(&tx,Incoming::Joined{received_at:"later".into()}),Err("STREAM_QUEUE_FULL"));
        for sequence in 0..1024 {match rx.try_recv().unwrap(){Incoming::Broadcast{payload,..}=>assert_eq!(payload["sequence"],sequence),_=>panic!("changed frame")}}
        deliver(&tx,Incoming::Joined{received_at:"after drain".into()}).unwrap();assert!(matches!(rx.try_recv().unwrap(),Incoming::Joined{..}));
    }
    fn reporter()->(ExitReporter,Arc<Mutex<Option<&'static str>>>,Arc<AtomicBool>){
        let reason=Arc::new(Mutex::new(None));let failed=Arc::new(AtomicBool::new(false));
        (ExitReporter{reason:reason.clone(),failed:failed.clone(),published:false},reason,failed)
    }
    #[test]
    fn saturated_queue_cannot_hide_published_reason_or_weaken_failed_gate(){
        let(tx,rx)=ingress_channel(64);
        for _ in 0..64{deliver(&tx,Incoming::Joined{received_at:"fixture".into()}).unwrap();}
        let error=deliver(&tx,Incoming::Joined{received_at:"overflow".into()}).unwrap_err();
        assert_eq!(error,"STREAM_QUEUE_FULL");
        let(mut reporter,reason,failed)=reporter();reporter.publish(error);
        assert!(tx.try_send(Incoming::Failed(error)).is_err());
        let stream=Realtime{incoming:rx,stop:Arc::new(AtomicBool::new(false)),failed,exit_reason:reason,worker:None,deadline_ms:0,account:None};
        assert_eq!(stream.exit_reason(),Some("STREAM_QUEUE_FULL"));assert!(!stream.healthy());assert!(stream.failure_signal().load(Ordering::SeqCst));
        // Existing queued data was neither consumed to find a reason nor dropped.
        assert_eq!(stream.incoming.try_iter().count(),64);
    }
    #[test]
    fn disconnected_consumer_is_not_queue_capacity_failure(){
        let(tx,rx)=ingress_channel(64);drop(rx);
        assert_eq!(deliver(&tx,Incoming::Joined{received_at:"fixture".into()}).unwrap_err(),"STREAM_CONSUMER_DISCONNECTED");
    }
    #[test]
    fn failed_signal_observer_always_finds_reason_already_published(){
        let(mut reporter,reason,failed)=reporter();let observed=failed.clone();
        let reader=thread::spawn(move||{while !observed.load(Ordering::SeqCst){thread::yield_now();}assert_eq!(*reason.lock().unwrap(),Some("STREAM_HEARTBEAT_TIMEOUT"));});
        reporter.publish("STREAM_HEARTBEAT_TIMEOUT");reader.join().unwrap();
    }
    #[test]
    fn first_reason_survives_later_cleanup_and_reporter_drop(){
        let(mut reporter,reason,failed)=reporter();reporter.publish("STREAM_SOCKET_CLOSED");reporter.publish("STREAM_FLUSH");drop(reporter);
        assert_eq!(*reason.lock().unwrap(),Some("STREAM_SOCKET_CLOSED"));assert!(failed.load(Ordering::SeqCst));
    }
    #[test]
    fn unwind_reports_fixed_panic_code_without_retaining_payload(){
        let(reporter,reason,failed)=reporter();let worker=thread::spawn(move||{let _reporter=reporter;panic!("fixture panic payload");});
        assert!(worker.join().is_err());assert_eq!(*reason.lock().unwrap(),Some("STREAM_WORKER_PANICKED"));assert!(failed.load(Ordering::SeqCst));
    }
    #[test]
    fn unreported_return_is_fixed_unexplained_and_unknown_input_is_sanitized(){
        let(reporter,reason,failed)=reporter();drop(reporter);
        assert_eq!(*reason.lock().unwrap(),Some("STREAM_WORKER_EXIT_UNEXPLAINED"));assert!(failed.load(Ordering::SeqCst));
        assert_eq!(sanitized_exit("https://fixture.invalid/?token=DO_NOT_STORE"),"STREAM_WORKER_EXIT_UNEXPLAINED");
        assert_eq!(sanitized_exit("server body DO_NOT_STORE"),"STREAM_WORKER_EXIT_UNEXPLAINED");
    }
    #[test]
    fn poisoned_diagnostic_mutex_still_publishes_failure(){
        let(mut reporter,reason,failed)=reporter();let poisoned=reason.clone();
        assert!(thread::spawn(move||{let _guard=poisoned.lock().unwrap();panic!("fixture mutex poison");}).join().is_err());
        reporter.publish("STREAM_READ_FAILED");assert!(failed.load(Ordering::SeqCst));assert_eq!(*reason.lock().unwrap_err().into_inner(),Some("STREAM_READ_FAILED"));
    }
    #[test]
    fn planned_deadline_and_requested_stop_have_distinct_socket_results(){
        for requested in [false,true]{
            let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let address=listener.local_addr().unwrap();
            let server=thread::spawn(move||{let(stream,_)=listener.accept().unwrap();let _ws=tungstenite::accept(stream).unwrap();});
            let(mut socket,_)=tungstenite::connect(format!("ws://{address}/fixture")).unwrap();
            let(tx,_rx)=ingress_channel(64);let mut protocol=Protocol::new(CUP).unwrap();let stop=AtomicBool::new(requested);
            let exit=run_socket(&mut socket,&mut protocol,&tx,&stop,if requested{now()+10_000}else{0}).unwrap();
            assert_eq!(exit,if requested{PlannedExit::RequestedStop}else{PlannedExit::Deadline});
            let(mut reporter,reason,failed)=reporter();reporter.publish(exit.code());
            assert_eq!(*reason.lock().unwrap(),Some(if requested{"STREAM_STOP_REQUESTED"}else{"STREAM_DEADLINE_REACHED"}));assert!(failed.load(Ordering::SeqCst));server.join().unwrap();
        }
    }
    #[test]
    fn protocol_requires_ack_and_known_topic() {
        let mut p = Protocol::new(CUP).unwrap();
        let frame = json!({"topic":p.topic,"event":"broadcast","payload":{"event":"markets_batch","payload":{"markets":[]}}});
        assert!(p.accept(&frame).is_err());
        assert!(p.accept(&json!({"topic":p.topic,"event":"phx_reply","ref":"1","payload":{"status":"error","response":{"token":"DO_NOT_LOG"}}})).is_err());
        assert!(matches!(
            p.accept(
                &json!({"topic":p.topic,"event":"phx_reply","ref":"1","payload":{"status":"ok"}})
            )
            .unwrap(),
            Some(Incoming::Joined { .. })
        ));
        assert!(matches!(
            p.accept(&frame).unwrap(),
            Some(Incoming::Broadcast { .. })
        ));
        assert!(p.accept(&json!({"topic":"foreign","event":"broadcast","payload":{"event":"markets_batch","payload":{}}})).is_err());
    }
    #[test]
    fn heartbeat_replies_are_correlated() {
        let mut p = Protocol::new(CUP).unwrap();
        let beat = p.beat().unwrap();
        assert!(p.beat().is_err());
        p.accept(&json!({"topic":"phoenix","event":"phx_reply","ref":"unrelated","payload":{"status":"ok"}})).unwrap();
        assert!(p.heartbeat.is_some());
        p.accept(&json!({"topic":"phoenix","event":"phx_reply","ref":beat["ref"],"payload":{"status":"ok"}})).unwrap();
        assert!(p.heartbeat.is_none())
    }
    #[test]
    fn rejects_arbitrary_auth_destinations_and_expired_tokens() {
        let base = json!({"supabaseUrl":"https://unit-test.supabase.co","anonKey":"DUMMY","token":"DUMMY","expiresAt":"2099-01-01T00:00:00Z"});
        assert!(Auth::parse(base.clone()).is_ok());
        for url in [
            "http://unit-test.supabase.co",
            "https://evil.test",
            "https://supabase.co.evil.test",
            "https://unit-test.supabase.co/path",
            "https://token@unit-test.supabase.co",
        ] {
            let mut v = base.clone();
            v["supabaseUrl"] = json!(url);
            assert!(Auth::parse(v).is_err())
        }
        let mut v = base;
        v["expiresAt"] = json!("2000-01-01T00:00:00Z");
        assert!(Auth::parse(v).is_err())
    }
    #[test]
    fn authenticated_official_custom_origins_are_supported_without_private_routing(){
        let make=|url:&str|json!({"supabaseUrl":url,"anonKey":"DUMMY","token":"DUMMY","expiresAt":"2099-01-01T00:00:00Z"});
        assert!(Auth::from_official_response(make("https://realtime.example.com")).is_ok());
        assert!(Auth::from_official_response(make("https://realtime.example.com/supabase")).is_ok());
        assert!(Auth::parse(make("https://realtime.example.com")).is_err());
        for url in ["http://realtime.example.com","https://127.0.0.1","https://localhost","https://host.local","https://token@realtime.example.com","https://realtime.example.com/?token=secret"]{assert!(Auth::from_official_response(make(url)).is_err())}
    }
    #[test]
    fn native_websocket_receives_join_and_broadcast() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let join: Value = serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(join["payload"]["config"]["private"], true);
            ws.send(Message::Text(json!({"topic":join["topic"],"event":"phx_reply","ref":"1","payload":{"status":"ok"}}).to_string().into())).unwrap();
            ws.send(Message::Text(json!({"topic":join["topic"],"event":"broadcast","payload":{"event":"markets_batch","payload":{"markets":[]}}}).to_string().into())).unwrap();
            thread::sleep(Duration::from_millis(300));
        });
        let (stream, _) = tungstenite::connect(format!("ws://{address}/realtime")).unwrap();
        let r = Realtime::start(stream, "DUMMY_TEST_TOKEN".into(), CUP, now() + 5000).unwrap();
        assert!(matches!(
            r.incoming.recv_timeout(Duration::from_secs(2)).unwrap(),
            Incoming::Joined { .. }
        ));
        assert!(matches!(
            r.incoming.recv_timeout(Duration::from_secs(2)).unwrap(),
            Incoming::Broadcast { .. }
        ));
        drop(r);
        server.join().unwrap();
    }
    fn account_protocol()->(Protocol,Arc<AccountSignal>,Arc<Mutex<Vec<Value>>>){
        let mut p=Protocol::new(CUP).unwrap();let signal=Arc::new(AccountSignal::default());let captured=Arc::new(Mutex::new(Vec::new()));let sink=captured.clone();
        p.account=Some(AccountTracker{tournament:CUP.into(),topic:format!("realtime:user:{CUP}"),joined:false,revision:None,revisions:Default::default(),standalone:Default::default(),signal:signal.clone(),options:AccountOptions{exchanges:[("101".into(),"201".into())].into(),capture:Box::new(move|event,raw,payload,received|{sink.lock().unwrap().push(json!({"event":event,"raw":raw,"payload":payload,"receivedAt":received}));Ok(())})}});p.next_ref=3;(p,signal,captured)
    }
    fn joined_account(p:&mut Protocol){let market=json!({"topic":p.topic,"event":"phx_reply","ref":"1","payload":{"status":"ok"}});assert!(p.accept(&market).unwrap().is_none());let account=json!({"topic":format!("realtime:user:{CUP}"),"event":"phx_reply","ref":"2","payload":{"status":"ok"}});assert!(matches!(p.accept(&account).unwrap(),Some(Incoming::Joined{..})));}
    fn account_batch(revision:u64,previous:u64)->Value{json!({"fills":[],"orderUpdates":[],"settlements":[],"refunds":[],"collateralChanges":[],"delivery":{"model":"best-effort-authoritative-resync","correlationId":"fixture","revision":revision,"previousRevision":previous,"sourceSequenceFrom":null,"sourceSequenceThrough":null}})}
    fn account_message(payload:Value)->Value{json!({"topic":format!("realtime:user:{CUP}"),"event":"broadcast","payload":{"event":"account_batch","payload":payload}})}
    #[test]fn account_join_is_independent_and_market_ready_waits_for_both_topics(){let(mut p,signal,_)=account_protocol();let market=json!({"topic":p.topic,"event":"phx_reply","ref":"1","payload":{"status":"ok"}});assert!(p.accept(&market).unwrap().is_none());assert_eq!(signal.generation(),0);let mut denied=json!({"topic":format!("realtime:user:{CUP}"),"event":"phx_reply","ref":"2","payload":{"status":"error","response":{"token":"NEVER_CAPTURE"}}});assert_eq!(p.accept(&denied).err(),Some("STREAM_JOIN_REJECTED"));denied["payload"]["status"]=json!("ok");assert!(matches!(p.accept(&denied).unwrap(),Some(Incoming::Joined{..})));assert!(signal.requires_reconciliation());}
    #[test]fn actual_resting_fill_invalidates_at_receive_without_market_queue_or_order_update(){let(mut p,signal,captured)=account_protocol();joined_account(&mut p);signal.reconciled_through(signal.generation());let before=signal.generation();let mut batch=account_batch(1,0);batch["fills"]=json!([{"orderId":"901","exchangeId":"101","marketId":"201","price":0.4,"quantity":3,"executedAt":"2026-10-16T22:00:00Z","tournamentId":CUP}]);let v=account_message(batch);assert!(p.accept_original(&v,"ORIGINAL_PUBLIC_ACCOUNT_FRAME").unwrap().is_none());assert_eq!(signal.generation(),before+1);assert!(signal.requires_reconciliation());assert_eq!(captured.lock().unwrap()[0]["raw"],"ORIGINAL_PUBLIC_ACCOUNT_FRAME");assert_eq!(signal.dirty_exchanges(),vec!["101"]);}
    #[test]fn duplicate_account_revision_cannot_renew_generation_or_receipt_clock(){let(mut p,signal,_)=account_protocol();joined_account(&mut p);let v=account_message(account_batch(1,0));p.accept(&v).unwrap();let before=(signal.generation(),signal.last_received_ms());p.accept(&v).unwrap();assert_eq!((signal.generation(),signal.last_received_ms()),before);}
    #[test]fn account_gap_and_compact_resync_invalidate_whole_scope_without_fills(){let(mut p,signal,_)=account_protocol();joined_account(&mut p);p.accept(&account_message(account_batch(1,0))).unwrap();signal.reconciled_through(signal.generation());let before=signal.generation();let mut compact=account_batch(4,3);compact["resyncRequired"]=json!(true);for field in ["fills","orderUpdates","settlements","refunds","collateralChanges"]{compact.as_object_mut().unwrap().remove(field);}p.accept(&account_message(compact)).unwrap();assert_eq!(signal.generation(),before+1);assert_eq!(signal.dirty_exchanges(),vec!["101"]);}
    #[test]fn foreign_tournament_does_not_adopt_or_dirty_and_wrong_current_scope_fails(){let(mut p,signal,_)=account_protocol();joined_account(&mut p);p.accept(&account_message(account_batch(1,0))).unwrap();signal.reconciled_through(signal.generation());let before=signal.generation();let mut b=account_batch(2,1);b["fills"]=json!([{"orderId":"foreign","exchangeId":"101","marketId":"201","tournamentId":"other"}]);p.accept(&account_message(b)).unwrap();assert_eq!(signal.generation(),before);let mut b=account_batch(3,2);b["fills"]=json!([{"orderId":"901","exchangeId":"101","marketId":"999","tournamentId":CUP}]);assert_eq!(p.accept(&account_message(b)).err(),Some("STREAM_ACCOUNT_SCOPE"));}
    #[test]fn nullable_fill_attribution_and_window_zero_order_update_only_invalidate(){let(mut p,signal,_)=account_protocol();joined_account(&mut p);let before=signal.generation();let mut b=account_batch(1,0);b["fills"]=json!([{"orderId":null,"exchangeId":"101","marketId":"201","price":null,"quantity":1,"tournamentId":CUP}]);b["orderUpdates"]=json!([{"orderId":"901","exchangeId":"101","marketId":"201","open":false,"quantityTraded":0,"totalCost":0,"latestTradePrice":null,"tournamentId":CUP}]);p.accept(&account_message(b)).unwrap();assert_eq!(signal.generation(),before+1);}
    #[test]fn reconciliation_completion_cannot_clear_an_event_arriving_during_reads(){let signal=AccountSignal::default();signal.invalidate_for_resync().unwrap();let old=signal.generation();signal.invalidate_for_resync().unwrap();signal.reconciled_through(old);assert!(signal.requires_reconciliation());signal.reconciled_through(signal.generation());assert!(!signal.requires_reconciliation());}
    #[test]fn account_capture_failure_is_fatal_and_cannot_hide_already_received_invalidation(){let(mut p,signal,_)=account_protocol();joined_account(&mut p);let before=signal.generation();p.account.as_mut().unwrap().options.capture=Box::new(|_,_,_,_|Err("STREAM_ACCOUNT_CAPTURE"));assert_eq!(p.accept(&account_message(account_batch(1,0))).err(),Some("STREAM_ACCOUNT_CAPTURE"));assert_eq!(signal.generation(),before+1);}
    #[test]fn standalone_refund_is_deduplicated_without_treating_it_as_cash(){let(mut p,signal,_)=account_protocol();joined_account(&mut p);let v=json!({"topic":format!("realtime:user:{CUP}"),"event":"broadcast","payload":{"event":"refund","payload":{"exchangeId":"101","marketId":"201","shares":4,"refundAmount":1.5,"tournamentId":CUP,"at":"2026-10-16T22:00:00Z"}}});p.accept(&v).unwrap();let before=signal.generation();let mut other_ref=v.clone();other_ref["ref"]=json!("different_transport_ref");p.accept(&other_ref).unwrap();assert_eq!(signal.generation(),before);}
    #[test]fn legacy_default_keeps_single_market_topic_and_rejects_account_frames(){let mut p=Protocol::new(CUP).unwrap();assert!(p.account.is_none());assert_eq!(p.next_ref,2);assert_eq!(p.accept(&account_message(account_batch(1,0))).err(),Some("STREAM_TOPIC"));}

    #[test]fn real_single_socket_account_receipt_invalidates_without_waiting_for_main_drain(){
        use std::net::TcpListener;let listener=TcpListener::bind("127.0.0.1:0").unwrap();let address=listener.local_addr().unwrap();
        let server=thread::spawn(move||{let(socket,_)=listener.accept().unwrap();let mut ws=tungstenite::accept(socket).unwrap();let mut joins=Vec::new();for _ in 0..2{let Message::Text(text)=ws.read().unwrap()else{panic!("join expected")};joins.push(serde_json::from_str::<Value>(&text).unwrap());}
            assert_eq!(joins[0]["ref"],"1");assert_eq!(joins[1]["ref"],"2");assert_ne!(joins[0]["topic"],joins[1]["topic"]);for join in &joins{ws.send(Message::Text(json!({"topic":join["topic"],"event":"phx_reply","ref":join["ref"],"payload":{"status":"ok"}}).to_string().into())).unwrap();}
            let mut batch=account_batch(1,0);batch["fills"]=json!([{"orderId":"901","exchangeId":"101","marketId":"201","price":0.4,"quantity":2,"tournamentId":CUP}]);ws.send(Message::Text(account_message(batch).to_string().into())).unwrap();thread::sleep(Duration::from_millis(250));
        });
        let(socket,_)=tungstenite::connect(format!("ws://{address}/realtime")).unwrap();let captured=Arc::new(Mutex::new(Vec::<String>::new()));let sink=captured.clone();
        let options=AccountOptions{exchanges:[("101".into(),"201".into())].into(),capture:Box::new(move|_,raw,_,_|{sink.lock().unwrap().push(raw.into());Ok(())})};
        let stream=Realtime::start_configured(socket,"DUMMY_TEST_TOKEN".into(),CUP,now()+5000,Some((format!("user:{CUP}"),options))).unwrap();let signal=stream.account_signal().unwrap();let until=Instant::now()+Duration::from_secs(2);while signal.generation()<2&&Instant::now()<until{thread::sleep(Duration::from_millis(1));}
        assert_eq!(signal.generation(),2);assert!(signal.requires_reconciliation());assert_eq!(captured.lock().unwrap().len(),1);assert!(matches!(stream.incoming.try_recv().unwrap(),Incoming::Joined{..}));assert!(!matches!(stream.incoming.try_recv(),Ok(Incoming::Broadcast{..})));drop(stream);server.join().unwrap();
    }
    #[test]fn account_authentication_is_exact_and_cannot_request_an_arbitrary_topic(){let(_,_,_)=account_protocol();let options=AccountOptions{exchanges:Default::default(),capture:Box::new(|_,_,_,_|Ok(()))};validate_account_options(&format!("user:{CUP}"),&options).unwrap();validate_account_options("user:12345",&options).unwrap();for topic in ["", "realtime:user:other", "market:101", "user:", "user:another:topic", "user:00000000/0000/0000/0000/000000000001"]{assert!(validate_account_options(topic,&options).is_err());}}

    #[test]fn changed_same_revision_is_captured_and_fails_closed_instead_of_hiding_mutation(){let(mut p,signal,captured)=account_protocol();joined_account(&mut p);let original=account_message(account_batch(1,0));p.accept(&original).unwrap();let before=(signal.generation(),signal.last_received_ms());let mut changed=account_batch(1,0);changed["fills"]=json!([{"orderId":"901","exchangeId":"101","marketId":"201","quantity":1,"price":0.4,"tournamentId":CUP}]);let bad=account_message(changed);assert_eq!(p.accept_original(&bad,"EXACT_CONFLICTING_ORIGINAL_BODY").err(),Some("STREAM_ACCOUNT_REVISION_CONFLICT"));assert_eq!(captured.lock().unwrap().last().unwrap()["raw"],"EXACT_CONFLICTING_ORIGINAL_BODY");assert_eq!(signal.generation(),before.0+1);assert!(signal.last_received_ms()>=before.1);assert_eq!(sanitized_exit("STREAM_ACCOUNT_REVISION_CONFLICT"),"STREAM_ACCOUNT_REVISION_CONFLICT");}
    #[test]fn lower_revision_requires_retained_exact_identity_and_unknown_old_revision_fails(){let(mut p,signal,_)=account_protocol();joined_account(&mut p);let original=account_message(account_batch(1,0));p.accept(&original).unwrap();p.accept(&account_message(account_batch(2,1))).unwrap();let before=(signal.generation(),signal.last_received_ms());p.accept(&original).unwrap();assert_eq!((signal.generation(),signal.last_received_ms()),before);p.account.as_mut().unwrap().revisions.remove(&1);assert_eq!(p.accept(&original).err(),Some("STREAM_ACCOUNT_REVISION_UNVERIFIABLE"));}

}
