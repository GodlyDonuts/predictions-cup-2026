# Moloch / Tartarus — SIG Predictions Cup 2026

Moloch is the sole official-account execution worker. Tartarus is the offline population-learning system under development. The contest uses nonredeemable SUSQie points. Source is published directly to `main`; no PR workflow is used.

## Current state — October 21, 18:03 UTC

Production R68 is verified trading with healthy realtime data as sole worker1018590. At18:02:26 it had430 reconciled fills,1671 acknowledged orders and1668 terminal orders. Three new R68 acknowledgments have no confirmed fills yet. Its transport repair preserves the financial engine, original ledger, account limiter and daily windows. [Fresh runtime proof](research/tartarus-learning-v2/r68-fresh-live-20261021.json).

R67's two verified sales realized+113.21 and−277.84 SUSQies against original FIFO, a combined−164.63. R68 reconciled the second sale after recovery; it executed before the old shutdown. Cash is16,896.10 and remaining attributed position cost78,849.72; cost is not current equity. [First sale](research/tartarus-learning-v2/r67-first-sale-20261021.json), [second sale and loss audit](research/tartarus-learning-v2/r67-second-sale-reconciled-20261021.json).

GPU population037 is still running. The latest fully evaluated checkpoint is unprofitable. Continuation040 is queued with founder retention, family-specific market selection,64-transition PPO batches and an equivalent replay-copy optimization. No learned model is deployed; no independent positive ROI is established.

Both external feeds are active:237 Kalshi tickers and378 Polymarket outcome books. A separate process-based evaluation experiment preserved exact behavior but took32.6% longer in its measured CPU window; it stays disabled. [Feed evidence](research/tartarus-learning-v2/current-external-feeds-20261021.json), [benchmark](research/tartarus-learning-v2/process-evaluation-benchmark-20261021.json).

- [Current production and research state](docs/CURRENT_STATE.md)
- [ML implementation and remaining requirements](research/tartarus-learning-v2/IMPLEMENTATION_STATUS.md)
- [Current execution ownership: issue120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120)
- [ML execution and evidence: issue178](https://github.com/GodlyDonuts/predictions-cup-2026/issues/178)
- [Agent workflow](docs/AGENT_WORKFLOW.md) and [coordination](docs/COORDINATION.md)
- [Previous README, preserved in full](README_HISTORY_20261021.md)

The historical README and dated research reports are evidence of earlier states, not current runtime instructions. Credentials, raw authentication material and private runtime/checkpoint directories must never be committed.
