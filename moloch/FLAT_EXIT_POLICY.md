# Exit policy changes without resetting a bank

## One-tick entry comparisons

`--entry-policy FILE` is a separate flat-only option accepting exactly `insideTicks:0` or `insideTicks:1`. It cannot change capital, quantities, rates, latency, fees or the fill model. It records an optional entry-policy transition that the terminal verifier and fill audit reproduce. Older histories retain their original metadata and hashes. The paired runner accepts a fifth entry-policy file only for a preregistered `entryInside` comparison, applies the same exit policy to both banks, and rejects other policy differences. Improvement remains subject to the existing spread, fee and value constraints; a one-tick spread cannot automatically be tightened into a crossed quote.

Phase62 is preregistered on exchanges922 and880, selected from the verified broad push capture. Both100,000-point banks retain the exact flat parent history and use fixed one-tick net gain exits with a30-minute maximum inventory age. Candidate entry improvement is the only policy difference. All147 core tests pass. This is a fresh forward experiment, not a profitability claim. Registry: `data/moloch-inside-registration-20261014-62.json`.

## Earlier exit experiments

`run.mjs --resume TERMINAL_PARENT --resume-entries --exit-policy FILE` accepts only four exit fields: `inventoryTakerAfterMs`, `inventoryMinimumGain`, `maximumInventoryAgeMs` and `inventoryExitMode`. The exact inherited bank must have no inventory or unresolved quotes. Initial cash, actual cash, realized history, fees, quantities, capital limits, attribution and read/write limits cannot be changed by this option. Constructor validation applies to every new field. Fixed-gain mode requires a positive bounded maximum inventory age.

The transition records the selected fields and prior/next policy hashes with zero new capital. Completed-tail reconstruction and fill auditing reapply the transition before reproducing later events. Older transitions omit the new field and retain their original hashes. Unit and terminal-chain tests verify financial preservation, forbidden capital changes, unresolved-quote rejection, CLI policy loading, complete reconstruction and the independent fill auditor. All 126 core tests pass, including sanitized channel-failure classification.

The one-tick candidate was registered at 21:11:43.603 UTC and transitioned both preserved flat banks. Aggregate realtime source47 and direct-market source48 each exited one with CHANNEL_ERROR before any market receipt. The new diagnostic classified source48 as unclassified rather than retaining its raw error or credential-bearing content. No new trade, fresh forward validation or profitability is claimed.

Both candidate ledgers were preserved flat. Explicit REST-only source49 now collects the same scope with entries disabled under the existing mutex and five-read budget. REST observation is available, but the realtime-dependent candidate validation remains pending. Registry and outcomes: `data/moloch-minimum-gain-registration-20261014-47.json` and `data/moloch-minimum-gain-forward-start-20261014.json`. No contest activation or actual order was introduced.
