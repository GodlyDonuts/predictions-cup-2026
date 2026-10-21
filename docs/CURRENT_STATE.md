# Current Moloch and Tartarus state

Observed October 21, 21:46 UTC. This page replaces repeated historical headlines; [the previous document is preserved in full](CURRENT_STATE_HISTORY_20261021.md).

The target is at least one distinct first-filled order/minute and remains unmet. The latest audited993YES sale realized+221.355SUSQies across six partial receipts of one physical order; twelve distinct orders filled todayUTC. All12 recurring policies are activated, confirmed21:42:35UTC. Deep Research is complete and the read-only census has identified stale forecast reviews as an additional blocker. A boundary-aligned forecast updater is installed; fresh forecasts for all12 active targets committed at21:35:16UTC. The same R72 worker1031505 remains running. R73’s cache patch is qualified, not deployed. [Current research, applied changes and limitations](../research/tartarus-learning-v2/turnaround-research-results-20261021.md).

## Production

R72 is verified live at 20:36:52 UTC as sole worker 1031505, with real-time subscriptions connected and no STOP. Its original 452 fills and complete 2,564,024,854-byte financial prefix were independently verified. At 20:38:29 the exchange acknowledged a new 10,000-share sell on exchange 865 (actual order 54443777); that acknowledgment is not a fill. Version 3 repeat-purchase review activation is in progress through the existing worker. Binary 1cf47b2ae833, control a7a6dcacf0d8, native/runtime libraries and all 24 engine approvals are unchanged. [Exact installation](../research/tartarus-learning-v2/r72-stream-join-installed-20261021.json).

R71 stopped at 20:06:41 UTC after original-ledger restoration with `LIVE_STREAM_JOIN_FAILED`; it did not reach fresh trading. The old worker discarded the specific startup cause. Source inspection found that public market broadcasts may legally arrive between market and private-account JOIN acknowledgments, while the worker required its first message to be the combined JOIN. R72 buffers those messages until both channels join, records typed failure reasons and retries transient subscription failures through the existing persisted connection budget. Five focused checks and the optimized replacement build pass. Native engine, original452fills and financial state are unchanged. Repeat-entry opt-in was pending at that historical startup boundary; the current count is above. [Repair and evidence](../research/tartarus-learning-v2/stream-join-r72-20261021.md).

The deployed source adds explicit version3 repeat acquisition after partial/full exits and the checked immutable report cache.159runtime and131native checks pass; only8of289release source files differ from R70. At initial startup, all12 refreshed non-inherited cohorts required version3 opt-in; all12 have since committed as recorded above. Legacy behavior, original cash/FIFO and financial controls remain unchanged. [Implementation and limits](../research/tartarus-learning-v2/recurring-cohort-20261021.md), [release composition](../research/tartarus-learning-v2/r71-recurring-release-20261021.json).

R70 recorded19additional official receipts across three filled sell orders, selling14,762shares and realizing-105.925SUSQies. Today UTC has11distinct filled orders. Full original433-fill preservation, remaining FIFO and every new original receipt match. More execution is established; account growth is not. [Receipt/FIFO evidence](../research/tartarus-learning-v2/r70-recurring-sales-20261021.json). Latest20:16:40official cash24,455.475 plus marked positions70,324.68 equals94,780.155(-5.219845% from100,000), not executable liquidation. [Latest account snapshot](../research/tartarus-learning-v2/r72-latest-account-20261021.json).

R69 had stopped18:43:28 before native replay/orders because its152,404,846-byte upgrade chain exceeded a128MiB parser bound; deployment omitted that size check. R70 raises the engineering bound to256MiB, moves parsed entries and verifies seals by reference.131 native checks pass. The actual22-entry152MB chain passed in9.39s/1.63GiB peak; the final23-entry202MB chain passed in12.03s. All original approvals, including failed R69's, are preserved. Only Session parsing differs from R69's289-source build; all financial engine rules remain unchanged. [Exact qualification](../research/tartarus-learning-v2/r70-history-bound-qualified-20261021.json).

R68 stopped18:22:52 when fundamental attempt20264 expired before SDK admission. Original SDK archives prove no HTTP was sent. The checked repair retires such attempts and continues only for genuine expiry; clock regression and actual uncertain acceptance retain their handling. File-only recovery appended the exact no-send proof, preserved STOP until installation and did not retry the order or alter finances. Historical reconciled exposure18533 still requires its original native startup verification. [Exact recovery](../research/tartarus-learning-v2/r68-expiry-recovery-20261021.json).

R68 additionally sold433 exchange851 NO shares at0.895 on order54228551, executed18:17:32 and reconciled18:21. Original receipts and exact FIFO establish387.535 proceeds,378.875 released cost and8.66 realized gross. All430 preceding fills and remaining FIFO lots match. Earlier R67 sales realized+113.21 and−277.84, net−164.63. [Latest sale/account audit](../research/tartarus-learning-v2/r68-last-sale-20261021.json), [earlier loss](../research/tartarus-learning-v2/r67-second-sale-reconciled-20261021.json).

R69 keeps the R68 stream repairs, adds shared immutable review maps and direct single-order lookup, and expands the native market capacity to the complete237-exchange scope. It also removes a four-lifetime research-observation quota; that quota did not limit trades.130 native,5 lookup-unit,6 Session-lookup,9 capacity,17 SDK proof and228 worker checks pass. A copied773617-event prefix exactly reproduces the original sealed financial report,430 fills and20263 orders; startup additionally validates the final433-fill boundary. Ten explicit source paths differ from deployed R68 among289 source files. [Exact source composition](../research/tartarus-learning-v2/r69-source-qualified-20261021.json), [original-prefix result](../research/tartarus-learning-v2/r69-original-prefix-parity-20261021.json).

The reference updater's expired request228844cf was bound to the confirmed-dead R68 process and absent from the original committed cohort journal. Its metadata was resolved under exclusive owner/updater locks, preserving the request and active generation38; no account request, forecast adoption or financial mutation occurred. Updater1031508 now follows R72. [Resolution evidence](../research/tartarus-learning-v2/r68-reference-resolution-20261021.json).

Last reconciled native financial state,19:30:20UTC:

| Measure | SUSQies |
|---|---:|
| Cash | 24,455.475 |
| Remaining attributed position cost | 71,193.080 |
| Total attributed realized gross | 68.640 |
| New-trading portion | -7.880 |
| Unchanged inherited realized portion | 76.52 |

The earlier18:56:58 official observation reports77,425.20 marked position value plus17,283.635 cash =94,708.835, down5,291.165 or5.291165% from the100,000 start. These marks are not executable liquidation proceeds. At that earlier boundary, today UTC had37 fill receipts across8 distinct filled orders; the later R70 audit above supersedes this count. The97,000 deadline was missed; no whole-account profit is established. [Latest official aggregates](../research/tartarus-learning-v2/r70-latest-account-20261021.json).

There remains one official worker and shared limiter. Daily windows and original monetary caps are unchanged. Laptop training creates no account client.

The individual-order lookup avoids rendering the entire20,000-order history on cache misses. A synthetic component benchmark drops the second full-render sample's144.47ms/lookup to roughly0.068ms; complete worker speed and increased fill throughput have not yet been measured. [Scope and exact limits](../research/tartarus-learning-v2/single-order-lookup-20261021.md).

The actual laptop external feeds are healthy:Kalshi237 tickers at1,193 REST admissions/minute, oldest receipt11.9s, no throttles; Polymarket378 outcome books/189 markets, configured30 REST admissions/s, receipt ages3.19s median/6.37s maximum. Both websocket feeds are receiving. A retired VPS Polymarket status file is not the live feed. These are observation freshness figures, not contract-equivalence or profit evidence. [Measured feed state](../research/tartarus-learning-v2/current-external-feeds-20261021.json).

## Learning

Active037 remains on its original frozen source (process18116 verified20:20UTC). The latest completed island001 member scored-10.56343% nominal/+0.0030525% stressed marked ROI on the two-day development window; its minimum daily-growth reward is-0.13955066. This is worse than the stronger retained founder and is not deployed. The15/2fills were BUY_NO with no realized FIFO gain. No independently profitable checkpoint has emerged. [Latest complete result](../research/tartarus-learning-v2/population-037-second-member-result-20261021.json).

Queued040 preserves the completed island populations before source migration. It includes founder retention, independent learned BUY/SELL target selection,64-transition PPO batches, faster equivalent history validation and owned-response delivery. The last optimization matched every observation, action, paid request/receipt and financial hash in four interleaved fitting-hour passes; measured collection speedup was1.040x alongside existing GPU training. That is not end-to-end training speed or ROI.

A separate optional process-evaluation implementation passed22 unique focused checks and exact parity. Four interleaved CPU passes over the same five-minute development slice took116.24s median locally versus154.12s with child worlds:32.6% slower including startup/IPC. It remains disabled and is not added to037/040. No optimizer step or protected-test access occurred. [Negative benchmark](../research/tartarus-learning-v2/process-evaluation-benchmark-20261021.json).

The full requested competition-history training plus4-day evolution/4-day independent test remains incomplete. Current detailed development uses2 fitting days and2 development days. ProtectedOctober21–25 is unconsumed; no learned model is deployed. [Implementation and evidence](../research/tartarus-learning-v2/IMPLEMENTATION_STATUS.md).

The additional unchanged-head financial-report cache is checked and published, with156 focused report/alpha/fundamental checks. It remains undeployed while fresh R70 execution timing is measured. [Candidate and exact limits](../research/tartarus-learning-v2/entry-report-cache-20261021.md).
