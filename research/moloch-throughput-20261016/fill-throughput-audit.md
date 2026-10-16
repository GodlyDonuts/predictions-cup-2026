# Completed-fill throughput audit — October16 2026

This bounded read-only audit examined the existing sole-worker415286 captures. No account requests, queue commands, service/configuration changes, orders or production writes were made. Analysis ran at nice10; private evidence output was written outside the runtime under `~/moloch-offline-audit-20261016/`.

## Finding

The first eight new passive BUY acknowledgments selected four exchanges with **zero observed public trade prints**, while the same retained stream contained415 unique prints across62 exchanges. The observed problem is selection into locally inactive flow during this window. Repeated books generally kept these quotes at the best same-side bid; they were not observed being displaced by a better bid. Same-price visible depth was sometimes large, but aggregate depth cannot identify our queue rank or establish that it was ahead of us.

The bot did execute two inherited inventory exits. Three actual fill rows are **two actual orders**, not three orders and not successful new-entry trades. No new passive BUY fill appears in the captured native evidence.

## Actual fills and attribution

| Logical qid | Market/exchange | Actual execution UTC | Side/action | Quantity | Price | Fees | Attribution |
|---|---|---|---|---|---|---|---|
|4403|261/950|06:51:49.857|NO SELL|38+3|.445|0|Inherited41NO at.440 FIFO: +.205 gross|
|4412|259/948|06:54:42.535|NO SELL|20|.905|0|Inherited remaining FIFO.895: +.200 gross|

q4403 has a retained closed-order receipt and complete owned tape with signed total−41 and two fill rows. q4412 has an actual source-bound native fill, not merely an ACK-derived fill. Its latest closure was not required for this execution-count result. Both were native `inventoryAON` exits; no maximumAge annotation appeared in their original quote records. ex948 had2377 prior NO sales (228+1765+262+81+41), exhausting the first466 lower-basis inherited shares and reaching the.895 lot. ex950 had no earlier Moloch fills and its adopted lot was41NO at.440.

The +.405 combined inherited gross matches44.725→45.130. New-trading realized remained−119.100. These figures do not turn historical holdings into new-strategy profit; all three observed fill fees were exactly0.000000.

## The first eight passive BUYs

All eight ACKs reported zero immediate quantityTraded. Six had independently retained UserRequested terminal receipts plus complete zero-fill tapes at the audit snapshot; the1003 pair remained open in that dispatch snapshot.

|qid|Exchange|Side|Quantity / limit|ACK UTC|Unique-frame post-ACK book observations|At best bid|Behind a better bid|Nearest pre-ACK same-price visible quantity|Closed UTC|
|---|---|---|---|---|---|---|---|---|---|
|4346|1017|NO|1868 / .455|06:43:37.879|72|72|0|15495|06:52:51.469|
|4347|1017|YES|1868 / .540|06:43:58.366|70|70|0|1868|06:53:12.142|
|4372|1000|NO|30 / .080|06:47:10.894|20|19|0|30|06:50:49.824|
|4373|1000|YES|30 / .915|06:47:44.787|17|17|0|3132|06:51:15.197|
|4377|1001|NO|302 / .730|06:48:11.919|28|28|0|2|06:53:50.835|
|4378|1001|YES|302 / .265|06:48:41.647|27|27|0|14217|06:54:09.657|
|4390|1003|NO|8941 / .235|06:49:33.125|36|36|0|8941|Not closed in snapshot|
|4391|1003|YES|1358 / .760|06:50:05.769|29|29|0|1358|Not closed in snapshot|

The remaining1000 observation had a lower visible best bid, not a better competing bid. Observations are distinct captured frame IDs; identical underlying book states can recur. Pre-ACK means before receipt acknowledgment, **not before venue acceptance**: our submitted order can already be included in that depth. In particular some visible quantities equal our submitted quantity. Do not interpret them as measured queue ahead. The nearest raw-stream snapshot also need not be the exact physical preflight REST receipt; its clock is retained privately.

## Public flow elsewhere in the same window

Executed time cutoff06:43:00Z; original delivery window06:42:39.891→06:55:25.009Z. Prints were deduplicated by exact exchange/id/sequence; conflicting repeated contents would reject analysis. Side was not invented: the public producer supplies canonical YES price and unsigned size, not aggressor or maker identity. A print is counted once, not once per YES and NO perspective.

|Exchange|Market|Unique prints|Total printed quantity|
|---|---|---|---|
|1033|344|59|41578|
|843|154|53|54398|
|873|184|23|9173|
|978|289|19|4861|
|967|278|17|3691|
|932|243|13|11771|
|979|290|13|1590|
|865|176|12|3689|
|894|205|12|3159|
|1043|354|12|2932|

Quoted1017/1000/1001/1003 each had zero observed prints throughout this execution-cutoff window, not just zero prices crossing our limits. The stream did carry the actual inherited-exit match IDs: ex950 public YES.555 corresponds to NO.445, and ex948 public YES.095 corresponds to NO.905; their sizes/IDs match the actual fill evidence. Those executions happened before the POST ACK receipt clocks, so their later public delivery must not be relabeled as new post-ACK demand.

## Evidence boundary and limitations

- Exact stream: `breadth-415286-1792132968131.jsonl`, frozen first94,983,942 bytes (~90.6MiB), original rows1..721. Initial terminal wrapping caused an erroneous850MB estimate; the verified95MB prefix supersedes it and is consistent with a128MiB capture bound.
- Full frozen-prefix SHA256: `f5329c5559c54f053abaf513ccc3797d08ad61445e5b2bebbf77b76ebf3336336`. Contains720 markets_batch records and one initial stream_reset; no other explicit event/gap was recorded in this prefix. This does not independently establish complete venue-wide tape coverage or absence of upstream losses.
- Initial two64MiB reads overlapped; their union covered the frozen prefix. Frame sequence deduplication corrected book counts, and a final single bounded prefix pass produced venue-wide counts. All originals remained unchanged.
- Frozen dispatch bytes SHA256: `ed6361d5d290e0bce3b61d28e07584df9da0b2927346562dc27d354f5bbd2af2`; head `7434b406d6cdc907a4b6b522d945e8937eaa052c30f69777de0bc305c35df6cc`. Private summary was generated06:55:42.763687Z. Dispatch snapshot includes additional later ACKs; the first-eight table deliberately avoids treating later short/unobserved windows as equivalent exposure.
- Private originals/provenance summaries remain under `~/moloch-offline-audit-20261016/`; local private copy `Temp/moloch-throughput-audit-summary-20261016.json` contains the original bounded per-order evidence. Public report omits actual account/order IDs and raw portfolio receipts.
- Printed volume is not necessarily accessible opposing flow, profitable spread capture or fill probability. Raw aggregate books do not reveal FIFO priority, hidden liquidity, cancellations ahead, aggressor side or private matching policy. No proxy fill is credited.

## Actionable implementation implication

A bounded causal recent-public-flow signal is a better read/nomination input than dirty-market lexical ordering alone. Preserve independent book freshness, account authority, exact passive/forecast policies, funding and expected-value checks; volume cannot authorize a losing trade. Use pre-decision retained flow only, and assess actual owned fill/markout outcomes after selection changes. Do not promote these top-ten historical observations as guaranteed current profitable targets. Increasing write usage or moving one tick alone has no evidence of fixing the zero-flow exchanges in this sample.
