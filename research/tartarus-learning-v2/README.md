# Tartarus Population Learning v2

The human specification and proposed configuration are preserved byte-for-byte in `TARTARUS_POPULATION_LEARNING_V2.md` and `EXPERIMENT_CONFIGURATION.json`, with hashes in `source-manifest.json`. [Task178](https://github.com/GodlyDonuts/predictions-cup-2026/issues/178) tracks solo implementation. The requested32 population members are simulated policies, not Codex agents or account clients.

## Implemented

- Exact integer target-exposure sizing, original FIFO, complementary YES/NO netting, cash/holdings reservations, pending-order targets and distinct order/market/commitment/fresh-market limits: [account](../../moloch/learning_v2/account.py) and [environment](../../moloch/learning_v2/reference_env.py).
- Arrival-time complementary displayed liquidity with persistent counterfactual depletion, partial fills, receipt-delayed money and charged cancel/fill races: [liquidity](../../moloch/learning_v2/liquidity.py).
- Virtual exchange time and one serialized completion-paced account budget, inclusive rolling minute, Retry-After and unknown-write blocking: [clock](../../moloch/learning_v2/clock.py).
- Shared contract encoder, set aggregation,512-unit GRU, autoregressive actions, monetary quantiles and distinct success head: [policy](../../moloch/learning_v2/policy.py).
- Fresh-policy fingerprint checks, whole-episode recurrent recomputation, elapsed-time PPO, separate monetary targets and immutable checkpoint bundles: [trainer](../../moloch/learning_v2/train_inner.py).
- Full-observation causal source compilation, predecoding outside the inner loop, mechanics-only target actions and a finite GPU historical trainer: [compiler](../../moloch/learning_v2/compile_trace.py), [episodes](../../moloch/learning_v2/episodes.py), [runner](../../moloch/learning_v2/run_experiment.py).

These components pass89 focused checks. Correct component behavior does not establish trading profitability.

## First actual historical PPO run

[The retained result](initial-ppo-development-result.json) records a1,579,806-parameter actor trained on1,200 decisions in four complete five-minute development episodes. Including decoding, matched-calendar hold baselines, receipt drainage, PPO and checkpoint writing, it took74.365 seconds on the RTX5070Ti laptop GPU.300-decision policy rollouts took about4.2-4.5 seconds; updates about6.4-7.1 seconds; source decoding9.13 seconds.

Checkpoint SHA256: `b5175012364e62f3ab4386e38174ba4120b67759d67059c0d08b4258c3b99e80` (private model file). Episode reference returns were **+0.0388455%, +0.012694%, -1.372289% and -0.728575%**, versus zero for the cash-only hold baselines. These are changing training policies on inspected history, not fixed-policy out-of-sample evidence. The checkpoint does **not** establish improvement. All terminal modeled accounts reconciled; losses are retained. Tournament outcomes were unavailable and no success labels were invented.

[The mechanics manifest](initial-mechanics.json) separates original account limits from explicit latency/preflight/freshness hypotheses. [The frozen chronology](initial-evaluation-protocol.json) keeps gradients inside fit-training. A common120-second terminal drain charges cancels and retains reservations until receipts. The baseline starts cash-only100k; bankroll and causal inventory curricula remain pending.

## Actual data and limits

[Original-input replay](initial-development-dataset.json) verifies25,160 trace records,4,000 cache applications and2,578 SIG row applications across3 rotations. The compiled observations contain107,817 books,115,441 references,24,093 reference gaps and980 SIG gaps, including three explicit rotation barriers. Receipt/availability/source clocks and original hashes are preserved; unknown publisher times remain unknown. Old selected strategy triggers and future outcome labels are not observations.

The inspected capture supplies **SIG and Kalshi; Polymarket is absent**. It is development data, not an untouched test. Processing-epoch/topic binding, complete trade tape, cross-segment continuity and settlement equivalence remain unverified. Arrival latency, passive queue probability, market response and execution stress are not calibrated. Simulation marks are neither executable liquidation nor competition ROI.

[GPU readiness](gpu-readiness.json) verifies RTX5070Ti,12GB-class memory, PyTorch2.11.0/CUDA12.8 and an actual GPU kernel. Installation follows [official PyTorch instructions](https://pytorch.org/get-started/locally/).

## Population execution

A four-member/two-cycle [end-to-end trial](population-smoke-result.json) completed in163.3 seconds, including a fresh-trajectory child after checkpoint transfer. The local mutation measured categorical market/family KL0.001031 against its0.001 target. All original losses are retained; the economic archive admitted zero entries. This is scheduler evidence, not a profitable-policy result.

The first full **32 logical learners / four islands / two cycles** experiment completed in1459.34 seconds, with one learner resident on the GPU at a time. Its second search block had no fresh SIG books, so the run is disqualified for finalist selection and admitted no archive entries. [Full retained results](population-32-v1-result.json) and [coverage diagnostic](v5-observation-coverage.json) preserve that failure. A revised32-member run is now running from fresh lineages under [a new split](development-evaluation-protocol-v2.json) and [explicit mechanics](development-mechanics-v2.json). It trains across fixed-depth bankroll stress points while evaluating every candidate on the same two search-development calendars. Physical limits are immutable inputs; mutation changes only learning parameters/weights. Parentage, protected elite checkpoints, optimizer resets, mutation diagnostics and failed evaluations are recorded. Revised results remain pending.

[The initial-run audit](initial-run-audit.md) records subsequent gap-depletion and recurrent burn-in corrections. [The independently compiled native-wallet differential](native-wallet-differential-result.json) matches ten exact accounting states. It is not full transport/simulator parity.

## Current evidence and speed

[The requirement map](IMPLEMENTATION_STATUS.md) distinguishes implemented code, executed evidence and unfinished requirements. [The real-history update benchmark](sequence-update-benchmark.json) measured8.356s versus.448s (18.65x) for batched stateless encoders/action likelihoods, preserving the original recurrent semantics. This is update speed, not end-to-end speed. A subsequent [two-episode trust-region run](ppo-v2-trust-smoke-result.json) accepted four backtracked updates in roughly1.18-1.59s each; its positive marked training returns are not validation or actual profits.

[The prospective forward protocol](forward-cohort-protocol.json) reserves23:20-00:00UTC, with finalists required before23:15. Source gaps, missing equivalence and unknown processing/sequence guarantees remain explicit. New Polymarket public REST/WS data is captured locally; the existing20-read/s Kalshi collector is reused. SIG observations are read from the existing sole worker's files, without another account client.

## Remaining

The complete experiment is unfinished: the full32-member run and useful reevaluated behavior archive, matched-compute ablations, locked finalists and forward results remain pending. Rust differential verification, Polymarket integration and execution calibration/stress evidence are also required. The learned checkpoint is not deployed. Next comes checkpoint-safe population transfer, behavior-controlled mutations, islands/archive and comparisons under measured equal budgets.

## Reproduce

Use a private environment pinned to `torch==2.11.0+cu128` and `numpy==2.5.2`:

```powershell
python -m unittest discover -s moloch/learning_v2 -t .
python -m moloch.learning_v2.compile_trace --originals ORIGINAL_CAPTURE_DIRECTORY --output NEW_PRIVATE_OUTPUT_DIRECTORY
python -m moloch.learning_v2.run_experiment --dataset COMPILED_CAPTURE --mechanics research/tartarus-learning-v2/initial-mechanics.json --protocol research/tartarus-learning-v2/initial-evaluation-protocol.json --output NEW_PRIVATE_RUN --episodes 4
```

Original captures and model weights remain outside Git. Hash-matched COMPLETE manifests, not partial files, establish completed runs.
