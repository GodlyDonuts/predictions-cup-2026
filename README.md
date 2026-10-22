# Moloch / Tartarus — SIG Predictions Cup 2026

Moloch is the sole official-account execution worker. Tartarus is the offline population-learning system under development. The contest uses nonredeemable SUSQie points. Source is published directly to `main`; no PR workflow is used.

## Current state — October 22, 00:41 UTC

R78 resumed verified trading on October 22 at **00:26:03 UTC** as sole worker **1055406**, with reference updater 1055409. Its executable matches checked source `06a4b3d447657d87b87aa6384d508f05597d65fc`. Verification preserved all 492 earlier fill receipts and the complete 2,612,655,396-byte original financial prefix. [Fresh live verification](research/tartarus-learning-v2/r78-fresh-live-20261022.json), [installation](research/tartarus-learning-v2/r78-deployment-20261022.json).

**The one-distinct-filled-order-per-minute objective remains unmet.** The completed 15-minute window ended at 00:41:06 UTC with one new filled order: 150 NO1077 shares at 0.850, first executed at 00:27:25.022. The 00:41:37 original-receipt/FIFO audit still has 493 receipts, no new realized gain, and rolling first-fill counts 0/1 minute, 0/5, 1/15 and 1/60. The daily bucket now follows the UTC observation date. [Receipt audit](research/tartarus-learning-v2/r78-turnover-window-20261022.json).

Fourteen completed scope observations produced 13 gaps with median **55.926 seconds** and maximum **224.096 seconds**, including stream recovery and reload work. These are decision-cycle measurements, not filled-order rates. All12 original-source reviews were current in the 00:40:43 diagnostic. A retained native1044 book was31seconds old and993 was absent from that particular native view; read-only census nominations do not themselves authorize or prove a current executable exit. [Completed timing window](research/tartarus-learning-v2/r78-scope-progress-20261022.json), [source-age and feed diagnostic](research/tartarus-learning-v2/r78-scheduling-diagnostic-20261022.json).

Reconciled attributed cash is **767.72**, remaining original position cost **95,102.19**, and attributed realized net **289.995**, including unchanged inherited 76.52. These are not current equity. The latest separate official account receipts, October 22 at 00:36:47–49, give marked equity **94,286.02 SUSQies, -5.71398%** from 100,000. Those marks are not executable liquidation or an atomic account snapshot. [Account evidence](research/tartarus-learning-v2/r78-latest-account-20261022.json).

R78 prioritizes current executable FIFO-profit exits and brings non-crossing passive hints into the shared queue with ready orders and book refreshes. It retains original prices, quantities and accounting. Its 10 FIFO, 146 integration and 235 worker checks passed; R77's preceding history-reader benchmark improved by 93.11%. These component results do not establish the required fill cadence or ROI. [Scheduling correction](research/tartarus-learning-v2/exit-priority-r78-20261021.md), [reader measurement](research/tartarus-learning-v2/closed-index-r77-20261021.md).

The separate reference relay remains active. R78 committed generation `dbdda35c3141` with all 12 reviewed strategies; transient capture failures recovered on retry. The SIG stream subsequently required automatic read-failure reconnections. The 15-minute timing window is complete; the separate exit observation continues on the same worker. Profitable turnover is not established. The dated R77 interruptions remain documented [here](research/tartarus-learning-v2/r77-relay-recovery-20261022.json).





The human targets at least **one distinct filled order per minute**, measured by first execution rather than repeated partial receipts. The latest retained official marked-account observation is linked above; earlier observations below are historical. [Execution objective and economics](research/tartarus-learning-v2/filled-minute-20261021.md).

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
