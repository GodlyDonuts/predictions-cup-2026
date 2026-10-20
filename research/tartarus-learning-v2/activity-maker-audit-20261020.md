# Activity-aware maker TRAIN experiment — October 20

The public-trade observation path improves concurrent quote exposure, but this finite baseline has not demonstrated an edge. All eight scenarios lost marked wealth. It is not a neural checkpoint, expert demonstration, independent test or production strategy.

The policy ranks markets using only received public sixty-second volume with five-minute local score decay, reads paid account/positions/books, keeps old quotes during reads, then cancels and submits up to fifty paired legs across twenty-five markets. It waits thirty seconds after processing and receipt. Prices improve one tick when the spread permits; expiry is sixty seconds from planning. The model action domain, simulator execution rules and API budgets are unchanged.

Two already-inspected fifteen-minute TRAIN calendars were repeated at quantities100/1000 under nominal and half-depth/0.005 adverse stress. These are two calendars, not eight independent days. Source157c873a and dataset7a85747f are frozen; the baseline retains24ms planning cost. Terminal cleanup attempts liquidation, but unavailable depth can leave inventory. Marked ROI uses last observed SIG midpoint, not executable liquidation or official tournament value.

| Quantity | TRAIN start ms | Stress | Fills | Marked ROI % | Realized FIFO SUSQies | Remaining shares |
| ---: | ---: | :---: | ---: | ---: | ---: | ---: |
|100|1792410601000|False|1|-0.0002500|0.000000|100|
|100|1792410601000|True|1|-0.0002500|0.000000|100|
|100|1792412401000|False|3|-0.0010000|-1.000000|0|
|100|1792412401000|True|2|-0.0010000|-1.000000|0|
|1000|1792410601000|False|3|-0.0019000|0.000000|760|
|1000|1792410601000|True|1|-0.0004750|0.000000|190|
|1000|1792412401000|False|3|-0.0070900|-7.090000|0|
|1000|1792412401000|True|2|-0.0015000|-1.500000|0|

The first calendar retained100shares at quantity100, and760/190shares at quantity1000. Cash declines in those cases include inventory acquisition and are not realized loss. The second calendar was fully flat: losses were1SUSQie in both quantity100 scenarios and7.09/1.50SUSQies at quantity1000. Scaling capital did not create positive expectancy.

Concurrent exposure improves substantially versus the prior corrected uniform scan: in the first calendar104of109prints occur during some live quote and34occur with own quotes in the same exchange. This is causal activity targeting, not a guaranteed fill or profitable execution signal. Queue assumptions remain uncalibrated.

The first001reporter failed after a case because it requested nonexistent fill field `origin`;002uses `execution_model`. The failed output is preserved privately and not credited. The strategy was not changed to repair reporting. No HTTP requests, optimizer steps, neural adoption, GPU diagnostics or production changes occurred.

Exact runner SHA256: `e82a6538674508fc73aca6560d4e6625a620a04dcf01493496947c53e6205b10`. [Runner](activity-maker-train-probe-002-runner.py.gz), [quantity100](activity-maker-train-probe-002-q100.json), [quantity1000](activity-maker-train-probe-002-q1000.json). Gzip preserves exact bytes; the frozen runtime source pins are included in each report.

Next: allow the current neural cohort to complete before judging its accepted updates and subsequent selection. Do not inject this losing baseline as an expert or use these inspected calendars as independent qualification. Full historical coverage and untouched multi-day validation remain outstanding.
