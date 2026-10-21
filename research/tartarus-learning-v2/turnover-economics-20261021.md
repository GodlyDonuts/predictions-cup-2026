# Minute-fill economics and exit-depth evidence

Observed October21,22:59UTC. The one-distinct-first-filled-order/minute objective remains unmet. R76 is installed and restoring the original ledger. This research and the diagnostic experiment add no SIG account client or production order.

## Completed external research

The authorized ChatGPT conversation completed a focused follow-up against public source20132b490fbeb3cd66d79b4ba159fd1bd7f9a407. [Captured report](turnover-economics-research-20261021.txt), [browser conversation](https://chatgpt.com/c/6ad928b5-1bb8-83e9-9aa3-b7e4aa97c4de). The captured equations may lose layout in DOM text. The report is external analysis, not execution authority. Its operational snapshot predates R76 installation.

The two R74 buys averaged9,996shares and8,410.985SUSQies per order. If equally sized buys and sells were balanced at60filled orders/hour,30purchases would require252,329.55SUSQies/hour and sales would need to replenish that principal. With10,838.09cash, that new cash pool would need to turn over in approximately2.58minutes. These are conditional capacity calculations, not forecasts of profit, future fills or venue liquidity. First fills alone do not establish substantial cash recycling.

Source inspection confirms that `alpha_proposal::compare` uses observed liquidation depth quantity without requiring profitable liquidation proceeds. Terminal reference value can justify an eventual payout hypothesis while saying little about the price of an executable exit a few minutes later. A persistent discount therefore cannot by itself establish repeatable profitable turnover.

The current `fifoProfitTurnover` rule uses the maximum original fee-inclusive unit cost in the selected FIFO prefix plus0.005, then anchors its limit at least one tick below the ask. Both `fundamental_policy::passive_terms` and `reviewed_inventory_exit::reserve_reviewed_inventory_exit` enforce that selection. The research proposes comparing a covered profitable bid-depth exit. It cannot be enabled by changing only a runtime price; native verification explicitly enforces the old formula. No such execution change is deployed.

## What the actual retained books show

The first [file-only census](exit-census-pre-r76-20261021.json) was observed22:52:37UTC using the last R74 inventory view22:42:41UTC and its retained books. **Those books/reviews are stale: zero current nominations.** Remaining zero-fee FIFO lots match original BUY-fill suffixes, including their original execution clocks; inherited/mismatched basis cannot become new profit authority.

At those dated prices,993YES had a9911-share profitable bid prefix with hypothetical marginal-limit gain416.94SUSQies;1044YES had a470-share prefix with hypothetical gain4.70. The current ask-relative rule already reached those bids. No held target in this sample demonstrated an ask-relative pricing miss. These are unsent counterfactual quotes, not sales or realized gains, and the same depth must not be counted again as new turnover. This sample does not justify weakening the floor or making another production replacement for the proposed exit variant.

A finite60-minute observation is queued to begin only when R76's own current-process progress is trading with a healthy feed. It samples existing files every15seconds at reduced scheduling priority. Repeated depth is not summed, and stale/inconsistent snapshots are gaps, not opportunities. The observer makes zero account requests and writes no production control. [Census](tools/turnover_exit_census.py), [six focused checks](tools/test_turnover_exit_census.py), [finite observer](tools/observe-turnover-exits-20261021.py). Fresh data and actual execution timing must decide whether a policy experiment has useful candidates.

## Prepared diagnostic-only performance change

The synchronous breadth recorder serialized each captured frame, parsed its own bytes back into a JSON tree, then cloned/serialized again to advance its capture chain. The prepared change carries the already computed sequence/hash alongside those exact bytes and advances the held chain only after successful durable append. Replay of external captures retains ordinary full chain verification. Capture size limits, rotation, sync behavior and failure state are unchanged.

Twelve recorder tests pass, including original-format byte parity and no chain advancement after a failed append. An isolated benchmark used the same341,781-byte retained public frame16times, alternating old/new order over three pairs. Every run produced the same5,472,935bytes and SHA91f8953db57bb834b5b6715efbda6cf370493e8e5832f9d20450cee09da95e62. Median old0.496269seconds, candidate0.181383seconds:63.45%less recorder time. This is not63.45%faster trading. The benchmark ran at low priority during original-ledger restoration; whole-worker timing is unmeasured. [Exact results](breadth-serialization-benchmark-20261021.json), [reproduction](tools/benchmark-breadth-serialization-20261021.py).

The first standalone harness omitted the linked release artifacts' required ThinLTO flags; its initial link failed. A test-only Unicode escape was also corrected before the successful run. Both failed build logs remain on the build host. Neither attempt touched production.

The recorder candidate is **not deployed**. R76's native engine, binary and source pin remain unchanged. Preserve its uninterrupted restoration and measure its live results before scheduling another replacement.
