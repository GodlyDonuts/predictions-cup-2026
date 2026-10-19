# Faster serial inference without changing sampled actions

The optional `acting_validation=consolidated_v1` mode reduces repeated GPU-host synchronization in the existing actor. It preserves the original distribution arithmetic, Torch sampling kernels, full raw action support and API/clock rules. Teacher-forced and gradient paths retain their original validation. This is an execution-cost improvement, not a new profitable strategy or completed full-history qualification.

Profiling the actual58,183,917-parameter GPU checkpoint found39 synchronizations in one ordinary forward pass. Repeated categorical mask/parameter/sample checks and nested Beta checks dominated this count. The new path validates each categorical mask and normalized parameters together before sampling, validates the two Beta concentrations together, and checks the returned fraction supports together. It also transfers scalar actions as one group, preserving categorical integer types and float64 latent values. Arbitrary sampler callbacks retain the original checked path; only the ordinary Torch sampler and the checked case-local sampler use the optimization.

The mode is part of saved checkpoint metadata, frozen experiment configuration and the behavior guard. Invalid masks, normalized categorical parameters and Beta concentrations/samples still fail before dispatch. Existing checkpoints default to `strict`; old frozen workers do not acquire the optimization from a main-branch edit. No price/quantity rounding, no artificial decision limit, no shortened evaluation horizon and no source-data shortcut is introduced.

## Actual finite GPU measurement

A pinned GPU017 checkpoint was evaluated on four already-inspected nominal/stressed cases,64 decisions per case, with every actually available SIG book target primed through paid simulated reads. Strict and consolidated modes ran in forward and reverse order, using the same conservative strict-mode virtual decision-cost calibration for comparison. Full raw actions, RNG states, physical financial digest, final observed features, carried recurrent state, clocks and API budgets matched **exactly**, including complete order records. This comparison does not normalize away requested-quantity or executable-remainder differences.

| Mode | Median wall seconds per256 decisions | Forward synchronization count |
| --- | ---: | ---: |
| Strict, latest frozen source | 5.866239 | 39 |
| Consolidated serial, latest frozen source | 4.606043 | 12 |

Measured throughput was **1.274times** strict serial inference on the frozen Git source while the main learner was also running. The earlier checkout measurement was1.262times and is retained. Its110 byte-different runtime files differed only by CRLF normalization, so a separate frozen-build measurement was performed and all115 runtime pins were checked against that exact frozen source. Both measurements preserved all compared paths exactly. [Frozen-build measurement](acting-validation-frozen-gpu-001.json) / [earlier checkout measurement](acting-validation-gpu-001.json).

This is not a full-generation speed guarantee, production latency measurement, GPU training-memory qualification or ROI result. Newly calibrated runs may have a different virtual inference cost and must generate fresh selection evidence. The earlier batched-encoding order-state mismatch is retained and does not justify enabling that separate batching mode.

## Verification and current limits

The complete learning-actions suite passed **284 tests in66.406seconds**. Nine focused checks cover CPU/CUDA bitwise actions/density/hidden/RNG equality, unchanged teacher-forced gradients and support checks, invalid inputs failing before clock/order changes, exact complete-stream finance and budgets, real PPO/checkpoint mode retention, benchmark priming/exactness failure detection and refusal to open an independent test.

A separate recorded-data factory run exercised the optional mode through two H32 policies, two worlds and two complete development cycles. Every saved founder/member/child checkpoint retained its declared validation mode, and runtime source pins were rechecked. It uses two-second training episodes and a four-second selection window with no linked collateral: [exact counts and losses](acting-validation-recorded-001.json). This is integration evidence, not a profitable or full-size training qualification.

That run completed1,448 decisions and52 accepted updates; its only negative worst-selection result was-9.207%, while the other seconds-long results were0%. Zero is not profitable evidence. Mutation-probe files were inspected separately from model checkpoints; they contain training feature frames rather than a model/mode bundle.

At22:42UTC, source `bb948445143d8fbaa889dd232c531aa5b35161af` was queued as the018 successor with consolidated serial acting and historical least-loss champion retention. Controller35976 (launcher7424) waits for the complete all-eight-member017 generation, then verifies evolved checkpoints and full training capacity before launching and checking new accepted updates. Superseded controller33568 acknowledged cancellation and exited; original trainer39824 stayed live. The new mode is **not yet active inside017**. Old selection scores, optimizer trajectories and financial worlds are not imported as fresh evidence; learned weights and parent search hyperparameters are preserved through the checked plan. Batched evaluation and optional owned-order/component-debt profiles remain disabled in this successor. [Exact pending configuration and evidence](profile-018-fast-serial-pending-001.json).

The larger objective remains incomplete: verified multi-day three-venue coverage, calibrated full API/collateral/lifecycle/valuation behavior and independent positive ROI are still missing. [Least-loss champion retention](negative-score-elitism-20261019.md) and the [optional debt bridge](component-execution-20261019.md) remain separate reported features. No account client, production financial state, STOP, activation or risk control changed.
