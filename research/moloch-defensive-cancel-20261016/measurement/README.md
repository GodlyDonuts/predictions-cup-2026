# Actual production feed-delay observation

The original 30-second metadata probe ran against sole SIG525450's genuine
breadth journal after startup, using the same reviewed observer4e and unchanged
probe as the previous measurement. It made zero account requests. Root retained
the [original compressed rows](actual-age-probe.jsonl.gz), reproduced every
statistic locally and pinned the raw bytes in
[fresh-artifact-pins.json](../deployment/fresh-artifact-pins.json).

| Observation | Prior2225 worker | Currentb112 worker |
| --- | ---: | ---: |
| Books / shared frames / exchanges | 779 /17 /214 | 869 /26 /219 |
| Receipt-to-probe median | 2069ms | 838ms |
| Receipt-to-probe p90 | 7923ms | 6064ms |
| Original source age median | 4157ms | 1747ms |
| Source-to-receipt median | 1715ms | 673ms |
| Same sequence with newer original timestamp | 0 | 0 |

These are different natural traffic windows. Book observations share frames;
they are not independent trials. Receipt-to-probe delay includes producer work,
append, filesystem/tail scheduling, local-cache HTTP and probe processing. No
append clock isolates these components, and changed network delay contributes
to source age. This is not a causal worker/CPU/whole-cycle benchmark, persistent
uptime proof, predictive-model qualification or profit attribution. Separate
quantiles must not be added together.

The [fixed reproducer](summarize-age.py) changes only the pinned capture hashes
and worker attribution from the prior algorithm. Original clocks, ages and
calculation rules are unchanged. Run:

```text
python summarize-age.py --input actual-age-probe.jsonl.gz --output reproduced.json
```

[Full report](age-report.json) / [bounded comparison](age-comparison.json).
