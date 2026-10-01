# Current state

Last portfolio review: **2026-10-01 19:52:07–19:53:13 UTC**. Exact cash and Alaska holdings were verified at 19:52:07; no open orders appeared at 19:52:36 and again with transaction history at 19:53:13. The previous four position quantities were unchanged. Data is historical and needs fresh reconciliation before any action.

## Account reconciliation

- Starting simulated cash: 100,000.00 SUSQies
- Current verified cash: 92,751.88 SUSQies
- Original cost of five held positions: 7,248.12 SUSQies
- Cash plus original cost: 100,000.00 SUSQies
- Open orders: none observed at 19:52:36 and 19:53:13 UTC
- Latest recorded order: Alaska Democratic Senate **NO**, market **377**, **5,000 shares**, fully filled for **1,649.50**; effective average **0.3299** is derived from the exact transaction debit. The UI displays a rounded 0.330. Submitted once at approximately 19:51:35 UTC; holdings/cash confirmed at 19:52:07 and transaction history at 19:53:13. Order ID and exact execution timestamp were not exposed and remain unknown
- [Alaska source audit](../data/orders/order_alaska_2026-10-01_1951.json); single-order cap 1,650.00 SUSQies, actual debit 0.50 below that cap. No fee line was displayed; no general fee schedule is inferred

Machine-readable current view: [`data/portfolio/latest.json`](../data/portfolio/latest.json). Trade ledger: [`data/trade_ledger.csv`](../data/trade_ledger.csv). Source snapshots: [`data/snapshots/`](../data/snapshots/).

## Important limitations

The portfolio page uses tournament valuation prices whose formula remains unverified. Its marks differed sharply from detail-page prices. Neither is a guaranteed full-position exit price. Current displayed gains/losses are not realized P&L.

See the [execution and valuation review](../research/execution_valuation_review_2026-10-01.md) for historical depth and exact observation windows. Quotes were not captured simultaneously and are already stale for execution.

If no trades, refunds, or sponsor interventions occur, the final payoff endpoints for these five positions are 92,751.88 SUSQies if all lose and 115,922.88 if all win. These are scenarios, not forecasts. Rhode Island and Massachusetts together have only 134.35 of possible upside from their combined entry cost.

## Current operating mode

- dot: execution coordinator, under separate user authorization
- Codex/Drogon: research, quantitative strategy and development only; bot API writes remain disabled
- No API key created or stored by repository setup
- No runner, autonomous trader, scheduler, or repository webhook enabled by setup
- Review current rules and exact market resolution conditions before relying on any forecast

## Next useful work

1. Reconcile fresh five-position state before any new scan, trade proposal or execution
2. Establish a read-only, tournament-specific reconciliation path using official v1 documentation
3. Investigate the valuation discrepancy without assuming it is a bug or an arbitrage opportunity
4. Review [Florida/NC conservative scenarios](../research/fl_nc_conservative_bands_2026-10-01.md) and the [new-race shortlist](../research/new_edges_2026-10-01.md). Subjective bands and illustrative price hurdles are not calibrated probabilities or standing executable limits
