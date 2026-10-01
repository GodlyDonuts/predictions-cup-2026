# Current state

Last portfolio review: **2026-10-01 22:21:59–22:22:36 UTC**. The terminal portfolio showed only MA 2,061 YES at **22:21:59**. The completed Florida sale and **zero open orders** were reverified at **22:22:30**; cash displayed **98,157.74** to two decimal places at **22:22:36**. These are historical observations and need fresh reconciliation before any action.

## Account reconciliation

- Starting simulated cash: **100,000.00 SUSQies**
- Observed cash display: **98,157.74 SUSQies**, two decimal places; exact hidden account precision was not exposed
- Separate quantity × price ledger cash: **98,157.745 SUSQies**
- Remaining FIFO position cost: **1,999.170 SUSQies**
- Arithmetic cash plus remaining FIFO cost: **100,156.915 SUSQies**, an accounting identity rather than current liquidation value
- Fourth-session realized P&L: **−112.500 SUSQies**; cumulative realized P&L: **156.915 SUSQies**, under the recorded analytical FIFO aggregate-lot convention
- Open orders: **none observed at 22:22:30 UTC**
- Arithmetic reconciliation: 97,370.245 + (7,500 × 0.105) = **98,157.745**

The display is 0.005 below ledger arithmetic. Both observations are retained separately. The UI's hidden balance precision and rounding method are unknown; no fee, rounding adjustment, or hidden balance is invented. In schema 2, `cash_verified` and `cash_exact_verified_at_utc` remain null; explicit `cash_displayed` and `cash_arithmetic_from_verified_qty_price` fields carry the distinct values.

| Remaining market | Side | Shares | FIFO cost | Acquisition price |
|---|---|---:|---:|---:|
| Democratic Massachusetts Senate (379) | YES | 2,061 | 1,999.170 | 0.970 |

FL, RI, AK and NC holdings are zero. The remaining MA holding retains unrealized/liquidation risk; the 156.915 realized gain is not whole-account liquidation profit.

Machine-readable historical view: [`data/portfolio/latest.json`](../data/portfolio/latest.json). Immutable [22:22 snapshot](../data/snapshots/portfolio_2026-10-01_2222.json). Append-only [trade ledger](../data/trade_ledger.csv). Prior [22:01 snapshot](../data/snapshots/portfolio_2026-10-01_2201.json) and all earlier snapshots are unchanged.

## Completed 22:19 Florida-exit session

The [immutable source audit](../data/orders/session_2026-10-01_2219.json) is preserved byte-for-byte. One limit sell was submitted at **22:21:19.621 UTC**: **7,500 Florida YES at 0.105**, immediately and completely filled. The transaction history confirmed **787.50** displayed proceeds; exact quantity × price proceeds are **787.500**. The completed order ID was not exposed and remains **null** in the audit, with the ledger order-ID cell blank. Exact execution timestamps and unexposed fill/exchange IDs remain unknown; the submission timestamp is not substituted for execution time.

Preflight found the full 7,500 shares owned and zero open orders. At **22:21:02.949**, the bid was **0.105 for 17,392 shares**, sufficient for this sale. The audit separately records the settled ticket, estimated proceeds and absence of an oversell warning before the one submission. Terminal holdings and transaction history confirmed FL quantity zero, MA 2,061 unchanged and zero open orders. No cancellation was required.

This disposal consumes the final 7,500-share Florida lot acquired at 0.120, for **900.000** FIFO cost. Session realized P&L is **787.500 − 900.000 = −112.500**; cumulative realized P&L is **269.415 − 112.500 = 156.915**. The ledger appends exactly **one executed aggregate** and preserves all 15 prior rows byte-for-byte. Its `cost_susqies` value is **−787.500**, the existing sign convention for sale proceeds. No canceled or unsubmitted intent creates a trade row.

The decision was risk reduction after a materially lower public Florida reference price. The [public-reference research bundle](../research/public_reference_2026-10-01/public_reference_report_2026-10-01.md) retains observation times, depth and settlement differences; these external prices are not guaranteed fair probabilities or arbitrage. The earlier 21:32 Florida order was canceled unfilled; this later executed sale is a distinct order and session.

## Historical sessions

The following descriptions apply only to their original observation windows, not current holdings.

## Completed 21:32 inventory-sale session

The [immutable source audit](../data/orders/session_2026-10-01_2132.json) is preserved byte-for-byte. All five submitted orders are terminal:

| Order | Outcome | Exact verified quantity × price proceeds | FIFO cost disposed | Session realized P&L |
|---|---|---:|---:|---:|
| RI YES sell 1080906 | 6,187 filled at 0.935 | 5,784.845 | 5,684.845 | 100.000 |
| AK NO sell 1082747 | 5,000 filled at 0.330 | 1,650.000 | 1,575.000 | 75.000 |
| NC NO sell 1088818 | 1,538 filled at 0.075 | 115.350 | 99.970 | 15.380 |
| FL YES sell 1084839 | 7,500 at 0.125 canceled unfilled | 0.000 | 0.000 | 0.000 |
| MA YES sell 1086473 | 2,061 at 0.975 canceled unfilled | 0.000 | 0.000 | 0.000 |
| **Session totals** | **12,725 shares sold** | **7,550.195** | **7,359.815** | **190.380** |

The 22:00:00 cancellation target is separate from actual submissions: FL at **22:00:12**, MA at **22:00:36**, once each. Fresh reload at **22:01:18** found no open orders and unchanged MA 2,061 / FL 7,500 holdings. No additional FL/MA fill was observed. The immutable audit retains the actual cancellation times even where its brief notes describe the target hour.

All RI fill-row quantities sum to 6,187, including one combined 469-share row. AK fills are 648 + 20 + 4,332; NC is 1,538. Their exact displayed tick prices and quantities determine proceeds without rounding each fill. The ledger appends exactly three executed order aggregates and excludes the two canceled unfilled orders. It preserves all old bytes, uses negative `cost_susqies` for proceeds, and retains milli-SUSQ precision. No precise execution timestamp or unexposed fill/exchange ID is fabricated.

RI disposal consumes the remaining 1,187-share 0.935 lot (1,109.845 cost) plus the 5,000-share 0.915 lot (4,575.000 cost). AK consumes its remaining 5,000-share 0.315 lot. NC consumes its 1,538-share 0.065 lot. Session FIFO gain is **100.000 + 75.000 + 15.380 = 190.380**; prior cumulative 79.035 + 190.380 = **269.415**. Cumulative per-market totals in the snapshot are replayed from all ledger sales, including the earlier RI at-cost sale. Platform weighted-average basis, tournament marks, and executable liquidation value are separate concepts.

## Earlier historical sessions

The following descriptions apply only to their original observation windows, not current holdings.

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

The portfolio page uses tournament valuation prices whose formula remains unverified. Its marks differed sharply from detail-page prices. Neither is a guaranteed full-position exit price. Current displayed gains/losses are not realized P&L. No fee or hidden-precision rule is inferred. Realized gain excludes all remaining position risk.

See the [execution and valuation review](../research/execution_valuation_review_2026-10-01.md) for historical depth and observation windows. Quotes were not captured simultaneously and are stale for execution.

Under standard 0-or-1 settlement and no further trades, fees, refunds or sponsor interventions, ledger-arithmetic endpoints for the one remaining MA position are **98,157.745** if it loses and **100,218.745 SUSQies** if it wins. These use the separate arithmetic balance, not an asserted hidden UI balance; they are scenarios, not forecasts or executable valuations.

## Current operating mode

- dot remains the sole execution coordinator, under separate user authorization
- Codex/Drogon remains research, quantitative strategy and development only; experimental bot remains paper-only and bot API writes disabled
- This reconciliation created no order, API key, runner, schedule, webhook or ownership handoff
- The ownership record and unrelated repository blobs remain unchanged; no independent code branch is merged or force-updated

## Next useful work

1. Refresh cash, holdings, orders and fills before any new trade proposal or execution
2. Keep paper/replay PR #5, read-only feed PR #6 and offline ML PR #7 separate from execution; synthetic tests are not evidence of profitable strategy or a live-trading gate pass
3. Investigate the valuation and cash-display precision questions without assuming a bug, fee, or arbitrage opportunity
4. Review [Florida/NC conservative scenarios](../research/fl_nc_conservative_bands_2026-10-01.md) and the [new-race shortlist](../research/new_edges_2026-10-01.md). Subjective bands and illustrative price hurdles are not calibrated probabilities or standing executable limits
