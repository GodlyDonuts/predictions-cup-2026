The new Rust replay tests the original dip hypothesis directly, using the median of strictly earlier bid receipts over one hour. A market needs20 earlier receipts spanning30minutes; entry asks must be below that median by1/2/4ticks. All three fixed alternatives use independently funded100cash, maximum3markets/3committed cost,25cumulative purchases, zero fees, one share/one-percent observed depth, a fixed protective limit, and independently later requests after at least3seconds. Profit exits target one tick; four-tick loss and two-hour risk exits remain, so losing positions are not held forever merely to inflate win rate. Publication gaps clear features/intents but preserve held cost and losses.

The original sealed public source contains31,447book receipts over86.277hours. Original source and sanitized account prefixes are hash verified; each account snapshot must have been known before the original book request and remain fresh/nonexcluded. No current account information is backfilled. Full native publication validation and all three funded replays took3.982seconds.

| Dip below earlier median | Closed trades | Realized counterfactual P&L | Final inventory | Fees |
| --- | ---: | ---: | ---: | ---: |
|1tick|3|−0.005|0|0|
|2ticks|3|−0.005|0|0|
|4ticks|2|+0.010|0|0|

Both positive four-tick roundtrips were in Tennessee Governor311/ex1000, buying0.820 and selling0.825. Their original UTC entries wereOctober4 19:10 and19:27; exits19:11 and20:37. This is a tiny, concentrated development result, not repeatable profitability, a source of scaled returns or permission for competition orders. The other alternatives lost, and all results remain retained. Thresholds were declared before this replay, but the capture has been examined in other research. Choosing four ticks after this score would require separately frozen future evidence.

Execution remains counterfactual: actual order acceptance/impact and fresh contract-rule review are absent. The3second setting is a minimum independently later request, not a claim of execution exactly3seconds later; sparse books can delay fills substantially. Depth-qualified quotes are not actual accepted fills. Same-market repeated roundtrips are correlated. Estimated final liquidation uses fresh depth only; missing prices remain unknown. Counterfactual expired/gap-cleared intents are unsubmitted simulation proposals, not claims that remote orders were cancelled/unfilled.

Four native checks verify exact funded FIFO/covered exits, delayed/fixed-limit/depth behavior, account/gap exclusions, preserved open costs and realized timeout losses. Synthetic gains in those checks establish accounting only. Code is backend-rust/median-dip-replay; every original fill/cash/count and source anchor is in data/median-dip-replay-results-20261006-v1.json. The next economic task is prospective validation and broader causal candidate collection, rather than scaling two same-market wins.
