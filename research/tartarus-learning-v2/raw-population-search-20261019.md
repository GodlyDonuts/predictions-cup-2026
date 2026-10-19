# Raw-action population search and expanded-history training

## What is implemented

`moloch.learning_actions.experiment` now has an opt-in `population_search` profile.
It keeps one resident GPU learner at a time while storing logical population
members as complete immutable candidate checkpoints. Equal islands preserve their
own champions and replace one weak slot each cycle. Cross-island migration is
scheduled separately; protected development archive parents can also participate.
Every new child receives a complete training block before selection.

The offspring mixture is 50% local, 25% medium, 15% large, 5% head reset and 5%
fresh representation. Local perturbations affect action heads, conditioners and
the target query; broader perturbations include the recurrent representation.
Noise operators preserve critic parameters. Reset operators explicitly reset the
relevant modules. Noise scales are searched against training-only behavior:
family/conditional-target categorical KL targets are .001, .01 and .1. The report
states whether the target band was reached; a closest trial is not relabeled as
an exact success. Raw quantity, price and time change is also reported. This is
not full joint-action KL or a measured executed-order distance.

Each parent and child independently recomputes recurrent state through the same
pinned causal training probe. Probe tensors, original dataset identity, calendar,
tensor digest and serialized file digest are retained. Optimizer moments are
reset after mutations and child learning rates vary. No prior on-policy
trajectory is trained against changed actor parameters.

The protected archive records action-frequency niches and worst development-case
scores. Negative or flat stepping stones remain explicitly research-only.
Positive repeated evidence requires positive scores without stale marks in at
least two nonoverlapping calendars under both nominal and adverse execution.
Even those entries are not deployment or independent-test qualifications.
Development selection can use multiple chronological blocks. Full daily-growth
evaluation keeps a continuous account and cannot use this block-reset shortcut.

## Current GPU run

Phase 011 uses the completed 45-minute-fit/20-minute-selection three-venue tape,
58,106,855 parameters, eight logical learners in four islands, three verified
phase-008 parents and five fresh founders. There are 16 counterfactual worlds per
learner, 16 decisions per compute chunk, 15-minute training episodes, and two
ten-minute development selection blocks under both execution profiles.
There is no reduction in the legal order/action universe or virtual API limits.
Sixteen cycles are configured; this is not a report of sixteen completed cycles.
The larger proposed 32-member logical population is supported but has not been
run on this raw-action profile.

The transition preserves two failed runs. Phase 009 rejected cross-dataset
parent weights. Explicit development transfer now verifies the original parent
experiment bytes and dataset hash, development-only status and prior train/search
calendars; these exposures are registered again before use. New scores are always
collected on the new dataset, and optimizer moments start fresh. This cannot
qualify a full-history experiment or make old test data independent again.

Phase 010 then failed with an actual CUDA out-of-memory error after 63,964
decisions and 64 accepted updates. Its process exited and failure remains
recorded. Phase 011 reduces compute batching from 32 worlds/32 decisions to
16/16, while recurrent state and financial worlds continue across chunks.
Early measured tensor allocation is about 2.4 GB; this is not a long-run peak
guarantee or a GPU-utilization benchmark.

Every 64 accepted updates, two rotating recovery slots save current weights and
optimizer state with exact hashes. These bounded, replaceable recovery copies
are distinct from immutable completed candidates. They neither restore the
in-progress simulated financial world nor count as evaluated profitable models.
Original production accounting and historical archives are untouched.

## Evidence and remaining work

Seventy-nine focused CPU checks pass, including two complete miniature PPO/search
cycles, island champion preservation, fresh child rollouts, reset optimizer
moments, categorical chain-rule agreement, reproducible operators, rollback on
failed measurement, archive qualification and hashed recovery-slot rotation.
These small checks establish wiring and accounting invariants, not performance
of a 58M-parameter search or market profitability.

The fit-only information audit finds 4,014 SIG book records, but only 64 best-price
pair changes across 11 of 32 contracts. All 32 have at least one two-sided book.
Repeated packets are not independent information. See
[fit information audit](expanded-fit-information-content-001.json).

Full competition three-venue history, the requested multi-day chronological
experiment, authoritative relationship/ALL collateral parity, market lifecycle
replay, calibrated passive execution and independently demonstrated positive ROI
remain incomplete. No model from these runs has production trading authority.
