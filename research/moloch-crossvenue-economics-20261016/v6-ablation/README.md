# Frozen v6: fixed exploratory ablation

This diagnostic was specified **after** the original negative v6 result, before this analysis. It is exploratory, not a prospective model qualification. The three fixed comparators are: unchanged original SIG BBO, SIG-local imbalance+spread ridge, and the unchanged seven-feature ridge. There was no threshold, feature, cost, horizon or hyperparameter search to obtain positive trades.

The original v6 producer SHA is0655e7609c50aea7c5ca7e68b1b48c89bb68a300dca2786861f320b16e19b02e; cutoff remains2026-10-16T12:03:33.366Z. Exactly the same two sealed gzip files, direct-mapping complete-case extraction, executable one-share depth, fee assumption0,0.005 buffer, ridge penalty1, three chronological folds and horizon+3second purges are used. No raw files or earlier reports were modified. Current reader hashes, source hashes and policy are pinned in report.json. The seven-feature MSE, selected returns and source bundle were checked **exactly equal** to the original published v6 report for every horizon.

The unchanged-BBO forecast is `−(original ask−original bid)−fee−buffer` for each side: crossing round-trip loss if the BBO never moves, with no fitting. The SIG-local comparator uses only original SIG imbalance and spread. It calls the original fixed ridge solver with the other five columns deliberately excluded (zero design columns with ridge diagonal1); these inactive columns have zero cross-products/RHS, exactly the two-feature objective. This is a fixed ablation, not imputation of missing external observations. Every comparator uses the same original complete-case OOS cohort; it does not gain additional coverage by dropping external requirements.

## Actual external variation

Among1,324 direct observations:

| Original innovation | Missing | Available zero | Available nonzero | Maximum absolute logit innovation |
|---|---:|---:|---:|---:|
|1s|0|1,324|0|0|
|5s|152|1,172|0|0|
|30s|232|1,089|3|0.021053298382812446|

The30s mean absolute innovation is0.00004293869263028452197802197802. All944 complete observations are QUIET on both sides under the unchanged joint threshold rule;380 are missing innovation coverage. These are genuine missing-vs-zero distinctions. The sample has almost no short-horizon external-price variation with which to identify price discovery. Observed direct book sources are801WS/523REST (analog observations excluded). This is a property of these original data, not evidence that all external markets are inactive or that future data will have no value.

## Out-of-sample comparisons

| Horizon | OOS clusters | Unchanged BBO MSE | SIG-local ridge MSE | Full ridge MSE | Selected by each |
|---|---:|---:|---:|---:|---|
|1s|295|1.652542373e−6|1.488906698e−6|1.537694746e−6|0/0/0|
|3s|196|1.785714286e−6|1.497750020e−6|1.489958338e−6|0/0/0|
|10s|206|2.305825243e−6|2.043123393e−6|2.204424079e−6|0/0/0|
|30s|204|1.899509804e−6|1.668343343e−6|1.740260107e−6|0/0/0|
|120s|56|8.928571429e−7|1.047668540e−6|1.182869341e−6|0/0/0|

All selected counterfactual net sums are0, and the predeclared100-selected-cluster requirement is unmet. SIG-local beats full-model MSE at four horizons; full is slightly better at3s. The no-fit unchanged-BBO comparator is best at120s, which has only56 OOS clusters. Prior improvement over a constant intercept can reflect ordinary variation in SIG spread/imbalance and mechanical crossing cost. It does not establish incremental Kalshi price discovery, causal predictive value, calibrated probabilities or executable alpha. This observational comparison cannot identify which correlated feature caused an MSE difference.

Missing labels and features remain selective; nonoverlap market/time clusters are not statistically independent across common election factors. Public source observations are not our fills; no queue probability, trading profit or execution/model approval is inferred. Optional new flow-cohort data were not included. All modelApproval/profitEstablished/executionAuthority flags remain false.

## Reproduction and checks

Six meaningful unit tests pass: exact original full-model/cohort parity, mechanical no-movement crossing loss, SIG-local independence from excluded external columns, missing/zero/nonzero distinction, insufficient cohort behavior and original source/producer pinning. The actual run was a bounded nice19 Python process using existing original files and private reader copies. All own analysis, SSH and SCP sessions have exited. No API, market-data client, build or production operations occurred.

Actual report SHA256:30ac93628e68d9444c878acf73c8ff512da8facfda0f3cc184b55dca6400796e. Actual analyze.py SHA256:7bba298c2120bfe66fd62cecf5eb61cc9ae7dda66492710e6555dfd1b9f0e1dd. The result was retrieved byte-for-byte with SCP and verified against the remote SHA; terminal summaries used60-character base64 lines. Root owns checked publication.

```powershell
python -m unittest discover -s research/moloch-crossvenue-economics-20261016/v6-ablation -p test_analyze.py
python research/moloch-crossvenue-economics-20261016/v6-ablation/analyze.py --input SEALED_V6_1.gz SEALED_V6_2.gz --as-of 2026-10-16T12:03:33.366Z --producer-source ORIGINAL0655.py --producer-sha256 0655e7609c50aea7c5ca7e68b1b48c89bb68a300dca2786861f320b16e19b02e --output NEW_REPORT.json
```
