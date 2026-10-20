# Ground learning targets in completed training outcomes

GPU020 is now verified running full-return training after GPU019's completed generation and checked offspring transfer. Its first cohort committed three updates but remained flat. [Current run and limitations](population-020-full-returns-20261019.md). The investigations below were performed while GPU019 was active; their original scope remains unchanged.

## Measured weakness

Two fresh **CPU training-prefix** probes use actual saved GPU019 member weights, the original 32-step chunk length and original GPU virtual-compute calibration. They use already inspected TRAIN history, not selection or held-out observations. They are not reproductions of original CUDA trajectories. No optimizer or GPU context is used, and model fingerprints remain unchanged.

| Probe | Economic reward transitions | Policy gradient L2 | Weighted value gradient L2 | Weighted entropy gradient L2 |
|---|---:|---:|---:|---:|
| Member0,32decisions | 1 | 27.7079 | 1.54768 | 0 |
| Member2,32decisions | 0 | 21.9206 | 0.00797 | 0.11912 |

The second prefix shows that large actor gradients can exist without a reward transition in that chunk; entropy is not the largest term there. Such gradients are not automatically a bug: a critic may carry legitimate delayed value. Loss scalar magnitudes alone also cannot measure gradients, and gradient norms do not establish the subsequent Adam displacement or any whole-run causal explanation. [Exact gradient probes](credit-gradient-audit-001.json).

The member0 behavior was then run unchanged through the **entire same 120-second CPU training episode**. It completed97decisions/four chunks and lost2.7473825% marked wealth. Sum of rewards exactly matches terminal log wealth−0.02785828868. The first32decisions have mean realized full-episode return−0.01654086, versus mean bootstrapped target+1.95529762 and mean critic value+1.65326252. Critic RMSE against the realized remaining return is1.94310; chunk-target RMSE is2.01578. Raw bootstrapped/full-return advantages retain0.83751 correlation, so this is not proof that every update has the wrong direction. It is a large calibration gap on this single sampled training path, not a population-wide performance theorem. [Complete episode probe](full-return-credit-audit-001.json).

## Next training comparison

Use the already implemented `closed_episode_return_replay=true` at a natural completed-generation boundary. Frozen behavior finishes each TRAIN cohort before optimization; original rewards are summed through the true terminal horizon, and normalization uses the entire cohort. Exact recorded probabilities, recurrent states, observations, economic rewards and source/accounting hashes are checked during reconstruction. No future observation is exposed to the actor. All API actions and original economic rewards remain unchanged. [Method, safeguards and prior evidence](closed-episode-returns-20261019.md).

This removes an estimated bootstrap from those training targets. It does **not** remove reward variance, recurrent truncation, execution-model error, overfitting or the need for genuinely unused test data. It adds reconstruction/IO work, and a KL stop can reduce accepted updates. Better ROI or wall-time efficiency must be measured rather than presumed.

`closed_episode_capacity.py` exercises the complete collect/seal/reconstruct/optimize path with actual configured architecture/world count on a short capacity horizon. The continuation controller requires this additional proof whenever the next profile enables full returns; an ordinary chunk-capacity pass is insufficient. It runs only after the exact previous learner releases the GPU. Failed checks cannot launch a new learner. CPU software fixtures check both successful handoff ordering and rejection of incomplete full-return evidence. No full-size GPU result is claimed until that scheduled probe actually passes.

The CPU suite passes360checks in70.540seconds with four CUDA checks skipped. A separate recorded-data run from frozen source `77c532ef114354a3084b050492fe2423eb058a97` completes two short CPU worlds,76decisions and four accepted full-return updates. All runtime pins match the snapshot; collection, sealed diary, reconstruction, original full-return targets and actual parameter changes are exercised. This small-model check is not full-size GPU capacity or profitability. [Recorded capacity integration](closed-capacity-recorded-001.json).

The original exact-cycle controller waited for GPU019's cycle0 completion, then preserved its checked eight-member population after replacements and stopped only that exact old trainer. Its complete-episode preflight subsequently rejected a verified desktop-host GPU context. The narrow repair, repeated full-size capacity check and successful GPU020 launch are documented in the [current run](population-020-full-returns-20261019.md). [Original queue evidence](full-return-queued-001.json) remains historical, not the current runtime state. No hot patch or early interruption occurred.

Current data still does not provide the requested competition-start/train/evolution/untouched-four-day coverage. No daily-growth qualification or independently positive ROI is established. Production account clients, STOP, activation, risk and financial state remain unchanged.
