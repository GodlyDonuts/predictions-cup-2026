# Moloch / Tartarus — SIG Predictions Cup 2026

Moloch is the sole official-account execution worker. Tartarus is the offline population-learning system under development. The contest uses nonredeemable SUSQie points. Source is published directly to `main`; no PR workflow is used.

## Current state — October 21, 17:37 UTC

Production R68 was installed at17:35:25 as sole worker1018590 and is restoring the original ledger. Its larger bounded ingress and timed reconnect admission repair the queue-overflow stop in R67. All425 existing fills, two known open orders, financial engine, accounting, account limiter and daily windows are preserved. Fresh R68 trading remains unverified. [Installation and50 focused checks](research/tartarus-learning-v2/r68-stream-throughput-installed-20261021.json).

Before the stop, five verified fills sold5,110 NO shares of exchange851 at0.895, realizing113.21 SUSQies against original FIFO. Cash is14,174.28 and remaining attributed position cost81,849.38. This partial sale does not establish positive whole-account ROI. [Original receipt and FIFO evidence](research/tartarus-learning-v2/r67-first-sale-20261021.json).

GPU population037 is still running. The latest fully evaluated checkpoint is unprofitable. Continuation040 is queued with founder retention, family-specific market selection,64-transition PPO batches and an equivalent replay-copy optimization. No learned model is deployed; no independent positive ROI is established.

- [Current production and research state](docs/CURRENT_STATE.md)
- [ML implementation and remaining requirements](research/tartarus-learning-v2/IMPLEMENTATION_STATUS.md)
- [Current execution ownership: issue120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120)
- [ML execution and evidence: issue178](https://github.com/GodlyDonuts/predictions-cup-2026/issues/178)
- [Agent workflow](docs/AGENT_WORKFLOW.md) and [coordination](docs/COORDINATION.md)
- [Previous README, preserved in full](README_HISTORY_20261021.md)

The historical README and dated research reports are evidence of earlier states, not current runtime instructions. Credentials, raw authentication material and private runtime/checkpoint directories must never be committed.
