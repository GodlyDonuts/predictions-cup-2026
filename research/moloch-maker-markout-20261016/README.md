# Actual ordinary resting-BUY markouts — October16

This offline analyzer joins genuine attributed fills to original SIG breadth books. It neither infers a fill from a public print/touch nor changes production. The finite input report ends **2026-10-16T13:35:02.604Z**, sequence260738, before the later runtime replacement. The breadth capture begins12:56:57.160UTC: most all-history fills therefore have no corresponding horizon observation.

## Actual result

The sealed report contains163 unique fills. Original dispatch evidence identifies50 ordinary BUY-fill observations,20,584 actual shares,24 intents;112 SELL observations and one reviewed BUY are excluded. Original POST ACK receipt precedes execution for all selected fills. This proves resting after acknowledgment, **not the venue's maker/taker classification**. No complete original liquidity-role flag is available.

| Horizon | Full-depth labels /50 | Shares | Covered intents /markets | Displayed-depth mark less entry/entry fees | Per share |
| --- | ---: | ---: | ---: | ---: | ---: |
|1s|9|6,416|5/5|−28.240000|−0.004401|
|5s|8|6,316|5/5|−28.190000|−0.004463|
|30s|8|6,260|4/4|−29.690000|−0.004743|
|120s|8|6,260|4/4|−31.050000|−0.004960|

No covered mark is positive. The1s marks are four negative/five flat;120s marks seven negative/one flat. One5s and one120s source snapshot arrived too late for the declared window. Other missing observations are41/42 per horizon, not zero returns. Covered cohorts differ by horizon, so changes between the sums are not a controlled holding-period comparison. Partial fills share intents/liquidity; there are only5 covered intents at1s and4 at120s. Fixed nonoverlapping market/time-window cluster counts are8/7/5/4, **not a claim of statistical independence**.

Every covered mark also has a causal pre-execution full-depth snapshot. Subsequent bid-value movement equals the displayed mark in this particular sample: the pre-execution bid valuation matched fill principal. That is useful for distinguishing a later bid move from simply subtracting an entry ask/bid spread, but **does not prove informed adverse selection**. The displayed pre-fill bid may include our own quote, which disappears or shrinks when filled. Own resting liquidity is not removed at either clock; self-trade prevention can make some displayed depth inaccessible. These are optimistic displayed-depth diagnostics, not guaranteed proceeds available to our account. The sample does not qualify a toxicity defense, forecast, sizing increase or parameter change.

All nine1s labels and seven5s labels arrived before native discovery of the fill. The hindsight join uses the actual execution clock and preserves that distinction. It does not pretend the worker knew those fills in time to act. Lots may already have netted or sold before a later horizon; no continued ownership or actual counterfactual exit is asserted.

## Fixed measurement and attribution

Horizon choices1/5/30/120s, source/receipt slack5s and maximum source age10s were declared before the actual run. For each fill, take the first original book with source time in `[execution+h, execution+h+5s]`. Source must not be future, and receipt must reach the horizon. A late, stale, malformed or depth-limited first candidate remains unavailable; no later favorable/deeper replacement is searched. A gap/reset clears continuity state. This does not prove a continuous trade tape: a later genuine full snapshot may resume point-in-time labeling.

YES liquidation walks original YES bids. NO liquidation walks `1−YES asks`, retaining each level's quantity. The entire actual fill quantity must fit before receiving a full-depth mark; partial displayed capacity is reported separately. Actual execution price/principal and actual entry fees are used. Future exit fees remain unknown, not fabricated as zero. Strict0.005 ticks accept only the existing≤1e−12 floating-representation tolerance in original book prices; no arbitrary economic price rounding is allowed.

The latest pre-execution snapshot must have both original source and receipt clocks no later than execution and be fresh at execution. A later-arriving old source never gets causal pre-fill credit. It is a source-level availability diagnostic, not a native-processing or physical-preflight clock.

Original native order scope, filled totals, actual IDs, receipt hashes, price limits and clocks must agree. Exact duplicate fill observations are counted once; conflicting duplicates reject. Package/reviewed native reservations and original dispatch `intent.fundamental` entries are excluded. Missing original intent or original POST ACK means attribution unavailable. Without the optional dispatch input no ordinary resting-fill sample is invented. Inherited adoption creates no actual BUY fills and receives no new-entry labels. Opposite BUY netting is not reconstructed as an additional fill or profit.

## Bounded reproduction and source checks

```text
python analyze.py --report ORIGINAL_REPORT --report-sha256 SHA \
  --dispatch ORIGINAL_DISPATCH SHA BYTES \
  --breadth CLOSED_BREADTH_OR_GZIP SHA BYTES [--breadth ...] \
  --output PRIVATE_RESULT
python -m unittest discover -s research/moloch-maker-markout-20261016 -p test_analyze.py -v
```

Supply breadth segments in original chronological order. Report is at most64MiB; each journal is streamed with2GiB compressed/file and decoded bounds,1million rows,16MiB per row. File hashes/byte counts are checked before and after reading. Original compact native report and every dispatch/breadth row seal/hash chain are checked with the existing raw-slice parser; no whole-bank replay, fresh account audit or independent receipt retrieval is claimed. Parser source SHA is included. Per-file decoded hashes are also compared with the execution owner's original capture manifest; gzip wrappers do not replace original byte provenance.

The actual local run verified2,112 breadth rows,294,638,727 original decoded bytes, and10,837 dispatch rows. The report's raw SHA is `93ec8735e455e26bc2d6144eae67cc805d84ac40ebd9820c70605f53adda0f5f`; dispatch SHA is `07052ce42f7ebfaf6b5b288614fc3fd65d5da72cd756519fc56991df11ba774d`. Exact wrapper/decoded pins appear in [report.json](report.json). Original inputs and individual label evidence remain private. The published aggregate omits own order/fill IDs, exact fill clocks and individual evidence. It retains report-local anonymous intent indices and public market/side aggregates. Preliminary reports remain separate and are not silently overwritten.

Fifteen focused synthetic tests cover YES/NO depth, shallow-first selection, partial-fill identity/dedup, conflicting duplicates, review/package exclusions, original ACK attribution, stale/late/missing/censored windows, actual fees, tick representation, original row-chain/gzip tampering and causal pre-fill comparisons. The actual finite-source run completed locally in roughly8seconds. No SSH, network/API, compiler, live bank, queue, service or control operation occurred.

Final analyzer source SHA256 is `c8d693b45c24543122500d0ca4cedf699d9ac3efaafec5b37d02b6dd9ec30cb2`; the published aggregate SHA256 is `ad9f6ef8a814e660e4cb7cafebfbd10274982fef179aa8c8893ec525415a8b9e`. Individual evidence and its private result hash are retained separately. The public report is a declared redaction of that final result, not a second selected dataset.
