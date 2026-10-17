# Victory Directive: new prospective price cohort

The unchanged price and v6 ablation protocols select **zero after-cost trades** at all five horizons on this new 30-minute cohort. External features slightly improve prediction MSE versus SIG-local features at three horizons; this does not establish an executable advantage. No model, probability, allocation or execution authority changed.

The existing read-only observer612336 ran from05:04:30 to05:34:32UTC on October17, consuming the existing sole611948 breadth file and local Kalshi cache. No SIG HTTP client, collector or writer was added. At seal05:38:27.913983UTC, its process was absent, final status stopped and both gzip files passed full decompression/footer/complete-JSON checks. Compressed inputs were downloaded byte-for-byte, checked before and after analysis and retained privately. [Seal and exact input hashes](seal.json), [original start](start.json), [final status](status.json).

The final status recorded17,215 samples/23,692 labels; the complete sealed bytes contain **17,349 decisions/23,697 labels/41,046 rows**. The discrepancy is retained explicitly; the reports count actual file rows, without rewriting the original status. Across234 observed markets, direct and analog mappings remain distinct. The evaluator excludes analogs; unmatched labels are0. Missing labels/features and pending/censored outcomes remain missing.

The original producer is3d3b5d43a6c79293d28b424fc4ab31626267acdd1c1b71918d9cdefd94824cd7. Its exact code matches the checked current observer except CRLF;58 focused checks passed at capture setup. Existing scoped_v2 sample IDs and source/arrival/decision/label clocks are checked. This bounded observed-feature capture does **not** authenticate a full raw-input trace. The preceding full-trace attempt failed the unchanged10GiB capacity guard before capture and was retained separately; it is not included as successful evidence.

|Horizon|OOS market/time clusters|Unchanged BBO MSE|SIG-local MSE|Full-model MSE|Selected BBO/local/full|
|---|---:|---:|---:|---:|---|
|1s|2872|1.453690808e-06|1.258416178e-06|1.243414095e-06|0/0/0|
|3s|2730|1.460622711e-06|1.304111174e-06|1.293973379e-06|0/0/0|
|10s|2620|1.369274809e-06|1.268858042e-06|1.267734112e-06|0/0/0|
|30s|1918|1.707507821e-06|1.593096164e-06|1.59361902e-06|0/0/0|
|120s|719|1.721140473e-06|1.588867957e-06|1.607489391e-06|0/0/0|

[Price report](price.json) and [ablation report](ablation.json) use the original fixed ridge penalties, three purged folds,100-selected-cluster minimum, zero fee and0.005/share round-trip buffer. One-share executable crossing comparisons are future SIG bid minus current ask for YES, and current bid minus future ask for NO. They are counterfactual, with no own fill, queue probability, slippage calibration, strategy profit or order approval. Market/time nonoverlap does not imply independent election factors.

Direct observations contain12,603 WebSocket and4,539 REST external sources. At1s only10 of17,110 available innovations are nonzero; at5s76/15,242; at30s271/16,110. There are2,878 missing-innovation decisions. The fixed diagnostic rule identifies only two YES-toxic/NO-favorable decisions; the rest of its complete cases are quiet. This is limited event variation, not proof Kalshi is always uninformative. Full MSE is lower than local at1/3/10s and higher at30/120s; all three comparators still abstain.

The existing evaluator and ablation source were unchanged. Local finite runs took3.484s/4.453s. Exact source-bundle, reader, producer and policy hashes are recorded in the reports. The unchanged v6 policy SHA is e10d621cf87e49fe89286b0bbc8a9406df7eb50439fa01484e03f53fbee03b39. Root checked exact full-model MSE, OOS IDs/counts and selection parity across both independent CLI outputs, plus all original input hashes after analysis. No threshold tuning or combining old negative cohorts as new independent evidence occurred.

Reproduce the price report with the existing moloch/external-data/crossvenue_economics.py CLI, both sealed inputs, as-of2026-10-17T05:38:27.913983+00:00, exact original producer/SHA, sample-id-format scoped_v2 and unchanged defaults. Run research/moloch-crossvenue-economics-20261016/v6-ablation/analyze.py against the same inputs/as-of/producer. These are research diagnostics; further event-rich prospective evidence is needed before alpha can reach production.
