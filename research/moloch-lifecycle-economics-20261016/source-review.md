# Independent lifecycle estimator source review

Reviewed by Viserion/main_takeover on **2026-10-16**, final author source **`f291a09b16e595b52fc60dd10cc23659a8d68f36`**, following `21bc0251` and `dd71e83c`. This reviewer did not implement the estimator. Scope was source/protocol plus synthetic offline tests; no current cohort/outcome files, account requests, SSH, compilation, native replay or production operations were used.

**No remaining concrete blocker found for the documented research-only scope.** This does not qualify a probability/value model, establish actual profitability, or grant financial/execution authority.

## Findings repaired before this conclusion

1. **Original-chain genesis compatibility.** Initial `journal` used 64 zeroes for both inputs. Actual native `Journal::default()` starts with an empty predecessor; ordinary `DispatchLog` starts with 64 zeroes. Author `dd71e83c` now selects each explicitly and tests both canonical serialization fixtures plus rejection of the opposite genesis. The protocol's named engineering revision preserves the original declaration/hash; no horizon or economic threshold was changed and no actual outcomes were read.
2. **Foreign coverage could qualify a mark.** A synthetic same-exchange/different-market coverage event previously produced `full_depth_gross_mark`. Final `_accept` checks the known exchange/market and available bound tournament context, with poisoning/rejection regressions.
3. **Source-reference verification was overstated.** A valid native row seal does not independently recompute embedded `sourceRef.hash`. Final output separates reader-verified native row hash from syntax-only `recordedSourceRefSha256`, and explicitly reports missing independent source-body/receipt authentication. It no longer calls the latter a verified book-body hash.
4. **Dispatch cutoff.** Native event clocks were bounded by manifest `asOfUtc` while available dispatch attempt/POST clocks were not. Final CLI bounds those original clocks too. Untimed `closed` records still rely on jointly sealed prefix membership and matching native terminal receipt pairs, not an invented dispatch availability clock.

## Accounting and label checks

- Accepted native event order drives FIFO; late execution timestamps do not reorder lots. Same-side SELL consumes FIFO. Opposite BUY uses actual `1-buyPrice` proceeds against opposite FIFO, then opens only the actual residual. ACK/public prints do not become fills.
- The oldest **remaining eligible** ordinary lot establishes the 60-second deadline. A first partial at 0s and completion at 59s is labelled at 60s, not 119s. Actual consumption of the oldest lot advances the deadline only to the next lot's original clock. Mixed inherited/nonordinary episodes remain excluded from ordinary maker labels.
- Exact rational proportional entry/exit fees are charged once per consumed quantity; missing fees leave net unknown. An independent synthetic probe bought 3 YES at .400 with 3 micro-units fee, bought 5 NO at .550 with 5 micro-units fee, then sold the residual 2 NO at .600 with 2 micro-units fee. It ended flat with **250,000 micro-units gross / 249,990 net**, conserving all 10 micro-units in fees.
- Marking walks full external depth, including exact NO complements and known own/unknown-order shadows at their price levels. Later terminal/fill knowledge cannot silently improve older depth. The first stale/shallow/late/gap/coverage failure is censored, rather than replaced by a later favorable book. Hypothetical marks and unsupported partial depth are never added to actual realized totals; future exit fees remain unknown.
- Confirmed terminal zero-fill requires the actual complete native terminal and matching original closed receipt pair. Open/unknown/missing observations are separate categories; early terminal zero is not a full-window fill-probability estimate.

## Verification and retained limits

On the exact frozen final source, bundled Python ran **32/32 tests PASS (0.028s)**:

```text
python -B research/moloch-lifecycle-economics-20261016/test_estimator.py
```

The separate residual-netting fee probe and valid-identifier foreign market/tournament coverage rejection probes passed. `git diff --check` passed. These are synthetic original-shaped serialization fixtures, not an executed Rust Session or a real-cohort replay.

The pinned CLI verifies original input bytes and full row chains, not the native financial engine, independent official receipts, source-body hashes or venue maker role. Unsupported settlement/package inventory boundaries fail closed. Ordinary realized cohort net allocates entry fees to consumed lots; it is not a claim of parity with the ledger's period realized metric, which charges fees when recorded. No scalability benchmark or model calibration was performed. Any later actual evaluation must preserve those distinctions and the pinned original cutoffs; source review alone does not authorize it.
