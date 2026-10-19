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

45 focused action/economic/replay/archive checks pass. The first completed GPU
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
