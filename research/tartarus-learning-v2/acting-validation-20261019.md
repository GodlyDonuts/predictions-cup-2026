# Faster serial inference without changing sampled actions

The optional `acting_validation=consolidated_v1` mode reduces repeated GPU-host synchronization in the existing actor. It preserves the original distribution arithmetic, Torch sampling kernels, full raw action support and API/clock rules. Teacher-forced and gradient paths retain their original validation. This is an execution-cost improvement, not a new profitable strategy or completed full-history qualification.

Profiling the actual58,183,917-parameter GPU checkpoint found39 synchronizations in one ordinary forward pass. Repeated categorical mask/parameter/sample checks and nested Beta checks dominated this count. The new path validates each categorical mask and normalized parameters together before sampling, validates the two Beta concentrations together, and checks the returned fraction supports together. It also transfers scalar actions as one group, preserving categorical integer types and float64 latent values. Arbitrary sampler callbacks retain the original checked path; only the ordinary Torch sampler and the checked case-local sampler use the optimization.

The mode is part of saved checkpoint metadata, frozen experiment configuration and the behavior guard. Invalid masks, normalized categorical parameters and Beta concentrations/samples still fail before dispatch. Existing checkpoints default to `strict`; old frozen workers do not acquire the optimization from a main-branch edit. No price/quantity rounding, no artificial decision limit, no shortened evaluation horizon and no source-data shortcut is introduced.

## Actual finite GPU measurement

A pinned GPU017 checkpoint was evaluated on four already-inspected nominal/stressed cases,64 decisions per case, with every actually available SIG book target primed through paid simulated reads. Strict and consolidated modes ran in forward and reverse order, using the same conservative strict-mode virtual decision-cost calibration for comparison. Full raw actions, RNG states, physical financial digest, final observed features, carried recurrent state, clocks and API budgets matched **exactly**, including complete order records. This comparison does not normalize away requested-quantity or executable-remainder differences.

| Mode | Median wall seconds per256 decisions | Forward synchronization count |
| --- | ---: | ---: |
| Strict | 5.908986 | 39 |
| Consolidated serial | 4.682196 | 12 |

Measured throughput was **1.262times** strict serial inference in this finite run while the main learner was also running. This is not a full-generation speed guarantee, production latency measurement, GPU training-memory qualification or ROI result. Newly calibrated runs may have a different virtual inference cost and must generate fresh selection evidence. The earlier batched-encoding order-state mismatch is retained and does not justify enabling that separate batching mode. [Exact measurement and source pins](acting-validation-gpu-001.json).

## Verification and current limits

The complete learning-actions suite passed **284 tests in66.406seconds**. Nine focused checks cover CPU/CUDA bitwise actions/density/hidden/RNG equality, unchanged teacher-forced gradients and support checks, invalid inputs failing before clock/order changes, exact complete-stream finance and budgets, real PPO/checkpoint mode retention, benchmark priming/exactness failure detection and refusal to open an independent test.

A separate recorded-data factory run exercised the optional mode through two H32 policies, two worlds and two complete development cycles. Every saved founder/member/child checkpoint retained its declared validation mode, and runtime source pins were rechecked. It uses two-second training episodes and a four-second selection window with no linked collateral: [exact counts and losses](acting-validation-recorded-001.json). This is integration evidence, not a profitable or full-size training qualification.

The larger objective remains incomplete: verified multi-day three-venue coverage, calibrated full API/collateral/lifecycle/valuation behavior and independent positive ROI are still missing. [Least-loss champion retention](negative-score-elitism-20261019.md) and the [optional debt bridge](component-execution-20261019.md) remain separate reported features. No account client, production financial state, STOP, activation or risk control changed.
