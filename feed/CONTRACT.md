# Source contract and assumptions

Reviewed 2026-10-01 around 21:01 UTC against the public [official API v1 reference](https://sig.thesuper.market/api/v1/docs), Introduction → Realtime feeds and Realtime → POST /realtime/token, and GET /markets/{id}/orderbook. OpenAPI info version 1.0.0, OpenAPI 3.1.0. Public documentation only was retrieved; no authenticated request or token operation was performed.

## Implemented documented semantics

- Tournament topic: `tournament:{tournament_id}:market:{market_id}`. All channels are private broadcasts. Global market topics are not substitutes for tournament context
- Event: `market_batch`, with `trades`, `bookDirty`, `marketSettled`, optional `books`, and `delivery`
- Delivery model: `best-effort-authoritative-resync`; `revision` and `previousRevision` describe a topic-local covered range. A gap occurs when previousRevision exceeds last accepted revision; duplicate or older revisions do not reapply arrays. Engine `sourceSequenceFrom`/`sourceSequenceThrough` are provenance, not the topic gap counter
- Full pushed books are per exchange, YES-perspective, sorted best-first, own orders included, versioned by `asOf.sequence` then exact timestamp instant. Newer books apply even in duplicate batches. Sequence gaps between books are not topic gaps
- `resyncRequired` can have no arrays. Check it before processing trades; retained books still apply by version
- Initial subscription, reconnect, token refresh, socket error, revision gap, compact/oversized resync, unknown trade sequence and expiry all require recovery. `book_dirty` and `market_settled` may also arrive individually
- Trade shape: `{id, sequence, exchangeId, marketId, price, quantity, executedAt, tournamentId}`; null id/sequence is unorderable and triggers recovery. Prices are YES-denominated, regardless of YES/NO space
- `nextExpiryAt` on pushed books requires a subsequent authoritative read; expiry itself emits no event
- Snapshot route is GET `/markets/{marketId}/orderbook?tournamentId={uuid}&depth=200`. Select exactly the matching `contexts[]` entry rather than relying on potentially different legacy top-level fields. Reject incomplete or foreign exchange coverage
- Standard account limits are shared across keys: 100 reads/30 writes per minute. `POST /realtime/token` consumes a write; open connections do not consume REST budget. The collector has a GET admission/cooldown callback only

## Conservative local decisions (not extra platform guarantees)

Local capture/fixture schema version 1 is our format version; the service does not publish a separate event schema version. The known `delivery.model` and required fields form the contract fingerprint; unknown fields are not persisted and malformed required fields invalidate state. No cryptographic payload signature verification is claimed: server-signed/private-channel provenance must be established by the future supported transport.

Timestamps accept valid calendar instants from 1970 onward, explicit Z/offset and up to 9 fractional digits. Comparison preserves nanoseconds; local arrival/latency clocks use milliseconds. Invalid dates and unsafe numeric counters are rejected instead of rounded. Scope and exchange IDs are explicitly configured; no discovery completeness is inferred. Empty depth is valid, unsorted/repeated-price levels and nonpositive quantity fail closed. Future contract expansions may need a reviewed parser update.

REST `asOf: null` is documented but cannot establish versioned readiness here. REST expiry metadata is retained as unknown even if an extension supplies the field; only the documented pushed-book field establishes expiryKnown. Settlement remains blocked pending authoritative market-state implementation. This milestone recovers books only: missing trade tape, settlement labels, fees, order ownership, aggressor direction, queues and account events remain unresolved.
