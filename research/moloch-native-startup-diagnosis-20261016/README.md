# Native startup diagnosis — October 16

Read-only source audit requested after the execution owner observed approximately 18 minutes restoring a 952 MB / 308,000-event bank. That latest timing is owner-reported, not independently profiled here. No compiler, account client, queue, live Session or production operation was run. Audited native code is the isolated `4b9dba40` baseline underlying the runtime-only fill feedback patch; line numbers below refer to that source. No native changes are proposed as already checked.

## Existing recovery does not expose a safe live snapshot shortcut

`session.rs:320–370` reads every original row, checks its chain, restores and checks source references, applies the unpublished engine, checks each upgrade boundary, checks the durable head, and only then publishes the reconstructed head/report. The consuming `advance_replay_unpublished` call at line343 already removes the old per-event transactional Engine clone. Do not propose that already-shipped fix again.

`report.json` is a projection, not a complete restart image. `legacy::load_snapshot` (`legacy.rs:62–84`) verifies a legacy financial continuation and invokes `Engine::from_seed_state`; this is not a general live Engine deserializer. `engine.rs:483–509` explicitly rejects owner capital, market capacity, exclusions, reviewed entry/exit reservations, collateral, packages, daily windows and unresolved orders. It does not restore current books, all live bindings, all independent private lifecycle state and source-reference maps as a live restart checkpoint. Using it for this bank would fail, or require a new audited checkpoint design; relabeling the report as a snapshot would discard authoritative state. Collateral checkpoint events refresh cash evidence, not replay checkpoints.

The twelve upgrade entries do not cause twelve full journal replays. `UpgradeChain::check_prefix` (`session.rs:170–179`) returns early until each exact byte boundary, then compares the original prefix hash/head/report/orders/fills/cursors/bindings. These original checks must remain. Report serialization happens at those finite boundaries and final publication, not for every replayed row.

## Concrete history-dependent work

| Path | Repeated work | Implication |
|---|---|---|
| `engine.rs:1644–1648`, `1137–1145` | Every unique trade appends `(executedMs,quantity)` to an unpruned per-exchange `flow` vector. Every eligible fresh flat book's target computation rescans its entire historical vector and filters to the last900000ms. | Genuine cumulative historical-trade work, potentially quadratic across a growing replay. This is the smallest newly identified candidate. |
| `engine.rs:1817`, `1206`, `837–858` | Most ordinary events run reconcile; desired scans current books and, for held scopes, historical orders. | Replay repeats planning, not just JSON parsing and ledger arithmetic. Skipping planning would change native orders/counters and cannot preserve original replay. |
| `engine.rs:562–573`, `736–749` | Every event audit computes pledge/opposite BUY totals by scanning all historical orders for each owned-side key. | Approximately events × historical orders × owned-side keys; retired history remains in the scan. |
| `engine.rs:1218–1295` | Reconcile scans all orders for cancellation, market occupancy, per-market funding and target-key existence. | Additional history-dependent scans even with few physical orders. An active-order index could preserve semantics but has more lifecycle mutation sites than the activity-window fix. |
| `journal.rs:13–41`, `session.rs:375–399` | Parse original JSON, clone/hash canonical row body, clone event, then clone/hash its source-free body and retain source identities. | Linear original-byte work plus logarithmic map lookups. No source evidence here of whole-prefix hashing on every row. Removing these checks is not an acceptable optimization. |

`wallet.audit` (`lib.rs:326–342`) checks current reservations, current owned lots and the cash/owned/realized identity; it does not replay all historical fills every event. The actual fill count is small relative to original public events. Package audit can search historical orders per package leg, but this audit has not measured live package cardinality, so it is not asserted as the present primary bottleneck.

## Smallest candidate after stable production

Prune **only private activity-vector entries** that already fail the existing `at - executedMs <= 900000` predicate, using the monotonic processed-time clock, before target evaluation. Keep the exact equality boundary and original execution timestamps. Preserve permanent `seen` / `legacy_seen` identities and every original journal/source event, including late-arriving old trades. An old trade can remain fully deduplicated/conflict-checked while contributing zero to the current fifteen-minute activity rank, exactly as today. Never substitute receipt time for execution time.

Because all future processed clocks are nondecreasing (`engine.rs:1477–1483`), an entry outside the window cannot reenter. Thus the activity sum, floating rank inputs and target tie order can remain exact. The rolling activity vector is an internal optimization, not dropped evidence or a relaxed authority check. A first minimal implementation could retain against the same predicate once per event or touched exchange; measure whether an ordered queue/index is actually needed before adding it. Existing execution times may arrive out of order, so a simple pop-front-only queue is not automatically correct.

This changes included native source bytes: `session.rs:113–138` hashes `engine.rs`, `session.rs` and fourteen other inputs with separators. Therefore even a semantically equivalent implementation requires a new candidate identity, exact original-prefix parity and the owner's explicit bridge. There is no existing same-identity snapshot switch that safely eliminates replay. Do not use migration/reset/compaction to obtain the speedup.

## Required checks and measurement

1. Exact per-prefix old/new report, orders, fills, valid raw books, live bindings, required scopes, package state, source cursors and event order on a finite frozen original prefix; verify all twelve original upgrade boundaries and final report seal.
2. Synthetic exact900000ms inclusion /900001ms exclusion, late and out-of-order executions, duplicate and conflicting trade IDs, repeated books/ticks and held/flat scope transitions. Retained historical evidence must still reject conflicts after activity eviction.
3. Compare candidate target ranks, chosen order terms/qids, write-budget counters and failure behavior at every synthetic prefix; do not merely compare final cash.
4. On one identical original sealed input, alternate old/new optimized offline runs and report replay elapsed/CPU/RSS and event-type/history cardinalities. Attribute parsing/hash, engine apply/planning and audit separately with nonfinancial offline instrumentation if necessary. Do not assume a speedup magnitude from code inspection.

Existing checked evidence is older and narrower: the [unpublished-staging audit](../moloch-throughput-20261016/native-hotpath-audit.md) replayed a555,521,087-byte prefix in202.499s before comparing64-event in-memory staging; removing an inner clone reduced that component's median787.196→264.785ms. The [Session batching benchmark](../moloch-live-latency-20261016/original-prefix-benchmark.json) records430,963,210-byte restart parity and approximately132s restore. Neither demonstrates the latest18-minute hot-path proportions or predicts a whole-worker improvement. The separate descriptive Python lifecycle replay does not execute native quote planning, so its faster time is not a replacement native restart benchmark.

Recommendation: first test activity-window retention; then, only if measured still necessary, a derived active-order index preserving original order iteration and every expiry/cancel/terminal/package/review transition. A trusted full-state checkpoint with original-chain verification is a larger separate feature, not the minimal remedy.
