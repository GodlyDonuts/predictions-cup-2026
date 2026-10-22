# Moloch / Tartarus — SIG Predictions Cup 2026

Moloch is the sole official-account execution worker. Tartarus is the offline population-learning system under development. The contest uses nonredeemable SUSQie points. Source is published directly to `main`; no PR workflow is used.

## Current state — October 22, 03:01 UTC

R81 is verified live at **02:51:09 UTC**, sole worker **1061072**, updater1061075. Source `efb48634eff9bdd3bcd73630376d822cb418f263`, binary `fe8280811298`. The full original 2,654,477,535-byte financial prefix and all 500 prior receipts match. Previously outstanding 20696 closed without a new fill. All 12 active strategy reviews refreshed through generation84. [Startup proof](research/tartarus-learning-v2/r81-fresh-live-20261022.json), [closure](research/tartarus-learning-v2/r81-prior-order-closed-20261022.json), [fresh reviews](research/tartarus-learning-v2/r81-reference-refresh-20261022.json).

**The one-distinct-filled-order-per-minute target remains unmet.** R81 bought 1,032 NO1077 at0.85 on physical55062903, first executed02:53:16.880. Its three partial receipts count as one order, with877.20 principal spent and no realized gain. The03:01:44 audit counts one new first-filled order over15minutes; original receipt and remaining FIFO identities pass. R80's preceding three sells realized a combined+68.985. [Current receipt audit](research/tartarus-learning-v2/r81-first-buy-20261022.json).

R81 removes repeated SELL history audits. Its next SELL20707 expired before SDK admission at02:54:32; the original no-HTTP proof retires that attempt. A separate R82 candidate moves the full sell execution packet after reservation/activation, following the working entry path. It is **building, not deployed**. R81 continues operating; no new account client, financial limit or source-clock change is introduced. [Original failure evidence](research/tartarus-learning-v2/r81-sell-expiry-20261022.json).

At02:05:32 UTC, bounded online archival retained16.196GB of cold SDK history losslessly in287.5MB of verified archives, leaving17.018GB available. Each original was decoded and rehashed before its raw duplicate was removed. The active worker was **not paused or restarted**; current/open generations and financial history were untouched. Seven real archival checks passed. [Archive evidence](research/tartarus-learning-v2/r80-cold-archive-20261022.json). Earlier cleanup removed only unused reproducible test binaries. [Cleanup inventory](research/tartarus-learning-v2/r80-disposable-tests-cleanup-20261022.json).

Reconciled cash is **8,884.115**, remaining original cost **87,054.78**, total attributed realized **358.98** including inherited76.52. These are not equity. The separate official account/position receipts at02:51:57–02:52:00 mark **94,158.735 SUSQies**, down **5.841265%** from100,000. Those values are neither an atomic snapshot nor executable liquidation. [Official aggregates](research/tartarus-learning-v2/r81-latest-account-20261022.json).

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
