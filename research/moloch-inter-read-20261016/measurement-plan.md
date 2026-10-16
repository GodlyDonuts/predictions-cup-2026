# Inter-read stream service: measurement and next economics test

This is a source-bound plan, prepared without SSH, account requests, builds, deployment or unchanged test reruns. The execution owner reported the corrected candidate passed **81 ARM64 worker tests: 78 existing and 3 new**, followed by an optimized build. At preparation time its promotion and fresh successor PID remain the execution owner's responsibility; no post-hook result is claimed here.

## Exact candidate and comparison

| Item | Pin |
|---|---|
| Candidate source | `4ee163561b8018e58d945b38b4f4eea5cd7b09ee` |
| Candidate executable SHA256 | `011ffde0a00a959959acf99ef1824328c9b75f176ac66eda6c8eba6e96832c37` |
| Frozen pre-hook PID/source | 503120 / `31aefba5c22b529d60acab65659876294f2e0c8d` |
| Frozen pre-hook aggregate SHA256 | `17b0fe5a10d6459bc0f9aef41033dbadef08095318838b54987c0dd9d654e2e1` |
| Pre-hook realtime receive→processing lag | median 12.524s; p90 24.467s; 1,344 events |
| Pre-hook physical attempt→ACK receipt | median 935ms; p90 1,163ms; 16 attempts |
| Pre-hook ACK receipt→native processing | median 7ms; 16 ACKs |
| Pre-hook ten-second process sample | 153 write calls; 29,062,147 written bytes; 3.31 user / 0.18 system CPU seconds |

The frozen [report](../moloch-hotpath-20261016/buffered-production/report.json) and [interpretation](../moloch-hotpath-20261016/buffered-production/README.md) remain unchanged. Its event window is **12:57:36.445203–13:06:39.223556 UTC**; the process sample is **13:00:39.221065–13:00:49.222567 UTC**. It contains 415 retained logical receipts, of which 183 are complete owned-fill tapes, and no actual-fill event inside that analysis window. These are observational cohorts, not randomized or identical workloads.

## What changed, and what did not

The runtime calls the existing bounded `drain_stream` **before each logical ordinary reconciliation read**, borrowing the same sole Session, Transport, source state, breadth recorder and Universe. Existing source batching and per-drain limits remain intact. A returned receipt flows straight into existing validation, full-tape fill application and independent terminal processing: there is no post-receipt drain hook before that application. A hook failure prevents the next GET and propagates without retrying an intent, clearing uncertainty, inventing a terminal or discarding an already committed fill.

The native engine and event semantics are unchanged. This is a runtime-only successor to the same financial engine; it does not require a newly invented native identity bridge or a funding/history reset. Monetary limits, native quote terms, actual fill attribution, cancellation races, read/write caps, Retry-After and complete fill-tape requirements remain authoritative.

The hook **cannot service frames during one blocking HTTP request, an internally paginated logical read, response validation, durable completion, or SDK admission/backoff waits**. It also does not add drains to every unrelated preflight/package path. A remaining long lag is therefore possible without disproving that the new hook executes correctly. A retained logical receipt can contain multiple HTTP admissions; receipt count is not the physical account-admission count.

`requestStartedAt` is recorded after SDK admission. `receivedAt` is recorded after validation and durable completion; aggregate pagination can cover several calls. Native `processedAt` is stamped before acceptance completes. Preserve all these original clocks. Their differences do not isolate provider time, queue wait, durable commit latency, or causal strategy improvement.

## Bounded collection protocol

1. Wait for the execution owner to provide the actual successor PID, exact executable/source-manifest SHA pins and an original confirmed **post-restoration fresh-trading** clock. Restoration CPU and earlier-process status must be excluded. Verify `/proc/PID/exe`, PID start ticks and original source manifest against those pins before measurement, and recheck process/executable identity afterward. Do not infer deployment from a Git commit or copied binary.
2. Reuse the filesystem-only inspectors under `moloch-hotpath-20261016`. Take three process samples across two five-second intervals. For a roughly comparable phase, begin around three minutes after confirmed readiness, rather than during restore. Independently retain matching-PID progress, selection and completed-status snapshots at ten-second cadence for at most six minutes or until a newly completed cycle. Stop if the PID changes; do not follow an unverified replacement.
3. Freeze at most 16MiB of native journal and 2MiB of dispatch at explicit byte cutoffs, excluding a partial trailing row. Verify raw row seals and contiguous suffix chains. Record byte offsets, hashes, first/last sequence and observation cutoff; state that the preceding full prefix was not recomputed. Filter original event clocks to the declared window, and report when the bounded suffix cannot cover that entire window.
4. Read at most 2,048 retained original receipt files, at most 8MiB per file and 64MiB total. Retain endpoint-class counts/timings and raw-file digests only. Missing labels, clipped windows and observation limits remain explicit. No account client, queue command, tracing, workload generator, Session open/replay or service mutation is needed.
5. Publish aggregate results and original-source pins only. Original account/order/fill data stays private. These independently read files are not an atomic account snapshot.

**Inspector baseline caveat:** the existing buffered-production collector intentionally pins the earlier `d44c87...` baseline. Do not pass a different baseline and loosen its hash guard. Its verified collection can be reused unchanged, but its built-in comparison still targets that earlier baseline. For this experiment, separately verify the exact `17b0fe...` file above and compare its aggregates with the new collection; label this additional comparison transparently. Do not call the older built-in comparison an inter-read effect. Preserve exact Git-LF source/artifact bytes where those raw hashes require them.

## Readout and decision criteria

Report these together, with counts and missing/censored outcomes:

- Realtime book original receipt→processing-start median/p90/max; REST book and public-trade lags separately. If the actual unchanged book-age policy is confirmed, count the fraction already older than that budget before processing; do not derive that count from a guessed policy or a quantile alone.
- Original complete fill-tape and owned-order request→receipt spans; actual fill execution→receipt→native processing; attempt→ACK receipt→native processing. No new fills means the fill-delay statistic is unavailable, not zero. Account updates and provider behavior are not inferred from public prints.
- Logical aggregate/order/fill-tape receipt counts, pending owned orders, selected/required scopes, actual source-event volume and genuine completed passes. Retain full-tape coverage and terminal/cancel races. A lower read count caused by missing work is not a successful latency improvement.
- CPU user/system seconds, write calls/bytes and resident memory for the same approximately ten-second duration. Do not extrapolate a burst into sustained throughput. Preserve the unbuffered and buffered-writer results as separate interventions.
- Original same-PID discovery selection→completed-status duration, only if both anchors are observed and belong to that pass. Flag a selection before the confirmed-readiness clock. A partial 30-scope pass and a completed 35-scope pass are different workloads; do not scale them linearly and declare a measured speedup.

A reduction in book backlog while complete owned-fill reconciliation and safety evidence are preserved is a useful mechanical result. Comparable unchanged/worse backlog warrants inspecting the remaining single-request/pagination/backoff and other synchronous preflight intervals; it does not justify dropping fill tapes, raising caps or extending source clocks. Neither outcome establishes alpha, queue priority, maker fills or profitability. One finite window is provisional; use later naturally occurring work for confirmation, without generating trades for this benchmark.

## Smallest evidence-qualified strategy work

The retained record supports a specific **entry-selection experiment**, not a newly proven profitable trade. The [exact FIFO audit](../maker-exit-window-20261016/README.md) found 40 closed ordinary BUY-fill cohorts: 3 winners, 28 losers and 9 flat, with 12,628 closed shares and −119.535000 ordinary realized. All closed shares used maximum-age exits. None of the available fixed 60/180/300/600-second full-depth labels was positive; missing observations were numerous. A single intermediate NO934 one-tick displayed opportunity is a hypothesis, not evidence for a blanket longer holding rule or a maker fill.

**Next concrete experiment:** use the already implemented `execution_distribution` observer on the worker's existing genuine ordinary attempts, complete terminal tapes, actual fills and source books, with financial authority remaining false. Predeclare one existing normalized pool specification and the existing ordinary holding horizon. Record zero/partial/full execution separately, then quantity-weighted *fill-conditional* executable value, uncertainty, adverse tail, actual reservation/owned capital time and write costs. Keep reviewed fundamental carry cohorts separate. Confirmed zero fills have zero principal PnL; open/unknown orders, missing future books and incomplete capital-time histories remain unresolved/censored.

The first actionable output should be a fixed cost-clearing comparison of **which existing ordinary entries to admit**, versus the unchanged selector on the same market/time blocks. Freeze feature choices, fees/buffer and chronological validation before inspecting the new outcomes; reuse the observer's declared sample/block/coverage gates and preserve genuine execution evidence. The observer is already wired but the retained baseline does not establish it was enabled or qualified, so its absence cannot be filled with invented estimates. A parent-reviewed shadow configuration would use existing reads and create no experimental orders. Only positive held-out, execution-conditioned results after the declared charges would justify proposing a separately bounded production gate; negative or insufficient evidence should be reported directly.

This prioritizes the actual loss mechanism: passive entry selection and its liquidation cost. Faster source handling may improve observation coverage, but it cannot turn the historical negative sample into positive expected returns. A [separate reviewed terminal-value forecast](../moloch-delaware-fundamental-20261015/README.md) can still be evaluated on its declared election horizon and explicit model/exception assumptions; those sources supply no calibrated one-minute price edge. Cross-venue price/flow studies also cannot supply the missing maker-fill proof: the fixed v7 flow study had zero decisions with all three identified windows, and the first price study selected no cost-clearing trades. Do not impute missing flow to zero or retune a sealed study until it selects trades.
