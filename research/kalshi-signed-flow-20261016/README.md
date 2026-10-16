# Original public Kalshi directional flow

Current SIG deployment is503120 and its read-only observer is503257/v8;
the unchanged signed-flow observer source now follows the replacement's capture.
[Current deployment and actual timing comparison](../moloch-hotpath-20261016/deployment-README.md).
The dated feed-only release below did not cause that later SIG replacement.

## Historical actual deployment at12:21UTC

The checked public-data release from`47f8693c` is live: supervisor33752,
collector9240, with the original cache/journals and pinned`ws8.22.0`. At12:21:48UTC
it had237 valid books, one connection, zero sequence gaps,135 actual public trade
frames and20 REST starts/sec. Six recoverable network errors and zero throttles
were recorded. The30-second snapshot default remains. SIG worker494293 was
not restarted by this feed update. [Exact release and feed observation](live-verification.json).

The source-bound observer496491/`67426474…` is also live on the existing all-market
capture. At12:05:08UTC it validated237 actual flow exports, one with30-second
trade evidence.456 complete decision rows retained flow snapshots; one contained
an identified observed signed quantity. Empty/missing flow is not zero market
trading; completeTradeTape remains false. The active gzip was not sealed and
this bounded live read is not a frozen training cohort. [Original-clock validation](observer-live-verification.json).

This enables data collection for later qualification. Existing seven-feature
pricing and toxicity models are unchanged; physical Kalshi pricing remains
inactive. Windows/SSH availability is still a dependency of this external cache.
The following source-handoff details record the implementation and its checks.

Primary documentation observed2026-10-16T11:48Z: [Public Trades](https://docs.kalshi.com/websockets/public-trades), [official AsyncAPI](https://docs.kalshi.com/asyncapi.yaml), tradePayload fields. The retained original AsyncAPI SHA-256 is `3c86bae7d89b44baba1119b3a097a54054c1d851e1dcd8ad894c7cb35b9890b6`. Canonical taker outcome YES (or book-side bid) represents positive YES-directional exposure; NO/ask represents negative exposure. This identifies direction, not the taker's literal buy/sell action, our fills, calibrated alpha, or matching-price changes. Deprecated taker_side is accepted only as its documented yes/no directional bit. Conflicting, absent or unknown side fields remain unsigned.

An existing original public trade was inspected read-only, with raw bodyText SHA independently matching `88d8a09da8856e6fb8ddff0967ebcf5394fc8ef8ec8f4305c7f40ddaedc0940b`: receipt10:16:22.192Z, execution ts_ms10:16:22.196Z, quantity122.95, canonical YES/bid and agreeing deprecated YES, block=false. This confirms actual fractional schema and bounded server-clock skew; it is not a new subscription or execution observation. No private paths, credentials, unrelated account fields or raw auth are published here.

`ShadowState.flowView(ticker, at)` exports OPTIONAL1/5/30second public-trade windows separately from book views. Membership uses original execution time in `(at-window, at]`, plus original receipt and first local availability no later than `at`. Original source time may lead receipt by at most the existing1second bound, but data remains quarantined until the as-of clock catches up. Missing execution time produces separately labelled receipt-only activity, never a substituted execution timestamp. Exact fixed-point quantities are aggregated without float rounding or price weighting. Known signed quantity is separate from full signed quantity; the latter is null when unsigned-direction or missing-execution-clock observations prevent an identified total, or no execution was observed.

Each window includes original evidence hashes and IDs, source/receipt/first-availability clocks, stream epoch/SID/sequence, exact quantity, prices and known block classification. The evidence digest covers the entire matched set; at most8 detailed rows are exported and evidenceComplete discloses truncation. Block and unknown-block counts are explicit: the aggregate covers observed public directional trades, not exclusively standard-orderbook executions. No hypothetical historical book snapshot is treated as a trade. The collector's quote-export hunk adds `tradeFlow` only; requests, schedules, refresh defaults and SIG authority remain unchanged.

Trade-ID dedup retains original clocks and hashes even if a retransmission has a later wire sequence. Conflicting immutable trade contents or bounded-capacity exhaustion explicitly resubscribe the trade SID, clearing its flow window without invalidating unrelated book SIDs. A SID gap/disconnect clears the affected stream. Each new actual socket receives its own connection epoch; no trades are joined across streams. Memory is bounded by32,768 remembered IDs,4,096 observations/ticker and180seconds of retained source/receipt history, with pruning at most once/second. Missing timestamps and side fields never manufacture signing or coverage.

`observedSubscriptionDurationMs` and `windowObservedSinceSubscription` describe original observed subscription age only. Every window has completeTradeTape=false: the public stream's documented sequence guarantees and absence of observed gaps do not establish exchange-wide complete trade coverage. Empty observed flow is not evidence of no market trading. No book timestamp, source clock or financial capability is renewed by trades, ticker events, timers or export calls. All outputs remain pricingAuthority=false / executionAuthority=false.

Validation:37 offline Node tests pass, including actual-shaped fractional trade fields, positive/negative exact flow, same-ID retransmission, unknown/conflicting side, missing execution time, causal first availability, original4ms clock skew, expiry, SID gap/conflict isolation and bounded-capacity denial. Collector syntax check and diff check pass. This is source-only; no collector deployment, live connection, SIG request, financial model approval or trading result is claimed.

## Optional observer inputs — separate from selected book modality

The observer separately validates/caches `tradeFlow` from actual WS quote exports before book eligibility/selection, so a REST book fallback does not erase fresh public-trade evidence. It requires exact ticker, source, actual stream epoch (matching envelope epoch when available), SID, original wire hashes, execution/receipt/consume clocks, exact decimal arithmetic, direction/block classification, coverage flags and source snapshot age at most2seconds. Original WS execution time can lead local receipt by at most1second but cannot exceed export/decision time. A canonical sourceSnapshotSha256 binds the retained complete export; it is distinct from the original individual wire-message hashes.

For complete detailed window evidence, the validator reconstructs the producer's original fixed field order to verify evidenceSha256 and recomputes counts, quantities and signed totals. Duplicate trade IDs within a window or conflicting evidence across windows reject locally. Incomplete/truncated evidence retains the explicitly reported original source snapshot for audit, but **never supplies signed model inputs**. Missing execution clocks, unidentified direction or no observed executions also leave inputs null. No complete trade tape is inferred from observing the subscription.

New prospective decisions retain optional `externalTradeFlow`: original sourceSnapshot plus sourceSnapshotSha256, first observer availability, source export age, decision clock and missing-aware observed1/5/30second signed quantities. At decision time, actual original execution times are checked again against each window, so cached records that aged out are removed rather than renewed. Observations after the export are unknown, not zero new flow. Snapshot duplicates cannot renew first observer availability; stale/malformed/disconnected evidence becomes missing locally. Existing seven-feature model, legacy producer reports, all source clocks, book freshness and fill/financial claims are unchanged. This is additional unqualified data collection for a future reviewed model, not a newly enabled strategy.

Follow-on checks:33observer+16economics+18toxicity tests pass. A genuine retained raw public trade was replayed through the actual Node flow exporter and its output validated by Python, preserving exact122.950000 and completeTradeTape=false. The offline replay explicitly used first-availability fixture+100ms; it does not claim that was the live historical availability clock. No actual collector restart or new account/market-data connection was performed.
