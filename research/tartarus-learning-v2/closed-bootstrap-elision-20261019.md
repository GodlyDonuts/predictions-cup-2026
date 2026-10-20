[Later checked public-trade path and queue](public-trade-stream-20261019.md). Earlier queue/source observations below are historical.

# Remove critic bootstraps unused by sealed full-return training

Closed-episode collection previously performed another observation preparation and model inference after every compute chunk to produce a critic bootstrap. Sealed episode optimization replaces these targets with the complete realized return, including later rewards and the terminal correction. The chunk bootstrap is therefore unused on this path.

`BatchStream.collect(..., bootstrap=False)` now omits that extra observation/model pass and carries an explicit absent bootstrap. Ordinary streaming collection retains its existing default. A bootstrap-free chunk cannot be optimized as an open streaming chunk: it requires a verified sealed episode session and prescribed complete returns. The optimizer no longer constructs discarded bootstrap targets before substituting the verified targets. Full-return collection selects this path directly; it does not alter policy parameters or optimizer hyperparameters.

This removes one encoder/recurrent inference per compute chunk. At32 decisions per chunk, it removes one of33 previous encoder calls (about3.03% of those calls, **not** a measured3.03% total training speedup). Observation preparation and device transfers for that redundant inference also disappear. No paid observation, legal action, actor decision, market time, rate budget, economic reward or selection window is skipped.

Twenty focused CPU tests passed in5.965seconds, with one CUDA-only test skipped. Exact conformance covers both the base and current469-feature named-API architecture, full and compact rollout statistics, and finished-world compaction. Stochastic sampled actions, RNG state, original input tensors, hidden states, economic rewards, virtual clocks and financial digests match the bootstrap-enabled path exactly. Forward hooks verify exactly one removed encoder call per chunk. Bootstrap-free sealed collection is also exercised through reconstruction, full-return optimization, selection and evolution. Invalid standalone optimization is rejected without changing parameters or optimizer state.

The first local test attempt caught a remaining unconditional `detach()` on the absent bootstrap; it failed before publication and was corrected. Failed tests are not counted as evidence. No active frozen source or checkpoint was changed. GPU capacity and wall-clock performance still require serial measurement after the current learner releases the device; the CPU fixtures establish semantics and removed work, not GPU throughput or ROI.

## Current queue and learning evidence

The complete action suite subsequently passed369CPU checks in70.735seconds, with four CUDA-only tests skipped. Source`5115a4fa7413f1df22b97d69799a5a2a521584d4` was pushed and read back on main. It includes both [rejection continuation](episode-rejection-continuation-20261019.md) and the persistent exploration grid.

The previous waiting controller19184/launcher43720 was cancelled through its existing cancellation interface. Its status reached`cancelled_before_handoff` and both processes were confirmed absent before the replacement was launched. The active trainer11404 was independently verified alive before and after the refresh; its frozen544a3986source was never edited. The replacement controller18544/launcher35128 is verified live and waiting for the same complete GPU020generation. [Exact queue/source/config evidence](population-021-queued-002.json). No next-profile GPU probe or learner has started yet.

At the evidence observation, GPU020 had89,646decisions and131committed accepted updates. Its fourth completed training cohort accepted all94 attempted updates; its corresponding development selection reward was-0.34421947915658196. All four completed selection rewards remain flat or negative. Thus low minibatch utilization is only one observed problem; neither its removal nor additional updates is proof of an economic edge. Full history, exact execution/valuation calibration, longer-horizon qualification and independent positive ROI remain incomplete.
