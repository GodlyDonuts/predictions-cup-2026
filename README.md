# Moloch / Tartarus â€” SIG Predictions Cup 2026

Moloch is the sole official-account execution worker. Tartarus is the offline population-learning system under development. The contest uses nonredeemable SUSQie points. Source is published directly to `main`; no PR workflow is used.

## Current state â€” October 22, 04:40 UTC

R83 remains the sole running worker **1063142**, binary `45e9522b528c`, execution source `427d200c2cf99f21fee06d9a346efd580cd40dc8`. **Delaware1075NO was activated at04:34:55 UTC without restarting trading**, increasing continuous reference coverage from12 to13 markets. Updater1066414 has subsequently committed new forecasts. Original financial controls, limiter and FIFO are unchanged. [Original worker proof](research/tartarus-learning-v2/r83-fresh-live-20261022.json), [same-worker Delaware commit](research/tartarus-learning-v2/delaware-turnover-activated-20261022.json).

**One distinct filled order/minute remains unmet.** The completed04:18:30–04:33:33 window had **two first-filled orders in15minutes (0.1333/minute)** and13empty UTC minutes. BUY1955YES865 and SELL3250YES865 generated **50.115SUSQies new realized net profit**. Subsequent partial receipts do not create additional trades. Original receipts and remaining FIFO reconcile. [Completed measurement](research/tartarus-learning-v2/r83-cadence-window-20261022.json), [financial audit](research/tartarus-learning-v2/r83-turnover-window-20261022.json).

At that audit cash was **13,036.295**, remaining original cost **83,081.465**, total attributed realized **537.845**, including unchanged inherited76.52. The latest published separate official account marks remain the04:19 receipts observed04:21:02: **94,266.395SUSQies**, down5.733605% from100,000. Realized gains do not establish whole-account profit or executable liquidation. [Dated official aggregates](research/tartarus-learning-v2/r83-latest-account-20261022.json).

R83 examined multiple candidates in a sweep, but median completed-scope spacing was125.53seconds. Four live stack samples identified full historical report construction and disposal among remaining costs; the SDK read ceiling was not saturated in the checked04:30–31 window. A source/preflight report-projection optimization is being checked offline and is **not deployed**. No new end-to-end speedup is claimed. [Scope measurements](research/tartarus-learning-v2/r83-scope-progress-20261022.json), [CPU evidence and next experiment](research/tartarus-learning-v2/r83-processing-bottleneck-20261022.md).

The human targets at least **one distinct filled order per minute**, measured by first execution rather than repeated partial receipts. The latest retained official marked-account observation is linked above; earlier observations below are historical. [Execution objective and economics](research/tartarus-learning-v2/filled-minute-20261021.md).

Repeat acquisition is activated for all12 reviewed markets, confirmed21:42:35UTC. ChatGPT Deep Research is complete. Its file-only opportunity census uncovered expired forecast reviews caused by reload requests timing out in the busy worker queue. The corrected boundary-aligned updater is installed without restarting trading; fresh forecasts for all12 active targets committed at21:35:16UTC. R73â€™s indexed cache is included in R74. [Research findings and applied corrections](research/tartarus-learning-v2/turnaround-research-results-20261021.md).

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
