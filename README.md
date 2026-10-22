# Moloch / Tartarus — SIG Predictions Cup 2026

Moloch is the sole official-account execution worker. Tartarus is the offline population-learning system under development. The contest uses nonredeemable SUSQie points. Source is published directly to `main`; no PR workflow is used.

## Current state — October 22, 02:31 UTC

R81 was installed at **02:31:01 UTC** as sole worker **1061072**, updater1061075, and is **restoring the original ledger; fresh trading is not yet verified**. Checked source `efb48634eff9bdd3bcd73630376d822cb418f263`, binary `fe8280811298`. All500 receipts, the full2,654,477,535-byte financial prefix and native engine were preserved. Known outstanding order20696 remains attributed and must reconcile. [Installation](research/tartarus-learning-v2/r81-deployment-20261022.json).

**The one-distinct-filled-order-per-minute target remains unmet.** R80 has three original-receipt-confirmed sells:470YES1044 at0.85 (+7.05),10,000NO1051 at0.87 (+86.275), and1,217YES1067 at0.305 (−24.34). Combined realized change **+68.985SUSQies**. The last sell first executed02:16:25.832; its three partial receipts count as one order. Total500 receipts. The02:24 audit has two new first-filled orders in15minutes. [Receipt audit](research/tartarus-learning-v2/r80-third-sell-20261022.json).

R80 excludes obsolete managed maximum-age nominations. Fresh sell reservations now reach later preparation but expire before physical dispatch. R81 eliminates repeated full-history audits of the same immutable SELL state, while keeping current book, original FIFO, request, review and unknown-acceptance checks. All396 Rust and8 updater checks pass. R81 is installed and restoring; execution-speed gains remain unmeasured. [Current expiration evidence](research/tartarus-learning-v2/r80-preintent-expiry-20261022.json), [R80 change](research/tartarus-learning-v2/managed-age-r80-20261022.md).

At02:05:32 UTC, bounded online archival retained16.196GB of cold SDK history losslessly in287.5MB of verified archives, leaving17.018GB available. Each original was decoded and rehashed before its raw duplicate was removed. The active worker was **not paused or restarted**; current/open generations and financial history were untouched. Seven real archival checks passed. [Archive evidence](research/tartarus-learning-v2/r80-cold-archive-20261022.json). Earlier cleanup removed only unused reproducible test binaries. [Cleanup inventory](research/tartarus-learning-v2/r80-disposable-tests-cleanup-20261022.json).

Reconciled cash is **9,761.315**, original remaining cost **86,177.58**, attributed realized **358.98** including inherited76.52. These are not equity. The last retained separate official account receipts at01:25:28–30 mark **94,232.46 SUSQies, −5.76754%** from100,000; they are not an atomic or executable liquidation snapshot. [Official marks](research/tartarus-learning-v2/r79-latest-account-20261022.json).

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
