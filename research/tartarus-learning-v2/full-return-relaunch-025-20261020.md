# GPU025: full-episode economic credit and recovered SIG history

Observed October 20, 2026, 20:18 UTC. This is development work, not profitable production qualification.

## Why training changed

Both CPU controls for GPU024 member004 completed and exactly matched their instrumented counterparts. Ordinary marked loss14972.9675 = fill-time impact -15835.83 + non-fill wealth movement862.8625. Stress marked loss25669.36 = fill-time impact -26063.755 + non-fill wealth movement394.395. Every fill lacked a delivered SIG book. These are sampled CPU development trajectories, not replacements for the original CUDA evaluation. The immediate failure is costly uninformed execution.

GPU024 used sixteen-decision bootstrapped return chunks. Its inherited actor repeatedly chose millisecond WAITs, making those credit windows short in market time. GPU025 uses the already-tested closed-episode mode: collect complete ten-minute training paths with fixed behavior weights, seal exact returns including terminal corrections, then optimize bounded replay minibatches. Earlier requests can therefore receive credit from later economic outcomes. This is a change in credit assignment, not a claim that information acquisition or profit has already been learned.

## Actual launch

- Source: `5ddda71d566f0a6cf77b536ec9b6782f8712221e`.
- Configuration SHA256: `b4c0e1587d10a2c4f0354f5b2252b7328c14f9ac1fc9073108dff85f4d84e8ed`.
- Start20:16:13UTC; actual GPU learner16052, wrapper16476, supervisor22076.
- Four candidates, two research islands, eight parallel worlds, two inherited parents and two fresh founders. Sixteen population cycles remain configured. Every candidate still completes its full registered episode and selection windows.
- Parent0 checkpoint `16a0abac918f81f6d368ed92c365e50ab11679a3f9b447fdee844660e1b042be`: preserved GPU024 recovery weights, explicit WAIT alpha-logit shift+4.4.
- Parent1 checkpoint `b558c33b0c59dd91f2724faed1d73c1a9a12045ac5e6d14dad12747250006157`: completed member004, weights preserved exactly; this parent is known to lose money.
- Fresh WAIT prior remains[1.4,9]. Initial entropy coefficients alternate0/.001. All legal actions and deadlines remain available. No artificial decision cap was added.
- CPU founder proof confirms finite weights, fresh optimizers, exact parent1 preservation and only WAIT embedding changes for parent0. Eight timing/population tests and nine episode/capacity tests pass with CUDA disabled.
- GPU024 intentionally stopped20:16:02UTC after copying verified parents. Its original files and final rotating recovery checkpoints remain. Its unfinished world was not resumed or counted as complete. There was no second concurrent GPU learner.
- Fresh log verifies1,920 rollout decisions and zero optimizer updates at20:17:41UTC. Zero updates at this stage is expected: the cohort must finish before full-return optimization. No completed GPU025 result exists yet.

## Data recovery and capture

The independent historical audit passed all237 SIG scopes:1,346,717 public trades in6,852 validated pages (156,099,349 bytes). Source request bounds September1 through October16,05:00UTC; observed earliest fact is October1. Actual competition-start coverage and historical catalog availability remain unverified. Audit SHA256 `dbda64ffd75e88ffe1abb1ca64c6a46c51f49da5cad0a87bba4de1c97698e16c`. See sig-trades-complete-audit-20261020.json. The causal SQLite index is building; GPU025 does not yet include it.

The original account queue handed over to prospective capture at20:15:54UTC, reader950785, controller949555. At20:17:08UTC the authenticated stream was subscribed (handshake101), with1,798 pushed books,29 trades, zero reported gaps/schema errors and measured mean trade delivery latency1673.59ms. This short observation does not establish continuous tape coverage. Contest/paper orders both zero; EXECUTION_STOP SHA remains `1b1b24f1379d9e3a11c13991afd588d085dc16d99c392eda2a3920769c0b2043`.

Full multi-day realistic historical execution remains incomplete: public candles/trades cannot recreate missing historical depth, queue or lifecycle facts. Current GPU025 still uses the exposed October19 development calendar and its partial-history reward; it is not the requested full daily-growth experiment or independent test. October21–25 remains reserved and unconsumed. No production financial state changed.
