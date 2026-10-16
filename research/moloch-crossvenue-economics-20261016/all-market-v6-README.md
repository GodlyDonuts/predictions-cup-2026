# Sealed all-market v6 economic evaluation

Observed/evaluated2026-10-16, cutoff **12:03:33.366Z**. Only the original sealed v6 cohort is included: decisions11:48:28.771–12:01:00.624Z. Before analysis, original PID496337 was absent and original status reported stopped; the two gzip files were sealed. The evaluator independently checked file identity/size/mtime while reading and original compressed hashes. No files were repaired, relabelled or mixed with v7 flow observations. No market/account requests, compiler jobs, production mutations or new trading clients were used. Private analysis scratch and read-only SSH/SCP sessions are closed.

Original producer SHA256:`0655e7609c50aea7c5ca7e68b1b48c89bb68a300dca2786861f320b16e19b02e`, explicit scoped_v2 sample-ID formula. Source bundle SHA256:`03c5481a4ab4242e4416a7bc0d0705227435d0c0a536744cb3682b3cb26578a3`. The report and summary pin each original gzip, producer, reader and result hash. Downloaded result bytes were verified against the remote manifest; no terminal-wrapped JSON was reassembled.

Inputs:3,251 original rows =1,348 decisions +1,903 matched labels,229 observed markets, zero unmatched labels. All1,348 prospective decisions retain lag-anchor metadata.24 analog-mapping decisions are excluded under the existing direct-party-reference policy; among1,324 direct observations,380 lack the complete seven-feature history and944 are complete. The book source was WS for816 decisions and REST for532. Exact same-source/stream causal anchor clocks, hashes and recomputed deltas were checked for analyzed direct observations; absent lag history was not imputed. Market mappings remain unqualified for contractual equivalence.

No thresholds or parameters changed: the same seven numeric features, fixed ridge penalty1.0, three chronological expanding folds, horizon+3second label-slack purge, 1-share counterfactual, fee assumption0 and buffer0.005/share. Fit standardization uses training data only. The fee assumption does not verify actual zero venue fees. Both original buy-ask/future-bid and opposite-side crossing costs use exact executable one-share depth.

| Horizon | Nonoverlap clusters | Out-of-sample clusters | Selected | Selected net sum | All OOS YES crossing sum | All OOS NO crossing sum |
|---|---:|---:|---:|---:|---:|---:|
|1s|343|295|0|0|−3.135000|−3.160000|
|3s|281|196|0|0|−2.105000|−2.085000|
|10s|257|206|0|0|−2.190000|−2.190000|
|30s|255|204|0|0|−2.185000|−2.160000|
|120s|84|56|0|0|−0.580000|−0.590000|

Every horizon **abstains**. Selected clusters are below the predeclared100 minimum, even where OOS coverage exceeds100. The intercept baseline also selects zero, with selected net sum0. Ridge MSE is lower than intercept MSE in this sample, but it does not beat the baseline on selected after-cost economics; statistical fit is not an executable edge. The unconditional crossing sums assume one acquisition per observation and are counterfactual research diagnostics, not realized account PnL.

Missing timely labels among944 complete direct-feature decisions:594/657/675/676/856 at1/3/10/30/120s. Missing outcomes are not assigned zero return. Eligibility depends on subsequent original SIG book arrival within the fixed horizon/slack and may be selective; the matched sample cannot identify performance on all missed opportunities. Nonoverlap market/time counts do not establish independence across common election factors, markets or time. No observed maker queue/fill probability, calibrated probability, model approval or profit is established.

Artifacts: `all-market-v6-report.json` is the unchanged evaluator output (remote name all-market-v6-economics.json), SHA256:`72a2447837531d415682346981416547d24016bd08d8aacc557b509f81c963b5`; `all-market-v6-summary.json` is the unchanged aggregate manifest/summary, SHA256:`7be5bf6061544e1a15eee0b99daa7e1f57003e674d2bebc6d91f4f254a4d6ab4`. The companion toxicity result is separate and uses its predeclared passive-shadow rules. All approval/authority flags remain false. Root owns publication; this task did not deploy anything.
