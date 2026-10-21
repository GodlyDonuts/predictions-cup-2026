# Moloch / Tartarus — SIG Predictions Cup 2026

Moloch is the sole official-account execution worker. Tartarus is the offline population-learning system under development. The contest uses nonredeemable SUSQie points. Source is published directly to `main`; no PR workflow is used.

## Current state — October 21, 19:49 UTC

R71 is installed as sole worker1028075 since19:47:55 and is restoring the original ledger. The new repeat-acquisition code and report-cache improvement passed290checks and the optimized build. All452fills and the complete2.564GB financial prefix are preserved. Fresh R71 trading and explicit version3 review opt-in are still pending. [Installed release](research/tartarus-learning-v2/r71-recurring-installed-20261021.json), [change and limitations](research/tartarus-learning-v2/recurring-cohort-20261021.md).

R70 resumed actual execution before replacement:19new official fill receipts across three sell orders moved14,762shares and realized-105.925SUSQies. Today UTC has11distinct filled orders. The latest19:48:09official snapshot is94,678.645SUSQies (-5.32% from100,000): cash24,455.475 and marked positions70,223.17. Increased activity is established; account growth is not. Marks are not guaranteed liquidation proceeds. [Receipt/FIFO audit](research/tartarus-learning-v2/r70-recurring-sales-20261021.json), [latest account](research/tartarus-learning-v2/r71-latest-account-20261021.json).

GPU population037 is still running. The latest fully evaluated checkpoint is unprofitable. Continuation040 is queued with founder retention, family-specific market selection,64-transition PPO batches and an equivalent replay-copy optimization. No learned model is deployed; no independent positive ROI is established.

Both external feeds are active:237 Kalshi tickers and378 Polymarket outcome books. A separate process-based evaluation experiment preserved exact behavior but took32.6% longer in its measured CPU window; it stays disabled. [Feed evidence](research/tartarus-learning-v2/current-external-feeds-20261021.json), [benchmark](research/tartarus-learning-v2/process-evaluation-benchmark-20261021.json).

- [Current production and research state](docs/CURRENT_STATE.md)
- [ML implementation and remaining requirements](research/tartarus-learning-v2/IMPLEMENTATION_STATUS.md)
- [Current execution ownership: issue120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120)
- [ML execution and evidence: issue178](https://github.com/GodlyDonuts/predictions-cup-2026/issues/178)
- [Agent workflow](docs/AGENT_WORKFLOW.md) and [coordination](docs/COORDINATION.md)
- [Previous README, preserved in full](README_HISTORY_20261021.md)

The historical README and dated research reports are evidence of earlier states, not current runtime instructions. Credentials, raw authentication material and private runtime/checkpoint directories must never be committed.
