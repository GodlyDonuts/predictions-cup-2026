# Original source-age probe: October16,15:15 UTC

The execution owner ran the pinned metadata probe for30.7366 seconds against
the existing SIG breadth suffix and existing loopback public cache. This task
verified the downloaded original and summarized it offline. No observer policy,
clock, service or trading source changed.

Original gzip SHA256
`2d9254749b565ed8c4c2d4458f2e7ef5a75717f1063ffde99d7abd3a14c91b75`;
decoded SHA256
`aeac77cc0c00c49f39b846dcdd3d6d104159bce8787725d9182f140ff5019887`.
The retained private original contains788 rows/483,629 decoded bytes. Its
installed observer source was pinned to
`4e4649c054a469baed4ba57651fae6c201aed8a479ba6f8551e5d28e0e0d891e`.
Raw metadata stays private; the [aggregate report](report.json) contains no
account holdings, cash, orders or credentials.

## Observed clocks and acceptance

There were777 book observations from15 original source frames across226
exchanges. **All777 were accepted by the original observer.** There were zero
observations of the same sequence with a newer source timestamp. This window
therefore supplies no evidence for the proposed version-dedup defect; it cannot
rule out a defect in an unobserved case.

| Original interval at probe observation | Minimum | Median | Maximum |
| --- | ---: | ---: | ---: |
| Source→probe observation | 866 ms | 8,824 ms | 13,496 ms |
| Original socket receipt→probe observation | 242 ms | 7,924 ms | 11,291 ms |
| Source→original socket receipt | 397 ms | 1,452 ms | 2,205 ms |

610/777 observations exceeded the frozen3-second source-age bound;580 exceeded
the receipt-age bound. Another30 had old source but receipt age within3 seconds.
These are book-weighted counts sharing15 frames, not independent samples or a
venue-wide latency estimate. The three medians refer to separate distributions
and must not be added.

Frames165–176 show a catch-up interval: original socket receipt times run from
15:15:15.850 through15:15:28.992, while first probe observations run from
15:15:27.141 through15:15:29.658. Their receipt→probe lag falls from11.291 seconds
to666 ms. Much of that observed delay occurs **after** original socket receipt.
The probe has no separate producer append clock, so it cannot divide this delay
between the worker's queue/recording, filesystem/tail scheduling, loopback-cache
fetching and probe processing. It is not proof of11 seconds of observer CPU or
network delay. The root's separately delegated stream-service work is not
validated by this pre-change capture.

## Public-trade matches

Nine first-seen public-trade probes were retained: seven have no target book in
this **fresh probe suffix**, and two match an old-source book. This is not
account-wide absence, a complete trade tape or proof of no market demand. Some
retained public prints were already old; the probe diagnoses evidence rather
than retroactively admitting them to the frozen prospective sampler.

The two matched cases have original book source/receipt ages7,739/6,287 ms and
9,245/8,563 ms, while their public-trade source ages are849 and1,034 ms. Their
fresh public arrivals do not make the older SIG observations fresh. No clock
renewal or relaxed bound is justified. Exact raw version/receipt/observation
metadata remains in the pinned private original for further source correlation.

## Reproduction and limits

Run `python summarize.py --input ORIGINAL_PRIVATE_GZIP --output OUTPUT_JSON`.
It requires both exact original hashes, validates clock equations, checks input
identity/hash again after reading and records its LF-normalized source hash.
The full original-file summary exited successfully and matches the root's
independently supplied counts/medians. This report adds no statistical fitting
or synthetic test claims.

The probe used the original `Tail` hash/chain validator on a fresh suffix and
followed rotations; it did **not** verify the full historical prefix or prove
continuous delivery for every exchange. Its initial suffix lacked older held
books. Aggregate silence or a heartbeat cannot fill that evidence gap; see the
[official-schema audit](../unchanged-book-coverage.md). Account requests remain0.
There is no repaired data, forecast approval, measured prospective sample gain,
fill attribution or trading-profit claim.
