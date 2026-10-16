# V2 + V3: pinned selection, analysis not run

The named original cohorts and unchanged analysis protocol were pinned before opening economic results. `analysis-selection.json` has raw SHA256 `a656ee74a1863ed887528f400650fe25e348ecf4bdeea2fddb9a3be1f81ed305`; the common cutoff is 2026-10-16T17:05:13.914Z. Original producer bytes, mapping, policy and each cohort's input manifest remain separately identified. No input IDs or source clocks were rewritten.

**Analysis NOT RUN.** V2 emitted two rows and V3's original seal records ten. Even the twelve-row upper bound, before label availability, event deduplication and purges, cannot meet the frozen minimum of 100 OOS observations and 100 selected observations. Replaying both large inputs again to fit the adapter would not change that count bound. This is insufficient evidence, not a negative fitted-model result: no coefficients, OOS returns, selected counts or qualification results were calculated here.

The prospective combined selection is retained for audit. The input-admitted adapter and original reader were copied unchanged into a private source-pinned scratch directory but never executed. V3 original-input membership replay is reported separately in `../../witness/actual-v3/`. No execution authority, observed-fill claim or model approval follows.
