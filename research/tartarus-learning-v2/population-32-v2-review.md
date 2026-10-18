# Revised population: completed run and rejected finalists

Observed October18,2026 at23:18UTC. This is conditional development evidence, not actual competition profit.

The revised32-policy/four-island/two-cycle experiment completed in **1180.967 seconds**, using actual CUDA training on the laptop. All64 member/cycle records had observable search data. Three final policies had strictly positive marked returns in both nominal search calendars. The provisional behavior archive contains seven entries, including zero-return baselines. It is not seven profitable strategies. [Retained aggregate and checkpoint hashes](population-32-v2-result.json).

Before the23:15UTC deadline, three candidate checkpoints were reviewed on the same two calendars under nominal execution and a fixed joint adverse profile: read latency900ms, arrival1200ms, receipt600ms, half displayed depth and one0.005 adverse execution tick, with original limit prices still enforced. Every candidate failed the declared requirement for strictly positive returns in all four cases. [Full review, frozen source hashes and timing](forward-finalists-001-result.json).

| Candidate | Nominal block1 | Nominal block2 | Adverse block1 | Adverse block2 |
|---|---:|---:|---:|---:|
| founder-8027 | +1.005772% | +1.504954% | -0.0068025% | +0.995% |
| founder-8013 | +0.2% | +0.41528% | 0% | +0.369645% |
| child-c1-10600732 | +0.015% | +0.3866395% | -0.97078% | +0.125941% |

These gains are external-reference marks on remaining inventory, not realized liquidation profits. All four modeled terminal accounts for each review reconciled, but accounting correctness does not validate the execution assumptions or settlement equivalence.

The review finished at23:14:03UTC with **zero qualified finalists**. The requested two-finalist forward comparison therefore failed to qualify before its deadline. The23:20-00:00 prospective cohort remains a recording, not a successful evaluation; models must not be selected after inspecting it. Original failed results, model checkpoints and the frozen evaluator source remain immutable outside Git. No policy was deployed.

## Following development experiment

The next branch appends explicit market/order/commitment headroom and pending-side exposure observations to the old feature prefix. SELL target support spans the owned exposure so a small position can be reduced without almost every sampled target being larger than the holding. The20/12 legacy checkpoint path retains its old unscaled action support during reevaluation;24/14 policies are a declared behavior change, not silently upgraded old checkpoints. A zero-initialized feature-expansion operator preserves the old prefix function and forbids optimizer/trajectory transfer.

[Fixed execution profiles](execution-profiles-v3.json) now apply during training and selection. Every candidate is evaluated on both calendars under both profiles; selection uses the weakest result. Training alternates the profiles by cycle, common to all policies. Policies cannot change execution mechanics, freshness, account limits or the target used by the evaluator.

A finite comparison is running with four founders, one island and two cycles per mode: independent PPO, local-only replacement and the multiscale mixture. It matches scheduled training/evaluation decisions, calendars, starting cash and random seeds. Mutation/probe overhead and accepted optimizer steps can differ and are measured; this is not exact FLOP matching, a complete A0-A6 ablation or an independent-seed statistical result. The archive is observational in these modes; archive reintroduction has not been implemented. No forward outcomes enter this comparison.

The central unresolved question remains whether learned decisions outperform the existing strategy under realistic execution. This run does not answer it. Current-policy replay comparison, actual-shaped causal inventory, three-venue compilation, locked outcomes, repeated seeds and execution calibration remain unfinished.

The first comparison attempt stopped before its first member result because the archive incorrectly treated the same calendar under different execution profiles as overlapping evidence. That failed attempt remains private and immutable. The fix requires two disjoint calendars within each profile and identical calendars across profiles; it does not count two execution worlds on one calendar as two independent historical blocks. The corrected comparison uses a new output directory.95 focused checks pass after the fix.

The four-member comparison uses the implementation's minimum two-member founder allowance (at most50% in this small trial), rather than claiming the25% ancestry cap of the32-member experiment. It is a small development comparison, not the complete island/archive ablation.
