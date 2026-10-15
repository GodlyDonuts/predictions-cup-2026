# Bounded durable session batch candidate

Task [#139](https://github.com/GodlyDonuts/predictions-cup-2026/issues/139), Viserion/session-batch-20261015. This adds `Session::accept_batch`, focused tests and a synthetic benchmark. Existing `accept`, open, replay, journal format and upgrade rules are unchanged. The parent subsequently wired normalized public-source events into the worker; actual financial acknowledgments/fills/terminal events retain serial admission. The execution owner remains [#120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120).

## Intended improvement and exact boundaries

A successful serial `accept` synchronizes one journal append, atomically publishes the head, and atomically publishes the derived report. A wide broadcast can therefore repeatedly serialize the full report and perform those durability operations for each normalized event. This is a hypothesis about local publication cost, not a measured explanation of all live latency.

The batch admits at most **32 input entries**, including duplicates, with separate **8 MiB** bounds on aggregate serialized raw input and final appended journal bytes (including row overhead/newlines). An iterative pre-clone shape check also limits each event to 262,144 JSON values and nesting depth64. Oversized/deep inputs are rejected before I/O. Input values already belong to the caller; these bounds do not claim that all engine history, parsed JSON, cloned candidate state, maps, final report and allocator RSS fit in8MiB.

Each entry supplies the same source ID, contiguous sequence and exact original normalized event as serial acceptance. The method:

1. Validates reserved fields, source IDs, input shape/bytes, prior references, within-batch duplicates/conflicts and per-source sequence continuity.
2. Builds a private candidate engine and advances every new event in original order, preserving each event's financial validation, clocks, gaps, quote decisions and duplicate behavior. An invalid last event discards the whole candidate before any append.
3. Constructs and verifies every ordinary individual row, including exactly the existing sourceRef, sequence, previous hash, row hash and newline bytes. There is no batch envelope or new commit marker.
4. Builds/hashes the final head and financial/report projection **before** the first file write. Historical references/facts are not cloned for transactional staging; only new references/cursors are staged. A merged cursor projection is materialized once for the final report.
5. Sets the session poison flag, appends the prepared row bytes, synchronizes the journal once, then publishes head once and report once through existing `atomic_json` file/rename/directory durability.
6. Updates in-memory engine/chain/references/cursors/facts only after those publications succeed, clears poison, and returns per-entry accepted flags plus appended row/byte counts. A duplicate-only or empty batch performs no publication and no journal sync.

Any I/O failure leaves the session poisoned. Neither API may accept further input in that session. A failed batch does not return successful row flags; callers must not submit an order or treat its memory/report as a committed financial action. Existing readers must continue checking projection/head anchors; two separate atomic projection files are not a joint multi-file snapshot.

`Engine::advance` still performs its existing per-event transactional clone and audit. This change reduces outer cloning and publication/synchronization frequency; it does not claim to remove every event clone, solve unbounded history growth, or make source normalization/account operations faster. The complete batch may wait longer before becoming visible than its earliest event would under serial admission. A future caller must bound queued latency and never reorder events to fill a batch.

## Crash contract: serial prefix, not atomic whole batch

With the unchanged journal format, **all-or-none batch crash atomicity is impossible**. The parent explicitly accepted the existing prefix contract:

- All candidate semantics are validated before any write.
- No batch success is returned before journal, head and report durability.
- A crash or failed write can leave zero rows, a complete valid prefix, a complete batch, or a partial last row.
- Existing open/replay may recover a complete valid serial prefix ahead of the old head and republish derived state. This may be only part of the attempted batch; source-reference retries must deduplicate that prefix and resume the remaining exact source entries.
- A partial final row, corrupted chain, missing committed rows or changed source prefix still fails closed. There is no truncation, automatic repair, silent rollback or HTTP retry.

This API is unsuitable for a strategy that requires several financial events to be crash-atomic as one economic transaction. It performs no account dispatch inside the batch. The caller must wait for successful admission before any subsequent execution decision and retain exact original source evidence for interrupted recovery.

## Focused checks

`tests/session_batch.rs` includes nine offline tests:

- Existing golden fee/netting, source-gap/cancellation/coverage and duplicate-precision cases produce byte-identical individual journals, heads, reports, cursors and restart results under serial and batch admission.
- Historical and within-batch duplicates are exactly once; a late conflicting reference or sequence gap changes no files/cursors and does not poison a still-unwritten session.
- Invalid late events/reserved fields/clock rollback reject the whole candidate before I/O.
- Entry count, raw bytes, final row-overhead bytes and nested-shape limits are checked; duplicate-only/empty calls do not touch unavailable projection paths.
- Real head/report publication failures are injected using isolated fixture directories. Both APIs then reject the poisoned session; reopening recovers the complete validated journal prefix without financial duplication.
- Every complete prefix of a prepared three-row batch can recover ahead of an old head. A partial last row remains unchanged and fails closed.
- A live-mode synthetic owned ACK, partial actual fill and independently complete terminal event match serial bytes/financial state; no public print or report creates actual fill credit.

The tests inject projection failures and construct interrupted journal prefixes; they do **not** claim to have injected a real disk-full journal-write failure or simulated power-loss storage hardware. Existing filesystem durability guarantees remain the boundary.

## Offline ARM64 benchmark protocol

After the parent approves CPU work, build and run with reduced priority in a separate source checkout:

```text
cargo test --locked --manifest-path moloch/native/Cargo.toml --test session_batch
cargo build --locked --release --manifest-path moloch/native/Cargo.toml --bin moloch-session-batch-bench
nice -n 10 moloch/native/target/release/moloch-session-batch-bench both 2 32
```

The binary creates a fresh private0700 directory under the OS temporary directory; synthetic input/policy/results files are0600. It reads no production files, credentials or account data and constructs no network client. Fixtures remain for independent verification.

There are237 fixed-clock synthetic book scopes per round, with identical ordered events/source IDs across serial and batch paths, quantity1 and three possible strategy markets. Default two rounds are474 events, not an observation of a live237-market broadcast. Both paths verify restart parity and exact journal/head/report/cursor equality. Output includes input/source identity, timings, burst timings, durable commit counts and hashes. Timers exclude fixture construction, initial open and restart verification. Both mode runs serial first, so cache/order/filesystem bias remains; this is not a production latency forecast or a calibrated own-fill benchmark.

`syncCallModel` reports **source-derived** counts, not instrumented syscalls: each accepted commit calls one journal sync and two atomic projection file syncs, plus two directory syncs on Unix. A measured Linux `strace -f -c -e trace=fsync,fdatasync` run in separate `serial` and `batch` modes can check actual whole-process syscall counts; initialization, fixture files and restart add extra syncs outside ingest timers. Do not label modeled counts as measured ones. No speed result is published until the parent runs and verifies the benchmark.

## Checked parent integration and measurements

On the ARM64 VPS, all103 native checks and locked native release passed. The fixed474-event synthetic fixture took3,552,963 microseconds through serial acceptance and301,495 microseconds through batches of at most32. Journal, head and report bytes match exactly; both restart checks pass. There were474 versus16 durable commits. This is approximately11.8 times faster for this fixture, with serial-first/cache bias and no production latency claim. Sync counts in the artifact are modeled from source, not traced syscalls. [Benchmark](../research/moloch-session-batch-20261015/benchmark.json).

The read-only original16,535-event/45,816,161-byte live prefix replayed in14,016ms with identical financial report, orders, fills and source-cursor hashes. No account request or source mutation occurred. Candidate native identity is0321e7458ed45da08bca7b030369d4e86ae2a9c0127b39fa7d4d49d33d5d0562. [Prefix proof](../research/moloch-session-batch-20261015/prefix-parity.json), [check-log hashes](../research/moloch-session-batch-20261015/checks.json).

Parent worker integration also limits each websocket drain to eight complete frames before returning to the shared REST/reconciliation path. This bounds frames, not wall time or derived events within one frame. Full/incomplete frame errors still stop; nothing is dropped to make a fresh-looking book. A private per-scope progress snapshot supplements the complete-cycle status and is never trading authority. Runtime173 checks pass, including65-event chunk/restart recovery with a deliberately invalid second chunk. The first32 events remain a valid durable prefix; the rejected second chunk adds no rows. This does not claim whole-input crash atomicity.

## Production promotion remains separate

`engine_identity()` includes `session.rs`, so this addition changes native identity even when the worker still calls serial `accept`. Do not install/restart it against an active bank. Future promotion requires the execution owner's stopped-process, exact immutable-prefix report/order/fill/cursor parity bridge and normal durable unknown-acceptance handling. This candidate does not create that approval, clear a STOP, renew activation or alter any historical bank.
