//! Wait for both native subscriptions without mistaking early market data for a failure.
use moloch_replay::realtime::Incoming;
use std::{error::Error, fmt, sync::mpsc::RecvTimeoutError, time::{Duration, Instant}};

const MAXIMUM_PENDING: usize = 64;
const MAXIMUM_PENDING_BYTES: usize = 4 * 1024 * 1024;

pub struct Joined {
    pub received_at: String,
    pub pending: Vec<Incoming>,
}

#[derive(Debug)]
pub struct JoinFailure {
    reason: &'static str,
    pub buffered: usize,
}
impl JoinFailure {
    pub fn reason(&self) -> &'static str { self.reason }
    pub fn retryable(&self) -> bool {
        matches!(self.reason, "STREAM_JOIN_TIMEOUT" | "STREAM_JOIN_DISCONNECTED" | "STREAM_JOIN_PENDING_OVERFLOW")
            || moloch_live_runtime::stream_recovery::StreamFailure::new(self.reason).is_recoverable()
    }
}
impl fmt::Display for JoinFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.reason) }
}
impl Error for JoinFailure {}

pub fn wait(mut receive: impl FnMut(Duration) -> Result<Incoming, RecvTimeoutError>, timeout: Duration) -> Result<Joined, JoinFailure> {
    let began = Instant::now();
    let mut pending = Vec::new();
    let mut bytes = 0usize;
    loop {
        let remaining = timeout.checked_sub(began.elapsed()).filter(|v| !v.is_zero())
            .ok_or(JoinFailure { reason: "STREAM_JOIN_TIMEOUT", buffered: pending.len() })?;
        match receive(remaining) {
            Ok(Incoming::Joined { received_at }) => return Ok(Joined { received_at, pending }),
            Ok(item @ Incoming::Broadcast { .. }) => {
                let size = match &item {
                    Incoming::Broadcast { event, payload, received_at } => event.len() + payload.to_string().len() + received_at.len(),
                    _ => unreachable!(),
                };
                bytes = bytes.saturating_add(size);
                if pending.len() >= MAXIMUM_PENDING || bytes > MAXIMUM_PENDING_BYTES {
                    return Err(JoinFailure { reason: "STREAM_JOIN_PENDING_OVERFLOW", buffered: pending.len() });
                }
                pending.push(item);
            }
            Ok(Incoming::Failed(reason)) => return Err(JoinFailure { reason, buffered: pending.len() }),
            Err(error) => return Err(JoinFailure {
                reason: match error { RecvTimeoutError::Timeout => "STREAM_JOIN_TIMEOUT", RecvTimeoutError::Disconnected => "STREAM_JOIN_DISCONNECTED" },
                buffered: pending.len(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::mpsc;
    fn broadcast(n: u64) -> Incoming {
        Incoming::Broadcast { event: "book_dirty".into(), payload: json!({"sequence": n}), received_at: format!("original-{n}") }
    }
    #[test]
    fn early_market_frames_survive_combined_join_in_original_order() {
        let (tx, rx) = mpsc::channel();
        tx.send(broadcast(1)).unwrap(); tx.send(broadcast(2)).unwrap();
        tx.send(Incoming::Joined { received_at: "both-joined".into() }).unwrap();
        tx.send(broadcast(3)).unwrap();
        let result = wait(|duration| rx.recv_timeout(duration), Duration::from_secs(1)).ok().unwrap();
        assert_eq!(result.received_at, "both-joined"); assert_eq!(result.pending.len(), 2);
        for (index, item) in result.pending.iter().enumerate() {
            match item { Incoming::Broadcast { payload, received_at, .. } => {
                assert_eq!(payload["sequence"], (index + 1) as u64);
                assert_eq!(received_at, &format!("original-{}", index + 1));
            }, _ => panic!("original broadcast missing") }
        }
        assert!(matches!(rx.try_recv(), Ok(Incoming::Broadcast { .. })));
    }
    #[test]
    fn missing_join_is_bounded_and_never_ready() {
        let (_tx, rx) = mpsc::channel();
        let error = wait(|duration| rx.recv_timeout(duration), Duration::from_millis(2)).err().unwrap();
        assert_eq!(error.reason(), "STREAM_JOIN_TIMEOUT"); assert!(error.retryable());
    }
    #[test]
    fn protocol_and_authorization_failures_are_not_transient_connections() {
        for reason in ["STREAM_JOIN_REJECTED", "STREAM_JSON", "STREAM_BEFORE_JOIN", "STREAM_ACCOUNT_ROW"] {
            let (tx, rx) = mpsc::channel(); tx.send(broadcast(1)).unwrap();
            tx.send(Incoming::Failed(reason)).unwrap();
            let error = wait(|duration| rx.recv_timeout(duration), Duration::from_secs(1)).err().unwrap();
            assert_eq!(error.reason(), reason); assert_eq!(error.buffered, 1); assert!(!error.retryable());
        }
    }
    #[test]
    fn transient_socket_failure_retains_reason() {
        let (tx, rx) = mpsc::channel(); tx.send(Incoming::Failed("STREAM_READ_FAILED")).unwrap();
        let error = wait(|duration| rx.recv_timeout(duration), Duration::from_secs(1)).err().unwrap();
        assert_eq!(error.reason(), "STREAM_READ_FAILED"); assert!(error.retryable());
    }
    #[test]
    fn early_frames_cannot_grow_without_bound() {
        let (tx, rx) = mpsc::channel();
        for i in 0..=MAXIMUM_PENDING { tx.send(broadcast(i as u64)).unwrap(); }
        let error = wait(|duration| rx.recv_timeout(duration), Duration::from_secs(1)).err().unwrap();
        assert_eq!(error.reason(), "STREAM_JOIN_PENDING_OVERFLOW"); assert_eq!(error.buffered, MAXIMUM_PENDING);
        let (tx, rx) = mpsc::channel();
        tx.send(Incoming::Broadcast { event: "book_dirty".into(), payload: json!({"data": "x".repeat(MAXIMUM_PENDING_BYTES)}), received_at: "original".into() }).unwrap();
        assert_eq!(wait(|duration| rx.recv_timeout(duration), Duration::from_secs(1)).err().unwrap().reason(), "STREAM_JOIN_PENDING_OVERFLOW");
    }
}
