# Atlas: earlier-year joint forecast diagnostic

The fixed existing Atlas model now reports joint-versus-product-marginals pair scores for strictly earlier-year fits. **No joint advantage is established.** This extends its existing diagnostic; the fitted model, CONFIG, all old functions except validation/export filtering and every original validation field remain exactly unchanged. No live forecast, control, probability, source clock or authority changed.

|Earlier-year joint diagnostic|Actual result|
|---|---:|
|Forecast folds / multi-race cycle blocks|20 /15|
|Pair observations, correlated within cycles|5,640|
|Equal-cycle joint pair Brier|0.08612221353006541|
|Equal-cycle product-marginals pair Brier|0.08565420374808323|
|Joint minus product, lower better|+0.00046800978198216665|
|Cycle-bootstrap95% percentile interval|[-0.0007028375915296667,+0.0017222658447823482]|

The sampler is unchanged:4,096 shared-t4 draws, year seed. The comparator is the product of the same model's exact marginal probabilities, not an independently refitted forecast. The target is both races' Democratic candidates winning. The block statistic equally weights15 election-cycle means;5,640 pairs are not independent repetitions.2,000 fixed-seed bootstrap replicates retain each cycle's pairs together. This interval is not a coverage guarantee and does not include all model/MonteCarlo/vintage uncertainty. Positive estimated difference and an interval spanning zero do not justify promoting joint allocation or replacing the model with a claimed superior independent forecast.

[Report](report.json) contains every earlier-year cutoff, training years, test race IDs and per-cycle scores. Original earlier-year marginals remain357 predictions/20cycles/Brier0.087274266; prior LOCO joint scores remain separately retrospective and may use later election years. Tests change held2022 labels and verify every chronological forecast stays identical, verify original sampler/pair identities and demonstrate that100,000 pairs in one cycle cannot overwhelm another cycle's equal weight.

Child15/15 tests and root15/15 tests pass on exact original trainingSHA96c140d777bf70d020ddafb6091c684c72676f1819543f00b85263864645d465. Root separately regenerated full validation, matched the child's full result exactly, and reproduced every old validation field and old fitted parameters. The [verification record](owner-verification.json) pins source/log/data hashes. Original full predictions/normalized training remain private; this report publishes derived cycle aggregates, not the raw polling archive.

The original restricted runtime directory is absent. Recovered polling CSV4,961,378B and governor CSV476,854B are present in a private recovered archive and their original manifest hashes were verified. The independent MIT Senate CSV is absent, so its original verification cannot be repeated here. Current cohort:416 races/1,291 observations/47states; Senate185races/13even cycles1998–2022 and Governor231races/23cycles including odd years. Historical publication clocks and LV/RV labels are missing. Eventual-candidate selection, revised outcomes, fixed shrinkage/variance floors/t4 tails and limited cycle-level evidence remain. No historical executable prices, own fills, deadline-liquidation model or trading P&L are supplied.

Installed eleven CTM/operator forecasts are a different forecast family; no Atlas covariance is silently attached to their point estimates. Current Atlas artifacts are expired/research-only and do not establish2026 transport calibration or exceptional contractual-state probabilities.

Reproduce using unchanged pinned originals and the existing model:

```text
python research/atlas_model/chronological-joint-20261017/reproduce.py --training ORIGINAL_TRAINING --old-validation ORIGINAL_FIT_V3_VALIDATION --old-model ORIGINAL_FIT_V3_MODEL --out NEW_PRIVATE_DIRECTORY
python research/atlas_model/test_model.py --training ORIGINAL_TRAINING --current ORIGINAL_CURRENT_V2
```

The reproducer refuses an existing output and verifies original training/validation/model-source pins before fitting. It exports no current artifact, creates no account client and does not authorize trading. [Task160](https://github.com/GodlyDonuts/predictions-cup-2026/issues/160).
