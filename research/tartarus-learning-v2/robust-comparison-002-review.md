# Fixed-stress comparison: actual results

Completed October18,2026. [Machine-readable report](robust-comparison-002-result.json) / [predeclared plan and source hashes](robust-comparison-002-plan.json). The complete run took720.299 seconds. No locked or forward outcomes were used and no learned policy was deployed.

Each mode used the same four initial founders, one island, two cycles,2400 training decisions and4800 evaluation decisions. Training used nominal then adverse mechanics; every policy was evaluated on two shared calendars under both execution profiles. Selection used the weakest of those four marked-wealth returns.

| Mode | Wall seconds | Final policies positive in all four cases | Weakest return by final slot |
|---|---:|---:|---|
| Independent recurrent PPO |240.163|1/4|+0.0134385%,0%,-1.466807%,0%|
| Local-only replacement |245.899|0/4|0%,0%,-1.466807%,0%|
| Multiscale mixture |234.226|0/4|0%,0%,-1.466807%,0%|

Scheduled work was matched; wall time, mutation/probe overhead and exact optimization work were not forced equal. All modes accepted sixteen optimizer steps in total. The recorded cumulative checkpoint counters are separately labeled in the report.

There was only one replacement in each population mode. The multiscale random draw chose **local**, producing the same child and final parameters as the local-only arm. This trial therefore **does not isolate any benefit or harm from medium/large/reset/fresh operators**. A single seed family and two reused development calendars do not establish statistical superiority of independent PPO. The current production strategy was not replayed, so this also does not establish improvement over production.

The replaced founder was weak after its first block but became the one positive-across-all-cases policy when retained in independent training. This is evidence for investigating early replacement, not proof of an optimal grace period. Subsequent new experiments give original founders two full training blocks before replacement; fresh/reset child grace already existed. The completed comparison remains unchanged, with its original one-block founder grace. New comparison defaults use three cycles so the longer grace still permits an actual replacement.

These four-member trials use the explicit minimum two-member founder allowance, an effective50% cap. The full32-member run uses25%. New manifests record the effective cap rather than labeling every population size25%.

## Inventory-bearing curriculum

The first [inventory-bearing run](ppo-inventory-001-result.json) completed two300-decision episodes and a real CUDA checkpoint in51.722 seconds. A synthetic95k starting bank was allocated using only books and reference marks available at episode start. Initial FIFO costs and fees are funded exactly. These stipulated starting lots are not historical fills, official holdings or earned profit; the initial mark-versus-basis difference is excluded from the return denominator and reward. Matched hold baselines returned zero.

The two changing training policies returned+0.128520% and+0.034977% in conditional marked wealth, including actual modeled SELL and EXIT actions. Both terminal accounts reconciled. This is training evidence, not a fixed-policy comparison, realized liquidation or actual SIG profit.

The shuffled starter invested only29,062.94 and29,729.70 before consuming the twelve market slots. It therefore did not achieve its intended nearly-funded condition. The following declared branch ranks initial candidates by currently recorded depth capacity, with seeded sides/ties and unchanged total/market/fresh-market limits. It never selects by future prices or increases recorded depth. Achieved funding must be measured again. The original shuffled protocol, results and source commit remain retained.

## Still incomplete

Unified three-venue compilation and semantic qualification, actual-shaped inherited inventory, broader execution calibration, full joint/executed mutation distance, archive reintroduction, complete A0-A6 comparisons, independent seeds, untouched locked/forward evidence and a general learned-policy production adapter remain unfinished. The original23:15 finalist deadline failed; the later positive development policy is not retroactively admitted to that forward cohort.

The [current-depth-capacity trial](ppo-inventory-002-result.json) achieved94,809.97 and94,809.965 cost from the95k starting bank, leaving190.03 and190.035 cash. Both modeled terminal accounts reconciled, but the two changing training policies returned **-0.747768% and-0.152312%**. The original positive shuffled-inventory results therefore did not establish strong behavior with almost all capital deployed. These losses remain retained; no model is deployed. The following finite eight-policy/two-island/four-cycle development run uses the nearly-funded search protocol, fixed nominal/adverse profiles and two-block founder grace. It is further training, not a controlled causal attribution of the initializer or grace changes.
