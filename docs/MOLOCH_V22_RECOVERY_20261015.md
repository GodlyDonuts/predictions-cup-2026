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

## Production launch

The later V21 stop was precisely identified as `LIVE_EXIT_REFRESH_STREAM_FAILED:STREAM_READ_FAILED` at 12:37:59.170 UTC. This is an allowed V22 market-data recovery class; the old generic error did not reveal it. The root verified all162 physical submission attempts were acknowledged,242 retained queue intents, no unknown/active admission and no manual review before archiving that exact STOP.

V22 started at12:44:56.495947 UTC as PID179819 from checked publication `8902b8c5`. The financial bridge file remained byte-for-byte unchanged. Bank, attributed lots, actual order/fill history, queue v1, account rate/backoff, financial risk and original entry deadline were preserved. Unit validation passed. The service memory cap was raised from512MiB to1GiB after a536,989,696-byte peak at the prior536,870,912-byte cap, with6,780MiB available on the VPS at the check. Memory pressure as the cause of the socket read failure is unproven.

At12:46:16 UTC the service was active with the new1GiB limit while replaying retained history. That process observation alone does not establish a healthy complete cycle, fresh exchange order or an observed automatic reconnect.

## Fresh verified production activity

At12:49:51 UTC, PID179819 was healthy with no execution STOP. Nine new V22 physical order acknowledgments were bound to official HTTP200 receipts. Five additional actual fills belonged to those newly acknowledged physical orders; each retained official owned-fill receipt had complete coverage and no remaining page. The native ledger had171 total acknowledgments and41 actual fills. [Sanitized launch receipt](../research/moloch-stream-recovery-20261015/rollout.json), [dated order/fill and ledger evidence](../research/moloch-stream-recovery-20261015/live-health.json).

The derived ledger at12:49:50.872 UTC reports new trading realized −17.925000 SUSQies and inherited gross17.990000, total0.065000, with zero fees. This is attributed lot accounting, not an official whole-account or leaderboard profit claim. Higher order activity has not demonstrated a positive edge; risk caps remain unchanged. The export reads several immutable receipts and separately dated live files, so it explicitly does not claim an atomic account snapshot.

The persistent recovery budget existed with zero recorded attempts. No natural automatic reconnect had yet been observed. Recovery correctness is covered by the targeted checks and independent review above; uninterrupted future uptime is not established by this observation.
