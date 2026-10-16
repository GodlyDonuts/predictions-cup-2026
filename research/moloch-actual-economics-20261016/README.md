# Actual production economics — October 16

The original ledger reconciles exactly: ordinary new trading realized **−119.100000**, inherited-lot gross **+43.395000**, combined **−75.705000**, zero fees. The newly filled Delaware forecast is open at395.600000 cost with zero realized profit. These are attributed FIFO measures; they are not official daily dashboard P&L.

The sealed observation through bank150533 contains119 unique actual fills plus38 repeated original fill events. Root verified every original journal row hash/sequence through that prefix and matched its last hash to the sealed report. The private extraction SHA256 is2ee8c7f2e0a78b9ce7689e5bc7eda9ccd80c87127cea4bf17aece1247b49ef0e. The filtered analyzer correctly labels full-chain verification as an extractor attestation: it cannot reconstruct omitted rows. Raw events and individual opening/current lot rows are not published.

| Finding | Original actual result |
| --- | --- |
| Closed new ordinary BUY lots |30;3 net winners,21 losers,6 flat|
| Ordinary shares acquired and fully closed |10,501|
| Acquisition principal / sale proceeds |6,794.060000 /6,674.960000|
| New realized per closed share |−0.011341777 approximately|
| Exit path |All88 FIFO closure fragments used original orders labelled maximumAge|
| Actual configured ordinary age threshold |60,000ms (one minute), verified original manifest/current control|
| Actual holding time |Minimum78,061ms; share-weighted1,879,435.51ms; maximum11,591,930ms|
| Largest loss |953 YES−67.555000|
| Second largest loss |885 NO−41.805000|
| Combined concentration |−109.360000,91.82% of ordinary loss|
| Current reviewed forecast |One430YES1042 lot at.920; open395.600000; realized0|

The two largest losses include buying3,643YES953 at.970 and selling at.960/.955/.950, and buying3,522NO885 at.300 then selling at.290/.285. This identifies adverse price movement and timed liquidation; it does not establish a causal benefit from removing the timer. The extraction contains no counterfactual executable later exits. Hindsight market exclusion, longer holding and a larger number of trades cannot be credited as profit.

The next strategy comparison must evaluate entry selection together with holding/exit behavior and capital turnover. Existing reviewed-value holdings are already separated from ordinary age exits. Production remains running while this audit is performed; no account client, bank edit, activation change or restart was needed.

`analyze.py` uses exact integer-millionth FIFO accounting, original duplicate identities, inherited/new/forecast origins and actual execution clocks. It rejects conflicting duplicates, missing basis, oversells, negative matched ages and endpoint mismatches. It verifies the final native positions, execution-clock lot state, cash, owned cost, fees and all three realized totals. Seven focused synthetic tests and root reproduction of the actual extraction pass; final full private result SHA256f8dc4e0d6eb4a5f9e63a03be267c7c28166f5c08776607202236ec0e182ec43f. [Published aggregate](actual-attribution.json).

Run with Python3:

```powershell
python -m unittest discover -s research/moloch-actual-economics-20261016/tests
python research/moloch-actual-economics-20261016/analyze.py --input PRIVATE_EXTRACTION.json --output PRIVATE_RESULT.json --inventory-age-ms 60000
```

The input is an ordered extraction of original adoption/fill events, exact native deduplicated fills and filled orders, sealed-prefix source metadata, expected native endpoint state and explicit original forecast qids. Keep originals private; publish relevant aggregates only. [Task #146](https://github.com/GodlyDonuts/predictions-cup-2026/issues/146).
