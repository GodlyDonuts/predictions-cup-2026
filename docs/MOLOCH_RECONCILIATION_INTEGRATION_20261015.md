# Owned-order read reduction in the existing Rust worker

Task: [#126](https://github.com/GodlyDonuts/predictions-cup-2026/issues/126). Execution and deployment remain in [#120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120). This integrates the separately checked [read planner](MOLOCH_RECONCILIATION_PLAN_20261015.md), without changing the native strategy, bank identity, funding, activation deadline or shared account limits.

## Resulting behavior

Previously each pending actual acknowledgment required an exact order GET followed by its complete owned-fill tape. Twenty pending orders therefore required forty logical endpoint operations before the worker returned to market reads and dispatch. When more than one owned order is pending, the integrated worker now obtains one fresh complete open-order list. A fully matched, fresh owned member permits omission of that order's redundant individual GET. **Every owned order still receives its complete fill tape in every cycle.** A healthy twenty-order open cohort therefore needs twenty-one logical endpoint operations, including all twenty full tapes. Pagination and read retries can increase the physical HTTP count.

Pending means an actual ACK exists and the durable dispatch log does not contain its terminal `closed` proof. Native quotes already fully filled or retired remain pending under this definition. The planner receives the ACK-admitted native quantity after any resize, credited native fill count, exact actual binding, terms, original clocks and cancel state. It never substitutes a public open remainder for the admitted quantity.

## Integration and durable boundaries

`reconciliation.rs` contains an injectable orchestration layer. The worker's implementation uses its existing single `Transport`, request helper, account queue, receipt store and retry/backoff behavior. It calls the existing native `accept` function for each actual fill and the existing terminal event plus durable `closed` dispatch append for terminal proof. It creates no client, background task, timer or alternate writer.

The fill and projection caches exist only in memory and start empty on every process restart. Fill watermarks are created only after the validated complete tape's actual fill events have been durably applied and the current native filled count has been reread. Cache validation failure discards that cache entry and continues reconciliation of other owned orders; it never discards already applied actual fills. Pending-set pruning affects only these caches, not journals or order history.

Freshness is checked twice. Before requesting a member's tape, the aggregate must still fit the twenty-second bound. After the shared queue admits the request, the tape's actual request-start time and projection sequence are checked against the aggregate again. A queue wait can invalidate a hint that was fresh before the call. No tape events are applied until member/tape and complete fill-feedback validation both succeed.

If a hint is stale or its returned tape fails these validations, the worker performs the original exact order GET and then obtains an independent later full fill tape. The rejected hint tape is not credited. Missing, expired, changed, canceled, acceptance-uncertain and native-fully-filled members also use exact pairs. A malformed or incomplete aggregate receipt cannot authorize skipping an order GET. A transport failure, shared-stop/lease failure or uncertain durability still propagates through the existing error path; this optimization does not weaken those fences.

A member observation can establish only that an owned order appeared open in the aggregate. It cannot establish terminal status. Even if its full tape now completes the native quote, the dispatch log remains pending until a later exact order GET and later complete tape establish terminal proof. No aggregate row is rewritten into a counterfeit `/orders/{id}` receipt.

Foreign or inconsistent open-order evidence blocks new submissions for the cycle. Existing owned cancellation handling retains priority. The final production authority still independently refreshes and checks the complete account/open-order state before any authorized new submission. Aggregate observations and cache values do not replace those dispatch facts.

## Diagnostics and validation

The private production status gains `ownedReconciliation` counters: pending orders, aggregate reads, exact order reads, complete fill reads, validated member observations, stale/invalid hint fallbacks, watermark invalidations and whether new submissions were blocked. These are logical operations, not a claim of achieved requests per minute or profitable trading.

The injected-backend integration suite executes the real planner, real feedback parsing and orchestration against synthetic official receipts. It covers:

* Twenty open orders: twenty-one operations, twenty complete tapes and no synthetic terminal claims.
* A sixty-second gap between cycles: a newly obtained aggregate remains usable; prior loop duration is not mistaken for snapshot age.
* Slow cohorts and a simulated twenty-five-second admission wait: later members fall back to exact GET plus a later tape.
* Invalid member projection before application, cancellation/missing rows and fully filled but dispatch-unclosed quotes.
* Foreign and malformed aggregates blocking new submissions while preserving complete owned reads.
* Failed native commit preventing fill-watermark advancement, and invalid cache metadata not starving other owned reconciliations.
* Propagation of injected aggregate durability failure without subsequent network calls, and unchanged two-read behavior for a single pending order.

This suite is an orchestration regression, not a live HTTP benchmark or profitability test. Linux test and release-build evidence must be recorded by the parent before deployment. Existing production continues while this source is checked; this document does not restart a runtime or clear a stop.
