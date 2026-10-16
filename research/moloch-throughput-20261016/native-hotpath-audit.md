# Native hot-path audit, October 16

Source baseline `ea733f6f`; read-only retained evidence from the [owned-exit audit](owned-exit-latency-audit.md). No account requests, queue commands, active profiler/ptrace, service mutation or live Session were used. A separate isolated Session candidate is described below; it does not change the current owned-read release's source or builder.

**A redundant transactional clone is present inside already unpublished Session staging.** Removing it is the smallest faithful candidate. The code evidence does not establish that this accounts for the observed69.5% process-average CPU or11.412second source delay. Read pacing, source parsing, durable projections, native historical scans and other work remain.

## Concrete current costs

The retained sealed report at sequence182099 has4,563 order records,4,536 retired,131 actual fill rows,768 actual order bindings and8,259 unique public trades. The order-record JSON alone is1,376,590 canonical bytes; the complete original report file is1,446,064bytes. The independently verified8,047-row original journal tail has5,959books,1,436gaps and224trade observations. These are actual retained sizes/counts, not CPU profiling measurements.

| Path | Work and scaling | Current protection/limitation |
|---|---|---|
| `native/src/session.rs:479` → `engine.rs:1453` | Single accept clones the entire Engine, then public `advance()` clones that unpublished candidate again. | Both copies include historical ordinary order records, actual fills/bindings and public-trade identity map. Engine books already use `Arc<Book>`. |
| `session.rs:535` | Batch clones Engine once initially, then calls cloning public `advance()` for **each** accepted event. A32-event batch therefore starts33 Engine clones. | Existing event validation/audit remains necessary; the extra rollback copy is unnecessary when the whole candidate is discarded on error. |
| `native/src/source.rs:381,553` | REST accept and every broadcast clone the entire RestSource transactionally. `scopes[*].book:Option<Value>` deep-copies stored raw books; `facts:BTreeMap<String,TradeFact>` clones all remembered print facts. | Source facts are immutable conflict evidence and unpruned. No source-clone change is included in the candidate. A whole-map `Arc::make_mut` alone would still copy the map on every insertion. |
| `engine.rs:557,829,1194` | Each event audits wallet/share pledges and reconciles targets. Pledge/opposite-BUY calculations repeatedly scan all order history; target selection and cancellation also inspect historical orders. | An active-order index could help later, but would require exact retired/cancel/partial/package parity. Do not skip financial checks or prune authoritative history as a performance shortcut. |
| `session.rs:547` and `atomic_json():49` | Each durable commit renders/hashes the full report, order history and fills, then serializes to disk. Batch prevalidates serialization to sink too. Journal fsync plus two atomic projection file+directory syncs remain. | The report is an intentionally durable current projection. Deferring it changes failure/publication semantics and is outside this narrow candidate. |
| `worker.rs:1113` | A drain handles up to64 frames; each frame may contain many markets/events and does Recorder/Universe/source work before admission. | A frame bound is not a wall-time or event bound. No source events may be dropped or future frames parsed against an incorrectly assumed admitted prefix. |

`Engine.seen` is an ordinary `BTreeMap` retained for conflict/deduplication; `orders:Vec<Order>` and `fills:Vec<Value>` also deep-copy. The earlier book `Arc` optimization does not make an Engine clone independent of history. Session's source-reference map itself is not cloned for each event, but the remembered source facts consume retained memory.

## What batching already does

`worker.rs:83–116` prepares original event clocks, packs at most32events/8MiB via `Session::accept_batch`, and observes each actually committed individual event afterward. Across-frame queued staging flushes on the event/byte bound and at drain exit. Each event retains its original row/hash/source cursor; this is not an atomic all-or-none journal batch. The worker does **not** fsync/publish one complete report for every event inside a full32-event batch.

Existing checked ARM64 evidence is relevant but not a new profile:

- [Synthetic batch fixture](../moloch-session-batch-20261015/benchmark.json):474events, serial3.553s/474commits versus batch.301s/16commits; exact original journal/head/report bytes and restart parity. Fresh small-history fixture; ordering/filesystem-cache bias disclosed.
- [Original retained-prefix fixture](../moloch-live-latency-20261016/original-prefix-benchmark.json):64consecutive original events on a3,405order/118fill bank, serial31.899s versus batch1.622s, exact files and restart parity. This is already committed batching benefit, not a forecast for the new candidate.
- [Earlier Arc-book benchmark](../moloch-engine-book-cow-20261015/engine-clone-arc.json):237markets×20levels but only26warm orders/no fills. Clone~.043ms and tick~.248ms there cannot represent today's4,563order/8,259trade state.

## Narrow candidate and validation

Only `native/src/session.rs` staging changes: clone the currently published Engine once, then consume that **unpublished** owner through the existing `advance_replay_unpublished()` API. It still calls exactly `apply()` and `audit()` on every event. Public `Engine::advance()` and its rollback contract remain unchanged. A rejected event drops the entire staged owner, including any earlier candidate ACK/fill mutations, before I/O or publication. Successful canonical event order, hashes, source references/cursors and projection contents are unchanged.

Existing durability semantics must remain explicit: after a filesystem failure a complete or partial journal prefix may already have been written. The Session remains poisoned; the source owner must restart/reconcile the validated serialized prefix. Neither existing code nor this candidate promises to undo disk writes that already succeeded.

Five focused tests compare live Session prefixes against public transactional Engine behavior, actual ACK/partial-fill/terminal serial-versus-batch exact bytes and restart, final-invalid-batch rollback after candidate cash mutation, failed individual admission with reusable source sequence, and poisoned head-publication recovery. Tests are source-only until the parent's ARM64 run.

Before any future immutable release: run all native/runtime checks and the existing batch benchmark on old/new candidates; compare the **same original prepared64events on the same sealed copied bank prefix**, including all financial/report/orders/fills/cursors/journal bytes, original failure/restart cases and wall/resource timings. `session.rs` participates in native identity, so require an explicit checked old-prefix bridge. No benchmark or bridge may alter the live original bank. Broader RestSource copy-on-write, active-order indexing or report-publication changes require separate scope and measurements.
