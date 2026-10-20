# Historical acquisition and neural recovery — October 20, 19:35 UTC

Two concrete bottlenecks were addressed. Neither establishes a profitable model.

## Historical storage

The original SIG acquisition stopped on storage exhaustion after 947,549 indexed trades and 158 completed exchange scopes. Its stale `acquiring` status was not evidence of a live reader. The repaired selected journal retains SHA256 `443d47f6a03dc027e3cba8555361f36cf05980446354f33afca0613272a6ad8d`.

Checked source `ebe1e08fbdaf445b72e1cceafc5d2b23c3eb874a` preserved 862 strictly superseded SDK files (16,252,918,090 original bytes) in 236,428,969 bytes of Zstandard tar archives. Every member was fully decompressed and hash verified before its original was removed. The active journal/checkpoint and all financial state were excluded. Five tests passed on the actual Linux host, including changed-source and changed-manifest rejection. Original owner and SDK exclusion locks were held throughout; the SDK lock was released afterward.

Archives and per-batch inventories/verification receipts reside at `/home/archon/moloch-history-storage-preservation-20261020-001`. Historical inspection of archived generations requires explicit extraction. The final available capacity was 21,870,735,360 bytes. Independent post-run checks confirmed the selected journal hash, original execution STOP hash, and NETWORK_DISABLED presence. Zero account requests occurred during this repair.

The historical reader's continuation is being qualified: reuse completed scopes in a new evidence directory, reread the incomplete scope, and stop before requests when less than 4 GiB remains. Acquisition has not yet resumed as of this report.

## Neural training

At 19:28 UTC, GPU023 and its supervisor were absent. Last log: 51,968 decisions, 406 optimizer updates. No exit record or causal diagnosis exists; do not attribute this to OOM without evidence. Its original run remains preserved.

Recovery snapshot: 51,712 decisions / 404 updates, SHA256 `29b49268a59e72f74b5a54ddb72a3819b545259e8b54afd17f9cd2a1a8411ac3`. Every tensor was finite. The new same-architecture transfer keeps weights, records development lineage, resets optimizer/financial trajectories, and requires new scores. A CPU founder probe verified exact inherited weights. Sixteen focused tests passed.

GPU024 launched at 19:32:26 UTC from checked source `16d578b3c48ead5268a240027abb5664118bb4f3`; configuration SHA256 `0c4069f1870e351dfb37c95a01f73c64c0e011e834b6b9f334342e7159757931`. Actual learner PID8848, wrapper23820, supervisor20196. The population has four inherited and four fresh founders. Fresh founders use WAIT fraction logits `[1.4,9]` instead of `[-3,9]`; other action heads and the complete legal deadline range are unchanged. This changes exploration probabilities, not a decision cap. First member is fresh; inherited models retain their existing learned WAIT weights.

At 19:34:53 UTC, GPU024 had logged 4,608 decisions / 36 updates with no stderr. Peak reported CUDA allocation had reached 5,227,329,024 bytes. These are training operations and simulated fills, not official trades or evidence of ROI. Source, data and history-provider bindings are frozen in its private handoff.

Still outstanding: full historical execution-fidelity coverage, complete daily-return reward evaluation, selection of a profitable candidate, and independent testing. The Oct21–25 prospective holdout remains reserved and unused. Current training remains inspected development on Oct19 executable observations with older history as paid information. Production remains stopped; no activation or orders were performed in this recovery.
