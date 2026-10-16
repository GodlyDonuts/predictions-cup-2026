# Can an unchanged SIG book be treated as freshly observed?

**The documented realtime feed provides no fresh per-exchange completeness
heartbeat. Silence, global socket health and other-market updates cannot renew
a held book.** The frozen trade sampler's stale-book rejection remains correct
as an observation-coverage decision. It does not prove the economic book has
changed or that its last quote is wrong; current evidence is unavailable.

The execution owner reported16 first-observed public prints consumed by the
new read-only cohort at15:07:11 UTC:1 missing SIG,1 old trade and14 stale SIG,
with0 samples and sampling enabled. Those counters are not outcome evidence.
This audit has not inspected the exact original rejected-trigger book/frame
pairs, so it does not identify which individual age or delivery interval caused
each rejection. No current cohort, timestamp or age limit is changed here.

## Retained primary specification

The original saved official OpenAPI has SHA256
`6b5ff9a661c06f1a2d1dda131bf870a70f4f79699ac9937375a278861e1f81bf`
(`runtime/moloch/api-spec-ioc-20261014.json`, retained privately). Relevant exact
JSON pointers are:

- `/paths/~1realtime~1token/post/description`: delivery model, versioned books,
  aggregate `markets_batch`, initial/reconnect/periodic REST resynchronization.
- `/components/schemas/BookVersion`: original `sequence` and `at` semantics.
- `/paths/~1markets~1{id}~1orderbook/get/description` and
  `/paths/~1exchanges~1{id}~1orderbook/get/description`: explicit tournament
  scope, per-exchange version and depth-limited REST books.
- `/components/schemas/CombinedOrderbookExchange`: REST book fields. This schema
  has no `nextExpiryAt` field; the realtime book description does.

This is evidence from the retained specification, not a claim that a new online
schema was fetched or that source descriptions constitute a lossless service.

`asOf.at` is documented as the **engine clock when the book was read**; sequence
identifies the last applied engine event. It is not defined specifically as the
last change in best price. Calling it a price-change clock would conflate two
different notions. Versions compare by sequence and then timestamp.

Realtime is best effort. Changes are coalesced, sent once, and can be dropped.
A topic's final batch can be lost while it subsequently stays quiet, so no later
gap need reveal that loss. Periodic authoritative REST resynchronization is
explicitly required even with a connected socket and no observed gap.

## What the original messages do and do not prove

| Evidence | What it establishes | What it cannot establish |
| --- | --- | --- |
| Phoenix heartbeat reply / socket health | The connection has responded | Target exchange current depth/version, account or trade completeness |
| A `markets_batch` listing other markets | Those named market batches were delivered | A tournament-wide snapshot or coverage through receipt time for omitted markets |
| A target market batch with contiguous revision | Delivery continuity relative to the last accepted revision, through the named batch | The final unseen revision, an independent current wall clock, or completeness after that batch |
| Engine source-sequence range | Provenance for events producing that batch | A global topic gap counter or a no-change timestamp for every exchange |
| Explicit complete target book in a pushed batch | A full snapshot read at publication, carrying its original engine version | An independent publication wall-clock or bounded network/consumer lag when only `asOf.at` and receipt time are available |
| New scoped REST book request/response | An explicit authoritative read of that exact context during the actual request interval | Lossless future delivery, account/fill completeness, or unchanged state after the read |

The aggregate tournament message contains only changed markets. Each has its
own revision; the aggregate has none. A named market's books include every
exchange named by its trades/book-dirty signals, not every tournament exchange.
Exchanges not named have not changed *through that batch*, but this assertion
does not close the missing-final-batch problem or extend coverage to current
receipt time. Settlement-only batches need not contain books.

Books can be newer than their batch's trades, and different books in the batch
can have different versions. Duplicate-revision books still apply by version.
`resyncRequired` can mean truncated arrays or a compact message with no arrays;
it requires REST recovery. A null match sequence also requires recovery. Neither
an empty book array nor an empty trade array is a fresh exchange-wide witness.

## Current source behavior

The native [realtime transport](../../../moloch/native/src/realtime.rs) consumes
heartbeat acknowledgments internally; they produce no scoped source coverage.
It emits actual original broadcasts with local receipt clocks.

The native [source parser](../../../moloch/native/src/source.rs) checks market
revision ranges and original book version/depth. Subscribe/reconnect creates
scope gaps. Revision gaps, compact/resync batches, unordered matches and
settlements invalidate the scope. It normalizes realtime `sourceAt` from
`book.asOf.at`; local `requestStartedAt` and `receivedAt` are receipt clocks.
They are not independent remote publication timestamps. Normalized books do not
retain the original realtime `nextExpiryAt`, although the parser validates it.
An independent expiry-aware witness would therefore need the original batch.

The [observer](../../../moloch/external-data/crossvenue_observer.py) also retains
original `asOf.at` as SIG source time. Its normalized breadth path does not emit
a coverage event for silent or omitted exchanges. No timestamped all-exchange
checkpoint exists in this schema. Manufacturing one from every incoming frame
would make a missed final target batch appear current.

## A separately versioned future read-witness design

There is **no documented realtime-only path** to a fresh independent clock for
a silent book. A newly received pushed full book is stronger evidence than
silence, but receipt time alone cannot bound publication age. Replacing its
old engine clock with local receipt time would hide a delayed old snapshot.
No such implementation is proposed for the current cohort.

A defensible separate research mode could consume an **original scoped REST
book already obtained by the sole account worker**, with no second client or
extra account request initiated by the observer. It would retain both:

1. The unchanged original book version/engine-clock/depth/hash, and the exact
   tournament/market/exchange and underlying original receipt.
2. The genuine fresh request-start, response-received and first observer
   availability clocks, plus original request endpoint/context/depth. The
   request start is a conservative lower bound for that explicit read, not an
   invented new engine or price timestamp.

The proof must bind the latest original retained receipt, not a caller-created
JSON assertion or old receipt loaded again. Its original request interval and
first availability must meet separately frozen bounds; using a generic API
coverage flag or global cash/account receipt is insufficient. A null book
version is not silently equated with a sequenced realtime snapshot. A newer
known realtime version wins; an older REST response cannot downgrade it. Equal
versions with conflicting depth are rejected, not averaged or relabeled.

The future mode also needs the actual subscription/source epoch, last accepted
per-market revision and any intervening gaps, reset/refresh/dirty/settlement/
resync signals. A snapshot before the latest unresolved invalidation cannot
repair it. Known expiry clocks must invalidate the witness when crossed; their
absence must not be represented as proof that no order can expire. A full
fresh REST observation is a point read, not continuous no-change proof over
the following interval. No stronger expiry semantics are invented from the
REST schema's missing field.

Such a mode requires independent policy/versioned sample IDs, original read
provenance and a fresh prospective cohort. It must keep source age, read age,
delivery age and unknown continuity separate, and censor future labels on
actual gaps as before. Its financial eligibility would remain a different
question under the production authority. There is no measured recovery count
or expected alpha gain without original candidate receipt/book/trigger joins.

To assess feasibility using existing data, the next bounded evidence packet
would contain one rejected original trade/flow snapshot and its exact cached
SIG row, the last scoped gap/reset/expiry signals, and any later original same-
scope REST book receipt with request/response clocks and complete depth/version.
No absent update, global heartbeat or collector counter can substitute for it.

This task changes only this document. No SSH, account/API request, build,
collector/observer/source change, timestamp rewrite or production action ran.
