//! Verify the complete append-only event chain before admitting each event.
use crate::Result;
use serde_json::Value;
use sha2::{Digest, Sha256};
#[derive(Default, Clone)]
pub struct Journal {
    pub sequence: u64,
    pub previous: String,
}
impl Journal {
    pub fn accept(&mut self, row: &Value) -> Result<Value> {
        let mut body = row.as_object().ok_or("JOURNAL_OBJECT")?.clone();
        let digest = body.remove("hash").ok_or("JOURNAL_HASH")?;
        if body.len() != 3
            || !body.contains_key("sequence")
            || !body.contains_key("previous")
            || !body.contains_key("event")
        {
            return Err("JOURNAL_FIELDS");
        }
        if body["sequence"].as_u64() != self.sequence.checked_add(1)
            || body["previous"].as_str() != Some(self.previous.as_str())
        {
            return Err("JOURNAL_CHAIN");
        }
        let actual = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&body).map_err(|_| "JOURNAL_JSON")?)
        );
        if digest.as_str() != Some(actual.as_str()) {
            return Err("JOURNAL_HASH_MISMATCH");
        }
        let event = body["event"].clone();
        if !event.is_object() {
            return Err("JOURNAL_EVENT");
        }
        self.sequence += 1;
        self.previous = actual;
        Ok(event)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn row(sequence: u64, previous: &str) -> Value {
        let mut body = json!({"sequence":sequence,"previous":previous,"event":{"type":"tick","receivedAt":"2026-10-14T00:00:00Z"}});
        body["hash"] = json!(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&body).unwrap())
        ));
        body
    }
    #[test]
    fn changed_skipped_and_reordered_events_fail() {
        let mut j = Journal::default();
        let first = row(1, "");
        j.accept(&first).unwrap();
        let old = j.previous.clone();
        assert!(j.accept(&first).is_err());
        assert!(j.accept(&row(3, &old)).is_err());
        let mut altered = row(2, &old);
        altered["event"]["type"] = json!("gap");
        assert!(j.accept(&altered).is_err());
        assert_eq!(j.previous, old);
        j.accept(&row(2, &old)).unwrap();
        assert_eq!(j.sequence, 2)
    }
}
