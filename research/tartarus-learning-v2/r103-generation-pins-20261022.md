# R103: reduce repeated work in the physical evidence window

**Qualified, undeployed at22:44UTC.** All430 functional checks pass (3 focused source-pin,149 fundamental,7 rank-cohort,271 combined worker), the final release links and its CLI loads. The separately executed production-source benchmark passes. [Exact hashes, commands/results summary and benchmark](r103-generation-pins-qualified-20261022.json). R101 continued trading during qualification. Its latest original-receipt audit verifies716fillrows/14distinct post-restoration filled orders, incremental realized-114.840, cash3175.630; the additional one-share RI top-up cost0.875 and created no realized profit. [Original accounting](r101-final-one-share-topup-audit-20261022.json).

The original Tennessee23142 attempt failed before network submission with `RANK_PHYSICAL_FACTS_EXPIRED`. Its final book receipt preceded submission preparation by6683ms; cohort ownership preparation consumed4543ms. These timings describe that attempt, not a calibrated estimate of future latency. [Original packet evidence](tennessee-owner-review-005-20261022.json).

## Changes

The active generation references the same immutable source documents from several independently reviewed contracts. A file-only benchmark of the actual retained generation includes110 pins,98 distinct full pin identities,68672867 cumulative bytes and28197137 distinct bytes. Full validation previously reread and hashed each repeated reference.

`GenerationFiles::validate_full` now hashes identical held content once **within each call**. The identity includes path, held descriptor identity, byte count, expected hash, maximum size and the original file version. Each reference still gets its own path/descriptor/version checks. Recent writes use the existing full-content fallback; a final metadata pass also checks every reference after the content reads. The set is discarded at return. Different expectations, versions, bounds or paths cannot share the content result; no later call inherits a prior verification.

`check_rank_review` also stops invoking a second complete cohort check inside its ownership-statement lookup. The enclosing function already checked the same immutable bank, dispatch and cohort references, and no mutation intervenes. All original generation, origin, positions, exclusion, funded-action and binding comparisons remain.

The candidate inherits the previously qualified R102 worker change: redundant derived action diagnostics run after the physical entry packet. The actual entry comparison and original physical approval still run independently. The native engine and financial model are unchanged.

## Measured result and limits

Three full validations of the retained production files took1080589 microseconds with the original loop and453024 microseconds with the candidate: approximately360ms versus151ms per call, a58% reduction for this component. Both paths used the same held inputs; constructing/verifying the pins precedes the timed loops. This is a low-priority, file-only VPS benchmark. It is not total order latency, a fresh-book admission result, a fill or a profitability result. The separate removed cohort-check cost has not been independently benchmarked.

Focused checks cover same-length content changes across calls, conflicting expected hashes on duplicated paths, and replacement of a duplicated pathname. The explicit production-source benchmark is an ignored-by-default test that the build driver runs separately with `--ignored`; it is not merely counted as a passing skipped test. Existing fundamental and rank-ownership checks cover the surrounding behavior.

The build and replacement scripts are in [tools](tools/build-generation-pins-r103-20261022.py) and [the one-owner handoff](tools/deploy-generation-pins-r103-20261022.py). The handoff retains the current four-scope source configuration and exact active reviews, all historical fills/FIFO, native bridges, shared limiter, unknown outcomes, financial limits and daily windows. It does not start another official account client. Qualification, publication, installation and fresh production operation must be verified separately; compilation or this benchmark alone does not establish a live improvement.
