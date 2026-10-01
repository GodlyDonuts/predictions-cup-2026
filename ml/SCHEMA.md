# Normalized offline books, version 1

A UTF-8, newline-terminated JSONL file, at most 100 MiB via the CLI, one header then at most 1,000,000 rows. Empty lines, unknown keys, unknown format/version, malformed values and partial final lines are errors. This is a deliberately explicit **offline normalized dataset**; it is not `sig-feed-capture`, `sig-feed-replay`, a trade tape, an execution simulator or proof that the producer preserved source truth.

## Header (exact keys)

```json
{"format":"sig-offline-books","schemaVersion":1,"datasetId":"my-capture-v1","dataKind":"real","normalization":"quality-segmented-books-v1","provenance":{"sourceSha256":"64 lowercase hexadecimal characters","normalizerVersion":"reviewed-normalizer-v1"}}
```

`dataKind` is `real` or `synthetic`. `sourceSha256` is the hash of the complete immutable source capture, or a separately archived manifest over all source captures. For the synthetic fixture it hashes the documented generator description/seed and makes no source-capture claim. `normalizerVersion` identifies reviewed normalizer code; this package contains no real normalizer. A syntactically valid hash is an audit pointer, not source authentication. Actual run manifests also hash **all exact normalized file bytes**, including excluded rows and the header.

## Common row fields (exact keys plus kind-specific fields)

- `kind`: `book` or `barrier`
- `rowId`: globally unique record identity; duplicates are a hard error
- `sessionId`: a producer-defined capture session, consistent across instruments recorded in the same session. Use real collection blocks, never invented IDs to satisfy evidence gates
- `tournamentId`, `marketId`, `exchangeId`: explicit strings identifying the YES-perspective instrument. IDs must match audited source scope; no side/complement inference
- `segmentId`: contiguous valid regime within a session/instrument; a new ID is required after a gap, invalidation or recovery. It must mean the producer verified valid recovery, not that an arbitrary timestamp was reached
- `sourceAt` (books only): exact book version source time
- `receivedAt`: original local callback receive time
- `availableAt`: earliest local time this validated normalized observation could be known, including any processing/recovery delay. Never backdate a reconstructed snapshot to its source time

All IDs match `[a-zA-Z0-9_.:-]{1,128}`. Times are valid canonical UTC `YYYY-MM-DDTHH:mm:ss[.fraction]Z`, zero to nine fractional digits (if the decimal point is present, at least one digit). Offsets, leap seconds, impossible dates, >9 fractional digits and normalized/rounded timestamp invention are rejected. Comparisons use nanosecond integers. Original timestamp strings are retained in the parsed rows and final prediction provenance. Received time cannot follow availability. Future source time is invalid quality; receive and availability do not imply exchange execution time.

Rows must already be globally nondecreasing in `availableAt`; the parser never sorts input. Availability must be **strictly increasing per instrument**, including barriers/duplicates/segment/session changes. Tied local timestamps have ambiguous ordering here and are rejected, not separated by made-up timestamps. Across-instrument ties are supported. A future audited adapter/schema with explicit ordinal semantics may expand this contract.

For any instrument, sessions cannot overlap or return to an earlier session after switching. For any session/instrument, closed segment IDs cannot be reused. New segments/sessions do not reset the instrument's source-version watermark. An engine-generation reset requires a separately audited input generation; do not rewrite source IDs/timestamps to force acceptance.

## Book-specific fields

```json
{"kind":"book","rowId":"capture-00001","sessionId":"session-01","tournamentId":"tournament-id","marketId":"277","exchangeId":"701","segmentId":"recovery-01","receivedAt":"2026-10-01T00:00:00.010Z","availableAt":"2026-10-01T00:00:00.020Z","sourceAt":"2026-10-01T00:00:00.000Z","sequence":100,"quality":{"continuous":true,"recovering":false,"stale":false,"expiryKnown":true},"nextExpiryAt":null,"bids":[{"price":0.4,"quantity":100}],"asks":[{"price":0.6,"quantity":200}]}
```

- `sequence`: nonnegative safe integer, never topic revision. Composite book identity is scope plus `(sequence, exact sourceAt)`. Following reviewed feed semantics, an equal sequence with a **later** exact source timestamp may be a newer book. Lower sequence or source time is rejected across every segment/session. Identical composite versions must have identical levels/expiry fields; conflicts fail the dataset, including conflicts following bad-quality rows
- Exact duplicate composite versions are counted/excluded across all sessions in this dataset; they do not become features/labels or renew freshness. The registry is not a cross-file database; combine captures into one deduplicated audited dataset rather than train across separately deduplicated overlapping files
- `quality` has exactly four Boolean flags as shown. Valid means continuous, not recovering, not stale and expiry known. A failure blocks the segment. Source age at **availability** must not exceed configured max age, and source time may not be in the future of receive time
- `nextExpiryAt`: `null` means **known no scheduled expiry only when `expiryKnown: true`**; otherwise a valid exact timestamp later than availability. Unknown expiry is excluded, including current PR #6 REST snapshots. An interval cannot bridge a previous book's expiry
- `bids`/`asks`: 1–200 levels each, strict best-first price order without duplicate prices. Each level has exactly `price` (finite number in [0,1]) and `quantity` (finite number >0 and <=1e12). Crossed books are schema errors. Locked books are accepted. Book emptiness is insufficient for this baseline and rejected. These are displayed YES quantities including own orders, not assured executable liquidity

The source-version registry observes even bad-quality book records, so new segment IDs cannot make an older recovery book current. If malformed/future data poisons chronology, fix the source adapter and produce a new audited dataset; never ignore the offending rows without preserving why.

## Barrier-specific field

`reason` is exactly one of `gap`, `recovery`, `disconnect`, `stale`, `expiry`, `settlement`. A barrier has only the common keys and reason, not invented book fields. It invalidates its instrument's current continuity segment. Subsequent books with that same segment ID are excluded. Resume requires a distinct segment backed by producer-reviewed recovery, plus continuing global timestamp/version monotonicity. A gap in interarrival time, prior-book source freshness or known expiry also blocks the old segment; it is not silently treated as clean history.

The normalizer must emit barriers for every relevant lifecycle invalidation and mark all affected instruments. This consumer cannot reconstruct omitted barriers or verify an asserted recovery. A settlement barrier must not be followed by ordinary books unless a separately reviewed authoritative state reconciliation genuinely establishes a new valid instrument regime. Book-only recovery cannot prove market-state resolution.

## Split/config contract

See `fixtures/config.json` for all required fields; unknown/missing keys fail. Durations are bounded integers in milliseconds; horizon/max source age/max gap are positive and <=1 day. Tolerance is 0 through horizon. Embargo is at least horizon+tolerance and <=1 day. Cuts must leave a nonempty purged validation window. Alpha candidates must be finite, strictly increasing and positive (<=1e6); an ill-conditioned solve fails rather than reports a model. Seed is an unsigned 32-bit integer. Bootstrap replicate count is 100–10,000.

`minSamples`/`minSessions` require train/validation/test keys and positive bounded integers (samples >=2). Real-mode hard floors also apply; see README. Purging uses actual future row availability and prior feature availability, not only a nominal forward horizon. Label endpoints at a purge boundary are excluded; feature starts exactly on their allowed boundary are accepted. All split/config/source identities are recorded.

## Artifacts and audit

- `selection.json`: canonical source/config/implementation/model hashes and validation-selected alpha
- `split-manifest.json`: source-row IDs, session IDs and availability by partition; exact quality, missing-label and split exclusions. It has no holdout targets/features
- `report.json`: methods, configuration, source provenance, runtime hashes, counts, coverage, model/scaler, candidate validation metrics, development or final metrics, uncertainty and limitations
- `test-predictions.json` (final only): features, regression target/prediction and past/current/label rows' preserved source, receive and availability timestamps, sequence and quality flags with scoped/session/segment IDs
- `complete.json`: completion marker and hashes of every produced artifact. No completion guarantee without checking these hashes

Coverage is event-row coverage of the supplied input, **not universe coverage, trade-tape completeness, source validity, independent sample count or elapsed-time liquidity coverage**. Input timing gaps rejected as a whole segment can reduce retained data substantially. Busy instruments/sessions contribute more rows; the baseline does not silently rebalance them.
