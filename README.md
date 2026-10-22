# Moloch / Tartarus — SIG Predictions Cup 2026

Moloch is the sole official-account execution worker. Tartarus is the offline population-learning system under development. The contest uses nonredeemable SUSQie points. Source is published directly to `main`; no PR workflow is used.

## Current state — October 22, 01:30 UTC

R79 is verified live as sole worker **1056637**, with its real-time feed connected. The 01:18:53 UTC verification checked the running binary, complete original 2,628,631,476-byte financial prefix, all 493 earlier fill receipts and unchanged native engine. The reference updater resumed and committed generation 75. [Fresh production proof](research/tartarus-learning-v2/r79-fresh-live-20261022.json).

**The one-distinct-filled-order-per-minute objective remains unmet.** The 01:26:22 original-receipt audit confirms one new filled order since R79 startup: 558 NO1077 shares at 0.855, first executed at 01:19:59.288. Two partial receipts count as one order. This spent 477.09 SUSQies and produced no realized gain. The audit has 495 receipts, original FIFO preserved, and rolling first-fill counts 0/1 minute, 0/5, 1/15 and 2/60. [Receipt and cadence audit](research/tartarus-learning-v2/r79-turnover-20261022.json).

The candidate sweep has examined up to five candidates within one cycle. A separate blocker is now verified: retired October 17 maximum-age quotes on managed markets 865 and 1051 repeatedly preempt current reviewed exits, then fail to produce an eligible candidate. R80 removes that duplicate historical nomination while preserving current reviewed exits and unmanaged age handling. **R80 is qualified, not deployed**: all 239 worker checks pass, and the runtime/native libraries are unchanged. [Diagnosis and correction](research/tartarus-learning-v2/managed-age-r80-20261022.md), [original live observations](research/tartarus-learning-v2/r79-managed-age-blocker-20261022.json).

Reconciled cash is **290.63**, remaining original position cost **95,579.28**, and attributed realized net **289.995**, including unchanged inherited 76.52. Those are not current equity. Separate official receipts at 01:25:28–30 give marked equity **94,232.46 SUSQies, -5.76754%** from 100,000. These marks are not executable liquidation or an atomic account snapshot. [Account evidence](research/tartarus-learning-v2/r79-latest-account-20261022.json).

The current 15-minute timing and 60-minute exit windows are still running. No further production replacement has occurred. File-only observers make zero account requests. More examined candidates, visible profitable bids and passing checks do not establish the required filled-order cadence or positive ROI.

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
