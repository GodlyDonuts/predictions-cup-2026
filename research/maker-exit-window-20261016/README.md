# Actual maker exit-window audit — October16

[Task156](https://github.com/GodlyDonuts/predictions-cup-2026/issues/156). This is a frozen historical diagnosis, not current account status or a policy change. Its cutoff is **2026-10-16T11:26:17.690Z**, before the later owner-capital contribution. The retained bank still has its40,000 initial allocation.

The source audit verifies all237,382 original compact journal row seals, their chain, and the729,558,692-byte prefix hash. The existing exact FIFO analyzer independently matches all144 unique actual fills, ending positions/cost/cash/fees and realized attribution. No financial engine replay, account request, runtime change or execution occurred. Twelve synthetic tests pass locally and on ARM64; actual-source parity passes.

## Actual outcomes

All12,628 closed ordinary shares exited through native orders marked `maximumAge`. Forty ordinary BUY-fill cohorts are fully closed:3 wins,28 losses and9 flat, totaling **−119.535000** with zero fees. One1540-share NO843 cohort remains open at877.800000 acquisition cost; it receives no unrealized-profit mark. Inherited gross realization is separately **+45.130000**; combined attributed realized is−74.405000. These are not whole-account/dashboard returns.

There is **no evidence of a premature60-second forced exit**: the shortest ordinary execution-to-exit holding is78.061seconds. The observed policy is60000ms, but actual exits often occur much later. Historical ordinary holding is15.278minutes at the median FIFO-close fragment and26.598minutes at the quantity-weighted mean. This sample spans earlier service stops/restarts and policies; it is not a benchmark of the current worker.

Recent examples from the same frozen prefix:

| Exchange/side | Actual acquired/closed shares | Actual gross result | Holding |
|---|---:|---:|---:|
|843 YES|431/431|0.000000|143.093s|
|843 NO, closed cohort|490/490|0.000000|167.995s|
|934 NO|966/966|0.000000|111.062s|
|1033 NO|173/173|−0.100000|238.364–608.391s|

For1033,153 shares sold at the entry price and20 sold one tick lower. Requested order size is not actual traded size; the YES843 row counts431 actual shares.

## Where the time went

All41 ordinary BUY-fill cohorts have original POST ACK receipts; none use an invented/recovered ACK time. Every passive ACK preceded its later fill. The signed median ACK-receipt minus fill-execution time is−122.804seconds: this is resting time, not negative network latency.

| Recorded interval | Median | P90 | Observations |
|---|---:|---:|---:|
|BUY execution → original fill-tape receipt|23.004s|261.998s|41 BUY-fill cohorts|
|Fill receipt → native ingestion|0.071s|0.756s|41|
|Native fill ingestion → first native fresh book|28.626s|332.996s|41;0 missing|
|Native fill ingestion → ultimately filled exit quote created|891.881s|10535.454s|101 FIFO-close fragments|
|That exit quote created → actual exit execution|3.459s|8.736s|101|
|Exit execution → original fill-tape receipt|16.797s|30.516s|101|
|Exit receipt → native ingestion|0.004s|0.012s|101|

The greatest measured interval is before the final successfully filled SELL draft. That does **not** prove the first exit draft was delayed891seconds: earlier unfilled/expired/cancelled drafts may exist. Economic eligibility, stale books, retries through new proved-unsent drafts, read scheduling and historical downtime are not causally separated here. Parsing the received fill tape is much smaller than this interval. Improving scheduling can make intended exits more timely; it does not establish positive alpha.

## Later displayed exit prices

A horizon label is the first valid original book whose source time reaches execution+60/180/300/600seconds and whose observation is within30seconds after that horizon. Source, receipt and native observer clocks must be fresh; REST request age is also checked. A stream's long-lived subscription request is not treated as book age. NO bids use the complement of YES asks. Full acquisition quantity must fit actual displayed depth. An unknown/unobserved fill cannot be exited before the native fill is known.

| Horizon | Full-depth labels /41 | Missing labels | Positive fixed-horizon labels | Any observed +1tick full-depth opportunity by this horizon |
|---:|---:|---:|---:|---:|
|60s|17|24|0|0|
|180s|14|27|0|0|
|300s|13|28|0|1|
|600s|21|20|0|1|

Coverage differs by horizon, so aggregate hypothetical gains across these sets are not a controlled comparison. Missing labels are not zero. The300/600second opportunity counts refer to the same cohort, not two independent successes. Overlapping fill cohorts can share displayed liquidity and cannot be summed into feasible portfolio profit.

The sole one-tick opportunity is NO934: source11:24:24.0388483Z, WS receipt11:24:25.270Z,213.077seconds after acquisition and191.483seconds after native fill ingestion. The displayed full-size marginal NO bid was0.650 against0.645 entry, giving **hypothetical**4.830000 gross for966shares at zero exit fees. The actual lot had already exited flat at111.062seconds. The source row hash is in `oneTickOpportunityEvidence`.

That packet comes from the **public breadth lane**, not an admitted native decision book. It retains original source/WS receive clocks but no original native decision-processing or physical preflight time. Breadth labels assume an immediate observer at the recorded receive clock, an optimistic source-level diagnostic. It does not prove the worker could have submitted or filled at that price. No maker fill is inferred from a quote touch or public print, and public prints have no proved aggressor/own-queue identity.

## Bounded next experiment

A blanket60→300/600second extension is unsupported: every available fixed-horizon label was nonpositive, and the single positive intermediate snapshot cannot establish a strategy. A concrete **shadow-only**, prospectively declared comparator is an adaptive maximum300-second window from actual acquisition: keep the one-tick covered-profit target while current full-depth liquidation remains within one tick of basis, attempt the existing fully guarded exit when full-depth bid reaches basis+0.005, and release the extra patience on a current adverse book or at300seconds. This is a hypothesis with additional holding risk, not an instruction to change production or a guaranteed loss bound. It should record actual native processing, earlier exit drafts, physical eligibility and unknown outcomes; a displayed touch cannot credit a shadow fill. Use the existing money/account limits and explicit stale-source behavior.

The immediate engineering diagnostic is to measure first eligible exit draft → physical selection/preflight → final attempted draft separately. That can test the large measured pre-final-draft interval without claiming that speed itself makes an unprofitable entry profitable. The existing healthy worker continues unchanged.

## Sources and reproduction

The public `report.json` contains aggregate counts, exact source hashes, bank report seal, cutoff and packet evidence; no raw own order/fill IDs or profile data. Raw inputs remain private on the VPS. Selected native books are incomplete observations. Only the two explicitly hashed frozen breadth captures supplement them; the old capture ends around09:20 and the later restart capture begins around11:18, leaving a material gap. No observation in that gap is invented.

- Bank prefix SHA256:`4f21f50a329450aec5245acbbb4f35d3a010db797d9ef935c97489d6db2e44c3`.
- Bank report envelope seal:`cb60756e4ef69512cffe155eecf6b56b0445fed28922a0814500164da6c909a4`.
- Aggregate report SHA256:`efccb99647775e992b2c36e5dbbb04f2741ead3ec6718ea53d31b1da64fc5120`.
- Private evidence TAR SHA256:`48aac34f27ec4551f12e8a634eced6dabe2c3b3cb84aa4cfe1e915c0ddaf12b7`.

Run `python -m unittest discover -s research/maker-exit-window-20261016 -p test_analyze.py -v`. The CLI accepts `--boundary PRIVATE_BOUNDARY_JSON --dispatch ORIGINAL_DISPATCH --breadth FROZEN_CAPTURE --output AGGREGATE_JSON`; repeat `--breadth` for explicitly retained sources. It reuses the checked FIFO and exact raw-seal parsers already in the repository, whose raw source hashes are recorded. Run at reduced priority. The final clock decomposition was a necessary second audit pass after the initial result; both versioned reports/logs are preserved. Neither invocation performed native engine replay or account/network calls.
