# Moloch / Tartarus — SIG Predictions Cup 2026

Moloch is the sole official-account execution worker. Tartarus is the offline population-learning system under development. The contest uses nonredeemable SUSQie points. Source is published directly to `main`; no PR workflow is used.

## Current state — October 21, 19:35 UTC

R70 is live as sole worker1022216. By19:30 its ledger has452fills, including19 new official fill receipts on three sell orders since startup. Those sales moved14,762shares and realized-105.925SUSQies in total. Cash is24,455.475; remaining FIFO cost71,193.080. Original433fills and all remaining lots match exactly. Today UTC has11distinct filled orders. Execution has resumed; account growth is not established. [Receipt/FIFO audit](research/tartarus-learning-v2/r70-recurring-sales-20261021.json).

The repeat-acquisition fix passes159 focused checks and is building for release; it is not deployed yet. Version1 top-ups prevented buying after any recorded exit in all12currently refreshed Moloch cohorts. Explicit version3 admits partial-sale and flat re-entry while retaining original fills, costs and physical-order reconciliation. [Change and limitations](research/tartarus-learning-v2/recurring-cohort-20261021.md).

The latest whole-account mark remains the18:56:58 official snapshot:94,708.835SUSQies, down5.29% from100,000. It predates the new sales and is not executable liquidation value. [Dated account snapshot](research/tartarus-learning-v2/r70-latest-account-20261021.json).

GPU population037 is still running. The latest fully evaluated checkpoint is unprofitable. Continuation040 is queued with founder retention, family-specific market selection,64-transition PPO batches and an equivalent replay-copy optimization. No learned model is deployed; no independent positive ROI is established.

Both external feeds are active:237 Kalshi tickers and378 Polymarket outcome books. A separate process-based evaluation experiment preserved exact behavior but took32.6% longer in its measured CPU window; it stays disabled. [Feed evidence](research/tartarus-learning-v2/current-external-feeds-20261021.json), [benchmark](research/tartarus-learning-v2/process-evaluation-benchmark-20261021.json).

- [Current production and research state](docs/CURRENT_STATE.md)
- [ML implementation and remaining requirements](research/tartarus-learning-v2/IMPLEMENTATION_STATUS.md)
- [Current execution ownership: issue120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120)
- [ML execution and evidence: issue178](https://github.com/GodlyDonuts/predictions-cup-2026/issues/178)
- [Agent workflow](docs/AGENT_WORKFLOW.md) and [coordination](docs/COORDINATION.md)
- [Previous README, preserved in full](README_HISTORY_20261021.md)

The historical README and dated research reports are evidence of earlier states, not current runtime instructions. Credentials, raw authentication material and private runtime/checkpoint directories must never be committed.
