# Sanitized realtime exit diagnostics — October 15

[Task #140](https://github.com/GodlyDonuts/predictions-cup-2026/issues/140), Viserion/stream-diagnostics-20261015. Isolated `codex/stream-diagnostics` from checked main `e30feb4e`. Scope: `moloch/native/src/realtime.rs`, its focused tests and this document. Root retains worker integration, financial identity/prefix checks, compilation, publication and production. Production V20 remains immutable during this source work.

## Concrete failure in prior diagnostics

The socket thread sets the authoritative failed flag before attempting to enqueue `Incoming::Failed(code)`. The worker checks `healthy()` before reading incoming messages and returns generic `LIVE_STREAM_FAILED`. If the64-slot queue is full, the failure message cannot be enqueued at all. Consequently the recorded generic stop does not establish whether the cause was queue capacity, heartbeat, socket I/O, protocol rejection or a different exit. No historical cause is inferred from this patch.

## Candidate behavior

The socket thread now owns an exit reporter with a separate `Arc<Mutex<Option<&'static str>>>`. It writes the first allowlisted fixed reason BEFORE setting the existing failed `AtomicBool`. This diagnostic does not depend on channel capacity, incoming-message consumption or worker progress. Existing `Incoming::Failed` delivery remains best effort for compatibility; it is not the authoritative source of the reason. Cleanup cannot overwrite an earlier cause.

`Realtime::exit_reason() -> Option<&'static str>` reads that out-of-band reason. `None` means no exit was published; it is never evidence of healthy coverage. If a worker is finished or absent without a recorded reason, the getter reports fixed `STREAM_WORKER_EXIT_UNEXPLAINED`. Poisoned diagnostic mutexes do not suppress the failed gate.

| Exit | Fixed diagnostic |
|---|---|
| Receiver capacity exhausted | `STREAM_QUEUE_FULL` |
| Receiver dropped | `STREAM_CONSUMER_DISCONNECTED` |
| Planned auth/connection deadline | `STREAM_DEADLINE_REACHED` |
| Owner requested stop/drop | `STREAM_STOP_REQUESTED` |
| Worker unwinds | `STREAM_WORKER_PANICKED` |
| Unreported exit/unknown diagnostic | `STREAM_WORKER_EXIT_UNEXPLAINED` |
| Existing heartbeat/socket/protocol failures | Existing allowlisted `STREAM_*` code |

The reporter's unwind guard records a fixed panic classification without retaining the panic payload or altering the global panic hook. An existing default hook may still print a panic just as before; this patch does not add payload logging. No URL, token, header, join payload, server body or socket error object is stored. Unknown codes map to the fixed unexplained value. Reasons are bounded static literals, not formatted error strings.

Health behavior is unchanged: failed sockets and finished workers remain unhealthy, and the existing shared failure signal continues to block physical write authority. Requested stop and planned deadline still set failure; their reason is now distinguishable. The queue remains64 frames; message/frame limits remain16MiB; no frame is dropped to make room for a diagnostic. A capacity failure still terminates the socket and requires the existing source recovery boundary. This change does not reconnect, increase queue memory, restart a worker, clear STOP, mint a token, reset coverage or retry an order.

## Root worker integration

Root candidate `6af4e5bd` now reads the out-of-band reason at ordinary drain and urgent-exit health gates. Failure still stops before processing queued frames or submitting an order. It also raises the maximum per drain from eight to64 complete frames, matching the existing receiver capacity; receiver size and memory limits are unchanged. This is a bounded consumer-throughput change, not evidence that the preceding failure was queue overflow and not a guarantee against slow REST requests filling the queue. V20 stopped before these diagnostics were available, so its exact cause cannot be recovered retrospectively.

After source review and checks, root can use `stream.exit_reason().unwrap_or("LIVE_STREAM_FAILED")` when `healthy()` fails and preserve this fixed code in private stopped/recovery diagnostics. Do not drain or process stale queued frames just to find `Incoming::Failed`, and do not treat a diagnostic category as automatic reconnect permission. A healthy socket must still satisfy ordinary source completeness, freshness, native funding and account authority.

`connect_stream` currently collapses unexpected join-wait outcomes into `LIVE_STREAM_JOIN_FAILED`. Root may separately read the new reason before dropping that stream if the join wait fails. Early DNS/handshake/auth setup failures happen before a `Realtime` worker exists and retain their existing returned errors; this patch does not invent an out-of-band object for them.

## Authored verification and limits

Parent ARM64 validation subsequently passed all111 native checks, including these eight new tests, plus173 combined runtime checks and the locked runtime release. The unchanged SDK had already passed150 checks. Native test log SHA256: `37686ae54d11bdcc03d08d5f32195b25e9b38eea305532d0305d07af55a14276`; runtime test log: `dff6442b445ae272543cd434164734f233cc3d90007c4717df84535d02cd284b`; release log: `2824ef0cbcac924f948e361b3566e0483e6f40bafdcb81036c2119c537943ad8`. Candidate binary SHA256: `160126b4fbe23f2ecef7761dbc4598dce647ec763fbee219eb64bca8c75608c0`. Changed realtime/worker source hashes matched the parent checkout and build host. These checks establish the diagnostic and build behavior; successful sustained production remains a separate observation.

Eight new focused tests cover full64-slot capacity with failed-message delivery unavailable; consumer disconnection; reason visibility when another thread observes the failed flag; first-cause preservation; fixed panic and unexplained exit guards; poisoned diagnostic storage; unknown/secret-like diagnostic sanitization; and actual local WebSocket loop exits distinguishing planned deadline from requested stop. Existing protocol/socket tests remain.

These are authored tests at initial source handoff, not passing Linux evidence. Rust is unavailable on this Windows device; no Linux jobs, SSH, live account API, credentials, runtime or bank changes were performed by this child. Root owns native/runtime/release validation and any immutable-prefix comparison before promotion. This patch improves observability only and claims no recovered trading time or profit. The separate bounded scheduling/reconnect proposals remain outside its scope.
