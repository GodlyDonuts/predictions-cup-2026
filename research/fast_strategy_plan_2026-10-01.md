# Predictions Cup: faster, evidence-driven strategy plan

Prepared 1 October 2026, 20:02 UTC. Public documentation checked on this date.

## Recommendation

Build a continuously running, event-driven opportunity scanner with selective market making. Use a VPS for uptime and demonstrably better latency after benchmarking. Faster trading is useful only when an expected edge survives execution costs, adverse selection and inventory risk.

This plan proposes engineering and research work. It does not authorize trading, account changes, credential creation or purchases.

## Current engineering status and scope

The engineering review reported at approximately 20:02 UTC that PR #4's four previously identified defects have been fixed and passed independent review at commit `157b2e2`, with 11 tests passing. The prior defects are historical context, not an assertion that this reviewed commit still contains them. This documentation research did not independently inspect the repository or rerun those tests.

A bounded, read-only, one-shot scan of the reviewed commit has been authorized. Trading has not been authorized by that instruction. The next milestone already assigned to Drogon is WebSocket ingestion and shadow evaluation under the existing issue #1. Preserve those changes and scope; do not replace the reviewed work with a separate implementation.

## Latest paper-training request

The user's 20:02 UTC request prioritizes a separate paper/shadow simulator and chronological, leakage-controlled ML evaluation while preserving competition capital. It also asks Drogon to change its existing coordination checks from 15 to 5 minutes. See the [recorded request](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5939519430) for the current scope. This research publication does not change a schedule, authorize competition order writes, or establish that a cadence change has succeeded.

## Verified platform constraints

### Realtime data

- Supabase Realtime WebSocket feeds are documented. No separate SSE interface was identified.
- Tournament market and account changes are grouped into approximately one-second batches. Tournament market messages include full order books for the exchanges they name, with an `asOf` version.
- The tournament market topic is `tournament:{tournament_id}:market:{market_id}`. Use the documented account channel returned by token setup for account events.
- Delivery is best-effort and has no replay buffer. Reconnects, token refreshes, socket errors, revision gaps, `resyncRequired` flags and trades lacking a sequence require authoritative REST recovery.
- Apply book versions by `sequence`, then timestamp, including a newer book carried by an otherwise duplicate batch. The batch topic revision and engine sequence are different counters.
- No event announces order expiry. Honor `nextExpiryAt` and refresh the relevant book after it.

### Rate limits and order operations

- Standard accounts have 100 read requests and 30 write requests per minute, shared across all keys. More keys do not add capacity.
- Bulk prices accept up to 100 exchange IDs per request.
- A placement batch supports up to 50 orders in one write request. It is best-effort, and already placed items remain after partial failure.
- Multi-leg placement supports up to 10 legs with atomic placement persistence. This does **not** document an all-legs-fully-filled guarantee or make a strategy automatically risk-free.
- Realtime token creation is a write. Open Realtime connections do not consume the REST request budget.
- Standard orders use positive integer quantities, hence a minimum of one share, and 0.005 price ticks between 0.005 and 0.995. Limit-order expiration is supported.
- Participant post-only, IOC/FOK and amend functionality were not found in the reviewed schema. No maker-rebate schedule was identified. Do not model these as supported features.
- DMM liquidity replacement and its queue-preserving behavior are administrator capabilities, not participant features.
- The v1 API is the supported interface for bots. Do not depend on the website's private API routes or attempt to avoid account rate limits.

Sources: [API reference](https://sig.thesuper.market/api/v1/docs), [1 October rate-limit and access changes; 28 September realtime books](https://sig.thesuper.market/docs/changelog), [market channels and one-second batching](https://sig.thesuper.market/docs/changelog?page=2).

## Prioritized next steps

### 1. Complete the authorized read-only scan

Run only the already authorized bounded scan on the reviewed commit. Record tournament identity, market discovery coverage, missing IDs, available book depth and data freshness. A scan result is evidence about observed conditions, not evidence that a profitable strategy exists.

All context-sensitive reads must specify the intended tournament. Do not silently fall back to global prices or assume an organization default cannot change.

### 2. Build and verify the realtime/shadow foundation

Subscribe first, obtain authoritative snapshots, and maintain local state using documented book versions and event deduplication. Recompute opportunity rankings after each useful batch, without making every computation an order request.

Use one account-wide limiter across every process and key. A proposed operating target is at most 80 reads and 24 ordinary writes per minute, leaving headroom for recovery, token refresh and risk cancellation. This is a design target, not a platform requirement. Reduce activity further when engine throttling or recovery demand warrants it.

Batch compatible placements and prioritize risk-reducing cancellations over new exposure. Avoid unnecessary cancel-and-replace churn. Narrow the actively quoted market set to what the cancellation budget can safely manage.

Persist the logical request ID, idempotency key and exact payload before placement. After an unknown result, reconcile or retry that identical operation with its original key. Never create a new key merely because an existing request timed out. Handle batch results item by item; an outer success or partial-success envelope does not mean every item filled.

Fail closed when data is stale, recovery is incomplete, the tournament context changes or account reconciliation fails. Use order expiry as an additional safeguard, not a substitute for recovery and cancellation handling.

### 3. Compare three testable sources of edge

**Cross-market consistency.** Use canonical relationship metadata and actual settlement wording to identify equivalent, exhaustive or mutually exclusive outcomes. Evaluate executable prices at the required size. Separate Democratic and Republican markets are not automatically perfect complements: third-party outcomes, candidate affiliations and fusion-party settlement rules can matter. Atomic placement alone does not remove legging and partial-fill exposure.

**Selective market making.** Quote only where expected spread capture exceeds adverse selection, inventory risk and the cost of unwinding. Rank opportunities by expected net value per scarce write request, rather than by expected trade count. Maintain genuine executable orders sized to the strategy's risk limits; no self-trading, deceptive quotes or attempts to manufacture reported marks.

**Reference-price dislocations.** Anchor fair probabilities to independent, timely information matched to the exact contract. Use microprice, book imbalance, recent aggressive flow and volatility as short-horizon execution features. Neither last traded price nor a local order-book signal should be assumed to be the true outcome probability.

SIG documents peer-to-peer matching, opposite-side netting and self-trade prevention. Liquidity must actually exist for an execution. Source: [Markets & Trading](https://sig.thesuper.market/docs/markets-and-trading).

### 4. Start with models that can be falsified

Begin with a regularized short-horizon flow/price model and an uncertainty-aware fair-probability baseline. Separate long-horizon outcome information from short-horizon execution signals.

Inventory should shift the quote midpoint. Volatility, uncertainty, thin depth and stale reference data should widen spreads or suppress quoting. Cap correlated exposure across related races and chamber-control markets, as well as exposure in individual contracts.

Promote a more complex model only if walk-forward, out-of-sample results improve after conservative execution assumptions. An LLM on each tick or a higher trade count is not evidence of a better model.

## Acceptance tests before any separately authorized trading

1. **Data integrity:** duplicate and out-of-order events, reconnects, token refreshes, revision gaps and expiry are replay-tested. No stale state can produce a trade decision. Identical event replay produces identical state.
2. **Execution safety:** partial fills, cancellations racing fills, response loss, retries, complement-side selling, FIFO position netting and collateral reconcile to authoritative records. Unknown orders cannot be duplicated.
3. **Capacity:** routine activity remains inside the shared account budget and retains cancellation/recovery headroom. The risk-off path still works near the budget boundary.
4. **Conservative fills:** shadow simulation uses available depth, pessimistic queue assumptions, latency, slippage and verified costs. Merely touching an order's price does not establish a fill. No assumed maker rebates. Verify the virtual-currency ledger rather than treating “no real-money fees” as proof of every execution-cost assumption.
5. **Real economics:** report realized trading P&L, estimated executable liquidation value and terminal settlement exposure separately from displayed account marks. Assess post-fill adverse movement and opportunity-level capital usage.
6. **Model evidence:** net edge survives out-of-sample evaluation and adverse execution scenarios. Until then, continue shadow observation rather than treating activity as success.
7. **Risk controls:** test stale-feed shutdown, inventory limits, correlated-event loss scenarios and account reconciliation failures before activation.

## VPS evaluation

Benchmark candidate regions before selecting one. Measure warmed API response-time p50/p95/p99, WebSocket event age, reconnect frequency, process availability and local scheduling jitter. Public-document download latency is not an order-engine latency benchmark.

Sustained bandwidth, network round-trip latency and uptime are separate properties. This workload does not automatically require high bandwidth. A VPS can prevent a laptop sleeping or disconnecting, but its region must be chosen from measurements. Approximately one-second feed batching also limits what a millisecond network improvement can accomplish.

Use shadow traffic within the authorized scope for the initial comparison. No VPS purchase or credential setup was performed as part of this research.

## Leaderboard and settlement interpretation

Current standings show competition account value. The API says tournament positions use `valuation_price`, but the reviewed public documentation does not specify the interim marking formula. An account-value gain therefore need not equal cash already realized or the proceeds available from liquidating the whole position at current depth. This observation is not evidence of a marking exploit.

The timestamped browser review found that All Time and Today displayed different leaders and figures. Do not describe an All Time gain as profit earned in the first few hours of the current competition. Trade counts and rank alone do not establish competitors' execution speed, order frequency, strategy, position sizes or realized profit.

The API defines calendar-aligned leaderboard periods and explicitly describes reset boundaries for Ongoing Play seasons. Changelog entries describe how rankings should clear following a reset. Neither fact proves a particular reset occurred for the current SIG tournament or establishes what earlier activity a screenshot includes.

**SIG-specific final ranking:** the Cup guide says the competition runs from 1 October 2026 at noon Eastern until 4 November 2026 at noon Eastern, with final rankings confirmed after every market settles. Final rank is determined by final SUSQie balance, starting from 100,000 SUSQies and requiring at least one completed trade. Winning shares pay one SUSQie; losing shares pay zero. Pending chamber-control questions and runoffs may delay finalization.

**Do not substitute generic platform closeout rules.** The API also documents a generic tournament closeout mechanism involving a proposed five-hour VWAP, and a tournament option to await actual outcomes. That generic mechanism is not evidence that the SIG Cup cashes out unresolved positions at a five-hour VWAP. The SIG-specific guide explicitly says finalization waits for all outcomes.

Sources: [public leaderboard](https://sig.thesuper.market/leaderboards), [Portfolio & Standings](https://sig.thesuper.market/docs/portfolio-and-leaderboards), [Cup results and schedule](https://sig.thesuper.market/docs/tournaments-and-organizations), [SIG settlement rules](https://sig.thesuper.market/docs/settlement-and-payouts), [standings and settlement-mode changes](https://sig.thesuper.market/docs/changelog?page=3), [reset behavior](https://sig.thesuper.market/docs/changelog?page=4), [API reference](https://sig.thesuper.market/api/v1/docs).

## Research boundaries

This research read public documentation only. It did not use account credentials, place or cancel orders, call authenticated APIs, purchase infrastructure, or modify repository code. Current engineering status and leaderboard observations above are explicitly attributed to the corresponding timestamped engineering and browser reviews rather than represented as independent checks by this researcher.
