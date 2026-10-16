# Actual fresh production and bounded latency comparison

Sole518852/source2225b1ef/binaryfb55f739 was verified fresh trading16:03:23UTC.
At [16:16:16](fresh-live-verification.json), it has14 new official ACKs, healthy
realtime/no STOP, a matching executable and successor full status. Cumulative194
fills and attributed realized−143.860000 remain. Orders are not fill/profit claims.

The same probe/observer4e source and30s protocol now observes779 books/17 frames/
214 exchanges, versus777/15/226 before. Receipt-to-probe median is7.924s→2.069s;
p90 is11.291s→7.923s. Source-age median is8.824s→4.157s. Separate quantiles are
not added together. Source-to-receipt median rose1.452s→1.715s.

[Original sealed probe](actual-age-probe.jsonl.gz), [report](age-report.json),
[comparison](age-comparison.json), [fixed reproducer](summarize-age.py).
Root independently reproduced every statistic from the original gzip. The
reproducer changes only capture-hash constants and attribution metadata from
the original summary algorithm. It explicitly distinguishes unchanged observer
normalizer from changed SIG worker; original private intermediate reports remain.

Different natural time/workload windows, shared frames and no append clock prevent
a causal whole-loop/CPU attribution or independent-sample claim. Receipt-to-probe
combines producer queue/append, filesystem/tail scheduling, cache HTTP and probe
work. The slower tail and remaining >3s source ages still restrict valid features.
No future model improvement, continuous feed coverage or profitability is proved.
