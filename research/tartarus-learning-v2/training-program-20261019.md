October19 ML update: phase014 is training the eight preserved lineages on237 SIG exchanges after a verified intermediate-valuation evaluation failure in013.149 focused checks pass; required daily/terminal marks remain strict. Official-mark calibration covers25 exchanges; no independent profitable ROI is established. [Active profile, failure and evidence](profile-014-learning-20261019.md).

Observed-world update: an opt-in dual-clock replay now separates original snapshot receipt from recorded information delivery, with strict role fences and stale-message protection.115 checks and a real-source CPU wiring check pass. The frozen full-size candidate comparison completed with approximately -3.45% worst marked ROI in both clock profiles. [Implementation and evidence](observed-snapshot-world-20261019.md).

Population update: phase011 is running on the expanded tape with eight logical learners/four islands, measured multiscale mutations, five fresh founders and protected development stepping stones. Two earlier failed phases remain recorded. Bounded training recovery and lower compute batching address the verified GPU OOM. See [raw population search](raw-population-search-20261019.md). No independent profitable ROI is established.

Expanded replay update: a verified 45-minute fit / 20-minute selection cohort now includes 32 SIG exchanges and 107 official engine-match prints, with mapped Kalshi/Polymarket references. The next population phase is queued at the completed-cycle boundary. This remains development evidence, not the requested multi-day test or demonstrated profit. See [history expansion](history-expansion-20261019.md).

# Raw-action population training — October 19, 2026

The new 58.1M-parameter raw-action actor now has a runnable GPU population
trainer, exact daily economic reward coordinator, immutable checkpoints,
nominal/adverse selection and persistent one-shot independent-test access.
This is implemented research infrastructure, not a profitable production model.

## Action freedom and learning

The actor chooses the operation first, then an eligible target. Previously,
hundreds of external read targets implicitly crowded out trading through their
number alone. All valid operations, targets, prices, quantities and expirations
remain available. A short-alarm initialization prior changes exploration
probabilities without removing any supported alarm. No old alpha filter or
strategy exposure cap is added.

Variable entity counts are padded without truncation. A fused recurrent PPO
sequence preserves the initial hidden state and reset boundaries, and is checked
against teacher-forced step-by-step inference. Warm-up builds the initial exchange
world only from earlier observations inside the same calendar role. Observing that
world still requires the actor's delayed API reads. Compute chunks preserve the
ongoing account, orders, virtual time and recurrent state.

## Economic objective

`GrowthLedger` tracks a continuous wallet through complete 24-hour blocks. The
full mode requires at least two days because the human's formula uses sample
standard deviation. Consecutive wealth samples contribute log growth; an exact
terminal correction makes their sum equal the supplied growth/consistency/evidence
score. It does not invent a positive label for fills or API acknowledgments.

Cash changes, fees and complementary payout netting are attributed exactly to
fills. Twice-micro integer valuation preserves half-micro midpoints. All contracts
share one conservative correlated opportunity cluster unless a finer grouping has
pinned economic evidence, preventing fill splitting from inflating N.

The current explicit valuation convention is original causal SIG midpoint marks.
Those marks are not full-depth sale proceeds, dashboard parity or settlement
payouts. Strict full mode refuses unavailable/stale position marks. A separately
opted-in development convention carries the last actually observed SIG midpoint
through data gaps, reports stale exchanges/counts, and cannot qualify a full
experiment. It never fabricates a price, substitutes cost basis or changes an
execution book's gap status.

Short inspected development runs use log marked wealth, have D/N unavailable,
and remain ineligible for the human's daily-growth qualification. They are useful
for debugging and learning experiments, not four-day evidence.

## Evolution and independent testing

`python -m moloch.learning_actions.experiment --config CONFIG.json --output NEW_RUN`
starts fresh founders, actual PPO updates, immutable model/optimizer checkpoints,
nominal plus adverse-depth/price selection, and champion-to-weakest population
mutation with reset optimizer moments. JSONL progress and failures remain local;
old checkpoint-reset baselines are not silently resumed.

Full-history mode enters through the verified historical specification. Training
is before cutoff minus eight days; evolution is the next four; testing is the
last four. Source intervals are half open, so the next role's first observation
cannot affect the preceding role's terminal wealth. Full four-day evaluation
keeps inventory continuous across days.

Before test prices are opened for evaluation, the exact positively selected
checkpoint is frozen and an exclusive persistent access record is created.
A failed test still consumes access. The registry is keyed by competition and test
calendar across dataset compilations, so changing hyperparameters/source does not purchase another look.
Repeated evolution can overfit the evolution partition; repeatedly inspecting
the final four days would also turn them into selection data. New independent
confirmation therefore needs a fresh future period.

## Actual history and remaining gaps

The [compiled dataset clock inventory](compiled-history-clock-inventory-20261019.json)
streams every completed local compiled dataset without exporting prices:

| Available compiled history | Dates (UTC) | Limitation |
|---|---|---|
| Native SIG development | Oct15 08:09 to Oct17 19:59 | SIG only; gaps up to 3.20h |
| Older SIG + Kalshi | Oct16 17:38 to 18:38 | No Polymarket; relocated duplicates are not more history |
| All-three-venue inspected subset | Oct19 04:16 to 04:31 | About 15min; aggregate SIG gap up to 90s |
| All-three-venue later development | Oct19 07:40 to 08:20 | About 40min with planned calendar gaps up to 305s |

This is a complete inventory of the current **compiled** datasets, not proof that
all original local/remote archives have been exhausted. It does not establish
competition-start-through-cutoff-minus-eight-days history or four-day evolution
and independent testing. Additional archive recovery/compilation is still needed;
missing historical books cannot be reconstructed faithfully from price candles.

Full production API parity and execution calibration also remain incomplete:
authoritative ALL collateral/relationship integration, historical realtime
resync, metadata/lifecycle endpoints, engine resizing and passive queue fills.
The policy has not earned deployment from these development studies. No production
worker, live financial ledger, risk/STOP control or account client was changed.

70 focused action/economic/replay/archive checks pass. The first completed GPU
population evaluated 16 candidates, with 1,822 training decisions and 21 accepted
PPO updates. Best worst-case marked ROI was -3.7146%; none qualified. This short
study is development evidence, not the requested multi-day test.

## Preserve training data and avoid hidden scheduling constraints

Only an explicit policy WAIT suspends decisions. A busy SIG queue or exhausted
write budget leaves independent external reads and planning available on the
next measured inference tick; exchange admissions still obey their limits.

The existing Kalshi/Polymarket recorders prune rolling journals. A separate
filesystem-only archiver now preserves original byte snapshots and SHA256
manifests before pruning, defers active files, keeps changed versions and
reserves 100 GiB of free disk. The first pass preserved 1,197 files totaling
7,870,677,997 bytes. It adds zero API requests and never changes original
collector files. These byte snapshots still require transport/semantic
verification before compilation.

## Batched GPU continuation

A controlled FP32 benchmark of the new raw-action collector uses32worlds and
32decisions/world (1,024decisions). Interleaved serial/batched comparisons give
10.260s versus2.685s median collection (3.82x). Exact clocks, wallets, orders and
fills match; recurrent state matches declared numeric tolerance. One batched
PPO update takes0.825s and is accepted, peaking3.67GB allocated CUDA memory.
This is the new raw-action collector, separate from the older9.51x legacy result.
[Measured result](raw-api-batch-benchmark-003.json).

Batched learners retain separate clocks, cash, inventories, orders, rate budgets
and recurrent memory. Completed worlds are masked out of learning while others
continue. Repeated simulated worlds do not create new independent historical
evidence. Training run007 now uses32worlds per learner,4population members,
16planned cycles and five-minute development episodes. It transfers a verified
raw-action development checkpoint from run006 and resets founder optimizer
moments, preserving learning progress rather than calling old narrow policies
unrestricted. This transfer is forbidden from full-history qualification.

The [raw archive first-receipt inventory](raw-archive-first-receipt-inventory-20261019.json)
finds earliest preserved Kalshi segment receipts on Oct18 05:55UTC and Polymarket
on Oct18 22:35UTC. This samples first rows, not full interval coverage. It still
does not supply the requested competition-start/8-day/4-day history. Remote or
other original archives have not been exhausted by this local inventory.

## API realism and independent-data continuation

[Current conformance scope](api-replay-conformance-20261019.json) is bound to the
public SIG OpenAPI read through the CLI. Token minting consumes one write;
connected book deliveries do not consume REST admissions; token lifetime is3h.
The new optional derived stream replays original admitted book observations at
their recorded availability clocks. It does not fabricate topic revisions from
engine sequence numbers. Gaps, expiry, refresh/disconnect and a configured120s
reconciliation cadence invalidate trust until a charged REST baseline arrives.
Models can still choose their raw orders and disconnect/reconnect the stream.

A recorded-data functional smoke received6original book snapshots, established
an authoritative baseline and ran128neural decisions with zero HTTP calls. Its
driver advanced to a recorded event for setup; this is no policy-performance or
profit claim. [Smoke](derived-stream-smoke-002.json). Run007 remains on its original
profile; the stream adapter is opt-in for a subsequent training phase.

Mechanical masks now exclude SIG operations only while their queue/rate budget
cannot admit them, and exclude malformed empty/oversized builders. External
reads and local planning stay available. No alpha, price menu, exposure or
lifetime-decision constraint is introduced.

Clock coverage alone can no longer qualify an independent test: the frozen
producer provenance must positively designate the exact unseen test calendar.
A persistent local data-use audit rejects previously optimized or overlapping
previous-test calendars across dataset versions. Current inspected datasets and
run007's train/selection calendars are registered retrospectively; these cannot
be relabeled independent later. The audit does not certify unknown manual or
other-host use. Future full experiment identities pin the actual split and replay
specification bytes, and future runs save their measured compute calibration.

A matched CPU-FP32 development reevaluation uses the original70byte-pinned trainer
source files and the same calendar, wealth convention, initial funds, seeds and
nominal/adverse cases for parent and two trained checkpoints. A common8ms virtual
compute cost is frozen from the controlled benchmark; the original run007 cost
was not saved and is not claimed recovered. CPU evaluation reserves GPU memory
for training; backend/RNG paths differ from the earlier GPU reports.
The parent loses8.63% in its worse case; one trained checkpoint is flat with zero
fills, while the other loses2.91%. None is strictly positive. This demonstrates
loss avoidance rather than alpha. [Comparison](raw-matched-development-cpu-001.json).

The next substantive missing model is passive execution: current replay matches
crossing displayed depth, not ordinary spread-capture fills. Public engine prints
lack an established aggressor label; any next flow/queue model must keep actual
execution clocks, shared volume, queue uncertainty and original-owner exclusions
explicit. Full historical data, full API parity and independent profitable ROI
remain unproved. Primary production state is untouched.

## Passive execution continuation

Two opt-in, explicitly uncalibrated flow models now complement crossing fills:
strict price-priority interception, and displayed-queue-ahead matching. Both use
original engine-print execution clocks, never the later public receipt as a new
counterparty. They share each print's volume across own orders; prior crossing
volume is debited first. Strict improvement fills use the quote's less favorable
limit price. Equal-price queue fills require recorded volume to consume displayed
queue ahead; quantity drops give no free cancellation credit, observed additions
are conservatively ahead, and own orders remain FIFO. Unknown initial queues do
not receive an invented zero ahead. Atomic rollback restores queue/depletion state.

Original-owner match exclusions can be supplied with a source pin. Their
completeness and actual queue/market reaction remain unverified; these models
cannot certify execution parity or production profitability. Their assumptions
are frozen independently of the policy; models cannot choose a more favorable
fill rule. [Implementation](../../moloch/learning_actions/passive.py).

A genuine event-loop gap is also fixed: a hidden future fill can create a receipt
that interrupts an already waiting actor. Hidden execution alone does not wake it;
all tied events finish before exposing the observation. Previously, a newly
created receipt could be processed while the actor slept through to its original
alarm. The new private-stream profile coalesces fills for one second, adds the
modeled receipt latency, fences disconnected/expired sessions and requires REST
when no stream is active. This replaces implicit private broker delivery only in
explicitly configured new experiments. Historical run007 remains unchanged.

The causal quoting reference reads delivered books and sizes against the available
bank, never future print prices. The tight-spread improvement reference placed
no quote packets. The joining reference attempted239packets on each arm, but
neither crossing-only nor the queue-flow hypothesis filled during this selected
8min interval;4,510shares of original prints did not yield intercepted volume.
That is an honest absence of opportunity under this setup, not proof that the
whole competition has no market-making opportunity. The tape covers only its
observed contracts. [Current reference](passive-reference-study-003.json).

70focused checks cover exact fill volume/FIFO, equal-price uncertainty, late
reports, duplicate trades, crossing-volume reuse, atomic rollback, expiry,
owner-exclusion hooks, dynamic receipt wake-up, stream coalescing and remaining
same-exchange admission room. Training can preserve several distinct verified
parents and a fresh founder rather than copying one winner into every lineage.

Next: transition at a preserved checkpoint to the expanded streaming/flow profile,
then assemble substantially longer three-venue development history from retained
originals. Full eight-day chronology, calibrated complete API/ALL/lifecycle parity
and independently positive ROI remain incomplete. None of this changes official
production holdings, bank history or execution controls.

## Active phase008 and recorder continuity

[Observed active phase](active-learning-20261019.json): the expanded profile is
now actually training on the laptop GPU, with32worlds/learner, three verified
completed parent checkpoints from phase007 and one genuinely fresh founder.
It enables derived SIG delivery, displayed-queue flow and private fills that
require an active stream. Initial observed work exceeds45kdecisions/85accepted
updates; counters in the linked observation are authoritative for that time.
Phase007 was superseded from completed checkpoints; its unfinished work is not
claimed completed. No qualified positive ROI has been established.

The public Polymarket recorder reached its prior3h finite duration and was
confirmed stopped with no matching process. Its existing journal segments were
verified archived before restarting the same public recorder/cache for24h at30
HTTP admissions/s. The fresh process is verified running with378cached books
across189markets. Kalshi collection continues. This adds no SIG account client
or production writes; original recording evidence and financial state remain.
