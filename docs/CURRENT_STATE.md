# Current state

Last portfolio review: **2026-10-01 21:26:00–21:26:19 UTC**. A fresh reload verified **zero open orders at 21:26:00**, and exact cash and terminal holdings were verified at **21:26:19**. Data is historical and needs fresh reconciliation before any action.

## Account reconciliation

- Starting simulated cash: **100,000.00 SUSQies**
- Current verified cash: **89,820.05 SUSQies**
- Remaining FIFO position cost: **10,258.985 SUSQies**, retaining half-cent precision
- Cash plus remaining cost: **100,079.035 SUSQies**
- Realized P&L: **79.035 SUSQies** exact (**79.04** rounded for display), under the analytical FIFO aggregate-lot convention
- Open orders: **none observed at 21:26:00 UTC**
- Session cash reconciliation: 88,729.50 − 4,575.00 + 3,390.55 + 1,650.00 + 625.00 = **89,820.05**

| Market | Side | Shares | Remaining FIFO cost | Acquisition prices |
|---|---|---:|---:|---|
| Democratic Rhode Island Senate (387) | YES | 6,187 | 5,684.845 | 1,187 at 0.935 + 5,000 at 0.915 |
| Democratic Massachusetts Senate (379) | YES | 2,061 | 1,999.17 | 0.970 |
| Democratic Alaska Senate (377) | NO | 5,000 | 1,575.00 | 0.315 remaining lot |
| Democratic Florida Senate (354) | YES | 7,500 | 900.00 | 0.120 |
| Democratic North Carolina Senate (277) | NO | 1,538 | 99.97 | 0.065 |

Machine-readable current view: [`data/portfolio/latest.json`](../data/portfolio/latest.json). Immutable [21:26 snapshot](../data/snapshots/portfolio_2026-10-01_2126.json). Append-only [trade ledger](../data/trade_ledger.csv). Prior [20:58 snapshot](../data/snapshots/portfolio_2026-10-01_2058.json) is unchanged.

## Completed 21:02 discretionary session

The [immutable source audit](../data/orders/session_2026-10-01_2102.json) is preserved exactly as recorded. All four submitted orders are terminal:

- RI YES buy **1056531**: **5,000 at 0.915**, cost **4,575.00**; 3,104 + 1,896 shares filled
- RI YES sell **1050409**: **3,569 of 4,756 at 0.950**, proceeds **3,390.55**; **1,187** unfilled shares canceled. Fills were 1,826 + 282 + 1 + 903 + 1 + 1 + 555 shares
- AK NO sell **1052464**: **5,000 at 0.330**, proceeds **1,650.00**; 1,883 + 224 + 1 + 2,892 shares filled
- FL YES sell **1054809**: **5,000 at 0.125**, proceeds **625.00**

The cancellation target was **21:25:00 UTC**. The one actual RI cancellation was submitted at **21:25:12**. At **21:24:46**, 1,742 shares remained; **555 additional shares filled for 527.25 before cancellation finalized**, leaving 1,187 canceled. The 555 are included once in the 3,569 total. AK and FL were already filled and were not canceled. Zero open orders was verified at 21:26:00 and cash at 21:26:19. This delay and fill/cancel race are retained, not replaced with the target time.

The three superseded, unsubmitted intents (RI buys at 0.920 and 0.840; RI sell at 0.945) created no trades. The ledger appends only the four actual executed aggregates; its `cost_susqies` is positive for buy debits and negative for sale proceeds. Canceled quantities and unsubmitted intents are excluded. Exact execution timestamps, per-fill IDs and exchange IDs were not captured; they remain unknown. Submission and verification times are not execution times.

### Exact FIFO analysis

This is analytical FIFO using the previously documented acquisition lots, not the platform's displayed average basis or interim tournament marks:

| Disposal | Proceeds | FIFO cost disposed | Realized P&L |
|---|---:|---:|---:|
| RI 3,569 YES from 0.935 lot | 3,390.55 | 3,337.015 | 53.535 |
| AK 5,000 NO from original aggregate lot | 1,650.00 | 1,649.50 | 0.50 |
| FL 5,000 YES from 0.120 lot | 625.00 | 600.00 | 25.00 |
| **Total** | **5,665.55** | **5,586.515** | **79.035** |

RI remaining cost is 1,187 × 0.935 + 5,000 × 0.915 = **5,684.845**. Its exact aggregate average is **5,684.845 / 6,187**; the snapshot preserves the ratio rather than substituting a rounded average. AK retains the newer 5,000-share 0.315 lot. The disposed original AK order ID remains unknown. Prior realized P&L was 0.00, so session and cumulative realized P&L are both 79.035. The 79.04 display rounding is not used in reconciliation.

The source audit includes interim observations and the AK `proceeds_verified_by_cash_change` value 695.31. That interim value is not the terminal AK proceeds; the full 5,000-share fill and exact final cash reconcile to **1,650.00**. The original audit is not rewritten to erase intermediate evidence.

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

The portfolio page uses tournament valuation prices whose formula remains unverified. Its marks differed sharply from detail-page prices. Neither is a guaranteed full-position exit price. Current displayed gains/losses are not realized P&L. No fee line surfaced; exact cash reconciliation does not establish a general fee schedule. Realized gain excludes all remaining position risk.

See the [execution and valuation review](../research/execution_valuation_review_2026-10-01.md) for historical depth and observation windows. Quotes were not captured simultaneously and are stale for execution.

If no further trades, fees, refunds, sponsor interventions or nonstandard settlements occur, the final payoff endpoints for these five positions are **89,820.05** if all lose and **112,106.05 SUSQies** if all win. These are settlement scenarios, not forecasts or executable valuations.

## Current operating mode

- dot remains the sole execution coordinator, under separate user authorization
- Codex/Drogon remains research, quantitative strategy and development only; experimental bot remains paper-only and bot API writes disabled
- This reconciliation created no order, API key, runner, schedule, webhook or ownership handoff
- The ownership record and independent code branches are unchanged; review current rules and exact market resolution conditions before relying on any forecast

## Next useful work

1. Refresh cash, holdings, orders and fills before any new trade proposal or execution
2. Keep isolated paper/replay PR #5 and feed-collector PR #6 separate from execution; synthetic tests are not evidence of profitable strategy or a live-trading gate pass
3. Investigate the valuation discrepancy without assuming it is a bug or an arbitrage opportunity
4. Review [Florida/NC conservative scenarios](../research/fl_nc_conservative_bands_2026-10-01.md) and the [new-race shortlist](../research/new_edges_2026-10-01.md). Subjective bands and illustrative price hurdles are not calibrated probabilities or standing executable limits
