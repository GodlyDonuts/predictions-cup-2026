# V24: bounded stream connection recovery

V23 stopped at 2026-10-15T19:16:14.443Z on `STREAM_DNS` while replacing its stream. The replacement code propagated setup failure directly to STOP. The existing recovery path could recover a disconnected socket, but could not survive a temporary DNS failure during the replacement itself.

The same checked V23 was restarted at19:23:22.995Z as PID206099 after verifying that all354 physical submission attempts had acknowledgments and that the manifest-selected account queue had no unknown acceptance, active admission or manual review. Control, risk, bank, four-entry financial bridge and durable recovery budget were unchanged. At19:31:34.661Z this process had fresh healthy trading progress,355 acknowledgments,354 terminals and86 actual fills. An acknowledgment is not a new fill or profit.

## Candidate behavior

Source `58e37f41` and worker integration `2f895e6a` add a deliberately typed connection-setup failure around **only** `Realtime::connect`. Only `STREAM_DNS` and `STREAM_CONNECT` can retry. The latter includes no acceptable public addresses as well as unsuccessful TCP setup; it does not prove that a TCP attempt occurred. Existing public-address restrictions remain.

Token-mint HTTP errors, credential/Auth parsing, TLS/handshake, JOIN, protocol, durable source admission and order errors are outside this classification. Their original failure handling remains. No order is retried.

Startup and stream replacement now consume the same persistent budget before every token mint or connection attempt. The inclusive rolling ten-minute window admits at most three attempts, at least60 seconds apart, also respecting the existing mint marker. Success and restart do not reset it. Exhaustion stops; it does not wait ten minutes to begin an unlimited retry loop. Persistence failure prevents the next network operation.

The worker checks STOP/control, resolved dispatch attempts and the global queue before every attempt. Waiting checks STOP/control and clock continuity every200ms or sooner. It drops the old socket once and clears prepared write facts before replacement. Failed setup neither restores healthy coverage nor binds execution authority. Confirmed JOIN and durable subscription-gap events precede resumed source recovery and any fresh writes.

## Validation and rollout boundary

All206 matching Linux runtime checks pass, including12 focused retry-policy checks. The focused checks cover charging startup, persisted restart history, shared spacing/exhaustion, boundary clocks, unchanged success budgets and refusal to treat arbitrary error text as retry authority. They do not simulate the production DNS outage end to end.

The locked ARM64 release passed at19:36UTC. Its binary SHA256 is `2c7c14d38d10a1fd1e4769f80b92f30ab2216d37473ee5b2c3d3eec8a9c4546b`. Controlled rollout is pending at this checkpoint. This candidate is built from checked main in a separate directory. The incomplete Paper2 financial-package engine is excluded. Native financial source, attributed bank, existing bridge, queue version1,40,000 total and10,000-per-market limits, daily authorization and strategy are unchanged. A controlled version change must still verify the exact old process/binary, durable stopped boundary, quiet queue, checked new binary and fresh live activity.

This repairs availability; it does not establish profitability. Paper2 package execution and its remaining strategy/accounting work are tracked separately in [task143](https://github.com/GodlyDonuts/predictions-cup-2026/issues/143).

## Production launch

V24 started at2026-10-15T19:37:18.274Z as PID211334. The stopped boundary had366 acknowledged physical attempts out of366,539 retained queue intents, no unknown acceptance, active admission or manual review. The financial bridge, risk limits, daily mandate and account queue version1 were preserved. Startup replay and fresh trading verification are pending; this process launch alone is not a successful live cycle.

## Launch rejected; V23 restored

The V24 process failed at19:37:34.922Z on UPGRADE_TARGET_IDENTITY before historical bank restoration. Unchanged Git source did not establish identical raw build inputs. No financial bridge was altered to bypass that rejection. After rechecking366/366 acknowledged physical attempts and the quiet global queue, the exact prior control and service unit were restored. Checked V23 restarted19:41:58.005Z as PID215346 with the existing bridge unchanged. Fresh trading verification is again pending replay. V24 remains rejected until the build identity discrepancy is independently resolved.

## V25 corrected build profile

The original build embeds a mixed line-ending profile: engine.rs, engine_live.rs and session.rs retain their original bytes; lib.rs, legacy.rs, journal.rs and native Cargo.lock use LF. Applying precisely that profile reproduces the approved engine identity 8520add404082af22ae484409cfcb972a895ffe612dec014c9d668c70b7ca282. The identity was also independently executed from the exact compiled native dependency emitted by the corrected runtime Cargo build (matching thin-LTO linkage). All206 runtime checks and the locked ARM64 release pass again. The corrected immutable release is V25, binary SHA256 5be7240f4e5b2b213a0a32838316f47ecea12af534eb57380059295e4faae165. No fifth bridge or financial policy change is needed. Controlled launch remains pending at this checkpoint.

## Corrected V25 launch

V25 started at 2026-10-15T19:50:28.817Z as PID 218325. The stopped boundary again had 366 acknowledged attempts out of 366, 539 retained queue intents and no unknown acceptance, active admission or manual review. The existing financial bridge, risk limits, daily mandate and queue version 1 were preserved. At 19:54:28 UTC the process was active, restoring history, with no STOP. Fresh trading verification remains pending. The V24 launch and rollback proofs are retained as rejected history; the corrected V25 build profile and launch proofs are in research/moloch-v25-recovery-20261015.

## Fresh V25 trading verified

At 20:01:22.307 UTC, PID 218325 had a healthy stream, no STOP, 373 total acknowledgments, 366 terminals and 86 recorded actual fills. The exact checked binary matches the running process. Seven new original official HTTP200 /orders acknowledgments were verified between 19:59:31.909 and 20:01:21.981 UTC. They are not claimed new fills; the last recorded fill remains 14:55:44 UTC. Full first-cycle status is still pending. [Source-bound aggregate proof](../research/moloch-v25-recovery-20261015/live-verification.json).
