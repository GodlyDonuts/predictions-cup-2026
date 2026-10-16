# Cross-venue economic evaluation — October 16

This is a standard-library-only offline reporting pipeline for original `crossvenue_observer.py` output. It has no network, credentials, account client, model approval or execution authority. The parent has now evaluated the first sealed prospective capture:200 decisions/291 future labels/35 markets, zero selected trades and abstention at all five horizons. [Original report and limitations](../moloch-crossvenue-economics-20261016/README.md).

## Run on sealed inputs

```powershell
python moloch/external-data/crossvenue_economics.py --input sealed-a.jsonl.gz sealed-b.jsonl.gz --as-of 2026-10-16T10:00:00Z --output new-report.json --producer-source retained-observer.py --producer-sha256 ACTUAL_SHA256 --sample-id-format scoped_v2 --fee-per-share 0 --buffer-per-share 0.005
```

The output file must be new. Inputs may be gzip or plain newline-delimited JSON, must be regular nonsymlink files, and must remain unchanged during parsing. Limits are 128 files, 500,000 records, 1 MiB per line, and 512 MiB total compressed/uncompressed input. Truncated gzip, conflicting duplicates, duplicate JSON keys, nonfinite numbers, invalid timestamps and inconsistent source/sample hashes stop the report. These bounds are safeguards, not a memory estimate; use smaller sealed batches on constrained hosts.

Each original compressed file's SHA256, source-bundle SHA256, source-code SHA256, exact UTC cutoff and cutoff/policy SHA256 appear in the report. This verifies the supplied observer records and their referenced hashes; it does not independently replay the full native or Kalshi journal prefix.

## Frozen data and fixed model

Exact `sampleId` joins original `decision_features` and `horizon_label` records. The explicitly selected `scoped_v2` ID is independently recomputed from market/exchange, original SIG hash, external hash and decision clock. Retained observer v2 used the older `legacy_unscoped_v1` format (SIG hash, external hash and decision clock only); that fixed format must be explicitly selected, never guessed or rewritten. CLI requires the retained producer source file and its expected SHA256, validates the original bytes, and records both format and producer hash in the cutoff/policy digest. This verifies declared producer provenance, not an independent proof that the process ran those bytes. Conflicting old-ID scope collisions fail closed. Both source receipt time and first observer availability must precede the decision. Future labels must use original source and receipt clocks within the requested horizon plus 3-second slack, and actually have become available by the cutoff. Later observations never replace missing labels with zero.

The fixed seven features are external log-odds changes at 1/5/30 seconds, external imbalance, SIG imbalance, SIG spread and external/SIG log-odds gap. A null feature drops the row and is counted; there is no imputation, interpolation or synthetic history. The default mapping cohort is `direct_party_reference`; analog mappings are counted and excluded. Even direct mappings remain conditional observations, not certified settlement equivalence.

There are two ridge regressions, one per YES/NO one-share executable crossing return. Penalty is fixed at 1.0; there is no search over features, penalties or thresholds. Standardization uses training data only. Coefficients and training-sample hashes are retained as research diagnostics, not an installable approved policy. No probability or fill-probability calibration is claimed.

Evaluation covers 1, 3, 10, 30 and 120 seconds using three expanding chronological folds. Training label windows must finish strictly before the test boundary, purging the full horizon plus label slack; their actual availability must also precede that boundary. Previous test rows may enter a later training fold only when these same causal conditions hold.

## Economic target and abstention

YES crossing is **future original SIG bid minus original entry SIG ask**; NO crossing is **original SIG bid minus future original SIG ask**, using the canonical binary complement. One-share entry and liquidation quantities must actually exist. Exact Decimal arithmetic verifies the observer's saved counterfactual fields and deducts the explicit round-trip fee and buffer assumptions. Exact aggregate monetary strings accompany floating-point regression metrics.

This is a one-share counterfactual, not a claim of actual fills, queue priority, large-position capacity or profit. Default fee zero is an explicit assumption; unknown fees and semantic differences remain unresolved. The 0.005 buffer is conservative policy, not learned execution cost.

Rows are greedily reduced to nonoverlapping market/time windows of horizon plus slack, independent of their outcomes. Multiple exchanges, YES/NO sides and frequent prints do not inflate that market's window count. **These clusters still need not be statistically independent across correlated markets.**

Each horizon reports held-out/selected cluster counts, selected net crossing return, unconditional YES/NO crossing returns, prediction MSE, and an intercept-only training baseline. Research selection requires positive predicted net return after fees/buffer. It abstains unless at least 100 held-out and 100 selected clusters exist, selected actual counterfactual net return is positive, and the model beats the intercept on both MSE and selected economic return. This is a fixed research screen; testing five horizons also creates multiple-comparison risk. Every output remains `UNQUALIFIED_RESEARCH`, `modelApproval:false`, `profitEstablished:false`, `executionAuthority:false`, even if that screen passes.

## Checks

`python -m unittest discover -s moloch/external-data -p test_crossvenue_economics.py -v`: **15/15 pass**. Tests include explicit retained legacy-producer hash/ID validation, actual observer-schema output, unchanged-midpoint crossing loss, positive synthetic held-out signal versus intercept, insufficient clusters, nonoverlap windows, missing history, feature lookahead, source/sample binding, exact cutoff/hash changes, duplicate conflicts, delayed/early/tampered labels, missing liquidation capacity, full purge/availability fences, truncated/bounded input and training-only standardization. Synthetic tests establish implementation behavior only.
