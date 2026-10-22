# Tartarus implementation status â€” October 22, 06:23 UTC

**Current work: the minute target remains unmet.** R85 is still the production worker; at07:16:34UTC it reported healthy trading,573 execution receipts and1746 terminal orders, with no active STOP. A new matched portfolio and237-quote reference package is complete. Fresh whole-portfolio searches found positive declared monetary sensitivities without a one-step first-place score gain. An explicit positive-expected-money objective is now implemented so that the distant leaderboard threshold does not prohibit incremental expected gains. Native checks passed140/0; combined runtime/worker qualification is in progress. The candidate is not deployed, and modeled gains are not realized ROI. [Current findings and remaining execution work](current-money-route-20261022.md).

**R85 resumed healthy trading at05:41:28 UTC**, independently verified05:41:44. Sole worker1069937, binary `5222f0f999fa`, execution source `7fb6fee064521c8e2f9f1bb2419bb7e6050b3efa`, updater1069940. The entire2,691,159,025-byte financial prefix and all535 original fills are unchanged. The original1075 reservation20937 was archived without financial mutation or retry; known20904 was reconciled terminal. Reference generation104 committed for all13 targets. A fresh15-minute fill-rate window is running; the target remains unproven. [Fresh production proof](r85-fresh-live-20261022.json), [original cohort recovery](r85-original-cohort-recovered-20261022.json).

R87 is a checked **offline candidate**, combining final-entry report deferral with a bounded original-event offset index. All249 worker checks pass; native/runtime libraries are unchanged. One live stack caught whole-journal parsing during cohort cleanup. R87 is not deployed. [Candidate and evidence](cohort-index-r87-20261022.md).

**One distinct filled order/minute remains unmet.** The completed05:41–05:56 UTC window had **one first-filled order in15minutes (0.0667/minute)** and14empty UTC minutes. SELL3,473 YES865 at0.88 realized **42.69 SUSQies net** against original FIFO. Five partial receipts belong to that single physical order. [Completed measurement](r85-cadence-window-20261022.json), [official-receipt/FIFO audit](r85-turnover-window-20261022.json).

The separate06:15 receipt/FIFO audit has two newly filled orders since R85 resumed:3,473 YES865 sold and6,064 YES993 sold,35 execution receipts, **305.75 additional realized net**. Dated cash22,392.215, remaining original cost74,101.215 and total attributed realized913.515 include unchanged inherited76.52. At the later06:22 official receipts, cash24,840.465 plus marked positions69,248.96 totals **94,089.425 SUSQies (−5.910575% from100,000)**. This non-atomic mark does not prove executable liquidation or account growth. [Later execution audit](r85-later-turnover-20261022.json), [latest official mark](r85-latest-account-20261022.json).

R85 median completed-scope spacing was **118.7945 seconds** over six intervals. Incoming fresh books still had gaps against the native view. This small, changing workload does not establish a general speedup. The broader rank strategy currently uses an expired nomination from2026-10-19; it is inactive while fresh current-position/market evidence is prepared. [Processing measurements](r85-scope-progress-20261022.json), [coverage](r85-book-coverage-20261022.json), [stale rank evidence](r85-stale-rank-nomination-20261022.json).

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
