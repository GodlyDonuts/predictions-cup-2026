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

The locked ARM64 release passed at19:36UTC. Its binary SHA256 is `2c7c14d38d10a1fd1e44769f80b92f30ab2216d37473ee5b2c3d3eec8a9c4546b`. Controlled rollout is pending at this checkpoint. This candidate is built from checked main in a separate directory. The incomplete Paper2 financial-package engine is excluded. Native financial source, attributed bank, existing bridge, queue version1,40,000 total and10,000-per-market limits, daily authorization and strategy are unchanged. A controlled version change must still verify the exact old process/binary, durable stopped boundary, quiet queue, checked new binary and fresh live activity.

This repairs availability; it does not establish profitability. Paper2 package execution and its remaining strategy/accounting work are tracked separately in [task143](https://github.com/GodlyDonuts/predictions-cup-2026/issues/143).
