# Capital turnover evidence — October 14

Eleven completed, contiguous SIG capture episodes (18–28) were replayed with verified snapshot hashes, source journal chains and exact original baseline accounting. Six independent 100,000-SUSQie banks compare 1%, 10% and 100% assumed participation with join-price or one-tick-inside quoting. Orders are funded, capped by the smaller observed paired queue, limited to 10,000 per market and 40,000 total commitment. Historical entry scopes, receipt/processing clocks, gaps, fees, modeled write pacing and inventory exits are retained. Original wallet history is untouched.

| Assumed participation | Quote improvement | Exact buy notional | Realized P&L | Remaining owned cost |
| --- | --- | --- | --- | --- |
| 1% | Join | 4.930000 | 0.000000 | 4.320000 |
| 1% | One tick | 7.630000 | 0.000000 | 6.270000 |
| 10% | Join | 45.250000 | 0.000000 | 44.640000 |
| 10% | One tick | 73.450000 | 0.000000 | 64.440000 |
| 100% | Join | 447.810000 | −9.585000 | 140.480000 |
| 100% | One tick | 532.860000 | −9.585000 | 140.630000 |

No variant establishes profitable turnover. Zero realized P&L does not mean break-even: these banks still hold inventory. Missing fresh final depth prevents a terminal liquidation mark. Full participation is a sensitivity assumption, not verified FIFO priority or market impact. The periods were previously examined and are not untouched holdouts. Separate banks are never summed.

An optional `repriceProfitableHedge` experiment creates a fresh opposite-side resting BUY, capped by owned unpledged shares, only when worst remaining acquisition basis plus both modeled fees leaves the required paired surplus. Old orders must reconcile before their funds/shares can be reused; late cancellation fills cannot create oversized replacements. Maximum-age or profitable-age all-or-none exits take precedence. The option requires `retainProfitableHedge`, respects entry scopes and is disabled in live policies. Three dedicated tests cover fresh replacement/netting, old fills during cancellation and maximum-age precedence. All six hedge sensitivity variants produced the same measured result as their controls; this is not promoted as an alpha improvement.

Reproduce with `node moloch/research/capital-turnover-20261014.mjs` and the same command with `--reprice-hedge`. Checked outputs: `data/moloch-capital-turnover-20261014.json` and `data/moloch-capital-turnover-hedge-20261014.json`.

The prospective scale benchmark is separate from these replays. At 09:41:13Z it had six simulated quotes, zero fills, unchanged 100,000 cash; the last pair was bounded to 1,963 shares by observed depth. Displayed/reserved capital is not deployed capital. It consumes the existing collector journal without network requests or actual orders.

Next constraint: four REST scopes require at least eight read completions per cycle under the existing 12-second pacing, whereas the quote source-silence timer is 60 seconds. This creates cancellation pressure and delays reconciliation. Once the current capture is terminal and exact liabilities are checked, retire only empty old scopes and preserve the inventory scope plus the new-entry scope, bringing a two-scope cycle under the silence interval. Do not change shared account limits, discard unknown orders, or replace a process merely because an observation times out.
