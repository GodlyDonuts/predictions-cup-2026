# Source availability delay

[Task167](https://github.com/GodlyDonuts/predictions-cup-2026/issues/167) follows the [fixed SIG-only screen](../moloch-sig-streaming-screen-20261017/README.md). Root independently reproduced this historical timestamp census using the existing original raw-token native reader and its book-clock parser. This is an execution-data bottleneck investigation, not a new model, API client or infrastructure rewrite.

Original sequence462977..478357 spans09:33:11.782-10:32:24.826UTC, before R25. The journal SHA is pinned in [the exact result](historical-clock-audit.json), and its seals/linkage/anchored boundaries are separately authenticated in the screen's root verification.

Of13,520 books,13,245 are normalized realtime and275 REST. The parser marks6,540 stale at processedAt. Of those,6,528 had source time within10s at receipt and6,333 waited more than10s between receipt and processing. Across all books the receipt-to-processing median is8,872ms; source-to-receipt median is535.1139ms. These are separate distributions, not an additive decomposition of a median.

Calculation: parse original book sourceAt/requestStartedAt/receivedAt and native event processedAt with the existing exact nanosecond reader; apply the unchanged10s freshness bound. Count stale source-age-at-receipt <=10s and stale receipt-to-processing >10s; take the separate all-book medians. No clock is reset, and stale observations do not become eligible trades.

All1,055 gap events omit reasons. Two237-scope receipt batches occur at09:33:33.842 and10:04:22.739, consistent with subscribed/rejoin all-scope invalidation in worker.connect_stream and RestSource.subscribed. This does not prove237 independent market outages. The remaining581 cannot be causally classified from the retained events. Missing books cannot establish market silence.

The evidence locates delay after receipt but cannot separate queued waiting, normalization, native planning/report persistence or blocking REST time. **It does not measure current R25 latency.** First obtain a bounded coherent new-process suffix and a reproducible hot-path measurement; keep the sole service operating. No source/TTL/risk/rate/model/activation change is made by this record.
