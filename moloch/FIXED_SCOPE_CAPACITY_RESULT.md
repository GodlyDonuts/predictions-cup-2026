# Fixed-scope capacity and exit sensitivity

The complete phase-42 source is now replayed with `scoped-policy.mjs --capacity` and `--fast-exits`. Snapshot/journal verification and exact recorded-baseline parity precede every comparison. These are retrospective alternatives; original live banks remain unchanged.

The original 1% strict-through case acquires 25 YES at 0.910, cost 22.750, and ends with a fresh +0.125 liquidation mark. At 10%/100% participation and under the existing through-capacity model, each alternative acquires 149 shares, cost 135.590, with a +0.745 mark. Displayed initial packet depth caps quantity; these are not guarantees that an actual added order would receive those fills. None has closed profit or qualifies for ranking while inventory and its covered quote remain.

Immediate break-even, one-minute and three-minute exit alternatives close the purchase for exactly zero realized gain, then reserve funds for further quotes. They release inventory sooner but do not establish profitable turnover. Fees, later post-activation executable depth, original clocks and reconciliation remain enforced. Data: `data/moloch-capacity-fixed-scope-20261014-42.json` and `data/moloch-fast-exit-fixed-scope-20261014-42.json`.

The first drain capture (43) completed with 22 receipts and exit zero. Realtime continuation 44 failed before any market receipt with CHANNEL_ERROR; its actual process exited one. Both original paper ledgers were stopped, hash-verified by continuation, and preserved with 25 shares still owned each. Explicit REST-only source 45 restored same-scope collection under the existing mutex, five-read budget, persistent backoff and trade-identity reconciliation, without realtime authentication. Entries remain disabled. This transport change is explicit recovery and does not submit, retry or activate contest orders.

Fresh depth marks have varied during recovery; a favorable mark is not a realized exit. All-or-none exit intents can expire without filling if the later executable depth no longer supports their limit. The full fixed-scope test remains unfinished until the original inventory and all remaining quote liabilities are closed and reconciled.
