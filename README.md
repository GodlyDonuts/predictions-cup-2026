# Moloch / Tartarus — SIG Predictions Cup 2026

Moloch is the sole official-account execution worker. Tartarus is the offline population-learning system under development. The contest uses nonredeemable SUSQie points. Source is published directly to `main`; no PR workflow is used.

## Current state — October 21, 18:43 UTC

Production R69 is installed as sole worker1021520 and is restoring the original ledger. Fresh current-process trading is not yet verified. It fixes a proved pre-submission expiry shutdown, removes the twelve-fresh-market ceiling, shares immutable review history and retrieves individual orders without rendering the entire history. All433 original fills and original holdings/FIFO are preserved. [Installation](research/tartarus-learning-v2/r69-throughput-installed-20261021.json), [checked source](research/tartarus-learning-v2/r69-source-qualified-20261021.json).

At18:37 official portfolio marks plus cash total94,909.215 SUSQies, down5.09% from100,000. Cash17,283.635; marked positions77,625.58. Today UTC has eight filled orders/37 partial fills. The latest R68 sale of433 NO shares realized+8.66; the preceding two audited R67 sales netted−164.63. Marks are not guaranteed liquidation proceeds. [Current account and exact sale audit](research/tartarus-learning-v2/r68-last-sale-20261021.json).

GPU population037 is still running. The latest fully evaluated checkpoint is unprofitable. Continuation040 is queued with founder retention, family-specific market selection,64-transition PPO batches and an equivalent replay-copy optimization. No learned model is deployed; no independent positive ROI is established.

Both external feeds are active:237 Kalshi tickers and378 Polymarket outcome books. A separate process-based evaluation experiment preserved exact behavior but took32.6% longer in its measured CPU window; it stays disabled. [Feed evidence](research/tartarus-learning-v2/current-external-feeds-20261021.json), [benchmark](research/tartarus-learning-v2/process-evaluation-benchmark-20261021.json).

- [Current production and research state](docs/CURRENT_STATE.md)
- [ML implementation and remaining requirements](research/tartarus-learning-v2/IMPLEMENTATION_STATUS.md)
- [Current execution ownership: issue120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120)
- [ML execution and evidence: issue178](https://github.com/GodlyDonuts/predictions-cup-2026/issues/178)
- [Agent workflow](docs/AGENT_WORKFLOW.md) and [coordination](docs/COORDINATION.md)
- [Previous README, preserved in full](README_HISTORY_20261021.md)

The historical README and dated research reports are evidence of earlier states, not current runtime instructions. Credentials, raw authentication material and private runtime/checkpoint directories must never be committed.
