# Selected R18 acquisition audit

This finite descriptive audit uses one private, original observer snapshot with SHA256 `657948917e932db5cfa6f2d2a69a0576e2b27e97652bc42c67283144db2632a9`. Its export manifest was observed at 2026-10-17T03:11:14.480759Z; the snapshot itself ends at **02:50:34.043Z**. The raw account/order/fill records remain private. The auditor performs no downloads, account requests, model fitting or financial writes.

The four selected, confirmed-full orders acquired **22,343 shares for 19,428.305 principal**, versus frozen limit principal 19,513.605: **85.300 price improvement relative to those limits**, not profit. All four used `boundedLimitMayRest`, across three exchanges. All acquired quantities remained held at the snapshot cutoff; actual fees on these rows were zero.

| Native qid | Quote creation to attempt | Attempt to first/full execution | First execution to original receipt |
|---|---:|---:|---:|
| 11680 | 14.247s | 0.994 / 0.994s | 8.868s |
| 11762 | 13.494s | 0.492 / 6.589s | 8.769s |
| 11793 | 16.362s | 1.001 / 1.001s | 8.409s |
| 11813 | 16.365s | 1.220 / 1.220s | 8.918s |

These selected orders show preparation and observation delays larger than their venue execution delays. They do not establish an unconditional fill probability: the snapshot is incomplete, excludes ordinary attempts and pre-cohort pending orders, and later acquisitions are outside its cutoff. No alternate execution mode or failed opportunity control is available. Bounded-limit orders may rest; a full observed fill is not evidence of immediate full depth or a maker/taker role.

Reservation time totals **484,202.656400 SUSQ·seconds**. It follows native committed reservation changes, including delayed knowledge of fills; it must not be labeled venue time-to-fill. Owned-capital observation time totals **9,921,703.459875 SUSQ·seconds**. For these entirely remaining BUY lots only, using each original execution clock instead yields **10,103,226.745985 SUSQ·seconds**, a difference of **181,523.286110**. The auditor checks that equality fill by fill. Both principal-time calculations exclude fees. This execution-clock comparison does not alter the ledger or assert an intratimestamp exchange chronology.

A prospective economic question is whether a fresh, review-qualified tiny residual top-up should receive equal read/dispatch priority to a larger acquisition with greater conditional marginal surplus. These four selected rows cannot prove a better allocation was available simultaneously, calibrate the reviews, or justify a new threshold. No profitability, comparative-mode effect, model qualification or execution authority is claimed.

Reproduce with Python 3 standard library and the original private artifact:

```powershell
python research/moloch-acquisition-audit-20261017/analyze.py ORIGINAL_PRIVATE_ARTIFACT.json --expected-sha256 657948917e932db5cfa6f2d2a69a0576e2b27e97652bc42c67283144db2632a9 --output NEW_SUMMARY.json
```

The input is bounded to 8 MiB, 4,096 rows and 256 fills per row. Output is create-new. SHA mismatch, duplicate fills/qids, attribution/clock inconsistencies, incomplete or no-longer-held quantities, and principal-time disagreement reject rather than fabricate missing observations. `summary.json` was generated from the verified original bytes; no synthetic outcome rows were used. Internal consistency and the pinned observer artifact are checked, but the underlying original receipt/journal membership is not independently replayed here. The summary also pins the exact auditor source bytes, so LF/CRLF source variants have different hashes.
