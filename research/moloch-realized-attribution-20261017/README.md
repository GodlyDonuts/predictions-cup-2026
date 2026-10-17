# Actual realized attribution — October 17

At the frozen native report boundary **396,960 events / 338 unique actual fill
rows**, report time **2026-10-17T03:41:47.135Z**, all **−184.970000** of
new-trading realized P&L is attributable to ordinary entries. Reviewed fundamental
entries have **zero realized P&L and 57,011.775000 held principal**. Their unresolved
terminal outcomes are neither losses nor profits in this attribution.

| Original entry class | Gross realized | Still-held principal |
|---|---:|---:|
| Ordinary | −184.970000 | 0.000000 |
| Reviewed fundamental | 0.000000 | 57,011.775000 |
| Original inherited lots | +45.130000 | 22,335.915000 |
| Package/unknown | 0.000000 | 0.000000 |

Fees are exactly zero in this source. Combined realized is **−139.840000**;
cash **16,092.385000** and owned cost **79,347.690000** reconcile exactly to native
positions and accounting. Original initial cash40,000 plus the separately recorded
55,579.915 owner contribution is capital, not profit.

Ordinary acquisitions comprise **41 native BUY qids, 45,511 shares and
23,698.040000 cumulative principal**. All their *filled inventory* is now closed:
22 entry-qid groups lost, four gained and15 were flat. This does not prove original
order terminal status or exclude an outstanding unfilled quantity.

Actual FIFO closes tagged `maximumAge` sold39,597 ordinary shares for
**−216.415000** gross. Opposite-BUY netting closed5,914 ordinary leg-shares for
**+31.445000**, producing the net−184.970000. Those are different realized
mechanisms, not hypothetical hedge prices. By original entry execution date UTC:
October15 **−116.020000**, October16 **−65.340000**, October17 **−3.610000**.
Qid12010's60-share round trip is flat; qid12101's1,022-share round trip lost3.61.
The two largest losing entry qids721/867 total−109.51; these results do not establish
the profitability of a different prospective strategy.

For actual maximum-age SELL matches, the quantity-weighted execution holding age
is **816.206167 seconds**, not a60-second execution guarantee. This report does
not identify how much came from discovery, pricing, source freshness or scheduling.
One579-share opposite-netting match has a closing execution timestamp18.417 seconds
earlier than the native FIFO entry it consumes. Its duration is **unavailable**,
not zero or negative. Financial FIFO retains native commit order. Fourteen global
execution-clock inversions are retained; no execution-time sorting makes totals fit.

## Reproduction and limits

The private3,980,494-byte input SHA256 is
`0e3d3f84f2557ee63ab426ec556cb3934edae81a39e543efda7aca996f33d491`.
Private source-manifest SHA256 is
`e56f8a5cbed0baf49510553cd658b452ca116e857674be7090b52caf2f751d7c`;
its observation clock is2026-10-17T03:46:41.942528+00:00. `summary.json` retains the
native sealed report hash, exact boundary, source hashes and auditor-code hash.
No raw report, venue order IDs, receipt rows or personal candidate data is published.

```text
python -B audit.py PRIVATE_REPORT --sha256 0e3d3f84f2557ee63ab426ec556cb3934edae81a39e543efda7aca996f33d491 --source-manifest PRIVATE_MANIFEST --out NEW_OUTPUT.json
python -B test_audit.py
```

Actual reconstruction verifies the canonical native report seal, all fill/order
terms, unique exact fill identities, exact endpoint FIFO lots, cash, cost, fees,
new/inherited realized and capital identity. Native `Engine.fills` append order
is the accounting order (`native/src/engine_live.rs`); financial semantics follow
`native/src/lib.rs::buy_fill_exact/sell_fill_exact`, at source base`bc6e642b`.
Inherited inventory starts at actual adoption08:44:12.730Z on October15; all source
fills follow it. Earlier fills would require original financial events and are
rejected. The owner contribution is applied at its actual retained commit clock,
not made available retrospectively. Packages, collateral transforms or missing
basis require additional original evidence and fail closed.

This is independent **sealed-report accounting reconstruction**, not full-bank
hash-chain replay or independent receipt authentication. Six hand-calculated and
adversarial tests pass, including netting/fees, changed duplicates, pre-adoption
activity, uncovered sales, missing economic transitions, endpoint mismatches and
refusal to treat an execution-clock conflict as a valid duration. No account
requests, mark-based profit, trading authority or forward-return claim is made.
