# Owned-order pagination in the learning simulator

The optional `OrderPagePolicy` v6 / `sig_scoped_owned_order_pages_v5` profile now exposes paid, scoped owned-order pages to training and evaluation. It is implemented and checked in the offline simulator. It is **not enabled in GPU017 or its already frozen pending018 handoff**, and it is not a production deployment or profitable checkpoint.

The contract under review is the locally saved official SIG OpenAPI, SHA256 `fbdf23e54d317392f9fb4a659b9511a01fad065e7a384f28091ef04894a732c9`, particularly `GET /orders` and its order/page schemas. This is a pinned saved version, not a claim of fresh live-schema verification. Original API documentation: <https://sig.thesuper.market/api/v1/docs>.

## Behavior

The model can choose open, closed, expired or all status; a limit from 1 through 200; market and exchange filters together; fresh queries or continuations. The default is 50 open orders. Every delivered continuation cursor remains independently selectable, including after another traversal finishes. No cursor or page becomes visible before its simulated HTTP receipt. Invalid scopes, cursors and limits incur a paid failed read without overwriting account cash or positions.

Each delivered cursor retains an immutable linked prefix of actually observed order identities. Reusing an old cursor can branch from its own prefix. Finishing a complete open/all scan may remove absent older orders only within the original filter and conservative starting revision. A multi-page scan never advances the global atomic order baseline: rows are read at each page's current virtual time. Newer private fills and ACKs take precedence over older read receipts. This prevents partial pages from pretending to be a complete account snapshot.

Simulated accepted placements, partial/full fills, cancellation, self-trade prevention and expiry update order history. Atomic basket failure restores that history alongside financial state. Open rows expose remaining quantity and no total-filled field; closed rows expose remaining plus filled quantity and total fills. Unfilled terminal records age out after 48 hours with explicitly incomplete coverage. Internal financial revisions are not presented as an official projection sequence. Query observations do not grant a free cash, position or collateral refresh.

Existing sampled fields select these query parameters; their joint probability is included by the inherited v5 policy density. The new architecture/protocol identity requires an explicit weight-preserving development migration, fresh optimizer/rollouts and new scores. Existing profiles keep their old semantics. This adds no artificial decision, position or quantity caps.

## Evidence and limits

The complete learning-actions suite passed **257 tests**. Fourteen owned-order tests include 401-order/three-page reads, cursor branching, combined filters, terminal history, archival coverage, stale receipt precedence, atomic rollback, migration, teacher probabilities, PPO updates, checkpoint restoration, population execution and sealed-episode replay.

A separate finite recorded-data integration used the actual `Experiment` factory, disk-backed three-venue corpus and new policy/tape types: two fresh H32 policies, two virtual worlds, 540 training decisions and 20 accepted updates. Its two-second training episodes and four-second selection window produced zero fills and 0% marked returns; these are plumbing checks, not strategy or profit evidence. A scripted paper-account probe on the same tape placed one valid-tick order, retrieved its paid open page, cancelled it and retrieved its paid closed page. Two reads and three writes were admitted (the third write was the profile's initial realtime-token request); no physical paper orders remained. The initial probe errors—busy-token handling and an invalid price tick—are retained explicitly in the evidence rather than counted as successful attempts. [Source-bound evidence](owned-order-pages-recorded-001.json).

Descending creation/ID keyset ordering, synchronous reporting projection, exact-time expiry and cancellation archival treatment remain explicit simulator assumptions pending exchange calibration. The implementation does not provide historical reporting-delay distributions or an atomic multi-page account view. Complete component collateral lending/lifecycle integration, official valuation coverage, the requested multi-day three-venue corpus and independent positive ROI remain unfinished. The [component ledger foundation](component-debt-ledger-20261019.md) remains separate and inactive.

GPU017 and CPU paired-return study002 continue from their frozen sources; pending018 remains the previously published evaluation-speed profile. No independent test was opened, no account client was created, and no production activation, STOP, limiter, risk cap or financial ledger was changed.
