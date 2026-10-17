# Ordinary entry cohorts — October 17, 2026

The full ordinary realized loss should not be assigned to today's guards. Of
`−184.970000`, `−151.615000` belongs to quotes created before the first evidenced
ordinary-side Enforce deployment. Later creation eras remain negative, too.
This is descriptive accounting, not an estimate of either guard's causal effect.

| Original quote creation era | BUY groups | Shares | Actual acquisition principal | Realized | Loss / win / flat groups |
|---|---:|---:|---:|---:|---:|
| Before Enforce deployment | 27 | 21,713 | 11,269.180000 | −151.615000 | 15 / 1 / 11 |
| After Enforce first fresh, before defensive deployment | 4 | 11,086 | 7,352.335000 | −24.980000 | 2 / 2 / 0 |
| After defensive first fresh | 10 | 12,712 | 5,076.525000 | −8.375000 | 5 / 1 / 4 |
| All ordinary entries | 41 | 45,511 | 23,698.040000 | −184.970000 | 22 / 4 / 15 |

All 41 groups' **filled inventory** was closed at the frozen boundary; that does
not prove their physical orders were terminal. Acquisition principal is cumulative
turnover, not simultaneous exposure. The two explicitly unknown startup intervals
contain no entries in this report.

## Boundary and release evidence

Input is the private, pinned 338-fill native financial report at sequence `396960`,
report time `2026-10-17T03:41:47.135Z`, raw SHA-256
`0e3d3f84f2557ee63ab426ec556cb3934edae81a39e543efda7aca996f33d491`.
The analyzer first reproduces the existing [financial audit](../moloch-realized-attribution-20261017/README.md):
cash `16092.385000`, owned cost `79347.690000`, combined realized `−139.840000`,
ordinary/new realized `−184.970000`, inherited gross `45.130000`, fees zero.
Reviewed fundamental realized remains zero with `57011.775000` principal held.
These are frozen historical quantities, not current account observations.

[activation-evidence.json](activation-evidence.json) pins five original public
deployment/control/freshness artifacts. Enforce deployed at
`2026-10-16T14:37:09.605684Z`, first verified fresh at `14:51:18.235085Z`;
defensive cancellation deployed at `17:07:56.937582Z`, first verified fresh at
`17:26:02.402458Z`. Labels use original native **quote creation**, not execution:
qid `7016`, created `14:33:51.066Z`, executed at `14:40:21.479Z` after the Enforce
launch but remains in the earlier era. Qid `7991` predates defensive deployment
even though its last matched close was observed during that release's startup.

The report lacks original per-entry intent/control hashes and guard decisions.
Creation eras cannot prove actual admission, subsequent deployment continuity,
whether a defensive cancellation triggered, or the outcome under another policy.

## Recurring mechanism and one prospective option

Maximum-age SELL matches realized `−216.415000` across 39,597 shares; actual
opposite-BUY netting contributed `+31.445000` across 5,914 matched entry-leg shares.
Maximum-age SELL losses occur in every era: `−151.615000`, `−31.435000`, and
`−33.365000`. Latest-era netting offsets `24.990000` of losses; omitting that
positive group would misstate the complete cohort. Netting is an actual ledger
transition, not evidence of additional alpha over an economically equivalent SELL.

Quantity-weighted execution-to-maximum-age-SELL durations are 1,051.027, 596.245,
and 472.057 seconds respectively, despite the configured 60-second oldest-lot
threshold. They include ingestion, planning, resting, and reconciliation effects;
this input cannot separate those delays. One 579-share netting match has reversed
original execution clocks and therefore an unavailable duration. Financial FIFO
follows native commit order, with no timestamp reordering or zero-age imputation.

The source mechanism is consistent with the losses: [native paired-spread selection](../../moloch/native/src/engine.rs)
requires a spread/edge proxy; [ordinary side edge](../../moloch/live-runtime/src/ordinary_side_edge.rs)
uses current depth-weighted microprice, not an estimated 60-second executable exit
value. Native maximum-age handling can override the fixed-gain exit floor.
[Worker dispatch](../../moloch/live-runtime/src/worker.rs) continues ordinary BUYs
when the optional empirical maker review is absent.

**One prospective loss-containment option:** require the existing horizon-matched
maker decision, with an explicitly reviewed bounded exploration sleeve when
evidence is unavailable, for new unhedged ordinary BUYs. Preserve covered hedges,
owned SELLs, fundamental entries, and reconciliation. This requires a separate
reviewed control/model decision; there is no qualified model or positive-EV claim
here. No longer holding period, counterfactual fill, or saved-loss estimate is
proposed.

## Reproduce

Python standard library only, from the repository root, using exact Git LF source:

```powershell
python -B -m unittest discover -s research/moloch-ordinary-cohorts-20261017 -p test_analyze.py
python -B research/moloch-ordinary-cohorts-20261017/analyze.py PRIVATE_REPORT --sha256 0e3d3f84f2557ee63ab426ec556cb3934edae81a39e543efda7aca996f33d491 --out NEW_OUTPUT.json
```

The fixed sibling auditor's Git LF hash is checked before import. Public evidence
is checked against exact Git LF hashes; private input is checked as raw bytes.
Output embeds analyzer, auditor, evidence, input and native-boundary hashes.
Wrong inputs fail before output; an existing output cannot be overwritten.
Five tests cover old resting quotes, startup uncertainty, exact boundaries,
unavailable negative durations, evidence substitution and fill-quantity mismatch.
No receipts are independently authenticated, books invented, or execution authority
granted by this report. Public output contains derived aggregates and native qids,
not raw reports, venue order identities or source receipt bodies.
