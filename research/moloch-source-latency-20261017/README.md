# Source availability delay

[Task167](https://github.com/GodlyDonuts/predictions-cup-2026/issues/167) follows the [fixed SIG-only screen](../moloch-sig-streaming-screen-20261017/README.md). Root independently reproduced this historical timestamp census using the existing original raw-token native reader and its book-clock parser. This is an execution-data bottleneck investigation, not a new model, API client or infrastructure rewrite.

Original sequence462977..478357 spans09:33:11.782-10:32:24.826UTC, before R25. The journal SHA is pinned in [the exact result](historical-clock-audit.json), and its seals/linkage/anchored boundaries are separately authenticated in the screen's root verification.

Of13,520 books,13,245 are normalized realtime and275 REST. The parser marks6,540 stale at processedAt. Of those,6,528 had source time within10s at receipt and6,333 waited more than10s between receipt and processing. Across all books the receipt-to-processing median is8,872ms; source-to-receipt median is535.1139ms. These are separate distributions, not an additive decomposition of a median.

Calculation: parse original book sourceAt/requestStartedAt/receivedAt and native event processedAt with the existing exact nanosecond reader; apply the unchanged10s freshness bound. Count stale source-age-at-receipt <=10s and stale receipt-to-processing >10s; take the separate all-book medians. No clock is reset, and stale observations do not become eligible trades.

All1,055 gap events omit reasons. Two237-scope receipt batches occur at09:33:33.842 and10:04:22.739, consistent with subscribed/rejoin all-scope invalidation in worker.connect_stream and RestSource.subscribed. This does not prove237 independent market outages. The remaining581 cannot be causally classified from the retained events. Missing books cannot establish market silence.

The evidence locates delay after receipt but cannot separate queued waiting, normalization, native planning/report persistence or blocking REST time. **It does not measure current R25 latency.** First obtain a bounded coherent new-process suffix and a reproducible hot-path measurement; keep the sole service operating. No source/TTL/risk/rate/model/activation change is made by this record.

## Current R25 observation

The separate [current-process packet audit](r25-current-clock-audit.json) authenticates all9 original files and737 raw sealed rows478358..479094, anchored at the controlled R25 boundary. Actual worker649399/source8251b883/binary0bc69f07/control5db85b5e is checked. The suffix ends11:02:31.257UTC; its first managed-planning row10:54:51.126 precedes the first verified trading heartbeat. This is a finite startup/early-trading sample, not a stationary latency benchmark.

There are411 books,373 realtime/38REST:211 stale,210 source-fresh at receipt and203 waiting more than10s before processing. Separate receipt-to-processing/source-to-receipt medians are9,657ms/596.9631ms; maximum receipt-to-processing34,581ms. Original381 fill tokens, cash/cost/realized remain unchanged; zero new fills and reservedCash0. No API calls were made by capture/verification. The different old/new samples do not establish a causal performance difference.

## Narrow source candidate (not yet qualified or deployed)

Periodic source acquisition previously blocked the original Session through a public Trades GET, tape application and an OrderBook GET before draining the stream. The candidate uses the existing LivePreflightReads/serviced_read implementation for the already-allowlisted **OrderBook GET only**, servicing queued frames before reads and while that same exclusive SDK task waits. Public Trades remains blocking. Its original returned tape is fully validated/applied before another callback. The returned original book is immediately validated/applied with no trailing hook.

All three source-acquisition callers use the adapter: selected-market refresh, owned-age refresh and the optional Paper5 source loop before any context/plan/permit. No callback is added to a frozen-plan acceptance or returned owned-fill commit. The Transport/limiter is unchanged and exclusively borrowed; task completion/failure is joined before reuse, source admission stays on the original thread and existing typed service failures remain fatal to the read path. The same helper already serves ordinary/reviewed preflight.

Only worker.rs and its five focused inline fixtures change. Tests cover exact tape-before-hook order, original book clocks, invalid tape, service failures before/after tape and during book wait, book failure without a trailing callback, and a genuine normalized stream gap during the pending book that the earlier REST request cannot clear. The helper allowlist, SDK/native source, monetary/model/risk/TTL/rate/activation policy and original financial ledger are unchanged. ARM64 qualification and actual post-install measurements remain pending; no speed or profit benefit is claimed.
