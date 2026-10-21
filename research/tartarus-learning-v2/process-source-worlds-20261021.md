**Update October21 06:58UTC:** Eight-world CUDA process admission passed; population034 is now training with all four retained parents and fresh fitness. The old033/full-generation watcher discussion below is dated history. See [actual admission and corrective handoff](population-034-process-admission-20261021.md).

# CPU process source worlds — October21 06:16UTC

The optional online-training backend is implemented and checked on main eaf72ddc. It removes serial host-world advancement while keeping the model, CUDA RNG and synchronous on-policy optimizer in the parent. It is not yet admitted to033/034. The repeated recorded-data source probe measured4.3657x faster source advancement with four workers; this is not whole-training speed or profitability.

## Runtime behavior

Set `process_source_worlds: true` with the existing `training_worlds` configuration. Each world gets one persistent CPU process created with spawn, its own source cursors, wallet, liquidity depletion, orders, observation memory and simulated account rate budget. Children do not construct an official account client or own a CUDA context. The parent sends one action from each exact prepared observation, then waits for all original world transitions before the next model step or PPO update. Finished-world padding and compaction retain original IDs and recurrent-state write-back.

Actor latency is calibrated against the original local environment before proxy creation; offline IPC latency is not charged as extra simulated production response time. A long source alarm has no observation timeout or automatic restart. A missing or failed worker poisons the cohort: partially acknowledged transitions cannot be retried, and no replacement financial world is fabricated. Explicit close after failure abandons only those private simulation worlds.

Spawned source initialization also exposed real contention in the existing global data-use registry. A WaitingDataUse subclass waits only on the original exact OS-lock busy result. It continues to reject protected calendars, malformed records and invalid identities. No holdout permission is removed. Hardware-only child configuration selects CPU/one thread; economic mechanics and source/calendar configuration remain unchanged.

This first process profile supports the current online PPO path. The separate closed-episode reconstruction path remains on its original backend; requesting their unsupported combination rejects explicitly. This is an engineering compatibility gap, not a restriction on model actions. No quantity, decision-frequency, position, venue-observation or execution-operation domain is narrowed by this backend.

## Checked evidence and limits

[31 focused checks](process-source-worlds-focused-checks-20261021.json) pass in33.511s, including real spawned processes, independent filled financial fixtures, byte-identical observation/action frames, final recurrent state and parent RNG, identical final model parameters/Adam state, full two-day human-reward conservation, finished-world compaction, missing-worker/no-retry behavior and original registry-lock/holdout rejection. The actual Experiment.train_parallel entrypoint also completes online updates over a retained SQLite fixture and matches the serial model/optimizer exactly. These are mechanism checks, not evidence of strategy ROI or full-history qualification.

The [recorded-data probe](probe-process-worlds-20261021.py) completed on four processes over the existing fit calendar, original two-day financial horizons and the same V3/fully audited parser configuration intended for034. It measures startup separately, excludes a warm-up and compares balanced source-advance rounds including IPC. Only a controlled raw alarm trace is used; no model inference, optimizer, real account request or candidate fitness is created. Exact observation/financial digests, paid-read counts and receipt records must agree. [Completed003 plan](process-source-worlds-real-source-plan-003-20261021.json), [exact result](process-source-worlds-real-source-result-003-20261021.json) and [runtime/public script binding](process-source-worlds-probe-source-binding-20261021.json).

The first probe stopped because the benchmark compared a parallel transport envelope (including state metadata) with a local economic result. Its wrapper also incorrectly indexed an optional cash field rather than using the environment default. The original failed attempt is retained; no timing or outcome from it was admitted. The corrected probe compares the exact economic fields and separately checks full state/receipt parity. [Correction record](process-source-worlds-probe-wrapper-correction-20261021.json).

The active033 learner remains on its frozen source. Last checked it had734 logged optimizer updates/27144 decisions, with six of eight member1 worlds complete. The existing034 watcher still waits for a full completed generation on its prior V3 source; it was not requeued for this unqualified backend. The whole learning objective remains incomplete: original history/calendar, execution/valuation qualification, full predecode-once infrastructure, completed evolution and independently positive ROI still require evidence. No production financial state or protected test data changed in this goal turn.


## Completed source timing and next admission

Three paired ten-minute source-advance rounds alternate serial/parallel ordering after an unmeasured warm-up. Median time improves from13.1585s to3.0141s, or4.3657x. The source windows have different event densities; this is an engineering measurement under concurrent existing training, not a statistical performance forecast. All four worlds retain exact observation/financial digests, API-request counts and full retained receipt records after every round. Initial cohort setup costs47.17s serial versus68.59s with processes. Four CPU-only worker identities match their actual process handles; sampled RSS totals2.285GB, not a worst-case memory bound.

Attempt002 completed its numerical rounds but failed final reporting on an unavailable optional psutil dependency. It remains a failed wrapper attempt with no admitted timing result. Attempt003 removes that dependency, saves each round before optional telemetry and uses existing Windows process metadata; its final report is complete. [Retained diagnosis](process-source-worlds-probe-telemetry-correction-20261021.json).

The next necessary step is to make the GPU-capacity handoff exercise this actual process backend, including eight-world memory/accepted-PPO evidence. The old batch capacity probe only exercises local worlds, so its green result cannot authorize this new backend. Current033 stays live and the previous034 watcher remains unchanged until that admission path is concrete. No full-generation evolution or independently positive ROI is claimed.


## Runtime-profile and operational readback

A further31-case run passes in39.501s after extending the spawned/local equality checks to both compact-rollout modes and encoder activation checkpointing used by the queued configuration. Final carried hidden states, parent RNG, model parameters and Adam moments remain identical. Only test coverage changed; the implementation measured by the recorded-source probe is unchanged. [Exact profile-matched check](process-source-worlds-profile-matched-checks-20261021.json).

Separate read-only production verification at06:18UTC confirms the same installed979680/startTicks53034794 and binaryf889b34762e5 in the trading loop with healthy realtime and no STOP. This goal turn changed no runtime control or financial file and made no account request. New trades or profits are not established by operational health. [Actual current-process proof](production-startup-repair-live-20261021.json).
