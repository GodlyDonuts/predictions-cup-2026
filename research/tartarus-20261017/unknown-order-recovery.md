# Expired uncertain sale recovery — October 18

Production R41 stopped at 05:11:45 UTC after original SELL intent q18533 received HTTP 503 without a venue order ID. The SDK correctly retained unknown acceptance. The order was for 7,061 exchange1068 YES shares at .985, expiring at 05:19:42.555 UTC. Its original attempt, error, permanent key and financial history remain authoritative.

The checked finite discovery helper uses the stopped writer's global owner lease, original SDK queue, persistent backoff and 100-read/minute limiter. Its optimized ARM64 build and focused matcher test pass. It submitted zero orders. The complete history read beginning 05:31:40 returned 1,888 rows with no matching original request and no open orders. [The aggregate comparison](checked-production-lane/unknown18533-discovery-observation.json) verifies unchanged balance10,213.785 and all23 nonzero signed positions against native snapshot574171, including7,061 affected YES shares. These are dated observations; absence alone does not prove the original HTTP request never executed.

## Candidate recovery, not yet applied

The candidate introduces a separately named `expired_attempt_exposure_reconciled_v1` native event. It retires an expired, unbound reservation after economic reconciliation. It creates no venue ID, acknowledgment, fill, proceeds, cost-basis change or claim of zero network attempts. The original logical intent can never be retried.

The recovery factory is limited to reviewed inventory sales. It requires the restored original bank, sole unresolved original attempt, no pending acknowledged orders, exact SDK payload/key binding, unchanged full-account signed positions and cash, matching durable owner profile, no collateral ledger or obligations, and complete order history with no open or potentially matching order. Every physical observation must start at least60 seconds after the original expiry and be no more than60 seconds old at qualification. Known acknowledged orders are reconciled by their real IDs and complete fill tapes first.

Receipts and original intent/attempt/error hashes remain in a private immutable bundle. The native event commits before the separate dispatch resolution and SDK reconciliation. Startup restores trust only by rechecking the exact retained bundle and its original committed native source event. A serialized resolution flag cannot release an uncertain attempt. Crashes or incomplete evidence retain the stop; recovery never removes it automatically.

This changes native semantics and therefore requires the existing explicit engine-upgrade chain and a checked successor binary. It is being qualified in an isolated copy; no recovery, bridge installation, restart or profitability is established by this document. Following recovery, the sole worker still must perform its normal current-account preflight. Existing risk limits and daily windows remain unchanged.

Official source: [SIG API v1 documentation](https://sig.thesuper.market/api/v1/docs), captured anonymously on October18. GET `/orders` supports complete paginated history and expiry information but documents no read-by-idempotency-key endpoint. POST documents same-key replay, but this recovery submits no POST and does not bypass the SDK's uncertainty barrier.
