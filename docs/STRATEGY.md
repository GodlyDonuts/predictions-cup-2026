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

The four positions and cost basis are recorded in `data/portfolio/latest.json`. Cash is 94,401.38 of the starting 100,000.00. No new target exposure or risk budget is set by this document.

Rhode Island and Massachusetts cost 3,998.65 for a maximum combined payout of 4,133. Florida YES and North Carolina NO contribute most of the four-position upside if their outcomes win, but probabilities and common political drivers require research. A high win probability alone does not imply a favorable price.

## Open questions

- What is the documented tournament valuation formula? Why do portfolio and detail marks diverge?
- What exact simulated fee rules apply, if any? Filled costs reconcile, but that is not a complete fee schedule.
- What independently supported election probability ranges justify the existing entry prices?
- Which outcomes create correlated drawdowns across the four positions?
- Can official v1 data and UI holdings/cash be reconciled consistently using the correct tournament context?
- Which quotes have enough size to matter after conservative slippage assumptions?

## Working rules

Record why a forecast changes. Prefer a short source-linked report over unsupported certainty. Preserve historical snapshots and label stale quotes. Do not chase the provisional leaderboard or interpret unexplained marks as executable profit. Research findings go to [issue #2](https://github.com/GodlyDonuts/predictions-cup-2026/issues/2); implementation belongs in [issue #3](https://github.com/GodlyDonuts/predictions-cup-2026/issues/3).
