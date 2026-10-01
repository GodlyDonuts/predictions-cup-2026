# Capture, fixtures and downstream integration

## Normalized observation capture: `sig-feed-capture`, schemaVersion 1

Each JSONL record has `format`, `schemaVersion`, sequential `captureIndex`, `kind` and local `observedAt` (UTC). Collector records also have configured `topic` and subscription `epoch`. Stored observations are append-only; status reports are derived. `config` includes the explicit tournament/market/exchange scope and freshness policy.

Pre-subscription-ack buffering preserves callback-time receipt separately from local drain/processing time. `event` holds original receive time `receivedAt`, supported event name, and a normalized allowlisted payload. `market_batch` includes:

- `delivery`: model, topic revision range and source engine sequence bounds. Free-form correlation diagnostics are omitted deliberately
- `books[]`: exchangeId (canonical string), asOf `{sequence, at}`, nextExpiryAt, expiryKnown, bids/asks of numeric `{price, quantity}` in YES perspective
- `trades[]`: stable string id, safe integer sequence or explicit null, scoped IDs, YES price, quantity and executedAt
- `bookDirty[]`: scoped exchange ID and source at
- `marketSettled[]`: scoped market, at, and `refund` boolean. Original free-form settledWith labels are deliberately omitted; these are invalidation hints only
- `resyncRequired`: boolean. Compact batches normalize absent arrays to empty arrays, with the flag retained

`snapshot` contains receivedAt and normalized books from the matched authoritative context. `recovery_started`, `recovery_finished`, `invalidation`, `disconnected`, `subscribed`, `stopped`, `rejected` and the CLI's final `summary` describe local lifecycle. A captured snapshot alone does not prove accepted recovery: check the later recovery result and invalidations. A failed/discarded recovery cannot be promoted to a valid segment by ignoring lifecycle records.

`trade_observed` is a deduplicated derived convenience row with `continuousAtReceipt`. If any trade in a batch has unknown sequence, every derived row in that batch is marked noncontinuous, regardless of trade ordering. It is not a fill, label of aggressor side, guarantee of complete history or assurance of source authenticity. Do not double count it and the containing batch's trades. Retain original `event` rows for audit, duplicates, gaps and dataset reconstruction. This capture is intentionally not a full raw wire archive; unknown fields, auth diagnostics and free text are stripped.

Metrics cover configured/observed/versioned exchange counts, accepted/duplicate/out-of-order batches, gaps, schema errors, books applied/older books, trade-ID duplicates/evictions, reconnects, recoveries/discards and negative-clock-skew samples. Event age and recovery response latency report count, sum, mean and max milliseconds. Event age combines source/arrival clocks and includes provider batching/backlog; it is not pure network RTT or order latency. No percentile or full-universe coverage claim is made.

## Offline test input: `sig-feed-replay`, schemaVersion 1

Fixture rows have explicit observedAt plus `config`, `subscribed`, `snapshot`, `event`, `disconnect`, `tick` or `stop` kinds. Snapshot/event payloads use source-shaped structures so the public-contract parser is tested. Observed timestamps must be monotonic. This hand-authored test format is distinct from normalized capture; the CLI rejects capture records rather than silently converting them. Capture round-trip/state reconstruction is a future separate adapter, not claimed here. Golden fixture replay is deterministic; injected runtime tests separately exercise concurrent subscription/REST/abort races.

## Paper PR #5 boundary and later book-feature research

The collector preserves supported feed semantics instead of manufacturing the paper engine's richer execution inputs. The WebSocket trade contract has no aggressor-side field, no queue-order IDs and no own-order attribution. Stream books include own orders. A trade at a price cannot safely be classified as a buy/sell aggressor or simulated fill; REST trade YES/NO space is also not an aggressor label. Batch engine sequence and topic revision are distinct and must not be interchanged with paper-engine sequence assumptions. No conversion to paper fills is enabled in this PR.

A later offline baseline may consume valid normalized book observations to calculate spread, midprice, displayed depth imbalance and short-horizon forward-price labels. Entry point: `event.payload.books` and accepted `snapshot.books`, segmented by scope/epoch and explicit lifecycle state. Reconstruct versions with `compareVersion`; omit ambiguous equal-version conflicts, gaps, resync windows, stale/expired intervals, unknown expiry as required by policy, clock-skewed/future observations, and unversioned snapshots. Keep receive time for causal feature availability and source `asOf` for provenance; a book may be newer than the trades in its batch.

Chronological splits must purge/embargo overlapping forward labels, never let revised/future snapshots into earlier features, and report retained/excluded coverage. Synthetic fixtures test code only; do not train on them and call the result market evidence. Real authorized read-only capture, representative data, fill/fee/latency calibration and held-out performance are prerequisites for any strategy/P&L claim. No profitable model is trained or claimed here.
