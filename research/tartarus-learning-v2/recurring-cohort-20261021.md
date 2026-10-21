# Repeated acquisition after a sale

Observed October 21, 19:35 UTC. Production remains R70; the change below is qualified in isolation and is building for release. It is not deployed yet.

## The restriction

All 12 currently refreshed, non-inherited Moloch cohorts have recorded exit bindings. Version 1 same-side top-ups stop admitting purchases as soon as an exit is recorded, even if that exit was subsequently retired without being sent. They also require positive remaining inventory. Consequently a completed sell can permanently remove that cohort from entry nomination. This prevents the buy/sell/rebuy cycle the human requested.

## The change

Explicit version 3 same-side top-ups permit purchases after a partial sale or a full close. The original cohort remains intact. The proposal carries all prior original entry and exit closures, actual order identities, remaining FIFO lots and the current position hash. The native engine reconstructs those fills before reserving a new purchase. The next exit includes both original and subsequent acquisitions.

An unresolved prior submission or an open physical order still needs reconciliation. Cash is credited only by actual fills. Original costs, realized results, source clocks, shared account pacing and current monetary limits remain in force. Legacy version 1 and inherited version 2 behavior is unchanged. Existing maximum-entry counts also remain; this change does not provide unlimited lifetime turnover.

The release also includes the checked immutable report-cache change from `cbd84fcc`. This removes repeated full-report rendering at an unchanged native head. Full-worker latency improvement has not yet been measured.

## Evidence

The exact isolated R70 source plus these patches passed 159 checks: 137 fundamental-policy checks, three ledger-snapshot checks and 19 alpha-proposal checks. New cases exercise a partial sale and a flat round trip through genuine synthetic API receipts, native reservation, physical dispatch authorization and restart. They retain original tags, fills, cash and FIFO costs, and prepare a subsequent sale. Open exits, changed acknowledgments, changed positions, omitted history and mismatched quantities fail without financial mutation. [Qualification](recurring-cohort-qualified-20261021.json).

R70 itself has resumed actual execution. At 19:30:20 the ledger contains 452 fills, up from 433 at startup. Nineteen new receipts span three filled sell orders and 14,762 shares. Receipt-by-receipt verification and original FIFO reconstruction give:

| Exchange | Shares sold | Realized gross SUSQies |
| --- | ---: | ---: |
| 1044 YES | 3,896 | +38.960 |
| 1051 NO | 866 | +4.330 |
| 1067 YES | 10,000 | -149.215 |
| Total | 14,762 | -105.925 |

Cash is 24,455.475 and remaining original FIFO cost is 71,193.080. These are ledger amounts, not a fresh whole-account market valuation. Today UTC contains 11 distinct filled orders, with partial receipts counted separately. More execution is established; positive account growth is not. [Original-receipt audit](r70-recurring-sales-20261021.json), [file-only auditor](tools/audit-r70-sales-20261021.py).

Next: complete the optimized release, preserve the exact active control and financial journal during one planned worker replacement, opt the reviewed cohorts into version 3 through the existing local reload channel, then verify current-process entries, fills and realized results. Keep the current sole worker trading until that replacement is ready.
