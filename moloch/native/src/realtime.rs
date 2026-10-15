//! Supabase Phoenix v1 transport. Credentials remain in memory; error messages
//! never include URLs, request headers, join payloads or server error bodies.
use crate::Result;
use serde_json::{Value, json};
use std::{
    net::{TcpStream, ToSocketAddrs},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};
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
            url: u.to_string(),
            key: text("anonKey")?.into(),
            token: text("token")?.into(),
            deadline_ms: (expiry as u64 - 60000).min(now() + 30 * 60000),
        })
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
        "STREAM_FRAME" | "STREAM_JOIN_REJECTED" | "STREAM_HEARTBEAT_REJECTED"
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
        })
    }
    fn join(&self, token: &str) -> Value {
        json!({"topic":self.topic,"event":"phx_join","payload":{"config":{"broadcast":{"ack":false,"self":false},"presence":{"enabled":false},"postgres_changes":[],"private":true},"access_token":token},"ref":"1","join_ref":"1"})
    }
    fn accept(&mut self, v: &Value) -> Result<Option<Incoming>> {
        let event = v["event"].as_str().ok_or("STREAM_FRAME")?;
        let topic = v["topic"].as_str().ok_or("STREAM_FRAME")?;
        if event == "phx_reply" {
            if topic == self.topic && v["ref"] == "1" {
                if self.joined || v["payload"]["status"] != "ok" {
                    return Err("STREAM_JOIN_REJECTED");
                }
                self.joined = true;
                return Ok(Some(Incoming::Joined { received_at: iso() }));
            }
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
}
impl Realtime {
    pub fn connect(auth: Auth, tournament: &str) -> Result<Self> {
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
        Self::start(socket, auth.token, tournament, auth.deadline_ms)
    }
    fn start(
        mut socket: WebSocket<MaybeTlsStream<TcpStream>>,
        token: String,
        tournament: &str,
        deadline_ms: u64,
    ) -> Result<Self> {
        let mut protocol = Protocol::new(tournament)?;
        socket
            .send(Message::Text(protocol.join(&token).to_string().into()))
            .map_err(|_| "STREAM_JOIN_SEND")?;
        drop(token);
        tcp(socket.get_ref())?
            .set_read_timeout(Some(Duration::from_millis(100)))
            .map_err(|_| "STREAM_TIMEOUT")?;
        let (tx, incoming) = mpsc::sync_channel(64);
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
        })
    }
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
        if !protocol.joined && began.elapsed() > Duration::from_secs(10) {
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
                if let Some(incoming) = protocol.accept(&v)? {
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
    fn reporter()->(ExitReporter,Arc<Mutex<Option<&'static str>>>,Arc<AtomicBool>){
        let reason=Arc::new(Mutex::new(None));let failed=Arc::new(AtomicBool::new(false));
        (ExitReporter{reason:reason.clone(),failed:failed.clone(),published:false},reason,failed)
    }
    #[test]
    fn saturated_queue_cannot_hide_published_reason_or_weaken_failed_gate(){
        let(tx,rx)=mpsc::sync_channel(64);
        for _ in 0..64{deliver(&tx,Incoming::Joined{received_at:"fixture".into()}).unwrap();}
        let error=deliver(&tx,Incoming::Joined{received_at:"overflow".into()}).unwrap_err();
        assert_eq!(error,"STREAM_QUEUE_FULL");
        let(mut reporter,reason,failed)=reporter();reporter.publish(error);
        assert!(tx.try_send(Incoming::Failed(error)).is_err());
        let stream=Realtime{incoming:rx,stop:Arc::new(AtomicBool::new(false)),failed,exit_reason:reason,worker:None,deadline_ms:0};
        assert_eq!(stream.exit_reason(),Some("STREAM_QUEUE_FULL"));assert!(!stream.healthy());assert!(stream.failure_signal().load(Ordering::SeqCst));
        // Existing queued data was neither consumed to find a reason nor dropped.
        assert_eq!(stream.incoming.try_iter().count(),64);
    }
    #[test]
    fn disconnected_consumer_is_not_queue_capacity_failure(){
        let(tx,rx)=mpsc::sync_channel(64);drop(rx);
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
            let(tx,_rx)=mpsc::sync_channel(64);let mut protocol=Protocol::new(CUP).unwrap();let stop=AtomicBool::new(requested);
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
}
