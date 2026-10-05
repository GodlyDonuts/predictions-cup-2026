# Quote outcomes and funded inventory, October 5

The passive hypothesis still loses in the supported development sample. Learning only from subsequent prices obscures the important selection: a quote is more likely to fill when its inventory becomes less valuable. This change freezes twenty book/flow features at quote placement, predicts arrival and value conditional on filling separately, and runs decisions through one funded wallet.

The original eighteen immutable captures produced26,143 placements and26,024 outcomes in45.08s. Censored outcomes are excluded from model targets rather than assigned zero profit. Training uses labels resolved before12:51UTC; validation starts12:51 and resolves before14:37; the later development comparison starts14:37. Both model components use their own training-only normalization. Six fixed ridge penalties are selected on validation error; thresholds are fixed at0.1 expected tick and1% predicted fill probability. These are research estimates, not calibrated risk bounds.

| Quote placement | Later filled outcomes | Paired outcomes | Conditional net ticks with adverse exit tick | With exact later exit price |
|---|---:|---:|---:|---:|
| Join best prices,3s latency |35|1|−68|−34|
| Improve each side one tick,3s latency |9|0|−17|−8|

These overlapping outcomes are not portfolio returns. Removing the exit stress changes only supported one-sided labels; missing exits and censored paths remain unknown. Exact-price exit sensitivity still assumes a counterfactual fill at observed later depth, and provides no guaranteed limit execution. Both selectors choose zero later opportunities under both cost cases. Avoiding trades has not established profitability.

The funded replay compares controls and selectors on the later development period and three additional sealed sessions, totaling eight separate alternatives in135.80s. One wallet reserves both quote legs, uses exact one-share cash/basis accounting, limits committed capital to5, cumulative purchases to25 and simultaneous markets to3, and retains unknown reservations after capture interruptions. It never combines alternatives or latency copies into profit.

The inside-price development control made four counterfactual purchases, closed three outcomes for−0.040, and retained0.925 known inventory cost plus2.060 unresolved reservations. Its total economic result is incomplete. Join-price controls and subsequent inside-price control had no known fills but retained2.985 unresolved reservations; they are incomplete observations, not completed zero-profit runs. Selected wallets made no purchases and realized zero. The additional sessions are now examined data; further tuning on them cannot be reported as untouched validation.

Twenty-eight targeted correctness checks passed, including actual feed→wallet paired payout and gap liability handling. Sixty-four Python/JavaScript predictions match within1e−10 across both frozen cost cases. Four split/normalization audits verified actual purged labels. Models, source hashes and compact replay receipts are saved; large capture-derived datasets remain local. No API requests, contest orders, live-bank replacements or activation renewal resulted from these experiments.

Code: [quote economics](../live-paper/realtime-spread-economics.mjs), [funded wallet](../live-paper/funded-quote-replay.mjs), [trainer](../live-paper/train-quote-outcomes.py), [selector](../live-paper/quote-outcome-forecast.mjs). Evidence: [dataset summary](../data/realtime-quote-dataset-summary-20261005.json), [funded replay](../data/funded-quote-replay-summary-20261005.json), [parity](../data/quote-outcome-parity-20261005.json), [live observation](../data/quote-selector-runtime-20261005.json).

The next economic work is broader primary-source settlement forecasts and complete quote-window tape evidence. Repeating the zero-signal fits or scaling the losing controls is not progress toward profit. The original pilot remains expired; existing attributed holdings and frozen paper histories remain intact.
