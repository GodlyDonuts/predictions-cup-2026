# Exclusive incremental journal append worker

This offline Rust worker preserves the existing Node `Journal` envelope and its exact compact `JSON.stringify` bytes. It verifies every original complete line, original record hash chain, supplied full-file SHA-256, byte count, sequence and tail hash before admitting an append. It rejects partial tails; it never truncates, repairs, resets accounting or renews execution authority.

On Windows the file stays open with `FILE_SHARE_READ` only. Other ordinary file writers, truncation, deletion, rename and competing append owners are denied by the operating system, including while initial verification runs. The worker compares stable volume/file-index identity and the reopened path before commits. This exclusive file handle protects the private journal, not the shared trading account: account ownership and authorized execution remain the parent's responsibility. Non-Windows builds fail closed because this crate does not yet implement an equivalent exclusive writer handle there.

The worker maintains an incremental full-file SHA-256 state. Each append checks the exact prior head, writes all bytes and calls `sync_all` before returning a durable receipt. Partial write or durability failure makes the session fail closed and requires full reconciliation. No per-append historical reread occurs. The performance test compares identical resulting bytes against the former Node pattern of hashing the full journal before and after each append; results are in `benchmark-results.json`.

Build from `backend-rust`:

```powershell
cargo build --offline --release --manifest-path journal-append/Cargo.toml
cargo test --offline --manifest-path journal-append/Cargo.toml
```

From repository root run `node --test backend-rust/journal-append/test.mjs`. The test copies a sealed original local paper journal into an isolated temporary directory. It tests concurrent mutation/replacement denial, exact decimal/Unicode/number bytes, trusted ownership and STOP guards, duplicate/stale head rejection, partial writes, and a crashed worker with a lost acknowledgment. Existing runtime journals, configurations and banks stay untouched.

The persistent executable accepts one JSON command per line and returns one JSON response. `open` requires:

```json
{"op":"open","path":"C:/absolute/private/journal.jsonl","createNew":false,"ownerToken":"unique-parent-owned-token","generation":1,"canOperate":true,"stopped":false,"stopPaths":["C:/absolute/private/STOP"],"expectedBytes":0,"expectedSequence":0,"expectedTailHash":"","expectedSourceHash":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"}
```

`commit` repeats the owner token, generation, trusted admission/STOP fields and the exact current `expectedBytes`, `expectedSequence`, `expectedTailHash`, `expectedSourceHash`. It supplies `bodyJson`, the already compact caller-produced body in this exact order: `sequence`, `previous`, `at`, `event`, `state`. The worker validates canonical bytes without sorting or reserializing the nested event/state. Its response contains `durable:true`, `recordJson` and a receipt with engine hash, stable file ID, sequence, tail hash, byte count and incremental source hash. `status` is an observation; `close` releases the handle.

The integrating parent must bind the executable/dependencies and embedded engine hash, serialize commands, validate the exact expected record and receipt, retain its last acknowledged state and treat malformed output, process death, missing acknowledgments or unknown acceptance as reconciliation barriers. Never blindly retry a commit or replacement from a stale head. A fresh restart verifies the entire observed file again. Trusted `canOperate` fields and an owner token are local recording guards; agent messages cannot grant contest-order authority.

This crate supplies journal integrity/durability only. It does not validate economic strategy transitions or source publication semantics, make account requests, place orders or change a frozen cohort. An inventory-preserving production continuation needs its own bank, original journal-prefix/tail binding, original funded state and ownership/source frontier checks before this worker is integrated.
