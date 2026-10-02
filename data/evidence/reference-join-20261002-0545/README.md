# Conservative cross-venue lead join

This deterministic offline join compares the published cached Cup identity/bulk table with the independently collected public reference bundle. Both compressed and uncompressed reference SHA256 values were verified before analysis. No source collection, contest orders or wallet changes occur.

Of 237 Cup exchanges, **77 have structured identity candidates** among the 993 external generic-party contracts; **160 remain unmatched**. Two candidate rows involve an explicitly special external Senate election and are classified separately. This is not 77 verified equivalent contracts. Cup special-election status is not explicit, and every candidate needs complete current rule/settlement verification.

Matching uses the supplied structured race/state/office, General stage, 2026 election date and party. House district is parsed only from explicit structured raceName. Titles or opaque IDs cannot fill missing structured metadata; named-candidate/nonstandard external records cannot join as generic party outcomes. Duplicate external contracts remain distinct. No distinct party contracts are assumed to be complements.

Ranking is the largest observed `external YES bid − Cup ask` or `Cup bid − external YES ask`, before fees, available quantities, settlement timing and risk. These are historical price discrepancies, not expected profit or probabilities. Cup receipts are roughly an hour older than the reference receipts, and source quote age is unknown on both venues. All rows have `executable:false` and `probabilityInputReady:false`; zero/unknown external top size is flagged.

Reproduce from the repository root:

```sh
node live-paper/reference-join.mjs --cup data/evidence/breadth-identity-20261002-0530/joined-leads.json --reference research/public_reference_2026-10-02/reference.json.gz > reproduced.json
```

Stdout including its final newline must match `leads.json`; `manifest.json` binds exact input/output hashes and normalized code hashes. This verifies reproducibility from published derived inputs, not authenticity of omitted original raw sources. Read the preserved rule text, terms links, flags and separate receipt clocks in each row before selecting fresh verification work. No external quote becomes a calibrated probability-range input automatically.
