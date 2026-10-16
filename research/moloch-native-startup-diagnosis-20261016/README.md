# Native startup diagnosis — October 16

## Owner validation at20:19UTC: compiled, original-prefix comparison running

The isolated activity-window candidate now passes actual ARM64 **77 native
library tests**, including all five new window/identity/parity tests. One existing
explicit original-prefix benchmark remains ignored by the unit invocation; the
separate original-prefix run below is its own evidence. Optimized native release
and both actual compiled identity probes pass. Candidate identity is
`0ba143798b09a09c6d685d3804e6c49e9e09ffa0b9e9ec406167aa4d89e0b25d`;
production retains native8acc and binary1180882d. This is not a built successor
production worker or an approved live engine migration.

The same helper source is linked to exact optimized baseline/candidate artifacts,
with matching native profiles and `serde_json` arbitrary-precision features.
R1's evidence helper incorrectly treated read-atime changes as source mutation;
R2 retained that failure and corrected stable identity checks. R2's candidate
probe link then failed because the primary library is in `release/` while its
transitive libraries are in `release/deps/`. R3 adds the actual dependency artifact
directory. Original failed logs remain; no failed invocation is reported as a pass.
[Exact build evidence and pins](validation/artifact-pins.json).

The owner froze the original308166-event /952440605-byte bank and all twelve
original bridges, exact sixteen source inputs, optimized probes and compiler
evidence before either outcome. At20:14UTC one reduced-priority detached
baseline-first comparison started. Baseline PID544702 is observed running; no
parity result or speed benefit is yet claimed. Only a candidate **copy** may
receive a thirteenth offline bridge after the baseline exits successfully. The
live worker continues independently. One pair remains cache/CPU confounded.
[Frozen input declaration](validation/benchmark-input-manifest.json).

The read-only audit and uncompiled handoff notes below are retained as history;
the dated owner evidence above supersedes their compilation status.

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


## Separate prospective candidate: touched-exchange activity eviction

The candidate changes only `engine.rs`'s existing unique-trade append block: append the original tuple, then retain exactly `at - executedMs <= 900000` in that exchange's vector. It does not scan all237 exchange vectors on every event. A quiet exchange retains the finite window present at its last unique observation and cannot grow until another unique observation; the existing target-time filter remains unchanged. Permanent trade/source identity maps are untouched. This is a different native identity and is **not** part of the runtime-only BUY-tie recovery release.

Five inline tests are authored: exact window equality/next millisecond and quiet-exchange independence; late/out-of-order observations with permanent duplicate/conflict rejection; future cache predicate retention while future source events and clock regressions still fail; exact full-history versus pruned target rank bits and reconcile report/order/fill/book/binding/package parity at36 prefixes; legacy original-provenance duplicate retention. The target comparison deliberately reinstates the complete original activity tuples in the reference snapshot, rather than fabricating a new source clock. These are focused synthetic tests, not the required full original308k-event/12-bridge parity proof.

`git diff --check` passes. No local Rust toolchain is available; tests are **authored/uncompiled** at handoff. No performance run, full original-prefix comparison, bridge or deployment was performed. The execution owner must compile/run the tests, compare exact original-prefix old/new outputs at every existing upgrade boundary and final head/report, and measure matched optimized replay before considering promotion. The previously identified historical-order scans remain even if activity eviction is effective.


## Offline-only one-pair harness: review before running

`offline_probe.rs` is a separate binary linked to one pinned optimized native build. Its only library calls are `engine_identity`, `propose_live_engine_upgrade`, and `Session::open_live` on detached copies. It never invokes the production worker executable or any account, queue, HTTP or source-admission API. Build the IDENTICAL helper source against baseline and candidate crates in separate NEW private Cargo/target directories, with native path dependency and `serde_json = "1"`. Capture actual Cargo compiler-artifact metadata, compiler/version/flags, source and probe hashes. This helper is uncompiled here.

Owner-reviewed command (all fields must be genuine final pins):

```
python3 -I -B /PRIVATE_REVIEWED_SOURCE/benchmark.py /PRIVATE_APPROVED/benchmark-manifest.json ACTUAL_MANIFEST_SHA256
```

There are no built-in placeholder successes. Required manifest fields:

- `schemaVersion: 1`, `approvedOfflineOnly: true`, `productionAuthorization: false`, `frozenRoot` under `/tmp/moloch-native-flow-frozen-...`, unique create-new `outputRoot` under `/tmp/moloch-native-flow-benchmark-...`, actual `expectedSequence`, `expectedBridgeCount: 12`, `maximumProbeSeconds` between1 and3600, actual `offlineApprovedAt`.
- `files`: exact keys `manifest.json`, `head.json`, `report.json`, `events.jsonl`, `engine-upgrade-v1.json`, each actual `bytes`/`sha256`. Owner supplies the previously frozen detached bank; driver never copies/truncates/reads production bank. Every input must be regular, single-link, stable and bounded.
- `baseline` and `candidate`: `nativeSourceRoot`, exact16 ordered native `sourceFiles` entries from `benchmark.INPUTS` each byte/SHA pinned, actual `engineIdentity`, pinned `probePath`/`probe` and `buildEvidencePath`/`buildEvidence` (each bytes/SHA), `optimizedProfile: "release"`. Owner reviews pinned compiler evidence for actual matched flags; the profile string is not independent compiler verification. Only `src/engine.rs` may differ. Raw16-input identity must match the actual compiled probe identity; original twelfth bridge must target baseline identity.
- `oldBinarySha256` must equal the ACTUAL detached baseline probe executable hash, not the running production binary. Then proposal `previousProcessStopped` refers to the successfully exited detached baseline process. Production540259 remains running; this grants no live release or production bridge authority.

All original file and build/source pins are frozen into a create-new receipt BEFORE original reports/bridges are parsed or replays run. Baseline must reproduce original manifest/head/report/events/bridge raw bytes. Only after that process has exited does the candidate propose an offline thirteenth bridge with the existing API. The original twelve sealed entries must be unchanged; the entire original bridge file is also retained byte-for-byte. Only candidate COPY bridge changes. Its Session replay checks all twelve original boundaries plus the thirteenth exact original final report. Final original manifest/head/report/events bytes and separately captured public report/orders/fills/valid raw books/bindings/required scopes/packages/policy bytes must match baseline. This is not serialization of every private cache; all original Session source-reference/chain checks remain active.

Subprocesses run nice19 with explicit deadlines; failures retain exact stdout/stderr/exit and never proceed to the next phase. Scratch headroom8GiB required. Probe files need normal copies if Cargo hard-linked executable outputs. Source and original artifact pins are rechecked after successful replay. No input/bridge/production permission is inferred from a failed run.

One baseline-first pair records open-live elapsed separately from process wall/user/system CPU and explicitly cumulative child RSS high-water. Full-file checks/copies warm caches and candidate proposal warms code; this is NOT cold-cache or matched-cache evidence. One pair cannot establish stable speedup. Copy/proposal/hash time is separate from open-live elapsed, while process timing includes comparison-state serialization. Alternate/repeat only under a separate finite declared plan after correctness.

Seven pure Python guard tests pass locally: `python -m unittest discover -s research/moloch-native-startup-diagnosis-20261016 -p test_benchmark.py -v`. They test immutable bridge bindings, exact source-input set, forbidden production roots and create-new results. They do NOT execute Rust, real original prefixes, Linux file-identity paths, models or production. Actual helper compilation/full-prefix validation remains owner work after review.
