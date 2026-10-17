# Frozen five-minute original-book clock comparison

Read-only task167 measurement. Only compare_book_clocks.py and this document are owned by the child; the existing [clock-comparison-protocol.json](clock-comparison-protocol.json) is unchanged and authoritative. It was frozen at12:10:16UTC, uses300000ms and10,000ms book age, and compares original R25/PID649399 with R26/PID651324. No alpha, order/execution eligibility, actual network-causation, fill/profit or causal improvement claim follows from any result.

## Admission and fixed window

The CLI takes original ZIP paths plus expected raw SHA256 for each. It verifies every original file against its manifest pin, not only selected books. A packet manifest named packet-manifest.json or manifest.json may be a direct dictionary or contain object.files. Duplicate/ambiguous member names, unpinned files, malformed pins, unsafe paths, oversized files and changed ZIPs reject. ZIP/member hashes are checked before computation and the full ZIP is checked again afterward. No extraction, networking or financial Session is used.

Every original native row is raw-seal verified using the checked existing verify-original-evidence.py reader. Sequence/previous links begin at the original anchor head, end at the exact report/head and bank manifest identity, and agree with the last original processed clock/source cursor. A complete native suffix must start before the selected window and extend at least through its end. If either side lacks a full five minutes, no comparison result is produced. Original stopped-anchor authenticity is supplied by root's independent packet provenance; self-hashing a file alone is not account authority or underlying receipt-body authentication.

Old START is ONLY progress.at from the separately pinned original first-fresh proof ZIP, member progress.original.json, with phase=trading/realtimeHealthy=true/PID649399. The later current-capture progress is not substituted. New START is first-trading-progress.original.json in the new packet, with the same checks and PID651324. Include ALL original native book events whose processedAt is in [START,START+300000ms), including explicit invalid books; do not infer unchanged prices, complete tape or market silence between observations. Gap event count is retained; a gap does not fabricate book observations.

## Metrics

All original clocks use the existing n.instant nanosecond UTC parser; no millisecond datetime conversion or timestamp renewal. Output retains original progress.at plus exact window-start/end nanoseconds. Separate distributions have their own denominators: receipt-to-processing if receipt<=processing, source-to-receipt if source<=receipt, and source-to-processing if source<=processing. Median is the middle nanosecond difference or exact mean of the two middle differences; p90 is deterministic nearest-rank ceil(.90*n); max is the largest. Decimal millisecond strings preserve submillisecond values. Medians of different distributions must not be added to invent a paired latency decomposition.

For every parseable book, report both: (1) any-original-clock stale, max(processed-source,processed-request,processed-receipt)>10000ms, and (2) native-clock stale, source for official_sig_realtime and request for normalized REST. Count stale books whose source was0..10000ms old at receipt and stale books whose receipt-to-processing exceeds10000ms. Invalid flags and clocks are counted separately; stale timestamps do not become valid because a schema parser succeeds. read_book runs with an effectively unlimited age only to classify structural/clock validity, while the explicit fixed age comparisons supply staleness metrics. Missing/unparseable fields, source-ahead, receipt-ahead and mismatches are explicit; missing data never become zeros. Distribution differences are observational, not execution authority.

Output separates all/realtime/normalized REST and unknown transport; omitted native transport means normalized official REST, per the checked source schema. No parameter overrides/retuning, eligibility filtering or profitable-row selection exists. Caller-provided protocol raw SHA must match the actual file and its normalized-LF SHA must equal the frozen b6a8db505e27ae0bbbff2e18d281d8f8ff8d06877cb497d8f5bdddd76c5d3bfd. Root must separately establish that no model generations were changed during the new window; this clock reader does not claim to verify the complete fundamental sidecar history.

## Commands

```text
python research/moloch-source-latency-20261017/compare_book_clocks.py --old-packet OLD.zip --old-sha256 OLD_RAW_SHA --old-first-progress-packet FIRST_PROOF.zip --old-first-progress-sha256 FIRST_RAW_SHA --new-packet NEW.zip --new-sha256 NEW_RAW_SHA --protocol research/moloch-source-latency-20261017/clock-comparison-protocol.json --protocol-sha256 ACTUAL_PROTOCOL_RAW_SHA --output NEW_PRIVATE_RESULT.json
```

For explicit old-side validation before the new packet exists, replace both new-packet arguments with --old-only. Output then has new:null/comparisonPerformed:false, never a fictional new-side result. All outputs are create-new; old artifacts remain unchanged. Private raw books/account data are not exported by the aggregate clock report. Output includes packet member names/hashes, never their account contents.

## Actual old-only validation

Original old packet SHA9c530759b0f07429b5053e00acf199200553648eaafca7670e66ebde822d75be and separate first-progress proof ZIP SHA3ccb2b313594e0ec7a8af8a9346a0d9ba9dc8bd1f7b51b2aaba2010d2aa9dd71 both verify all members. First progress is10:56:24.123UTC; full window ends11:01:24.123 exclusive. Original anchored suffix478358..479094 contains737 rows and extends through11:02:31.257, so the entire fixed window is covered. There are292 native events/245 books/zero gap events in the window:218 realtime and27 REST;11 REST books explicitly invalid.

Old aggregate:124 timing-stale books,123 stale despite source-fresh receipt,122 stale with receipt-to-processing over10s. All-book receipt-to-processing median9311ms/p9026198/max29503; source-to-receipt median546.0275ms/p901143.4356/max27944.3708. Realtime receipt-to-processing median12863.5ms/p9026569/max29503; REST median77ms/p90194/max197. These are distinct cohorts and invalid flags are not suppressed. Syntax compilation and genuine old-only anchored computation passed. No new packet existed or was measured at this handoff. Root owns both capture and later comparison/result publication. No API/SSH/build/runtime/model change occurred.
