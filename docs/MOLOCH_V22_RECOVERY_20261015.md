# Moloch V22: bounded recovery while retaining financial authority

Execution task: [#120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120). Supporting work: [stream recovery #141](https://github.com/GodlyDonuts/predictions-cup-2026/issues/141) and [exit coverage #137](https://github.com/GodlyDonuts/predictions-cup-2026/issues/137).

## Production remains separate from candidate development

V21 started at 12:22:27 UTC as PID172804. Its first complete cycle finished at 12:28:40 UTC with a healthy stream. At 12:37:05 UTC it remained healthy, with 162 acknowledged physical orders and 35 actual fills: 23 additional acknowledgments and four additional fills since launch. These counts establish exchange activity, not profitable strategy performance. New trading realized was last measured at −3.320000 SUSQies, separate from 17.990000 inherited gross gain. Risk limits have not increased.

V22 is built in a separate source tree. The live immutable V21 binary, bank and account queue are not edited during compilation. A checked upgrade still requires a graceful sole-owner stop, resolved physical attempts, an explicitly quiet shared queue and exact binary/control binding. The financial engine identity is unchanged, so the existing three-entry financial upgrade chain must remain byte-for-byte unchanged.

## Recover only a typed market-data failure

The worker catches only `StreamFailure` through an actual error downcast at a synchronous cycle boundary. A string containing the same error text cannot enter recovery. The seven allowed deadline/socket/read/flush/heartbeat/queue-full reasons are listed in the [recovery policy](MOLOCH_STREAM_RECOVERY_20261015.md). Protocol, JSON, JOIN rejection, consumer-disconnection, panic, source, Session, filesystem and financial write errors remain fatal. An uncertain order is never retried or replaced by this path.

Before recovery, every physical submission attempt must have an acknowledgment, and the actual manifest-selected queue must explicitly show an empty global `unknown` object, `active:null` and `manual_review:false`. Missing or wrongly typed fields deny recovery. STOP and exact control binding are checked throughout. The old socket is dropped and joined before a replacement token is requested; the same Transport and persistent account limiter are retained.

The worker waits in at-most200ms intervals for both the existing60-second authentication marker and the persistent recovery budget. Three attempts in a rolling10-minute window cause an immediate STOP; it does not wait ten minutes and automatically continue. The budget is strictly decoded from at most4KiB, initialized durably before first connect, and persisted before each replacement mint. Corruption, clock regression, or a missing budget after a prior recovery status never reseeds it. A failed mint, connection, JOIN, gap publication or final health check propagates to STOP without a replacement loop.

Only a confirmed JOIN applies actual `RestSource::subscribed` gap events through the durable native Session. Breadth/universe subscription boundaries are recorded, retained trade facts and histories remain, and every REST window keeps the earlier of its old boundary and now-minus60seconds. The replacement's health is checked after all gap/reset publication, then the authority is bound to its new failure signal last. JOIN is not a coverage certificate: normal complete tape and fresh valid book evidence are still required before a scope can quote.

## Repair an actual urgent-exit coverage blocker

Live V21 initially deferred urgent exits with `EXIT_REFRESH_FINAL_BOOK_INVALID`: the target source was blocked after subscription or a delivery gap, and a fresh book alone could not restore missing trade coverage. V22 fetches and durably accepts the actual official trade interval before starting the seven account/contract/book facts, only when that target source is blocked. The interval begins at the earlier retained per-exchange window or source recovery boundary, including when the hint belongs to a different scope than the ordinary read currently being processed.

Incomplete pagination or missing boundary coverage defers the exit. Complete tape may also emit a valid cached book and create a native successor; every emitted event is admitted, but that cached book cannot certify later facts. The final fresh book must match the seven-fact certificate, and the selected new quote retains its original activation and expiry. The original expired quote is never renewed. All account, ownership, reservation, physical-liability, unknown-acceptance and final write checks remain.

## Verification and remaining limits

The final combined runtime passed all189 ARM64 checks: the prior runtime checks, ten recovery policy/budget checks, strict queue/budget-file checks including missing-budget-after-use refusal, a real late-JOIN/earliest-window regression, exhaustion/auth-clock gates, and six actual source/Session exit lifecycle cases. The locked release passed in1m57s. The unchanged native and SDK sources retain their111 and150 passing checks. Six changed/integrated source hashes matched the parent checkout and build host exactly. [Canonical build, source and log evidence](../research/moloch-stream-recovery-20261015/checks.json).

The checked candidate source is `01ed6ef0`; immutable binary SHA256 is `4fe304b0f4e515f1c89551aaaa8d1016d7223a02a8a5e357cfcaeb38927e6b9e`. Financial identity remains `0321e7458ed45da08bca7b030369d4e86ae2a9c0127b39fa7d4d49d33d5d0562`. Initial release compilation was interrupted to add the missing-state guard; only the final full test/release artifacts above approve promotion.

The causal exit fixture first admits a real invalid book and denies certification, then admits actual complete coverage and its cached book, then obtains later fresh facts and checks the same successor's original clocks. Incomplete pagination and a late-starting tape window remain blocked. Source, financial and account records are synthetic in tests; there is no physical HTTP/account end-to-end reconnect test. Independent review found and corrected the urgent path's earlier-window omission and the replacement's missing final-health check. Correctness tests do not establish profitable execution or uninterrupted uptime.

Production promotion and fresh observations must be recorded below. The existing new-entry cutoff remains 2026-10-15 at14:44:34.413 UTC; existing position management continues afterwards. Neither this release nor its recovery budget renews that activation.
