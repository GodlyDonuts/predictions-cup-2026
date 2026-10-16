# V9: unchanged frozen price-only protocol

The existing price-only protocol selects **zero after-cost trades** at every horizon on this newly sealed cohort. SIG-local features have lower prediction error than the full external model at four of five horizons. These are counterfactual one-share crossing diagnostics, not actual fills/profit or evidence for a live oracle, toxicity defense or model approval.

## Cohort and unchanged procedure

The two original v9 files contain58,036 rows:22,579 decision records and35,457 horizon labels, zero unmatched labels. Decisions run **2026-10-16T14:04:59.581Z–14:35:46.570Z**. Actual post-stop seal cutoff is **2026-10-16T14:38:21.271728+00:00**. There are234 observed market/exchange scopes:231 direct party references with22,207 decisions, plus three candidate-reference analog scopes with372 decisions. Analogs are excluded from fitting; unobserved configured scopes receive no imputed data.

The untouched [price evaluator](../../../../moloch/external-data/crossvenue_economics.py) and [v6 ablation](../v6-ablation/analyze.py) use their original byte-identical readers, features and policy. No new wrapper or source edit was required. Seven inputs remain external1/5/30s logit changes, external imbalance, SIG imbalance, SIG spread and external/SIG logit gap. Ridge penalty1, three chronological folds, horizon+3s purge and actual label-availability fence, direct mappings only, one-share displayed capacity, assumed zero fees,0.005/share buffer and positive predicted net selection remain fixed. Coefficients are fit only on earlier v9 folds according to the frozen procedure; earlier v6 fitted coefficients are not treated as a universally calibrated model.

The SIG-local ablation excludes five external columns and uses only SIG imbalance/spread, on the **same complete-case OOS cohort**. Its inactive zero design columns implement the fixed two-feature objective; missing original features are never imputed. The unchanged-BBO no-fit forecast remains negative original spread less fee/buffer. The full-model OOS counts, sample hashes, selected hashes, MSE and exact selected net sums agree **exactly** between the original evaluator and ablation at every horizon.

No flow-completeness gate or optional signed-flow feature is applied. V7's separate flow-complete study is neither mixed in nor reinterpreted. Direct-reference mappings remain unqualified cross-venue semantic references, not identical-resolution or riskless hedge certificates.

## Training, coverage and economics

| Horizon | Training clusters in folds1/2/3 | OOS clusters | Full /local /BBO selected | Selected net crossing sum, each |
| --- | --- | ---: | --- | ---: |
|1s|1,395/2,827/4,381|4,444|0/0/0|0|
|3s|1,210/2,393/3,763|3,831|0/0/0|0|
|10s|1,168/2,233/3,450|3,489|0/0/0|0|
|30s|805/1,670/2,495|2,530|0/0/0|0|
|120s|217/451/715|817|0/0/0|0|

All folds fit under the unchanged16-training-cluster rule. OOS count exceeds100 everywhere, but **zero selected clusters fails the separate100-selected-cluster requirement**. The evaluator's historical `INSUFFICIENT_INDEPENDENT_CLUSTERS` reason therefore refers to selected coverage here, not an absence of training/OOS data. Every horizon abstains; zero selected sum is an empty strategy diagnostic, not earned profit.

| Horizon | Full ridge MSE | SIG-local ridge MSE | Unchanged BBO MSE | Intercept MSE |
| --- | ---: | ---: | ---: | ---: |
|1s|1.471334379e−6|1.469517619e−6|1.752362736e−6|4.219957116e−6|
|3s|1.761937083e−6|1.766559270e−6|2.150221874e−6|4.343862771e−6|
|10s|2.243286723e−6|2.239084676e−6|2.815993121e−6|4.424035296e−6|
|30s|2.115605109e−6|2.109096253e−6|2.480237154e−6|4.489920475e−6|
|120s|2.635613556e−6|2.605874005e−6|2.983476132e−6|5.119659886e−6|

The full model improves on the intercept but does not demonstrate incremental external alpha: SIG-local is better at four horizons, full slightly better at3s, and no comparator selects a positive predicted after-cost opportunity. Spread/imbalance can explain mechanical crossing-loss predictability. No significance or causality claim follows from these correlated observational errors.

4,141 direct decisions lack complete features. Among feature-complete decisions, missing horizon labels are11,045/11,584/11,602/12,037/15,922 at1/3/10/30/120s, before deterministic nonoverlap pruning. Missing features/labels are selective; they are not zeros. Labels are future executable-side displayed one-share BBO crossings with original clocks; no actual order or queue fill is inferred. Nonoverlapping market/time clusters still share election/factor risk across markets.

## External variation

| Original external change | Missing | Available zero | Available nonzero | Maximum absolute logit change |
| --- | ---: | ---: | ---: | ---: |
|1s|307|21,898|2|0.04058586848|
|5s|2,820|19,329|58|0.1194593639|
|30s|1,418|20,580|209|0.1401797659|

Direct selected-book sources are14,468WS/7,739REST. The unchanged diagnostic threshold states have18,064QUIET, one FAVORABLE and one TOXIC per YES side, with the NO classifications reversed;4,141 have missing innovation coverage. More variation than v6 is present, but short-horizon innovations remain sparse. Neither zero nor nonzero innovations constitute inferred flow, independent samples or trading approval.

## Original hashes and reproduction

Original producer bytes are36,610B, SHA256 `674264745ced43b053881ad7408a5031a6e389c8652ff950b23bca0e1448b34d`. This is the downloaded immutable producer, **not** its newline-normalized Git blob SHA `2c30a76b07a4684aca529801375cfd6edad513e79624b1a3e0e5e58ffeedbd66`. Both formats use the explicitly validated scoped sample-ID formula. All original causal lag anchors/clocks are validated by the unchanged readers.

| Original sealed file | Bytes | SHA256 |
| --- | ---: | --- |
| `20261016T14-1792159499583772195.jsonl.gz` |7,753,039|`cce246edfe4ccee8b9ae294f1d68aef0cf6a9232eb57ac5b91228c88950b53df`|
| `20261016T14-1792160558379312407.jsonl.gz` |7,281,644|`9af61d80b70c30c9892261e687866e6d00aff4cf45d6df08b748f23c4c3b5a3b`|

Input hashes/lengths were independently checked locally before and after analysis. Source-bundle SHA is `ade2aae3f93e7e8409aa1f30cae27db2244314f8be1ed486f440cec025710d09`. The v6 ablation policy SHA remains `e10d621cf87e49fe89286b0bbc8a9406df7eb50439fa01484e03f53fbee03b39`. Reader and original evaluator hashes are recorded in [report.json](report.json); none changed. Six existing ablation checks passed locally, including exact cohort/full-model parity and missing/zero distinctions. Actual bounded local evaluation took10.75seconds at below-normal process priority. No SSH, API/client, producer/runtime edits, builds or production restart occurred.

Run the unchanged price evaluator with `--input FILE1 FILE2 --as-of 2026-10-16T14:38:21.271728+00:00 --producer-source ORIGINAL674.py --producer-sha256 674264745ced43b053881ad7408a5031a6e389c8652ff950b23bca0e1448b34d --sample-id-format scoped_v2 --output NEW_PRICE_REPORT`, leaving all policy defaults unchanged. Run the v6-ablation CLI on the same inputs/as-of/producer, without the sample-ID argument (its frozen format is already scoped). The combined artifact preserves both original results plus exact parity/scope metadata; no wrapper or altered policy is needed. It contains aggregates and research model coefficients, not original data rows.

Published Git-LF combined report SHA256: `014fd74380bcc92fa67f9bb3d0cbac21f9eb963c2b274d38ac493e51d42bcbb9`. All model approval, execution authority and profit-established flags remain false. Root owns publication and actual execution.
