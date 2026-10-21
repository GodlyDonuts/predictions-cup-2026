# Moloch / Tartarus — SIG Predictions Cup 2026

Moloch is the sole official-account execution worker. Tartarus is the offline population-learning system under development. The contest uses nonredeemable SUSQie points. Source is published directly to `main`; no PR workflow is used.

## Current state — October 21, 23:12 UTC

R76 is verified live as sole worker 1041499. At 23:05:28 UTC its executable, original 481 fills and complete 2,601,300,909-byte financial prefix matched the installation evidence. Original no-network repeat entry 20449 was recovered without resubmission. [Live verification](research/tartarus-learning-v2/r76-fresh-live-20261021.json), [recovery](research/tartarus-learning-v2/r76-topup-recovery-20261021.json).

At 23:12:27 UTC, the original-receipt audit counted **one distinct first-filled order in 15 minutes**, four in 60 minutes and 16 today. **The one-trade-per-minute target remains unmet.** R76 order 20545/54709337 bought six 1043 NO shares for 4.86 SUSQies at 23:08:50.266. The preceding 385-share 966 buy was an R74 resting order filled during restoration, not a new R76 decision. Cash is 10,479.03 and remaining original position cost is 85,390.88; neither is current equity. Attributed realized net remains 289.995, including the unchanged 76.52 inherited portion. [Original receipts, FIFO and cadence](research/tartarus-learning-v2/r76-turnover-20261021.json).

R76's 14-market exit-hint pass measured 505 ms at 23:11:39; completed scope work still takes roughly 60–120 seconds. Another order acknowledgment is not counted as a fill. Focused ChatGPT research is complete, and the finite file-only exit census is running. An isolated recorder optimization remains undeployed. [Execution evidence and limits](research/tartarus-learning-v2/hint-batch-r76-20261021.md), [turnover economics](research/tartarus-learning-v2/turnover-economics-20261021.md).


The human targets at least **one distinct filled order per minute**, measured by first execution rather than repeated partial receipts. The latest retained official whole-account equity remains94,780.155 at20:16:40; it is historical, not a current liquidation value. [Execution objective and economics](research/tartarus-learning-v2/filled-minute-20261021.md).

Repeat acquisition is activated for all12 reviewed markets, confirmed21:42:35UTC. ChatGPT Deep Research is complete. Its file-only opportunity census uncovered expired forecast reviews caused by reload requests timing out in the busy worker queue. The corrected boundary-aligned updater is installed without restarting trading; fresh forecasts for all12 active targets committed at21:35:16UTC. R73’s indexed cache is included in R74. [Research findings and applied corrections](research/tartarus-learning-v2/turnaround-research-results-20261021.md).

Historical R72 was verified live at 20:36:52 UTC as sole worker 1031505 (replaced by R74 at22:02UTC), with real-time subscriptions connected and no STOP. Its original 452 fills and complete 2,564,024,854-byte financial prefix were independently verified. Binary 1cf47b2ae833, control a7a6dcacf0d8, native/runtime libraries and all 24 engine approvals are unchanged. [Exact installation](research/tartarus-learning-v2/r72-stream-join-installed-20261021.json). The subscription-startup fix is documented [here](research/tartarus-learning-v2/stream-join-r72-20261021.md).

R70 resumed actual execution before replacement:19new official fill receipts across three sell orders moved14,762shares and realized-105.925SUSQies. At the earlier R70 boundary, today UTC had11distinct filled orders. The20:16:40official snapshot is94,780.155SUSQies (-5.22% from100,000): cash24,455.475 and marked positions70,324.68. Increased activity is established; account growth is not. Marks are not guaranteed liquidation proceeds. [Receipt/FIFO audit](research/tartarus-learning-v2/r70-recurring-sales-20261021.json), [latest account](research/tartarus-learning-v2/r72-latest-account-20261021.json).

GPU population037 is still running. The latest fully evaluated checkpoint is unprofitable. Continuation040 is queued with founder retention, family-specific market selection,64-transition PPO batches and an equivalent replay-copy optimization. No learned model is deployed; no independent positive ROI is established.

Both external feeds are active:237 Kalshi tickers and378 Polymarket outcome books. A separate process-based evaluation experiment preserved exact behavior but took32.6% longer in its measured CPU window; it stays disabled. [Feed evidence](research/tartarus-learning-v2/current-external-feeds-20261021.json), [benchmark](research/tartarus-learning-v2/process-evaluation-benchmark-20261021.json).

- [Current production and research state](docs/CURRENT_STATE.md)
- [ML implementation and remaining requirements](research/tartarus-learning-v2/IMPLEMENTATION_STATUS.md)
- [Current execution ownership: issue120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120)
- [ML execution and evidence: issue178](https://github.com/GodlyDonuts/predictions-cup-2026/issues/178)
- [Agent workflow](docs/AGENT_WORKFLOW.md) and [coordination](docs/COORDINATION.md)
- [Previous README, preserved in full](README_HISTORY_20261021.md)

The historical README and dated research reports are evidence of earlier states, not current runtime instructions. Credentials, raw authentication material and private runtime/checkpoint directories must never be committed.
