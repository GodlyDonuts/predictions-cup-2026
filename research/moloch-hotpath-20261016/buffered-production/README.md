# Buffered writer: production comparison

**The live sample confirms that tiny writes were eliminated; it does not establish a broad latency improvement.** The six-minute watch ended with the first pass still incomplete. Original source/receipt/ACK clocks and matching-PID diagnostics are retained as aggregates in [report.json](report.json).

## Actual observation

The parent confirmed fresh trading at **2026-10-16T12:57:36.445203Z**. Only after that signal did this observer connect and run at nice19. It verified:

- PID503120, source `31aefba5c22b529d60acab65659876294f2e0c8d`.
- Actual `/proc/PID/exe` SHA256 `0fa66973f3f51144b1e56dadd0f1453b9ac6c49f9d0279a61a53aee8a99e22f4`.
- Original source-manifest SHA256 `0e3afb5a72e392704b803c7f5ab05ed95c4ac8b98e7e546aded1e33406932ad8`, with the same full source commit.
- Unchanged process start ticks, executable descriptor identity and source-manifest bytes through capture.

Event analysis runs from the confirmed readiness clock through **13:06:39.223556 UTC**. The exact ten-second process sample is **13:00:39.221065–13:00:49.222567 UTC**. Thirty-seven independent diagnostic snapshots cover up to six minutes. No account requests, SDK/controller construction, trace/profiling, replay, build, restart, policy change or runtime write occurred. All observer SSH sessions and jobs were closed afterward.

| Component | Recorded baseline, PID494293 | Buffered, PID503120 |
|---|---:|---:|
| Process sample duration |10.002s|10.002s|
| Write syscalls |694314|153|
| Written characters |24460194bytes|29062147bytes|
| Bytes per write syscall |35.23|189948.67|
| User CPU |2.66s|3.31s|
| System CPU |0.88s|0.18s|
| Total one-core utilization |35.39%|34.89%|
| Physical disk reads |0|0|
| Realtime book receipt→processing median |11.383s,2894events|12.524s,1344events|
| Realtime book receipt→processing p90 |25.119s|24.467s|
| REST book receipt span median |336ms,88receipts|355ms,46receipts|
| Owned-fill receipt span median |217ms,263receipts|232ms,183receipts|
| Physical attempt→ACK receipt median |761ms,22attempts|935ms,16attempts|
| ACK receipt→native processing median |7ms|7ms|

The baseline uses the exact committed ten-second sample from the preceding report, not its separate busier burst. The written bytes, selected scopes, order population, market activity and event windows differ. The syscall reduction is a measured I/O improvement; these process samples show essentially equal total CPU utilization and **do not** establish a whole-loop speedup. The report sets `wholeLoopSpeedupEstablished:false`.

During the six-minute watch, original matching-PID progress moved from **10/30 scopes at13:00:21.946** to **21/30 at13:06:27.031**. No matching-PID completed-cycle status appeared. The retained status belonged to an older process and was excluded. The original new-PID selection clock was12:57:17.247,19.198s before the parent's confirmed readiness clock; this earlier original clock is explicitly flagged and does not replace the event-analysis start. By the last observed progress clock the first selection had been running9m09.784s, with completion still unknown. A30-scope partial pass cannot be compared to the baseline's completed35-scope19m21.122s pass as a measured speedup.

The415 retained receipts span540.959s: **45.81/min**, compared with33.66/min before. Their union occupies128.996s,23.85% of that span, leaving411.963s outside retained request→receipt intervals. That remainder mixes admission/pacing, local processing and scheduling. Owned-fill tapes account for183/415receipts (**44.1%**), versus263/722 (**36.4%**) before. New activity has more outstanding-order work; the larger observed read rate and16 ACKs are not new fills or profit. No actual-fill event occurred inside the new analysis window.

## Provenance and limits

Public aggregate report SHA256: `17b0fe5a10d6459bc0f9aef41033dbadef08095318838b54987c0dd9d654e2e1`.

- Native suffix: bytes756264268–773041424, SHA256 `07e2b8d40933fe0745048939806aa9b24c62ac6a9fa69d189ad452f9524dfd0b`, rows245051–249422. Earlier rows are filtered out by the original processing clock.
- Dispatch suffix: bytes3246461–5343214, SHA256 `a70b93f9255b7700105dfefe4bc4c7d28d4f8678bcf74db4e7449dd7360469c6e`, rows6415–10450. All16 dated attempts have ACK receipts inside the analysis window.
- Both original bounded suffix chains and raw row seals were verified. The preceding full prefix was not independently recomputed.415 receipts total598440original bytes; the selection was not truncated.
- Actual observer source SHA256 `da98d4c3438433c0c5a12eae9c93c743a38d1a4c78ec145b57043775862dc999`; transferred source/baseline archive SHA256 `6502f3fbaaaf6c77b3eb93cb7038fe679a14b1390d3656462690d66984ffca5f`.
- Focused observer tests passed **9/9 locally and9/9 ARM64**, including stale-PID exclusion, original source/baseline pins, PID reuse/counter regression, cycle rollover and original selection-to-status attribution.

The continuing12.5-second median realtime backlog merits a separate source audit of synchronous read batches between stream drains. This report does not attribute every delayed frame to a specific function or claim that draining more often creates profitable quotes. Original full fill reconciliation, unknown-order guards and shared pacing remain necessary.

## Reproducing the bounded observer

This bounded filesystem-only observer reuses the exact SHA-pinned preceding audit parser and original aggregate baseline. It validates `/proc/PID/exe` against the expected executable SHA and pins the original source manifest's bytes and `sourceCommit`. No process arguments, environment, credentials, HTTP/SDK/account state, process traces or engine replay are read or invoked. Only the explicitly chosen new output file is created; replacing runtime files or the source/baseline inputs is refused.

Three process samples span two5-second intervals. PID start ticks, monotonic counters and unchanged executable identity distinguish the actual worker from a recycled PID. Matching-PID original progress/status/selection diagnostics are captured independently before and after sampling, then at10-second intervals for at most six minutes or until a newly completed cycle is observed. Historical, missing, pre-readiness or future clocks are labelled unavailable. A first-cycle count is never borrowed from the preceding process. Scope rollover is not reconstructed from overwritten progress files. Original matching-PID selection clocks can separately anchor a selection-to-status interval; a selection predating the human-confirmed readiness clock is explicitly flagged and does not change the event-analysis cutoff.

The observer freezes at most16MiB of native journal and2MiB of dispatch, verifies the raw original row seals and suffix chains, and records offsets, byte counts and SHA256s. It does not claim full-prefix verification. At most2048 original receipts (8MiB each and64MiB total maximum) supply aggregate endpoint-class timing; truncation is explicit. Only receipt clocks and endpoint class are retained in memory after each original file is checked. The retained request/receipt clocks remain unchanged. No raw account/order/fill data appears in output.

The baseline has a10.002-second recorded process sample and a21m30.515s post-restoration event window. A shorter new event window will have different counts and market/owned-order activity. Compare syscall rate, bytes/write, CPU and each event/receipt class separately. Whole-loop speedup is not inferred from either a microbenchmark or a10-second CPU burst. Physical trading frequency and profitable alpha remain separate outcomes.

`requestStartedAt` begins after SDK admission, and `receivedAt` ends after response validation and durable completion. Those spans exclude pre-admission queue wait and may include pagination. Native `processedAt` precedes `accept_batch` completion. Receipt-to-processing lag therefore measures the start of local event handling, not complete durable processing. A lower book backlog cannot be relabelled faster provider HTTP or proof that the limiter was bypassed.

```sh
python3 test_inspect.py
nice -n 15 python3 inspect.py \
  --root /home/archon/apps/moloch/shared/live-production-v1 \
  --pid ACTUAL_NEW_PID --since ORIGINAL_POST_RESTORE_READINESS_UTC \
  --source-commit CHECKED_SOURCE_COMMIT --binary-sha256 ACTUAL_EXPECTED_SHA256 \
  --source-manifest ORIGINAL_SOURCE_MANIFEST --source-manifest-sha256 ORIGINAL_MANIFEST_SHA256 \
  --baseline ../report.json --output /tmp/new-buffered-production-report.json
```

Keep this directory and the parent audit directory in their repository-relative layout so the pinned parser and original aggregate baseline are available. The output path must not exist. No service restart, control change, permit, new account reader, build or workload generator is part of this comparison.
