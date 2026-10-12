# Viserion capital stress audit

The historical Maine paper quote reserves **999.920000 SUSQies** for 1,724 Republican NO shares at 0.580000. Its point reference, 0.625267, implies +78.040308 conditional expected settlement surplus. The bank's separately declared minus-three-point margin scenario, conservatively floored to 0.414044, implies -286.108144. Fractional Kelly sizing across both assumptions permits zero extra shares, whereas the point estimate alone funds the full quote.

This is a shadow diagnostic of settlement risk. Neither assumption is a calibrated confidence bound, and a final-election probability does not forecast the short-term resale price. The audit does not prove a harvest loss, an actual fill, or live profitability. The frozen paper bank and state are unchanged. The state reports zero paper fills and zero realized profit at its historical observation.

The exposure's maximum ordinary losing-settlement loss is 999.920000; all-win settlement surplus is 724.080000, with the bank's zero-fee scenario. Fees are not independently verified for a live order. A fresh executable book, complete source census, validated population transport, and independent execution performance are all absent. The result always carries `executionAuthority:false` and `actualOrders:0`.

`screen.mjs` applies the existing reference Kelly implementation separately to explicit scenarios, selects the smallest funded quantity, and reports evidence gaps independently. It counts existing and pending quantities in every scenario; cash, depth and event exposure retain their original constraints. It never changes allocations or submits orders. Scenario values and money are canonical decimal strings; surplus and exposure use exact micro-SUSQies.

Run from the repository root:

```powershell
node --test research/viserion-capital-stress-20261012/screen.test.mjs paper/reference-kelly-sizing-v1.test.mjs
node research/viserion-capital-stress-20261012/audit.mjs
```

Ten checks passed: stress exposure, losing-scenario rejection, held/pending custody, cash/event/depth constraints, stale/future books, evidence gaps, malformed scenarios, and the existing reference-sizing suite. `results.json` includes source file hashes and observation clocks. Re-running audits current time against the same historical evidence; it makes no account calls.

Next: test whether subsequent, funded executable exits establish harvest edge across stress regimes before promoting this candidate. Source-census and transport research belong to their existing owners. Do not replace the frozen bank or call point-estimate fields confidence bounds.
