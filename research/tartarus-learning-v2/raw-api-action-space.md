# Expanded training action space — October 19, 2026

New Tartarus experiments use
[current-training-action-profile.json](current-training-action-profile.json)
and `moloch.learning_actions.entry.open_experiment`. This selects a new raw API
policy and event-driven collector. Evolution/test entry requires the actual candidate checkpoint and verifies its
SHA256; it cannot evaluate a newly initialized actor under an unrelated finalist
hash. The old fixed-step, target-fraction learners
and their checkpoints remain historical baselines; their results are not
retroactively attributed to this new action space.

## Restrictions removed

| Previous restriction | New training path |
|---|---|
| One decision per second | Event-driven decisions, interrupted by observable receipts or chosen alarms |
| A finite episode ending after a compute batch | Chunks preserve time, inventory, orders, builder and recurrent memory |
| Five offsets around the current quote | All 199 valid SIG limit-price ticks, plus market orders |
| Three patience choices | No expiry, or a chosen future representable UTC timestamp |
| Portfolio-fraction targets | Raw positive integer share quantity through 2,147,483,647 |
| 10,000-share/per-market/portfolio/fresh-market strategy caps | No such training caps; actual funds, collateral and API eligibility apply |
| One individual order as the only placement | Individual orders, up to 50 batch items, up to 10 atomic legs, all-or-none baskets |
| Cancel only an automatically selected order | Choose any known own order, or cancel an exchange, market or entire scope |
| Compulsory round-robin reads | Policy chooses SIG, Kalshi and Polymarket read targets |
| Missing reference marks prevent submitting orders | Raw orders do not require an external valuation/alpha signal |
| Fixed number of market/own-order slots | Dynamic entity tokens include every supplied contract, own order and read target |

The policy has 58,106,855 parameters at the default 4,096-unit recurrent size.
Market prices, side, raw quantity, expiry and observation requests are decisions
it can learn. It can assemble a basket through planning tokens, then submit it;
planning tokens do not themselves place orders or consume exchange writes.

Raw quantities and times use float64 latent samples followed by integer
conversion over their complete supported domains, with logarithmic spacing.
This preserves positive probability for every legal integer and avoids both a
small preset quantity menu and float32's inability to address every 31-bit integer.
Latent quantity is not a target percentage or maximum exposure allocation.

## Real constraints and API evidence

The current official [SIG API reference](https://sig.thesuper.market/api/v1/docs)
was fetched through the CLI on October 19. Its embedded OpenAPI snapshot hashes
to `a7d5b76ce9c79afdb8ad327bd606d0b8d1287904dd690dd1a0a1009c02a5092d`
after the recorded JSON extraction. Participant placement endpoints include
single, batch and multi-leg orders; the documented standard account budget is
100 reads and 30 writes per minute. A batch or multi-leg HTTP request counts as
one API write. There can additionally be engine-side throttling; historical
calibration of that behavior remains incomplete.

The **new offline profile** models those standard API ceilings without the old
600/2500 ms completion gaps or the compulsory sixteen-read preflight. Requests
still take modeled network/engine/receipt time and share one serialized SIG
budget. The existing official production worker remains on its original
100-read/24-write policy and completion pacing; this task does not activate a
new worker or alter its controls.

The new event stream accounts for measured actor compute cost rather than a
one-second decision grid. The GPU smoke measured local actor inference at
roughly 7–9 ms and froze a 9-millisecond p95 estimate. This is a modeling
assumption for the measured local shape, not an exchange limit or a guaranteed
remote production latency. Zero-time infinite reasoning is not treated as free
market time. Replay computation can still run faster than virtual time.

SELL can open the complementary position when it is not a fully covered closing
sale. Opposite BUY fills net FIFO inventory and credit complementary payout.
Resting buys on other exchanges do not escrow cash. Market orders do not rest;
explicit server expiration does not manufacture a charged cancel. Ordinary batch
items are independent; multi-leg failure restores cash, inventory and depleted
liquidity; all-or-none also requires the complete requested quantity of each leg.
Idempotent placement responses are retained separately from later mutable orders.

ALL buying power is automatic in the API, not a separate free-money borrow call.
The simulator exposes it through a supplied **verified guaranteed-payout oracle**.
Without authoritative graph/collateral inputs it does not invent an advance from
Kalshi marks, statistical scenarios or empty inventory. Optional relationship
constraints are exposed when authoritative relationship data and a validator
are supplied. Missing evidence remains a declared data gap, not an implicit claim
that the old cash-only strategy was optimal.

## Clock, learning and independent testing

The existing frozen split checks still govern experiment opening: train before
cutoff minus eight days, evolution in the next four, independent test in the
last four with a frozen finalist. `open_experiment` refuses a failed full-history
specification rather than falling back to inspected minutes.

`stream.Stream.run_chunk` yields bounded work without ending an episode or
resetting its wallet or memory. A test executes 1,050 decisions across chunks
and verifies the same continuing episode. `training.collect_chunk` retains its
initial recurrent state; `training.update` teacher-forces the actual actions,
clips PPO changes, checks KL and restores rejected optimizer changes. It refuses
evolution/test optimization, stale behavior or absent economic reward labels.
There is no fallback to the old interval-HOLD reward. The complete multi-day
reward coordinator still needs qualified daily wealth and independent-event
accounting; the existence of a callable optimizer does not complete that work.

## Verification and remaining work

The expanded suite passes **75 tests**, including 20 new action tests. They
exercise all limit ticks and API quantity boundaries; raw orders above the old
10,000-share ceiling; manual three-venue observations; complement sales; delayed
cash visibility; arbitrary expiration; market orders; partial-funding rejection
of atomic baskets; rollback of depleted depth; idempotency; the 30-write rolling
budget; unknown-acceptance preservation; collateral funding boundaries; stale
receipt rejection; continuing chunks; dynamic policy inference; and a synthetic
optimizer qualification with explicitly supplied reward labels.

[Real-data GPU smoke](raw-api-action-smoke-001.json) uses previously inspected
history, a newly initialized 58.1M-parameter actor and 128 neural decisions over
approximately 3.39 simulated seconds. Collection took 1.289 wall seconds. There
were one simulated SIG read and three simulated writes, no modeled fill and no
HTTP. The episode continued rather than ending at the chunk boundary. No learned
skill, profit or independent evaluation is claimed.

**Full production API replay parity is not established.** In particular,
historical realtime delivery/reconnect/resync, all metadata/history endpoints,
automatic collateral calibration and the relationship-preflight race require
additional authoritative data and validation. Market lifecycle, quote continuity,
fees, engine admission/quote resizing and passive fills also need calibration.
The new engine currently assumes recorded quotes remain until an explicit gap
or replacement; removing the old ten-second strategy timeout does not prove that
all historical intervals have complete coverage. Such assumptions cannot be
used as evidence of profitable independent performance.

Existing narrow-action checkpoints require retraining. Their old likelihoods
and optimizer states cannot be loaded into this different architecture and
called unrestricted. The previous 9.51x benchmark applies to its original
collector; it has not been re-established for this new event-driven policy.
