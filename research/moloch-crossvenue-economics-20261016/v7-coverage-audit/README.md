# Why sealed v7 had no flow-complete decisions

The retained captures show sparse same-ticker executions plus export/decision
delay. This audit found **no missing exported trade evidence** and no demonstrated
collector or observer bug. The original v7 result, features and thresholds remain
unchanged. This is a coverage diagnosis, not a tradable model or profit result.

## Original evidence

- Sealed observer gzip: 2,598,985 bytes, SHA256
  `3fd8f87f38a93e180d572b5f6f41f672032611fdf5c689737e0f9ad02ce6af0b`.
- Original closed local collector gzip: 67,109,548 bytes, SHA256
  `f1e93c7c8e708d5205eea208dba1f5c0e5cca3515323bc61dd5cc29b60930308`.
- Original observer source SHA256
  `674264745ced43b053881ad7408a5031a6e389c8652ff950b23bca0e1448b34d`.
  Frozen evaluator LF SHA256
  `12b185a9dd4efd0f6e72583d4274c240256416818a351018a27543525d4bb06c`;
  its Windows CRLF bytes have a separate raw hash recorded in the report.

The audit pins both compressed files before and after reading, validates every
original trade body hash and JSON, rejects conflicting duplicate trade IDs, and
joins retained flow evidence to exact original trade ID/body hash/source/receipt
clocks. Every direct decision passes the original frozen flow validator. No
source timestamp or captured first availability is changed.

The collector file contains 537 unique public trade prints across 63 tickers,
versus 83,085 order-book snapshot/delta frames. The actual observer decision
interval is **12:01:37.211–12:36:07.245 UTC on October16**: 257 collector trades
across 48 tickers were received in that interval. The 3,813 direct decisions
cover 231 tickers; 70 candidate-reference analog decisions remain excluded.
The publisher's full trade-tape completeness is not established.

## Exact decision-window accounting

| Window | Actual identified decisions | Same-ticker receipt eligible at original export | Optimistic receipt eligible by decision |
| --- | ---: | ---: | ---: |
| 1 second | 0 | 0 | 3 |
| 5 seconds | 5 | 5 | 8 |
| 30 seconds | 48 | 48 | 50 |

The middle column agrees exactly with retained identified inputs. At all three
horizons there are zero cases of an original same-ticker, source-window and
receipt-eligible-at-export trade absent from the stored evidence. The 30-second
snapshots bind 57 distinct original trade IDs. Seven original five-second
snapshots contain trades, but two age out before their later decision; these
correctly yield only five identified inputs.

The final column is only an upper bound: a wire receipt does not prove when
processing/export/observer consumption made that print available. No new model
feature is reconstructed from it. Only three direct decisions even have a
same-ticker trade received in the preceding execution-time second. Their
tickers are CONTROLH-2026-D, KXHOUSERACE-SC01-26-R and SENATETX-26-R. Their frozen
snapshot ages are 757/971/797 ms respectively; all three trades arrived **after**
the corresponding original export. The report retains their original IDs and
clocks. Zero identified one-second windows is therefore correct. Even optimistic
instant delivery could supply at most three flow-complete decisions in this
capture, before price/label validity, nonoverlap clustering or training/OOS
requirements. That cannot meet the unchanged 100-OOS/100-selected thresholds.

## Timing and code path

The original exporter constructs `flowView` windows from actual execution time,
requiring actual receipt and processing availability no later than export. The
observer polls the existing cache once per second and freezes first observer
availability. `_flow_for_decision` then intersects the original evidence with
the later decision's true window; it never relabels old executions as recent.
Sources: [exporter](../../../moloch/external-data/kalshi-websocket.mjs),
[observer](../../../moloch/external-data/crossvenue_observer.py),
[frozen evaluator](../v7-flow/analyze.py).

Across direct decisions, source export→decision age has median917 ms,
p90 1,242 ms, range662–1,461 ms; 996/3,813 snapshots are at least one second old.
Export→observer availability has median625 ms and p90 726 ms; availability→
decision has median260 ms and p90 472 ms. Quantiles describe separate measured
intervals and must not be added as if the same rows defined each quantile.
The timestamps do not isolate serialization, tunnel/network transfer or local
processing inside export→availability. No specific CPU/network fix is proven.

Reducing delivery lag could recover a few missing observations, but it cannot
create the absent same-ticker executions. More productive prospective sampling
would separately predeclare decision timing around actual trade arrivals,
requiring a genuinely current SIG book and subsequent labels, or longer-window
missingness-aware features with explicit availability indicators. Neither is
the frozen v7 experiment, and neither is qualified without a new held-out study.
An empty window remains missing under v7; it is not converted to signed zero.
Party-reference mapping also remains conditional on unverified resolution
equivalence and supplies no execution authority.

## Reproduction and verification

`audit.py` requires `--observer`, `--observer-sha256`, `--collector`,
`--collector-sha256`, and `--output` paths. Use the original private gzips with
the exact hashes above; no network or credential input is supported. Output is
the committed aggregate [report](report.json). The source-checking diagnostic
requires the exact original observer and unchanged frozen evaluator.

Five local Python tests passed: after-export receipt isolation, strict execution
window/future/late-receipt exclusion, refusal to retime a longer window into one
second, original compressed hash tampering, and duplicate/raw-trade conflicts.
The full original-capture audit exited successfully. No SIG/Kalshi requests, SSH,
collector/observer changes, production restart, forecast review change or order
operation occurred.
