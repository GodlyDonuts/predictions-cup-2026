# Tartarus implementation status â€” October 22, 05:21 UTC

**R85 installed at 05:19:45 UTC; original-ledger restoration is in progress.** Sole worker1069937, binary `5222f0f999fa`, checked execution source `7fb6fee064521c8e2f9f1bb2419bb7e6050b3efa`, updater1069940. The replacement fixes the proved-no-HTTP cohort failure that stopped R83 and includes R84 source/preflight report deferral. It preserves all535 fills, the full2,691,159,025-byte original financial prefix, all prior engine approvals and known order20904. **Fresh trading is not yet verified.** [Installation evidence](r85-local-guard-cohort-installed-20261022.json), [repair and validation](local-guard-cohort-r85-20261022.md).

R86 is a checked **offline candidate**, excluded from the current production binary. Its final-entry projection scope passed247 worker checks and preserves original receipts, durable head and error behavior. The retained20937 timing trace shows a5.532-second post-book gap before preparation; that gap is not attributed wholly to report work. R85 remains in place for fresh live measurements before another replacement. [Candidate and evidence](final-packet-r86-20261022.md).

**One distinct filled order/minute remains unmet.** The completed 04:35–04:50 UTC window had **one first-filled order in 15 minutes (0.0667/minute)** and 14 empty UTC minutes. SELL 1,472 YES993 shares realized **66.24 SUSQies net** against original FIFO. The preceding window had two first-filled orders and +50.115 realized net. Partial receipts do not create additional trades. [Latest completed measurement](r83-post-delaware-cadence-20261022.json), [official-receipt/FIFO audit](r83-post-delaware-turnover-20261022.json).

At the 04:50:25 audit, cash was **14,147.655**, remaining original cost **82,036.345**, total attributed realized **604.085**, including unchanged inherited 76.52. The separate official 04:39 receipts, observed 04:41:49, mark the account at **94,293.565 SUSQies**, down 5.706435% from 100,000. These are dated, non-atomic marks; realized gains do not establish whole-account profit or executable liquidation. [Official aggregates](r83-account-after-delaware-20261022.json).

R83 examined multiple candidates in a sweep, but median completed-scope spacing was 125.53 seconds. Fresh incoming books were absent or stale in the native view at two measured timestamps. Qualified R84 reduces repeated full-report construction during source/preflight reads. It was never deployed and is included in R85; the latter reproduces and repairs the newer no-HTTP cohort failure. No restored-trading or end-to-end speedup claim is made. [Source optimization](source-projection-r84-20261022.md), [cohort repair](local-guard-cohort-r85-20261022.md).

## Running and queued work

GPU population037 remains active (process18116 verified at20:20UTC). A newly completed second member in island001 produced-10.56343% nominal marked ROI and+0.0030525% stressed marked ROI on the two-day development window. Its minimum daily-growth reward is-0.13955066; it is not a profitable usable checkpoint. The two stochastic cases took145/102decisions and produced15/2fills, with no realized FIFO gain. All new fills in those cases were BUY_NO, so effective exits remain a weakness. [Original-result digest and metrics](population-037-second-member-result-20261021.json).

Continuation040 is queued on helper2288 for the original trainer18116. It replaces039 before any039 launch. Each island's latest completed generation, learned hyperparameters and all original checkpoints are preserved before migration. New040 training has not started. [Exact queue/source pins](population-040-queued-20261021.json).

The queued source includes:

- Founder evaluation/retention, so a worse first trained generation cannot erase its stronger starting policy.
- Family-conditioned target attention: BUY and SELL can learn different market preferences. The3072-parameter zero-initialized extension preserves old behavior at installation and imposes no action/quantity restriction.
- 64-transition compute chunks and equivalent source-validation improvements.
- Ownership transfer of fresh built-in historical responses, avoiding redundant deep copies. Generic/custom providers retain defensive snapshots.39 focused checks pass; four matched fitting-hour runs have identical complete observation/action/request/receipt/financial hashes and1.040x measured collection speedup. [Benchmark](owned-history-replay-benchmark-20261021.json).

[Target-selection implementation and model audit](family-target-query-20261021.md), [founder retention](founder-retention-20261021.md).

Optional process-based evolution evaluation is checked on sourcee0d83619 but disabled.22 unique focused checks cover behavior, clocks, finance and isolation. In four interleaved CPU passes over a five-minute development slice, all observations/actions/complete results matched, but child processes took154.12s median versus116.24s locally (32.6% slower). This measured result does not justify changing active037 or queued040. No optimization or independent test was run. [Benchmark and exact limits](process-evaluation-benchmark-20261021.json).

## Results and limits

The2,768-update checkpoint completed the full October17â€“19 development window at-4.3489075% nominal/-4.93724% stressed marked ROI. Its minimum daily-growth reward remains worse than the matched founder. The first island's final2,773-update checkpoint is a different checkpoint and its final evaluation is pending. [Later-checkpoint result](population-037-later-result-20261021.json).

The separate fitting-hour behavior audit found large orders, long uninterruptible waits, and SELL choices against markets with zero delivered holdings. API-defined complement conversion turned those SELL attempts into BUYs; it was not learning effective exits. This motivated family-dependent target selection rather than restricting legal choices. [Behavior evidence](population-037-fit-behavior-20261021.json).

Three venues provide information; execution is SIG-only. Requests, receipts, virtual clocks, modeled inference latency and rate budgets are explicit. The model can select all implemented raw API action families, quantities, prices, builder operations and waits. Software checks do not establish full API parity, calibrated real execution or profitable trading skill.

Current detailed development is2 fitting days plus2 development days. Full competition execution history and the requested4+4 evaluation chronology remain incomplete. ProtectedOctober21â€“25 has not been consumed. No learned model is deployed or independently profitable.

Production is separate from ML. The latest official marked equity remains the 20:16:40 observation of 94,780.155 SUSQies, down 5.22% from the start. R70's last three filled sell orders realized a net loss of 105.925; no whole-account profit is established. [Current production state](../../docs/CURRENT_STATE.md).
