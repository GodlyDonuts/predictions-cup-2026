# Inherited timing diversity — October 20

GPU024's inherited founder retains the old millisecond WAIT prior. It executes many decisions with little virtual progress. The fresh founder, with a longer initial WAIT preference, completed eight ten-minute training paths much sooner. This is a throughput observation, not a demonstrated profitability advantage.

`inherited_wait_logit_shifts` now permits explicit per-founder WAIT-logit mutations in development transfer experiments. Entries are null (unchanged) or two finite logit deltas. Fresh founders must have null entries. The operation is recorded separately for every changed founder and uses a fresh optimizer and new trajectories/scores. The active GPU024 source and weights are unchanged.

The mutation touches only the WAIT family embedding, projecting its change through the existing fraction head. At a fixed recurrent/market context, quantity logits and all non-WAIT operation parameters remain unchanged within the checked numeric tolerance; all non-WAIT weights remain bitwise unchanged. No deadline, action, order-size or decision-frequency limit is introduced. Future recurrent paths can differ because the actor chooses different timing.

Eight CPU tests pass, including existing population evolution coverage, exact zero-mutation identity, invalid-input preservation and fixed-context parameter isolation. A CPU probe of the actual inherited founder found zero-context median alarms of 12.67ms unchanged, 32.11ms for alpha-logit shift +2.2, and 920.80ms for +4.4. These are distribution probes, not measured deployment latency or returns. See inherited-clock-mutation-probe-20261020.json.

Next experiments should retain an unchanged control and vary timing alongside information acquisition. The completed fill-impact control shows the ordinary losing candidate never had a delivered SIG book at any fill; reducing unnecessary inference alone cannot solve that economic failure. No independent-test data or production financial state was touched.
