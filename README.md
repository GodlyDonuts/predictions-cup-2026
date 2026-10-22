# Moloch / Tartarus â€” SIG Predictions Cup 2026

Moloch is the sole official-account execution worker. Tartarus is the offline population-learning system under development. The contest uses nonredeemable SUSQie points. Source is published directly to `main`; no PR workflow is used.

## Current state â€” October 22, 05:21 UTC

**R85 installed at 05:19:45 UTC; original-ledger restoration is in progress.** Sole worker1069937, binary `5222f0f999fa`, checked execution source `7fb6fee064521c8e2f9f1bb2419bb7e6050b3efa`, updater1069940. The replacement fixes the proved-no-HTTP cohort failure that stopped R83 and includes R84 source/preflight report deferral. It preserves all535 fills, the full2,691,159,025-byte original financial prefix, all prior engine approvals and known order20904. **Fresh trading is not yet verified.** [Installation evidence](research/tartarus-learning-v2/r85-local-guard-cohort-installed-20261022.json), [repair and validation](research/tartarus-learning-v2/local-guard-cohort-r85-20261022.md).

R86 is a checked **offline candidate**, excluded from the current production binary. Its final-entry projection scope passed247 worker checks and preserves original receipts, durable head and error behavior. The retained20937 timing trace shows a5.532-second post-book gap before preparation; that gap is not attributed wholly to report work. R85 remains in place for fresh live measurements before another replacement. [Candidate and evidence](research/tartarus-learning-v2/final-packet-r86-20261022.md).

**One distinct filled order/minute remains unmet.** The completed 04:35–04:50 UTC window had **one first-filled order in 15 minutes (0.0667/minute)** and 14 empty UTC minutes. SELL 1,472 YES993 shares realized **66.24 SUSQies net** against original FIFO. The preceding window had two first-filled orders and +50.115 realized net. Partial receipts do not create additional trades. [Latest completed measurement](research/tartarus-learning-v2/r83-post-delaware-cadence-20261022.json), [official-receipt/FIFO audit](research/tartarus-learning-v2/r83-post-delaware-turnover-20261022.json).

At the 04:50:25 audit, cash was **14,147.655**, remaining original cost **82,036.345**, total attributed realized **604.085**, including unchanged inherited 76.52. The separate official 04:39 receipts, observed 04:41:49, mark the account at **94,293.565 SUSQies**, down 5.706435% from 100,000. These are dated, non-atomic marks; realized gains do not establish whole-account profit or executable liquidation. [Official aggregates](research/tartarus-learning-v2/r83-account-after-delaware-20261022.json).

R83 examined multiple candidates in a sweep, but median completed-scope spacing was 125.53 seconds. Fresh incoming books were absent or stale in the native view at two measured timestamps. Qualified R84 reduces repeated full-report construction during source/preflight reads. It was never deployed and is included in R85; the latter reproduces and repairs the newer no-HTTP cohort failure. No restored-trading or end-to-end speedup claim is made. [Source optimization](research/tartarus-learning-v2/source-projection-r84-20261022.md), [cohort repair](research/tartarus-learning-v2/local-guard-cohort-r85-20261022.md).

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
