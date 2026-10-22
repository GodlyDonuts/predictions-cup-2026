# Moloch / Tartarus — SIG Predictions Cup 2026

Moloch is the sole official-account execution worker. Tartarus is the offline population-learning system under development. The contest uses nonredeemable SUSQie points. Source is published directly to `main`; no PR workflow is used.

## Current state — October 22, 04:03 UTC

R83 was installed at **03:55:45 UTC** as sole worker **1063142**, updater1063145. It is restoring the original ledger; fresh trading is not yet verified. Execution source `427d200c2cf99f21fee06d9a346efd580cd40dc8`, binary `45e9522b528c`. All520 original fill records, the2,678,758,545-byte financial prefix and both known open orders were preserved. [Installation](research/tartarus-learning-v2/r83-local-retirement-sweep-installed-20261022.json), [file-only observers](research/tartarus-learning-v2/r83-observers-20261022.json).

**The one-distinct-filled-order-per-minute target remains unmet.** R82’s first15-minute window had one filled order. By its final stopped audit, two SELL orders had filled:2,000YES865 at0.875 for **+10.00SUSQies**, and2,235YES993 at0.76 for **+111.75SUSQies**; total **+121.75 realized net**, against original FIFO. Sixteen partial receipts count as two physical orders. [Cadence window](research/tartarus-learning-v2/r82-cadence-window-20261022.json), [final original-receipt audit](research/tartarus-learning-v2/r82-final-turnover-audit-20261022.json).

All12 measured R82 candidate sweeps stopped after one pass; median scope interval was58.485seconds. R83 continues after exact never-attempted local draft retirements so it can examine the next candidate without another full scope cycle. All244 worker checks pass; runtime/native libraries are unchanged. Its live throughput is still being measured. [Measured bottleneck and change](research/tartarus-learning-v2/local-retirement-sweep-r83-20261022.md).

At02:05:32 UTC, bounded online archival retained16.196GB of cold SDK history losslessly in287.5MB of verified archives, leaving17.018GB available. Each original was decoded and rehashed before its raw duplicate was removed. The active worker was **not paused or restarted**; current/open generations and financial history were untouched. Seven real archival checks passed. [Archive evidence](research/tartarus-learning-v2/r80-cold-archive-20261022.json). Earlier cleanup removed only unused reproducible test binaries. [Cleanup inventory](research/tartarus-learning-v2/r80-disposable-tests-cleanup-20261022.json).

At the final R82 report03:52:47, reconciled cash is **11,793.595**, remaining original cost **84,267.05**, and total attributed realized **480.73**, including unchanged inherited76.52. These are not equity. Earlier separate official receipts mark **94,219.545 SUSQies**, down **5.780455%** from100,000; they predate these sales and are not executable liquidation. [Original accounting](research/tartarus-learning-v2/r82-final-turnover-audit-20261022.json), [earlier official marks](research/tartarus-learning-v2/r82-latest-account-20261022.json).

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
