# Replay book reuse and measured source throughput — October21 05:45UTC

The objective remains incomplete. This goal turn made concrete progress: profiled the actual live learner, implemented immutable source-book reuse, passed56 checks and completed two actual-source comparisons. No production financial state, protected test prices, active033 configuration or queued034 source was changed.

## What the profile actually supports

The20-second live sample contains399 observations, zero sampling errors. All sampled stacks are in source-clock advancement;132 include Book materialization,109 include JSON decode and38 include Book validation. Counts overlap and cannot be added. This is a short event-heavy sample, not a measured GPU utilization fraction for the whole training job. [Exact profile](population-033-runtime-profile-20261021.json).

The current batched trainer still advances independent financial worlds sequentially on the host. A long model-chosen alarm can hold a batch inside one world's replay while other worlds and the GPU wait. This is confirmed by the current source loop and a fresh actual7108 stack inside advance_alarm. Observation timeout is not process failure:7108 is confirmed live; its latest completed logged chunk has631 optimizer updates/24731 decisions, with three of eight member1 worlds complete. No restart was issued.

## Change and evidence

Each verified SQLite store now weakly shares a frozen Book only for the same original record and exact representation clock. World and information clocks stay distinct when their original times differ. Existing bounded view caches and active worlds retain their normal ownership; the weak pool adds no strong lifetime. Liquidity depletion, invalidation, orders, wallets, API budgets, actor permissions and cursors remain separate. Five new tests exercise identity, clock differences, cross-store isolation, mutable economic state and weak reclamation. The broader56 passing checks include eager-versus-disk observation, action, paid-history and finance comparisons. [Focused checks](immutable-book-sharing-focused-checks-20261021.json).

On10000 actual retained book rows, paired world/information construction is1.78x faster in the repeated warm experiment. This is construction-only, not training speed. The first broader source probe had a cold11.29s baseline and later4.79s baseline; its raw1.83x aggregate ratio is contaminated by warm-up and must not be advertised as the replay gain. [Original immutable measurement](immutable-book-sharing-source-probe-001-20261021.json).

The repeated experiment excludes its first full source pass as warm-up, then balances three baseline and three reuse passes. Median actual source advancement improves from4.9576s to4.6410s: **1.0682x, or6.8% faster**. Every case preserves the exact observation digest, financial digest,995 paid SIG reads,11971 Kalshi reads,17605 Polymarket reads,14197 book deliveries and272 original public trades over the same retained ten-minute window. No model inference, orders or optimizer ran in this data-only measurement. [Warm repeated result](immutable-book-sharing-warm-probe-002-20261021.json).

## Remaining work

This is bounded immutable reuse, not the full proposed predecode-once replay architecture. The larger host bottleneck remains sequential world advancement. A future parallel source-world implementation must preserve each world's clocks, wallets, receipts, budgets and on-policy update boundary, and demonstrate equivalent trajectories before a training handoff. It must not share account clients or change production. Current033 continues frozen; corrected034 still waits for its complete generation on sourceeb437d5c, including audited parsing and mandatory V3 baseline refetch. This optional optimization did not cancel or requeue that pending handoff.

Latest completed same-CPU570-update development outcomes remain -2.350% nominal/+.124% stressed marked ROI; both human daily rewards are negative. Full competition-start books, the requested complete chronology, valuation/execution qualification and independently positive ROI remain unproven. The independent October21–25 interval remains unused for gradients, model selection and these measurements.
