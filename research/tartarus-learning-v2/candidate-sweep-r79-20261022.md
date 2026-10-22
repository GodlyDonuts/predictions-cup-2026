# R79: scan the next candidate after a rejected read

R79 was installed October 22 at 00:58:31 UTC and verified live at 01:18:53 UTC as sole worker 1056637. Its complete original financial prefix and all 493 earlier receipts were verified unchanged. [Fresh proof](r79-fresh-live-20261022.json).

The 01:26:22 receipt audit confirms one new filled order: 558 NO1077 bought at 0.855, first filled at 01:19:59.288. Two partial receipts count as one physical order. No new realized gain occurred. The one-filled-order-per-minute target remains unmet. [Audit](r79-turnover-20261022.json).

Live diagnostics show a completed five-candidate sweep. They also expose obsolete managed maximum-age nominations repeatedly preempting current reviewed exits; R80 is qualified to correct that separate routing problem, not yet deployed. [Diagnosis](managed-age-r80-20261022.md).

The ordinary dispatcher previously considered at most one read nomination between complete source, reconciliation, package/rank and reporting cycles. A buy rejected by its book prefilter therefore delayed the next candidate until another full cycle. The recent completed scope gaps had median55.926seconds, including read-only failures and a224.096-second stream-recovery gap. These observations do not isolate each component's cost.

R79 keeps scanning candidates in the existing worker after a read-only pass, until the cursor repeats or does not advance, no candidate exists, current submission permission is absent, or the dispatch journal changes. Any intent, attempt, acknowledgment, cancellation, no-network outcome or other journal mutation ends the sweep. An uncertain attempt is never treated as a harmless rejection to retry. Even benign journal changes end the sweep conservatively.

Before another pass, STOP/stream controls and entry windows are checked again, pending reference reloads are served, and ordinary account reconciliation is refreshed when due. Permission is recomputed after paced reads. Each physical candidate still obtains its own original account/contract/book packet and final SDK authorization. No physical permit is reused. All requests use the same serialized Transport and persisted account limiter; no additional client, new price rule, quantity rule or financial authority is introduced.

The new diagnostic records sweep passes and cursor count separately from actual fills. No claim is made that more examined candidates will produce more fills or profit. More candidate reads can also consume account capacity that would otherwise serve breadth discovery; live results must establish the tradeoff.

All238 worker checks passed, including three new cases for rejected-read continuation, cursor-cycle termination, and stopping without an ACK after a journal mutation or loss of permission. The optimized ARM64 executable loads and hashes to `300e04fe90b7d7daa306652b42c259c84540a72cca350cc62edc3dbc0a7555f9`. Only the worker source changed. Its SHA256 is `b00b712143d03805317b9947740159453cc01f59cdc8a68e1fd555dbd6467462`; the R78 runtime and original native libraries are reused exactly. [Qualification](r79-candidate-sweep-qualified-20261022.json), [build](tools/build-candidate-sweep-r79-20261022.py).

The old worker drained through its existing STOP path. The original financial state, native engine, shared limiter and daily windows remain unchanged. File-only startup verification succeeded. The 15-minute scope and 60-minute exit observers are measuring the live worker. Installation is verified separately from fresh execution, fills and profitability.
