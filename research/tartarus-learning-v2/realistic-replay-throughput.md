# Faster replay and GPU learning — October 19, 2026

The new vector collector is measured at **9.51 times** the throughput of separate
policy calls for the same 56,538,142-parameter model, real inspected tape,
32 independent replay wallets, one-second decision interval and original
information/execution clocks. This is a software throughput result, not profit,
an isolated hardware benchmark or a complete tournament training result.

[Raw measured result](realistic-replay-throughput-wide-001.json) contains both
runs per mode, workload/source hashes, phase timings, CUDA context and the actual
GPU PPO update. One warm-up precedes the measured runs. Another learner and the
desktop app were using CUDA concurrently; results should not be presented as
uncontended device capacity.

| Measured workload | Separate policy calls | Batched collector |
|---|---:|---:|
| 32 worlds × 40 recurrent frames, median wall time | 14.898 s | 1.567 s |
| Aggregate virtual seconds / wall second | 85.9 | 817.1 |
| Full model parameters | 56,538,142 | 56,538,142 |

The 1,280 virtual seconds include recurrent burn-in and run concurrently across
32 worlds; they are **not** 1,280 seconds of new independent market history.
This is about 25.5-times clock speed per world in this particular short workload,
not 817-times clock speed for a single continuous live trading trajectory.
No full-day/tournament-duration throughput forecast has been validated.

An additional stochastic collection took **2.046 s**, followed by an actual PPO
optimizer update taking **1.074 s**. All 32 worlds passed terminal relative
interval-accounting qualification; 1,120 decision samples were eligible for
learning. Peak allocation owned by this benchmark was about **1.85 GiB**; this
does not include the other learner or desktop GPU allocations. The accepted
update is evidence the collector feeds the existing trainer, not evidence of
learned trading skill. It uses the original development interval-HOLD objective,
**not** the proposed multi-day growth score.

## What changed

* `fast_replay.PreparedTape` loads/validates data once and shares the original
  read-only rows and predecoded immutable `Book` objects among replay worlds.
  Every world still owns fresh wallet, orders, source caches, liquidity depletion
  and rate-budget state. No history is read from a future role.
* `vector_rollout.collect` advances worlds directly instead of coordinating a
  Python thread/queue/barrier for every actor frame. It forms one observation
  batch and calls the policy once per frame.
* Recurrent hidden state stays on the GPU between frames. Only required actor
  observations and actions cross the device boundary, packed by dtype.
* Collection can return complete bounded `Trajectory` objects accepted by the
  existing PPO trainer. Burn-in, virtual elapsed time, source freshness, charged
  cancels, receipt drain and reward accounting are retained. The caller must mask
  worlds whose `qualified_relative_interval_accounting` is false before learning;
  the benchmark demonstrates this masking.

The initial smoke profile showed repeated JSON decoding and many small model
calls. After batching, CPU event advancement and observation work account for a
larger fraction of elapsed time. Further optimization should target measured
CPU work before making the network smaller or changing decision frequency.

## Correctness and limitations

The combined focused suite passes **55 tests**. Three new differential checks
verify prepared-vs-original observations and masks; separate wallet/liquidity
state despite shared original books; and original-vs-vector complete trajectory
rewards, elapsed time, recurrent boundaries and receipt drain. A fixture also
performs an actual PPO update.

On the real-data GPU workload, discrete deterministic choices and complete final
accounting match the separate-call reference. Maximum raw fractional-action
difference is 0.00000519 from GPU floating-point batching; this is measured,
not claimed bit-identical inference. The tested workloads' resulting accounting
is exact. This does not prove every possible future action near an integer
rounding boundary will be numerically identical.

SIG limits remain 100 read admissions/minute with 600 ms completion pacing and
24 writes/minute with 2500 ms completion pacing. Kalshi/Polymarket stay separately
paced, observation-only sources. No data subsampling, shortened virtual latency,
extra API request, new production worker or production limit change was used.

Full historical dataset coverage, calibrated execution assumptions, complete SIG
API action parity, the new reward's population coordinator and untouched final
testing remain separate requirements. Replaying inspected minutes faster does
not establish those requirements or cure insufficient independent data.

## Use

For a real frozen experiment, open through
`fast_replay.open_prepared_replay(spec, dataset, mechanics, role, ...)`, which
preserves the full-history/source/role checks. Use `PreparedTape.from_loaded`
only when the original tape has already been verified and the intended role is
explicit. `vector_rollout.collect` accepts that tape, the original mechanics,
one policy and independent episode start times. It returns `(trajectory, report)`.
Never train on a test-role trajectory; the collector explicitly refuses it.

Reproduce the engineering benchmark using already inspected data:

```powershell
python -m moloch.learning_realism.benchmark --dataset INSPECTED_FLOW_DATA --mechanics research/tartarus-learning-v2/development-mechanics-v2.json --output NEW_RESULT.json --worlds 32 --steps 40 --hidden-size 4096 --market-hidden-size 128 --repeats 2 --ppo-probe
python -m unittest moloch.learning_realism.test_vector moloch.learning_realism.test_replay moloch.learning_flow.test_features moloch.learning_flow.test_episodes moloch.learning_v2.test_clock moloch.learning_v2.test_reference_env -v
```
