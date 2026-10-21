# Learning results and raw-intent scheduling — October 21

## Actual outcome

Population 029's first trained member completed October 17–19 development selection. It lost **11.6935% nominal / 41.6825% stressed marked wealth**, with human daily-growth rewards **−0.128855 / −0.524317**, respectively. Both cases cover two complete days; independently profitable event evidence `N` was zero. There were 56/71 simulated fills. These are conditional marked replay results, not dashboard P&L, executable liquidation or independent qualification.

The short matched training-window comparison was less informative: the 33-update checkpoint stayed flat in all eight cases. The founder lost money in two and stayed flat in six, giving a mean paired loss reduction of 1.7700 percentage points. That is loss avoidance, **not learned positive return**, and did not translate to the longer selection window.

Evidence: [full-day selection summary](population-029-first-daily-selection-20261021.json), [all matched cases](population-029-learning-effect-20261021.json).

## Identified execution-availability problem

The policy already chooses an operation before its target, so a large read catalog does not by itself explain the missing trade requests. The actual availability mask hid SIG order, cancel and batch families while a managed read owned the queue or pacing was pending. That prevented the actor from requesting a future admission turn in many states. Merely exposing intents was insufficient: interruptible WAIT and local preparation then abandoned the pending priority, and readiness could arrive without a timely actor wakeup.

The explicit **`model_pausable_raw_intents_service_v2`** profile fixes these interactions:

- Local SIG intents remain selectable during ordinary `BUSY`/`RATE_WAIT`. The unchanged dispatcher decides whether a network admission is possible.
- Interruptible WAIT and local preparation preserve a pending foreground turn. Explicit uninterruptible WAIT declines it and lets data collection continue.
- A repeated pacing response schedules the next turn rather than leaving the collector parked indefinitely.
- Readiness is an indexed V2 wakeup. A turn arriving during inference is not treated as already observed; an interruptible wait exposes it before sleeping again.
- Poisoned and unknown-acceptance states remain closed. No cap, balance, depth, price, quantity, expiry or dispatch retry rule was relaxed.

V1 retains its original event/admission law. Profiles are explicit in checkpoint metadata and behavior contracts; migration requires fresh trajectories and optimizer moments. The initial 59 core/action/population checks passed; the final readiness changes passed 40 managed/wake/replay/batch checks.

## Model-level smoke result

All four parent initializations preserved their parameter hashes and reset optimizer moments. On a fixed already-used ten-minute training interval, the inherited checkpoint admitted 10 SIG writes and produced 13 simulated fills under one seed, versus no fills in its earlier matched V1 run. Another seed stayed flat with no write admission. The active seed lost **3.8094%**, so restored execution is not profitability evidence. Admission caps held throughout both cases.

The existing raw-API simulation budget is 100 reads / 30 writes per minute with zero additional completion gaps, plus the configured request/receipt latencies. This was **not changed** by V2. The production worker's separate 100-read/24-write limits and 600/2500 ms completion pacing are unchanged. Production-pacing qualification remains necessary before treating replay as a production forecast.

See [CPU bootstrap and smoke proof](population-030-raw-intent-cpu-proof-20261021.json).

## Current learning run

Population **030** started under checked source `b1033fbfc644677200c74102cfc9e7bec6ea8c9a`, configuration SHA-256 `bcd813b1d064d18cd1ac855f5f4c963791930f7af8c4e9592067ce90b2230604`. Actual GPU process 5860 was verified as the sole learner. The exact old simulated-training process was stopped only after the CPU gate; its completed negative result, checkpoints and unfinished work remain in place. No old selection fitness was reused.

Member 0 inherits the verified 33-update checkpoint; the other three inherit their original untrained founders. All new optimizer moments start empty. Train/evolution dates, dataset, mechanics, bank and legal API parameter domains remain unchanged. Source/query replay optimizations are included. The first new cohort is collecting; new V2 fitness and positive ROI are unproven. [Start evidence](population-030-start-20261021.json).

Independent October 21–25 is now being captured blind and remains protected and unconsumed. No production financial state or collector control changed.

## Bounded source compiler

`moloch.learning_actions.compile_prospective_source` now streams the pinned original export, canonical producer chain, REST records and session scope without materializing the entire capture. It checks all original hashes before and after, receipt order, explicit development bounds, per-exchange scope, source counts and terminal producer hash. Failed or incomplete input produces no completion artifact.

The shared OS-locked calendar registry records development exposure before reading prices and rejects overlap with the protected independent calendar, even if a caller supplies a later cutoff argument. The CLI supports only `fit`/`search`; it does not admit independent data. Seven focused tests cover these boundaries and tampering cases.

On the original fixture, the bounded compiler reproduced **every normalized observation and trade row byte** from the earlier verified decoder: 14,598 books, 237 gaps and 272 prints across 237 exchanges, in 11.853 seconds. This is source compilation, not policy inference/training throughput or source qualification. [Parity result](bounded-capture-compiler-parity-20261021.json).

Continued exports still require their pinned original transport audit and session manifest. A segment crossing the protected test boundary must remain raw for later qualified handling; changing a role name or cutoff is insufficient to make it development data.
