# Tartarus implementation status — October21,20:14UTC

This is the current summary. [All previous implementation notes are preserved](IMPLEMENTATION_STATUS_HISTORY_20261021.md).

R71 stopped at 20:06:41 UTC after original-ledger restoration with `LIVE_STREAM_JOIN_FAILED`; it did not reach fresh trading. The old worker discarded the specific startup cause. Source inspection found that public market broadcasts may legally arrive between market and private-account JOIN acknowledgments, while the worker required its first message to be the combined JOIN. R72 buffers those messages until both channels join, records typed failure reasons and retries transient subscription failures through the existing persisted connection budget. Five focused checks and the optimized replacement build pass; installation is next. Native engine, original452fills and financial state are unchanged. Repeat-entry version3 opt-in is still pending. [Repair and evidence](stream-join-r72-20261021.md).

## Running and queued work

GPU population037 remains active on its frozen source, using independent historical worlds and one GPU compute coordinator. The first island saved a2,773-update checkpoint after36,730 decisions and is evaluating it. The second island's first completed member had negligible nominal marked gain and zero stressed return; it has moved to its next member.

Continuation040 is queued on helper2288 for the original trainer18116. It replaces039 before any039 launch. Each island's latest completed generation, learned hyperparameters and all original checkpoints are preserved before migration. New040 training has not started. [Exact queue/source pins](population-040-queued-20261021.json).

The queued source includes:

- Founder evaluation/retention, so a worse first trained generation cannot erase its stronger starting policy.
- Family-conditioned target attention: BUY and SELL can learn different market preferences. The3072-parameter zero-initialized extension preserves old behavior at installation and imposes no action/quantity restriction.
- 64-transition compute chunks and equivalent source-validation improvements.
- Ownership transfer of fresh built-in historical responses, avoiding redundant deep copies. Generic/custom providers retain defensive snapshots.39 focused checks pass; four matched fitting-hour runs have identical complete observation/action/request/receipt/financial hashes and1.040x measured collection speedup. [Benchmark](owned-history-replay-benchmark-20261021.json).

[Target-selection implementation and model audit](family-target-query-20261021.md), [founder retention](founder-retention-20261021.md).

Optional process-based evolution evaluation is checked on sourcee0d83619 but disabled.22 unique focused checks cover behavior, clocks, finance and isolation. In four interleaved CPU passes over a five-minute development slice, all observations/actions/complete results matched, but child processes took154.12s median versus116.24s locally (32.6% slower). This measured result does not justify changing active037 or queued040. No optimization or independent test was run. [Benchmark and exact limits](process-evaluation-benchmark-20261021.json).

## Results and limits

The2,768-update checkpoint completed the full October17–19 development window at-4.3489075% nominal/-4.93724% stressed marked ROI. Its minimum daily-growth reward remains worse than the matched founder. The first island's final2,773-update checkpoint is a different checkpoint and its final evaluation is pending. [Later-checkpoint result](population-037-later-result-20261021.json).

The separate fitting-hour behavior audit found large orders, long uninterruptible waits, and SELL choices against markets with zero delivered holdings. API-defined complement conversion turned those SELL attempts into BUYs; it was not learning effective exits. This motivated family-dependent target selection rather than restricting legal choices. [Behavior evidence](population-037-fit-behavior-20261021.json).

Three venues provide information; execution is SIG-only. Requests, receipts, virtual clocks, modeled inference latency and rate budgets are explicit. The model can select all implemented raw API action families, quantities, prices, builder operations and waits. Software checks do not establish full API parity, calibrated real execution or profitable trading skill.

Current detailed development is2 fitting days plus2 development days. Full competition execution history and the requested4+4 evaluation chronology remain incomplete. ProtectedOctober21–25 has not been consumed. No learned model is deployed or independently profitable.

Production is separate: R70 is verified live at19:16:28 as sole1022216, with healthy realtime, no STOP,433 preserved original fills and all1675 acknowledged orders terminal. The complete original financial prefix is independently verified. Additional R70 fills or increased turnover are not yet established. At18:56:58 official marked equity is94,708.835, down5.29% from initial. The latest R68 sale realized+8.66; earlier R67 sales net-164.63. [Fresh live proof](r70-fresh-live-20261021.json), [current production state](../../docs/CURRENT_STATE.md).
