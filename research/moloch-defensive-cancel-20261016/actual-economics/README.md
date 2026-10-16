# Finite actual economics: defensive-cancel release

Source base `db87606d`; deployed source `b112c6c1`, PID525450, launched
2026-10-16T17:07:56Z. Three finite private captures started **17:31:12.275,
17:33:20.229 and17:35:09.320UTC**. No account API, new collector, service/control,
strategy or financial changes were made. SSH only read existing files at nice19.
Raw account/receipt/journal material remains private, outsideGit. The linked
[aggregate report](report.json) contains exact original file hashes, byte counts,
offsets and capture clocks. Capture indexes and earlier derived reports remain
preserved; nothing was overwritten to obtain a better result.

The auditor verified the entire frozen dispatch chain (13,140 rows) from its
zero-hash genesis, and11,418 complete native rows, sequences284332–295749, from
a32MiB suffix. It also verified29 complete original breadth rows from a4MiB
suffix. Incomplete first/last suffix fragments are excluded explicitly. Native
report295127 is sealed and its previous hash matches the captured journal row;
final sealed report296346 lies beyond that suffix. This is **not** a full-bank
replay or atomic cross-file account snapshot. Independent report/progress/
dispatch times stay separate. Receipt checks reproduce the SDK's typed struct
serialization, preserving original body numeric bytes and field order, then
check exact order/fill identity and economics.

## Actual realized money and its origin

Four pre-launch BUY fill rows on exchange894/market205 belong to two orders:
2479YES@0.335 and1291NO@0.660. They produced1291 complementary pairs, whose exact
net is1291×(1−0.335−0.660)=**6.455**. Residual inventory is1188YES, basis397.980.
This exchange is absent from inherited adoption and has no earlier fill in the
retained complete native fill report.

New version q8087 sold that residual at0.340: two actual fill IDs,364+824 units,
**one physical SELL order**, both executed17:25:48.287. Proceeds403.920 minus
basis397.980 and recorded fees0 equals **5.940**. Original full owned-order and
complete fill-tape receipts confirm1188 fully filled and closed. The entire
six-row/two-BUY-one-SELL lifecycle earned12.395; only the5.940 residual exit
belongs to the new version's execution window. It is not evidence that its new
BUY entries have a positive expectation.

As of final bank17:35:03.863:200 actual fill rows, ordinary realized
**−176.595**, inherited gross **+45.130**, combined **−131.465**. Eight new POST
ACKs comprise one carried-inventory SELL and seven BUYs. **Zero new BUY fill
rows** have been observed in this finite window. An ACK is not a fill, and
cumulative duplicate tape events are not counted again.

## Cancellation: mandatory observed, optional defense not observed

Three original DELETE attempts follow launch. Every durable attempt has
`defensive:null`: these are the existing mandatory lane. No optional defensive
attempt or `defensive-cancel.json` result was observed. That feature's first
live behavior remains unverified; enabling its control is not evidence it ran.

| Native qid | DELETE attempt | DELETE ACK | Independent complete terminal evidence |
|---|---|---|---|
|8093|17:31:26.864|17:31:28.169|Owned order closed0fills17:31:38.024; complete empty tape17:31:38.888; native terminal applied17:31:38.987|
|8171|17:34:24.658|17:34:26.538|Owned order closed0fills17:34:39.564; complete empty tape17:34:40.515; final native quote retired0fills, exact application clock outside frozen suffix|
|8079|17:35:08.214|Not yet in captured prefix|Unresolved at that dispatch boundary; no terminal release claimed|

Both confirmed cancellations are `UserRequested` and bind their original POST
IDs, exchange, side/action and complete empty tapes. Funding release is tied
to terminal reconciliation, approximately10.8s/14.0s after DELETE ACK, never to
the ACK itself. No avoided loss or cancel-generated profit is credited.

## Concrete sizing and latency constraints

q8086 is **YES BUY487@0.040**. Its fresh retained pre-POST book has YES
bid33171@0.040 and ask487@0.045. Integer YES microprice44927micros gives
4927micros above its limit, greater than original minimum1000. q8126 is **YES
BUY38@0.695**, with YES bid4451@0.695 and ask38@0.700; integer microprice699957
means4957micros edge. Both are passive at their own bid, with large displayed
same-side quantity; this does not prove price-time priority or fill probability.

The original policy has pairedSpread, quantity10000 and entryDepthBps10000.
[Native BUY sizing](https://github.com/GodlyDonuts/predictions-cup-2026/blob/db87606d/moloch/native/src/engine.rs#L1326-L1378)
limits a mate by the first batch leg, and the first leg by its still-required
opposite BUY target's own-side depth; fee-inclusive paired unit cost also enters
funding headroom. Thus487/38 fit the smaller opposite top depth in these
snapshots. This is **paired BUY symmetry/funding**, not a full-inventory
liquidation-depth guard. The later physical side-edge filter can submit one
qualifying side while native paired sizing remains coupled. These finite books
are consistent with that mechanism; the exact earlier sizing book is not
reconstructed as a new authoritative quote or a larger-fill counterfactual.

Seven BUY draft-to-ACK ages are51.914–163.197s, median84.151s. The latest q8178
split is **114.786s from original draft to first recorded preflight GET**, then
**7.662s from that GET to ACK**, plus0.245s to native ACK application. Its actual
diagnostic book age at ACK is **1.465s**. The preflight/ACK/apply interval is
7.907s, with3.803s recorded-span union and4.104s outside those spans. The114.786s
upstream interval includes source/planning/selection work not independently
timed in this trace; it cannot all be called queue wait. Frozen stale quote
and side-edge checks continue to bind original price/size/expiry. A fresh HTTP
book does not renew the original draft lifetime.

Discovery nominated35 scopes at17:24:41; progress reached32/35 at17:34:53.
This is a long serial cycle, not high-frequency physical execution. The worker
performs one preflight action per pass with mandatory cancellations/urgent exits
first, and optional defense only when no ready urgent exit exists
([dispatch](https://github.com/GodlyDonuts/predictions-cup-2026/blob/db87606d/moloch/live-runtime/src/worker.rs#L1329-L1467)).
No per-candidate defensive rejection log exists here, so zero optional activity
cannot be causally assigned to that priority guard or to missing valid signals.

Capital is **not established as the bottleneck**: final bank cash72716.935 and
reserved cash7423.175 coexist with these small orders. Per-market/pair/risk
limits can still bind. The seed's static3markets/40000 are not current effective
bounds: authorized breadth/capital events are retained separately. Raising cash
or quantity alone has no observed profitability justification.

The finite public native suffix contains no observed post-ACK prints for several
new quoted scopes. AL02/ex1007 has one print at17:30:58,50units@0.305YES;
old q8093NO0.690 requires YES-equivalent price at least0.310, so that print was
not price-compatible. Later q8171 had not yet been acknowledged then. Trades
absent after suffix end are **unavailable**, not zero. This is incomplete
observed coverage, not proof of no demand, lost queue priority or missed fills.

Next useful bounded work is to separate draft-to-first-GET dwell from active
preflight service across candidate classes, and replay the paired-depth sizing
constraint against actual own fill/nonfill denominators before proposing a
separate one-sided maker sizing policy. Neither deeper bids at a higher limit,
larger funding, nor microprice edge may be credited as executable profit. The
current report supports investigating allocation/scheduling and passive fill
conversion; it does not authorize price/risk/source changes.

Stale files are explicitly excluded as current blockers: deferred.json is17:00,
quote-priority-deferred06:26, execution-model03:22, exit-refresh15:18. Fresh
progress17:34:53 reports healthy trading/1220ACKs/200fill rows. Their old clocks
are not evidence of a current process or model failure.

## Reproduction

```
python audit.py --private-input PRIVATE_ORIGINAL_CAPTURE_DIRECTORY --output NEW_JSON
```

The checked run validated original byte pins, full dispatch chain, finite
source suffix chains, report membership, original typed receipt hashes and
fill/terminal scope. No generic mirror test suite was added. The public result
is aggregate diagnostic evidence, not a P&L forecast or financial authority.
