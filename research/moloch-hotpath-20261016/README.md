# Post-restoration hotpath audit — October 16

The clearest measured waste is tiny unbuffered filesystem writes. Native `session::atomic_json` passes a `File` directly to `serde_json::to_writer`; the serializer emits many short writes for every complete report. A bounded buffer can remove these syscalls while retaining the same bytes, file sync, atomic rename and directory sync. This audit establishes a mechanical bottleneck, not profitable trading performance.

The sole worker remained PID494293. No account requests, SDK/controller creation, process tracing, pausing, replay, builds, service changes or financial mutations were performed. Python analysis ran at nice15.

## Finite original evidence

Analysis window: **2026-10-16 11:47:44.050–12:09:14.564608 UTC**, after restoration and capital attribution. The attached aggregate [report.json](report.json) contains exact source byte offsets and SHA256s. It verifies original row seals and contiguous chains inside each bounded suffix; it does **not** independently reverify the preceding full prefix.

- Native journal: bytes725870699–742645201, SHA256 `fbc7fe815b5d35c6a7c0f8d2772076a65f22d016d9b3cd7251bb49c29d3d6dbd`, rows236404–241057. Time-window filtering excludes earlier rows.
- Dispatch: bytes3036004–5132117, SHA256 `f22040a57508fcee494ad83f0a432dc1dd30d47001e263a6619aaf798cb542f67`, rows5993–10099. This suffix was captured later; dated rows are filtered to the same cutoff. Undated records are excluded from time-window counts.
- 722 retained original receipts, each at most8MiB, selected by filesystem metadata and original request/receipt clocks. The report binds their ordered raw-file digests. It contains no raw account/order/fill records, profile IDs or credentials.
- Diagnostic files were read independently, not as an atomic multi-file snapshot. `/proc` samples at12:09:16–26 occurred after the finite event cutoff and are explicitly separate observations.

## Measurements

The first full35-scope trading pass ran from discovery selection at11:47:44.050 to status at12:07:05.172: **19m21.122s**. By12:08:51 the next pass was at3/39 scopes; the process and realtime connection were healthy. Scope selection and processing remain serialized.

| Measured component | Result | Interpretation |
|---|---:|---|
| Original receipt window | 1286.873s | First request start through last receipt end |
| Union of receipt intervals | 211.960s, 16.47% of that window | Network start through validation and durable queue completion |
| Outside retained receipt intervals | 1074.913s | Includes limiter pacing/admission, local processing, serialization, diagnostics and scheduling; not all CPU |
| REST book receipt span,88 | median336ms /p90 421ms | Includes validation and queue completion |
| Owned-fill receipt span,263 | median217ms /p90 269ms | Largest read class,36.4% of receipts |
| Open-order list receipt span,93 | median226ms /p90 590ms | Original complete aggregate receipts |
| Physical attempt→ACK receipt,22 | median761ms /p90 863ms /max1267ms | No censored attempts in this bounded window; includes admission and HTTP |
| Realtime book receipt→processing start,2894 events | median11.383s /p90 25.119s /max37.595s | Actual local stream backlog before native acceptance starts |
| REST book receipt→processing start,124 events | median65ms /p90 73ms | No comparable long receipt-to-handler lag |
| ACK receipt→native processing start,22 | median7ms /max10ms | ACK ingestion itself is prompt |

The SDK sets `requestStartedAt` **after** admission and the durable `network` command (`transport-service/src/lib.rs`, `perform`). It sets `receivedAt` **after** response validation and durable completion. Those spans cannot measure limiter wait and are not pure provider latency; paginated aggregate receipts can span several requests. The shared600ms completion pacing remains necessary. Even its minimum is not a complete explanation: 722 receipts over21m26.873s is only33.7/min, well below the100/min ceiling. No exact queue-wait/CPU partition is available without additional instrumentation.

The original native `processedAt` is stamped before `Session::accept_batch`; it is not a commit completion timestamp. The1242 distinct timestamp groups (median2events, p90 6) also cannot be assumed to equal batch count. Existing32-event/64-frame batching is already active; simply proposing it again would not address this writer cost.

## Direct process I/O evidence

Initial bounded sample, same PID/start-ticks,12:06:05.701–12:06:15.704 UTC:

| Counter | Start | End | Delta |
|---|---:|---:|---:|
| user ticks,100Hz |93529|93956|4.27 CPU seconds|
| system ticks,100Hz |25938|26303|3.65 CPU seconds|
| write syscalls |248485603|252239874|3754271|
| written characters |2356006952|2376618443|20611491 bytes|
| physical written bytes |2377768960|2398527488|20758528 bytes|

That is **79.2% of one CPU core**, including36.5% in the kernel, and **5.49bytes per write syscall**. The observed native report was1919365bytes. The repeated complete report writer is a source-backed explanation for this pattern; `/proc` alone does not attribute every syscall to a specific function.

The independently recorded second10.002s sample in report.json was less busy:35.4% of one core,694314 write syscalls,24460194bytes (35.23bytes/call), zero physical disk reads, resident memory373395456bytes. Its two5s subintervals varied from55.6% to15.2% CPU and689314 to5000 write calls. This variability rules out treating one burst as the steady-state CPU percentage.

The SDK's queue Store serializes full records into a byte vector and uses `write_all` (`account-queue/src/main.rs`, `persist`); that is a separate state-size cost, not the same millions-of-tiny-file-write mechanism. Native accept still renders complete historical order/fill projections, hashes the report and rewrites it. Those are plausible remaining O(history) costs, but their individual CPU time was **not measured** here.

## Smallest actionable fix and validation

Use a64KiB `BufWriter` only inside native `atomic_json`, explicitly flush it before `File::sync_all`, then preserve the existing rename and directory sync. Do not remove report fields, skip syncs, delay source clocks, change quote prices/risk limits or skip reconciliation.

Source/test handoff `319c4646f0891e520701a5ad650aead20307f491` contains the narrow buffering change, three authored byte-parity/failure tests and a temp-only benchmark binary. Root owns compilation, independent review and any controlled deployment. This report does not claim those Rust tests or the benchmark passed. Required evidence is exact original report byte parity, write/flush failure preserving the old destination, rename failure preserving the destination, and same-frozen-report syscall/timing comparison with all syncs retained. After deployment, compare equivalent live post-restoration windows; a microbenchmark cannot establish whole-loop speedup or alpha.

The included audit tests passed **6/6 locally and6/6 ARM64**. They cover overlapping receipt intervals, exact native lexeme-preserving seals/tamper rejection, finite suffix/torn-row bounds, PID reuse, syscall arithmetic, invalid/missing timing and censored attempt/ACK handling.

```powershell
python research/moloch-hotpath-20261016/test_inspect.py
```

On Linux, to repeat a new bounded observation (output path must be outside live runtime files):

```sh
nice -n 15 python3 inspect.py --root /home/archon/apps/moloch/shared/live-production-v1 --pid ACTUAL_PID --since ORIGINAL_POST_RESTORE_UTC --output /tmp/new-hotpath-report.json
```

The analysis is read-only with respect to its input files. Original account evidence stays private; only aggregates, source hashes and timing counters are published.
