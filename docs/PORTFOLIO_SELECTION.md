# Baal: sequential portfolio selection

## Purpose and limits

`npm run research:portfolio` builds a **new paper model bank**, selecting a model and an entry threshold by sequential simulated wallet results rather than lowest forecast RMSE. It does not activate contest execution, change capital limits, modify existing experiments, request account data, or promise a return. A high leaderboard hurdle is a reason to look for a scalable edge, not evidence that a particular strategy has one. This change evaluates strategy selection; it does not evaluate capital scaling.

The implementation reuses the existing ridge/boosted-stump fitting and delayed FIFO paper engine. It does **not** introduce a new alpha model or claim one has been discovered. Each family can produce 128 trained variants; its three fixed margins produce up to **384 independent counterfactual wallets**. Their profits are never added together. At most one model/margin survives selection.

## Chronological procedure

1. Verify the complete journal hash chain, original byte-prefix hashes, event ordering, account-exclusion presence, and request/receipt chronology. An observation cannot be hidden in an earlier journal event.
2. Divide cycle records into approximately **60% fit / 20% selection / 20% holdout**, preserving whole equal-time record groups. Purge training labels whose final exit is not strictly before the fitting cutoff. Fit statistics, weights, and normalization use only those matured training rows.
3. Replay every trained model at margins **0.005, 0.01, and 0.015** on the selection interval. Each wallet starts flat, warms up causally, and continues across three checkpoints without resets. There is one shared cash/exposure budget per wallet across its configured instruments, not a separate bankroll per trade. Instrument coverage gates, invalidations, delayed entries/exits, configured fees, exact FIFO, and depth limits are retained.
4. Rank by terminal conservative net PnL. Defaults require at least **20 completed round trips**, no negative checkpoint-to-checkpoint PnL, gains in at least **two of three blocks**, positive total PnL, and available terminal liquidation valuation. These are transparent heuristic screens, **not confidence intervals or proof of statistical independence**. The no-trade baseline wins when nobody qualifies.
5. Freeze the one model/margin and its selection hash **before** evaluating the final holdout. The holdout starts flat with fresh warmup. A bad result is reported; it cannot trigger a switch to another model or a refit. All variants and rejection reasons remain in the bank.
6. Bind the published bank to the **entire** consumed source prefix, so future paper processing begins after the holdout. The fit cutoff and each data role remain separate metadata. Existing weights are not retrained on the holdout.

This is a single chronological split, not repeated walk-forward validation. Selection still searches many correlated alternatives. Historical data may already have been inspected by a human or earlier experiments; calling a segment a holdout does not make it prospectively preregistered. This implementation guarantees that its holdout is excluded from **this run's fitting and selection**. Independent future paper evidence remains necessary.

## Unsold positions and costs

Selection uses current depth-checked liquidation estimates, not midpoint marks. Books older than `maxDataAgeMs` are unavailable to selection valuation. If a full exit cannot be valued, the checkpoint uses **cash only and zero value for remaining inventory**; it does not hide open cost behind realized gains. The result still records realized PnL, fees, turnover, completed trades, pending/expired intents, open shares, and unsold shares. A missing quote does not invent a fill or force a sale.

Configured fees, adverse ticks, latency and depth haircuts stay unchanged. Separate cost configurations remain separate experiments. The cash-only floor is a conservative diagnostic, not a prediction of actual settlement. Even a complete book is snapshot evidence, not proof of a real fill at that size.

## Running a new experiment

Use Node 22+, an immutable complete prefix of the existing collector journal, and a **new output path** in a separately named paper cohort. Do not renumber, reserialize, reset, or replace the original journal. The output directory must already exist.

```sh
npm run research:portfolio -- --journal /path/to/frozen-journal.jsonl --config live-paper/fee-free-config.json --out /path/to/new-cohort/ml-bank.json
```

The fee-free configuration means zero additional explicit fees, while retaining its other assumptions. To compare the nonlinear family, use a distinct output path and append `--family boosted_stumps`. This is another experiment, not an untouched test of the previously selected family. The CLI intentionally does not tune the stability gates or sweep risk budgets.

Both `ml-shadow.mjs` and `ml-shadow-v2.mjs` recognize the frozen `candidatePlan` and run only its selected margin. Banks without that field keep all three historical margins. A no-trade plan has zero candidate wallets. Unknown plans, incompatible targets/lifecycles, mismatched model IDs, and unsupported margins are rejected. Current saved states remain bound to their original bank hashes.

The output reports `productionActivation: false` and `scalingEvaluated: false`. Creating code or a bank does not restart a collector, renew a source/capacity review, transfer execution ownership, or deploy a live trading strategy.

## Verification record

New regressions cover portfolio profit versus RMSE ranking; no-trade fallback; block concentration; stale and thin-book inventory; both existing consumers; exact source bytes; immutable outputs; delayed FIFO behavior; held-out source mutation; purging; malformed inputs; and end-to-end CLI publication.

Synthetic positive, shifted, and missing-exit fixtures all preserve the selected model and margin when only the holdout changes. In the missing-exit fixture, 10 unsold shares remain charged rather than vanishing from results. The synthetic winner-path fixtures lower the completed-trade minimum to one solely to exercise integration; the production CLI default remains 20. These tests do not measure competition returns.

Final test totals and selected-suite exclusions are recorded in the implementation's completion comment in coordination issue #1. No production capture was retrained, no live model was activated, and no contest orders were submitted by this development task.
