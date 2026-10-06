// Supabase Realtime Phoenix websocket transport — the concrete RealtimeSdk
// injected into realtime_client. Protocol: wss://<host>/realtime/v1/websocket,
// topic 'realtime:<topic>', join payload carries access_token + private:true.
// Events: 'broadcast' payloads deliver {type:'broadcast', event, payload};
// 'phx_reply' status 'ok' on join => SUBSCRIBED; error/timeout/closed map to
// CHANNEL_ERROR/TIMED_OUT/CLOSED. 'phoenix/heartbeat' answers keepalive.

use serde_json::{json, Value as V};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tungstenite::{connect, stream::MaybeTlsStream, Message};
use std::net::TcpStream;

use crate::realtime_client::{BroadcastCb, RealtimeChannel, RealtimeSdk, SdkOptions, StatusCb};

type Ws = tungstenite::WebSocket<MaybeTlsStream<TcpStream>>;

struct ChannelState {
    topic: String,
    join_ref: i64,
    broadcast_cbs: Vec<(String, BroadcastCb)>,
    status_cb: Option<StatusCb>,
    joined: bool,
}
struct Shared {
    socket: Mutex<Option<Ws>>,
    channels: Mutex<BTreeMap<String, Arc<Mutex<ChannelState>>>>,
    ref_counter: AtomicI64,
    connected: AtomicBool,
    token_slot: Arc<Mutex<Option<String>>>,
    logger: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    heartbeat_cb: Mutex<Option<Arc<dyn Fn(&str) + Send + Sync>>>,
    reconnect_after_ms: i64,
    url: String,
    anon_key: String,
    generation: Arc<AtomicI64>,
    stop: AtomicBool,
}
pub struct SupabaseRealtime { shared: Arc<Shared> }
pub struct SupabaseChannel { shared: Arc<Shared>, state: Arc<Mutex<ChannelState>> }

fn send(shared: &Shared, msg: &V) -> Result<(), String> {
    let mut guard = shared.socket.lock().map_err(|e| e.to_string())?;
    let ws = guard.as_mut().ok_or("REALTIME_SOCKET_CLOSED")?;
    ws.send(Message::Text(serde_json::to_string(msg).map_err(|e| e.to_string())?)).map_err(|e| e.to_string())?;
    ws.flush().map_err(|e| e.to_string())
}
fn push(shared: &Shared, topic: &str, event: &str, payload: V, ref_id: i64) -> Result<(), String> {
    send(shared, &json!({"topic": topic, "event": event, "payload": payload, "ref": ref_id.to_string()}))
}
fn reader_thread(shared: Arc<Shared>) {
    loop {
        if shared.stop.load(Ordering::SeqCst) { return; }
        let msg = {
            let mut guard = match shared.socket.lock() { Ok(g) => g, Err(_) => return };
            match guard.as_mut() {
                Some(ws) => ws.read(),
                None => return,
            }
        };
        match msg {
            Ok(Message::Text(text)) => {
                if let Some(log) = &shared.logger { log("receive"); }
                let v: V = match serde_json::from_str(&text) { Ok(v) => v, Err(_) => continue };
                let topic = v["topic"].as_str().unwrap_or("");
                let event = v["event"].as_str().unwrap_or("");
                if topic == "phoenix" && event == "phx_reply" && v["payload"]["response"].is_null() == false {
                    // heartbeat reply
                    if let Some(cb) = shared.heartbeat_cb.lock().unwrap().clone() {
                        let status = v["payload"]["status"].as_str().unwrap_or("unknown");
                        cb(if status == "ok" { "ok" } else { "error" });
                    }
                    continue;
                }
                let channels = shared.channels.lock().unwrap().clone();
                if let Some(ch) = channels.get(topic) {
                    let mut st = ch.lock().unwrap();
                    match event {
                        "phx_reply" => {
                            let status = v["payload"]["status"].as_str().unwrap_or("error");
                            if let Some(cb) = st.status_cb.clone() {
                                match status {
                                    "ok" => { st.joined = true; cb("SUBSCRIBED", &V::Null); }
                                    "timeout" => cb("TIMED_OUT", &v["payload"]),
                                    _ => cb("CHANNEL_ERROR", &v["payload"]),
                                }
                            }
                        }
                        "broadcast" => {
                            for (filter, cb) in st.broadcast_cbs.clone() {
                                if filter == "*" || filter == v["payload"]["event"].as_str().unwrap_or("") {
                                    cb(&v["payload"]);
                                }
                            }
                        }
                        "phx_close" | "phx_error" => {
                            st.joined = false;
                            if let Some(cb) = st.status_cb.clone() { cb("CLOSED", &v["payload"]); }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Message::Ping(p)) => {
                let _ = shared.socket.lock().map(|mut g| g.as_mut().map(|ws| ws.send(Message::Pong(p))));
            }
            Ok(Message::Close(_)) | Err(_) => {
                if let Some(log) = &shared.logger { log("error"); }
                shared.connected.store(false, Ordering::SeqCst);
                // Notify channels of socket-level close.
                let channels = shared.channels.lock().unwrap().clone();
                for (_t, ch) in channels {
                    let mut st = ch.lock().unwrap();
                    st.joined = false;
                    if let Some(cb) = st.status_cb.clone() { cb("CLOSED", &V::Null); }
                }
                return;
            }
            _ => {}
        }
    }
}
fn heartbeat_thread(shared: Arc<Shared>) {
    let mut failures = 0u32;
    while !shared.stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_secs(30));
        if shared.stop.load(Ordering::SeqCst) { return; }
        if !shared.connected.load(Ordering::SeqCst) {
            if let Some(cb) = shared.heartbeat_cb.lock().unwrap().clone() { cb("timeout"); }
            failures += 1;
            if failures > 2 {
                // Surface socket failure; host drives reconnect pacing.
                let channels = shared.channels.lock().unwrap().clone();
                for (_t, ch) in channels {
                    let mut st = ch.lock().unwrap();
                    st.joined = false;
                    if let Some(cb) = st.status_cb.clone() { cb("TIMED_OUT", &V::Null); }
                }
                return;
            }
            continue;
        }
        failures = 0;
        let r = shared.ref_counter.fetch_add(1, Ordering::SeqCst);
        if let Some(cb) = shared.heartbeat_cb.lock().unwrap().clone() { cb("sent"); }
        if push(&shared, "phoenix", "heartbeat", json!({}), r).is_err() {
            shared.connected.store(false, Ordering::SeqCst);
        }
    }
}
impl SupabaseRealtime {
    /// createClient(url, anonKey, {accessToken, realtime:{logger, reconnectAfterMs}})
    pub fn create(url: &str, anon_key: &str, opts: &SdkOptions) -> Result<Self, String> {
        let ws_url = format!("{}/realtime/v1/websocket?apikey={}&vsn=1.0.0", url.trim_end_matches('/'), anon_key);
        let (socket, _resp) = connect(&ws_url).map_err(|_| "REALTIME_CONNECTION_FAILED".to_string())?;
        let shared = Arc::new(Shared {
            socket: Mutex::new(Some(socket)),
            channels: Mutex::new(BTreeMap::new()),
            ref_counter: AtomicI64::new(0),
            connected: AtomicBool::new(true),
            token_slot: opts.access_token_slot.clone(),
            logger: opts.logger.clone(),
            heartbeat_cb: Mutex::new(None),
            reconnect_after_ms: opts.reconnect_after_ms,
            url: url.to_string(), anon_key: anon_key.to_string(),
            generation: Arc::new(AtomicI64::new(0)),
            stop: AtomicBool::new(false),
        });
        let s = shared.clone();
        std::thread::spawn(move || reader_thread(s));
        let s = shared.clone();
        std::thread::spawn(move || heartbeat_thread(s));
        Ok(Self { shared })
    }
}
impl RealtimeSdk for SupabaseRealtime {
    fn channel(&self, topic: &str, private: bool) -> Result<Box<dyn RealtimeChannel>, String> {
        let _ = private;
        let name = format!("realtime:{}", topic);
        let state = Arc::new(Mutex::new(ChannelState { topic: name.clone(), join_ref: 0,
            broadcast_cbs: vec![], status_cb: None, joined: false }));
        self.shared.channels.lock().unwrap().insert(name, state.clone());
        Ok(Box::new(SupabaseChannel { shared: self.shared.clone(), state }))
    }
    fn remove_all_channels(&self) -> Result<(), String> {
        let channels: Vec<Arc<Mutex<ChannelState>>> = self.shared.channels.lock().unwrap().values().cloned().collect();
        for ch in channels {
            let st = ch.lock().unwrap();
            let _ = push(&self.shared, &st.topic, "phx_leave", json!({}), self.shared.ref_counter.fetch_add(1, Ordering::SeqCst));
        }
        self.shared.channels.lock().unwrap().clear();
        Ok(())
    }
    fn set_auth(&self, token: &str) -> Result<(), String> {
        *self.shared.token_slot.lock().map_err(|e| e.to_string())? = Some(token.to_string());
        Ok(())
    }
    fn disconnect(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let mut g = self.shared.socket.lock().unwrap();
        if let Some(ws) = g.as_mut() { let _ = ws.close(None); }
        self.shared.connected.store(false, Ordering::SeqCst);
    }
    fn on_heartbeat(&self, cb: Arc<dyn Fn(&str) + Send + Sync>) {
        *self.shared.heartbeat_cb.lock().unwrap() = Some(cb);
    }
    fn connection_state(&self) -> String {
        if self.shared.connected.load(Ordering::SeqCst) { "open".into() }
        else if self.shared.stop.load(Ordering::SeqCst) { "closed".into() } else { "closing".into() }
    }
}
impl RealtimeChannel for SupabaseChannel {
    fn on(&mut self, kind: &str, event: &str, cb: BroadcastCb) {
        if kind == "broadcast" {
            self.state.lock().unwrap().broadcast_cbs.push((event.to_string(), cb));
        }
    }
    fn subscribe(&mut self, cb: StatusCb) {
        {
            self.state.lock().unwrap().status_cb = Some(cb.clone());
        }
        let token = self.shared.token_slot.lock().unwrap().clone().unwrap_or_default();
        let topic = self.state.lock().unwrap().topic.clone();
        let join_ref = self.shared.ref_counter.fetch_add(1, Ordering::SeqCst);
        self.state.lock().unwrap().join_ref = join_ref;
        let payload = json!({"config": {"broadcast": {"ack": false, "self": false}, "presence": {"key": ""},
            "private": true, "postgres_changes": []}, "access_token": token});
        if let Err(_e) = push(&self.shared, &topic, "phx_join", payload, join_ref) {
            cb("CHANNEL_ERROR", &json!({"message": e_desc(_e)}));
        }
    }
}
fn e_desc(e: String) -> String { e }
/// createClient factory matching CreateClient signature.
pub fn create_supabase_client(url: &str, anon_key: &str, opts: &SdkOptions) -> Result<Box<dyn RealtimeSdk>, String> {
    Ok(Box::new(SupabaseRealtime::create(url, anon_key, opts)?))
}
