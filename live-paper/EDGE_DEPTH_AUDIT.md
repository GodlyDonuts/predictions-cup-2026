# Trial horizon depth diagnosis

Run `node live-paper/edge-depth-audit.mjs` in the Windows workspace. It reads the preserved deployment trial snapshot, active frozen bank, and original public-book capture. It verifies the complete public hash chain and exact entry receipt before finding the first same-market quote whose request starts at or after the five-minute deadline. A session boundary or market invalidation before that quote revokes the observation. Later favorable quotes cannot replace the first deadline observation.

This is a read-only diagnostic with no account client or wallet mutation. The original bank, top-depth censor policy, journal and model remain unchanged. Original consumer checkpoint timing remains necessary for an exact engine replay; this tool reports source quote eligibility separately.

The alternative depth walk skips levels with less than one whole share of capacity at the unchanged one-percent participation cap. Capacity is floored separately per level. It uses the first eligible bid's worse price and one adverse tick; no fee is added. It never pools fractional capacity across price levels or pretends a thin best bid can support the whole order.

Actual subsequent observations on October 3 UTC:

| Market | Entry receipt | First due quote | Best bid size | Eligible deeper bid | Diagnostic net per share |
| --- | --- | --- | ---: | ---: | ---: |
| 226 | 01:39:44.898 | 01:44:54.666 | 60 | 0.080 | -0.025 |
| 377 | 01:40:25.550 | 01:45:31.513 | 74 | 0.620 | -0.020 |

Both on-time quotes had new source versions but failed the frozen top-depth minimum of 100 shares. Deeper liquidity existed, at worse prices; using it would still give negative modeled one-share returns. This verifies that maturity-aware collection obtained the required quotes, and separates censoring from a profitable opportunity. These two diagnostics are neither original completed labels nor actual fills, and do not establish strategy performance. The source-bound report is `data/edge-trial-outcome-depth.json`.

Any future full-depth strategy needs its own prospective definition and causal fill/exit tests. Do not reinterpret these historical diagnostics as a reason to enable or scale the losing pilot.
