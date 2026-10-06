// Cross-runtime parity: journal bytes produced by live-paper/transport-runtime.mjs
// must verify under the Rust reader, and Rust commits must carry the JS layout.
use baal_service_runtime::journal::{read_verified_source, Journal, JournalReadOptions};
use baal_service_runtime::json_text::js_stringify;
use serde_json::json;

const JS_JOURNAL: &str = concat!(
    "{\"sequence\":1,\"previous\":\"\",\"at\":\"2026-10-06T08:08:17.018Z\",\"event\":{\"kind\":\"first\",\"clockAt\":\"2026-10-06T00:00:00.000Z\"},\"state\":{\"a\":1,\"nested\":{\"x\":0.5,\"list\":[1,\"two\",null]}},\"hash\":\"bdb958f0c4807fd23d3abd26ce3c943f398a29b59aa3ef8b77c087bc12f969b0\"}\n",
    "{\"sequence\":2,\"previous\":\"bdb958f0c4807fd23d3abd26ce3c943f398a29b59aa3ef8b77c087bc12f969b0\",\"at\":\"2026-10-06T08:08:17.019Z\",\"event\":{\"kind\":\"second\",\"clockAt\":\"2026-10-06T00:00:01.000Z\"},\"state\":{\"b\":2},\"hash\":\"89c812e16355e7416a04890654c10b26bd3bf38c321c6f1b97c1528a9dd75eb4\"}\n",
);

#[test]
fn verifies_js_written_journal() {
    let dir = std::env::temp_dir().join(format!("parity-j-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("journal.jsonl");
    std::fs::write(&file, JS_JOURNAL).unwrap();
    let j = Journal::open(&file).unwrap();
    assert_eq!(j.sequence, 2);
    assert_eq!(j.previous, "89c812e16355e7416a04890654c10b26bd3bf38c321c6f1b97c1528a9dd75eb4");
    let src = read_verified_source(&file, &JournalReadOptions::default()).unwrap().unwrap();
    assert_eq!(src.rows.len(), 2);
    assert_eq!(src.prefix_hashes.len(), 2);
    // rows keep sequence/previous/at/event/hash; state is stripped.
    assert!(src.rows[0].get("state").is_none());
    assert_eq!(src.rows[0]["event"]["kind"].as_str(), Some("first"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn rust_commit_matches_js_shape() {
    // sha256(JSON.stringify(body)) over {sequence,previous,at,event,state}:
    // replicate a JS-written record byte-for-byte with a fixed `at`.
    let dir = std::env::temp_dir().join(format!("parity-r-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("journal.jsonl");
    let mut j = Journal::open(&file).unwrap();
    let record = j
        .commit(&json!({"a": 1, "nested": {"x": 0.5, "list": [1, "two", null]}}), &json!({"kind": "first", "clockAt": "2026-10-06T00:00:00.000Z"}))
        .unwrap();
    let keys: Vec<String> = record.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, vec!["sequence", "previous", "at", "event", "state", "hash"]);
    assert_eq!(record["previous"].as_str(), Some(""));
    // The stored line is js_stringify with hash appended last.
    let raw = std::fs::read_to_string(&file).unwrap();
    let line = raw.trim_end();
    assert_eq!(line, js_stringify(&record));
    std::fs::remove_dir_all(&dir).unwrap();
}
