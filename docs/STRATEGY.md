# Strategy and open questions

## Objective

Develop evidence-backed forecasts and execution discipline for final settled SUSQies. The [official rules](https://predictionscup.com/rules/) rank verified totals after all markets resolve. Interim leaderboard and portfolio marks are provisional. Trading ends November 4, 2026 at noon Eastern (17:00 UTC); final settlement may wait for runoffs or other unresolved markets.

This is a research framework, not authorization for an order or a standing instruction to increase risk.

## Evaluate each candidate

1. Read the exact market resolution terms and identify authoritative sources, close/settlement timing, exceptions, and sponsor discretion.
2. Build an independently justified probability range; keep the date, inputs, method, and uncertainty visible.
3. Compare it with actual available depth and all known costs. Record unverified fees explicitly rather than assuming zero.
4. Calculate entry cost, maximum loss, settlement scenarios, and correlated exposure. Stress-test the portfolio as a whole.
5. Separate forecast edge from structural payout relationships. Apparent cross-market inconsistencies need matching outcomes, executable depth, funding, and settlement terms.
6. Reconcile fresh account state and use bounded limit prices only within separately authorized execution scope.

## Existing positions

The five positions and cost basis are recorded in `data/portfolio/latest.json` (observed 2026-10-01 19:52:07–19:53:13 UTC). Cash is 92,751.88 of the starting 100,000.00; aggregate position cost is 7,248.12. No new target exposure or risk budget is set by this document.

Rhode Island and Massachusetts cost 3,998.65 for a maximum combined payout of 4,133. Florida YES, North Carolina NO and Alaska NO contribute most of the five-position upside if their outcomes win, but probabilities and common political drivers require research. Alaska's 5,000 NO shares cost 1,649.50 at a derived average of 0.3299; the latest audit records the verified fill and unknown order ID. A high win probability alone does not imply a favorable price.

## Open questions

- What is the documented tournament valuation formula? Why do portfolio and detail marks diverge?
- What exact simulated fee rules apply, if any? Filled costs reconcile, but that is not a complete fee schedule.
- What independently supported election probability ranges justify the existing entry prices?
- Which outcomes create correlated drawdowns across the five positions?
- Can official v1 data and UI holdings/cash be reconciled consistently using the correct tournament context?
- Which quotes have enough size to matter after conservative slippage assumptions?

## Latest research, not standing trading instructions

The [Florida/NC conservative scenarios](../research/fl_nc_conservative_bands_2026-10-01.md) widen the Florida evidence range after conflicting primary polls. Their 8–30% / 5–20% subjective bands and 0.060 / 0.035 illustrative all-in buy hurdles are neither calibrated probabilities nor standing executable limits. No additions at the historical 0.120 / 0.065 entry quotes are supported by that particular lower-scenario rule; it is not a liquidation instruction.

The [new-race shortlist](../research/new_edges_2026-10-01.md) preserves timestamped public-model evidence, polling limitations and Alaska settlement mapping. Alaska's 39% / 42.6% published non-D complements and 30–50% subjective stress band are forecast-dependent research, not a validated structural edge. At 30% the filled Alaska position has scenario expected loss 149.50; at a losing NO settlement its maximum entry loss is 1,649.50. No first-place probability or guaranteed edge is established.

## Working rules

Record why a forecast changes. Prefer a short source-linked report over unsupported certainty. Preserve historical snapshots and label stale quotes. Do not chase the provisional leaderboard or interpret unexplained marks as executable profit. Research findings go to [issue #2](https://github.com/GodlyDonuts/predictions-cup-2026/issues/2); implementation belongs in [issue #3](https://github.com/GodlyDonuts/predictions-cup-2026/issues/3).
