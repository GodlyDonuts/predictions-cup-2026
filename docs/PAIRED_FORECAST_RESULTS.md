# Pooled liquidation forecast — October 5

The generic forecaster predicts changes in future two-leg NO liquidation quotes. It shrinks toward unchanged liquidation, then subtracts current acquisition cost. Features are discount to one, quoted roundtrip cost, a strictly earlier five-minute change and a missing-history indicator. No state, market identity or winner is a predictor. Three horizons (5 minutes,30 minutes,2 hours), ridge penalty1000 and one-cent minimum expected net were fixed before fitting.

The original sealed source contains75.863 hours.297,598 causal minute observations across117 discovered pairs were processed in4.832 seconds. The training cutoff isOctober4 16:23:21.617UTC, at60% of the source clock interval. Training uses only labels already observable by that cutoff. Feature normalization and coefficients remain frozen on later rows. Labels require both quote requests after the target horizon, at most60 seconds late; gaps are censored.

| Horizon | Training labels | Later labels | Forecast RMSE | Unchanged liquidation RMSE | Selected quote signals |
| --- | ---: | ---: | ---: | ---: | ---: |
| 5 minutes |136,833|90,829|0.003179|0.003263|0|
|30 minutes|133,894|87,328|0.005777|0.005886|0|
|2 hours|112,205|71,853|0.009362|0.009466|0|

Prediction error improved slightly, but the modeled movement did not cover acquisition/liquidation costs plus the declared margin. Improved RMSE is not profit. Bulk quotes lack depth, capital-constrained fills, order acceptance and failed-hedge outcomes. Overlapping labels and related elections are correlated; UTC-day breakdowns are retained without independent-sample confidence claims. The later segment is held out from fitting this model, but has been examined by other development diagnostics. It is not untouched by all research, and no production winner was selected.

The next economic direction is source-backed value and appropriate holding horizons. A later Kansas Senate primary-source audit adds co/efficient's Republican+5 and Siena's45–45 tie to the old Emerson Democratic+2 result. Equal-source margin sensitivity is−1 point, retaining the original single-poll uncertainty rather than inventing independent-poll precision. The original model's ordinary transport range falls from0.600–0.643 to0.405–0.446; adverse three-point shift yields0.246. This is an unvalidated transport sensitivity, not a confidence bound or near-term trading forecast. The linked Siena detailed fieldwork packet is unavailable; no new-entry permission or existing-source clock renewal follows. Original Kansas wallet and research remain frozen and attributed.

Primary evidence: [Emerson](https://emersoncollegepolling.com/kansas-2026/), [co/efficient](https://battlegrounds.coefficient.org/ks/), [Siena October3 summary](https://sri.siena.edu/2026/10/03/nytimes-siena-poll-of-likely-voters-senate-races/). Exact public source hashes/observation clocks are retained in data/kansas-poll-audit-sources-20261005-v2.json; source bodies remain restricted runtime files. No account client or order was created for this research.

Run the four causal/gap/shrinkage checks with `node --test research/paired-forecast-v1.test.mjs`. The replay refuses to overwrite its frozen result, preserving all three attempts rather than selecting a favorable score. See data/paired-forecast-results-20261005-v1.json for coefficients, normalization, source frontier and every score.
