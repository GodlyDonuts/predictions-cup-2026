# Why the recent posted quotes did not fill

Observed retained V27 files at **2026-10-15T22:38:35.735486Z**, with follow-up
terminal/policy reads through22:41:54Z. This was a bounded read-only SSH study:
no account HTTP request, SDK client, runtime/control edit or order operation.
The separate SSH session was closed after the reads.

The last20 normal physical intents in that observation all had genuine retained
HTTP200 POST acknowledgments and zero recorded actual fills.16 were covered
SELLs and4 were BUYs. **14 of the16 SELL limits were above even the contemporaneous
best ask.** The other2 joined the ask; all4 BUYs joined the bid in a one-tick
spread. None had executable opposite-side quantity at its frozen limit in the
observed POST preflight book. Joining a displayed best price does not establish
queue position, fill probability or a profitable future exit.

## Price evidence

These books were received1.101--1.349seconds before their corresponding ACKs.
They describe actual fresh preflight observations, not a stale mark substituted
for executable depth. Prices below use the quoted outcome; NO prices are exact
complements of the YES book.

| Exchange/outcome | Submitted SELL | Best bid / ask | Consequence at that observation |
| --- | ---: | ---: | --- |
|1048 NO|0.690|0.635 /0.640|10ticks above the ask on its later posting|
|1058 YES|0.300|0.250 /0.255|9ticks above the ask on its later posting|
|1066 NO|0.315|0.265 /0.270|9ticks above the ask|
|1076 YES|0.905|0.840 /0.845|12ticks above the ask on its earlier posting|
|1068 YES|0.975|0.945 /0.950|5ticks above the ask|
|1075 NO|0.900|0.895 /0.900|Joins the ask; zero immediately executable bids at0.900|

The immutable policy observation has `inventoryExitMode="fixedGain"`,
`inventoryMinimumGain="0.005000"`, `inventoryTakerAfterMs=0`,
`maximumInventoryAgeMs=60000`, `lifetimeMs=600000`, `insideTicks=0`, and
`requireCurrentFlow=false`. The native ordinary inventory target takes the worst
remaining lot basis and adds the fixed gain, rounded to the tick. These old
holdings are consequently often offered far above the current market. They do
not have a new-entry forced-loss timer merely because they were inherited.

This is not grounds for selling at a loss just to record transactions. In
particular, exchange1068 holds an older FIFO block at0.970 before a later block
at0.925. A weighted average below the current0.945bid does not prove the first
FIFO slice can be sold profitably. Nor does buying the complementary outcome
create independent alpha: on the canonical complementary book it has the same
net economics as the corresponding covered sale.

## Time and opportunity costs

Fresh posting works: intent-to-ACK took0.603--0.870seconds in these20 cases. The
large delay precedes physical submission. The four BUYs spent185.426,
252.501,257.840 and277.638seconds between native creation and actual ACK;
their mean was243.351seconds. Their remaining lifetimes after acknowledgment
were322.362--414.574seconds. Across all20 quotes the range was322.362--517.658
seconds. The10-minute lifetime starts at native creation, not at the later POST.

One BUY was created22:27:53.386Z and acknowledged22:32:11.226Z. Three distant
SELLs were physically posted at22:30:20.142Z,22:30:42.772Z and22:31:06.908Z
while that BUY already existed. This proves three competing submission
opportunities, not a counterfactual257-second speed improvement: mandatory
cancellations, actual cached-book freshness and intervening reads still matter.

There was cancellation/expiry churn without fills. One0.975SELL was acknowledged
22:24:06.816Z, canceled by official ACK22:28:49.402Z and observed terminal
`UserRequested`22:29:06.848Z; a later offer on the same exchange again used0.975.
One ask-joining0.900SELL was canceled22:29:41.490Z and later reoffered at0.900.
Other sampled zero-fill orders ended `Expired`. None of those sampled inherited
SELLs had `inventoryExitReason="maximumAge"`. The records do not identify a
single common trigger for every local cancellation, so this study does not
attribute every cancellation to price movement.

The observed normal POST cadence was far below the24writes/minute ceiling.
The issue demonstrated here is competition for serialized selection/preflight
opportunities and read packets, not proof that the physical write limit was
exhausted. An ordinary preflight contains seven actual reads. Crucially, the
book used in this study was its last read: **14avoided POSTs does not prove
98avoided reads**. A before-preflight gate only saves that packet if an already
admitted current native book is fresh at selection time.

## Narrow selection change

`quote_priority::from_native` reads only valid books already admitted by the
actual bank and reuses `package_admission::native_books` transport-clock rules.
WS freshness uses official source/receipt clocks; REST additionally retains
request age. It does not create another data source or change a quote.

The returned index order preserves owned maximum-age/AON risk exits first, then
covered sales with visible current bid depth, then competitive passive quotes,
then candidates needing a source refresh. Fair rotation uses the existing
exchange/side/action cursor within each class. A fresh ordinary SELL above the
current same-side best ask is deferred until a later source makes it competitive.
Fresh BUYs behind the best bid or crossing the ask are also deferred. Missing,
stale or gap-invalidated books cannot prove a distant price and remain refresh
candidates. Pending cancels/retired/expired quotes are not new submissions.

Root integration belongs before the ordinary preflight budget is consumed,
with another check against the actual newly admitted book after preflight.
Cancellations and actual owned reconciliation keep their existing branches.
Original basis, frozen limit/quantity, funding, activation, caps, unknown guards
and final physical authority remain mandatory. Existing inventory is still
watched; this policy does not release a reservation, cancel a physical order,
cross a spread or force a historical loss.

Replaying the20observed POST/preflight observations gives14distant floors
deferred and6competitive candidates retained, including all4 BUYs. This is a
selection-count result. It is not a measured scheduler latency improvement,
profitable fill, forecast of profit or proof that the remaining quotes have
positive executable EV. The four BUYs were still unfilled; lack of aggressive
flow, unknown queue priority and adverse-selection risk remain. One-tick-wide
books also leave no inside tick without crossing the ask.

## Source and validation limits

Private retained source paths under
`/home/archon/apps/moloch/shared/live-production-v1`:

- `dispatch.ndjson`: original normal intents, POST acknowledgments, cancellation
  acknowledgments and terminal proof references.
- `bank-all-markets-v1/events.jsonl`: a bounded32MiB tail of the captured
  298,867,486-byte prefix;15,466complete parsed events,20:50:05.410--22:38:35.356Z.
  This study did not independently verify the entire cryptographic chain.
- `bank-all-markets-v1/report.json`: coherent derived bank observation at
  22:38:35.356Z,90 actual fills; unchanged recorded cash/PnL across this sample.
- `bank-all-markets-v1/manifest.json`: original immutable policy. The current
  replayed managed-market allowance and inherited fresh-market allowance are
  separate from that original manifest observation.
- `receipts/<hash>.json`: exact original terminal receipts selected by existing
  dispatch proof references, without publishing actual order/fill IDs or profile.
- `progress.json`: fresh trading-phase observation22:36:51.868Z. A latest full
  status file was older, so it was not presented as the current financial view.

The latest target books at the main observation were22:38:10--22:38:20Z,
15--25seconds old and therefore stale under the10-second rule. They were not
treated as a fresh current execution offer. Public print observations do not
establish actual owned fills or an aggressor. All observations are retained-file
evidence, not a new atomic account snapshot.

Tests include the20-observation count, exact NO complements, bounded partial
depth, urgent exits, fair rotation, stale/future/missing/incomplete books,
unchanged quote values, actual native Session WS/REST freshness and committed
source-gap invalidation. Local diff checks pass. Linux checks and any production
integration are owned by the parent; no unrun test or live strategy result is
claimed here.
