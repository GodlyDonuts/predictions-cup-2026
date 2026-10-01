# Current state

Last portfolio review: **2026-10-01 20:57:59–20:58:30 UTC**. Transaction history was reconciled from 20:57:59; a fresh reload verified **zero open orders at 20:58:06**, and exact cash was verified at **20:58:30**. Data is historical and needs fresh reconciliation before any action.

## Account reconciliation

- Starting simulated cash: **100,000.00 SUSQies**
- Current verified cash: **88,729.50 SUSQies**
- Remaining FIFO position cost: **11,270.50 SUSQies**
- Cash plus remaining cost: **100,000.00 SUSQies**
- Realized P&L: **0.00 SUSQies**; the only sale disposed of the original RI lot at cost
- Open orders: **none observed at 20:58:06 UTC**
- Cash reconciliation: 92,751.88 + 1,999.48 − 4,446.86 − 1,575.00 = **88,729.50**

| Market | Side | Shares | Remaining cost | Average entry |
|---|---|---:|---:|---:|
| Democratic Rhode Island Senate (387) | YES | 4,756 | 4,446.86 | 0.935 |
| Democratic Massachusetts Senate (379) | YES | 2,061 | 1,999.17 | 0.970 |
| Democratic Alaska Senate (377) | NO | 10,000 | 3,224.50 | 0.32245 (aggregate) |
| Democratic Florida Senate (354) | YES | 12,500 | 1,500.00 | 0.120 |
| Democratic North Carolina Senate (277) | NO | 1,538 | 99.97 | 0.065 |

Machine-readable current view: [`data/portfolio/latest.json`](../data/portfolio/latest.json). Immutable [20:58 snapshot](../data/snapshots/portfolio_2026-10-01_2058.json). Append-only [trade ledger](../data/trade_ledger.csv).

## Completed 20:36 discretionary session

[Session audit](../data/orders/session_2026-10-01_2036.json) contains exact observed order IDs, intent IDs, submission/cancellation/verification times and transaction-history fill quantities. All five order intents are terminal:

- RI YES sell **2,072 at 0.965**: fully filled, proceeds **1,999.48**, realized P&L **0.00** against the original lot. Completed sell order ID and exact execution timestamps were not exposed and remain unknown
- RI YES buy order **1023469**: **4,756 of 5,000** filled at **0.935**, cost **4,446.86**; **244** unfilled shares canceled at **20:57:06 UTC**
- AK NO buy order **1026814**: **5,000** filled at **0.315**, cost **1,575.00**. Combined with the original 5,000-share lot costing 1,649.50, remaining AK basis is **3,224.50**
- AK NO sell order **1028485**, 5,000 at 0.335: **unfilled**, canceled at **20:57:19 UTC**
- FL YES sell order **1030028**, 5,000 at 0.130: **unfilled**, canceled at **20:57:39 UTC**

The cancellation target was 20:57 UTC; actual cancellation submission times and the later zero-open-orders observation above are authoritative. No additional fill/cancel race was observed. Each cancellation was submitted once; a transient Florida UI wait did not trigger a duplicate submission.

The ledger adds only three actual trade aggregates, not canceled unfilled quotes. Its existing `cost_susqies` column is signed cash cost: positive for purchase debits, negative for sale proceeds. This permits initial cash minus cumulative signed cost to reconstruct cash. It is not a realized-P&L field. Exact fill timestamps remain unknown; submission and verification times are not substitutes. The source audit preserves the 100+1,972 RI sale fills and 1+3,907+2+1,090 AK purchase fills without inventing execution IDs. Rounded AK fill displays sum to 1,575.00, while exact per-fill products can carry half-cent precision; aggregate accounting uses the exact 5,000 × 0.315 cost.

## Important limitations

The portfolio page uses tournament valuation prices whose formula remains unverified. Its marks differed sharply from detail-page prices. Neither is a guaranteed full-position exit price. Current displayed gains/losses are not realized P&L. No fee line surfaced; exact cash reconciliation does not establish a general fee schedule.

See the [execution and valuation review](../research/execution_valuation_review_2026-10-01.md) for historical depth and observation windows. Quotes were not captured simultaneously and are stale for execution.

If no further trades, fees, refunds, or sponsor interventions occur, the final payoff endpoints for these five positions are **88,729.50** if all lose and **119,584.50 SUSQies** if all win. These are settlement scenarios, not forecasts or executable valuations. RI and MA together have 370.97 of possible upside over their remaining basis.

## Current operating mode

- dot remains the sole execution coordinator, under separate user authorization
- Codex/Drogon remains research, quantitative strategy and development only; experimental bot remains paper-only and bot API writes disabled
- This reconciliation created no order, API key, runner, schedule, webhook or ownership handoff
- The ownership record is unchanged; review current rules and exact market resolution conditions before relying on any forecast

## Next useful work

1. Refresh cash, holdings, orders and fills before any new trade proposal or execution
2. Review the isolated offline paper/replay draft PR #5; its synthetic tests are not evidence of profitable strategy or a live-trading gate pass
3. Investigate the valuation discrepancy without assuming it is a bug or an arbitrage opportunity
4. Review [Florida/NC conservative scenarios](../research/fl_nc_conservative_bands_2026-10-01.md) and the [new-race shortlist](../research/new_edges_2026-10-01.md). Subjective bands and illustrative price hurdles are not calibrated probabilities or standing executable limits
