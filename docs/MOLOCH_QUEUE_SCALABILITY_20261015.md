# Queue scalability audit — October 15

The queue is the growing storage bottleneck. At09:40UTC, the execution owner observed98 permanent intent/key bindings,39,628 bytes of state and a67,108,252-byte active journal. A rotation at09:30 had70 bindings: approximately64MiB of full snapshots accumulated in ten minutes while the permanent state grew. Capacity-triggered maintenance restores short-term operation; it does not remove this amplification.

[Task #129](https://github.com/GodlyDonuts/predictions-cup-2026/issues/129) measured the unchanged Rust core using a separate, synthetic ARM64 VPS fixture at nice10. No account calls, production queue accesses or Store/SDK/worker changes occurred. All benchmark jobs and the SSH session ended. The [sanitized results](../research/queue-state-amplification-20261015/results/arm64-20261015.json) contain source/artifact hashes, measurements and explicit model assumptions. The [reproducible harness](../research/queue-state-amplification-20261015/README.md) passed36 Rust checks and a locked offline release build; three Python checks match the size projection to every record of small deterministic traces. Raw synthetic archives remain private; no production history was removed or compressed.

## Measured cost

Each queue operation clones the full State, hashes its full serialization and writes another full serialization. Both permanent maps retain their bindings. Status also constructs a full JSON result. With synthetic29-byte intent/key identities, each additional binding adds390 bytes to every later record. A first-entry comma difference is handled explicitly; this coefficient was checked at1,2,70,98 and1000 bindings.

| Retained bindings | State bytes | Clone/hash/serialize median | Persist including fsync, two samples | Status including fsync |
|---:|---:|---:|---:|---:|
| 98 | 40,205 | 0.554 ms | 1.642–7.821 ms | 1.964 ms |
| 1,000 | 391,985 | 5.472 ms | 7.539–7.684 ms | 10.049 ms |
| 10,000 | 3,901,985 | 61.569 ms | 64.167–68.507 ms | 94.360 ms |

These are wall-clock measurements under a live host and nice10, not isolated process-CPU counters or stable latency percentiles. CPU-work samples numbered seven per point; fsync had only two. The98-binding synthetic size is about1.5% above the observed state. Actual identity lengths, rate histories and scheduling vary. The larger100,000-binding case below was compression-only; CPU sample points stopped at10,000.

## Twenty-day storage model

The requested100 reads/minute plus1/5/24 contest writes/minute cannot coexist under the current shared600ms read/2500ms write completion gaps, even with zero HTTP latency: they consume62.5/72.5/120 seconds per minute. Pacing-only read upper bounds are95/79/0; real HTTP, reconciliation and inclusive rolling windows reduce them further. The zero-read case is a mathematical ceiling, not a usable trading schedule.

The following deterministic model starts at98 bindings, retains every new binding and counts the minimum three local records per admitted operation. Identity widths are fixed synthetic assumptions. It excludes checkpoint copies, status/wait records, active/rate-array overhead, identity growth and sequence digits; total storage is larger. It is a capacity stress calculation, not a production forecast.

| Writes/min | Pacing-only read ceiling/min | Twenty-day journal demand at that ceiling | Raw disk budget reached | Demand at offered100 reads/min |
|---:|---:|---:|---:|---:|
| 1 | 95 | 46.9 TB | 1.23 days | 49.3 TB |
| 5 | 79 | 204.1 TB | 0.63 days | 255.1 TB |
| 24 | 0 | 279.6 TB | 0.55 days | 1,444.4 TB |

TB is decimal. The budget is the measured211,514,302,464 free bytes, approximately197GiB, rather than all server capacity. These modeled times exclude new unrelated disk use. Demand grows approximately quadratically because the retained map grows while it is repeatedly rewritten. Free space must be monitored from actual source bytes; repeating today's bytes/minute for twenty days misses that growth.

At the fixed-width model, a single v1 record exceeds64MiB at172,073 bindings, approximately4.98 days at24 writes/min. Compaction cannot make that record fit. Its bounded second failure must remain fail-closed. Projections after this point are explicitly counterfactual. Linear extrapolation of measured compute-work wall latency also approaches or exceeds a serialized worker's time budget at large maps; it is uncertain beyond10,000 measured bindings and is not a measured CPU forecast.

## Lossless archive compression

Unique deterministic SHA256 payload hashes prevent artificial all-zero compression gains. Every credited result was decompressed and checked against the entire original byte count and SHA256.

| Independent synthetic file | xz8MiB dictionary | xz64MiB dictionary | zstd3/windowLog23 | zstd3/windowLog27 |
|---|---:|---:|---:|---:|
| 16.75MB,70→77 bindings,557 valid records | 386× | 384× | 411× | 415× |
| 39.00MB,100,000 bindings,one valid record | 4.28× | Unvalidated timeout | 4.49× | 5.81× |

Current-cohort zstd long-window compression took about0.042 seconds. The large case took about0.467 seconds wall and0.495 child process CPU seconds with windowLog27, producing6.72MB. The xz large-window batch hit its120-second budget; its partial output was retained but never credited. Independent compression loses temporal redundancy when one large state nearly fills a generation. The early415× ratio cannot justify twenty-day retention. Even the pacing-only journal models require approximately222×/965×/1322× average compression to fit the current free budget, before checkpoints. Solid cross-archive groups could exploit more redundancy but were not measured or implemented; no saving is assigned to them.

Source inspection and a synthetic restart test confirm v1 startup loads its current checkpoint and active tail without recursively opening historical anchor sources. That is not permission to remove them. A proposed archiver must exclude the active journal, current manifest, current checkpoint and its immediate anchor.source_journal. Only older immutable files may be considered. It must create a new archive, independently round-trip-check raw size/hash, pin archive and index durability, and append a hash-chained index mapping original basenames to codec/version/parameters, lengths and hashes. Recheck the protected set under the existing owner lock before any approved plaintext removal. Restoration must recreate and verify original bytes/names before selecting historical manifests. Failures leave the source intact. No archiver or removal is implemented here.

## Narrow proposed migration, requiring parent approval

Compression is an interim storage measure; it leaves full-state cloning, hashing, write bandwidth and the one-record cap intact. The structural fix should stay in the existing core and queue namespace:

1. Add a v2 reader while retaining the strict v1 reader. Migrate only under the existing exclusive owner/queue locks with a verified v1 head, full state and immutable source length/hash. Preserve original files, all permanent intent/key bindings, active/unknown state, clocks, backoff and read profile. No new backend or account client.
2. Append a bounded v2 transition containing sequence/previous hash, the bounded operational state, only new permanent bindings and explicit unknown insert/remove operations. Hash the delta itself. Do not hash/serialize or clone all permanent maps per command; otherwise only disk cost is fixed. Stage existing checks against referenced map entries and commit map mutations only after durable append. Use the unchanged State::apply as the offline equivalence oracle for accepted/rejected commands, clock poison, rate waits, crashes and recovery.
3. Checkpoint periodically or at a bounded delta-tail threshold rather than every near-cap full-state record. Stream full checkpoint serialization/hash. A691,298-binding fixed-width state is about270MB, already above the existing256MiB checkpoint cap; v2 needs an explicit reviewed streaming/sharded checkpoint bound. It must not silently relax v1 limits. A tiny tail can then replay bounded transitions without cloning full maps for every command.
4. Pin checkpoint/new-tail directory entries and atomically switch a schema_version2 manifest before admitting new work. The actual old-reader regression proves the current v1 binary refuses that manifest and does not fall back to the older journal. Downgrade requires a lossless export of the CURRENT v2 head and state, never selection of a stale v1 manifest.
5. Keep full status available as an explicit diagnostic, but approve a bounded runtime status query rather than serializing all historical bindings on every poll. Archive/index policy remains separate; history is preserved throughout.

Before a core implementation is authorized, require deterministic v1/v2 State-and-response parity, partial append and every checkpoint-switch fault, active network-write recovery, unknown/idempotency conflict preservation, clock/backoff equivalence, failed directory pins, and old-reader refusal. Compare actual delta bytes, CPU-work latency, replay time and retained disk on the same fixture before any deployment. This audit proposes that work; it does not change the live format.
