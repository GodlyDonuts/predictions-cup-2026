# Trade-cadence research and applied corrections — October 21

The current human target is at least one exchange-confirmed distinct filled order per minute, together with account growth. It is **not achieved**. One physical order counts once at its first actual fill; partial receipts, acknowledgments and cancellations are not additional filled orders.

## Research provenance and conclusions

The human explicitly requested ChatGPT Deep Research in the open browser. The completed [research conversation](https://chatgpt.com/c/6ad928b5-1bb8-83e9-9aa3-b7e4aa97c4de) inspected published commit `f5f5b397c90056ddb05d795d2cc7dc8e6f7b7991`, official documentation and supplied operational observations. [Retained report text](turnaround-deep-research-20261021.txt) and [subsequent pricing analysis](turnaround-research-followup-20261021.txt) are external model output, not independently verified findings or execution authority. Text extraction loses some equation formatting. Its inability to fully inspect the current interactive API schema is explicitly preserved.

The useful recommendations are to retain the Rust executor, finish recurring entry, remove measured execution delays and evaluate simple executable external-reference opportunities before another broad strategy rewrite. Python is suitable for this research. A rewrite is not presently justified by evidence that Python would improve execution or economics.

Historical passive fill incidence was only41/891 ordinary BUY orders. That dated, selection-dependent fraction is not a future probability; it nevertheless contradicts the assumption that placing more passive quotes guarantees60 worthwhile fills/hour. Marketable limits can improve execution probability only when actual depth and a justified value estimate support the price.

The follow-up identifies an important distinction: reducing the allowance to zero still leaves the dated993YES and872YES asks above their respective entry ceilings.1043NO instead had numerical headroom already. Blanket removal of allowances would not diagnose these different cases. None of these forecast values is independently calibrated or a guaranteed settlement mapping.

## Concrete blocker found and repaired

The new file-only census observed all12 refreshed production reviews expired at21:28:39UTC, despite arriving external feeds. The continuous reference updater still used a CLI whose response timeout could run out while the sole worker was busy. Its60-second request lifetime started before that queue delay. Original request `9088d6eb...` and the subsequent `d762880e...` were independently resolved as expired and uncommitted; neither was blindly repeated.

The recurring-policy coordinator had already solved this by connecting to the exact existing worker, verifying its Unix peer UID/PID and process start ticks, waiting for the worker's actual poll boundary, and only then minting the unchanged60-second request. That correction is now also implemented in [the continuous updater](../../moloch/external-data/run-live-reference-reload.py). Replies have a75-second bounded wait; uncertain outcomes retain durable pending metadata for exact reconciliation.

At21:32:41UTC, updater1036249 and recurring watcher1036250 replaced the old helper processes. **Trading worker1031505 was not stopped or restarted.** The immutable updater source hash is `85e17cf68c22664e55d14b0cb8a4f4961b533d33dc800045a4631594769d93fb`. Only the helper path/hash were added to configuration. Existing pending requests, source clocks, original holdings/FIFO, request lifetime and account limiter remain intact. At21:35:16.787UTC the updater committed generation25fa83ed… with fresh forecasts for all12 active targets. The original cohort journal independently contains that generation, and the21:36:06check finds all12 sources/reviews fresh, the same worker alive and no STOP. [Installation and fresh commit proof](reference-boundary-live-20261021.json).

Three real Linux local-socket checks verify no bytes before the owner boundary, rejection of the wrong peer, and STOP before issuance. Seven existing reference-selection/reconciliation tests pass, including late commits and ambiguous evidence. [Reproducible installer](tools/install-reference-boundary-20261021.py), [boundary tests](tools/test-reference-boundary-20261021.py).

## Read-only opportunity census

[The census](tools/turnover_opportunity_census.py) reads the existing recorder tails, all237 observed SIG books, current reviewed forecast files and the worker's derived inventory view. It creates no SIG client and makes no API requests. Only the14 installed reviews are assessed; two are historically expired, leaving12 actively refreshed strategy targets. It does not claim all237 markets have qualified external models.

Each observation records original source age, conditional payout, entry ceiling, zero-allowance counterfactual, whether the allowance is decisive, visible depth, proposed size/cost and explicit rejection reasons. Size uses a50% displayed-depth haircut and existing principal/quantity constraints. Those are conditional research estimates, not orders, verified account eligibility, calibrated expected profit or evidence of future counterparties. The account view can lag; actual production preflight is still required.

Fresh quote versions do not create additional signal episodes while the same condition persists. Missing/stale books cannot prove an old economic episode ended. Four focused arithmetic/freshness tests pass. A positive episode is never reported as a fill or a completed BUY→SELL→REBUY cycle.

## Remaining work

All12 recurring targets are confirmed at21:42:35:851,872,1051,1043,1044,1067,885,966,1077,865,977,993. [Original activation completion](recurring-complete-20261021.json). No new rebuy fill is established in the21:45:49 audit.

The R73 two-line order-history cache correction remains built and qualified, not deployed. The production worker still takes roughly60–120seconds per scope. A restart costs approximately20minutes of original-ledger replay; no claim of a whole-worker speedup is made from a component test.

Next evidence must show fresh reference commits, recurring entry actually reaching the physical path, exchange-confirmed fills, and net economics. The last independently reconciled sale is still+221.355SUSQies on one993YES order; the last official account mark remains94,780.155 at20:16:40UTC. Increasing volume is not itself evidence of recovering the account loss.

## Dispatch latency now being corrected

Entry20449 (1077NO,10,000shares) expired before HTTP at21:37:44. A repeated whole-cohort post-intent validation consumed3.756seconds after3.789seconds of policy preparation. The existing SDK recorded positive no-admission proof and preserved the original intent; it was not retried. A subsequent order20468 did reach SIG, but its book was already9.919seconds old at acknowledgment. Its post-intent validation took1.765seconds. [Original later trace](dispatch-ack-q20468-20261021.json). This is an acknowledgment, not a fill.

R74 replaces the repeated cohort rescan with verification tied to the same opaque decision: unchanged native head and review files, exactly the original two chained dispatch rows, exact quote/spec/permit/ownership tag, and no competing unknown attempt. The SDK still checks the original book deadline and actual account/limiter state. It also includes the previously qualified R73 order-index cache. ARM64 compilation and focused failure cases are in progress; R74 is not yet deployed. No extension of price freshness or resetting of financial history is involved.
