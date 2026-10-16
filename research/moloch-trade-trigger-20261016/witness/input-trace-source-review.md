# Independent input-trace source review

Viserion/main_takeover reviewed frozen source **`69f7bbc303523c3fa9b934761d55682af76a9ecc`** on **2026-10-16**. Producer Git blob: `527d69ecaf70197e60cc5defb9c808138b7d77b2`. This reviewer did not implement the trace. Review and tests used only frozen code and synthetic temporary files; no current cohort/outcomes, SSH, account/cache network calls, compilation or production operations were used.

**No concrete activation blocker found for the documented opt-in read-only capture.** This conclusion does not approve a trading model or declare the captured witness independently verified. Parent retains activation and publication.

## Checked behavior

- [`trace_cache_poll`](https://github.com/GodlyDonuts/predictions-cup-2026/blob/69f7bbc303523c3fa9b934761d55682af76a9ecc/moloch/external-data/crossvenue_observer.py#L915) retains original response bytes as base64 and hashes them **before parsing**. Route, status, request/read clocks and truncation are separate. A failed open/read records unavailable bytes rather than inventing a body. The unchanged two-route loop still skips the remaining route on a transport failure; no extra request is introduced.
- [`apply`](https://github.com/GodlyDonuts/predictions-cup-2026/blob/69f7bbc303523c3fa9b934761d55682af76a9ecc/moloch/external-data/crossvenue_observer.py#L859) records the exact sampler argument and input reference before invoking the existing callback, then its completion clock and output ordinal range. No-sample calls remain visible. Failed callbacks cannot gain an apply-complete; unfinished inputs prevent a complete seal. Sampler `now` is an invocation argument, not independent collector-publication time; trace I/O precedes the callback and changes the capture condition.
- [`Tail`](https://github.com/GodlyDonuts/predictions-cup-2026/blob/69f7bbc303523c3fa9b934761d55682af76a9ecc/moloch/external-data/crossvenue_observer.py#L145) records exact original SIG line bytes including newline, byte offset/length and raw-line SHA. Consumed sequence/hash/previous and held file identity are retained. Initial EOF/partial-line bookmarks do not claim history-prefix verification. Rotation records old/new bookmarks without inventing cross-segment continuity.
- [`InputTrace.append`](https://github.com/GodlyDonuts/predictions-cup-2026/blob/69f7bbc303523c3fa9b934761d55682af76a9ecc/moloch/external-data/crossvenue_observer.py#L816) compresses each member before checking the shared 2GiB trace/sample limit, reserves 64KiB for a seal, checks 10GiB free headroom and file identity/expected sizes, handles partial writes and fsyncs each member. Failures poison capture through a distinct error rather than entering the cache's recoverable I/O branch. Trace mode bypasses `Output.prune`; existing files are neither deleted nor repaired.
- [`seal`](https://github.com/GodlyDonuts/predictions-cup-2026/blob/69f7bbc303523c3fa9b934761d55682af76a9ecc/moloch/external-data/crossvenue_observer.py#L888) compares actual compressed bytes/hashes against its written-member state, checks pinned producer/mapping references, pending calls and poison state, and writes a create-new bounded terminal file with Unix file/directory sync. `status.json` is not the witness. A healthy-looking status cannot substitute for an intact complete seal and independent verification. Errors affect this sidecar/output only, not a financial-service STOP.

## Verification and limits

Exact frozen source independently passed **94/94 tests (1.011s)**: 16 trace tests and 78 existing observer/economics/toxicity/trade tests. The unchanged golden logical sample bytes passed. From `moloch/external-data`:

```text
python -B -m unittest test_crossvenue_observer_input_trace test_crossvenue_observer test_crossvenue_economics test_crossvenue_toxicity test_trade_triggered_observer
```

Retained limits matter: SIG raw files must be independently retained and matched to references. A suffix chain is not a history audit. The pre-existing reader can rotate while the previous segment still has unread bytes; bookmarks expose offsets/sizes, but capture does not repair that reader. Independent verification must account for such omissions rather than claim complete source coverage. Mapping-file hashes do not alone certify effective cache mappings or contract equivalence. Trace compression/fsync can alter latency and eligible sampling, so traced and untraced cohorts are distinct conditions.

`wholeWitnessVerified`, financial authority and clock-renewal claims remain false. Successful capture still requires a separate original-byte membership check, exact ordered sampler replay with recorded arguments/emission ranges, source-conflict and missingness checks, and first-consumed future-book verification. No actual new outcomes, calibration, profit or latency improvement was established by this review.
