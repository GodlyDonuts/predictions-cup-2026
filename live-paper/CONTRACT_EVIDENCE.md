# Fresh scoped contract evidence

`contract-evidence.mjs` captures the official `/markets/{id}` and
`/markets/{id}/nodes` endpoints with explicit tournament context, through the
existing collector's serialized client. The runner performs four reads once at
startup for NH381/382 when the basket observer is configured and no shared
cooldown or STOP applies. It creates no API client, recurring loop or order.

The reader gives short-lived permits only to its own exact GET requests, fenced
before queue admission, before network use and after receipt. STOP revokes the
permits. Partial results, foreign/default contexts, stale/overlapping receipts,
closed markets, scoped settlement overrides, mismatched exchanges, composite
nodes and incompatible predicates fail closed. A metadata failure cannot create
an eligible package; ordinary service failure leaves collection running with
an explicit unavailable status. Authentication/manual-review and durable journal
failures retain their existing fatal behavior.

Successful captures persist a curated projection, source hashes and individual
receipt times in restricted `contract-pair-evidence.json` and the hash-linked root
journal. Explicit market/exchange/contract identities, race/stage/election/date,
party predicates and future settlement time must agree. Evidence expires 120
seconds after capture starts. A title match is insufficient.

Distinct party-winner predicates in the same election support **conditional
normal mutual exclusion**. This does not establish an engine relationship,
refund compatibility, equal fills, override protection or unconditional profit.
`orderEligible` and `executionAuthority` remain false. A future package executor
must independently verify current account ownership, books/depth, API side-price
conventions, budget, acknowledgements and all partial-fill outcomes.

The API schemas used are checked in as `openapi.json`: `Market`,
`MarketPricingContext`, `MarketTreeContractNode`, and the two scoped market GETs.
Tests execute both the pure reader and the real runner body with injected IO;
they establish capture and fencing correctness, not trading profitability.
