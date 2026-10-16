# Bounded restart review — 2026-10-16

**Conclusion:** a small systemd change can safely retry process failures while respecting persistent STOP. It cannot yet provide automatic OS-crash recovery: the V1 SDK's pathname lease survives abrupt termination. Do not claim that a restart directive solves that blocker, and do not delete the lease from an `ExecStartPre` shell command.

Scope: read-only source and actual user-service inspection; no SIG requests, queue inspection/commands, unit/runtime/control edits, service actions or failure injection. Review baseline is `e7bb62d1fe20dbce5fd54e35134a569f686e9409`. The observation below is an inventory, not a fresh trading-health or profitability assertion.

## Actual service observation

Read-only SSH `systemctl --user cat/show moloch-live.service` and `loginctl show-user archon -p Linger`, completed before11:42:53UTC, returned:

| Property | Observed value |
|---|---|
| MainPID |494293|
| Type / restart |simple / no|
| Current restart delay / start limit |100ms /5starts per10s|
| Stop behavior |30s; KillMode=control-group; KillSignal=SIGTERM; SendSIGKILL=yes|
| Boot/session persistence |unit enabled, WantedBy=default.target, Linger=yes|
| Sandbox |UMask0077, NoNewPrivileges, PrivateTmp, ProtectSystem=strict, ProtectHome=read-only|
| Resource limit |MemoryMax=1G|

The unit starts the immutable `paper6-d0a3113c3cc3/moloch-live-runtime` release with the existing full-account-v2 control. Existing writable paths are the original live root and capture.lock. No separate Moloch execution supervisor appears in this unit. The Kalshi collector's supervisor is a different public-data process and is not an execution recovery mechanism.

## Smallest unit proposal — not installed

Add a drop-in with the following content, preserving the original ExecStart, control, sandbox, locks and limits:

```ini
[Unit]
ConditionPathExists=!/home/archon/apps/moloch/shared/live-production-v1/EXECUTION_STOP
ConditionPathExists=!/home/archon/apps/moloch/shared/EXECUTION_STOP
StartLimitIntervalSec=3600
StartLimitBurst=3

[Service]
Restart=on-failure
RestartSec=60s
```

This is a rate bound of three starts/hour, not a lifetime or durable cross-reboot restart counter. Hitting the start limit stops automatic attempts; do not add a timer that resets failed state or repeatedly starts the service. Explicit manual `systemctl stop` is not automatically restarted by this policy. To preserve an operator stop through reboot, retain the existing durable STOP flag; enabled+linger otherwise starts the service at user-manager boot. Conditions are tested before ExecStart and skip startup without marking failure when either flag exists. Removing a STOP does not itself schedule a new start.

These semantics are documented by the primary [systemd v255 service manual](https://github.com/systemd/systemd/blob/v255/man/systemd.service.xml) (`Restart`, including manual-stop exception) and [unit manual](https://github.com/systemd/systemd/blob/v255/man/systemd.unit.xml) (`ConditionPathExists`, start-limit behavior). This is an appropriate small candidate for operator validation, **not complete crash recovery**.

## Source-backed failure matrix

| Trigger | Existing source behavior | Proposed restart outcome |
|---|---|---|
| Local or global durable/manual STOP already present |worker checks STOP at worker.rs1410, but only after account preflight reads at1402–1408|Unit conditions skip the entire executable, including those reads. No flag deletion.|
| Normal error within main execution closure |worker.rs1575 attempts durable local STOP and stopped status; main.rs exits1|The next activation is skipped by STOP. No automatic recovery of that deliberate safety stop.|
| Bad configuration, missing credential/lease, startup read failure before the closure |worker.rs1389–1410 propagates Err before its STOP-writing result wrapper; main.rs exits1|Up to configured start-rate bound; original controls, scope and shared limiter remain. This is not an order retry.|
| Ordinary durable attempt has no verified ACK/no-network proof after restart |dispatch.rs82 rejects via DISPATCH_UNKNOWN_ACCEPTANCE_MANUAL_RECONCILIATION; worker.rs1431 requires it before constructing continuing write Transport|No replacement order; startup becomes a durable STOP if it reaches this guarded phase.|
| Crash with SDK write admission still active |State::recover, account-queue/src/main.rs114–120, moves its binding to unknown rather than assuming not sent|If lease recovery later permits opening, persisted uncertainty still blocks new submissions. Never clear it in a supervisor.|
| Package attempt/unknown without definitive source outcome |package_worker.rs135–166 preserves unresolved acceptance and blocks submissions; corresponding signed/probabilistic recovery paths remain authoritative|Restart does not recreate a staged send or retry an old package key.|
| Rust panic with normal unwinding |Queue Store Drop can remove its own exact lease; Session/capture OS locks release|A bounded restart can replay and revalidate; any unresolved ordinary/package attempt still blocks. Not guaranteed for panic=abort or teardown I/O failure.|
| SIGKILL, OOM abort, OS power failure, default signal termination |Drop is not guaranteed; V1 queue lock pathname remains|Restart is attempted but Transport fails EXCLUSIVE_QUEUE_LOCK_REQUIRED. **Availability blocker remains.**|
| Human systemctl stop |systemd does not automatically restart a service it explicitly stopped|Stays stopped until an explicit start; use persistent STOP for cross-reboot intent.|
| STOP persistence fails, e.g. disk full |worker propagates persistence error; no success/health inferred|Bounded attempts only; queue/journal/identity failures retain their original fences. No cleanup or reset.|

`Type=simple` means the existing90s startup timeout is not a limit on ledger replay readiness: the process is already considered started. An active PID therefore remains distinct from fresh restored trading health. Do not add a guessed watchdog that kills long replay or turns a slow preflight into new uncertainty.

## Concrete missing crash-recovery primitive

`backend-rust/transport-service/src/admission.rs75–83` selects the actual V1 Store directly. `backend-rust/account-queue/src/main.rs165–170` uses `create_new(true)` for `offline-account-queue.lock`; it does not acquire an OS lock on that lease descriptor or recover a stale pathname. Its Drop at249 only removes a byte-identical lease during normal teardown. OS release of the separate worker capture lock does not remove this file, prove historical acceptance, or authorize another writer. No stale-owner fallback was found in this call path.

The smallest subsequent **source-reviewed SDK task**, outside this documentation scope, is an explicit owner-death recovery operation under the same exclusive capture owner, with exact original account scope, lease descriptor/path identity, boot/process-start identity, immutable source snapshot and current queue-journal verification. It must reject a live/ambiguous owner and symlink/replacement races; preserve original lease evidence; retain all active/unknown bindings, Retry-After, admission indices and limiter state; and let existing recovery convert interrupted writes to unknown. PID absence or file age alone is insufficient, especially across reboot/PID reuse. A durable OS-held queue-controller lease is an alternative design that requires an explicit migration bridge for the old pathname contract. Neither design should clear unknown acceptance or grant resend permission.

Before deploying a complete uptime solution, run synthetic process tests outside production: STOP skips ExecStart; a normal error that writes STOP is not restarted; an unwinding panic restarts without a duplicate send; SIGKILL during idle/read/write requires positively verified owner recovery and preserves unknown writes; late ACK/partial fill survives restart; scope/lease replacement, clock regression and torn queue tails remain blocked; manual stop and bounded start-limit exhaustion remain stopped; reboot with STOP does not launch. These checks have **not** been run in this read-only task. Root remains the sole deployment owner.
