# Buffered writer: production comparison

Prepared locally. **No post-deployment observation has been performed.** The parent execution owner must provide the new PID, actual post-restoration readiness clock, runtime SHA256, source commit and original source-manifest path/SHA before the observer is run. The initial report records that pending state rather than an assumed result.

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
