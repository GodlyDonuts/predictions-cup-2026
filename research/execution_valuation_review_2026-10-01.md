# Predictions Cup execution and valuation review

Observed 2026-10-01, 18:50–18:55 UTC. Read-only review in the signed-in cloud browser. No orders, cancellations, credentials, or account settings changed.

## Verified holdings

Cash: **94,401.38 SUSQies**, shown exactly on every inspected market detail. Four positions; portfolio showed **no open orders** at 18:50–18:51 UTC.

| Market | ID | Holding | Average cost | Cost basis |
|---|---:|---:|---:|---:|
| [Democratic Rhode Island Senate](https://sig.thesuper.market/markets/387?tournament=midterm-elections) | 387 | 2,072 YES | 0.965 | 1,999.48 |
| [Democratic Massachusetts Senate](https://sig.thesuper.market/markets/379?tournament=midterm-elections) | 379 | 2,061 YES | 0.970 | 1,999.17 |
| [Democratic Florida Senate](https://sig.thesuper.market/markets/354?tournament=midterm-elections) | 354 | 12,500 YES | 0.120 | 1,500.00 |
| [Democratic North Carolina Senate](https://sig.thesuper.market/markets/277?tournament=midterm-elections) | 277 | 1,538 NO | 0.065 | 99.97 |

Total original cost = 5,598.62; cash plus this cost = 100,000.00. Florida filled order 910025 was separately verified earlier at 18:47:09 UTC and all 12,500 shares are visible now. The reconciliation shows no unexplained cash debit in these holdings. It does not independently establish the platform's full fee policy.

## Valuation discrepancy: what is established

At 18:50 UTC the [Portfolio page](https://sig.thesuper.market/portfolio) displayed account value **98,974** (rounded), holdings value **4,573.08**, and these marks:

- Rhode Island YES: 0.664, value 1,376.61, unrealized loss 622.87
- Massachusetts YES: 0.629, value 1,296.55, unrealized loss 702.62
- Florida YES: 0.128, value 1,601.89, unrealized gain 101.89
- North Carolina NO: 0.194, value 298.03, unrealized gain 198.06

The marks move continuously; they are rounded and were not captured atomically with all later book observations. On the market-detail pages shortly afterward, the same quantities and cost basis were valued instead at:

- Rhode Island 0.970, value 2,009.84, P&L +10.36
- Massachusetts 0.975, value 2,009.48, P&L +10.31
- Florida 0.115, value 1,437.50, P&L -62.50
- North Carolina NO 0.060, value 92.28, P&L -7.69

The public API [open-positions documentation](https://sig.thesuper.market/api/v1/docs#tag/portfolio/GET/portfolio/positions) and TournamentPortfolioPosition model call the portfolio currentPrice a **tournament valuation price**. The documentation reviewed does **not define its formula**. No public documentation was found explaining why it differs this much from market-detail marks. These observations establish an unexplained difference between two valuation views; they do **not** establish a bug, stale data, global/tournament-context mix-up, a fee, or a particular smoothing formula.

Neither valuation view is an executable quote for the full holding. A weighted liquidation estimate must walk the actual bid depth. Do not treat the portfolio's apparent ~1% account loss, or the detail page's small gain/loss, as realized P&L.

## Executable order-book snapshots

Quantities below are shares. Bid means the price received for selling the held side. Ask means the price paid for buying that side. These are observations, not commitments or fill guarantees.

### Rhode Island YES, 18:51 UTC

- Best bid: 100 @ 0.960
- Additional bids: 75 @ 0.945; 516 @ 0.910; 999 @ 0.840; 96 @ 0.830; 32,884 @ 0.760
- Best ask: 25 @ 0.965
- Additional asks: 50,121 @ 0.970; 50 @ 0.975; 25,537 @ 0.980; 502,470 @ 0.995
- Detail mark: 0.970
- Selling all 2,072 against this static visible bid ladder would return approximately **1,772.64**, average 0.85552, a loss of **226.85** against cost. Only 100 shares were available near the displayed top bid.

### Massachusetts YES, 18:52 UTC (top depth reconfirmed 18:53)

- Best bid: 1,680 @ 0.965; next 525 @ 0.960
- Further bids: 50 @ 0.945; 1,063 @ 0.940; 3,314 @ 0.905
- Best ask: 10,915 @ 0.975; next 4,381 @ 0.980
- Detail mark: 0.975
- Selling all 2,061 against that static bid ladder would return **1,986.96**, average 0.96408, a loss of **12.21** against cost

### Florida YES, 18:53 UTC

- Best bid: 60,033 @ 0.110; next 18,728 @ 0.100
- Best ask: 200 @ 0.115; then 50 @ 0.120; 1,600 @ 0.130; 585 @ 0.180
- Detail mark: 0.115
- Selling all 12,500 at the observed best bid would return **1,375.00**, a loss of **125.00** against cost
- The lowest displayed buy quote does not cover a sizable order: only 250 shares were offered at or below 0.120

### North Carolina NO, 18:55 UTC

The orderbook was switched to NO perspective for a direct read.

- Best bid: 17,358 @ 0.060; then 2,350 @ 0.055; 100 @ 0.050; 4,160 @ 0.045
- Best ask: 50 @ 0.065; then 6 @ 0.070; 217 @ 0.080; 100 @ 0.085
- Detail mark: 0.060
- Selling all 1,538 at the observed best bid would return **92.28**, a loss of **7.69** against cost

Combining these **non-simultaneous, static snapshots** gives illustrative full liquidation proceeds of 5,226.88 and cash-plus-proceeds of **99,628.26**. This is not a current account valuation or guaranteed execution result. Cash plus the four detail marks is approximately **99,950.48**, also a synthetic non-simultaneous total.

## Final ranking and settlement

The [Official Rules](https://predictionscup.com/rules/) distinguish interim standings from final scores:

- Final rank is based on verified total SUSQies after **all markets resolve** (§10); interim rankings are dynamic and provisional
- Trading ends **November 4, 2026, noon Eastern Standard Time**, equivalent to 17:00 UTC (§6). Positions then lock
- The [Settlement & Payouts guide](https://sig.thesuper.market/docs/settlement-and-payouts) specifies 1 SUSQie for each winning share and 0 for a losing share; payouts enter the holder's competition balance
- Cancelled/N/A outcomes refund the refundable cost of remaining shares
- Final rankings wait for every market, including runoffs and manually settled chamber-control questions, to settle
- Therefore current tournament valuation marks are not a substitute for the final event-based payouts described in the rules

If no further trades occur, no refunds/interventions occur, and all four positions win, final cash would be 112,572.38. If all lose, it would remain 94,401.38. These are payoff endpoints, not forecasts. The current portfolio has only 5,598.62 of original cost exposed. Rhode Island and Massachusetts together cost 3,998.65 for a combined maximum payout of 4,133, so their combined best-case gain from entry is only 134.35.

The rules say SUSQies are non-monetary, nonredeemable competition currency and there is no participant cost, fee, or financial obligation (§3). That statement concerns entry/participation; the reviewed guide does not separately describe a simulated transaction-fee schedule. Observed costs reconcile exactly with fills.

## API and bot implications

[Public API reference](https://sig.thesuper.market/api/v1/docs), [trading guide](https://sig.thesuper.market/docs/markets-and-trading), and [October 1 changelog](https://sig.thesuper.market/docs/changelog):

- Automated trading strategies are expressly permitted. The official rules allow multiple bots on a single participant account; only one account per participant
- The only supported programmatic interface is **/api/v1**. Website-internal endpoints may block automated requests
- All API endpoints require an API key. Scopes are read (data and portfolio), trade (order placement/cancellation), and privileged admin. No key was created, read, entered, or stored during this review
- Standard per-account budget: **100 reads and 30 writes per minute**, shared across all keys. Open realtime connections do not consume this budget
- Bulk price reads support up to 100 exchange IDs per call
- Realtime market/account updates arrive in one-second batches. Delivery is best-effort, without replay; REST reconciliation is required on reconnect, token refresh, sequence/revision gaps, errors, or resyncRequired
- Realtime tokens expire in 3 hours. API-key expiry choices are no expiry, 30 days, 90 days, 6 months, or 1 year
- Use explicit tournament context consistently. Orders refer to **exchangeId**, not marketId. Resolve the Midterm Elections tournament and pass its UUID where supported; use tournament-specific portfolio endpoints
- Fresh limit prices use a **0.005 tick**, between 0.005 and 0.995
- Order creation requires idempotency keys. Retry only the identical logical operation with the same key after uncertain outcomes; reconcile fills and state before submitting anything new
- Supported batch orders are best-effort, up to 50; atomic multi-leg requests support up to 10. An atomic request is not a promise every order fills
- Bots cannot assume sponsors maintain liquidity. Rules explicitly allow sponsor liquidity to change or disappear; API/order errors are not guaranteed to be corrected

Recommended engineering safeguards: begin with read-only reconciliation of tournament cash, holdings, fills, portfolio valuation prices, and executable depth; retain separately named portfolio marks, last/detail marks, midpoint and liquidation estimate; record source timestamps; set exposure and slippage limits; use limit orders, idempotency, a kill switch, and explicit stale-data handling. Do not optimize for unexplained interim marks or chase first place by treating paper marks as executable arbitrage.

## Unresolved limits

1. Exact tournament valuation formula and reason for divergence from detail marks remain **unverified**
2. Order books change quickly; all depth above needs a fresh read before any decision or execution
3. No authenticated API request was made, so API-vs-UI valuation consistency is untested
4. This review did not research election probabilities or recommend increasing/decreasing a specific position
5. No trades, order cancellations, account changes, credential creation, or external messages were performed
